//! Cut-certified mobility primitives for FABRIC subject cells.
//!
//! A cut certificate captures the minimum state needed to move authority for a
//! hot cell without hand-wavy drain heuristics. Mobility stays explicit: the
//! caller mints a certificate from the current cell, then applies a lawful
//! mobility operation that yields a concrete next `SubjectCell`.

use super::fabric::{CellEpoch, CellId, SubjectCell};
use crate::remote::NodeId;
use crate::types::{ObligationId, Time};
use crate::util::DetHasher;
use std::hash::{Hash, Hasher};
use thiserror::Error;

/// Deterministic digest of consumer-side state captured at a certified cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ConsumerStateDigest(u64);

impl ConsumerStateDigest {
    /// Empty digest used when no consumer-side state has been retained.
    pub const ZERO: Self = Self(0);

    /// Create a new digest from a stable 64-bit value.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Return the raw digest value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Deterministic digest for a warm-restorable capsule snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CapsuleDigest(u64);

impl CapsuleDigest {
    /// Empty digest indicating that no capsule payload was supplied.
    pub const ZERO: Self = Self(0);

    /// Create a new digest from a stable 64-bit value.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Return the raw digest value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    #[must_use]
    const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// Proof artifact that a subject cell was cut at a well-defined frontier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CutCertificate {
    /// Cell whose authority this cut certifies.
    pub cell_id: CellId,
    /// Cell epoch fenced into the certificate.
    pub epoch: CellEpoch,
    /// Canonicalized live obligation frontier captured at the cut.
    pub obligation_frontier: Vec<ObligationId>,
    /// Opaque digest of consumer-side state retained at the cut.
    pub consumer_state_digest: ConsumerStateDigest,
    /// Logical time when the cut was minted.
    pub timestamp: Time,
    /// Steward that signed the cut.
    pub signer: NodeId,
}

impl CutCertificate {
    /// Mint a new certificate for the current subject cell state.
    pub fn issue(
        cell: &SubjectCell,
        obligation_frontier: impl IntoIterator<Item = ObligationId>,
        consumer_state_digest: ConsumerStateDigest,
        timestamp: Time,
        signer: NodeId,
    ) -> Result<Self, CutMobilityError> {
        if !contains_node(&cell.steward_set, &signer) {
            return Err(CutMobilityError::SignerNotInStewardSet {
                cell_id: cell.cell_id,
                signer,
            });
        }

        Ok(Self {
            cell_id: cell.cell_id,
            epoch: cell.epoch,
            obligation_frontier: canonicalize_frontier(obligation_frontier),
            consumer_state_digest,
            timestamp,
            signer,
        })
    }

    /// Verify that this certificate still applies to `cell`.
    pub fn validate_for(&self, cell: &SubjectCell) -> Result<(), CutMobilityError> {
        if self.cell_id != cell.cell_id {
            return Err(CutMobilityError::CellMismatch {
                certificate_cell: self.cell_id,
                actual_cell: cell.cell_id,
            });
        }
        if self.epoch != cell.epoch {
            return Err(CutMobilityError::EpochMismatch {
                certificate_epoch: self.epoch,
                actual_epoch: cell.epoch,
            });
        }
        if !contains_node(&cell.steward_set, &self.signer) {
            return Err(CutMobilityError::SignerNotInStewardSet {
                cell_id: cell.cell_id,
                signer: self.signer.clone(),
            });
        }
        Ok(())
    }

    /// Return true if the certificate explicitly captures `obligation`.
    #[must_use]
    pub fn covers_obligation(&self, obligation: ObligationId) -> bool {
        self.obligation_frontier.binary_search(&obligation).is_ok()
    }

    /// Deterministic digest of the cut frontier and attached consumer state.
    #[must_use]
    pub fn obligation_frontier_digest(&self) -> u64 {
        stable_hash((
            "cut-frontier",
            self.cell_id.raw(),
            self.epoch,
            &self.obligation_frontier,
            self.consumer_state_digest.raw(),
            self.timestamp.as_nanos(),
            self.signer.as_str(),
        ))
    }

