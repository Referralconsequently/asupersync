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

use super::fabric::{CellEpoch, CellId, SubjectCell, SubjectPattern};
use super::jetstream::{AckPolicy, DeliverPolicy};
use crate::remote::NodeId;
use crate::types::ObligationId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;
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

/// Replay pacing applied to pull-based consumer delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConsumerReplayPolicy {
    /// Replay as quickly as policy gates allow.
    #[default]
    Instant,
    /// Preserve source pacing semantics when replaying historical windows.
    Original,
}

/// High-level dispatch mode for a FABRIC consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConsumerDispatchMode {
    /// Windows are pushed according to the active delivery policy.
    #[default]
    Push,
    /// Windows are served only in response to queued pull requests.
    Pull,
}

/// Static consumer configuration for the FABRIC delivery engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricConsumerConfig {
    /// Stable durable consumer name, when the consumer survives process restarts.
    pub durable_name: Option<String>,
    /// Optional subject filter narrowing which stream slice the consumer may see.
    pub filter_subject: Option<SubjectPattern>,
    /// Acknowledgement semantics for delivered messages.
    pub ack_policy: AckPolicy,
    /// Maximum number of delivery attempts before policy escalation.
    pub max_deliver: u16,
    /// Maximum number of messages that may remain pending acknowledgement.
    pub max_ack_pending: usize,
    /// Maximum queued pull requests waiting for service.
    pub max_waiting: usize,
    /// Ack deadline carried into obligation-backed pending state.
    pub ack_wait: Duration,
    /// Replay pacing for historical or recovery delivery.
    pub replay_policy: ConsumerReplayPolicy,
    /// Starting delivery anchor for replay-oriented pull requests.
    pub deliver_policy: DeliverPolicy,
    /// Whether explicit flow-control pause/resume is enabled.
    pub flow_control: bool,
    /// Heartbeat cadence while actively delivering.
    pub heartbeat: Option<Duration>,
    /// Heartbeat cadence while idle.
    pub idle_heartbeat: Option<Duration>,
}

impl Default for FabricConsumerConfig {
    fn default() -> Self {
        Self {
            durable_name: None,
            filter_subject: None,
            ack_policy: AckPolicy::Explicit,
            max_deliver: 1,
            max_ack_pending: 256,
            max_waiting: 64,
            ack_wait: Duration::from_secs(30),
            replay_policy: ConsumerReplayPolicy::Instant,
            deliver_policy: DeliverPolicy::All,
            flow_control: false,
            heartbeat: None,
            idle_heartbeat: None,
        }
    }
}

impl FabricConsumerConfig {
    /// Validate the consumer configuration before construction.
    pub fn validate(&self) -> Result<(), FabricConsumerError> {
        if self.max_deliver == 0 {
            return Err(FabricConsumerError::InvalidMaxDeliver);
        }
        if self.max_ack_pending == 0 {
            return Err(FabricConsumerError::InvalidMaxAckPending);
        }
        if self.max_waiting == 0 {
            return Err(FabricConsumerError::InvalidMaxWaiting);
        }
        if self.ack_wait.is_zero() {
            return Err(FabricConsumerError::InvalidAckWait);
        }
        if self.heartbeat.is_some_and(|duration| duration.is_zero()) {
            return Err(FabricConsumerError::InvalidHeartbeat { field: "heartbeat" });
        }
        if self
            .idle_heartbeat
            .is_some_and(|duration| duration.is_zero())
        {
            return Err(FabricConsumerError::InvalidHeartbeat {
                field: "idle_heartbeat",
            });
        }
        Ok(())
    }
}

/// Dynamic consumer-delivery policy toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FabricConsumerDeliveryPolicy {
    /// Whether the consumer currently runs in push or pull mode.
    pub mode: ConsumerDispatchMode,
    /// Whether the engine is paused by explicit flow control.
    pub paused: bool,
}

impl Default for FabricConsumerDeliveryPolicy {
    fn default() -> Self {
        Self {
            mode: ConsumerDispatchMode::Push,
            paused: false,
        }
    }
}

