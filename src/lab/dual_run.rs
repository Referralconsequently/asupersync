//! Dual-run scenario types for lab-vs-live differential testing.
//!
//! This module implements the shared seed plumbing and replay metadata
//! types defined by the `DualRunScenarioSpec` contract
//! (`docs/lab_live_scenario_adapter_contract.md`).
//!
//! # Seed Flow
//!
//! ```text
//! DualRunScenarioSpec.seed_plan
//!     ├─→ Lab adapter: SeedPlan → LabConfig (inherit or override)
//!     └─→ Live adapter: SeedPlan → live runner seed (inherit or override)
//!
//! SeedPlan.canonical_seed + scenario_id → deterministic execution
//! SeedPlan.seed_lineage_id → artifact traceability
//! ```
//!
//! # Scenario Identity
//!
//! The system distinguishes two layers of identity:
//!
//! - **Scenario family**: the stable adversarial case (e.g., "cancel during
//!   two-phase send") — survives shrinking, promotion, and reruns.
//! - **Execution instance**: one concrete run of a family (seed + config
//!   snapshot) — unique per execution.
//!
//! This separation lets reruns, shrink steps, and regression promotion
//! carry the family identity cleanly while tracking which specific
//! execution produced evidence.
//!
//! # Replay Metadata
//!
//! [`ReplayMetadata`] captures both identity layers plus enough provenance
//! to rerun or explain a mismatch. It is emitted into normalized
//! observables and mismatch bundles.

use crate::lab::config::LabConfig;
use crate::test_logging::{derive_component_seed, derive_scenario_seed};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ============================================================================
// Seed Mode and Replay Policy
// ============================================================================

/// How an adapter derives its effective seed from the canonical seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedMode {
    /// Use `canonical_seed` directly (or derived via `derive_scenario_seed`).
    Inherit,
    /// The adapter provides its own seed, overriding the canonical one.
    /// The override value is stored in `SeedPlan::lab_seed_override` or
    /// `SeedPlan::live_seed_override`.
    Override,
}

/// Replay strategy for seed-based reproducibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayPolicy {
    /// Run with exactly one seed. Simplest and most common.
    SingleSeed,
    /// Sweep a range of seeds derived from the canonical seed.
    /// Used for schedule exploration.
    SeedSweep,
    /// Replay from a previously captured trace bundle.
    /// Seed is informational; the trace dictates scheduling.
    ReplayBundle,
}

// ============================================================================
// Seed Plan
// ============================================================================

/// Deterministic seed plan for dual-run scenario execution.
///
/// This is the single source of truth for how both lab and live adapters
/// obtain their seeds. It enforces the contract rule: "The live adapter
/// may not silently pick a different seed than the lab adapter."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedPlan {
    /// Stable seed chosen by the scenario author.
    pub canonical_seed: u64,

    /// Stable token emitted into mismatch artifacts and repro commands.
    /// Typically the scenario_id or a human-readable lineage tag.
    pub seed_lineage_id: String,

    /// How the lab adapter derives its effective seed.
    pub lab_seed_mode: SeedMode,

    /// How the live adapter derives its effective seed.
    pub live_seed_mode: SeedMode,

    /// Replay strategy.
    pub replay_policy: ReplayPolicy,

    /// Explicit lab seed override (only used when `lab_seed_mode == Override`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lab_seed_override: Option<u64>,

    /// Explicit live seed override (only used when `live_seed_mode == Override`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_seed_override: Option<u64>,

    /// Optional entropy seed override. When `None`, entropy derives from
    /// the effective seed via `derive_component_seed(seed, "entropy")`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entropy_seed_override: Option<u64>,
}

impl SeedPlan {
    /// Create a simple seed plan that inherits the canonical seed on both sides.
    #[must_use]
    pub fn inherit(canonical_seed: u64, lineage_id: impl Into<String>) -> Self {
        Self {
            canonical_seed,
            seed_lineage_id: lineage_id.into(),
            lab_seed_mode: SeedMode::Inherit,
            live_seed_mode: SeedMode::Inherit,
            replay_policy: ReplayPolicy::SingleSeed,
            lab_seed_override: None,
            live_seed_override: None,
            entropy_seed_override: None,
        }
    }

    /// Compute the effective seed for the lab adapter.
    #[must_use]
    pub fn effective_lab_seed(&self) -> u64 {
        match self.lab_seed_mode {
            SeedMode::Inherit => self.canonical_seed,
            SeedMode::Override => self.lab_seed_override.unwrap_or(self.canonical_seed),
        }
    }

    /// Compute the effective seed for the live adapter.
    #[must_use]
    pub fn effective_live_seed(&self) -> u64 {
        match self.live_seed_mode {
            SeedMode::Inherit => self.canonical_seed,
            SeedMode::Override => self.live_seed_override.unwrap_or(self.canonical_seed),
        }
    }

    /// Compute the effective entropy seed for an adapter.
    /// Uses the explicit override if set, otherwise derives from the
    /// given effective seed.
    #[must_use]
    pub fn effective_entropy_seed(&self, effective_seed: u64) -> u64 {
        self.entropy_seed_override
            .unwrap_or_else(|| derive_component_seed(effective_seed, "entropy"))
    }

    /// Build a [`LabConfig`] from this seed plan.
    ///
    /// Sets `seed` and `entropy_seed` according to the plan's lab mode.
    #[must_use]
    pub fn to_lab_config(&self) -> LabConfig {
        let seed = self.effective_lab_seed();
        let entropy = self.effective_entropy_seed(seed);
        LabConfig::new(seed).entropy_seed(entropy)
    }

    /// Generate seeds for a sweep of `count` derived seeds.
    ///
    /// Each seed is deterministically derived from the canonical seed
    /// using `derive_scenario_seed` with a sweep index tag.
    /// Only meaningful when `replay_policy == SeedSweep`.
    #[must_use]
    pub fn sweep_seeds(&self, count: usize) -> Vec<u64> {
        (0..count)
            .map(|i| {
                let tag = format!("sweep:{i}");
                derive_scenario_seed(self.canonical_seed, &tag)
            })
            .collect()
    }

    /// Set lab seed mode to override with the given seed.
    #[must_use]
    pub fn with_lab_override(mut self, seed: u64) -> Self {
        self.lab_seed_mode = SeedMode::Override;
        self.lab_seed_override = Some(seed);
        self
    }

    /// Set live seed mode to override with the given seed.
    #[must_use]
    pub fn with_live_override(mut self, seed: u64) -> Self {
        self.live_seed_mode = SeedMode::Override;
        self.live_seed_override = Some(seed);
        self
    }

    /// Set the replay policy.
    #[must_use]
    pub fn with_replay_policy(mut self, policy: ReplayPolicy) -> Self {
        self.replay_policy = policy;
        self
    }

    /// Set an explicit entropy seed override for both adapters.
    #[must_use]
    pub fn with_entropy_seed(mut self, seed: u64) -> Self {
        self.entropy_seed_override = Some(seed);
        self
    }
}

impl fmt::Display for SeedPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SeedPlan(canonical=0x{:X}, lineage={}, lab={:?}, live={:?}, policy={:?})",
            self.canonical_seed,
            self.seed_lineage_id,
            self.lab_seed_mode,
            self.live_seed_mode,
            self.replay_policy,
        )
    }
}

// ============================================================================
// Scenario Identity
// ============================================================================

/// Stable identifier for a scenario family.
///
/// A family represents the abstract adversarial case independent of any
/// particular execution. The same family survives shrinking, promotion
/// into regression suites, and reruns with different seeds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScenarioFamilyId {
    /// Primary stable identifier (e.g., `"phase1.cancel.race.one_loser"`).
    pub id: String,
    /// Semantic surface being exercised (e.g., `"cancellation.race"`).
    pub surface_id: String,
    /// Versioned comparator contract for this surface.
    pub surface_contract_version: String,
}

impl ScenarioFamilyId {
    /// Create a new scenario family identifier.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        surface_id: impl Into<String>,
        contract_version: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            surface_id: surface_id.into(),
            surface_contract_version: contract_version.into(),
        }
    }
}

impl fmt::Display for ScenarioFamilyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{}({})",
            self.id, self.surface_id, self.surface_contract_version
        )
    }
}

/// Unique identifier for a specific execution of a scenario family.
///
/// Combines the family identity with the concrete seed and a monotonic
/// run counter. Two executions of the same family with different seeds
/// produce different instance IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecutionInstanceId {
    /// Which scenario family this execution belongs to.
    pub family_id: String,
    /// Effective seed used for this execution.
    pub effective_seed: u64,
    /// Runtime kind that produced this instance.
    pub runtime_kind: RuntimeKind,
    /// Monotonic run index within a sweep (0 for single-seed runs).
    pub run_index: u32,
}

impl ExecutionInstanceId {
    /// Create a new execution instance ID for a single-seed lab run.
    #[must_use]
    pub fn lab(family_id: impl Into<String>, seed: u64) -> Self {
        Self {
            family_id: family_id.into(),
            effective_seed: seed,
            runtime_kind: RuntimeKind::Lab,
            run_index: 0,
        }
    }

    /// Create a new execution instance ID for a single-seed live run.
    #[must_use]
    pub fn live(family_id: impl Into<String>, seed: u64) -> Self {
        Self {
            family_id: family_id.into(),
            effective_seed: seed,
            runtime_kind: RuntimeKind::Live,
            run_index: 0,
        }
    }

    /// Set the run index (for sweep runs).
    #[must_use]
    pub fn with_run_index(mut self, index: u32) -> Self {
        self.run_index = index;
        self
    }

    /// Produce a stable string key for this instance.
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{}:{}:0x{:X}:{}",
            self.family_id, self.runtime_kind, self.effective_seed, self.run_index
        )
    }
}

impl fmt::Display for ExecutionInstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}@0x{:X}#{}]",
            self.family_id, self.runtime_kind, self.effective_seed, self.run_index
        )
    }
}

/// Which runtime produced an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    /// Deterministic lab runtime (`LabRuntime`).
    Lab,
    /// Live runtime (`RuntimeBuilder::current_thread()` for Phase 1).
    Live,
}

impl fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lab => write!(f, "lab"),
            Self::Live => write!(f, "live"),
        }
    }
}

// ============================================================================
// Replay Metadata
// ============================================================================

/// Replay and provenance metadata for a single execution.
///
/// Captures everything needed to rerun or explain a mismatch:
/// family identity (what scenario?), instance identity (which run?),
/// effective seeds, trace evidence, and repro commands.
///
/// This maps to the `provenance` section of the normalized observable
/// schema (`lab-live-normalized-observable-v1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMetadata {
    /// Scenario family identity.
    pub family: ScenarioFamilyId,

    /// Execution instance identity.
    pub instance: ExecutionInstanceId,

    /// Seed plan that produced this execution.
    pub seed_plan: SeedPlan,

    /// Effective seed actually used by the adapter.
    pub effective_seed: u64,

    /// Effective entropy seed actually used.
    pub effective_entropy_seed: u64,

    /// Trace fingerprint from lab execution (Foata/Mazurkiewicz class).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_fingerprint: Option<u64>,

    /// Schedule hash from lab execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_hash: Option<u64>,

    /// Event hash from lab execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<u64>,

    /// Total events observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_count: Option<u64>,

    /// Total scheduler steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps_total: Option<u64>,

    /// Path to artifact bundle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,

    /// Direct deterministic rerun command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repro_command: Option<String>,

    /// Hash of the config used for this execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_hash: Option<String>,
}

