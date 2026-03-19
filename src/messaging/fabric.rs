//! Foundational brokerless subject-fabric types and placement rules.
//!
//! The goal of this module is deliberately narrow: define the smallest
//! trustworthy `SubjectCell` model plus the canonical subject-partition and
//! deterministic placement rules that later brokerless beads can build on.
//! It does not attempt to implement the full control log, cursor leasing, or
//! stream semantics yet.

use super::class::{AckKind, DeliveryClass};
pub use super::subject::{Subject, SubjectPattern, SubjectPatternError, SubjectToken};
use crate::cx::Cx;
use crate::distributed::HashRing;
use crate::error::{Error as AsupersyncError, ErrorKind};
use crate::remote::NodeId;
use crate::util::DetHasher;
use parking_lot::Mutex;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

fn fabric_input_error(message: impl Into<String>) -> AsupersyncError {
    AsupersyncError::new(ErrorKind::User).with_message(message)
}

fn parse_subject(raw: impl AsRef<str>) -> Result<Subject, AsupersyncError> {
    Subject::parse(raw.as_ref()).map_err(|error| fabric_input_error(error.to_string()))
}

fn parse_subject_pattern(raw: impl AsRef<str>) -> Result<SubjectPattern, AsupersyncError> {
    SubjectPattern::parse(raw.as_ref()).map_err(|error| fabric_input_error(error.to_string()))
}

/// Minimal public Browser/Native FABRIC handle.
///
/// This surface intentionally models the NATS-small API promised by the FABRIC
/// plan without pretending the full distributed data plane is implemented yet.
/// The current behavior is an in-process semantic seam that:
///
/// - validates subjects and subject patterns,
/// - preserves explicit `&Cx` propagation on every async entry point, and
/// - keeps Layer 0 publish/subscribe on the default
///   [`DeliveryClass::EphemeralInteractive`] path.
#[derive(Debug, Clone)]
pub struct Fabric {
    endpoint: String,
    state: Arc<Mutex<FabricState>>,
}

#[derive(Debug, Default)]
struct FabricState {
    published: Vec<FabricMessage>,
}

/// Published or received packet-plane message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricMessage {
    /// Concrete subject of the message.
    pub subject: Subject,
    /// Message payload bytes.
    pub payload: Vec<u8>,
    /// Semantic class applied to the message.
    pub delivery_class: DeliveryClass,
}

/// Packet-plane publish acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReceipt {
    /// Subject accepted by the packet plane.
    pub subject: Subject,
    /// Number of payload bytes accepted.
    pub payload_len: usize,
    /// Acknowledgement boundary reached by the operation.
    pub ack_kind: AckKind,
    /// Delivery class used for the publish.
    pub delivery_class: DeliveryClass,
}

/// Request/reply response envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricReply {
    /// Reply subject echoed by the current semantic seam.
    pub subject: Subject,
    /// Reply payload bytes.
    pub payload: Vec<u8>,
    /// Acknowledgement boundary observed for the request.
    pub ack_kind: AckKind,
    /// Delivery class used for the request.
    pub delivery_class: DeliveryClass,
}

/// Capture policy for stream declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapturePolicy {
    /// Stream capture is disabled.
    #[default]
    Disabled,
    /// Capture only when the caller explicitly opts into the stream.
    ExplicitOptIn,
}

/// Public stream configuration for `Fabric::stream`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricStreamConfig {
    /// Subjects captured by the stream declaration.
    pub subjects: Vec<SubjectPattern>,
    /// Requested delivery class for the stream surface.
    pub delivery_class: DeliveryClass,
    /// Capture behavior for matching packet-plane traffic.
    pub capture_policy: CapturePolicy,
    /// Optional request timeout carried into stream operations.
    pub request_timeout: Option<Duration>,
}

impl Default for FabricStreamConfig {
    fn default() -> Self {
        Self {
            subjects: Vec::new(),
            delivery_class: DeliveryClass::EphemeralInteractive,
            capture_policy: CapturePolicy::ExplicitOptIn,
            request_timeout: None,
        }
    }
}

impl FabricStreamConfig {
    fn validate(&self) -> Result<(), AsupersyncError> {
        if self.subjects.is_empty() {
            return Err(AsupersyncError::new(ErrorKind::ConfigError)
                .with_message("stream config must declare at least one subject pattern"));
        }

        SubjectPattern::validate_non_overlapping(&self.subjects)
            .map_err(|error| fabric_input_error(error.to_string()))?;
        Ok(())
    }
}

/// Ergonomic alias matching the planned user-facing `stream(...)` example.
pub type StreamConfig = FabricStreamConfig;

/// Handle returned by `Fabric::stream`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricStreamHandle {
    endpoint: String,
    config: FabricStreamConfig,
}

impl FabricStreamHandle {
    /// Return the configured stream declaration.
    #[must_use]
    pub fn config(&self) -> &FabricStreamConfig {
        &self.config
    }

    /// Return the endpoint that created the stream declaration.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// Subscription handle returned by `Fabric::subscribe`.
#[derive(Debug, Clone)]
pub struct FabricSubscription {
    pattern: SubjectPattern,
    next_index: usize,
    state: Arc<Mutex<FabricState>>,
}

impl FabricSubscription {
    /// Return the subscribed pattern.
    #[must_use]
    pub fn pattern(&self) -> &SubjectPattern {
        &self.pattern
    }

    /// Return the next matching message, if one is currently available.
    ///
    /// Cancellation propagates by returning `None` once the supplied `Cx`
    /// observes a cancellation request.
    pub async fn next(&mut self, cx: &Cx) -> Option<FabricMessage> {
        if cx.checkpoint().is_err() {
            return None;
        }

        let state = self.state.lock();
        let published = &state.published;

        while self.next_index < published.len() {
            let message = published[self.next_index].clone();
            self.next_index += 1;
            if self.pattern.matches(&message.subject) {
                return Some(message);
            }
        }

        None
    }
}

impl Fabric {
    /// Connect to a known fabric endpoint.
    pub async fn connect(cx: &Cx, endpoint: impl AsRef<str>) -> Result<Self, AsupersyncError> {
        cx.checkpoint()?;

        let endpoint = endpoint.as_ref().trim();
        if endpoint.is_empty() {
            return Err(AsupersyncError::new(ErrorKind::ConfigError)
                .with_message("fabric endpoint must not be empty"));
        }

        Ok(Self {
            endpoint: endpoint.to_owned(),
            state: Arc::new(Mutex::new(FabricState::default())),
        })
    }

    /// Return the endpoint used for the current handle.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Publish a packet-plane message with the default delivery class.
    pub async fn publish(
        &self,
        cx: &Cx,
        subject: impl AsRef<str>,
        payload: impl Into<Vec<u8>>,
    ) -> Result<PublishReceipt, AsupersyncError> {
        cx.checkpoint()?;

        let subject = parse_subject(subject)?;
        let payload = payload.into();
        let message = FabricMessage {
            subject: subject.clone(),
            payload: payload.clone(),
            delivery_class: DeliveryClass::EphemeralInteractive,
        };
        self.state.lock().published.push(message);

        Ok(PublishReceipt {
            subject,
            payload_len: payload.len(),
            ack_kind: AckKind::Accepted,
            delivery_class: DeliveryClass::EphemeralInteractive,
        })
    }