/// Pull request admitted into the consumer wait queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    /// Maximum number of messages requested.
    pub batch_size: u32,
    /// Named demand class used to interpret the request.
    pub demand_class: ConsumerDemandClass,
    /// Optional byte bound used to tighten the batch size conservatively.
    pub max_bytes: Option<u32>,
    /// Optional expiry in logical cursor ticks relative to enqueue time.
    pub expires: Option<u64>,
    /// Whether the request should fail fast when no data is currently available.
    pub no_wait: bool,
}

impl PullRequest {
    /// Create a new pull request with the required batch size and demand class.
    pub fn new(
        batch_size: u32,
        demand_class: ConsumerDemandClass,
    ) -> Result<Self, FabricConsumerError> {
        if batch_size == 0 {
            return Err(FabricConsumerError::InvalidPullBatchSize);
        }
        Ok(Self {
            batch_size,
            demand_class,
            max_bytes: None,
            expires: None,
            no_wait: false,
        })
    }

    /// Cap the request by a byte budget.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: u32) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }

    /// Expire the request after `ticks` logical cursor ticks.
    #[must_use]
    pub fn with_expires(mut self, ticks: u64) -> Self {
        self.expires = Some(ticks);
        self
    }

    /// Mark the request as no-wait.
    #[must_use]
    pub fn with_no_wait(mut self) -> Self {
        self.no_wait = true;
        self
    }

    fn effective_batch_size(&self) -> Result<u64, FabricConsumerError> {
        if self.max_bytes == Some(0) {
            return Err(FabricConsumerError::InvalidPullMaxBytes);
        }
        if self.expires == Some(0) {
            return Err(FabricConsumerError::InvalidPullExpiry);
        }
        let batch_size = u64::from(self.batch_size);
        let byte_bound = self.max_bytes.map_or(batch_size, u64::from);
        Ok(batch_size.min(byte_bound).max(1))
    }

    fn is_expired(&self, enqueued_at_tick: u64, current_tick: u64) -> bool {
        self.expires
            .is_some_and(|ttl| current_tick > enqueued_at_tick.saturating_add(ttl))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedPullRequest {
    request: PullRequest,
    enqueued_at_tick: u64,
}

/// Pending acknowledgement tracked against an obligation id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAckState {
    /// Inclusive sequence window still awaiting acknowledgement.
    pub window: SequenceWindow,
    /// Cursor delivery mode used when the attempt was issued.
    pub delivery_mode: CursorDeliveryMode,
    /// Monotonic attempt number for the logical delivery.
    pub delivery_attempt: u32,
}

/// Dynamic consumer state surfaced to policy and tests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FabricConsumerState {
    /// Total messages dispatched by this consumer engine.
    pub delivered_count: u64,
    /// Messages currently pending acknowledgement.
    pub pending_count: u64,
    /// Highest sequence durably acknowledged by the engine.
    pub ack_floor: u64,
    /// Pending acknowledgements keyed by their obligation id.
    pub pending_acks: BTreeMap<ObligationId, PendingAckState>,
    next_delivery_attempt: u32,
}

impl FabricConsumerState {
    fn next_attempt(&mut self) -> u32 {
        self.next_delivery_attempt = self.next_delivery_attempt.saturating_add(1).max(1);
        self.next_delivery_attempt
    }
}

/// Public request shape returned with a scheduled delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledConsumerRequest {
    /// A push delivery pinned to a concrete window.
    Push(SequenceWindow),
    /// A pull request resolved into a concrete delivery.
    Pull(PullRequest),
}

/// Concrete delivery scheduled by the consumer engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledConsumerDelivery {
    /// High-level request shape that led to this delivery.
    pub request: ScheduledConsumerRequest,
    /// Window selected for the concrete delivery attempt.
    pub window: SequenceWindow,
    /// Cursor attempt certificate minted for the delivery.
    pub attempt: AttemptCertificate,
    /// Delivery plan chosen from the current cursor lease plus capsule coverage.
    pub plan: DeliveryPlan,
}

