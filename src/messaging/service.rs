//! Contract-carrying service schemas for the FABRIC lane.

use super::class::{DeliveryClass, DeliveryClassPolicy, DeliveryClassPolicyError};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
use thiserror::Error;

/// Payload-shape declaration for FABRIC service requests and replies.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PayloadShape {
    /// No payload is carried.
    #[default]
    Empty,
    /// The payload is a JSON document.
    JsonDocument,
    /// The payload is an opaque binary frame.
    BinaryBlob,
    /// The payload is encoded directly into the subject path.
    SubjectEncoded,
    /// The payload follows an externally named schema.
    NamedSchema {
        /// Human-readable schema identifier.
        schema: String,
    },
}

impl PayloadShape {
    fn validate(&self, field: &str) -> Result<(), ServiceContractError> {
        if let Self::NamedSchema { schema } = self
            && schema.trim().is_empty()
        {
            return Err(ServiceContractError::EmptyNamedSchema {
                field: field.to_owned(),
            });
        }
        Ok(())
    }
}

/// Reply-shape declaration for service responses.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplyShape {
    /// The service does not emit a reply payload.
    #[default]
    None,
    /// The service emits exactly one reply payload.
    Unary {
        /// Payload shape for the reply.
        shape: PayloadShape,
    },
    /// The service emits a bounded stream of reply payloads.
    Stream {
        /// Payload shape for each streamed reply item.
        shape: PayloadShape,
    },
}

impl ReplyShape {
    fn validate(&self, field: &str) -> Result<(), ServiceContractError> {
        match self {
            Self::None => Ok(()),
            Self::Unary { shape } | Self::Stream { shape } => shape.validate(field),
        }
    }
}

/// Cleanup urgency promised by the service boundary.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CleanupUrgency {
    /// Cleanup can happen in the background after request cancellation.
    Background,
    /// Cleanup should complete promptly before the service fully unwinds.
    #[default]
    Prompt,
    /// Cleanup is urgent and should be prioritized immediately.
    Immediate,
}

impl fmt::Display for CleanupUrgency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Background => "background",
            Self::Prompt => "prompt",
            Self::Immediate => "immediate",
        };
        write!(f, "{name}")
    }
}

/// Budget semantics attached to a FABRIC service surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSemantics {
    /// Cleanup urgency when request cancellation occurs.
    pub cleanup_urgency: CleanupUrgency,
    /// Default timeout applied when the caller does not override it.
    pub default_timeout: Option<Duration>,
    /// Whether the caller may request a narrower timeout.
    pub allow_timeout_override: bool,
    /// Whether caller-provided priority hints are honored.
    pub honor_priority_hints: bool,
}

impl Default for BudgetSemantics {
    fn default() -> Self {
        Self {
            cleanup_urgency: CleanupUrgency::Prompt,
            default_timeout: Some(Duration::from_secs(30)),
            allow_timeout_override: true,
            honor_priority_hints: false,
        }
    }
}

impl BudgetSemantics {
    fn validate(&self) -> Result<(), ServiceContractError> {
        if self
            .default_timeout
            .is_some_and(|timeout| timeout.is_zero())
        {
            return Err(ServiceContractError::ZeroDuration {
                field: "budget_semantics.default_timeout".to_owned(),
            });
        }
        Ok(())
    }
}

/// Cancellation protocol expected from the service implementation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CancellationObligations {
    /// Best-effort drain with no reply guarantee.
    BestEffortDrain,
    /// Drain outstanding work before resolving the reply path.
    #[default]
    DrainBeforeReply,
    /// Drain outstanding work and run compensation before completion.
    DrainAndCompensate,
}

impl fmt::Display for CancellationObligations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::BestEffortDrain => "best-effort-drain",
            Self::DrainBeforeReply => "drain-before-reply",
            Self::DrainAndCompensate => "drain-and-compensate",
        };
        write!(f, "{name}")
    }
}