    /// Deterministic digest of the full certificate payload.
    #[must_use]
    pub fn certificate_digest(&self) -> u64 {
        stable_hash(("cut-certificate", self))
    }
}

/// Lawful state-mobility transitions that may occur from a certified cut.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MobilityOperation {
    /// Urgently drain traffic away from a hot source steward.
    Evacuate {
        /// Current active steward being evacuated.
        from: NodeId,
        /// Steward that will take over authority.
        to: NodeId,
    },
    /// Planned service or consumer handoff under an explicit cut.
    Handoff {
        /// Current active steward handing off authority.
        from: NodeId,
        /// Successor steward taking over authority.
        to: NodeId,
    },
    /// Restore a warm capsule into a rebased epoch and target node.
    WarmRestore {
        /// Node where the warm capsule is restored.
        target: NodeId,
        /// Fresh epoch rebinding the restored cell away from live authority.
        restored_epoch: CellEpoch,
        /// Digest of the capsule payload being restored.
        capsule_digest: CapsuleDigest,
    },
    /// Promote another steward after the current active one fails.
    Failover {
        /// Steward deemed failed for the active lease.
        failed: NodeId,
        /// Steward promoted to continue service.
        promote_to: NodeId,
    },
}

impl MobilityOperation {
    /// Validate this operation against `cell` and `certificate`, then produce
    /// the concrete next cell state.
    pub fn certify(
        &self,
        cell: &SubjectCell,
        certificate: &CutCertificate,
    ) -> Result<CertifiedMobility, CutMobilityError> {
        certificate.validate_for(cell)?;

        let resulting_cell = match self {
            Self::Evacuate { from, to } => certify_evacuation(cell, certificate, from, to)?,
            Self::Handoff { from, to } => certify_handoff(cell, certificate, from, to)?,
            Self::WarmRestore {
                target,
                restored_epoch,
                capsule_digest,
            } => certify_warm_restore(cell, certificate, target, *restored_epoch, *capsule_digest)?,
            Self::Failover { failed, promote_to } => {
                certify_failover(cell, certificate, failed, promote_to)?
            }
        };

        Ok(CertifiedMobility {
            certificate: certificate.clone(),
            operation: self.clone(),
            obligation_frontier_digest: certificate.obligation_frontier_digest(),
            resulting_cell,
        })
    }
}

/// Concrete proof artifact for an applied mobility operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertifiedMobility {
    /// Cut certificate authorizing the move.
    pub certificate: CutCertificate,
    /// Operation applied to the certified cut.
    pub operation: MobilityOperation,
    /// Deterministic digest of the obligation frontier the move preserves.
    pub obligation_frontier_digest: u64,
    /// Resulting cell after the mobility operation.
    pub resulting_cell: SubjectCell,
}

impl CertifiedMobility {
    /// Deterministic digest of the transition proof.
    #[must_use]
    pub fn mobility_digest(&self) -> u64 {
        stable_hash((
            "certified-mobility",
            self.certificate.certificate_digest(),
            &self.operation,
            self.obligation_frontier_digest,
            self.resulting_cell.cell_id.raw(),
            self.resulting_cell.epoch,
            self.resulting_cell
                .control_capsule
                .sequencer_lease_generation,
            self.resulting_cell.control_capsule.policy_revision,
        ))
    }
}

impl SubjectCell {
    /// Mint a cut certificate rooted at the current subject cell.
    pub fn issue_cut_certificate(
        &self,
        obligation_frontier: impl IntoIterator<Item = ObligationId>,
        consumer_state_digest: ConsumerStateDigest,
        timestamp: Time,
        signer: NodeId,
    ) -> Result<CutCertificate, CutMobilityError> {
        CutCertificate::issue(
            self,
            obligation_frontier,
            consumer_state_digest,
            timestamp,
            signer,
        )
    }

    /// Apply a cut-certified mobility operation to the current subject cell.
    pub fn certify_mobility(
        &self,
        certificate: &CutCertificate,
        operation: &MobilityOperation,
    ) -> Result<CertifiedMobility, CutMobilityError> {
        operation.certify(self, certificate)
    }
}

