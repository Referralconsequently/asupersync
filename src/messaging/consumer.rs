//! Consumer cursor leases over recoverable FABRIC capsules.
//!
//! The current fabric lane still lacks the full delegated cursor-partition and
//! read-ticket control plane, but this module establishes the deterministic
//! state machine that later beads can refine:
//!
//! - cursor authority is fenced by cell epoch plus lease generation,
//! - delivery attempts are certified with obligation-backed metadata,
//! - failover and contested transfer are deterministic lease transitions, and
//! - stale acknowledgements collapse to an explicit no-op instead of
//!   reanimating stale authority.

use super::fabric::{CellEpoch, CellId, SubjectCell};
use crate::remote::NodeId;
use crate::types::ObligationId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use thiserror::Error;

/// Inclusive sequence window requested or served by a consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SequenceWindow {
    start: u64,
    end: u64,
}

impl SequenceWindow {
    /// Create a new inclusive window.
    pub fn new(start: u64, end: u64) -> Result<Self, ConsumerCursorError> {
        if start > end {
            return Err(ConsumerCursorError::InvalidSequenceWindow { start, end });
        }
        Ok(Self { start, end })
    }

    /// Return the first covered sequence number.
    #[must_use]
    pub const fn start(self) -> u64 {
        self.start
    }

    /// Return the last covered sequence number.
    #[must_use]
    pub const fn end(self) -> u64 {
        self.end
    }

    /// Return true when the window fully contains `other`.
    #[must_use]
    pub const fn contains_window(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    /// Return true when the window contains `sequence`.
    #[must_use]
    pub const fn contains_sequence(self, sequence: u64) -> bool {
        self.start <= sequence && sequence <= self.end
    }
}

impl fmt::Display for SequenceWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..={}", self.start, self.end)
    }
}

/// Pull consumers can request explicit windows or named demand classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConsumerDemandClass {
    /// Resume from the current tail.
    Tail,
    /// Catch up a lagging consumer from durable state.
    CatchUp,
    /// Replay a historical slice.
    Replay,
}

/// Request selector captured in an attempt certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CursorRequest {
    /// Single-sequence delivery.
    Sequence(u64),
    /// Explicit inclusive window.
    Window(SequenceWindow),
    /// Demand-class request with no concrete window yet attached.
    DemandClass(ConsumerDemandClass),
}

impl CursorRequest {
    #[must_use]
    fn requested_window(self) -> Option<SequenceWindow> {
        match self {
            Self::Sequence(sequence) => Some(SequenceWindow {
                start: sequence,
                end: sequence,
            }),
            Self::Window(window) => Some(window),
            Self::DemandClass(_) => None,
        }
    }
}

/// Push and pull flows bind cursor authority differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CursorDeliveryMode {
    /// Consumer explicitly requests a sequence/window/demand class.
    Pull(CursorRequest),
    /// Delivery stays pinned to the currently leased peer for the window.
    Push {
        /// Inclusive window pinned to the current lease holder.
        window: SequenceWindow,
    },
}

impl CursorDeliveryMode {
    #[must_use]
    fn requested_window(self) -> Option<SequenceWindow> {
        match self {
            Self::Pull(request) => request.requested_window(),
            Self::Push { window } => Some(window),
        }
    }
}

/// Authority location for the current cursor lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CursorLeaseScope {
    /// Cursor authority still lives in the cell control capsule.
    ControlCapsule,
    /// Authority has been delegated into a narrower cursor partition.
    DelegatedCursorPartition {
        /// Deterministic delegated partition identifier.
        partition: u16,
    },
}

/// Peer currently allowed to serve under the active cursor lease.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CursorLeaseHolder {
    /// One of the cell stewards.
    Steward(NodeId),
    /// A delegated relay peer serving through a read ticket.
    Relay(NodeId),
}

impl CursorLeaseHolder {
    /// Return the underlying peer regardless of holder type.
    #[must_use]
    pub fn node(&self) -> &NodeId {
        match self {
            Self::Steward(node) | Self::Relay(node) => node,
        }
    }
}

/// Active cursor authority lease derived from the control capsule or a
/// delegated partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorAuthorityLease {
    /// Cell whose authority this lease fences.
    pub cell_id: CellId,
    /// Epoch paired with the cell id.
    pub epoch: CellEpoch,
    /// Where the authoritative cursor state currently lives.
    pub scope: CursorLeaseScope,
    /// Peer currently serving under the lease.
    pub holder: CursorLeaseHolder,
    /// Monotonic generation fencing stale attempts and acks.
    pub lease_generation: u64,
    /// Control-capsule policy revision captured when the lease was minted.
    pub policy_revision: u64,
}

impl CursorAuthorityLease {
    /// Derive the initial cursor authority from a subject cell.
    pub fn from_subject_cell(cell: &SubjectCell) -> Result<Self, ConsumerCursorError> {
        let Some(active) = cell.control_capsule.active_sequencer.clone() else {
            return Err(ConsumerCursorError::NoActiveSequencer {
                cell_id: cell.cell_id,
            });
        };

        Ok(Self {
            cell_id: cell.cell_id,
            epoch: cell.epoch,
            scope: CursorLeaseScope::ControlCapsule,
            holder: CursorLeaseHolder::Steward(active),
            lease_generation: cell.control_capsule.sequencer_lease_generation,
            policy_revision: cell.control_capsule.policy_revision,
        })
    }
}