    /// Subscribe to a packet-plane subject pattern.
    pub async fn subscribe(
        &self,
        cx: &Cx,
        subject_pattern: impl AsRef<str>,
    ) -> Result<FabricSubscription, AsupersyncError> {
        cx.checkpoint()?;

        Ok(FabricSubscription {
            pattern: parse_subject_pattern(subject_pattern)?,
            next_index: 0,
            state: Arc::clone(&self.state),
        })
    }

    /// Issue a bounded request/reply interaction.
    ///
    /// The current API-design seam performs an immediate loopback reply so the
    /// public surface is testable before the full authority/data plane lands.
    pub async fn request(
        &self,
        cx: &Cx,
        subject: impl AsRef<str>,
        payload: impl Into<Vec<u8>>,
    ) -> Result<FabricReply, AsupersyncError> {
        let payload = payload.into();
        let receipt = self.publish(cx, subject, payload.clone()).await?;

        Ok(FabricReply {
            subject: receipt.subject,
            payload,
            ack_kind: receipt.ack_kind,
            delivery_class: receipt.delivery_class,
        })
    }

    /// Opt into a stream declaration with explicit configuration.
    pub async fn stream(
        &self,
        cx: &Cx,
        config: FabricStreamConfig,
    ) -> Result<FabricStreamHandle, AsupersyncError> {
        cx.checkpoint()?;
        config.validate()?;

        Ok(FabricStreamHandle {
            endpoint: self.endpoint.clone(),
            config,
        })
    }
}

/// Compact identifier for a subject cell.
///
/// `CellId` is deterministic for a given canonical subject partition and
/// membership epoch so replay and placement evidence stay stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId(u128);

impl CellId {
    /// Derive a stable cell id for the given subject partition and epoch.
    #[must_use]
    pub fn for_partition(epoch: CellEpoch, subject_partition: &SubjectPattern) -> Self {
        let canonical = subject_partition.canonical_key();
        let lower = stable_hash((
            "subject-cell",
            epoch.membership_epoch,
            epoch.generation,
            &canonical,
        ));
        let upper = stable_hash((
            "subject-cell:v2",
            epoch.membership_epoch,
            epoch.generation,
            &canonical,
        ));
        Self((u128::from(upper) << 64) | u128::from(lower))
    }

    /// Return the raw 128-bit identifier.
    #[must_use]
    pub const fn raw(self) -> u128 {
        self.0
    }
}

impl fmt::Display for CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cell-{:032x}", self.0)
    }
}

/// Membership epoch and local generation for a subject cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellEpoch {
    /// Cluster or roster epoch used for placement.
    pub membership_epoch: u64,
    /// Per-cell generation inside the membership epoch.
    pub generation: u64,
}

impl CellEpoch {
    /// Create a new cell epoch descriptor.
    #[must_use]
    pub const fn new(membership_epoch: u64, generation: u64) -> Self {
        Self {
            membership_epoch,
            generation,
        }
    }
}

impl SubjectPattern {
    /// Aggregate ephemeral reply subjects before placement.
    ///
    /// This intentionally collapses reply-space suffix churn so fabric cells do
    /// not explode on per-request inbox identifiers.
    #[must_use]
    pub fn aggregate_reply_space(&self, policy: ReplySpaceCompactionPolicy) -> Self {
        if !policy.enabled
            || !self.is_reply_subject()
            || self.segments().len() <= policy.preserve_segments
        {
            return self.clone();
        }

        let keep = policy.preserve_segments.max(1).min(self.segments().len());
        let mut segments = self.segments()[..keep].to_vec();
        if !matches!(segments.last(), Some(SubjectToken::Tail)) {
            segments.push(SubjectToken::Tail);
        }
        Self::from_tokens(segments).expect("reply-space compaction must produce a valid pattern")
    }

    /// Validate that the provided set of patterns is pairwise non-overlapping.
    pub fn validate_non_overlapping(patterns: &[Self]) -> Result<(), FabricError> {
        for (index, left) in patterns.iter().enumerate() {
            for right in patterns.iter().skip(index + 1) {
                if left.overlaps(right) {
                    return Err(FabricError::OverlappingSubjectPartitions {
                        left: left.clone(),
                        right: right.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn is_reply_subject(&self) -> bool {
        matches!(
            self.segments().first(),
            Some(SubjectToken::Literal(prefix))
                if prefix == "_INBOX" || prefix == "_RPLY" || prefix == "reply"
        )
    }

    fn literal_segments(&self) -> Result<Vec<String>, SubjectPatternError> {
        self.segments()
            .iter()
            .map(|segment| match segment {
                SubjectToken::Literal(value) => Ok(value.clone()),
                SubjectToken::One | SubjectToken::Tail => Err(
                    SubjectPatternError::LiteralOnlyPatternRequired(self.canonical_key()),
                ),
            })
            .collect()
    }
}

/// Reply-space compaction settings applied before placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplySpaceCompactionPolicy {
    /// Whether reply-space aggregation is enabled.
    pub enabled: bool,
    /// Number of leading segments to keep before collapsing the suffix.
    pub preserve_segments: usize,
}

impl Default for ReplySpaceCompactionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            preserve_segments: 3,
        }
    }
}

/// Deterministic literal-prefix rewrite applied before placement.
///
/// This models the "import/export morphism" stage from the fabric plan without
/// allowing wildcard-bearing rewrites that would re-introduce ambiguity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectPrefixMorphism {
    from: Vec<String>,
    to: Vec<String>,
}

impl SubjectPrefixMorphism {
    /// Create a new literal-prefix rewrite.
    pub fn new(from: &str, to: &str) -> Result<Self, SubjectPatternError> {
        let from = SubjectPattern::parse(from)?;
        let to = SubjectPattern::parse(to)?;

        Ok(Self {
            from: from.literal_segments()?,
            to: to.literal_segments()?,
        })
    }

    fn apply(&self, pattern: &SubjectPattern) -> Option<SubjectPattern> {
        if pattern.segments().len() < self.from.len() {
            return None;
        }

        let mut remainder = Vec::new();
        for (index, segment) in pattern.segments().iter().enumerate() {
            let Some(expected) = self.from.get(index) else {
                remainder.push(segment.clone());
                continue;
            };

            match segment {
                SubjectToken::Literal(value) if value == expected => {}
                _ => return None,
            }
        }

        let mut rewritten = self
            .to
            .iter()
            .cloned()
            .map(SubjectToken::Literal)
            .collect::<Vec<_>>();
        rewritten.extend(remainder);
        Some(
            SubjectPattern::from_tokens(rewritten)
                .expect("rewritten literal-prefix morphism must stay syntactically valid"),
        )
    }
}

/// Canonicalization pipeline that runs before subject-cell placement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NormalizationPolicy {
    /// Ordered literal-prefix rewrites that canonicalize alias subject spaces.
    pub morphisms: Vec<SubjectPrefixMorphism>,
    /// Reply-space aggregation policy applied after morphisms.
    pub reply_space_policy: ReplySpaceCompactionPolicy,
}