/// Capture policy for service-plane requests and replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRules {
    /// Whether request envelopes are captured for replay or diagnostics.
    pub capture_requests: bool,
    /// Whether reply envelopes are captured for replay or diagnostics.
    pub capture_replies: bool,
    /// Whether payload hashes are retained alongside captures.
    pub record_payload_hashes: bool,
    /// Whether branch attachments are retained when present.
    pub record_branch_artifacts: bool,
}

impl Default for CaptureRules {
    fn default() -> Self {
        Self {
            capture_requests: true,
            capture_replies: true,
            record_payload_hashes: true,
            record_branch_artifacts: false,
        }
    }
}

/// Compensation guarantee attached to a service surface.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CompensationSemantics {
    /// No compensation is promised.
    #[default]
    None,
    /// Compensation is attempted but not mandatory for every failure path.
    BestEffort,
    /// Compensation is part of the declared contract.
    Required,
}

impl fmt::Display for CompensationSemantics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::None => "none",
            Self::BestEffort => "best-effort",
            Self::Required => "required",
        };
        write!(f, "{name}")
    }
}

/// Mobility envelope for a service surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MobilityConstraint {
    /// The service may execute on any eligible region.
    #[default]
    Unrestricted,
    /// The service may move only inside the named region boundary.
    BoundedRegion {
        /// Human-readable region label.
        region: String,
    },
    /// The service is pinned to its current authority boundary.
    Pinned,
}

impl MobilityConstraint {
    fn validate(&self, field: &str) -> Result<(), ServiceContractError> {
        if let Self::BoundedRegion { region } = self
            && region.trim().is_empty()
        {
            return Err(ServiceContractError::EmptyBoundedRegion {
                field: field.to_owned(),
            });
        }
        Ok(())
    }

    /// Returns whether the provider's mobility boundary satisfies a required contract boundary.
    #[must_use]
    pub fn satisfies(&self, required: &Self) -> bool {
        match required {
            Self::Unrestricted => true,
            Self::BoundedRegion { region } => match self {
                Self::BoundedRegion {
                    region: provider_region,
                } => provider_region == region,
                Self::Pinned => true,
                Self::Unrestricted => false,
            },
            Self::Pinned => matches!(self, Self::Pinned),
        }
    }
}

impl fmt::Display for MobilityConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrestricted => write!(f, "unrestricted"),
            Self::BoundedRegion { region } => write!(f, "bounded-region({region})"),
            Self::Pinned => write!(f, "pinned"),
        }
    }
}

/// Evidence depth required by the service boundary.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    /// Minimal audit trail.
    Minimal,
    /// Standard operational evidence.
    #[default]
    Standard,
    /// Rich diagnostics and replay metadata.
    Detailed,
    /// Forensic-grade evidence and replay linkage.
    Forensic,
}

impl fmt::Display for EvidenceLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Detailed => "detailed",
            Self::Forensic => "forensic",
        };
        write!(f, "{name}")
    }
}

/// Overload response declared by the service provider.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverloadPolicy {
    /// Reject new requests once overload is detected.
    #[default]
    RejectNew,
    /// Admit work only while the bounded pending queue has capacity.
    QueueWithinBudget {
        /// Maximum queued requests once overload begins.
        max_pending: u32,
    },
    /// Prefer dropping the weakest delivery-class traffic first.
    DropEphemeral,
    /// Fail fast before request execution starts.
    FailFast,
}

impl OverloadPolicy {
    fn validate(&self) -> Result<(), ServiceContractError> {
        if let Self::QueueWithinBudget { max_pending } = self
            && *max_pending == 0
        {
            return Err(ServiceContractError::InvalidQueueCapacity);
        }
        Ok(())
    }
}