/// Stable reference to the lease a read-delegation ticket was minted from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorLeaseRef {
    /// Scope of the delegated cursor authority.
    pub scope: CursorLeaseScope,
    /// Holder that owned the lease when the ticket was issued.
    pub holder: CursorLeaseHolder,
    /// Generation fencing stale delegated reads.
    pub lease_generation: u64,
}

impl CursorLeaseRef {
    /// Capture the current lease as a ticket-stable reference.
    #[must_use]
    pub fn from_authority_lease(lease: &CursorAuthorityLease) -> Self {
        Self {
            scope: lease.scope,
            holder: lease.holder.clone(),
            lease_generation: lease.lease_generation,
        }
    }
}

/// Cacheability metadata carried by a delegated read ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CacheabilityRule {
    /// The delegated payload must not be cached.
    NoCache,
    /// The delegated payload may be cached privately for a bounded interval.
    Private {
        /// Maximum private-cache age in logical ticks.
        max_age_ticks: u64,
    },
    /// The delegated payload may be cached by shared intermediaries.
    Shared {
        /// Maximum shared-cache age in logical ticks.
        max_age_ticks: u64,
    },
}

/// Opaque handle used to revoke a ticket after issuance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadDelegationRevocationHandle(u64);

impl ReadDelegationRevocationHandle {
    /// Return the stable handle value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Logical expiry for a delegated read ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadDelegationExpiry {
    /// Logical tick when the ticket was issued.
    pub issued_at_tick: u64,
    /// Last logical tick where the ticket remains valid.
    pub not_after_tick: u64,
}

impl ReadDelegationExpiry {
    /// Create a bounded expiry window in logical cursor ticks.
    pub fn new(issued_at_tick: u64, ttl_ticks: u64) -> Result<Self, ConsumerCursorError> {
        if ttl_ticks == 0 {
            return Err(ConsumerCursorError::InvalidReadDelegationTtl { ttl_ticks });
        }
        Ok(Self {
            issued_at_tick,
            not_after_tick: issued_at_tick.saturating_add(ttl_ticks),
        })
    }

    #[must_use]
    fn is_expired(self, current_tick: u64) -> bool {
        current_tick > self.not_after_tick
    }
}

/// Obligation-backed proof that a relay may serve a specific window for the
/// current cursor lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDelegationTicket {
    /// Cell whose data may be served.
    pub cell_id: CellId,
    /// Epoch bound into the delegation.
    pub epoch: CellEpoch,
    /// Lease reference that this ticket delegates from.
    pub cursor_lease_ref: CursorLeaseRef,
    /// Relay peer allowed to serve.
    pub relay: NodeId,
    /// Inclusive segment window the relay may serve.
    pub segment_window: SequenceWindow,
    /// Logical expiry bound for the ticket.
    pub expiry: ReadDelegationExpiry,
    /// Cacheability metadata the relay must preserve.
    pub cacheability_rules: CacheabilityRule,
    /// Revocation handle recorded by the issuing cursor authority.
    pub revocation_handle: ReadDelegationRevocationHandle,
}

impl ReadDelegationTicket {
    /// Bind a relay to the current lease for one concrete window.
    pub fn new(
        lease: &CursorAuthorityLease,
        relay: NodeId,
        segment_window: SequenceWindow,
        issued_at_tick: u64,
        ttl_ticks: u64,
        cacheability_rules: CacheabilityRule,
        revocation_handle: ReadDelegationRevocationHandle,
    ) -> Result<Self, ConsumerCursorError> {
        Ok(Self {
            cell_id: lease.cell_id,
            epoch: lease.epoch,
            cursor_lease_ref: CursorLeaseRef::from_authority_lease(lease),
            relay,
            segment_window,
            expiry: ReadDelegationExpiry::new(issued_at_tick, ttl_ticks)?,
            cacheability_rules,
            revocation_handle,
        })
    }

    fn validate(
        &self,
        lease: &CursorAuthorityLease,
        relay: &NodeId,
        window: SequenceWindow,
        current_tick: u64,
        revoked_tickets: &BTreeSet<ReadDelegationRevocationHandle>,
    ) -> Result<(), ConsumerCursorError> {
        if self.cell_id != lease.cell_id || self.epoch != lease.epoch {
            return Err(ConsumerCursorError::StaleReadDelegationEpoch {
                relay: relay.clone(),
                ticket_cell: self.cell_id,
                ticket_epoch: self.epoch,
                current_cell: lease.cell_id,
                current_epoch: lease.epoch,
            });
        }
        if revoked_tickets.contains(&self.revocation_handle) {
            return Err(ConsumerCursorError::RevokedReadDelegationTicket {
                relay: relay.clone(),
                revocation_handle: self.revocation_handle,
            });
        }
        if self.expiry.is_expired(current_tick) {
            return Err(ConsumerCursorError::ExpiredReadDelegationTicket {
                relay: relay.clone(),
                expired_at_tick: self.expiry.not_after_tick,
                current_tick,
            });
        }
        if self.cursor_lease_ref != CursorLeaseRef::from_authority_lease(lease)
            || &self.relay != relay
            || !self.segment_window.contains_window(window)
        {
            return Err(ConsumerCursorError::InvalidReadDelegationTicket {
                relay: relay.clone(),
                requested_window: window,
            });
        }
        Ok(())
    }
}