/// Deterministic failures while minting or applying cut-certified mobility.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CutMobilityError {
    /// The certificate signer is not one of the cell's currently lawful stewards.
    #[error("cut certificate signer `{signer}` is not in the steward set for `{cell_id}`")]
    SignerNotInStewardSet {
        /// Cell the signer attempted to certify.
        cell_id: CellId,
        /// Unauthorized signer.
        signer: NodeId,
    },
    /// The certificate refers to a different cell id.
    #[error("cut certificate targets `{certificate_cell}` but current cell is `{actual_cell}`")]
    CellMismatch {
        /// Cell encoded into the certificate.
        certificate_cell: CellId,
        /// Cell the caller attempted to move.
        actual_cell: CellId,
    },
    /// The certificate refers to a different epoch.
    #[error(
        "cut certificate epoch {certificate_epoch:?} does not match current epoch {actual_epoch:?}"
    )]
    EpochMismatch {
        /// Epoch encoded into the certificate.
        certificate_epoch: CellEpoch,
        /// Epoch currently owned by the cell.
        actual_epoch: CellEpoch,
    },
    /// The cell has no active sequencer to evacuate or hand off.
    #[error("subject cell `{cell_id}` has no active sequencer")]
    NoActiveSequencer {
        /// Cell lacking an active sequencer.
        cell_id: CellId,
    },
    /// The requested source does not match the active steward.
    #[error("mobility source `{requested}` does not match active sequencer `{active}`")]
    SourceNotActive {
        /// Source requested by the operation.
        requested: NodeId,
        /// Actual active sequencer.
        active: NodeId,
    },
    /// Planned mobility requires the signer to match the active source.
    #[error("cut certificate signer `{signer}` must match mobility source `{active_source}`")]
    SignerMustMatchSource {
        /// Signer attached to the cut certificate.
        signer: NodeId,
        /// Active source steward being moved.
        active_source: NodeId,
    },
    /// The target node is not currently part of the steward set.
    #[error("mobility target `{target}` is not in the steward set for `{cell_id}`")]
    TargetNotInStewardSet {
        /// Cell being moved.
        cell_id: CellId,
        /// Proposed target node.
        target: NodeId,
    },
    /// The requested target is the same as the source or failed node.
    #[error("mobility target `{target}` must differ from source `{current_source}`")]
    TargetMatchesSource {
        /// Source node being moved away from.
        current_source: NodeId,
        /// Target node proposed by the operation.
        target: NodeId,
    },
    /// Failover must be acknowledged by a surviving signer.
    #[error("failover signer `{signer}` cannot also be the failed steward `{failed}`")]
    FailoverSignedByFailedNode {
        /// Signer attached to the certificate.
        signer: NodeId,
        /// Failed active steward.
        failed: NodeId,
    },
    /// Warm restore needs captured consumer state to restore meaningfully.
    #[error("warm restore requires a non-zero consumer-state digest")]
    MissingConsumerStateDigest,
    /// Warm restore must point at a concrete capsule payload.
    #[error("warm restore requires a non-zero capsule digest")]
    MissingCapsuleDigest,
    /// Warm restore must rebind into a newer epoch.
    #[error(
        "warm restore epoch {restored_epoch:?} must be newer than cut epoch {certificate_epoch:?}"
    )]
    StaleRestoreEpoch {
        /// Epoch requested by the restore.
        restored_epoch: CellEpoch,
        /// Epoch attached to the cut certificate.
        certificate_epoch: CellEpoch,
    },
}

fn certify_evacuation(
    cell: &SubjectCell,
    certificate: &CutCertificate,
    from: &NodeId,
    to: &NodeId,
) -> Result<SubjectCell, CutMobilityError> {
    let active = require_active_sequencer(cell)?;
    if from != active {
        return Err(CutMobilityError::SourceNotActive {
            requested: from.clone(),
            active: active.clone(),
        });
    }
    if &certificate.signer != from {
        return Err(CutMobilityError::SignerMustMatchSource {
            signer: certificate.signer.clone(),
            active_source: from.clone(),
        });
    }
    if from == to {
        return Err(CutMobilityError::TargetMatchesSource {
            current_source: from.clone(),
            target: to.clone(),
        });
    }
    if !contains_node(&cell.steward_set, to) {
        return Err(CutMobilityError::TargetNotInStewardSet {
            cell_id: cell.cell_id,
            target: to.clone(),
        });
    }

    let mut moved = advance_control_state(cell);
    moved.control_capsule.active_sequencer = Some(to.clone());
    move_node_to_front(&mut moved.steward_set, to);
    move_node_to_back(&mut moved.steward_set, from);
    move_node_to_front(&mut moved.control_capsule.steward_pool, to);
    move_node_to_back(&mut moved.control_capsule.steward_pool, from);
    Ok(moved)
}