impl ReplayMetadata {
    /// Create replay metadata for a lab execution from a seed plan.
    #[must_use]
    pub fn for_lab(family: ScenarioFamilyId, seed_plan: &SeedPlan) -> Self {
        let effective_seed = seed_plan.effective_lab_seed();
        let effective_entropy_seed = seed_plan.effective_entropy_seed(effective_seed);
        let instance = ExecutionInstanceId::lab(&family.id, effective_seed);

        Self {
            family,
            instance,
            seed_plan: seed_plan.clone(),
            effective_seed,
            effective_entropy_seed,
            trace_fingerprint: None,
            schedule_hash: None,
            event_hash: None,
            event_count: None,
            steps_total: None,
            artifact_path: None,
            repro_command: None,
            config_hash: None,
        }
    }

    /// Create replay metadata for a live execution from a seed plan.
    #[must_use]
    pub fn for_live(family: ScenarioFamilyId, seed_plan: &SeedPlan) -> Self {
        let effective_seed = seed_plan.effective_live_seed();
        let effective_entropy_seed = seed_plan.effective_entropy_seed(effective_seed);
        let instance = ExecutionInstanceId::live(&family.id, effective_seed);

        Self {
            family,
            instance,
            seed_plan: seed_plan.clone(),
            effective_seed,
            effective_entropy_seed,
            trace_fingerprint: None,
            schedule_hash: None,
            event_hash: None,
            event_count: None,
            steps_total: None,
            artifact_path: None,
            repro_command: None,
            config_hash: None,
        }
    }

    /// Update from a `LabRunReport`'s trace certificate.
    #[must_use]
    pub fn with_lab_report(
        mut self,
        trace_fingerprint: u64,
        event_hash: u64,
        event_count: u64,
        schedule_hash: u64,
        steps_total: u64,
    ) -> Self {
        self.trace_fingerprint = Some(trace_fingerprint);
        self.event_hash = Some(event_hash);
        self.event_count = Some(event_count);
        self.schedule_hash = Some(schedule_hash);
        self.steps_total = Some(steps_total);
        self
    }

    /// Set the repro command.
    #[must_use]
    pub fn with_repro_command(mut self, cmd: impl Into<String>) -> Self {
        self.repro_command = Some(cmd.into());
        self
    }

    /// Set the artifact path.
    #[must_use]
    pub fn with_artifact_path(mut self, path: impl Into<String>) -> Self {
        self.artifact_path = Some(path.into());
        self
    }

    /// Generate a default repro command for this execution.
    #[must_use]
    pub fn default_repro_command(&self) -> String {
        format!(
            "ASUPERSYNC_SEED=0x{:X} cargo test {} -- --nocapture",
            self.effective_seed, self.family.id
        )
    }
}

// ============================================================================
// Seed Lineage Record
// ============================================================================

/// Complete record of seeds used across a dual-run pair.
///
/// Emitted into mismatch bundles and summary records so that every
/// seed decision is auditable. Satisfies the contract requirement:
/// "Seed rewrites must be explicit in `seed_plan`, never hidden."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedLineageRecord {
    /// Seed lineage identifier from the plan.
    pub seed_lineage_id: String,

    /// Canonical seed from the plan.
    pub canonical_seed: u64,

    /// Effective lab seed actually used.
    pub lab_effective_seed: u64,

    /// Effective live seed actually used.
    pub live_effective_seed: u64,

    /// Lab seed mode.
    pub lab_seed_mode: SeedMode,

    /// Live seed mode.
    pub live_seed_mode: SeedMode,

    /// Effective lab entropy seed.
    pub lab_entropy_seed: u64,

    /// Effective live entropy seed.
    pub live_entropy_seed: u64,

    /// Replay policy used.
    pub replay_policy: ReplayPolicy,

    /// Whether lab and live used the same effective seed.
    pub seeds_match: bool,

    /// Additional audit annotations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl SeedLineageRecord {
    /// Build a lineage record from a seed plan.
    #[must_use]
    pub fn from_plan(plan: &SeedPlan) -> Self {
        let lab_seed = plan.effective_lab_seed();
        let live_seed = plan.effective_live_seed();
        let lab_entropy = plan.effective_entropy_seed(lab_seed);
        let live_entropy = plan.effective_entropy_seed(live_seed);

        Self {
            seed_lineage_id: plan.seed_lineage_id.clone(),
            canonical_seed: plan.canonical_seed,
            lab_effective_seed: lab_seed,
            live_effective_seed: live_seed,
            lab_seed_mode: plan.lab_seed_mode,
            live_seed_mode: plan.live_seed_mode,
            lab_entropy_seed: lab_entropy,
            live_entropy_seed: live_entropy,
            replay_policy: plan.replay_policy,
            seeds_match: lab_seed == live_seed,
            annotations: BTreeMap::new(),
        }
    }

    /// Add an audit annotation.
    #[must_use]
    pub fn with_annotation(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }
}

// ============================================================================
// Dual-Run Scenario Spec (partial — shared seed/replay fields only)
// ============================================================================

/// Schema version for the dual-run scenario spec.
pub const DUAL_RUN_SCHEMA_VERSION: &str = "lab-live-scenario-spec-v1";

/// Rollout phase for a dual-run scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Phase 1: cancellation, combinators, channels, obligations, region
    /// close, sync primitives. Current-thread live runner only.
    #[serde(rename = "Phase 1")]
    Phase1,
    /// Phase 2: timers, virtualized transport.
    #[serde(rename = "Phase 2")]
    Phase2,
    /// Phase 3: actor/supervision, HTTP/gRPC on captured boundaries.
    #[serde(rename = "Phase 3")]
    Phase3,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Phase1 => write!(f, "Phase 1"),
            Self::Phase2 => write!(f, "Phase 2"),
            Self::Phase3 => write!(f, "Phase 3"),
        }
    }
}

/// Core identity and seed fields of a `DualRunScenarioSpec`.
///
/// This struct captures the seed-plan-aware subset of the full
/// `DualRunScenarioSpec` contract. The full contract includes
/// participants, operations, perturbations, expectations, and bindings
/// which are built by downstream beads (`asupersync-2a6k9.2.4`+).
///
/// This bead (`asupersync-2a6k9.2.3`) makes seeds, parameters, and
/// replay metadata first-class across both execution paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualRunScenarioIdentity {
    /// Stable contract discriminator.
    pub schema_version: String,

    /// Stable case identifier reused across lab and live.
    pub scenario_id: String,

    /// Semantic surface being exercised.
    pub surface_id: String,

    /// Versioned comparator contract.
    pub surface_contract_version: String,

    /// Human-readable scenario meaning.
    pub description: String,

    /// Rollout phase from the scope matrix.
    pub phase: Phase,

    /// Deterministic seed and rerun lineage.
    pub seed_plan: SeedPlan,

    /// Ownership, tags, bead lineage.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl DualRunScenarioIdentity {
    /// Create a Phase 1 scenario identity with inherited seeds.
    #[must_use]
    pub fn phase1(
        scenario_id: impl Into<String>,
        surface_id: impl Into<String>,
        contract_version: impl Into<String>,
        description: impl Into<String>,
        canonical_seed: u64,
    ) -> Self {
        let sid = scenario_id.into();
        Self {
            schema_version: DUAL_RUN_SCHEMA_VERSION.to_string(),
            scenario_id: sid.clone(),
            surface_id: surface_id.into(),
            surface_contract_version: contract_version.into(),
            description: description.into(),
            phase: Phase::Phase1,
            seed_plan: SeedPlan::inherit(canonical_seed, sid),
            metadata: BTreeMap::new(),
        }
    }

    /// Extract the scenario family identity.
    #[must_use]
    pub fn family_id(&self) -> ScenarioFamilyId {
        ScenarioFamilyId::new(
            &self.scenario_id,
            &self.surface_id,
            &self.surface_contract_version,
        )
    }

    /// Build lab replay metadata from this identity.
    #[must_use]
    pub fn lab_replay_metadata(&self) -> ReplayMetadata {
        ReplayMetadata::for_lab(self.family_id(), &self.seed_plan)
    }

    /// Build live replay metadata from this identity.
    #[must_use]
    pub fn live_replay_metadata(&self) -> ReplayMetadata {
        ReplayMetadata::for_live(self.family_id(), &self.seed_plan)
    }

    /// Build a seed lineage record for audit.
    #[must_use]
    pub fn seed_lineage(&self) -> SeedLineageRecord {
        SeedLineageRecord::from_plan(&self.seed_plan)
    }

    /// Build a `LabConfig` from this identity's seed plan.
    #[must_use]
    pub fn to_lab_config(&self) -> LabConfig {
        self.seed_plan.to_lab_config()
    }

    /// Set a metadata annotation.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Override the seed plan.
    #[must_use]
    pub fn with_seed_plan(mut self, plan: SeedPlan) -> Self {
        self.seed_plan = plan;
        self
    }
}

// ============================================================================
// Normalized Observable Schema (lab-live-normalized-observable-v1)
// ============================================================================

/// Schema version for normalized observables.
pub const NORMALIZED_OBSERVABLE_SCHEMA_VERSION: &str = "lab-live-normalized-observable-v1";

/// Outcome class for the terminal result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeClass {
    Ok,
    Err,
    Cancelled,
    Panicked,
}

impl fmt::Display for OutcomeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Err => write!(f, "err"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Panicked => write!(f, "panicked"),
        }
    }
}

/// Terminal phase of the cancellation protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelTerminalPhase {
    NotCancelled,
    CancelRequested,
    Cancelling,
    Finalizing,
    Completed,
}

/// Loser drain status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrainStatus {
    NotApplicable,
    Complete,
    Incomplete,
}

/// Region close state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionState {
    Open,
    Closing,
    Draining,
    Finalizing,
    Closed,
}

/// Comparison tolerance for resource counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterTolerance {
    Exact,
    AtLeast,
    AtMost,
    Unsupported,
}

/// Terminal outcome subrecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalOutcome {
    pub class: OutcomeClass,
    pub severity: OutcomeClass,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_reason_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panic_class: Option<String>,
}

impl TerminalOutcome {
    /// Create an Ok terminal outcome.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            class: OutcomeClass::Ok,
            severity: OutcomeClass::Ok,
            surface_result: None,
            error_class: None,
            cancel_reason_class: None,
            panic_class: None,
        }
    }

    /// Create a Cancelled terminal outcome.
    #[must_use]
    pub fn cancelled(reason_class: impl Into<String>) -> Self {
        Self {
            class: OutcomeClass::Cancelled,
            severity: OutcomeClass::Cancelled,
            surface_result: None,
            error_class: None,
            cancel_reason_class: Some(reason_class.into()),
            panic_class: None,
        }
    }

    /// Create an Err terminal outcome.
    #[must_use]
    pub fn err(error_class: impl Into<String>) -> Self {
        Self {
            class: OutcomeClass::Err,
            severity: OutcomeClass::Err,
            surface_result: None,
            error_class: Some(error_class.into()),
            cancel_reason_class: None,
            panic_class: None,
        }
    }
}