/// Attempt certificate emitted for each delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptCertificate {
    /// Cell being served.
    pub cell_id: CellId,
    /// Epoch used when the attempt was minted.
    pub epoch: CellEpoch,
    /// Captured cursor authority lease.
    pub cursor_authority_lease: CursorAuthorityLease,
    /// Requested sequence/window or push pin.
    pub delivery_mode: CursorDeliveryMode,
    /// Monotonic retry counter for this logical delivery.
    pub delivery_attempt: u32,
    /// Obligation backing the attempt.
    pub obligation_id: ObligationId,
}

/// Coverage map for symbols retained in recoverable capsules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoverableCapsule {
    coverage: BTreeMap<NodeId, Vec<SequenceWindow>>,
}

impl RecoverableCapsule {
    /// Record that `node` retains symbols for `window`.
    #[must_use]
    pub fn with_window(mut self, node: NodeId, window: SequenceWindow) -> Self {
        self.insert_window(node, window);
        self
    }

    /// Record another retained window for `node`.
    pub fn insert_window(&mut self, node: NodeId, window: SequenceWindow) {
        self.coverage.entry(node).or_default().push(window);
    }

    #[must_use]
    fn node_covers(&self, node: &NodeId, window: SequenceWindow) -> bool {
        self.coverage.get(node).is_some_and(|ranges| {
            ranges
                .iter()
                .any(|candidate| candidate.contains_window(window))
        })
    }

    #[must_use]
    fn reconstruction_contributors(&self, window: SequenceWindow) -> Option<Vec<NodeId>> {
        let mut current = window.start();
        let mut contributors = Vec::new();

        while current <= window.end() {
            let mut best: Option<(u64, NodeId)> = None;

            for (node, ranges) in &self.coverage {
                for range in ranges {
                    if !range.contains_sequence(current) {
                        continue;
                    }

                    let candidate = (range.end(), node.clone());
                    if best.as_ref().is_none_or(|(best_end, best_node)| {
                        candidate.0 > *best_end
                            || (candidate.0 == *best_end
                                && candidate.1.as_str() < best_node.as_str())
                    }) {
                        best = Some(candidate);
                    }
                }
            }

            let (best_end, best_node) = best?;
            if contributors.last() != Some(&best_node) {
                contributors.push(best_node);
            }
            if best_end >= window.end() {
                break;
            }
            current = best_end.saturating_add(1);
        }

        Some(contributors)
    }
}

/// Concrete delivery path chosen for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryPlan {
    /// Current steward serves directly under the active lease.
    CurrentSteward(NodeId),
    /// Delegated relay serves under a read ticket bound to the lease.
    LeasedRelay {
        /// Relay peer serving the request.
        relay: NodeId,
        /// Ticket proving the relay is bound to the current lease/window.
        ticket: ReadDelegationTicket,
    },
    /// No single peer has the whole window; reconstruct from distributed
    /// symbols deterministically.
    Reconstructed {
        /// Deterministic contributor order used for reconstruction.
        contributors: Vec<NodeId>,
    },
}

/// Result of applying an acknowledgement attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AckResolution {
    /// The acknowledgement commits against the current lease holder.
    Committed {
        /// Obligation closed by the acknowledgement.
        obligation_id: ObligationId,
        /// Holder the acknowledgement commits against.
        against: CursorLeaseHolder,
    },
    /// The attempt refers to a stale lease generation and collapses to a no-op.
    StaleNoOp {
        /// Obligation associated with the stale acknowledgement.
        obligation_id: ObligationId,
        /// Current generation that fenced out the stale attempt.
        current_generation: u64,
        /// Holder that currently owns the lease.
        current_holder: CursorLeaseHolder,
    },
}

/// Transfer claim presented during a contested cursor move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorTransferProposal {
    /// Peer that wants authority next.
    pub proposed_holder: CursorLeaseHolder,
    /// Generation the proposer believes is current.
    pub expected_generation: u64,
    /// Obligation backing the transfer attempt.
    pub transfer_obligation: ObligationId,
}

/// Deterministic outcome of contested transfer resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContestedTransferResolution {
    /// One proposal won and minted a new lease generation.
    Accepted {
        /// Lease minted for the winning proposal.
        new_lease: CursorAuthorityLease,
        /// Obligation that won the contested transfer.
        winning_obligation: ObligationId,
    },
    /// All proposals were stale relative to the current lease.
    StaleNoOp {
        /// Lease that remains authoritative after rejecting stale proposals.
        current_lease: CursorAuthorityLease,
    },
}