/// Full FABRIC service contract schema for one service boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceContractSchema {
    /// Request payload shape.
    pub request_shape: PayloadShape,
    /// Reply payload shape.
    pub reply_shape: ReplyShape,
    /// Cancellation duties for the service implementation.
    pub cancellation_obligations: CancellationObligations,
    /// Timeout and priority behavior.
    pub budget_semantics: BudgetSemantics,
    /// Minimum delivery/durability class promised by the contract.
    pub durability_class: DeliveryClass,
    /// Capture and replay policy.
    pub capture_rules: CaptureRules,
    /// Compensation semantics required by the contract.
    pub compensation_semantics: CompensationSemantics,
    /// Mobility envelope required by the contract.
    pub mobility_constraints: MobilityConstraint,
    /// Evidence depth required by the contract.
    pub evidence_requirements: EvidenceLevel,
    /// Overload behavior exposed to callers.
    pub overload_policy: OverloadPolicy,
}

impl Default for ServiceContractSchema {
    fn default() -> Self {
        Self {
            request_shape: PayloadShape::JsonDocument,
            reply_shape: ReplyShape::Unary {
                shape: PayloadShape::JsonDocument,
            },
            cancellation_obligations: CancellationObligations::default(),
            budget_semantics: BudgetSemantics::default(),
            durability_class: DeliveryClass::ObligationBacked,
            capture_rules: CaptureRules::default(),
            compensation_semantics: CompensationSemantics::None,
            mobility_constraints: MobilityConstraint::Unrestricted,
            evidence_requirements: EvidenceLevel::Standard,
            overload_policy: OverloadPolicy::default(),
        }
    }
}

impl ServiceContractSchema {
    /// Validate the contract for internal consistency.
    pub fn validate(&self) -> Result<(), ServiceContractError> {
        self.request_shape.validate("request_shape")?;
        self.reply_shape.validate("reply_shape")?;
        self.budget_semantics.validate()?;
        self.mobility_constraints.validate("mobility_constraints")?;
        self.overload_policy.validate()?;
        Ok(())
    }
}

/// Provider-declared guarantees that bound caller-selected options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTerms {
    /// Provider-admitted delivery classes and default choice.
    pub admissible_classes: DeliveryClassPolicy,
    /// Strongest durability class the provider guarantees on this surface.
    pub guaranteed_durability: DeliveryClass,
    /// Compensation policy the provider will honor.
    pub compensation_policy: CompensationSemantics,
    /// Provider mobility boundary.
    pub mobility_constraint: MobilityConstraint,
    /// Provider evidence guarantee.
    pub evidence_level: EvidenceLevel,
}

impl ProviderTerms {
    /// Validate provider terms against the contract envelope.
    pub fn validate_against(
        &self,
        contract: &ServiceContractSchema,
    ) -> Result<(), ServiceContractError> {
        self.mobility_constraint
            .validate("provider_terms.mobility_constraint")?;

        if self.guaranteed_durability < contract.durability_class {
            return Err(ServiceContractError::ProviderGuaranteeBelowContractFloor {
                guaranteed_durability: self.guaranteed_durability,
                required_durability: contract.durability_class,
            });
        }
        if self.compensation_policy < contract.compensation_semantics {
            return Err(ServiceContractError::ProviderCompensationBelowContract {
                provider: self.compensation_policy,
                required: contract.compensation_semantics,
            });
        }
        if self.evidence_level < contract.evidence_requirements {
            return Err(ServiceContractError::ProviderEvidenceBelowContract {
                provider: self.evidence_level,
                required: contract.evidence_requirements,
            });
        }
        if !self
            .mobility_constraint
            .satisfies(&contract.mobility_constraints)
        {
            return Err(ServiceContractError::ProviderMobilityIncompatible {
                provider: self.mobility_constraint.clone(),
                required: contract.mobility_constraints.clone(),
            });
        }
        for class in self.admissible_classes.admissible_classes() {
            if *class < contract.durability_class {
                return Err(ServiceContractError::ProviderClassBelowContractFloor {
                    class: *class,
                    required_durability: contract.durability_class,
                });
            }
            if *class > self.guaranteed_durability {
                return Err(
                    ServiceContractError::ProviderClassAboveGuaranteedDurability {
                        class: *class,
                        guaranteed_durability: self.guaranteed_durability,
                    },
                );
            }
        }
        Ok(())
    }
}