/// Cancellation subrecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancellationRecord {
    pub requested: bool,
    pub acknowledged: bool,
    pub cleanup_completed: bool,
    pub finalization_completed: bool,
    pub terminal_phase: CancelTerminalPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_observed: Option<bool>,
}

impl CancellationRecord {
    /// No cancellation occurred.
    #[must_use]
    pub fn none() -> Self {
        Self {
            requested: false,
            acknowledged: false,
            cleanup_completed: false,
            finalization_completed: false,
            terminal_phase: CancelTerminalPhase::NotCancelled,
            checkpoint_observed: None,
        }
    }

    /// Full cancellation protocol completed.
    #[must_use]
    pub fn completed() -> Self {
        Self {
            requested: true,
            acknowledged: true,
            cleanup_completed: true,
            finalization_completed: true,
            terminal_phase: CancelTerminalPhase::Completed,
            checkpoint_observed: Some(true),
        }
    }
}

/// Loser drain subrecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoserDrainRecord {
    pub applicable: bool,
    pub expected_losers: u32,
    pub drained_losers: u32,
    pub status: DrainStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

impl LoserDrainRecord {
    /// No loser drain applicable.
    #[must_use]
    pub fn not_applicable() -> Self {
        Self {
            applicable: false,
            expected_losers: 0,
            drained_losers: 0,
            status: DrainStatus::NotApplicable,
            evidence: None,
        }
    }

    /// All losers drained.
    #[must_use]
    pub fn complete(expected: u32) -> Self {
        Self {
            applicable: true,
            expected_losers: expected,
            drained_losers: expected,
            status: DrainStatus::Complete,
            evidence: None,
        }
    }
}

/// Region close subrecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionCloseRecord {
    pub root_state: RegionState,
    pub quiescent: bool,
    pub live_children: u32,
    pub finalizers_pending: u32,
    pub close_completed: bool,
}

impl RegionCloseRecord {
    /// Region closed to quiescence.
    #[must_use]
    pub fn quiescent() -> Self {
        Self {
            root_state: RegionState::Closed,
            quiescent: true,
            live_children: 0,
            finalizers_pending: 0,
            close_completed: true,
        }
    }
}

/// Obligation balance subrecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationBalanceRecord {
    pub reserved: u32,
    pub committed: u32,
    pub aborted: u32,
    pub leaked: u32,
    pub unresolved: u32,
    pub balanced: bool,
}

impl ObligationBalanceRecord {
    /// Fully balanced (no leaks, no unresolved).
    #[must_use]
    pub fn balanced(reserved: u32, committed: u32, aborted: u32) -> Self {
        Self {
            reserved,
            committed,
            aborted,
            leaked: 0,
            unresolved: 0,
            balanced: true,
        }
    }

    /// Zero obligations.
    #[must_use]
    pub fn zero() -> Self {
        Self::balanced(0, 0, 0)
    }

    /// Recompute `balanced` and `unresolved` from the other fields.
    #[must_use]
    pub fn recompute(mut self) -> Self {
        let terminal = self.committed + self.aborted + self.leaked;
        self.unresolved = self.reserved.saturating_sub(terminal);
        self.balanced = self.leaked == 0 && self.unresolved == 0;
        self
    }
}

/// Resource surface subrecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSurfaceRecord {
    pub contract_scope: String,
    #[serde(default)]
    pub counters: BTreeMap<String, i64>,
    #[serde(default)]
    pub tolerances: BTreeMap<String, CounterTolerance>,
}

impl ResourceSurfaceRecord {
    /// Create a resource surface with no counters.
    #[must_use]
    pub fn empty(scope: impl Into<String>) -> Self {
        Self {
            contract_scope: scope.into(),
            counters: BTreeMap::new(),
            tolerances: BTreeMap::new(),
        }
    }

    /// Add an exact counter.
    #[must_use]
    pub fn with_counter(mut self, name: impl Into<String>, value: i64) -> Self {
        let n = name.into();
        self.counters.insert(n.clone(), value);
        self.tolerances.insert(n, CounterTolerance::Exact);
        self
    }

    /// Add a counter with a specific tolerance.
    #[must_use]
    pub fn with_counter_tolerance(
        mut self,
        name: impl Into<String>,
        value: i64,
        tolerance: CounterTolerance,
    ) -> Self {
        let n = name.into();
        self.counters.insert(n.clone(), value);
        self.tolerances.insert(n, tolerance);
        self
    }
}

/// Semantic section of a normalized observable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedSemantics {
    pub terminal_outcome: TerminalOutcome,
    pub cancellation: CancellationRecord,
    pub loser_drain: LoserDrainRecord,
    pub region_close: RegionCloseRecord,
    pub obligation_balance: ObligationBalanceRecord,
    pub resource_surface: ResourceSurfaceRecord,
}

/// Complete normalized observable record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedObservable {
    pub schema_version: String,
    pub scenario_id: String,
    pub surface_id: String,
    pub surface_contract_version: String,
    pub runtime_kind: RuntimeKind,
    pub semantics: NormalizedSemantics,
    pub provenance: ReplayMetadata,
}

impl NormalizedObservable {
    /// Create a normalized observable from identity and semantics.
    #[must_use]
    pub fn new(
        identity: &DualRunScenarioIdentity,
        runtime_kind: RuntimeKind,
        semantics: NormalizedSemantics,
        provenance: ReplayMetadata,
    ) -> Self {
        Self {
            schema_version: NORMALIZED_OBSERVABLE_SCHEMA_VERSION.to_string(),
            scenario_id: identity.scenario_id.clone(),
            surface_id: identity.surface_id.clone(),
            surface_contract_version: identity.surface_contract_version.clone(),
            runtime_kind,
            semantics,
            provenance,
        }
    }
}

// ============================================================================
// Witness / Assertion Helpers
// ============================================================================

/// A single mismatch between lab and live observables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMismatch {
    /// Dot-separated path to the mismatched field.
    pub field: String,
    /// Description of the mismatch.
    pub description: String,
    /// Lab-side value (display representation).
    pub lab_value: String,
    /// Live-side value (display representation).
    pub live_value: String,
}

impl fmt::Display for SemanticMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} (lab={}, live={})",
            self.field, self.description, self.lab_value, self.live_value
        )
    }
}

/// Result of comparing two normalized observables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonVerdict {
    /// Scenario identity.
    pub scenario_id: String,
    /// Surface identity.
    pub surface_id: String,
    /// Whether the comparison passed (no semantic mismatches).
    pub passed: bool,
    /// Semantic mismatches found.
    pub mismatches: Vec<SemanticMismatch>,
    /// Seed lineage record for audit.
    pub seed_lineage: SeedLineageRecord,
}

impl ComparisonVerdict {
    /// Whether the verdict indicates semantic equivalence.
    #[must_use]
    pub fn is_equivalent(&self) -> bool {
        self.passed
    }

    /// Format a human-readable summary.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.passed {
            format!(
                "PASS: {} on {} (seed lineage: {})",
                self.scenario_id, self.surface_id, self.seed_lineage.seed_lineage_id
            )
        } else {
            let mismatch_list: Vec<String> =
                self.mismatches.iter().map(ToString::to_string).collect();
            format!(
                "FAIL: {} on {} — {} mismatch(es):\n  {}",
                self.scenario_id,
                self.surface_id,
                self.mismatches.len(),
                mismatch_list.join("\n  ")
            )
        }
    }
}

impl fmt::Display for ComparisonVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

/// Compare two normalized observables and produce a verdict.
///
/// Compares all semantic fields. Provenance is recorded but not compared
/// (audit-only by default).
#[must_use]
pub fn compare_observables(
    lab: &NormalizedObservable,
    live: &NormalizedObservable,
    seed_lineage: SeedLineageRecord,
) -> ComparisonVerdict {
    let mut mismatches = Vec::new();

    // Schema version
    if lab.schema_version != live.schema_version {
        mismatches.push(SemanticMismatch {
            field: "schema_version".to_string(),
            description: "Schema version mismatch".to_string(),
            lab_value: lab.schema_version.clone(),
            live_value: live.schema_version.clone(),
        });
    }

    // Scenario identity
    if lab.scenario_id != live.scenario_id {
        mismatches.push(SemanticMismatch {
            field: "scenario_id".to_string(),
            description: "Scenario ID mismatch".to_string(),
            lab_value: lab.scenario_id.clone(),
            live_value: live.scenario_id.clone(),
        });
    }

    // Terminal outcome
    compare_terminal_outcome(
        &lab.semantics.terminal_outcome,
        &live.semantics.terminal_outcome,
        &mut mismatches,
    );

    // Cancellation
    compare_cancellation(
        &lab.semantics.cancellation,
        &live.semantics.cancellation,
        &mut mismatches,
    );

    // Loser drain
    compare_loser_drain(
        &lab.semantics.loser_drain,
        &live.semantics.loser_drain,
        &mut mismatches,
    );

    // Region close
    compare_region_close(
        &lab.semantics.region_close,
        &live.semantics.region_close,
        &mut mismatches,
    );

    // Obligation balance
    compare_obligation_balance(
        &lab.semantics.obligation_balance,
        &live.semantics.obligation_balance,
        &mut mismatches,
    );

    // Resource surface
    compare_resource_surface(
        &lab.semantics.resource_surface,
        &live.semantics.resource_surface,
        &mut mismatches,
    );

    ComparisonVerdict {
        scenario_id: lab.scenario_id.clone(),
        surface_id: lab.surface_id.clone(),
        passed: mismatches.is_empty(),
        mismatches,
        seed_lineage,
    }
}

fn compare_terminal_outcome(
    lab: &TerminalOutcome,
    live: &TerminalOutcome,
    mismatches: &mut Vec<SemanticMismatch>,
) {
    if lab.class != live.class {
        mismatches.push(SemanticMismatch {
            field: "semantics.terminal_outcome.class".to_string(),
            description: "Terminal outcome class mismatch".to_string(),
            lab_value: format!("{}", lab.class),
            live_value: format!("{}", live.class),
        });
    }
    if lab.severity != live.severity {
        mismatches.push(SemanticMismatch {
            field: "semantics.terminal_outcome.severity".to_string(),
            description: "Terminal outcome severity mismatch".to_string(),
            lab_value: format!("{}", lab.severity),
            live_value: format!("{}", live.severity),
        });
    }
    if lab.surface_result != live.surface_result {
        mismatches.push(SemanticMismatch {
            field: "semantics.terminal_outcome.surface_result".to_string(),
            description: "Surface result mismatch".to_string(),
            lab_value: format!("{:?}", lab.surface_result),
            live_value: format!("{:?}", live.surface_result),
        });
    }
    if lab.error_class != live.error_class {
        mismatches.push(SemanticMismatch {
            field: "semantics.terminal_outcome.error_class".to_string(),
            description: "Error class mismatch".to_string(),
            lab_value: format!("{:?}", lab.error_class),
            live_value: format!("{:?}", live.error_class),
        });
    }
}