/// Consumer cursor authority state backed by the control capsule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricConsumerCursor {
    steward_pool: Vec<NodeId>,
    current_lease: CursorAuthorityLease,
    ticket_clock: u64,
    next_revocation_handle: u64,
    revoked_tickets: BTreeSet<ReadDelegationRevocationHandle>,
}

impl FabricConsumerCursor {
    /// Build cursor authority from the current subject cell.
    pub fn new(cell: &SubjectCell) -> Result<Self, ConsumerCursorError> {
        Ok(Self {
            steward_pool: cell.control_capsule.steward_pool.clone(),
            current_lease: CursorAuthorityLease::from_subject_cell(cell)?,
            ticket_clock: 0,
            next_revocation_handle: 1,
            revoked_tickets: BTreeSet::new(),
        })
    }

    /// Return the current lease.
    #[must_use]
    pub fn current_lease(&self) -> &CursorAuthorityLease {
        &self.current_lease
    }

    /// Return the current logical ticket clock.
    #[must_use]
    pub const fn ticket_clock(&self) -> u64 {
        self.ticket_clock
    }

    /// Advance logical ticket time for deterministic expiry tests.
    pub fn advance_ticket_clock(&mut self, ticks: u64) -> u64 {
        self.ticket_clock = self.ticket_clock.saturating_add(ticks);
        self.ticket_clock
    }

    /// Mint an obligation-backed attempt certificate.
    pub fn issue_attempt(
        &self,
        delivery_mode: CursorDeliveryMode,
        delivery_attempt: u32,
        obligation_id: ObligationId,
    ) -> Result<AttemptCertificate, ConsumerCursorError> {
        if delivery_attempt == 0 {
            return Err(ConsumerCursorError::InvalidDeliveryAttempt);
        }

        Ok(AttemptCertificate {
            cell_id: self.current_lease.cell_id,
            epoch: self.current_lease.epoch,
            cursor_authority_lease: self.current_lease.clone(),
            delivery_mode,
            delivery_attempt,
            obligation_id,
        })
    }

    /// Bind a relay ticket to the current lease for one window.
    pub fn grant_read_ticket(
        &mut self,
        relay: NodeId,
        segment_window: SequenceWindow,
        ttl_ticks: u64,
        cacheability_rules: CacheabilityRule,
    ) -> Result<ReadDelegationTicket, ConsumerCursorError> {
        if self.steward_pool.iter().any(|node| node == &relay) {
            return Err(ConsumerCursorError::RelayMustNotBeSteward { relay });
        }
        let revocation_handle = ReadDelegationRevocationHandle(self.next_revocation_handle);
        self.next_revocation_handle = self.next_revocation_handle.saturating_add(1);
        ReadDelegationTicket::new(
            &self.current_lease,
            relay,
            segment_window,
            self.ticket_clock,
            ttl_ticks,
            cacheability_rules,
            revocation_handle,
        )
    }

    /// Revoke a previously issued ticket by handle.
    pub fn revoke_read_ticket(&mut self, handle: ReadDelegationRevocationHandle) {
        self.revoked_tickets.insert(handle);
    }

    /// Choose the concrete serving path for the current lease.
    pub fn plan_delivery(
        &self,
        delivery_mode: CursorDeliveryMode,
        capsule: &RecoverableCapsule,
        ticket: Option<&ReadDelegationTicket>,
    ) -> Result<DeliveryPlan, ConsumerCursorError> {
        let Some(window) = delivery_mode.requested_window() else {
            return Ok(DeliveryPlan::CurrentSteward(
                self.current_lease.holder.node().clone(),
            ));
        };

        match &self.current_lease.holder {
            CursorLeaseHolder::Steward(node) if capsule.node_covers(node, window) => {
                Ok(DeliveryPlan::CurrentSteward(node.clone()))
            }
            CursorLeaseHolder::Relay(node) => {
                let Some(ticket) = ticket else {
                    return Err(ConsumerCursorError::MissingReadDelegationTicket {
                        relay: node.clone(),
                    });
                };
                ticket.validate(
                    &self.current_lease,
                    node,
                    window,
                    self.ticket_clock,
                    &self.revoked_tickets,
                )?;
                if capsule.node_covers(node, window) {
                    Ok(DeliveryPlan::LeasedRelay {
                        relay: node.clone(),
                        ticket: ticket.clone(),
                    })
                } else {
                    capsule
                        .reconstruction_contributors(window)
                        .map(|contributors| DeliveryPlan::Reconstructed { contributors })
                        .ok_or(ConsumerCursorError::UnrecoverableWindow { window })
                }
            }
            CursorLeaseHolder::Steward(_) => capsule
                .reconstruction_contributors(window)
                .map(|contributors| DeliveryPlan::Reconstructed { contributors })
                .ok_or(ConsumerCursorError::UnrecoverableWindow { window }),
        }
    }