impl NormalizationPolicy {
    /// Produce the authoritative canonical subject partition used for placement.
    pub fn normalize(&self, pattern: &SubjectPattern) -> Result<SubjectPattern, FabricError> {
        let mut canonical = pattern.clone();
        let mut seen = BTreeSet::from([canonical.canonical_key()]);
        let mut index = 0;

        while index < self.morphisms.len() {
            let Some(candidate) = self.morphisms[index].apply(&canonical) else {
                index += 1;
                continue;
            };

            for other in self.morphisms.iter().skip(index + 1) {
                let Some(other_candidate) = other.apply(&canonical) else {
                    continue;
                };
                if candidate != other_candidate {
                    return Err(FabricError::ConflictingSubjectMorphisms {
                        subject: pattern.clone(),
                        left: candidate,
                        right: other_candidate,
                    });
                }
            }

            if candidate == canonical {
                index += 1;
                continue;
            }

            if !seen.insert(candidate.canonical_key()) {
                return Err(FabricError::CyclicSubjectMorphisms {
                    subject: pattern.clone(),
                    cycle_point: candidate,
                });
            }

            canonical = candidate;
            index = 0;
        }

        Ok(canonical.aggregate_reply_space(self.reply_space_policy))
    }
}

/// Coarse cell traffic temperature used to scale stewardship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellTemperature {
    /// Minimal steward footprint for cold partitions.
    Cold,
    /// Intermediate steward footprint.
    Warm,
    /// Wider steward set for hot partitions.
    Hot,
}

/// Observed load signal used to steer temperature transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedCellLoad {
    /// Approximate publish arrival rate for the cell.
    pub publishes_per_second: u64,
}

impl ObservedCellLoad {
    /// Create a simple load sample from a publish rate estimate.
    #[must_use]
    pub const fn new(publishes_per_second: u64) -> Self {
        Self {
            publishes_per_second,
        }
    }
}

/// Hysteresis thresholds that damp steward-set temperature changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThermalHysteresis {
    /// Promote cold cells to warm once this rate is reached.
    pub cold_to_warm_publishes_per_second: u64,
    /// Demote warm cells back to cold only once load falls below this rate.
    pub warm_to_cold_publishes_per_second: u64,
    /// Promote warm cells to hot once this rate is reached.
    pub warm_to_hot_publishes_per_second: u64,
    /// Demote hot cells back to warm only once load falls below this rate.
    pub hot_to_warm_publishes_per_second: u64,
}

impl Default for ThermalHysteresis {
    fn default() -> Self {
        Self {
            cold_to_warm_publishes_per_second: 128,
            warm_to_cold_publishes_per_second: 48,
            warm_to_hot_publishes_per_second: 1_024,
            hot_to_warm_publishes_per_second: 512,
        }
    }
}

/// Explicit budget limiting how aggressively a steward set may change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebalanceBudget {
    /// Maximum node additions/removals allowed in a single rebalance step.
    pub max_steward_changes: usize,
}

impl Default for RebalanceBudget {
    fn default() -> Self {
        Self {
            max_steward_changes: 2,
        }
    }
}

/// Incremental steward-set transition plan under hysteresis and budget limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebalancePlan {
    /// Temperature recommended after applying hysteresis to the observed load.
    pub next_temperature: CellTemperature,
    /// Steward set after applying the rebalance budget to the desired target.
    pub next_stewards: Vec<NodeId>,
    /// Newly added stewards in this incremental rebalance step.
    pub added_stewards: Vec<NodeId>,
    /// Stewards removed in this incremental rebalance step.
    pub removed_stewards: Vec<NodeId>,
}

/// Storage class used during steward negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageClass {
    /// Ephemeral memory-only participation.
    Ephemeral,
    /// General durable node.
    Standard,
    /// Durable or archival-capable node.
    Durable,
}

/// Health tier used during steward negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StewardHealth {
    /// Fully eligible and healthy.
    Healthy,
    /// Eligible but less preferred.
    Degraded,
    /// Draining; still visible but last resort.
    Draining,
    /// Not eligible for stewardship.
    Unavailable,
}

/// Logical role a node may play inside the subject fabric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeRole {
    /// Publisher or request originator for a subject flow.
    Origin,
    /// Passive subscriber consuming pushed messages.
    Subscriber,
    /// Stateful consumer with explicit cursor or delivery ownership.
    Consumer,
    /// Node eligible to steward the control and data capsules of a cell.
    Steward,
    /// Node eligible to store repair symbols outside the active steward quorum.
    RepairWitness,
    /// Node allowed to relay traffic across topology boundaries.
    Bridge,
}

/// Candidate node used during steward placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StewardCandidate {
    /// Stable identity of the candidate node.
    pub node_id: NodeId,
    /// Logical roles currently available on the node.
    pub roles: BTreeSet<NodeRole>,
    /// Current health state used during placement scoring.
    pub health: StewardHealth,
    /// Durability tier offered by the node.
    pub storage_class: StorageClass,
    /// Failure-domain label used to diversify steward placement.
    pub failure_domain: String,
    /// Measured or budgeted one-way latency envelope in milliseconds.
    pub latency_millis: u32,
}

impl StewardCandidate {
    /// Create a new candidate with conservative defaults.
    #[must_use]
    pub fn new(node_id: NodeId, failure_domain: impl Into<String>) -> Self {
        Self {
            node_id,
            roles: BTreeSet::new(),
            health: StewardHealth::Healthy,
            storage_class: StorageClass::Standard,
            failure_domain: failure_domain.into(),
            latency_millis: 10,
        }
    }

    /// Mark the candidate with an additional role.
    #[must_use]
    pub fn with_role(mut self, role: NodeRole) -> Self {
        self.roles.insert(role);
        self
    }

    /// Override the candidate health.
    #[must_use]
    pub fn with_health(mut self, health: StewardHealth) -> Self {
        self.health = health;
        self
    }

    /// Override the storage class.
    #[must_use]
    pub fn with_storage_class(mut self, storage_class: StorageClass) -> Self {
        self.storage_class = storage_class;
        self
    }

    /// Override the measured latency envelope.
    #[must_use]
    pub fn with_latency_millis(mut self, latency_millis: u32) -> Self {
        self.latency_millis = latency_millis;
        self
    }

    /// Return true when the node is currently eligible to act as a steward.
    #[must_use]
    pub fn is_steward_eligible(&self) -> bool {
        self.roles.contains(&NodeRole::Steward) && self.health != StewardHealth::Unavailable
    }

    /// Return true when the node can also act as a repair witness.
    #[must_use]
    pub fn can_repair(&self) -> bool {
        self.roles.contains(&NodeRole::RepairWitness) || self.is_steward_eligible()
    }
}

/// Foundational placement policy for a `SubjectCell`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementPolicy {
    /// Virtual node count used by the deterministic hash ring.
    pub vnodes_per_node: usize,
    /// Number of candidate nodes to consider before final negotiation.
    pub candidate_pool_size: usize,
    /// Target steward count for cold cells.
    pub cold_stewards: usize,
    /// Target steward count for warm cells.
    pub warm_stewards: usize,
    /// Target steward count for hot cells.
    pub hot_stewards: usize,
    /// Soft latency cap for preferred candidates.
    pub max_latency_millis: u32,
    /// Load thresholds used to damp temperature transitions.
    pub thermal_hysteresis: ThermalHysteresis,
    /// Budget limiting how many steward moves one rebalance may perform.
    pub rebalance_budget: RebalanceBudget,
    /// Canonicalization rules applied before consistent hashing.
    pub normalization: NormalizationPolicy,
}