fn compare_cancellation(
    lab: &CancellationRecord,
    live: &CancellationRecord,
    mismatches: &mut Vec<SemanticMismatch>,
) {
    let fields = [
        ("requested", lab.requested, live.requested),
        ("acknowledged", lab.acknowledged, live.acknowledged),
        (
            "cleanup_completed",
            lab.cleanup_completed,
            live.cleanup_completed,
        ),
        (
            "finalization_completed",
            lab.finalization_completed,
            live.finalization_completed,
        ),
    ];
    for (name, lab_val, live_val) in fields {
        if lab_val != live_val {
            mismatches.push(SemanticMismatch {
                field: format!("semantics.cancellation.{name}"),
                description: format!("Cancellation {name} mismatch"),
                lab_value: format!("{lab_val}"),
                live_value: format!("{live_val}"),
            });
        }
    }
    if lab.terminal_phase != live.terminal_phase {
        mismatches.push(SemanticMismatch {
            field: "semantics.cancellation.terminal_phase".to_string(),
            description: "Cancellation terminal phase mismatch".to_string(),
            lab_value: format!("{:?}", lab.terminal_phase),
            live_value: format!("{:?}", live.terminal_phase),
        });
    }
    // checkpoint_observed: only compare if both sides report it
    if let (Some(lab_cp), Some(live_cp)) = (lab.checkpoint_observed, live.checkpoint_observed) {
        if lab_cp != live_cp {
            mismatches.push(SemanticMismatch {
                field: "semantics.cancellation.checkpoint_observed".to_string(),
                description: "Checkpoint observed mismatch".to_string(),
                lab_value: format!("{lab_cp}"),
                live_value: format!("{live_cp}"),
            });
        }
    }
}

fn compare_loser_drain(
    lab: &LoserDrainRecord,
    live: &LoserDrainRecord,
    mismatches: &mut Vec<SemanticMismatch>,
) {
    if lab.status != live.status {
        mismatches.push(SemanticMismatch {
            field: "semantics.loser_drain.status".to_string(),
            description: "Loser drain status mismatch".to_string(),
            lab_value: format!("{:?}", lab.status),
            live_value: format!("{:?}", live.status),
        });
    }
    if lab.applicable != live.applicable {
        mismatches.push(SemanticMismatch {
            field: "semantics.loser_drain.applicable".to_string(),
            description: "Loser drain applicability mismatch".to_string(),
            lab_value: format!("{}", lab.applicable),
            live_value: format!("{}", live.applicable),
        });
    }
    if lab.expected_losers != live.expected_losers {
        mismatches.push(SemanticMismatch {
            field: "semantics.loser_drain.expected_losers".to_string(),
            description: "Expected losers count mismatch".to_string(),
            lab_value: format!("{}", lab.expected_losers),
            live_value: format!("{}", live.expected_losers),
        });
    }
    if lab.drained_losers != live.drained_losers {
        mismatches.push(SemanticMismatch {
            field: "semantics.loser_drain.drained_losers".to_string(),
            description: "Drained losers count mismatch".to_string(),
            lab_value: format!("{}", lab.drained_losers),
            live_value: format!("{}", live.drained_losers),
        });
    }
}

fn compare_region_close(
    lab: &RegionCloseRecord,
    live: &RegionCloseRecord,
    mismatches: &mut Vec<SemanticMismatch>,
) {
    if lab.root_state != live.root_state {
        mismatches.push(SemanticMismatch {
            field: "semantics.region_close.root_state".to_string(),
            description: "Region root state mismatch".to_string(),
            lab_value: format!("{:?}", lab.root_state),
            live_value: format!("{:?}", live.root_state),
        });
    }
    if lab.quiescent != live.quiescent {
        mismatches.push(SemanticMismatch {
            field: "semantics.region_close.quiescent".to_string(),
            description: "Region quiescence mismatch".to_string(),
            lab_value: format!("{}", lab.quiescent),
            live_value: format!("{}", live.quiescent),
        });
    }
    if lab.close_completed != live.close_completed {
        mismatches.push(SemanticMismatch {
            field: "semantics.region_close.close_completed".to_string(),
            description: "Region close completed mismatch".to_string(),
            lab_value: format!("{}", lab.close_completed),
            live_value: format!("{}", live.close_completed),
        });
    }
}

fn compare_obligation_balance(
    lab: &ObligationBalanceRecord,
    live: &ObligationBalanceRecord,
    mismatches: &mut Vec<SemanticMismatch>,
) {
    if lab.balanced != live.balanced {
        mismatches.push(SemanticMismatch {
            field: "semantics.obligation_balance.balanced".to_string(),
            description: "Obligation balance mismatch".to_string(),
            lab_value: format!("{}", lab.balanced),
            live_value: format!("{}", live.balanced),
        });
    }
    if lab.leaked != live.leaked {
        mismatches.push(SemanticMismatch {
            field: "semantics.obligation_balance.leaked".to_string(),
            description: "Leaked obligation count mismatch".to_string(),
            lab_value: format!("{}", lab.leaked),
            live_value: format!("{}", live.leaked),
        });
    }
    if lab.unresolved != live.unresolved {
        mismatches.push(SemanticMismatch {
            field: "semantics.obligation_balance.unresolved".to_string(),
            description: "Unresolved obligation count mismatch".to_string(),
            lab_value: format!("{}", lab.unresolved),
            live_value: format!("{}", live.unresolved),
        });
    }
}

fn compare_resource_surface(
    lab: &ResourceSurfaceRecord,
    live: &ResourceSurfaceRecord,
    mismatches: &mut Vec<SemanticMismatch>,
) {
    if lab.contract_scope != live.contract_scope {
        mismatches.push(SemanticMismatch {
            field: "semantics.resource_surface.contract_scope".to_string(),
            description: "Resource surface contract scope mismatch".to_string(),
            lab_value: lab.contract_scope.clone(),
            live_value: live.contract_scope.clone(),
        });
        return; // No point comparing counters if scopes differ.
    }

    // Compare counters using declared tolerances.
    for (name, &lab_val) in &lab.counters {
        let live_val = live.counters.get(name).copied().unwrap_or(0);
        let tolerance = lab
            .tolerances
            .get(name)
            .copied()
            .unwrap_or(CounterTolerance::Exact);

        let mismatch = match tolerance {
            CounterTolerance::Exact => lab_val != live_val,
            CounterTolerance::AtLeast => live_val < lab_val,
            CounterTolerance::AtMost => live_val > lab_val,
            CounterTolerance::Unsupported => false,
        };

        if mismatch {
            mismatches.push(SemanticMismatch {
                field: format!("semantics.resource_surface.counters.{name}"),
                description: format!("Counter '{name}' mismatch (tolerance: {tolerance:?})"),
                lab_value: format!("{lab_val}"),
                live_value: format!("{live_val}"),
            });
        }
    }

    // Check for counters in live but not in lab.
    for name in live.counters.keys() {
        if !lab.counters.contains_key(name) {
            let live_val = live.counters[name];
            mismatches.push(SemanticMismatch {
                field: format!("semantics.resource_surface.counters.{name}"),
                description: format!("Counter '{name}' present in live but not in lab"),
                lab_value: "absent".to_string(),
                live_value: format!("{live_val}"),
            });
        }
    }
}

// ============================================================================
// Assertion Helpers
// ============================================================================

/// Assert that a normalized observable satisfies the core Asupersync
/// invariants: no obligation leaks, region closed to quiescence, and
/// losers drained (if applicable).
///
/// Returns a list of invariant violations (empty if all pass).
#[must_use]
pub fn check_core_invariants(obs: &NormalizedObservable) -> Vec<String> {
    let mut violations = Vec::new();

    // Obligation balance
    if !obs.semantics.obligation_balance.balanced {
        violations.push(format!(
            "Obligation balance: leaked={}, unresolved={}",
            obs.semantics.obligation_balance.leaked, obs.semantics.obligation_balance.unresolved
        ));
    }

    // Region quiescence
    if !obs.semantics.region_close.quiescent {
        violations.push(format!(
            "Region not quiescent: state={:?}, live_children={}, finalizers_pending={}",
            obs.semantics.region_close.root_state,
            obs.semantics.region_close.live_children,
            obs.semantics.region_close.finalizers_pending
        ));
    }

    // Loser drain
    if obs.semantics.loser_drain.applicable
        && obs.semantics.loser_drain.status == DrainStatus::Incomplete
    {
        violations.push(format!(
            "Incomplete loser drain: expected={}, drained={}",
            obs.semantics.loser_drain.expected_losers, obs.semantics.loser_drain.drained_losers
        ));
    }

    // Cancellation protocol completion
    if obs.semantics.cancellation.requested && !obs.semantics.cancellation.cleanup_completed {
        violations.push(format!(
            "Cancellation cleanup incomplete: phase={:?}",
            obs.semantics.cancellation.terminal_phase
        ));
    }

    violations
}

/// Assert a normalized observable against expected semantics.
///
/// Returns mismatches between actual and expected values.
#[must_use]
pub fn assert_semantics(
    actual: &NormalizedSemantics,
    expected: &NormalizedSemantics,
) -> Vec<SemanticMismatch> {
    // Build temporary observables just for comparison.
    let lab = NormalizedObservable {
        schema_version: NORMALIZED_OBSERVABLE_SCHEMA_VERSION.to_string(),
        scenario_id: String::new(),
        surface_id: String::new(),
        surface_contract_version: String::new(),
        runtime_kind: RuntimeKind::Lab,
        semantics: expected.clone(),
        provenance: ReplayMetadata::for_lab(
            ScenarioFamilyId::new("", "", ""),
            &SeedPlan::inherit(0, ""),
        ),
    };
    let live = NormalizedObservable {
        schema_version: NORMALIZED_OBSERVABLE_SCHEMA_VERSION.to_string(),
        scenario_id: String::new(),
        surface_id: String::new(),
        surface_contract_version: String::new(),
        runtime_kind: RuntimeKind::Live,
        semantics: actual.clone(),
        provenance: ReplayMetadata::for_live(
            ScenarioFamilyId::new("", "", ""),
            &SeedPlan::inherit(0, ""),
        ),
    };

    let verdict = compare_observables(
        &lab,
        &live,
        SeedLineageRecord::from_plan(&SeedPlan::inherit(0, "")),
    );
    verdict.mismatches
}

// ============================================================================
// Dual-Run Harness Entrypoint
// ============================================================================

/// Result of a dual-run harness execution.
#[derive(Debug, Clone)]
pub struct DualRunResult {
    /// Lab-side normalized observable.
    pub lab: NormalizedObservable,
    /// Live-side normalized observable.
    pub live: NormalizedObservable,
    /// Comparison verdict.
    pub verdict: ComparisonVerdict,
    /// Core invariant violations for the lab run.
    pub lab_invariant_violations: Vec<String>,
    /// Core invariant violations for the live run.
    pub live_invariant_violations: Vec<String>,
    /// Seed lineage record.
    pub seed_lineage: SeedLineageRecord,
}