    /// Fail over authority to another steward that already holds symbols and
    /// the control capsule state.
    pub fn failover(
        &mut self,
        next_steward: NodeId,
    ) -> Result<&CursorAuthorityLease, ConsumerCursorError> {
        if !self.steward_pool.iter().any(|node| node == &next_steward) {
            return Err(ConsumerCursorError::UnknownSteward {
                cell_id: self.current_lease.cell_id,
                steward: next_steward,
            });
        }

        self.current_lease.holder = CursorLeaseHolder::Steward(next_steward);
        self.current_lease.lease_generation = self.current_lease.lease_generation.saturating_add(1);
        self.current_lease.scope = CursorLeaseScope::ControlCapsule;
        Ok(&self.current_lease)
    }

    /// Resolve a contested cursor transfer deterministically using control
    /// capsule order first, then stable relay ordering, then obligation id.
    pub fn resolve_contested_transfer(
        &mut self,
        proposals: &[CursorTransferProposal],
    ) -> ContestedTransferResolution {
        let valid = proposals
            .iter()
            .filter(|proposal| proposal.expected_generation == self.current_lease.lease_generation)
            .filter_map(|proposal| {
                self.transfer_rank(&proposal.proposed_holder)
                    .map(|rank| (rank, proposal))
            })
            .min_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| left.1.transfer_obligation.cmp(&right.1.transfer_obligation))
            });

        let Some((_, winner)) = valid else {
            return ContestedTransferResolution::StaleNoOp {
                current_lease: self.current_lease.clone(),
            };
        };

        self.current_lease.holder = winner.proposed_holder.clone();
        self.current_lease.lease_generation = self.current_lease.lease_generation.saturating_add(1);
        self.current_lease.scope = match winner.proposed_holder {
            CursorLeaseHolder::Steward(_) => CursorLeaseScope::ControlCapsule,
            CursorLeaseHolder::Relay(_) => {
                CursorLeaseScope::DelegatedCursorPartition { partition: 0 }
            }
        };

        ContestedTransferResolution::Accepted {
            new_lease: self.current_lease.clone(),
            winning_obligation: winner.transfer_obligation,
        }
    }

    /// Apply an acknowledgement attempt against the current lease.
    pub fn acknowledge(
        &self,
        attempt: &AttemptCertificate,
    ) -> Result<AckResolution, ConsumerCursorError> {
        if attempt.cell_id != self.current_lease.cell_id
            || attempt.epoch != self.current_lease.epoch
        {
            return Err(ConsumerCursorError::AttemptScopeMismatch {
                certificate_cell: attempt.cell_id,
                certificate_epoch: attempt.epoch,
                current_cell: self.current_lease.cell_id,
                current_epoch: self.current_lease.epoch,
            });
        }

        if attempt.cursor_authority_lease.lease_generation == self.current_lease.lease_generation {
            Ok(AckResolution::Committed {
                obligation_id: attempt.obligation_id,
                against: self.current_lease.holder.clone(),
            })
        } else {
            Ok(AckResolution::StaleNoOp {
                obligation_id: attempt.obligation_id,
                current_generation: self.current_lease.lease_generation,
                current_holder: self.current_lease.holder.clone(),
            })
        }
    }

    fn transfer_rank(&self, holder: &CursorLeaseHolder) -> Option<(u8, usize, String)> {
        match holder {
            CursorLeaseHolder::Steward(node) => self
                .steward_pool
                .iter()
                .position(|candidate| candidate == node)
                .map(|index| (0, index, node.as_str().to_owned())),
            CursorLeaseHolder::Relay(node) => Some((1, usize::MAX, node.as_str().to_owned())),
        }
    }
}