impl Default for PlacementPolicy {
    fn default() -> Self {
        Self {
            vnodes_per_node: 64,
            candidate_pool_size: 6,
            cold_stewards: 1,
            warm_stewards: 3,
            hot_stewards: 5,
            max_latency_millis: 150,
            thermal_hysteresis: ThermalHysteresis::default(),
            rebalance_budget: RebalanceBudget::default(),
            normalization: NormalizationPolicy::default(),
        }
    }
}

impl PlacementPolicy {
    /// Recommend the next cell temperature from the current load sample.
    #[must_use]
    pub fn recommend_temperature(
        &self,
        current: CellTemperature,
        observed_load: ObservedCellLoad,
    ) -> CellTemperature {
        let rate = observed_load.publishes_per_second;

        match current {
            CellTemperature::Cold => {
                if rate >= self.thermal_hysteresis.warm_to_hot_publishes_per_second {
                    CellTemperature::Hot
                } else if rate >= self.thermal_hysteresis.cold_to_warm_publishes_per_second {
                    CellTemperature::Warm
                } else {
                    CellTemperature::Cold
                }
            }
            CellTemperature::Warm => {
                if rate >= self.thermal_hysteresis.warm_to_hot_publishes_per_second {
                    CellTemperature::Hot
                } else if rate <= self.thermal_hysteresis.warm_to_cold_publishes_per_second {
                    CellTemperature::Cold
                } else {
                    CellTemperature::Warm
                }
            }
            CellTemperature::Hot => {
                if rate <= self.thermal_hysteresis.hot_to_warm_publishes_per_second {
                    CellTemperature::Warm
                } else {
                    CellTemperature::Hot
                }
            }
        }
    }

    fn target_steward_count(&self, temperature: CellTemperature) -> usize {
        match temperature {
            CellTemperature::Cold => self.cold_stewards,
            CellTemperature::Warm => self.warm_stewards,
            CellTemperature::Hot => self.hot_stewards,
        }
    }

    /// Plan an incremental steward-set transition subject to the rebalance budget.
    pub fn plan_rebalance(
        &self,
        subject_partition: &SubjectPattern,
        candidates: &[StewardCandidate],
        current_stewards: &[NodeId],
        current_temperature: CellTemperature,
        observed_load: ObservedCellLoad,
    ) -> Result<RebalancePlan, FabricError> {
        let next_temperature = self.recommend_temperature(current_temperature, observed_load);
        let canonical_partition = self.normalization.normalize(subject_partition)?;
        let desired_stewards =
            self.select_stewards(&canonical_partition, candidates, next_temperature)?;
        let next_stewards = self.advance_toward_desired(
            current_stewards,
            &desired_stewards,
            self.target_steward_count(next_temperature),
        );

        let added_stewards = next_stewards
            .iter()
            .filter(|node| !contains_node(current_stewards, node))
            .cloned()
            .collect();
        let removed_stewards = current_stewards
            .iter()
            .filter(|node| !contains_node(&next_stewards, node))
            .cloned()
            .collect();

        Ok(RebalancePlan {
            next_temperature,
            next_stewards,
            added_stewards,
            removed_stewards,
        })
    }

    fn candidate_pool<'a>(
        &self,
        subject_partition: &SubjectPattern,
        candidates: &'a [StewardCandidate],
        temperature: CellTemperature,
    ) -> Result<Vec<&'a StewardCandidate>, FabricError> {
        let eligible: Vec<&StewardCandidate> = candidates
            .iter()
            .filter(|candidate| candidate.is_steward_eligible())
            .collect();
        if eligible.is_empty() {
            return Err(FabricError::NoStewardCandidates {
                partition: subject_partition.clone(),
            });
        }

        let required = self
            .candidate_pool_size
            .max(self.target_steward_count(temperature))
            .min(eligible.len());

        let mut ring = HashRing::new(self.vnodes_per_node.max(1));
        let mut by_node = BTreeMap::new();
        for candidate in &eligible {
            let key = candidate.node_id.as_str().to_string();
            ring.add_node(key.clone());
            by_node.insert(key, *candidate);
        }

        let subject_key = subject_partition.canonical_key();
        let mut pool = Vec::new();
        let mut seen = BTreeSet::new();
        for salt in 0_u64.. {
            if pool.len() >= required || seen.len() >= eligible.len() {
                break;
            }
            let lookup = (&subject_key, salt);
            let Some(node_id) = ring.node_for_key(&lookup) else {
                break;
            };
            if !seen.insert(node_id.to_string()) {
                continue;
            }
            if let Some(candidate) = by_node.get(node_id) {
                pool.push(*candidate);
            }
        }

        Ok(pool)
    }

    fn select_stewards(
        &self,
        subject_partition: &SubjectPattern,
        candidates: &[StewardCandidate],
        temperature: CellTemperature,
    ) -> Result<Vec<NodeId>, FabricError> {
        let pool = self.candidate_pool(subject_partition, candidates, temperature)?;
        let target = self.target_steward_count(temperature).min(pool.len());
        if target == 0 {
            return Err(FabricError::NoStewardCandidates {
                partition: subject_partition.clone(),
            });
        }

        let mut preferred: Vec<&StewardCandidate> = pool
            .iter()
            .copied()
            .filter(|candidate| candidate.latency_millis <= self.max_latency_millis)
            .collect();
        let mut fallback: Vec<&StewardCandidate> = pool
            .iter()
            .copied()
            .filter(|candidate| candidate.latency_millis > self.max_latency_millis)
            .collect();

        preferred.sort_by(|left, right| compare_candidates(left, right, temperature));
        fallback.sort_by(|left, right| compare_candidates(left, right, temperature));
        preferred.extend(fallback);

        let mut selected = Vec::with_capacity(target);
        let mut selected_ids = BTreeSet::new();
        let mut used_domains = BTreeSet::new();

        for candidate in &preferred {
            if selected.len() >= target {
                break;
            }
            if !used_domains.insert(candidate.failure_domain.clone()) {
                continue;
            }
            selected_ids.insert(candidate.node_id.as_str().to_string());
            selected.push(candidate.node_id.clone());
        }

        for candidate in preferred {
            if selected.len() >= target {
                break;
            }
            if !selected_ids.insert(candidate.node_id.as_str().to_string()) {
                continue;
            }
            selected.push(candidate.node_id.clone());
        }

        Ok(selected)
    }

    fn advance_toward_desired(
        &self,
        current_stewards: &[NodeId],
        desired_stewards: &[NodeId],
        target_len: usize,
    ) -> Vec<NodeId> {
        let desired_ids = desired_stewards
            .iter()
            .map(NodeId::as_str)
            .collect::<BTreeSet<_>>();
        let mut remaining_budget = self.rebalance_budget.max_steward_changes;
        let mut next = current_stewards.to_vec();

        while next.len() > target_len && remaining_budget > 0 {
            let remove_index = next
                .iter()
                .rposition(|node| !desired_ids.contains(node.as_str()))
                .unwrap_or_else(|| next.len().saturating_sub(1));
            next.remove(remove_index);
            remaining_budget = remaining_budget.saturating_sub(1);
        }

        for desired in desired_stewards {
            if contains_node(&next, desired) {
                continue;
            }

            if next.len() < target_len {
                if remaining_budget == 0 {
                    break;
                }
                next.push(desired.clone());
                remaining_budget = remaining_budget.saturating_sub(1);
                continue;
            }

            let Some(remove_index) = next
                .iter()
                .rposition(|node| !desired_ids.contains(node.as_str()))
            else {
                continue;
            };
            if remaining_budget < 2 {
                break;
            }
            next.remove(remove_index);
            remaining_budget = remaining_budget.saturating_sub(1);
            next.push(desired.clone());
            remaining_budget = remaining_budget.saturating_sub(1);
        }

        next
    }
}