fn certify_handoff(
    cell: &SubjectCell,
    certificate: &CutCertificate,
    from: &NodeId,
    to: &NodeId,
) -> Result<SubjectCell, CutMobilityError> {
    let active = require_active_sequencer(cell)?;
    if from != active {
        return Err(CutMobilityError::SourceNotActive {
            requested: from.clone(),
            active: active.clone(),
        });
    }
    if &certificate.signer != from {
        return Err(CutMobilityError::SignerMustMatchSource {
            signer: certificate.signer.clone(),
            active_source: from.clone(),
        });
    }
    if from == to {
        return Err(CutMobilityError::TargetMatchesSource {
            current_source: from.clone(),
            target: to.clone(),
        });
    }
    if !contains_node(&cell.steward_set, to) {
        return Err(CutMobilityError::TargetNotInStewardSet {
            cell_id: cell.cell_id,
            target: to.clone(),
        });
    }

    let mut moved = advance_control_state(cell);
    moved.control_capsule.active_sequencer = Some(to.clone());
    Ok(moved)
}

fn certify_warm_restore(
    cell: &SubjectCell,
    certificate: &CutCertificate,
    target: &NodeId,
    restored_epoch: CellEpoch,
    capsule_digest: CapsuleDigest,
) -> Result<SubjectCell, CutMobilityError> {
    if certificate.consumer_state_digest == ConsumerStateDigest::ZERO {
        return Err(CutMobilityError::MissingConsumerStateDigest);
    }
    if capsule_digest.is_zero() {
        return Err(CutMobilityError::MissingCapsuleDigest);
    }
    if restored_epoch <= certificate.epoch {
        return Err(CutMobilityError::StaleRestoreEpoch {
            restored_epoch,
            certificate_epoch: certificate.epoch,
        });
    }

    let mut restored = cell.clone();
    restored.epoch = restored_epoch;
    restored.cell_id = CellId::for_partition(restored_epoch, &restored.subject_partition);
    restored.control_capsule.active_sequencer = Some(target.clone());
    restored.control_capsule.sequencer_lease_generation = restored_epoch.generation;
    restored.control_capsule.policy_revision =
        restored.control_capsule.policy_revision.saturating_add(1);
    ensure_node_at_front(&mut restored.steward_set, target.clone());
    ensure_node_at_front(&mut restored.control_capsule.steward_pool, target.clone());
    Ok(restored)
}

fn certify_failover(
    cell: &SubjectCell,
    certificate: &CutCertificate,
    failed: &NodeId,
    promote_to: &NodeId,
) -> Result<SubjectCell, CutMobilityError> {
    let active = require_active_sequencer(cell)?;
    if failed != active {
        return Err(CutMobilityError::SourceNotActive {
            requested: failed.clone(),
            active: active.clone(),
        });
    }
    if &certificate.signer == failed {
        return Err(CutMobilityError::FailoverSignedByFailedNode {
            signer: certificate.signer.clone(),
            failed: failed.clone(),
        });
    }
    if failed == promote_to {
        return Err(CutMobilityError::TargetMatchesSource {
            current_source: failed.clone(),
            target: promote_to.clone(),
        });
    }
    if !contains_node(&cell.steward_set, promote_to) {
        return Err(CutMobilityError::TargetNotInStewardSet {
            cell_id: cell.cell_id,
            target: promote_to.clone(),
        });
    }

    let mut moved = advance_control_state(cell);
    moved.steward_set.retain(|node| node != failed);
    moved
        .control_capsule
        .steward_pool
        .retain(|node| node != failed);
    move_node_to_front(&mut moved.steward_set, promote_to);
    move_node_to_front(&mut moved.control_capsule.steward_pool, promote_to);
    moved.control_capsule.active_sequencer = Some(promote_to.clone());
    Ok(moved)
}