/// Result of polling the next queued pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullDispatchOutcome {
    /// A concrete delivery was scheduled immediately.
    Scheduled(ScheduledConsumerDelivery),
    /// No data was available yet; the request remains queued.
    Waiting(PullRequest),
}

/// High-level policy-driven consumer engine layered on top of cursor leases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricConsumer {
    cursor: FabricConsumerCursor,
    config: FabricConsumerConfig,
    policy: FabricConsumerDeliveryPolicy,
    state: FabricConsumerState,
    waiting_pull_requests: Vec<QueuedPullRequest>,
}

impl FabricConsumer {
    /// Construct a new consumer engine from the current subject cell.
    pub fn new(
        cell: &SubjectCell,
        config: FabricConsumerConfig,
    ) -> Result<Self, FabricConsumerError> {
        config.validate()?;
        Ok(Self {
            cursor: FabricConsumerCursor::new(cell)?,
            config,
            policy: FabricConsumerDeliveryPolicy::default(),
            state: FabricConsumerState::default(),
            waiting_pull_requests: Vec::new(),
        })
    }

    /// Return the static consumer configuration.
    #[must_use]
    pub fn config(&self) -> &FabricConsumerConfig {
        &self.config
    }

    /// Return the current dynamic delivery policy.
    #[must_use]
    pub fn policy(&self) -> &FabricConsumerDeliveryPolicy {
        &self.policy
    }

    /// Return the dynamic consumer state.
    #[must_use]
    pub fn state(&self) -> &FabricConsumerState {
        &self.state
    }

    /// Return the number of queued pull requests still waiting for service.
    #[must_use]
    pub fn waiting_pull_request_count(&self) -> usize {
        self.waiting_pull_requests.len()
    }

    /// Return the current cursor lease.
    #[must_use]
    pub fn current_lease(&self) -> &CursorAuthorityLease {
        self.cursor.current_lease()
    }

    /// Advance logical time for ticket and pull-request expiry testing.
    pub fn advance_clock(&mut self, ticks: u64) -> u64 {
        self.cursor.advance_ticket_clock(ticks)
    }

    /// Switch between push and pull delivery.
    pub fn switch_mode(&mut self, mode: ConsumerDispatchMode) {
        self.policy.mode = mode;
        if mode == ConsumerDispatchMode::Push {
            self.waiting_pull_requests.clear();
        }
    }

    /// Pause the consumer with explicit flow control.
    pub fn pause(&mut self) -> Result<(), FabricConsumerError> {
        if !self.config.flow_control {
            return Err(FabricConsumerError::FlowControlDisabled);
        }
        self.policy.paused = true;
        Ok(())
    }

    /// Resume the consumer after an explicit pause.
    pub fn resume(&mut self) {
        self.policy.paused = false;
    }

    /// Queue a pull request for later dispatch.
    pub fn queue_pull_request(&mut self, request: PullRequest) -> Result<(), FabricConsumerError> {
        if self.policy.mode != ConsumerDispatchMode::Pull {
            return Err(FabricConsumerError::PullModeRequired);
        }
        let _ = request.effective_batch_size()?;
        if self.waiting_pull_requests.len() >= self.config.max_waiting {
            return Err(FabricConsumerError::MaxWaitingExceeded {
                limit: self.config.max_waiting,
            });
        }
        self.waiting_pull_requests.push(QueuedPullRequest {
            request,
            enqueued_at_tick: self.cursor.ticket_clock(),
        });
        Ok(())
    }

    /// Dispatch a concrete push window under the active lease.
    pub fn dispatch_push(
        &mut self,
        window: SequenceWindow,
        obligation_id: ObligationId,
        capsule: &RecoverableCapsule,
        ticket: Option<&ReadDelegationTicket>,
    ) -> Result<ScheduledConsumerDelivery, FabricConsumerError> {
        if self.policy.mode != ConsumerDispatchMode::Push {
            return Err(FabricConsumerError::PushModeRequired);
        }
        let delivery_mode = CursorDeliveryMode::Push { window };
        self.schedule_delivery(
            ScheduledConsumerRequest::Push(window),
            delivery_mode,
            window,
            obligation_id,
            capsule,
            ticket,
        )
    }