impl DualRunResult {
    /// Whether the dual-run passed: no semantic mismatches and no invariant
    /// violations on either side.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.verdict.passed
            && self.lab_invariant_violations.is_empty()
            && self.live_invariant_violations.is_empty()
    }

    /// Formatted summary of the result.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![self.verdict.summary()];
        if !self.lab_invariant_violations.is_empty() {
            parts.push(format!(
                "Lab invariant violations: {}",
                self.lab_invariant_violations.join("; ")
            ));
        }
        if !self.live_invariant_violations.is_empty() {
            parts.push(format!(
                "Live invariant violations: {}",
                self.live_invariant_violations.join("; ")
            ));
        }
        parts.join("\n")
    }
}

impl fmt::Display for DualRunResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

/// Builder for dual-run differential test harnesses.
///
/// # Usage
///
/// ```ignore
/// let result = DualRunHarness::phase1(
///     "cancel.race.one_loser",
///     "cancellation.race",
///     "v1",
///     "Race two tasks, cancel loser, verify drain",
///     42,
/// )
/// .lab(|config| {
///     let mut lab = LabRuntime::new(config);
///     // ... run scenario ...
///     make_happy_semantics()
/// })
/// .live(|seed, entropy_seed| {
///     // ... run scenario on current-thread runtime ...
///     make_happy_semantics()
/// })
/// .run();
///
/// assert!(result.passed());
/// ```
pub struct DualRunHarness {
    identity: DualRunScenarioIdentity,
    lab_fn: Option<Box<dyn FnOnce(LabConfig) -> NormalizedSemantics>>,
    live_fn: Option<Box<dyn FnOnce(u64, u64) -> NormalizedSemantics>>,
}

impl DualRunHarness {
    /// Create a Phase 1 harness builder.
    #[must_use]
    pub fn phase1(
        scenario_id: impl Into<String>,
        surface_id: impl Into<String>,
        contract_version: impl Into<String>,
        description: impl Into<String>,
        canonical_seed: u64,
    ) -> Self {
        Self {
            identity: DualRunScenarioIdentity::phase1(
                scenario_id,
                surface_id,
                contract_version,
                description,
                canonical_seed,
            ),
            lab_fn: None,
            live_fn: None,
        }
    }

    /// Create a harness from an existing identity.
    #[must_use]
    pub fn from_identity(identity: DualRunScenarioIdentity) -> Self {
        Self {
            identity,
            lab_fn: None,
            live_fn: None,
        }
    }

    /// Set the lab execution function.
    ///
    /// Receives a `LabConfig` derived from the seed plan. Must return
    /// normalized semantics from the lab execution.
    #[must_use]
    pub fn lab(mut self, f: impl FnOnce(LabConfig) -> NormalizedSemantics + 'static) -> Self {
        self.lab_fn = Some(Box::new(f));
        self
    }

    /// Set the live execution function.
    ///
    /// Receives `(effective_seed, entropy_seed)` derived from the seed plan.
    /// Must return normalized semantics from the live execution.
    #[must_use]
    pub fn live(mut self, f: impl FnOnce(u64, u64) -> NormalizedSemantics + 'static) -> Self {
        self.live_fn = Some(Box::new(f));
        self
    }

    /// Override the seed plan.
    #[must_use]
    pub fn with_seed_plan(mut self, plan: SeedPlan) -> Self {
        self.identity.seed_plan = plan;
        self
    }

    /// Execute both sides and produce a comparison result.
    ///
    /// # Panics
    ///
    /// Panics if either `lab` or `live` was not set.
    pub fn run(self) -> DualRunResult {
        let lab_fn = self.lab_fn.expect("DualRunHarness: lab function not set");
        let live_fn = self.live_fn.expect("DualRunHarness: live function not set");

        let plan = &self.identity.seed_plan;
        let family = self.identity.family_id();

        // Run lab side.
        let lab_config = plan.to_lab_config();
        let lab_semantics = lab_fn(lab_config);
        let lab_prov = ReplayMetadata::for_lab(family.clone(), plan);
        let lab_obs =
            NormalizedObservable::new(&self.identity, RuntimeKind::Lab, lab_semantics, lab_prov);

        // Run live side.
        let live_seed = plan.effective_live_seed();
        let live_entropy = plan.effective_entropy_seed(live_seed);
        let live_semantics = live_fn(live_seed, live_entropy);
        let live_prov = ReplayMetadata::for_live(family, plan);
        let live_obs =
            NormalizedObservable::new(&self.identity, RuntimeKind::Live, live_semantics, live_prov);

        // Check invariants.
        let lab_violations = check_core_invariants(&lab_obs);
        let live_violations = check_core_invariants(&live_obs);

        // Compare.
        let lineage = SeedLineageRecord::from_plan(plan);
        let verdict = compare_observables(&lab_obs, &live_obs, lineage.clone());

        // Log result.
        tracing::info!(
            scenario_id = %self.identity.scenario_id,
            surface_id = %self.identity.surface_id,
            seed = %format_args!("0x{:X}", plan.canonical_seed),
            passed = verdict.passed,
            lab_violations = lab_violations.len(),
            live_violations = live_violations.len(),
            mismatches = verdict.mismatches.len(),
            "DUAL_RUN_RESULT"
        );

        DualRunResult {
            lab: lab_obs,
            live: live_obs,
            verdict,
            lab_invariant_violations: lab_violations,
            live_invariant_violations: live_violations,
            seed_lineage: lineage,
        }
    }
}