/// Deterministic cursor-lease failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConsumerCursorError {
    /// Sequence windows must be ordered.
    #[error("invalid sequence window `{start}..={end}`")]
    InvalidSequenceWindow {
        /// Proposed start of the invalid window.
        start: u64,
        /// Proposed end of the invalid window.
        end: u64,
    },
    /// Subject cell must expose an active sequencer to seed cursor authority.
    #[error("subject cell `{cell_id}` has no active sequencer")]
    NoActiveSequencer {
        /// Cell whose control capsule lacked an active sequencer.
        cell_id: CellId,
    },
    /// Delivery attempts are 1-based.
    #[error("delivery attempt must be greater than zero")]
    InvalidDeliveryAttempt,
    /// Delegated read tickets must remain bounded in logical time.
    #[error("read-delegation ticket ttl must be greater than zero, got `{ttl_ticks}`")]
    InvalidReadDelegationTtl {
        /// Requested logical time-to-live for the ticket.
        ttl_ticks: u64,
    },
    /// Failover target is not in the steward pool.
    #[error("steward `{steward}` is not in the steward pool for cell `{cell_id}`")]
    UnknownSteward {
        /// Cell whose steward pool was consulted.
        cell_id: CellId,
        /// Proposed failover target not present in the steward pool.
        steward: NodeId,
    },
    /// Relay delegation is only meaningful for non-stewards.
    #[error("relay `{relay}` is already a steward and does not need a read ticket")]
    RelayMustNotBeSteward {
        /// Relay peer that was already part of the steward set.
        relay: NodeId,
    },
    /// Relay serving requires a lease-bound read ticket.
    #[error("relay `{relay}` is missing a read-delegation ticket")]
    MissingReadDelegationTicket {
        /// Relay peer that tried to serve without a bound ticket.
        relay: NodeId,
    },
    /// The provided ticket was minted for an earlier or different epoch.
    #[error(
        "read-delegation ticket for relay `{relay}` is stale for `{ticket_cell}`@{ticket_epoch:?}; current lease is `{current_cell}`@{current_epoch:?}"
    )]
    StaleReadDelegationEpoch {
        /// Relay peer carrying the stale ticket.
        relay: NodeId,
        /// Cell bound into the stale ticket.
        ticket_cell: CellId,
        /// Epoch bound into the stale ticket.
        ticket_epoch: CellEpoch,
        /// Cell currently owned by this cursor state machine.
        current_cell: CellId,
        /// Current epoch of the active lease.
        current_epoch: CellEpoch,
    },
    /// The provided ticket has expired in logical cursor time.
    #[error(
        "read-delegation ticket for relay `{relay}` expired at tick `{expired_at_tick}` (current `{current_tick}`)"
    )]
    ExpiredReadDelegationTicket {
        /// Relay peer carrying the expired ticket.
        relay: NodeId,
        /// Last valid logical tick for the ticket.
        expired_at_tick: u64,
        /// Current logical cursor tick.
        current_tick: u64,
    },
    /// The provided ticket was explicitly revoked after issuance.
    #[error(
        "read-delegation ticket for relay `{relay}` was revoked via handle `{revocation_handle:?}`"
    )]
    RevokedReadDelegationTicket {
        /// Relay peer carrying the revoked ticket.
        relay: NodeId,
        /// Revocation handle that fenced the ticket.
        revocation_handle: ReadDelegationRevocationHandle,
    },
    /// The provided ticket does not match the current lease or requested window.
    #[error(
        "read-delegation ticket for relay `{relay}` does not match the current lease/window `{requested_window}`"
    )]
    InvalidReadDelegationTicket {
        /// Relay peer named in the invalid ticket.
        relay: NodeId,
        /// Window the caller asked the relay to serve.
        requested_window: SequenceWindow,
    },
    /// No recoverable path exists for the requested window.
    #[error("requested delivery window `{window}` is not recoverable from the capsule")]
    UnrecoverableWindow {
        /// Window whose bytes were not reconstructable.
        window: SequenceWindow,
    },
    /// Attempt certificate must stay scoped to the current cell/epoch.
    #[error("attempt certificate scope does not match the current cursor lease")]
    AttemptScopeMismatch {
        /// Cell encoded in the stale or foreign attempt certificate.
        certificate_cell: CellId,
        /// Epoch encoded in the stale or foreign attempt certificate.
        certificate_epoch: CellEpoch,
        /// Cell currently owned by this cursor state machine.
        current_cell: CellId,
        /// Epoch currently owned by this cursor state machine.
        current_epoch: CellEpoch,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::fabric::{
        CellTemperature, DataCapsule, NodeRole, PlacementPolicy, RepairPolicy, StewardCandidate,
        StorageClass, SubjectPattern,
    };

    fn candidate(name: &str, domain: &str) -> StewardCandidate {
        StewardCandidate::new(NodeId::new(name), domain)
            .with_role(NodeRole::Steward)
            .with_role(NodeRole::RepairWitness)
            .with_storage_class(StorageClass::Durable)
    }

    fn test_cell() -> SubjectCell {
        SubjectCell::new(
            &SubjectPattern::parse("orders.created").expect("pattern"),
            CellEpoch::new(7, 11),
            &[
                candidate("node-a", "rack-a"),
                candidate("node-b", "rack-b"),
                candidate("node-c", "rack-c"),
            ],
            &PlacementPolicy {
                cold_stewards: 3,
                warm_stewards: 3,
                hot_stewards: 3,
                ..PlacementPolicy::default()
            },
            RepairPolicy::default(),
            DataCapsule {
                temperature: CellTemperature::Warm,
                retained_message_blocks: 4,
            },
        )
        .expect("cell")
    }

    fn obligation(index: u32) -> ObligationId {
        ObligationId::new_for_test(index, 0)
    }

    #[test]
    fn cursor_lease_starts_from_the_control_capsule() {
        let cell = test_cell();
        let cursor = FabricConsumerCursor::new(&cell).expect("cursor");

        assert_eq!(cursor.current_lease().cell_id, cell.cell_id);
        assert_eq!(cursor.current_lease().epoch, cell.epoch);
        assert_eq!(
            cursor.current_lease().holder,
            CursorLeaseHolder::Steward(NodeId::new("node-a"))
        );
        assert_eq!(
            cursor.current_lease().lease_generation,
            cell.control_capsule.sequencer_lease_generation
        );
    }

    #[test]
    fn delivery_attempts_commit_against_the_current_lease_holder() {
        let cell = test_cell();
        let cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(10, 12).expect("window");
        let attempt = cursor
            .issue_attempt(CursorDeliveryMode::Push { window }, 1, obligation(10))
            .expect("attempt");
        let capsule = RecoverableCapsule::default().with_window(NodeId::new("node-a"), window);

        assert_eq!(
            cursor.plan_delivery(attempt.delivery_mode, &capsule, None),
            Ok(DeliveryPlan::CurrentSteward(NodeId::new("node-a")))
        );
        assert_eq!(
            cursor.acknowledge(&attempt),
            Ok(AckResolution::Committed {
                obligation_id: obligation(10),
                against: CursorLeaseHolder::Steward(NodeId::new("node-a")),
            })
        );
    }

    #[test]
    fn pull_demand_class_attempts_preserve_the_named_request() {
        let cell = test_cell();
        let cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let attempt = cursor
            .issue_attempt(
                CursorDeliveryMode::Pull(CursorRequest::DemandClass(ConsumerDemandClass::Tail)),
                2,
                obligation(11),
            )
            .expect("attempt");

        assert_eq!(
            attempt.delivery_mode,
            CursorDeliveryMode::Pull(CursorRequest::DemandClass(ConsumerDemandClass::Tail))
        );
        assert_eq!(
            cursor.plan_delivery(attempt.delivery_mode, &RecoverableCapsule::default(), None),
            Ok(DeliveryPlan::CurrentSteward(NodeId::new("node-a")))
        );
    }

    #[test]
    fn failover_bumps_generation_and_turns_old_acks_into_stale_noops() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(20, 20).expect("window");
        let first_attempt = cursor
            .issue_attempt(
                CursorDeliveryMode::Pull(CursorRequest::Window(window)),
                1,
                obligation(12),
            )
            .expect("attempt");

        cursor.failover(NodeId::new("node-b")).expect("failover");
        assert_eq!(
            cursor.acknowledge(&first_attempt),
            Ok(AckResolution::StaleNoOp {
                obligation_id: obligation(12),
                current_generation: cell.control_capsule.sequencer_lease_generation + 1,
                current_holder: CursorLeaseHolder::Steward(NodeId::new("node-b")),
            })
        );

        let second_attempt = cursor
            .issue_attempt(
                CursorDeliveryMode::Pull(CursorRequest::Sequence(20)),
                2,
                obligation(13),
            )
            .expect("attempt");
        assert_eq!(
            cursor.acknowledge(&second_attempt),
            Ok(AckResolution::Committed {
                obligation_id: obligation(13),
                against: CursorLeaseHolder::Steward(NodeId::new("node-b")),
            })
        );
    }

    #[test]
    fn relay_delivery_requires_a_matching_read_ticket() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(30, 35).expect("window");

        let resolution = cursor.resolve_contested_transfer(&[CursorTransferProposal {
            proposed_holder: CursorLeaseHolder::Relay(NodeId::new("relay-1")),
            expected_generation: cursor.current_lease().lease_generation,
            transfer_obligation: obligation(14),
        }]);
        assert!(matches!(
            resolution,
            ContestedTransferResolution::Accepted { .. }
        ));

        let ticket = cursor
            .grant_read_ticket(
                NodeId::new("relay-1"),
                window,
                4,
                CacheabilityRule::Private { max_age_ticks: 2 },
            )
            .expect("ticket");
        let capsule = RecoverableCapsule::default().with_window(NodeId::new("relay-1"), window);

        assert_eq!(
            ticket.cursor_lease_ref.lease_generation,
            cursor.current_lease().lease_generation
        );
        assert_eq!(ticket.segment_window, window);
        assert_eq!(
            ticket.cacheability_rules,
            CacheabilityRule::Private { max_age_ticks: 2 }
        );
        assert_eq!(ticket.expiry.issued_at_tick, 0);
        assert_eq!(ticket.expiry.not_after_tick, 4);

        assert_eq!(
            cursor.plan_delivery(CursorDeliveryMode::Push { window }, &capsule, Some(&ticket)),
            Ok(DeliveryPlan::LeasedRelay {
                relay: NodeId::new("relay-1"),
                ticket,
            })
        );
    }

    #[test]
    fn relay_delivery_rejects_missing_ticket_when_authority_is_delegated() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(36, 38).expect("window");
        let relay = NodeId::new("relay-2");

        cursor.resolve_contested_transfer(&[CursorTransferProposal {
            proposed_holder: CursorLeaseHolder::Relay(relay.clone()),
            expected_generation: cursor.current_lease().lease_generation,
            transfer_obligation: obligation(15),
        }]);

        let capsule = RecoverableCapsule::default().with_window(relay.clone(), window);

        assert_eq!(
            cursor.plan_delivery(CursorDeliveryMode::Push { window }, &capsule, None),
            Err(ConsumerCursorError::MissingReadDelegationTicket { relay })
        );
    }

    #[test]
    fn read_delegation_ticket_expiry_is_enforced() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(46, 49).expect("window");
        let relay = NodeId::new("relay-expiring");

        cursor.resolve_contested_transfer(&[CursorTransferProposal {
            proposed_holder: CursorLeaseHolder::Relay(relay.clone()),
            expected_generation: cursor.current_lease().lease_generation,
            transfer_obligation: obligation(16),
        }]);

        let ticket = cursor
            .grant_read_ticket(relay.clone(), window, 1, CacheabilityRule::NoCache)
            .expect("ticket");
        cursor.advance_ticket_clock(2);

        let capsule = RecoverableCapsule::default().with_window(relay.clone(), window);

        assert_eq!(
            cursor.plan_delivery(CursorDeliveryMode::Push { window }, &capsule, Some(&ticket)),
            Err(ConsumerCursorError::ExpiredReadDelegationTicket {
                relay,
                expired_at_tick: 1,
                current_tick: 2,
            })
        );
    }

    #[test]
    fn read_delegation_ticket_revocation_is_enforced() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(50, 52).expect("window");
        let relay = NodeId::new("relay-revoked");

        cursor.resolve_contested_transfer(&[CursorTransferProposal {
            proposed_holder: CursorLeaseHolder::Relay(relay.clone()),
            expected_generation: cursor.current_lease().lease_generation,
            transfer_obligation: obligation(17),
        }]);

        let ticket = cursor
            .grant_read_ticket(
                relay.clone(),
                window,
                5,
                CacheabilityRule::Shared { max_age_ticks: 1 },
            )
            .expect("ticket");
        cursor.revoke_read_ticket(ticket.revocation_handle);

        let capsule = RecoverableCapsule::default().with_window(relay.clone(), window);

        assert_eq!(
            cursor.plan_delivery(CursorDeliveryMode::Push { window }, &capsule, Some(&ticket)),
            Err(ConsumerCursorError::RevokedReadDelegationTicket {
                relay,
                revocation_handle: ticket.revocation_handle,
            })
        );
    }

    #[test]
    fn stale_epoch_read_delegation_ticket_is_rejected() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(60, 63).expect("window");
        let relay = NodeId::new("relay-stale");

        cursor.resolve_contested_transfer(&[CursorTransferProposal {
            proposed_holder: CursorLeaseHolder::Relay(relay.clone()),
            expected_generation: cursor.current_lease().lease_generation,
            transfer_obligation: obligation(18),
        }]);

        let mut ticket = cursor
            .grant_read_ticket(relay.clone(), window, 5, CacheabilityRule::NoCache)
            .expect("ticket");
        ticket.epoch = CellEpoch::new(6, 99);

        let capsule = RecoverableCapsule::default().with_window(relay.clone(), window);

        assert_eq!(
            cursor.plan_delivery(CursorDeliveryMode::Push { window }, &capsule, Some(&ticket)),
            Err(ConsumerCursorError::StaleReadDelegationEpoch {
                relay,
                ticket_cell: cell.cell_id,
                ticket_epoch: CellEpoch::new(6, 99),
                current_cell: cell.cell_id,
                current_epoch: cell.epoch,
            })
        );
    }

    #[test]
    fn reconstruction_is_used_when_no_single_peer_covers_the_full_window() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let window = SequenceWindow::new(40, 45).expect("window");

        cursor
            .failover(NodeId::new("node-b"))
            .expect("make node-b current");

        let capsule = RecoverableCapsule::default()
            .with_window(
                NodeId::new("node-a"),
                SequenceWindow::new(40, 42).expect("left window"),
            )
            .with_window(
                NodeId::new("node-c"),
                SequenceWindow::new(43, 45).expect("right window"),
            );

        assert_eq!(
            cursor.plan_delivery(
                CursorDeliveryMode::Pull(CursorRequest::Window(window)),
                &capsule,
                None
            ),
            Ok(DeliveryPlan::Reconstructed {
                contributors: vec![NodeId::new("node-a"), NodeId::new("node-c")],
            })
        );
    }

    #[test]
    fn contested_transfer_prefers_steward_order_and_filters_stale_proposals() {
        let cell = test_cell();
        let mut cursor = FabricConsumerCursor::new(&cell).expect("cursor");
        let current_generation = cursor.current_lease().lease_generation;

        let resolution = cursor.resolve_contested_transfer(&[
            CursorTransferProposal {
                proposed_holder: CursorLeaseHolder::Steward(NodeId::new("node-c")),
                expected_generation: current_generation,
                transfer_obligation: obligation(20),
            },
            CursorTransferProposal {
                proposed_holder: CursorLeaseHolder::Steward(NodeId::new("node-b")),
                expected_generation: current_generation,
                transfer_obligation: obligation(21),
            },
            CursorTransferProposal {
                proposed_holder: CursorLeaseHolder::Relay(NodeId::new("relay-2")),
                expected_generation: current_generation.saturating_sub(1),
                transfer_obligation: obligation(22),
            },
        ]);

        assert_eq!(
            resolution,
            ContestedTransferResolution::Accepted {
                new_lease: cursor.current_lease().clone(),
                winning_obligation: obligation(21),
            }
        );
        assert_eq!(
            cursor.current_lease().holder,
            CursorLeaseHolder::Steward(NodeId::new("node-b"))
        );
        assert_eq!(
            cursor.current_lease().lease_generation,
            current_generation + 1
        );
    }
}