    /// Try to dispatch the next queued pull request.
    pub fn dispatch_next_pull(
        &mut self,
        available_tail: u64,
        obligation_id: ObligationId,
        capsule: &RecoverableCapsule,
        ticket: Option<&ReadDelegationTicket>,
    ) -> Result<PullDispatchOutcome, FabricConsumerError> {
        if self.policy.mode != ConsumerDispatchMode::Pull {
            return Err(FabricConsumerError::PullModeRequired);
        }

        let Some(mut queued) = self.pop_next_live_pull_request() else {
            return Err(FabricConsumerError::NoQueuedPullRequests);
        };
        let request = queued.request.clone();
        let Some(window) = self.resolve_pull_window(&request, available_tail)? else {
            if request.no_wait {
                return Err(FabricConsumerError::NoDataAvailable {
                    demand_class: request.demand_class,
                    available_tail,
                });
            }
            queued.enqueued_at_tick = self.cursor.ticket_clock();
            self.waiting_pull_requests.insert(0, queued);
            return Ok(PullDispatchOutcome::Waiting(request));
        };

        let delivery = self.schedule_delivery(
            ScheduledConsumerRequest::Pull(request),
            CursorDeliveryMode::Pull(CursorRequest::Window(window)),
            window,
            obligation_id,
            capsule,
            ticket,
        )?;
        Ok(PullDispatchOutcome::Scheduled(delivery))
    }

    /// Apply an acknowledgement attempt and update pending state on success.
    pub fn acknowledge_delivery(
        &mut self,
        attempt: &AttemptCertificate,
    ) -> Result<AckResolution, FabricConsumerError> {
        let resolution = self.cursor.acknowledge(attempt)?;
        if matches!(resolution, AckResolution::Committed { .. })
            && let Some(pending) = self.state.pending_acks.remove(&attempt.obligation_id)
        {
            self.state.pending_count = self
                .state
                .pending_count
                .saturating_sub(window_len(pending.window));
            self.state.ack_floor = self.state.ack_floor.max(pending.window.end());
        }
        Ok(resolution)
    }

    fn pop_next_live_pull_request(&mut self) -> Option<QueuedPullRequest> {
        let current_tick = self.cursor.ticket_clock();
        while !self.waiting_pull_requests.is_empty() {
            let queued = self.waiting_pull_requests.remove(0);
            if !queued
                .request
                .is_expired(queued.enqueued_at_tick, current_tick)
            {
                return Some(queued);
            }
        }
        None
    }

    fn resolve_pull_window(
        &self,
        request: &PullRequest,
        available_tail: u64,
    ) -> Result<Option<SequenceWindow>, FabricConsumerError> {
        let batch = request.effective_batch_size()?;
        if available_tail == 0 {
            return Ok(None);
        }

        let next_unacked = self.state.ack_floor.saturating_add(1).max(1);
        let resolve = match request.demand_class {
            ConsumerDemandClass::Tail => {
                let start = available_tail
                    .saturating_sub(batch.saturating_sub(1))
                    .max(1);
                Some((start, available_tail))
            }
            ConsumerDemandClass::CatchUp => {
                if next_unacked > available_tail {
                    None
                } else {
                    Some((
                        next_unacked,
                        available_tail.min(next_unacked.saturating_add(batch).saturating_sub(1)),
                    ))
                }
            }
            ConsumerDemandClass::Replay => {
                let start = self.replay_start_sequence(available_tail);
                if start > available_tail {
                    None
                } else {
                    Some((
                        start,
                        available_tail.min(start.saturating_add(batch).saturating_sub(1)),
                    ))
                }
            }
        };

        match resolve {
            Some((start, end)) => Ok(Some(SequenceWindow::new(start, end)?)),
            None => Ok(None),
        }
    }