/// Minimal control-plane state owned by a subject cell.
///
/// This is intentionally slim: a future `ControlCapsuleV1` bead will replace
/// the placeholder fields with the actual replicated control log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCapsule {
    /// Full steward pool negotiated for the current cell epoch.
    pub steward_pool: Vec<NodeId>,
    /// Steward currently holding the append lease, if any.
    pub active_sequencer: Option<NodeId>,
    /// Lease generation fencing stale sequencer authority.
    pub sequencer_lease_generation: u64,
    /// Monotonic revision of the policy snapshot stored in the capsule.
    pub policy_revision: u64,
}

impl ControlCapsule {
    fn new(steward_pool: Vec<NodeId>, epoch: CellEpoch) -> Self {
        Self {
            active_sequencer: steward_pool.first().cloned(),
            steward_pool,
            sequencer_lease_generation: epoch.generation,
            policy_revision: 1,
        }
    }
}

/// Minimal data-plane configuration owned by a subject cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataCapsule {
    /// Current traffic temperature of the cell.
    pub temperature: CellTemperature,
    /// Number of recent message blocks retained inline by the cell.
    pub retained_message_blocks: usize,
}

impl Default for DataCapsule {
    fn default() -> Self {
        Self {
            temperature: CellTemperature::Cold,
            retained_message_blocks: 1,
        }
    }
}

/// Repair and recoverability policy for a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairPolicy {
    /// Minimum recoverability class the cell should preserve during churn.
    pub recoverability_target: u8,
    /// Number of repair witnesses to keep for cold cells.
    pub cold_witnesses: usize,
    /// Number of repair witnesses to keep for hot cells.
    pub hot_witnesses: usize,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self {
            recoverability_target: 2,
            cold_witnesses: 1,
            hot_witnesses: 3,
        }
    }
}

/// Smallest sovereign unit of the brokerless subject fabric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectCell {
    /// Deterministic identifier for this canonical subject partition and epoch.
    pub cell_id: CellId,
    /// Canonical non-overlapping subject slice owned by this cell.
    pub subject_partition: SubjectPattern,
    /// Active steward set selected for the current temperature and epoch.
    pub steward_set: Vec<NodeId>,
    /// Current control-plane capsule placeholder for the cell.
    pub control_capsule: ControlCapsule,
    /// Current data-plane capsule placeholder for the cell.
    pub data_capsule: DataCapsule,
    /// Repair and recoverability policy attached to the cell.
    pub repair_policy: RepairPolicy,
    /// Membership epoch and generation fenced into the cell identity.
    pub epoch: CellEpoch,
}

impl SubjectCell {
    /// Create a new subject cell with deterministic placement.
    pub fn new(
        subject_partition: &SubjectPattern,
        epoch: CellEpoch,
        candidates: &[StewardCandidate],
        placement_policy: &PlacementPolicy,
        repair_policy: RepairPolicy,
        data_capsule: DataCapsule,
    ) -> Result<Self, FabricError> {
        let canonical_partition = placement_policy
            .normalization
            .normalize(subject_partition)?;
        let steward_set = placement_policy.select_stewards(
            &canonical_partition,
            candidates,
            data_capsule.temperature,
        )?;
        let control_capsule = ControlCapsule::new(steward_set.clone(), epoch);
        let cell_id = CellId::for_partition(epoch, &canonical_partition);

        Ok(Self {
            cell_id,
            subject_partition: canonical_partition,
            steward_set,
            control_capsule,
            data_capsule,
            repair_policy,
            epoch,
        })
    }
}

/// Errors produced by foundational fabric modeling and placement.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FabricError {
    /// Two canonical subject partitions still overlap after normalization.
    #[error("subject partitions `{left}` and `{right}` overlap")]
    OverlappingSubjectPartitions {
        /// Left partition in the conflicting pair.
        left: SubjectPattern,
        /// Right partition in the conflicting pair.
        right: SubjectPattern,
    },
    /// No steward-eligible nodes were available for the requested partition.
    #[error("no steward-eligible candidates available for partition `{partition}`")]
    NoStewardCandidates {
        /// Canonical partition that could not be placed.
        partition: SubjectPattern,
    },
    /// Multiple distinct morphisms claimed the same subject and disagreed on the result.
    #[error("subject `{subject}` matched multiple canonical morphisms (`{left}` and `{right}`)")]
    ConflictingSubjectMorphisms {
        /// Original subject presented to the normalization pipeline.
        subject: SubjectPattern,
        /// First canonical candidate produced by a matching morphism.
        left: SubjectPattern,
        /// Conflicting canonical candidate produced by another morphism.
        right: SubjectPattern,
    },
    /// Prefix morphisms cycled instead of converging on one canonical partition.
    #[error("subject `{subject}` entered a morphism cycle at `{cycle_point}`")]
    CyclicSubjectMorphisms {
        /// Original subject presented to the normalization pipeline.
        subject: SubjectPattern,
        /// Canonical subject that repeated while chasing morphisms.
        cycle_point: SubjectPattern,
    },
}