fn require_active_sequencer(cell: &SubjectCell) -> Result<&NodeId, CutMobilityError> {
    cell.control_capsule
        .active_sequencer
        .as_ref()
        .ok_or(CutMobilityError::NoActiveSequencer {
            cell_id: cell.cell_id,
        })
}

fn canonicalize_frontier(
    obligation_frontier: impl IntoIterator<Item = ObligationId>,
) -> Vec<ObligationId> {
    let mut frontier: Vec<_> = obligation_frontier.into_iter().collect();
    frontier.sort_unstable();
    frontier.dedup();
    frontier
}

fn advance_control_state(cell: &SubjectCell) -> SubjectCell {
    let mut next = cell.clone();
    next.control_capsule.sequencer_lease_generation = next
        .control_capsule
        .sequencer_lease_generation
        .saturating_add(1);
    next.control_capsule.policy_revision = next.control_capsule.policy_revision.saturating_add(1);
    next
}

fn contains_node(nodes: &[NodeId], candidate: &NodeId) -> bool {
    nodes.iter().any(|node| node == candidate)
}

fn move_node_to_front(nodes: &mut Vec<NodeId>, candidate: &NodeId) {
    if let Some(index) = nodes.iter().position(|node| node == candidate) {
        let node = nodes.remove(index);
        nodes.insert(0, node);
    }
}

fn move_node_to_back(nodes: &mut Vec<NodeId>, candidate: &NodeId) {
    if let Some(index) = nodes.iter().position(|node| node == candidate) {
        let node = nodes.remove(index);
        nodes.push(node);
    }
}

fn ensure_node_at_front(nodes: &mut Vec<NodeId>, candidate: NodeId) {
    if let Some(index) = nodes.iter().position(|node| node == &candidate) {
        let node = nodes.remove(index);
        nodes.insert(0, node);
    } else {
        nodes.insert(0, candidate);
    }
}