/// Convenience: run a dual-run test and assert it passes.
///
/// Panics with a detailed message if the test fails.
pub fn assert_dual_run_passes(result: &DualRunResult) {
    if !result.passed() {
        panic!(
            "Dual-run test failed for scenario '{}' on surface '{}':\n{}",
            result.verdict.scenario_id,
            result.verdict.surface_id,
            result.summary()
        );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test(name: &str) {
        crate::test_utils::init_test_logging();
        crate::test_phase!(name);
    }

    // --- SeedMode ---

    #[test]
    fn seed_mode_serde_roundtrip() {
        init_test("seed_mode_serde_roundtrip");
        let json = serde_json::to_string(&SeedMode::Inherit).unwrap();
        assert_eq!(json, "\"inherit\"");
        let parsed: SeedMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, SeedMode::Inherit);

        let json = serde_json::to_string(&SeedMode::Override).unwrap();
        assert_eq!(json, "\"override\"");
        let parsed: SeedMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, SeedMode::Override);
        crate::test_complete!("seed_mode_serde_roundtrip");
    }

    // --- ReplayPolicy ---

    #[test]
    fn replay_policy_serde_roundtrip() {
        init_test("replay_policy_serde_roundtrip");
        for policy in [
            ReplayPolicy::SingleSeed,
            ReplayPolicy::SeedSweep,
            ReplayPolicy::ReplayBundle,
        ] {
            let json = serde_json::to_string(&policy).unwrap();
            let parsed: ReplayPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, policy);
        }
        crate::test_complete!("replay_policy_serde_roundtrip");
    }

    // --- SeedPlan ---

    #[test]
    fn seed_plan_inherit_uses_canonical() {
        init_test("seed_plan_inherit_uses_canonical");
        let plan = SeedPlan::inherit(0xBEEF, "test-scenario");
        assert_eq!(plan.effective_lab_seed(), 0xBEEF);
        assert_eq!(plan.effective_live_seed(), 0xBEEF);
        assert_eq!(plan.lab_seed_mode, SeedMode::Inherit);
        assert_eq!(plan.live_seed_mode, SeedMode::Inherit);
        crate::test_complete!("seed_plan_inherit_uses_canonical");
    }

    #[test]
    fn seed_plan_override_uses_explicit_seed() {
        init_test("seed_plan_override_uses_explicit_seed");
        let plan = SeedPlan::inherit(0xBEEF, "test")
            .with_lab_override(0xCAFE)
            .with_live_override(0xFACE);
        assert_eq!(plan.effective_lab_seed(), 0xCAFE);
        assert_eq!(plan.effective_live_seed(), 0xFACE);
        assert_eq!(plan.lab_seed_mode, SeedMode::Override);
        assert_eq!(plan.live_seed_mode, SeedMode::Override);
        crate::test_complete!("seed_plan_override_uses_explicit_seed");
    }

    #[test]
    fn seed_plan_override_without_value_falls_back_to_canonical() {
        init_test("seed_plan_override_without_value_falls_back");
        let mut plan = SeedPlan::inherit(0xBEEF, "test");
        plan.lab_seed_mode = SeedMode::Override;
        // No lab_seed_override set — should fall back to canonical.
        assert_eq!(plan.effective_lab_seed(), 0xBEEF);
        crate::test_complete!("seed_plan_override_without_value_falls_back");
    }

    #[test]
    fn seed_plan_entropy_derives_from_effective() {
        init_test("seed_plan_entropy_derives_from_effective");
        let plan = SeedPlan::inherit(42, "test");
        let entropy = plan.effective_entropy_seed(42);
        // Must be deterministic.
        assert_eq!(entropy, plan.effective_entropy_seed(42));
        // Must differ from the seed itself (extremely unlikely to collide).
        assert_ne!(entropy, 42);
        crate::test_complete!("seed_plan_entropy_derives_from_effective");
    }

    #[test]
    fn seed_plan_entropy_override() {
        init_test("seed_plan_entropy_override");
        let plan = SeedPlan::inherit(42, "test").with_entropy_seed(999);
        assert_eq!(plan.effective_entropy_seed(42), 999);
        assert_eq!(plan.effective_entropy_seed(100), 999);
        crate::test_complete!("seed_plan_entropy_override");
    }

    #[test]
    fn seed_plan_to_lab_config() {
        init_test("seed_plan_to_lab_config");
        let plan = SeedPlan::inherit(0xDEAD, "test");
        let config = plan.to_lab_config();
        assert_eq!(config.seed, 0xDEAD);
        let expected_entropy = plan.effective_entropy_seed(0xDEAD);
        assert_eq!(config.entropy_seed, expected_entropy);
        crate::test_complete!("seed_plan_to_lab_config");
    }

    #[test]
    fn seed_plan_to_lab_config_with_override() {
        init_test("seed_plan_to_lab_config_with_override");
        let plan = SeedPlan::inherit(0xDEAD, "test").with_lab_override(0xCAFE);
        let config = plan.to_lab_config();
        assert_eq!(config.seed, 0xCAFE);
        crate::test_complete!("seed_plan_to_lab_config_with_override");
    }

    #[test]
    fn seed_plan_sweep_deterministic() {
        init_test("seed_plan_sweep_deterministic");
        let plan = SeedPlan::inherit(42, "test").with_replay_policy(ReplayPolicy::SeedSweep);
        let seeds1 = plan.sweep_seeds(5);
        let seeds2 = plan.sweep_seeds(5);
        assert_eq!(seeds1, seeds2);
        assert_eq!(seeds1.len(), 5);
        // All seeds should be distinct.
        let mut unique = seeds1.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 5);
        crate::test_complete!("seed_plan_sweep_deterministic");
    }

    #[test]
    fn seed_plan_serde_roundtrip() {
        init_test("seed_plan_serde_roundtrip");
        let plan = SeedPlan::inherit(0xABCD, "lineage-1")
            .with_lab_override(0x1234)
            .with_entropy_seed(0x5678)
            .with_replay_policy(ReplayPolicy::SeedSweep);
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let parsed: SeedPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, plan);
        crate::test_complete!("seed_plan_serde_roundtrip");
    }

    #[test]
    fn seed_plan_display() {
        init_test("seed_plan_display");
        let plan = SeedPlan::inherit(42, "test-scenario");
        let display = format!("{plan}");
        assert!(display.contains("0x2A"));
        assert!(display.contains("test-scenario"));
        crate::test_complete!("seed_plan_display");
    }

    // --- ScenarioFamilyId ---

    #[test]
    fn scenario_family_id_display() {
        init_test("scenario_family_id_display");
        let fam = ScenarioFamilyId::new("cancel.race", "cancellation.race", "v1");
        let s = format!("{fam}");
        assert!(s.contains("cancel.race"));
        assert!(s.contains("cancellation.race"));
        assert!(s.contains("v1"));
        crate::test_complete!("scenario_family_id_display");
    }

    #[test]
    fn scenario_family_id_serde_roundtrip() {
        init_test("scenario_family_id_serde_roundtrip");
        let fam = ScenarioFamilyId::new("cancel.race", "cancellation.race", "v1");
        let json = serde_json::to_string(&fam).unwrap();
        let parsed: ScenarioFamilyId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, fam);
        crate::test_complete!("scenario_family_id_serde_roundtrip");
    }

    // --- ExecutionInstanceId ---

    #[test]
    fn execution_instance_lab_vs_live() {
        init_test("execution_instance_lab_vs_live");
        let lab = ExecutionInstanceId::lab("test-family", 42);
        let live = ExecutionInstanceId::live("test-family", 42);
        assert_eq!(lab.runtime_kind, RuntimeKind::Lab);
        assert_eq!(live.runtime_kind, RuntimeKind::Live);
        assert_ne!(lab.key(), live.key());
        crate::test_complete!("execution_instance_lab_vs_live");
    }

    #[test]
    fn execution_instance_key_stable() {
        init_test("execution_instance_key_stable");
        let inst = ExecutionInstanceId::lab("fam", 0xBEEF).with_run_index(3);
        let key1 = inst.key();
        let key2 = inst.key();
        assert_eq!(key1, key2);
        assert!(key1.contains("fam"));
        assert!(key1.contains("0xBEEF"));
        assert!(key1.contains("3"));
        crate::test_complete!("execution_instance_key_stable");
    }

    // --- RuntimeKind ---

    #[test]
    fn runtime_kind_display() {
        init_test("runtime_kind_display");
        assert_eq!(format!("{}", RuntimeKind::Lab), "lab");
        assert_eq!(format!("{}", RuntimeKind::Live), "live");
        crate::test_complete!("runtime_kind_display");
    }

    // --- ReplayMetadata ---

    #[test]
    fn replay_metadata_lab_seeds_match_plan() {
        init_test("replay_metadata_lab_seeds_match_plan");
        let family = ScenarioFamilyId::new("test", "surface", "v1");
        let plan = SeedPlan::inherit(0xDEAD, "lineage");
        let meta = ReplayMetadata::for_lab(family, &plan);
        assert_eq!(meta.effective_seed, 0xDEAD);
        assert_eq!(meta.instance.runtime_kind, RuntimeKind::Lab);
        assert_eq!(
            meta.effective_entropy_seed,
            plan.effective_entropy_seed(0xDEAD)
        );
        crate::test_complete!("replay_metadata_lab_seeds_match_plan");
    }

    #[test]
    fn replay_metadata_live_seeds_match_plan() {
        init_test("replay_metadata_live_seeds_match_plan");
        let family = ScenarioFamilyId::new("test", "surface", "v1");
        let plan = SeedPlan::inherit(0xCAFE, "lineage");
        let meta = ReplayMetadata::for_live(family, &plan);
        assert_eq!(meta.effective_seed, 0xCAFE);
        assert_eq!(meta.instance.runtime_kind, RuntimeKind::Live);
        crate::test_complete!("replay_metadata_live_seeds_match_plan");
    }

    #[test]
    fn replay_metadata_with_overrides() {
        init_test("replay_metadata_with_overrides");
        let family = ScenarioFamilyId::new("test", "surface", "v1");
        let plan = SeedPlan::inherit(42, "lineage").with_lab_override(999);
        let meta = ReplayMetadata::for_lab(family, &plan);
        assert_eq!(meta.effective_seed, 999);
        crate::test_complete!("replay_metadata_with_overrides");
    }

    #[test]
    fn replay_metadata_with_lab_report() {
        init_test("replay_metadata_with_lab_report");
        let family = ScenarioFamilyId::new("test", "surface", "v1");
        let plan = SeedPlan::inherit(42, "lineage");
        let meta = ReplayMetadata::for_lab(family, &plan)
            .with_lab_report(0xF1, 0xE1, 100, 0x51, 500)
            .with_repro_command("cargo test test -- --nocapture")
            .with_artifact_path("/tmp/artifacts/test");
        assert_eq!(meta.trace_fingerprint, Some(0xF1));
        assert_eq!(meta.event_count, Some(100));
        assert_eq!(meta.steps_total, Some(500));
        assert!(meta.repro_command.is_some());
        assert!(meta.artifact_path.is_some());
        crate::test_complete!("replay_metadata_with_lab_report");
    }

    #[test]
    fn replay_metadata_default_repro_command() {
        init_test("replay_metadata_default_repro_command");
        let family = ScenarioFamilyId::new("cancel.race", "surface", "v1");
        let plan = SeedPlan::inherit(0xDEAD, "lineage");
        let meta = ReplayMetadata::for_lab(family, &plan);
        let cmd = meta.default_repro_command();
        assert!(cmd.contains("0xDEAD"));
        assert!(cmd.contains("cancel.race"));
        crate::test_complete!("replay_metadata_default_repro_command");
    }

    #[test]
    fn replay_metadata_serde_roundtrip() {
        init_test("replay_metadata_serde_roundtrip");
        let family = ScenarioFamilyId::new("test", "surface", "v1");
        let plan = SeedPlan::inherit(42, "lineage");
        let meta = ReplayMetadata::for_lab(family, &plan).with_repro_command("cargo test");
        let json = serde_json::to_string_pretty(&meta).unwrap();
        let parsed: ReplayMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.effective_seed, meta.effective_seed);
        assert_eq!(parsed.family.id, "test");
        crate::test_complete!("replay_metadata_serde_roundtrip");
    }

    // --- SeedLineageRecord ---

    #[test]
    fn seed_lineage_record_inherit_seeds_match() {
        init_test("seed_lineage_record_inherit_seeds_match");
        let plan = SeedPlan::inherit(0xBEEF, "lineage-1");
        let record = SeedLineageRecord::from_plan(&plan);
        assert!(record.seeds_match);
        assert_eq!(record.lab_effective_seed, 0xBEEF);
        assert_eq!(record.live_effective_seed, 0xBEEF);
        assert_eq!(record.lab_entropy_seed, record.live_entropy_seed);
        crate::test_complete!("seed_lineage_record_inherit_seeds_match");
    }

    #[test]
    fn seed_lineage_record_override_seeds_differ() {
        init_test("seed_lineage_record_override_seeds_differ");
        let plan = SeedPlan::inherit(42, "lineage-1")
            .with_lab_override(100)
            .with_live_override(200);
        let record = SeedLineageRecord::from_plan(&plan);
        assert!(!record.seeds_match);
        assert_eq!(record.lab_effective_seed, 100);
        assert_eq!(record.live_effective_seed, 200);
        crate::test_complete!("seed_lineage_record_override_seeds_differ");
    }

    #[test]
    fn seed_lineage_record_serde_roundtrip() {
        init_test("seed_lineage_record_serde_roundtrip");
        let plan = SeedPlan::inherit(42, "lin");
        let record = SeedLineageRecord::from_plan(&plan).with_annotation("source", "test");
        let json = serde_json::to_string(&record).unwrap();
        let parsed: SeedLineageRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.canonical_seed, 42);
        assert_eq!(parsed.annotations.get("source").unwrap(), "test");
        crate::test_complete!("seed_lineage_record_serde_roundtrip");
    }

    // --- DualRunScenarioIdentity ---

    #[test]
    fn dual_run_scenario_identity_phase1() {
        init_test("dual_run_scenario_identity_phase1");
        let ident = DualRunScenarioIdentity::phase1(
            "phase1.cancel.race.one_loser",
            "cancellation.race",
            "v1",
            "Race two tasks, cancel loser, verify drain",
            42,
        );
        assert_eq!(ident.schema_version, DUAL_RUN_SCHEMA_VERSION);
        assert_eq!(ident.phase, Phase::Phase1);
        assert_eq!(ident.seed_plan.canonical_seed, 42);
        assert_eq!(
            ident.seed_plan.seed_lineage_id,
            "phase1.cancel.race.one_loser"
        );
        crate::test_complete!("dual_run_scenario_identity_phase1");
    }

    #[test]
    fn dual_run_identity_lab_config() {
        init_test("dual_run_identity_lab_config");
        let ident = DualRunScenarioIdentity::phase1("test", "s", "v1", "d", 0xBEEF);
        let config = ident.to_lab_config();
        assert_eq!(config.seed, 0xBEEF);
        crate::test_complete!("dual_run_identity_lab_config");
    }

    #[test]
    fn dual_run_identity_replay_metadata_lab_live_differ() {
        init_test("dual_run_identity_replay_metadata_lab_live_differ");
        let ident = DualRunScenarioIdentity::phase1("test", "s", "v1", "d", 42);
        let lab_meta = ident.lab_replay_metadata();
        let live_meta = ident.live_replay_metadata();
        assert_eq!(lab_meta.instance.runtime_kind, RuntimeKind::Lab);
        assert_eq!(live_meta.instance.runtime_kind, RuntimeKind::Live);
        // With inherit mode, effective seeds match.
        assert_eq!(lab_meta.effective_seed, live_meta.effective_seed);
        crate::test_complete!("dual_run_identity_replay_metadata_lab_live_differ");
    }

    #[test]
    fn dual_run_identity_family_id() {
        init_test("dual_run_identity_family_id");
        let ident = DualRunScenarioIdentity::phase1("test", "surface", "v1", "desc", 42);
        let fam = ident.family_id();
        assert_eq!(fam.id, "test");
        assert_eq!(fam.surface_id, "surface");
        assert_eq!(fam.surface_contract_version, "v1");
        crate::test_complete!("dual_run_identity_family_id");
    }

    #[test]
    fn dual_run_identity_seed_lineage() {
        init_test("dual_run_identity_seed_lineage");
        let ident = DualRunScenarioIdentity::phase1("test", "s", "v1", "d", 42);
        let lineage = ident.seed_lineage();
        assert!(lineage.seeds_match);
        assert_eq!(lineage.canonical_seed, 42);
        crate::test_complete!("dual_run_identity_seed_lineage");
    }

    #[test]
    fn dual_run_identity_with_seed_plan_override() {
        init_test("dual_run_identity_with_seed_plan_override");
        let plan = SeedPlan::inherit(99, "custom-lineage").with_lab_override(0xFF);
        let ident =
            DualRunScenarioIdentity::phase1("test", "s", "v1", "d", 42).with_seed_plan(plan);
        assert_eq!(ident.seed_plan.canonical_seed, 99);
        assert_eq!(ident.to_lab_config().seed, 0xFF);
        crate::test_complete!("dual_run_identity_with_seed_plan_override");
    }

    #[test]
    fn dual_run_identity_metadata() {
        init_test("dual_run_identity_metadata");
        let ident = DualRunScenarioIdentity::phase1("test", "s", "v1", "d", 42)
            .with_metadata("bead", "2a6k9.2.3")
            .with_metadata("author", "SapphireHill");
        assert_eq!(ident.metadata.get("bead").unwrap(), "2a6k9.2.3");
        assert_eq!(ident.metadata.get("author").unwrap(), "SapphireHill");
        crate::test_complete!("dual_run_identity_metadata");
    }

    #[test]
    fn dual_run_identity_serde_roundtrip() {
        init_test("dual_run_identity_serde_roundtrip");
        let ident = DualRunScenarioIdentity::phase1(
            "phase1.cancel.race.one_loser",
            "cancellation.race",
            "v1",
            "Race two tasks, cancel loser, verify drain",
            42,
        )
        .with_metadata("bead", "2a6k9.2.3");
        let json = serde_json::to_string_pretty(&ident).unwrap();
        let parsed: DualRunScenarioIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.scenario_id, ident.scenario_id);
        assert_eq!(parsed.seed_plan, ident.seed_plan);
        assert_eq!(parsed.phase, Phase::Phase1);
        crate::test_complete!("dual_run_identity_serde_roundtrip");
    }

    // --- Cross-cutting: seed determinism across lab and live ---

    #[test]
    fn same_plan_produces_same_lab_config() {
        init_test("same_plan_produces_same_lab_config");
        let plan = SeedPlan::inherit(0xCAFE_BABE, "determinism-check");
        let c1 = plan.to_lab_config();
        let c2 = plan.to_lab_config();
        assert_eq!(c1.seed, c2.seed);
        assert_eq!(c1.entropy_seed, c2.entropy_seed);
        crate::test_complete!("same_plan_produces_same_lab_config");
    }

    #[test]
    fn inherit_mode_lab_live_seeds_identical() {
        init_test("inherit_mode_lab_live_seeds_identical");
        let plan = SeedPlan::inherit(0xDEAD_BEEF, "identical-check");
        assert_eq!(plan.effective_lab_seed(), plan.effective_live_seed());
        let lab_ent = plan.effective_entropy_seed(plan.effective_lab_seed());
        let live_ent = plan.effective_entropy_seed(plan.effective_live_seed());
        assert_eq!(lab_ent, live_ent);
        crate::test_complete!("inherit_mode_lab_live_seeds_identical");
    }

    #[test]
    fn different_canonical_seeds_produce_different_entropies() {
        init_test("different_canonical_seeds_different_entropies");
        let p1 = SeedPlan::inherit(1, "a");
        let p2 = SeedPlan::inherit(2, "b");
        assert_ne!(
            p1.effective_entropy_seed(p1.effective_lab_seed()),
            p2.effective_entropy_seed(p2.effective_lab_seed())
        );
        crate::test_complete!("different_canonical_seeds_different_entropies");
    }

    // --- Normalized Observable types ---

    fn make_happy_semantics() -> NormalizedSemantics {
        NormalizedSemantics {
            terminal_outcome: TerminalOutcome::ok(),
            cancellation: CancellationRecord::none(),
            loser_drain: LoserDrainRecord::not_applicable(),
            region_close: RegionCloseRecord::quiescent(),
            obligation_balance: ObligationBalanceRecord::zero(),
            resource_surface: ResourceSurfaceRecord::empty("test"),
        }
    }

    fn make_observable(kind: RuntimeKind, semantics: NormalizedSemantics) -> NormalizedObservable {
        let ident = DualRunScenarioIdentity::phase1("test", "s", "v1", "d", 42);
        let prov = match kind {
            RuntimeKind::Lab => ident.lab_replay_metadata(),
            RuntimeKind::Live => ident.live_replay_metadata(),
        };
        NormalizedObservable::new(&ident, kind, semantics, prov)
    }

    #[test]
    fn terminal_outcome_ok_serde() {
        init_test("terminal_outcome_ok_serde");
        let t = TerminalOutcome::ok();
        let json = serde_json::to_string(&t).unwrap();
        let parsed: TerminalOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.class, OutcomeClass::Ok);
        crate::test_complete!("terminal_outcome_ok_serde");
    }

    #[test]
    fn terminal_outcome_cancelled() {
        init_test("terminal_outcome_cancelled");
        let t = TerminalOutcome::cancelled("user_request");
        assert_eq!(t.class, OutcomeClass::Cancelled);
        assert_eq!(t.cancel_reason_class.as_deref(), Some("user_request"));
        crate::test_complete!("terminal_outcome_cancelled");
    }

    #[test]
    fn cancellation_record_none_vs_completed() {
        init_test("cancellation_record_none_vs_completed");
        let none = CancellationRecord::none();
        let completed = CancellationRecord::completed();
        assert!(!none.requested);
        assert!(completed.requested);
        assert!(completed.acknowledged);
        assert!(completed.cleanup_completed);
        assert!(completed.finalization_completed);
        assert_eq!(completed.terminal_phase, CancelTerminalPhase::Completed);
        crate::test_complete!("cancellation_record_none_vs_completed");
    }

    #[test]
    fn loser_drain_complete() {
        init_test("loser_drain_complete");
        let drain = LoserDrainRecord::complete(3);
        assert!(drain.applicable);
        assert_eq!(drain.expected_losers, 3);
        assert_eq!(drain.drained_losers, 3);
        assert_eq!(drain.status, DrainStatus::Complete);
        crate::test_complete!("loser_drain_complete");
    }

    #[test]
    fn obligation_balance_recompute() {
        init_test("obligation_balance_recompute");
        let b = ObligationBalanceRecord {
            reserved: 10,
            committed: 7,
            aborted: 2,
            leaked: 1,
            unresolved: 99, // wrong, should recompute
            balanced: true, // wrong
        }
        .recompute();
        assert_eq!(b.unresolved, 0); // 10 - (7+2+1) = 0
        assert!(!b.balanced); // leaked > 0
        crate::test_complete!("obligation_balance_recompute");
    }

    #[test]
    fn resource_surface_counter_tolerance() {
        init_test("resource_surface_counter_tolerance");
        let rs = ResourceSurfaceRecord::empty("test-surface")
            .with_counter("msgs", 5)
            .with_counter_tolerance("bytes", 100, CounterTolerance::AtLeast);
        assert_eq!(rs.counters["msgs"], 5);
        assert_eq!(rs.tolerances["msgs"], CounterTolerance::Exact);
        assert_eq!(rs.tolerances["bytes"], CounterTolerance::AtLeast);
        crate::test_complete!("resource_surface_counter_tolerance");
    }

    #[test]
    fn normalized_observable_serde_roundtrip() {
        init_test("normalized_observable_serde_roundtrip");
        let obs = make_observable(RuntimeKind::Lab, make_happy_semantics());
        let json = serde_json::to_string_pretty(&obs).unwrap();
        let parsed: NormalizedObservable = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, NORMALIZED_OBSERVABLE_SCHEMA_VERSION);
        assert_eq!(parsed.runtime_kind, RuntimeKind::Lab);
        assert_eq!(parsed.semantics.terminal_outcome.class, OutcomeClass::Ok);
        crate::test_complete!("normalized_observable_serde_roundtrip");
    }

    // --- Compare / Verdict ---

    #[test]
    fn compare_identical_observables_passes() {
        init_test("compare_identical_observables_passes");
        let lab = make_observable(RuntimeKind::Lab, make_happy_semantics());
        let live = make_observable(RuntimeKind::Live, make_happy_semantics());
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        assert!(verdict.passed);
        assert!(verdict.mismatches.is_empty());
        crate::test_complete!("compare_identical_observables_passes");
    }

    #[test]
    fn compare_outcome_mismatch_fails() {
        init_test("compare_outcome_mismatch_fails");
        let lab_sem = make_happy_semantics();
        let mut live_sem = make_happy_semantics();
        live_sem.terminal_outcome = TerminalOutcome::cancelled("timeout");
        let lab = make_observable(RuntimeKind::Lab, lab_sem);
        let live = make_observable(RuntimeKind::Live, live_sem);
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        assert!(!verdict.passed);
        assert!(
            verdict
                .mismatches
                .iter()
                .any(|m| m.field.contains("terminal_outcome.class"))
        );
        crate::test_complete!("compare_outcome_mismatch_fails");
    }

    #[test]
    fn compare_obligation_leak_mismatch() {
        init_test("compare_obligation_leak_mismatch");
        let lab_sem = make_happy_semantics();
        let mut live_sem = make_happy_semantics();
        live_sem.obligation_balance = ObligationBalanceRecord {
            reserved: 5,
            committed: 3,
            aborted: 0,
            leaked: 2,
            unresolved: 0,
            balanced: false,
        };
        let lab = make_observable(RuntimeKind::Lab, lab_sem);
        let live = make_observable(RuntimeKind::Live, live_sem);
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        assert!(!verdict.passed);
        assert!(
            verdict
                .mismatches
                .iter()
                .any(|m| m.field.contains("leaked"))
        );
        crate::test_complete!("compare_obligation_leak_mismatch");
    }

    #[test]
    fn compare_resource_counter_exact_mismatch() {
        init_test("compare_resource_counter_exact_mismatch");
        let mut lab_sem = make_happy_semantics();
        lab_sem.resource_surface = ResourceSurfaceRecord::empty("test").with_counter("msgs", 5);
        let mut live_sem = make_happy_semantics();
        live_sem.resource_surface = ResourceSurfaceRecord::empty("test").with_counter("msgs", 3);
        let lab = make_observable(RuntimeKind::Lab, lab_sem);
        let live = make_observable(RuntimeKind::Live, live_sem);
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        assert!(!verdict.passed);
        assert!(
            verdict
                .mismatches
                .iter()
                .any(|m| m.field.contains("counters.msgs"))
        );
        crate::test_complete!("compare_resource_counter_exact_mismatch");
    }

    #[test]
    fn compare_resource_counter_at_least_passes() {
        init_test("compare_resource_counter_at_least_passes");
        let mut lab_sem = make_happy_semantics();
        lab_sem.resource_surface = ResourceSurfaceRecord::empty("test").with_counter_tolerance(
            "msgs",
            5,
            CounterTolerance::AtLeast,
        );
        let mut live_sem = make_happy_semantics();
        live_sem.resource_surface = ResourceSurfaceRecord::empty("test").with_counter_tolerance(
            "msgs",
            7,
            CounterTolerance::AtLeast,
        );
        let lab = make_observable(RuntimeKind::Lab, lab_sem);
        let live = make_observable(RuntimeKind::Live, live_sem);
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        assert!(verdict.passed);
        crate::test_complete!("compare_resource_counter_at_least_passes");
    }

    #[test]
    fn compare_cancellation_mismatch() {
        init_test("compare_cancellation_mismatch");
        let mut lab_sem = make_happy_semantics();
        lab_sem.cancellation = CancellationRecord::completed();
        let live_sem = make_happy_semantics(); // no cancellation
        let lab = make_observable(RuntimeKind::Lab, lab_sem);
        let live = make_observable(RuntimeKind::Live, live_sem);
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        assert!(!verdict.passed);
        assert!(
            verdict
                .mismatches
                .iter()
                .any(|m| m.field.contains("cancellation"))
        );
        crate::test_complete!("compare_cancellation_mismatch");
    }

    #[test]
    fn verdict_display_pass() {
        init_test("verdict_display_pass");
        let lab = make_observable(RuntimeKind::Lab, make_happy_semantics());
        let live = make_observable(RuntimeKind::Live, make_happy_semantics());
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        let summary = verdict.summary();
        assert!(summary.contains("PASS"));
        crate::test_complete!("verdict_display_pass");
    }

    #[test]
    fn verdict_display_fail() {
        init_test("verdict_display_fail");
        let lab_sem = make_happy_semantics();
        let mut live_sem = make_happy_semantics();
        live_sem.region_close.quiescent = false;
        let lab = make_observable(RuntimeKind::Lab, lab_sem);
        let live = make_observable(RuntimeKind::Live, live_sem);
        let plan = SeedPlan::inherit(42, "test");
        let verdict = compare_observables(&lab, &live, SeedLineageRecord::from_plan(&plan));
        let summary = verdict.summary();
        assert!(summary.contains("FAIL"));
        assert!(summary.contains("mismatch"));
        crate::test_complete!("verdict_display_fail");
    }

    // --- Core Invariant Checks ---

    #[test]
    fn check_core_invariants_all_pass() {
        init_test("check_core_invariants_all_pass");
        let obs = make_observable(RuntimeKind::Lab, make_happy_semantics());
        let violations = check_core_invariants(&obs);
        assert!(violations.is_empty());
        crate::test_complete!("check_core_invariants_all_pass");
    }

    #[test]
    fn check_core_invariants_obligation_leak() {
        init_test("check_core_invariants_obligation_leak");
        let mut sem = make_happy_semantics();
        sem.obligation_balance.leaked = 1;
        sem.obligation_balance.balanced = false;
        let obs = make_observable(RuntimeKind::Lab, sem);
        let violations = check_core_invariants(&obs);
        assert!(!violations.is_empty());
        assert!(violations[0].contains("leaked"));
        crate::test_complete!("check_core_invariants_obligation_leak");
    }

    #[test]
    fn check_core_invariants_not_quiescent() {
        init_test("check_core_invariants_not_quiescent");
        let mut sem = make_happy_semantics();
        sem.region_close.quiescent = false;
        sem.region_close.live_children = 2;
        let obs = make_observable(RuntimeKind::Lab, sem);
        let violations = check_core_invariants(&obs);
        assert!(violations.iter().any(|v| v.contains("quiescent")));
        crate::test_complete!("check_core_invariants_not_quiescent");
    }

    #[test]
    fn check_core_invariants_incomplete_drain() {
        init_test("check_core_invariants_incomplete_drain");
        let mut sem = make_happy_semantics();
        sem.loser_drain = LoserDrainRecord {
            applicable: true,
            expected_losers: 3,
            drained_losers: 1,
            status: DrainStatus::Incomplete,
            evidence: None,
        };
        let obs = make_observable(RuntimeKind::Lab, sem);
        let violations = check_core_invariants(&obs);
        assert!(violations.iter().any(|v| v.contains("drain")));
        crate::test_complete!("check_core_invariants_incomplete_drain");
    }

    #[test]
    fn check_core_invariants_cancel_incomplete() {
        init_test("check_core_invariants_cancel_incomplete");
        let mut sem = make_happy_semantics();
        sem.cancellation.requested = true;
        sem.cancellation.cleanup_completed = false;
        sem.cancellation.terminal_phase = CancelTerminalPhase::Cancelling;
        let obs = make_observable(RuntimeKind::Lab, sem);
        let violations = check_core_invariants(&obs);
        assert!(violations.iter().any(|v| v.contains("Cancellation")));
        crate::test_complete!("check_core_invariants_cancel_incomplete");
    }

    // --- assert_semantics ---

    #[test]
    fn assert_semantics_identical_passes() {
        init_test("assert_semantics_identical_passes");
        let sem = make_happy_semantics();
        let mismatches = assert_semantics(&sem, &sem);
        assert!(mismatches.is_empty());
        crate::test_complete!("assert_semantics_identical_passes");
    }

    #[test]
    fn assert_semantics_detects_diff() {
        init_test("assert_semantics_detects_diff");
        let expected = make_happy_semantics();
        let mut actual = make_happy_semantics();
        actual.terminal_outcome = TerminalOutcome::err("network_error");
        let mismatches = assert_semantics(&actual, &expected);
        assert!(!mismatches.is_empty());
        crate::test_complete!("assert_semantics_detects_diff");
    }

    // --- DualRunHarness ---

    #[test]
    fn harness_identical_runs_pass() {
        init_test("harness_identical_runs_pass");
        let result = DualRunHarness::phase1(
            "test.happy_path",
            "test.surface",
            "v1",
            "Both sides produce identical semantics",
            42,
        )
        .lab(|_config| make_happy_semantics())
        .live(|_seed, _entropy| make_happy_semantics())
        .run();

        assert!(result.passed());
        assert!(result.verdict.is_equivalent());
        assert!(result.lab_invariant_violations.is_empty());
        assert!(result.live_invariant_violations.is_empty());
        crate::test_complete!("harness_identical_runs_pass");
    }

    #[test]
    fn harness_outcome_mismatch_fails() {
        init_test("harness_outcome_mismatch_fails");
        let result = DualRunHarness::phase1(
            "test.mismatch",
            "test.surface",
            "v1",
            "Lab succeeds, live cancels",
            42,
        )
        .lab(|_config| make_happy_semantics())
        .live(|_seed, _entropy| {
            let mut sem = make_happy_semantics();
            sem.terminal_outcome = TerminalOutcome::cancelled("timeout");
            sem
        })
        .run();

        assert!(!result.passed());
        assert!(!result.verdict.is_equivalent());
        crate::test_complete!("harness_outcome_mismatch_fails");
    }

    #[test]
    fn harness_lab_invariant_violation_fails() {
        init_test("harness_lab_invariant_violation_fails");
        let result = DualRunHarness::phase1(
            "test.leak",
            "test.surface",
            "v1",
            "Lab leaks obligations",
            42,
        )
        .lab(|_config| {
            let mut sem = make_happy_semantics();
            sem.obligation_balance.leaked = 1;
            sem.obligation_balance.balanced = false;
            sem
        })
        .live(|_seed, _entropy| {
            let mut sem = make_happy_semantics();
            sem.obligation_balance.leaked = 1;
            sem.obligation_balance.balanced = false;
            sem
        })
        .run();

        // Semantics match (both leak), but invariant check catches it.
        assert!(result.verdict.is_equivalent());
        assert!(!result.lab_invariant_violations.is_empty());
        assert!(!result.passed()); // Failed due to invariant violations.
        crate::test_complete!("harness_lab_invariant_violation_fails");
    }

    #[test]
    fn harness_receives_correct_seeds() {
        init_test("harness_receives_correct_seeds");
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        let captured_lab_seed = Arc::new(AtomicU64::new(0));
        let captured_live_seed = Arc::new(AtomicU64::new(0));
        let lab_clone = Arc::clone(&captured_lab_seed);
        let live_clone = Arc::clone(&captured_live_seed);

        let result = DualRunHarness::phase1("test.seeds", "s", "v1", "d", 0xBEEF)
            .lab(move |config| {
                lab_clone.store(config.seed, Ordering::Relaxed);
                make_happy_semantics()
            })
            .live(move |seed, _entropy| {
                live_clone.store(seed, Ordering::Relaxed);
                make_happy_semantics()
            })
            .run();

        assert!(result.passed());
        assert_eq!(captured_lab_seed.load(Ordering::Relaxed), 0xBEEF);
        assert_eq!(captured_live_seed.load(Ordering::Relaxed), 0xBEEF);
        crate::test_complete!("harness_receives_correct_seeds");
    }

    #[test]
    fn harness_with_custom_seed_plan() {
        init_test("harness_with_custom_seed_plan");
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        let captured_lab = Arc::new(AtomicU64::new(0));
        let captured_live = Arc::new(AtomicU64::new(0));
        let lab_c = Arc::clone(&captured_lab);
        let live_c = Arc::clone(&captured_live);

        let plan = SeedPlan::inherit(42, "custom")
            .with_lab_override(0xCAFE)
            .with_live_override(0xFACE);

        let result = DualRunHarness::phase1("test", "s", "v1", "d", 42)
            .with_seed_plan(plan)
            .lab(move |config| {
                lab_c.store(config.seed, Ordering::Relaxed);
                make_happy_semantics()
            })
            .live(move |seed, _entropy| {
                live_c.store(seed, Ordering::Relaxed);
                make_happy_semantics()
            })
            .run();

        assert_eq!(captured_lab.load(Ordering::Relaxed), 0xCAFE);
        assert_eq!(captured_live.load(Ordering::Relaxed), 0xFACE);
        // Semantics match despite different seeds.
        assert!(result.verdict.is_equivalent());
        // But seeds don't match.
        assert!(!result.seed_lineage.seeds_match);
        crate::test_complete!("harness_with_custom_seed_plan");
    }

    #[test]
    fn harness_from_identity() {
        init_test("harness_from_identity");
        let ident = DualRunScenarioIdentity::phase1("test", "s", "v1", "d", 99);
        let result = DualRunHarness::from_identity(ident)
            .lab(|_| make_happy_semantics())
            .live(|_, _| make_happy_semantics())
            .run();
        assert!(result.passed());
        assert_eq!(result.verdict.scenario_id, "test");
        crate::test_complete!("harness_from_identity");
    }

    #[test]
    fn dual_run_result_display() {
        init_test("dual_run_result_display");
        let result = DualRunHarness::phase1("test", "s", "v1", "d", 42)
            .lab(|_| make_happy_semantics())
            .live(|_, _| make_happy_semantics())
            .run();
        let summary = format!("{result}");
        assert!(summary.contains("PASS"));
        crate::test_complete!("dual_run_result_display");
    }

    #[test]
    #[should_panic(expected = "Dual-run test failed")]
    fn assert_dual_run_passes_panics_on_failure() {
        init_test("assert_dual_run_passes_panics_on_failure");
        let result = DualRunHarness::phase1("test", "s", "v1", "d", 42)
            .lab(|_| make_happy_semantics())
            .live(|_, _| {
                let mut sem = make_happy_semantics();
                sem.terminal_outcome = TerminalOutcome::err("oops");
                sem
            })
            .run();
        assert_dual_run_passes(&result);
    }
}