/// Caller-selected options that stay bounded by provider terms.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CallerOptions {
    /// Explicit delivery class request, or `None` for the provider default.
    pub requested_class: Option<DeliveryClass>,
    /// Narrower timeout requested by the caller.
    pub timeout_override: Option<Duration>,
    /// Optional scheduling hint in the range `0..=255`.
    pub priority_hint: Option<u8>,
}

/// Effective caller request after provider-bound validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedServiceRequest {
    /// Delivery class selected for the request.
    pub delivery_class: DeliveryClass,
    /// Effective timeout after applying defaults and caller overrides.
    pub timeout: Option<Duration>,
    /// Caller-provided priority hint, if honored.
    pub priority_hint: Option<u8>,
    /// Provider durability guarantee for the selected request.
    pub guaranteed_durability: DeliveryClass,
    /// Provider evidence guarantee.
    pub evidence_level: EvidenceLevel,
    /// Provider mobility boundary.
    pub mobility_constraint: MobilityConstraint,
    /// Compensation policy enforced for the request.
    pub compensation_policy: CompensationSemantics,
    /// Overload policy presented at the service boundary.
    pub overload_policy: OverloadPolicy,
}

/// Registered FABRIC service surface with provider/caller authority split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceRegistration {
    /// Human-readable service name.
    pub service_name: String,
    /// Structural contract for the service boundary.
    pub contract: ServiceContractSchema,
    /// Provider-declared bounds and guarantees.
    pub provider_terms: ProviderTerms,
}

impl ServiceRegistration {
    /// Register a service surface with validated provider terms.
    pub fn new(
        service_name: impl Into<String>,
        contract: ServiceContractSchema,
        provider_terms: ProviderTerms,
    ) -> Result<Self, ServiceContractError> {
        let service_name = service_name.into();
        if service_name.trim().is_empty() {
            return Err(ServiceContractError::EmptyServiceName);
        }
        contract.validate()?;
        provider_terms.validate_against(&contract)?;
        Ok(Self {
            service_name,
            contract,
            provider_terms,
        })
    }

    /// Validate caller-selected options against provider-declared bounds.
    pub fn validate_caller(
        &self,
        caller: &CallerOptions,
    ) -> Result<ValidatedServiceRequest, ServiceContractError> {
        if caller
            .timeout_override
            .is_some_and(|timeout| timeout.is_zero())
        {
            return Err(ServiceContractError::ZeroDuration {
                field: "caller_options.timeout_override".to_owned(),
            });
        }
        if caller.timeout_override.is_some()
            && !self.contract.budget_semantics.allow_timeout_override
        {
            return Err(ServiceContractError::TimeoutOverrideNotAllowed);
        }
        if caller.priority_hint.is_some() && !self.contract.budget_semantics.honor_priority_hints {
            return Err(ServiceContractError::PriorityHintsNotAllowed);
        }

        let delivery_class = self
            .provider_terms
            .admissible_classes
            .select_for_caller(caller.requested_class)?;

        Ok(ValidatedServiceRequest {
            delivery_class,
            timeout: caller
                .timeout_override
                .or(self.contract.budget_semantics.default_timeout),
            priority_hint: caller.priority_hint,
            guaranteed_durability: self.provider_terms.guaranteed_durability,
            evidence_level: self.provider_terms.evidence_level,
            mobility_constraint: self.provider_terms.mobility_constraint.clone(),
            compensation_policy: self.provider_terms.compensation_policy,
            overload_policy: self.contract.overload_policy.clone(),
        })
    }
}