fn stable_hash<T: Hash>(value: T) -> u64 {
    let mut hasher = DetHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
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
    fn evacuation_carries_obligation_frontier_proof() {
        let cell = test_cell();
        let certificate = cell
            .issue_cut_certificate(
                [obligation(7), obligation(3), obligation(7)],
                ConsumerStateDigest::new(0xfeed_cafe),
                Time::from_secs(9),
                NodeId::new("node-a"),
            )
            .expect("certificate");

        let proof = cell
            .certify_mobility(
                &certificate,
                &MobilityOperation::Evacuate {
                    from: NodeId::new("node-a"),
                    to: NodeId::new("node-b"),
                },
            )
            .expect("evacuation proof");

        assert_eq!(
            certificate.obligation_frontier,
            vec![obligation(3), obligation(7)]
        );
        assert!(certificate.covers_obligation(obligation(3)));
        assert_eq!(
            proof.obligation_frontier_digest,
            certificate.obligation_frontier_digest()
        );
        assert_eq!(
            proof.resulting_cell.control_capsule.active_sequencer,
            Some(NodeId::new("node-b"))
        );
        assert_eq!(
            proof.resulting_cell.steward_set.first(),
            Some(&NodeId::new("node-b"))
        );
        assert_eq!(
            proof.resulting_cell.steward_set.last(),
            Some(&NodeId::new("node-a"))
        );
        assert_eq!(
            proof
                .resulting_cell
                .control_capsule
                .sequencer_lease_generation,
            cell.control_capsule.sequencer_lease_generation + 1
        );
    }

    #[test]
    fn handoff_uses_explicit_cut_certificate() {
        let cell = test_cell();
        let certificate = cell
            .issue_cut_certificate(
                [obligation(10)],
                ConsumerStateDigest::new(0x1234),
                Time::from_secs(11),
                NodeId::new("node-a"),
            )
            .expect("certificate");

        let proof = cell
            .certify_mobility(
                &certificate,
                &MobilityOperation::Handoff {
                    from: NodeId::new("node-a"),
                    to: NodeId::new("node-c"),
                },
            )
            .expect("handoff proof");

        assert_eq!(
            proof.resulting_cell.control_capsule.active_sequencer,
            Some(NodeId::new("node-c"))
        );
        assert_eq!(proof.resulting_cell.steward_set, cell.steward_set);
        assert_eq!(
            proof.resulting_cell.control_capsule.steward_pool,
            cell.control_capsule.steward_pool
        );
        assert_eq!(
            proof.resulting_cell.control_capsule.policy_revision,
            cell.control_capsule.policy_revision + 1
        );
    }

    #[test]
    fn warm_restore_rebinds_epoch_and_cell_id_from_capsule() {
        let cell = test_cell();
        let restored_epoch = CellEpoch::new(8, 1);
        let certificate = cell
            .issue_cut_certificate(
                [obligation(2)],
                ConsumerStateDigest::new(0xface_b00c),
                Time::from_secs(13),
                NodeId::new("node-a"),
            )
            .expect("certificate");

        let proof = cell
            .certify_mobility(
                &certificate,
                &MobilityOperation::WarmRestore {
                    target: NodeId::new("edge-restore"),
                    restored_epoch,
                    capsule_digest: CapsuleDigest::new(0x9abc),
                },
            )
            .expect("warm restore proof");

        assert_eq!(proof.resulting_cell.epoch, restored_epoch);
        assert_eq!(
            proof.resulting_cell.cell_id,
            CellId::for_partition(restored_epoch, &cell.subject_partition)
        );
        assert_ne!(proof.resulting_cell.cell_id, cell.cell_id);
        assert_eq!(
            proof.resulting_cell.control_capsule.active_sequencer,
            Some(NodeId::new("edge-restore"))
        );
        assert_eq!(
            proof.resulting_cell.steward_set.first(),
            Some(&NodeId::new("edge-restore"))
        );
    }

    #[test]
    fn failover_removes_failed_steward_and_promotes_replacement() {
        let cell = test_cell();
        let certificate = cell
            .issue_cut_certificate(
                [obligation(1), obligation(4)],
                ConsumerStateDigest::new(0x2222),
                Time::from_secs(21),
                NodeId::new("node-b"),
            )
            .expect("certificate");

        let proof = cell
            .certify_mobility(
                &certificate,
                &MobilityOperation::Failover {
                    failed: NodeId::new("node-a"),
                    promote_to: NodeId::new("node-c"),
                },
            )
            .expect("failover proof");

        assert_eq!(
            proof.resulting_cell.control_capsule.active_sequencer,
            Some(NodeId::new("node-c"))
        );
        assert!(
            !proof
                .resulting_cell
                .steward_set
                .contains(&NodeId::new("node-a"))
        );
        assert!(
            !proof
                .resulting_cell
                .control_capsule
                .steward_pool
                .contains(&NodeId::new("node-a"))
        );
        assert_eq!(
            proof.resulting_cell.steward_set.first(),
            Some(&NodeId::new("node-c"))
        );
        assert_eq!(
            proof
                .resulting_cell
                .control_capsule
                .sequencer_lease_generation,
            cell.control_capsule.sequencer_lease_generation + 1
        );
    }

    #[test]
    fn warm_restore_rejects_missing_capsule_or_consumer_state() {
        let cell = test_cell();
        let empty_certificate = cell
            .issue_cut_certificate(
                [],
                ConsumerStateDigest::ZERO,
                Time::from_secs(14),
                NodeId::new("node-a"),
            )
            .expect("certificate");

        let err = cell
            .certify_mobility(
                &empty_certificate,
                &MobilityOperation::WarmRestore {
                    target: NodeId::new("edge-restore"),
                    restored_epoch: CellEpoch::new(9, 1),
                    capsule_digest: CapsuleDigest::ZERO,
                },
            )
            .expect_err("restore without state must fail");

        assert_eq!(err, CutMobilityError::MissingConsumerStateDigest);
    }
}