fn stable_hash<T: Hash>(value: T) -> u64 {
    let mut hasher = DetHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

fn compare_candidates(
    left: &StewardCandidate,
    right: &StewardCandidate,
    temperature: CellTemperature,
) -> std::cmp::Ordering {
    candidate_score(right, temperature)
        .cmp(&candidate_score(left, temperature))
        .then_with(|| left.latency_millis.cmp(&right.latency_millis))
        .then_with(|| left.failure_domain.cmp(&right.failure_domain))
        .then_with(|| left.node_id.as_str().cmp(right.node_id.as_str()))
}

fn candidate_score(candidate: &StewardCandidate, temperature: CellTemperature) -> u64 {
    let health_score = match candidate.health {
        StewardHealth::Healthy => 400_u64,
        StewardHealth::Degraded => 250,
        StewardHealth::Draining => 100,
        StewardHealth::Unavailable => 0,
    };
    let storage_score = match candidate.storage_class {
        StorageClass::Ephemeral => 40_u64,
        StorageClass::Standard => 80,
        StorageClass::Durable => 120,
    };
    // Only an explicit RepairWitness role differentiates extra repair capacity
    // beyond ordinary stewardship during hot-cell placement.
    let hot_repair_bonus = if matches!(temperature, CellTemperature::Hot)
        && candidate.roles.contains(&NodeRole::RepairWitness)
    {
        40_u64
    } else {
        0
    };
    let latency_credit = 1_000_u64.saturating_sub(u64::from(candidate.latency_millis));

    health_score + storage_score + hot_repair_bonus + latency_credit
}

fn contains_node(nodes: &[NodeId], candidate: &NodeId) -> bool {
    nodes.iter().any(|node| node == candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::run_test_with_cx;
    use std::time::Duration;

    fn candidate(
        name: &str,
        domain: &str,
        storage_class: StorageClass,
        latency_millis: u32,
    ) -> StewardCandidate {
        StewardCandidate::new(NodeId::new(name), domain)
            .with_role(NodeRole::Steward)
            .with_role(NodeRole::RepairWitness)
            .with_storage_class(storage_class)
            .with_latency_millis(latency_millis)
    }

    #[test]
    fn stream_config_defaults_to_ephemeral_interactive() {
        let config = FabricStreamConfig::default();
        assert_eq!(config.delivery_class, DeliveryClass::EphemeralInteractive);
        assert_eq!(config.capture_policy, CapturePolicy::ExplicitOptIn);
        assert!(config.subjects.is_empty());
    }

    #[test]
    fn stream_config_rejects_empty_subject_lists() {
        let err = FabricStreamConfig::default()
            .validate()
            .expect_err("empty stream declarations must fail closed");
        assert_eq!(err.kind(), ErrorKind::ConfigError);
    }

    #[test]
    fn stream_config_rejects_overlapping_subjects() {
        let config = FabricStreamConfig {
            subjects: vec![
                SubjectPattern::parse("orders.>").expect("orders wildcard"),
                SubjectPattern::parse("orders.created").expect("orders literal"),
            ],
            ..FabricStreamConfig::default()
        };

        let err = config
            .validate()
            .expect_err("overlapping capture declarations must be rejected");
        assert_eq!(err.kind(), ErrorKind::User);
    }

    #[test]
    fn connect_rejects_blank_endpoints() {
        run_test_with_cx(|cx| async move {
            let err = Fabric::connect(&cx, "   ")
                .await
                .expect_err("blank endpoint must fail");
            assert_eq!(err.kind(), ErrorKind::ConfigError);
        });
    }

    #[test]
    fn publish_and_subscribe_round_trip_with_ephemeral_defaults() {
        run_test_with_cx(|cx| async move {
            let fabric = Fabric::connect(&cx, "node1:4222").await.expect("connect");
            let mut subscription = fabric.subscribe(&cx, "orders.>").await.expect("subscribe");

            let receipt = fabric
                .publish(&cx, "orders.created", b"payload".to_vec())
                .await
                .expect("publish");
            let message = subscription.next(&cx).await.expect("message");

            assert_eq!(receipt.ack_kind, AckKind::Accepted);
            assert_eq!(receipt.delivery_class, DeliveryClass::EphemeralInteractive);
            assert_eq!(message.delivery_class, DeliveryClass::EphemeralInteractive);
            assert_eq!(message.subject.as_str(), "orders.created");
            assert_eq!(message.payload, b"payload".to_vec());
        });
    }

    #[test]
    fn request_uses_same_surface_and_returns_reply() {
        run_test_with_cx(|cx| async move {
            let fabric = Fabric::connect(&cx, "node1:4222").await.expect("connect");
            let reply = fabric
                .request(&cx, "service.lookup", b"lookup".to_vec())
                .await
                .expect("request");

            assert_eq!(reply.ack_kind, AckKind::Accepted);
            assert_eq!(reply.delivery_class, DeliveryClass::EphemeralInteractive);
            assert_eq!(reply.subject.as_str(), "service.lookup");
            assert_eq!(reply.payload, b"lookup".to_vec());
        });
    }

    #[test]
    fn stream_accepts_explicit_subjects_and_preserves_endpoint() {
        run_test_with_cx(|cx| async move {
            let fabric = Fabric::connect(&cx, "node1:4222").await.expect("connect");
            let handle = fabric
                .stream(
                    &cx,
                    FabricStreamConfig {
                        subjects: vec![SubjectPattern::parse("orders.>").expect("pattern")],
                        delivery_class: DeliveryClass::DurableOrdered,
                        capture_policy: CapturePolicy::ExplicitOptIn,
                        request_timeout: Some(Duration::from_secs(5)),
                    },
                )
                .await
                .expect("stream");

            assert_eq!(handle.endpoint(), "node1:4222");
            assert_eq!(
                handle.config().delivery_class,
                DeliveryClass::DurableOrdered
            );
            assert_eq!(handle.config().subjects.len(), 1);
        });
    }

    #[test]
    fn parse_subject_pattern_trims_outer_whitespace() {
        let pattern = SubjectPattern::parse("  orders.created.>  ").expect("pattern");
        assert_eq!(pattern.canonical_key(), "orders.created.>");
    }

    #[test]
    fn parse_subject_pattern_rejects_non_terminal_tail_wildcard() {
        let err = SubjectPattern::parse("orders.>.created").expect_err("should reject");
        assert_eq!(err, SubjectPatternError::TailWildcardMustBeTerminal);
    }

    #[test]
    fn reply_space_aggregation_compacts_ephemeral_suffixes() {
        let pattern =
            SubjectPattern::parse("_INBOX.orders.region.instance.12345").expect("pattern");
        let compacted = pattern.aggregate_reply_space(ReplySpaceCompactionPolicy {
            enabled: true,
            preserve_segments: 3,
        });
        assert_eq!(compacted.canonical_key(), "_INBOX.orders.region.>");
    }

    #[test]
    fn overlap_detection_handles_literals_and_wildcards() {
        let left = SubjectPattern::parse("orders.*").expect("left");
        let right = SubjectPattern::parse("orders.created").expect("right");
        let third = SubjectPattern::parse("metrics.>").expect("third");
        let fourth = SubjectPattern::parse("orders.created").expect("fourth");

        assert!(left.overlaps(&right));
        assert!(!left.overlaps(&third));
        assert!(third.overlaps(&SubjectPattern::parse("metrics.region.1").expect("tail")));
        assert!(right.overlaps(&fourth));
    }

    #[test]
    fn tail_wildcard_requires_a_non_empty_suffix() {
        let wildcard = SubjectPattern::parse("orders.>").expect("wildcard");
        let bare_prefix = SubjectPattern::parse("orders").expect("bare prefix");

        assert!(!wildcard.overlaps(&bare_prefix));
        assert!(wildcard.overlaps(&SubjectPattern::parse("orders.created").expect("expanded")));
    }

    #[test]
    fn normalization_policy_applies_prefix_morphisms() {
        let policy = NormalizationPolicy {
            morphisms: vec![SubjectPrefixMorphism::new("svc.orders", "orders").expect("morphism")],
            reply_space_policy: ReplySpaceCompactionPolicy {
                enabled: true,
                preserve_segments: 3,
            },
        };

        let canonical = policy
            .normalize(&SubjectPattern::parse("svc.orders.created").expect("pattern"))
            .expect("normalized");

        assert_eq!(canonical.canonical_key(), "orders.created");
    }

    #[test]
    fn normalization_policy_chains_prefix_morphisms() {
        let policy = NormalizationPolicy {
            morphisms: vec![
                SubjectPrefixMorphism::new("svc.orders", "orders").expect("morphism"),
                SubjectPrefixMorphism::new("orders", "canonical.orders").expect("morphism"),
            ],
            reply_space_policy: ReplySpaceCompactionPolicy::default(),
        };

        let canonical = policy
            .normalize(&SubjectPattern::parse("svc.orders.created").expect("pattern"))
            .expect("normalized");

        assert_eq!(canonical.canonical_key(), "canonical.orders.created");
    }

    #[test]
    fn normalization_policy_rejects_morphism_cycles() {
        let policy = NormalizationPolicy {
            morphisms: vec![
                SubjectPrefixMorphism::new("svc.orders", "orders").expect("morphism"),
                SubjectPrefixMorphism::new("orders", "svc.orders").expect("morphism"),
            ],
            reply_space_policy: ReplySpaceCompactionPolicy::default(),
        };

        let err = policy
            .normalize(&SubjectPattern::parse("svc.orders.created").expect("pattern"))
            .expect_err("should reject cycle");

        assert!(matches!(err, FabricError::CyclicSubjectMorphisms { .. }));
    }

    #[test]
    fn normalization_policy_can_compact_reply_space_after_morphism() {
        let policy = NormalizationPolicy {
            morphisms: vec![SubjectPrefixMorphism::new("svc", "_INBOX").expect("morphism")],
            reply_space_policy: ReplySpaceCompactionPolicy {
                enabled: true,
                preserve_segments: 3,
            },
        };

        let canonical = policy
            .normalize(&SubjectPattern::parse("svc.orders.region.instance.123").expect("pattern"))
            .expect("normalized");

        assert_eq!(canonical.canonical_key(), "_INBOX.orders.region.>");
    }

    #[test]
    fn non_overlapping_validation_rejects_conflicts() {
        let patterns = vec![
            SubjectPattern::parse("orders.created").expect("orders.created"),
            SubjectPattern::parse("orders.*").expect("orders.*"),
        ];
        let err = SubjectPattern::validate_non_overlapping(&patterns).expect_err("should overlap");
        assert!(matches!(
            err,
            FabricError::OverlappingSubjectPartitions { .. }
        ));
    }

    #[test]
    fn cell_id_is_stable_for_same_partition_and_epoch() {
        let partition = SubjectPattern::parse("orders.created").expect("pattern");
        let epoch = CellEpoch::new(7, 3);
        let first = CellId::for_partition(epoch, &partition);
        let second = CellId::for_partition(epoch, &partition);

        assert_eq!(first, second);
        assert_ne!(
            first,
            CellId::for_partition(CellEpoch::new(8, 3), &partition)
        );
    }

    #[test]
    fn alias_subjects_collapse_to_the_same_subject_cell() {
        let policy = PlacementPolicy {
            normalization: NormalizationPolicy {
                morphisms: vec![
                    SubjectPrefixMorphism::new("svc.orders", "orders").expect("morphism"),
                ],
                ..NormalizationPolicy::default()
            },
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 5),
            candidate("node-b", "rack-b", StorageClass::Standard, 7),
            candidate("node-c", "rack-c", StorageClass::Standard, 9),
        ];
        let epoch = CellEpoch::new(17, 4);

        let canonical = SubjectCell::new(
            &SubjectPattern::parse("orders.created").expect("canonical"),
            epoch,
            &candidates,
            &policy,
            RepairPolicy::default(),
            DataCapsule::default(),
        )
        .expect("canonical cell");
        let aliased = SubjectCell::new(
            &SubjectPattern::parse("svc.orders.created").expect("aliased"),
            epoch,
            &candidates,
            &policy,
            RepairPolicy::default(),
            DataCapsule::default(),
        )
        .expect("aliased cell");

        assert_eq!(canonical.subject_partition, aliased.subject_partition);
        assert_eq!(canonical.cell_id, aliased.cell_id);
        assert_eq!(canonical.steward_set, aliased.steward_set);
    }

    #[test]
    fn thermal_hysteresis_damps_temperature_flips() {
        let policy = PlacementPolicy::default();

        assert_eq!(
            policy.recommend_temperature(CellTemperature::Warm, ObservedCellLoad::new(64)),
            CellTemperature::Warm
        );
        assert_eq!(
            policy.recommend_temperature(CellTemperature::Warm, ObservedCellLoad::new(32)),
            CellTemperature::Cold
        );
        assert_eq!(
            policy.recommend_temperature(CellTemperature::Hot, ObservedCellLoad::new(768)),
            CellTemperature::Hot
        );
        assert_eq!(
            policy.recommend_temperature(CellTemperature::Hot, ObservedCellLoad::new(256)),
            CellTemperature::Warm
        );
    }

    #[test]
    fn rebalance_budget_limits_steward_churn() {
        let partition = SubjectPattern::parse("orders.created").expect("pattern");
        let policy = PlacementPolicy {
            cold_stewards: 1,
            warm_stewards: 2,
            hot_stewards: 3,
            candidate_pool_size: 5,
            rebalance_budget: RebalanceBudget {
                max_steward_changes: 1,
            },
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 5),
            candidate("node-b", "rack-b", StorageClass::Durable, 6),
            candidate("node-c", "rack-c", StorageClass::Standard, 7),
        ];
        let current_stewards = vec![NodeId::new("node-a")];

        let plan = policy
            .plan_rebalance(
                &partition,
                &candidates,
                &current_stewards,
                CellTemperature::Cold,
                ObservedCellLoad::new(2_048),
            )
            .expect("rebalance");

        assert_eq!(plan.next_temperature, CellTemperature::Hot);
        assert_eq!(plan.added_stewards.len(), 1);
        assert!(plan.removed_stewards.is_empty());
        assert_eq!(plan.next_stewards.len(), 2);
        assert!(
            plan.next_stewards
                .iter()
                .any(|node| node.as_str() == "node-a")
        );
    }

    #[test]
    fn rebalance_planning_uses_normalized_subject_partition() {
        let policy = PlacementPolicy {
            cold_stewards: 1,
            warm_stewards: 1,
            hot_stewards: 1,
            candidate_pool_size: 4,
            normalization: NormalizationPolicy {
                morphisms: vec![
                    SubjectPrefixMorphism::new("svc.orders", "orders").expect("morphism"),
                ],
                ..NormalizationPolicy::default()
            },
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 5),
            candidate("node-b", "rack-b", StorageClass::Durable, 6),
            candidate("node-c", "rack-c", StorageClass::Standard, 7),
            candidate("node-d", "rack-d", StorageClass::Standard, 8),
            candidate("node-e", "rack-e", StorageClass::Standard, 9),
        ];
        let alias_subjects = [
            "svc.orders.created",
            "svc.orders.updated",
            "svc.orders.cancelled",
            "svc.orders.fulfilled",
            "svc.orders.archived",
            "svc.orders.audit",
            "svc.orders.retry",
            "svc.orders.snapshot",
        ];

        let (aliased, current_stewards) = alias_subjects
            .iter()
            .find_map(|raw| {
                let aliased = SubjectPattern::parse(raw).expect("pattern");
                let canonical = policy.normalization.normalize(&aliased).expect("canonical");
                let raw_stewards = policy
                    .select_stewards(&aliased, &candidates, CellTemperature::Warm)
                    .expect("raw placement");
                let canonical_stewards = policy
                    .select_stewards(&canonical, &candidates, CellTemperature::Warm)
                    .expect("canonical placement");

                (raw_stewards != canonical_stewards).then_some((aliased, canonical_stewards))
            })
            .expect("expected at least one alias subject to hash differently before normalization");

        let plan = policy
            .plan_rebalance(
                &aliased,
                &candidates,
                &current_stewards,
                CellTemperature::Warm,
                ObservedCellLoad::new(256),
            )
            .expect("rebalance");

        assert_eq!(plan.next_stewards, current_stewards);
        assert!(plan.added_stewards.is_empty());
        assert!(plan.removed_stewards.is_empty());
    }

    #[test]
    fn placement_is_deterministic_and_filters_ineligible_nodes() {
        let partition = SubjectPattern::parse("orders.created").expect("pattern");
        let policy = PlacementPolicy {
            cold_stewards: 2,
            warm_stewards: 2,
            hot_stewards: 2,
            candidate_pool_size: 4,
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 8),
            candidate("node-b", "rack-b", StorageClass::Standard, 12),
            StewardCandidate::new(NodeId::new("observer"), "rack-c")
                .with_role(NodeRole::Subscriber)
                .with_health(StewardHealth::Healthy),
        ];

        let first = policy
            .select_stewards(&partition, &candidates, CellTemperature::Warm)
            .expect("placement");
        let second = policy
            .select_stewards(&partition, &candidates, CellTemperature::Warm)
            .expect("placement");

        assert_eq!(first, second);
        assert!(first.iter().all(|node| node.as_str() != "observer"));
    }

    #[test]
    fn hot_cells_widen_steward_set() {
        let partition = SubjectPattern::parse("orders.created").expect("pattern");
        let policy = PlacementPolicy {
            cold_stewards: 1,
            warm_stewards: 2,
            hot_stewards: 3,
            candidate_pool_size: 5,
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 5),
            candidate("node-b", "rack-b", StorageClass::Durable, 6),
            candidate("node-c", "rack-c", StorageClass::Standard, 7),
        ];

        let cold = policy
            .select_stewards(&partition, &candidates, CellTemperature::Cold)
            .expect("cold");
        let hot = policy
            .select_stewards(&partition, &candidates, CellTemperature::Hot)
            .expect("hot");

        assert_eq!(cold.len(), 1);
        assert_eq!(hot.len(), 3);
    }

    #[test]
    fn placement_prefers_failure_domain_diversity() {
        let partition = SubjectPattern::parse("orders.created").expect("pattern");
        let policy = PlacementPolicy {
            cold_stewards: 2,
            warm_stewards: 2,
            hot_stewards: 2,
            candidate_pool_size: 4,
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 5),
            candidate("node-b", "rack-a", StorageClass::Durable, 5),
            candidate("node-c", "rack-b", StorageClass::Standard, 6),
            candidate("node-d", "rack-c", StorageClass::Standard, 7),
        ];

        let selected = policy
            .select_stewards(&partition, &candidates, CellTemperature::Warm)
            .expect("selected");
        assert_eq!(selected.len(), 2);
        assert!(selected.iter().any(|node| node.as_str() == "node-a"));
        assert!(
            selected
                .iter()
                .any(|node| node.as_str() == "node-c" || node.as_str() == "node-d")
        );
    }

    #[test]
    fn placement_falls_back_to_high_latency_candidates_to_fill_steward_set() {
        let partition = SubjectPattern::parse("orders.created").expect("pattern");
        let policy = PlacementPolicy {
            cold_stewards: 3,
            warm_stewards: 3,
            hot_stewards: 3,
            candidate_pool_size: 3,
            max_latency_millis: 20,
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 5),
            candidate("node-b", "rack-b", StorageClass::Standard, 7),
            candidate("node-c", "rack-c", StorageClass::Standard, 250),
        ];

        let selected = policy
            .select_stewards(&partition, &candidates, CellTemperature::Warm)
            .expect("selected");

        assert_eq!(selected.len(), 3);
        assert!(selected.iter().any(|node| node.as_str() == "node-c"));
    }

    #[test]
    fn hot_placement_prefers_explicit_repair_witness_capacity() {
        let partition = SubjectPattern::parse("orders.created").expect("pattern");
        let policy = PlacementPolicy {
            cold_stewards: 1,
            warm_stewards: 1,
            hot_stewards: 1,
            candidate_pool_size: 2,
            max_latency_millis: 20,
            ..PlacementPolicy::default()
        };
        let candidates = vec![
            StewardCandidate::new(NodeId::new("node-a"), "rack-a")
                .with_role(NodeRole::Steward)
                .with_storage_class(StorageClass::Standard)
                .with_latency_millis(5),
            StewardCandidate::new(NodeId::new("node-b"), "rack-b")
                .with_role(NodeRole::Steward)
                .with_role(NodeRole::RepairWitness)
                .with_storage_class(StorageClass::Standard)
                .with_latency_millis(5),
        ];

        let warm = policy
            .select_stewards(&partition, &candidates, CellTemperature::Warm)
            .expect("warm");
        let hot = policy
            .select_stewards(&partition, &candidates, CellTemperature::Hot)
            .expect("hot");

        assert_eq!(warm, vec![NodeId::new("node-a")]);
        assert_eq!(hot, vec![NodeId::new("node-b")]);
    }

    #[test]
    fn subject_cell_construction_builds_capsules_and_compacts_reply_space() {
        let subject_partition =
            SubjectPattern::parse("_INBOX.orders.region.instance.123").expect("pattern");
        let policy = PlacementPolicy {
            cold_stewards: 2,
            warm_stewards: 2,
            hot_stewards: 3,
            candidate_pool_size: 4,
            normalization: NormalizationPolicy {
                morphisms: Vec::new(),
                reply_space_policy: ReplySpaceCompactionPolicy {
                    enabled: true,
                    preserve_segments: 3,
                },
            },
            ..PlacementPolicy::default()
        };
        let data_capsule = DataCapsule {
            temperature: CellTemperature::Warm,
            retained_message_blocks: 4,
        };
        let candidates = vec![
            candidate("node-a", "rack-a", StorageClass::Durable, 5),
            candidate("node-b", "rack-b", StorageClass::Standard, 6),
            candidate("node-c", "rack-c", StorageClass::Standard, 7),
        ];

        let cell = SubjectCell::new(
            &subject_partition,
            CellEpoch::new(11, 2),
            &candidates,
            &policy,
            RepairPolicy::default(),
            data_capsule,
        )
        .expect("cell");

        assert_eq!(
            cell.subject_partition.canonical_key(),
            "_INBOX.orders.region.>"
        );
        assert_eq!(
            cell.control_capsule.active_sequencer,
            cell.steward_set.first().cloned()
        );
        assert_eq!(cell.steward_set.len(), 2);
    }
}