/// Validation failure for FABRIC service contracts and caller requests.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServiceContractError {
    /// Service name must not be empty.
    #[error("service name must not be empty")]
    EmptyServiceName,
    /// Named schema references must be non-empty.
    #[error("named schema at `{field}` must not be empty")]
    EmptyNamedSchema {
        /// Field that declared an empty schema name.
        field: String,
    },
    /// Bounded-region mobility constraints require a non-empty region label.
    #[error("bounded-region mobility constraint at `{field}` must declare a region label")]
    EmptyBoundedRegion {
        /// Field that declared an empty region label.
        field: String,
    },
    /// Duration-valued fields must be non-zero when present.
    #[error("duration at `{field}` must be greater than zero")]
    ZeroDuration {
        /// Field that contained a zero duration.
        field: String,
    },
    /// Queue-based overload policies require positive capacity.
    #[error("queue-within-budget overload policy must declare max_pending > 0")]
    InvalidQueueCapacity,
    /// Provider durability guarantee is weaker than the contract floor.
    #[error(
        "provider guaranteed durability {guaranteed_durability} is weaker than contract floor {required_durability}"
    )]
    ProviderGuaranteeBelowContractFloor {
        /// Provider-declared durability guarantee.
        guaranteed_durability: DeliveryClass,
        /// Minimum durability required by the contract.
        required_durability: DeliveryClass,
    },
    /// Provider compensation guarantee is weaker than the contract requirement.
    #[error("provider compensation `{provider}` is weaker than contract requirement `{required}`")]
    ProviderCompensationBelowContract {
        /// Provider-declared compensation policy.
        provider: CompensationSemantics,
        /// Contract-required compensation policy.
        required: CompensationSemantics,
    },
    /// Provider evidence guarantee is weaker than the contract requirement.
    #[error(
        "provider evidence level `{provider}` is weaker than contract requirement `{required}`"
    )]
    ProviderEvidenceBelowContract {
        /// Provider-declared evidence level.
        provider: EvidenceLevel,
        /// Contract-required evidence level.
        required: EvidenceLevel,
    },
    /// Provider mobility guarantee is incompatible with the contract.
    #[error("provider mobility `{provider}` does not satisfy contract requirement `{required}`")]
    ProviderMobilityIncompatible {
        /// Provider-declared mobility boundary.
        provider: MobilityConstraint,
        /// Contract-required mobility boundary.
        required: MobilityConstraint,
    },
    /// Provider admitted a class weaker than the contract floor.
    #[error("provider admitted delivery class {class} below contract floor {required_durability}")]
    ProviderClassBelowContractFloor {
        /// Admitted delivery class.
        class: DeliveryClass,
        /// Contract floor for the service.
        required_durability: DeliveryClass,
    },
    /// Provider admitted a class it cannot guarantee durably.
    #[error(
        "provider admitted delivery class {class} above guaranteed durability {guaranteed_durability}"
    )]
    ProviderClassAboveGuaranteedDurability {
        /// Admitted delivery class.
        class: DeliveryClass,
        /// Strongest provider-guaranteed class.
        guaranteed_durability: DeliveryClass,
    },
    /// Caller tried to override timeout when the contract forbids it.
    #[error("caller timeout overrides are not allowed by the contract budget semantics")]
    TimeoutOverrideNotAllowed,
    /// Caller tried to pass a priority hint when the contract ignores hints.
    #[error("caller priority hints are not allowed by the contract budget semantics")]
    PriorityHintsNotAllowed,
    /// Delivery-class selection failed against the provider policy.
    #[error(transparent)]
    DeliveryClassPolicy(#[from] DeliveryClassPolicyError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_terms() -> ProviderTerms {
        ProviderTerms {
            admissible_classes: DeliveryClassPolicy::new(
                DeliveryClass::ObligationBacked,
                [DeliveryClass::ObligationBacked, DeliveryClass::MobilitySafe],
            )
            .expect("provider policy"),
            guaranteed_durability: DeliveryClass::MobilitySafe,
            compensation_policy: CompensationSemantics::BestEffort,
            mobility_constraint: MobilityConstraint::Pinned,
            evidence_level: EvidenceLevel::Detailed,
        }
    }

    fn contract() -> ServiceContractSchema {
        ServiceContractSchema {
            budget_semantics: BudgetSemantics {
                honor_priority_hints: true,
                ..BudgetSemantics::default()
            },
            compensation_semantics: CompensationSemantics::BestEffort,
            mobility_constraints: MobilityConstraint::Unrestricted,
            evidence_requirements: EvidenceLevel::Standard,
            ..ServiceContractSchema::default()
        }
    }

    #[test]
    fn service_registration_accepts_valid_contract() {
        let registration =
            ServiceRegistration::new("fabric.echo", contract(), provider_terms()).expect("valid");

        assert_eq!(registration.service_name, "fabric.echo");
        assert_eq!(
            registration.provider_terms.guaranteed_durability,
            DeliveryClass::MobilitySafe
        );
    }

    #[test]
    fn service_registration_rejects_provider_terms_below_contract() {
        let provider_terms = ProviderTerms {
            guaranteed_durability: DeliveryClass::DurableOrdered,
            ..provider_terms()
        };

        let err = ServiceRegistration::new("fabric.echo", contract(), provider_terms)
            .expect_err("durability floor should be enforced");

        assert_eq!(
            err,
            ServiceContractError::ProviderGuaranteeBelowContractFloor {
                guaranteed_durability: DeliveryClass::DurableOrdered,
                required_durability: DeliveryClass::ObligationBacked,
            }
        );
    }

    #[test]
    fn validate_caller_accepts_in_bounds_request() {
        let registration =
            ServiceRegistration::new("fabric.echo", contract(), provider_terms()).expect("valid");
        let caller = CallerOptions {
            requested_class: Some(DeliveryClass::MobilitySafe),
            timeout_override: Some(Duration::from_secs(5)),
            priority_hint: Some(200),
        };

        let validated = registration
            .validate_caller(&caller)
            .expect("caller request should be valid");

        assert_eq!(validated.delivery_class, DeliveryClass::MobilitySafe);
        assert_eq!(validated.timeout, Some(Duration::from_secs(5)));
        assert_eq!(validated.priority_hint, Some(200));
        assert_eq!(validated.mobility_constraint, MobilityConstraint::Pinned);
    }

    #[test]
    fn validate_caller_rejects_out_of_bounds_delivery_class() {
        let registration =
            ServiceRegistration::new("fabric.echo", contract(), provider_terms()).expect("valid");
        let caller = CallerOptions {
            requested_class: Some(DeliveryClass::ForensicReplayable),
            ..CallerOptions::default()
        };

        let err = registration
            .validate_caller(&caller)
            .expect_err("caller class should be rejected");

        assert_eq!(
            err,
            ServiceContractError::DeliveryClassPolicy(
                DeliveryClassPolicyError::RequestedClassNotAdmissible {
                    requested: DeliveryClass::ForensicReplayable,
                    default_class: DeliveryClass::ObligationBacked,
                }
            )
        );
    }

    #[test]
    fn validate_caller_rejects_timeout_override_when_disabled() {
        let mut contract = contract();
        contract.budget_semantics.allow_timeout_override = false;
        let registration =
            ServiceRegistration::new("fabric.echo", contract, provider_terms()).expect("valid");
        let caller = CallerOptions {
            timeout_override: Some(Duration::from_secs(1)),
            ..CallerOptions::default()
        };

        let err = registration
            .validate_caller(&caller)
            .expect_err("timeout override should be rejected");

        assert_eq!(err, ServiceContractError::TimeoutOverrideNotAllowed);
    }
}