    fn replay_start_sequence(&self, available_tail: u64) -> u64 {
        match self.config.deliver_policy {
            DeliverPolicy::All => 1,
            DeliverPolicy::New => available_tail.saturating_add(1),
            DeliverPolicy::ByStartSequence(sequence) => sequence.max(1),
            DeliverPolicy::Last | DeliverPolicy::LastPerSubject => available_tail.max(1),
        }
    }

    fn schedule_delivery(
        &mut self,
        request: ScheduledConsumerRequest,
        delivery_mode: CursorDeliveryMode,
        window: SequenceWindow,
        obligation_id: ObligationId,
        capsule: &RecoverableCapsule,
        ticket: Option<&ReadDelegationTicket>,
    ) -> Result<ScheduledConsumerDelivery, FabricConsumerError> {
        if self.policy.paused {
            return Err(FabricConsumerError::ConsumerPaused);
        }

        let window_messages = window_len(window);
        if self.state.pending_count.saturating_add(window_messages)
            > self.config.max_ack_pending as u64
        {
            return Err(FabricConsumerError::MaxAckPendingExceeded {
                limit: self.config.max_ack_pending,
                pending: self.state.pending_count,
            });
        }

        let delivery_attempt = self.state.next_attempt();
        let attempt = self
            .cursor
            .issue_attempt(delivery_mode, delivery_attempt, obligation_id)?;
        let plan = self.cursor.plan_delivery(delivery_mode, capsule, ticket)?;

        self.state.delivered_count = self.state.delivered_count.saturating_add(window_messages);
        self.state.pending_count = self.state.pending_count.saturating_add(window_messages);
        self.state.pending_acks.insert(
            obligation_id,
            PendingAckState {
                window,
                delivery_mode,
                delivery_attempt,
            },
        );

        Ok(ScheduledConsumerDelivery {
            request,
            window,
            attempt,
            plan,
        })
    }
}

fn window_len(window: SequenceWindow) -> u64 {
    window
        .end()
        .saturating_sub(window.start())
        .saturating_add(1)
}

/// High-level consumer-engine failures layered on top of cursor errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FabricConsumerError {
    /// Consumer delivery attempts require a positive retry budget.
    #[error("consumer max_deliver must be greater than zero")]
    InvalidMaxDeliver,
    /// Pending-ack flow control must reserve at least one message slot.
    #[error("consumer max_ack_pending must be greater than zero")]
    InvalidMaxAckPending,
    /// Pull-mode wait queues must reserve at least one slot.
    #[error("consumer max_waiting must be greater than zero")]
    InvalidMaxWaiting,
    /// Ack deadlines must be explicit and positive.
    #[error("consumer ack_wait must be greater than zero")]
    InvalidAckWait,
    /// Heartbeat fields must never be zero-duration sentinels.
    #[error("consumer {field} must be greater than zero when configured")]
    InvalidHeartbeat {
        /// Name of the invalid heartbeat field.
        field: &'static str,
    },
    /// Pull requests must ask for at least one message.
    #[error("pull request batch_size must be greater than zero")]
    InvalidPullBatchSize,
    /// Pull requests may not pretend that zero bytes are useful demand.
    #[error("pull request max_bytes must be greater than zero when configured")]
    InvalidPullMaxBytes,
    /// Pull-request expiries use logical ticks and must be positive.
    #[error("pull request expires must be greater than zero when configured")]
    InvalidPullExpiry,
    /// Push-only operations were attempted while the consumer is in pull mode.
    #[error("consumer is not in push mode")]
    PushModeRequired,
    /// Pull-only operations were attempted while the consumer is not in pull mode.
    #[error("consumer is not in pull mode")]
    PullModeRequired,
    /// Flow-control pause/resume is disabled in the static config.
    #[error("consumer flow control is disabled")]
    FlowControlDisabled,
    /// Dispatch is paused until the operator resumes the consumer.
    #[error("consumer dispatch is paused")]
    ConsumerPaused,
    /// Flow-control backpressure blocked another dispatch.
    #[error("consumer pending messages `{pending}` exceed or meet max_ack_pending `{limit}`")]
    MaxAckPendingExceeded {
        /// Configured pending-message limit.
        limit: usize,
        /// Current pending message count.
        pending: u64,
    },
    /// Pull queue admission exceeded the configured waiting bound.
    #[error("consumer already has max_waiting `{limit}` queued pull requests")]
    MaxWaitingExceeded {
        /// Configured max waiting pull requests.
        limit: usize,
    },
    /// No queued pull request was available when dispatch was attempted.
    #[error("consumer has no queued pull requests")]
    NoQueuedPullRequests,
    /// A no-wait pull request found no data.
    #[error(
        "no data available for pull request class `{demand_class:?}` at tail `{available_tail}`"
    )]
    NoDataAvailable {
        /// Demand class of the request that could not be served.
        demand_class: ConsumerDemandClass,
        /// Tail sequence visible to the consumer at dispatch time.
        available_tail: u64,
    },
    /// Low-level cursor machinery rejected the operation.
    #[error(transparent)]
    Cursor(#[from] ConsumerCursorError),
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

    #[test]
    fn fabric_consumer_creation_preserves_config_and_starts_clean() {
        let cell = test_cell();
        let config = FabricConsumerConfig {
            durable_name: Some("orders-durable".to_owned()),
            filter_subject: Some(SubjectPattern::parse("orders.*").expect("pattern")),
            flow_control: true,
            heartbeat: Some(std::time::Duration::from_secs(5)),
            idle_heartbeat: Some(std::time::Duration::from_secs(15)),
            ..FabricConsumerConfig::default()
        };

        let consumer = FabricConsumer::new(&cell, config.clone()).expect("consumer");
        assert_eq!(consumer.config(), &config);
        assert_eq!(consumer.policy().mode, ConsumerDispatchMode::Push);
        assert!(!consumer.policy().paused);
        assert_eq!(consumer.state().delivered_count, 0);
        assert_eq!(consumer.state().pending_count, 0);
        assert_eq!(consumer.state().ack_floor, 0);
        assert_eq!(consumer.waiting_pull_request_count(), 0);
        assert_eq!(
            consumer.current_lease().holder,
            CursorLeaseHolder::Steward(NodeId::new("node-a"))
        );
    }

    #[test]
    fn fabric_consumer_mode_switching_clears_waiting_pull_requests() {
        let cell = test_cell();
        let mut consumer =
            FabricConsumer::new(&cell, FabricConsumerConfig::default()).expect("consumer");

        consumer.switch_mode(ConsumerDispatchMode::Pull);
        consumer
            .queue_pull_request(
                PullRequest::new(2, ConsumerDemandClass::CatchUp).expect("pull request"),
            )
            .expect("queue pull request");
        assert_eq!(consumer.waiting_pull_request_count(), 1);

        consumer.switch_mode(ConsumerDispatchMode::Push);
        assert_eq!(consumer.policy().mode, ConsumerDispatchMode::Push);
        assert_eq!(consumer.waiting_pull_request_count(), 0);
    }

    #[test]
    fn fabric_consumer_pull_queue_respects_max_waiting() {
        let cell = test_cell();
        let mut consumer = FabricConsumer::new(
            &cell,
            FabricConsumerConfig {
                max_waiting: 1,
                ..FabricConsumerConfig::default()
            },
        )
        .expect("consumer");

        consumer.switch_mode(ConsumerDispatchMode::Pull);
        consumer
            .queue_pull_request(
                PullRequest::new(1, ConsumerDemandClass::CatchUp).expect("first request"),
            )
            .expect("queue first");

        assert_eq!(
            consumer.queue_pull_request(
                PullRequest::new(1, ConsumerDemandClass::Tail).expect("second request")
            ),
            Err(FabricConsumerError::MaxWaitingExceeded { limit: 1 })
        );
    }

    #[test]
    fn fabric_consumer_pull_dispatches_catchup_then_tail_windows() {
        let cell = test_cell();
        let mut consumer =
            FabricConsumer::new(&cell, FabricConsumerConfig::default()).expect("consumer");
        let capsule = RecoverableCapsule::default().with_window(
            NodeId::new("node-a"),
            SequenceWindow::new(1, 12).expect("window"),
        );

        consumer.switch_mode(ConsumerDispatchMode::Pull);
        consumer
            .queue_pull_request(
                PullRequest::new(3, ConsumerDemandClass::CatchUp).expect("catchup request"),
            )
            .expect("queue catchup");

        let first_outcome = consumer
            .dispatch_next_pull(12, obligation(30), &capsule, None)
            .expect("dispatch catchup");
        let first = if let PullDispatchOutcome::Scheduled(delivery) = first_outcome {
            delivery
        } else {
            assert!(false, "catchup request should schedule");
            return;
        };
        assert_eq!(first.window, SequenceWindow::new(1, 3).expect("window"));
        assert_eq!(consumer.state().pending_count, 3);
        assert_eq!(
            consumer.acknowledge_delivery(&first.attempt),
            Ok(AckResolution::Committed {
                obligation_id: obligation(30),
                against: CursorLeaseHolder::Steward(NodeId::new("node-a")),
            })
        );
        assert_eq!(consumer.state().pending_count, 0);
        assert_eq!(consumer.state().ack_floor, 3);

        consumer
            .queue_pull_request(PullRequest::new(2, ConsumerDemandClass::Tail).expect("tail"))
            .expect("queue tail");
        let tail_outcome = consumer
            .dispatch_next_pull(12, obligation(31), &capsule, None)
            .expect("dispatch tail");
        let tail = if let PullDispatchOutcome::Scheduled(delivery) = tail_outcome {
            delivery
        } else {
            assert!(false, "tail request should schedule");
            return;
        };
        assert_eq!(tail.window, SequenceWindow::new(11, 12).expect("window"));
    }

    #[test]
    fn fabric_consumer_pause_and_resume_gate_dispatch() {
        let cell = test_cell();
        let mut consumer = FabricConsumer::new(
            &cell,
            FabricConsumerConfig {
                flow_control: true,
                ..FabricConsumerConfig::default()
            },
        )
        .expect("consumer");
        let window = SequenceWindow::new(1, 1).expect("window");
        let capsule = RecoverableCapsule::default().with_window(NodeId::new("node-a"), window);

        consumer.pause().expect("pause");
        assert_eq!(
            consumer.dispatch_push(window, obligation(32), &capsule, None),
            Err(FabricConsumerError::ConsumerPaused)
        );

        consumer.resume();
        let delivery = consumer
            .dispatch_push(window, obligation(32), &capsule, None)
            .expect("dispatch after resume");
        assert_eq!(delivery.window, window);
    }

    #[test]
    fn fabric_consumer_max_ack_pending_blocks_until_ack_commit() {
        let cell = test_cell();
        let mut consumer = FabricConsumer::new(
            &cell,
            FabricConsumerConfig {
                max_ack_pending: 2,
                ..FabricConsumerConfig::default()
            },
        )
        .expect("consumer");
        let first_window = SequenceWindow::new(1, 2).expect("window");
        let second_window = SequenceWindow::new(3, 3).expect("window");
        let capsule = RecoverableCapsule::default().with_window(
            NodeId::new("node-a"),
            SequenceWindow::new(1, 3).expect("capsule"),
        );

        let first = consumer
            .dispatch_push(first_window, obligation(33), &capsule, None)
            .expect("first dispatch");
        assert_eq!(consumer.state().pending_count, 2);
        assert_eq!(
            consumer.dispatch_push(second_window, obligation(34), &capsule, None),
            Err(FabricConsumerError::MaxAckPendingExceeded {
                limit: 2,
                pending: 2,
            })
        );

        assert!(matches!(
            consumer.acknowledge_delivery(&first.attempt),
            Ok(AckResolution::Committed { .. })
        ));
        assert_eq!(consumer.state().pending_count, 0);

        let second = consumer
            .dispatch_push(second_window, obligation(34), &capsule, None)
            .expect("second dispatch");
        assert_eq!(second.window, second_window);
    }
}
