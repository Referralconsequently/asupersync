//! Typed namespace morphisms and facet-checked certificates for FABRIC.
//!
//! This module keeps the morphism surface finite and inspectable. A morphism
//! declares how one subject language lowers into another, what authority it
//! carries, whether the rewrite is reversible, what privacy and sharing rules
//! apply, and what quota envelope bounds the handoff.

use super::ir::{EvidencePolicy, PrivacyPolicy};
use super::subject::SubjectPattern;
use crate::util::DetHasher;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::hash::Hasher;
use std::time::Duration;
use thiserror::Error;

/// Classification for a subject-language morphism.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum MorphismClass {
    /// Reversible, reply-authoritative, capability-bearing rewrites.
    Authoritative,
    /// Redacting or summarizing rewrites that must not originate authority.
    #[default]
    DerivedView,
    /// One-way export into a weaker trust or replay domain.
    Egress,
    /// Temporary sub-language delegation with bounded duration and revocation.
    Delegation,
}

impl MorphismClass {
    /// Exhaustive morphism-class taxonomy.
    pub const ALL: [Self; 4] = [
        Self::Authoritative,
        Self::DerivedView,
        Self::Egress,
        Self::Delegation,
    ];
}

/// Capability required to install or execute a morphism.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FabricCapability {
    /// Rewrite or normalize a subject namespace.
    #[default]
    RewriteNamespace,
    /// Move authority across a morphism boundary.
    CarryAuthority,
    /// Rebind replies as authoritative responses.
    ReplyAuthority,
    /// Attach or inspect evidence produced by the morphism.
    ObserveEvidence,
    /// Delegate a bounded sub-language to another actor or steward.
    DelegateNamespace,
    /// Export traffic into a weaker or cross-boundary domain.
    CrossBoundaryEgress,
}

impl FabricCapability {
    /// Exhaustive capability taxonomy for the morphism surface.
    pub const ALL: [Self; 6] = [
        Self::RewriteNamespace,
        Self::CarryAuthority,
        Self::ReplyAuthority,
        Self::ObserveEvidence,
        Self::DelegateNamespace,
        Self::CrossBoundaryEgress,
    ];

    /// Return true when the capability moves or rebinds authority.
    #[must_use]
    pub const fn is_authority_bearing(self) -> bool {
        matches!(
            self,
            Self::CarryAuthority | Self::ReplyAuthority | Self::DelegateNamespace
        )
    }
}

/// Reversibility promise attached to a morphism.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ReversibilityRequirement {
    /// The rewrite is explainable through retained evidence but not bijective.
    #[default]
    EvidenceBacked,
    /// The rewrite is structurally reversible without lossy steps.
    Bijective,
    /// The rewrite is intentionally one-way and may discard information.
    Irreversible,
}

/// Sharing boundary for traffic after the morphism applies.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SharingPolicy {
    /// Keep the rewritten traffic inside the local authority boundary.
    #[default]
    Private,
    /// Share only within a tenant-scoped boundary.
    TenantScoped,
    /// Share across a federated but still policy-bound domain.
    Federated,
    /// Allow public read access to the rewritten output.
    PublicRead,
}

/// Reply-handling rule for the rewritten namespace.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResponsePolicy {
    /// Preserve caller-managed reply semantics.
    #[default]
    PreserveCallerReplies,
    /// Rebind replies as authoritative responses from the morphism destination.
    ReplyAuthoritative,
    /// Forward replies opaquely without rebinding authority.
    ForwardOpaque,
    /// Strip reply semantics entirely.
    StripReplies,
}

/// Finite transform vocabulary for typed namespace rewrites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubjectTransform {
    /// Leave the subject language unchanged.
    #[default]
    Identity,
    /// Rename one namespace prefix to another.
    RenamePrefix {
        /// Source prefix to rewrite.
        from: SubjectPattern,
        /// Target prefix after rewriting.
        to: SubjectPattern,
    },
    /// Redact literal subject segments while preserving shape.
    RedactLiterals,
    /// Collapse the tail of a subject after keeping the leading prefix.
    SummarizeTail {
        /// Number of leading segments that remain visible.
        preserve_segments: usize,
    },
    /// Hash-partition the rewritten stream into a bounded number of buckets.
    HashPartition {
        /// Number of output buckets.
        buckets: u16,
    },
    /// Capture a single wildcard expansion by its 1-based index.
    WildcardCapture {
        /// 1-based wildcard/capture index.
        index: usize,
    },
    /// Deterministically hash one or more captured tokens into a bucket.
    DeterministicHash {
        /// Number of output buckets.
        buckets: u16,
        /// 1-based token indices used as the hash key. Empty means all tokens.
        source_indices: Vec<usize>,
    },
    /// Split a captured token and keep a bounded slice of the resulting pieces.
    SplitSlice {
        /// 1-based token index to split.
        index: usize,
        /// Delimiter used to split the selected token.
        delimiter: String,
        /// Zero-based starting piece after splitting.
        start: usize,
        /// Number of split pieces to keep.
        len: usize,
    },
    /// Keep the left-most characters from a captured token.
    LeftExtract {
        /// 1-based token index to project.
        index: usize,
        /// Number of characters to keep from the left.
        len: usize,
    },
    /// Keep the right-most characters from a captured token.
    RightExtract {
        /// 1-based token index to project.
        index: usize,
        /// Number of characters to keep from the right.
        len: usize,
    },
    /// Compose a finite sequence of transforms into a deterministic pipeline.
    Compose {
        /// Ordered transform pipeline.
        steps: Vec<SubjectTransform>,
    },
}

impl SubjectTransform {
    /// Return true when the transform intentionally discards information.
    #[must_use]
    pub fn is_lossy(&self) -> bool {
        match self {
            Self::Identity | Self::RenamePrefix { .. } => false,
            Self::Compose { steps } => steps.iter().any(Self::is_lossy),
            Self::RedactLiterals
            | Self::SummarizeTail { .. }
            | Self::HashPartition { .. }
            | Self::WildcardCapture { .. }
            | Self::DeterministicHash { .. }
            | Self::SplitSlice { .. }
            | Self::LeftExtract { .. }
            | Self::RightExtract { .. } => true,
        }
    }

    /// Return true when the transform admits a structural inverse.
    #[must_use]
    pub fn is_invertible(&self) -> bool {
        self.inverse().is_some()
    }

    /// Return the structural inverse when the transform is bijective.
    #[must_use]
    pub fn inverse(&self) -> Option<Self> {
        match self {
            Self::Identity => Some(Self::Identity),
            Self::RenamePrefix { from, to } => Some(Self::RenamePrefix {
                from: to.clone(),
                to: from.clone(),
            }),
            Self::Compose { steps } => {
                let mut inverse_steps = Vec::with_capacity(steps.len());
                for step in steps.iter().rev() {
                    inverse_steps.push(step.inverse()?);
                }
                Some(Self::Compose {
                    steps: inverse_steps,
                })
            }
            Self::RedactLiterals
            | Self::SummarizeTail { .. }
            | Self::HashPartition { .. }
            | Self::WildcardCapture { .. }
            | Self::DeterministicHash { .. }
            | Self::SplitSlice { .. }
            | Self::LeftExtract { .. }
            | Self::RightExtract { .. } => None,
        }
    }

    /// Apply the transform deterministically to a token vector.
    ///
    /// Higher layers can feed this with wildcard captures or a tokenized
    /// concrete subject depending on which facet they are evaluating.
    pub fn apply_tokens(&self, tokens: &[String]) -> Result<Vec<String>, MorphismEvaluationError> {
        match self {
            Self::Identity => Ok(tokens.to_vec()),
            Self::RenamePrefix { from, to } => {
                let from_literals = literal_only_segments(from)?;
                let to_literals = literal_only_segments(to)?;
                if tokens.starts_with(&from_literals) {
                    let mut rewritten = to_literals;
                    rewritten.extend_from_slice(&tokens[from_literals.len()..]);
                    Ok(rewritten)
                } else {
                    Ok(tokens.to_vec())
                }
            }
            Self::RedactLiterals => Ok(tokens.iter().map(|_| String::from("_")).collect()),
            Self::SummarizeTail { preserve_segments } => {
                if tokens.len() <= *preserve_segments {
                    return Ok(tokens.to_vec());
                }
                let mut summarized = tokens[..*preserve_segments].to_vec();
                summarized.push(String::from("..."));
                Ok(summarized)
            }
            Self::HashPartition { buckets } => Ok(vec![
                deterministic_bucket(tokens, &[], *buckets)?.to_string(),
            ]),
            Self::WildcardCapture { index } => Ok(vec![select_token(tokens, *index)?.to_owned()]),
            Self::DeterministicHash {
                buckets,
                source_indices,
            } => Ok(vec![
                deterministic_bucket(tokens, source_indices, *buckets)?.to_string(),
            ]),
            Self::SplitSlice {
                index,
                delimiter,
                start,
                len,
            } => {
                let token = select_token(tokens, *index)?;
                let pieces = token.split(delimiter).collect::<Vec<_>>();
                if *start >= pieces.len() {
                    return Ok(Vec::new());
                }
                let end = start.saturating_add(*len).min(pieces.len());
                Ok(pieces[*start..end]
                    .iter()
                    .map(|piece| (*piece).to_owned())
                    .collect())
            }
            Self::LeftExtract { index, len } => {
                let token = select_token(tokens, *index)?;
                Ok(vec![take_left(token, *len)])
            }
            Self::RightExtract { index, len } => {
                let token = select_token(tokens, *index)?;
                Ok(vec![take_right(token, *len)])
            }
            Self::Compose { steps } => {
                let mut current = tokens.to_vec();
                for step in steps {
                    current = step.apply_tokens(&current)?;
                }
                Ok(current)
            }
        }
    }

    fn validate(&self) -> Result<(), MorphismValidationError> {
        match self {
            Self::RenamePrefix { from, to } if from == to => {
                Err(MorphismValidationError::RenamePrefixIdentity)
            }
            Self::SummarizeTail { preserve_segments } if *preserve_segments == 0 => {
                Err(MorphismValidationError::SummarizeTailMustPreserveSegments)
            }
            Self::HashPartition { buckets } if *buckets == 0 => {
                Err(MorphismValidationError::HashPartitionRequiresBuckets)
            }
            Self::WildcardCapture { index } if *index == 0 => {
                Err(MorphismValidationError::WildcardCaptureRequiresIndex)
            }
            Self::DeterministicHash { buckets, .. } if *buckets == 0 => {
                Err(MorphismValidationError::DeterministicHashRequiresBuckets)
            }
            Self::DeterministicHash { source_indices, .. }
                if source_indices.iter().any(|index| *index == 0) =>
            {
                Err(MorphismValidationError::DeterministicHashIndexMustBePositive)
            }
            Self::SplitSlice { index, .. } if *index == 0 => {
                Err(MorphismValidationError::SplitSliceRequiresIndex)
            }
            Self::SplitSlice { delimiter, .. } if delimiter.is_empty() => {
                Err(MorphismValidationError::SplitSliceRequiresDelimiter)
            }
            Self::SplitSlice { len, .. } if *len == 0 => {
                Err(MorphismValidationError::SplitSliceRequiresLength)
            }
            Self::LeftExtract { index, .. } if *index == 0 => {
                Err(MorphismValidationError::LeftExtractRequiresIndex)
            }
            Self::LeftExtract { len, .. } if *len == 0 => {
                Err(MorphismValidationError::LeftExtractRequiresLength)
            }
            Self::RightExtract { index, .. } if *index == 0 => {
                Err(MorphismValidationError::RightExtractRequiresIndex)
            }
            Self::RightExtract { len, .. } if *len == 0 => {
                Err(MorphismValidationError::RightExtractRequiresLength)
            }
            Self::Compose { steps } if steps.is_empty() => {
                Err(MorphismValidationError::ComposeRequiresSteps)
            }
            Self::Compose { steps } => {
                for step in steps {
                    step.validate()?;
                }
                Ok(())
            }
            Self::Identity
            | Self::RedactLiterals
            | Self::RenamePrefix { .. }
            | Self::SummarizeTail { .. }
            | Self::HashPartition { .. }
            | Self::WildcardCapture { .. }
            | Self::DeterministicHash { .. }
            | Self::SplitSlice { .. }
            | Self::LeftExtract { .. }
            | Self::RightExtract { .. } => Ok(()),
        }
    }
}

/// Cost and handoff envelope for a morphism.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaPolicy {
    /// Maximum multiplicative expansion factor after rewriting.
    pub max_expansion_factor: u16,
    /// Maximum delivery fanout created by the morphism.
    pub max_fanout: u16,
    /// Maximum evidence or observability bytes emitted per decision.
    pub max_observability_bytes: u32,
    /// Maximum duration a delegated morphism may remain active.
    pub max_handoff_duration: Option<Duration>,
    /// Whether the handoff must support explicit revocation.
    pub revocation_required: bool,
}

impl Default for QuotaPolicy {
    fn default() -> Self {
        Self {
            max_expansion_factor: 1,
            max_fanout: 1,
            max_observability_bytes: 4_096,
            max_handoff_duration: None,
            revocation_required: false,
        }
    }
}

impl QuotaPolicy {
    fn validate(&self) -> Result<(), MorphismValidationError> {
        if self.max_expansion_factor == 0 {
            return Err(MorphismValidationError::ZeroMaxExpansionFactor);
        }
        if self.max_fanout == 0 {
            return Err(MorphismValidationError::ZeroMaxFanout);
        }
        if self.max_observability_bytes == 0 {
            return Err(MorphismValidationError::ZeroMaxObservabilityBytes);
        }
        if self
            .max_handoff_duration
            .is_some_and(|duration| duration.is_zero())
        {
            return Err(MorphismValidationError::ZeroMaxHandoffDuration);
        }
        Ok(())
    }
}

/// Typed namespace morphism declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Morphism {
    /// Source subject language accepted by the morphism.
    pub source_language: SubjectPattern,
    /// Destination subject language emitted by the morphism.
    pub dest_language: SubjectPattern,
    /// High-level morphism class.
    pub class: MorphismClass,
    /// Concrete transform algebra element.
    pub transform: SubjectTransform,
    /// Reversibility promise for the rewrite.
    pub reversibility: ReversibilityRequirement,
    /// Capabilities required to authorize the morphism.
    pub capability_requirements: Vec<FabricCapability>,
    /// Sharing boundary for the rewritten output.
    pub sharing_policy: SharingPolicy,
    /// Privacy and metadata disclosure policy.
    pub privacy_policy: PrivacyPolicy,
    /// Reply-handling semantics after rewriting.
    pub response_policy: ResponsePolicy,
    /// Bounded quota envelope for the morphism.
    pub quota_policy: QuotaPolicy,
    /// Evidence policy attached to the morphism.
    pub evidence_policy: EvidencePolicy,
}

impl Default for Morphism {
    fn default() -> Self {
        Self {
            source_language: SubjectPattern::new("fabric.subject.>"),
            dest_language: SubjectPattern::new("fabric.subject.>"),
            class: MorphismClass::DerivedView,
            transform: SubjectTransform::Identity,
            reversibility: ReversibilityRequirement::EvidenceBacked,
            capability_requirements: vec![FabricCapability::RewriteNamespace],
            sharing_policy: SharingPolicy::Private,
            privacy_policy: PrivacyPolicy::default(),
            response_policy: ResponsePolicy::PreserveCallerReplies,
            quota_policy: QuotaPolicy::default(),
            evidence_policy: EvidencePolicy::default(),
        }
    }
}

impl Morphism {
    /// Validate the morphism against class-specific guardrails.
    pub fn validate(&self) -> Result<(), MorphismValidationError> {
        self.transform.validate()?;
        self.quota_policy.validate()?;

        if let Some(duplicate) = duplicate_capability(&self.capability_requirements) {
            return Err(MorphismValidationError::DuplicateCapability(duplicate));
        }
        if self.reversibility == ReversibilityRequirement::Bijective
            && !self.transform.is_invertible()
        {
            return Err(MorphismValidationError::TransformCannotSatisfyBijectiveRequirement);
        }

        match self.class {
            MorphismClass::Authoritative => {
                if self.capability_requirements.is_empty() {
                    return Err(MorphismValidationError::AuthoritativeRequiresCapability);
                }
                if !self
                    .capability_requirements
                    .iter()
                    .copied()
                    .any(FabricCapability::is_authority_bearing)
                {
                    return Err(MorphismValidationError::AuthoritativeRequiresAuthorityCapability);
                }
                if self.response_policy != ResponsePolicy::ReplyAuthoritative {
                    return Err(MorphismValidationError::AuthoritativeRequiresReplyAuthority);
                }
                if self.reversibility == ReversibilityRequirement::Irreversible {
                    return Err(MorphismValidationError::AuthoritativeMustBeReversible);
                }
                if self.transform.is_lossy() {
                    return Err(MorphismValidationError::AuthoritativeTransformMustBeLossless);
                }
                if let SubjectTransform::RenamePrefix { from, to } = &self.transform
                    && (from.has_wildcards() || to.has_wildcards())
                {
                    return Err(MorphismValidationError::AuthoritativeRenameMustBeLiteralOnly);
                }
            }
            MorphismClass::DerivedView => {
                if self
                    .capability_requirements
                    .iter()
                    .copied()
                    .any(FabricCapability::is_authority_bearing)
                {
                    return Err(
                        MorphismValidationError::DerivedViewCannotRequireAuthorityCapability,
                    );
                }
                if self.response_policy == ResponsePolicy::ReplyAuthoritative {
                    return Err(MorphismValidationError::DerivedViewCannotOriginateReplyAuthority);
                }
            }
            MorphismClass::Egress => {
                if self.response_policy != ResponsePolicy::StripReplies {
                    return Err(MorphismValidationError::EgressMustStripReplies);
                }
                if self.reversibility != ReversibilityRequirement::Irreversible {
                    return Err(MorphismValidationError::EgressMustBeIrreversible);
                }
                if self.sharing_policy == SharingPolicy::Private {
                    return Err(MorphismValidationError::EgressMustCrossBoundary);
                }
            }
            MorphismClass::Delegation => {
                if !self
                    .capability_requirements
                    .contains(&FabricCapability::DelegateNamespace)
                {
                    return Err(MorphismValidationError::DelegationRequiresDelegateCapability);
                }
                if self.quota_policy.max_handoff_duration.is_none() {
                    return Err(MorphismValidationError::DelegationMustBeTimeBounded);
                }
                if !self.quota_policy.revocation_required {
                    return Err(MorphismValidationError::DelegationMustBeRevocable);
                }
            }
        }

        Ok(())
    }

    /// Return the authority facet of the morphism.
    #[must_use]
    pub fn authority_facet(&self) -> AuthorityFacet {
        AuthorityFacet {
            class: self.class,
            capability_requirements: canonical_capabilities(&self.capability_requirements),
            response_policy: self.response_policy,
        }
    }

    /// Return the reversibility facet of the morphism.
    #[must_use]
    pub fn reversibility_facet(&self) -> ReversibilityFacet {
        ReversibilityFacet {
            requirement: self.reversibility,
            lossy_transform: self.transform.is_lossy(),
        }
    }

    /// Return the secrecy and metadata-exposure facet of the morphism.
    #[must_use]
    pub fn secrecy_facet(&self) -> SecrecyFacet {
        SecrecyFacet {
            sharing_policy: self.sharing_policy,
            privacy_policy: self.privacy_policy.clone(),
        }
    }

    /// Return the bounded cost and quota facet of the morphism.
    #[must_use]
    pub fn cost_facet(&self) -> CostFacet {
        CostFacet {
            quota_policy: self.quota_policy.clone(),
        }
    }

    /// Return the observability and evidence facet of the morphism.
    #[must_use]
    pub fn observability_facet(&self) -> ObservabilityFacet {
        ObservabilityFacet {
            evidence_policy: self.evidence_policy.clone(),
        }
    }

    /// Return all five independently checkable morphism facets.
    #[must_use]
    pub fn facet_set(&self) -> MorphismFacetSet {
        MorphismFacetSet {
            authority: self.authority_facet(),
            reversibility: self.reversibility_facet(),
            secrecy: self.secrecy_facet(),
            cost: self.cost_facet(),
            observability: self.observability_facet(),
        }
    }

    /// Compile the morphism into a deterministic certificate.
    pub fn compile(&self) -> Result<MorphismCertificate, MorphismValidationError> {
        self.validate()?;

        let bytes = serde_json::to_vec(self)
            .map_err(|error| MorphismValidationError::SerializeCertificate(error.to_string()))?;
        let mut hasher = DetHasher::default();
        hasher.write(&bytes);

        Ok(MorphismCertificate {
            fingerprint: format!("{:016x}", hasher.finish()),
            class: self.class,
            source_language: self.source_language.clone(),
            dest_language: self.dest_language.clone(),
            transform: self.transform.clone(),
            facets: self.facet_set(),
        })
    }
}

/// Independently checkable authority facet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityFacet {
    /// Class that determines the authority envelope.
    pub class: MorphismClass,
    /// Canonicalized capability requirements.
    pub capability_requirements: Vec<FabricCapability>,
    /// Reply-handling policy for the rewritten namespace.
    pub response_policy: ResponsePolicy,
}

/// Independently checkable reversibility facet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReversibilityFacet {
    /// Declared reversibility requirement.
    pub requirement: ReversibilityRequirement,
    /// Whether the chosen transform is intrinsically lossy.
    pub lossy_transform: bool,
}

/// Independently checkable secrecy and metadata-exposure facet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecrecyFacet {
    /// Sharing boundary after the morphism applies.
    pub sharing_policy: SharingPolicy,
    /// Privacy rules for metadata and subject disclosure.
    pub privacy_policy: PrivacyPolicy,
}

/// Independently checkable cost and quota facet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostFacet {
    /// Quota envelope that bounds expansion, fanout, and delegation.
    pub quota_policy: QuotaPolicy,
}

/// Independently checkable observability and evidence facet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservabilityFacet {
    /// Evidence policy emitted by the morphism.
    pub evidence_policy: EvidencePolicy,
}

/// Aggregated view of the five morphism facets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MorphismFacetSet {
    /// Authority and capability requirements.
    pub authority: AuthorityFacet,
    /// Reversibility contract.
    pub reversibility: ReversibilityFacet,
    /// Secrecy and metadata-disclosure policy.
    pub secrecy: SecrecyFacet,
    /// Cost and quota envelope.
    pub cost: CostFacet,
    /// Evidence and observability obligations.
    pub observability: ObservabilityFacet,
}

/// Deterministic compiled artifact for a validated morphism.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MorphismCertificate {
    /// Stable fingerprint over the serialized morphism declaration.
    pub fingerprint: String,
    /// Class of the validated morphism.
    pub class: MorphismClass,
    /// Source language encoded into the certificate.
    pub source_language: SubjectPattern,
    /// Destination language encoded into the certificate.
    pub dest_language: SubjectPattern,
    /// Transform algebra element encoded into the certificate.
    pub transform: SubjectTransform,
    /// Faceted summary used by downstream validators.
    pub facets: MorphismFacetSet,
}

/// Validation failures for typed namespace morphisms.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MorphismValidationError {
    /// Authoritative morphisms must declare at least one capability.
    #[error("authoritative morphisms require at least one capability")]
    AuthoritativeRequiresCapability,
    /// Authoritative morphisms must carry an authority-bearing capability.
    #[error("authoritative morphisms require an authority-bearing capability")]
    AuthoritativeRequiresAuthorityCapability,
    /// Authoritative morphisms must control replies explicitly.
    #[error("authoritative morphisms must use reply-authoritative response policy")]
    AuthoritativeRequiresReplyAuthority,
    /// Authoritative morphisms must not be declared one-way.
    #[error("authoritative morphisms must be reversible")]
    AuthoritativeMustBeReversible,
    /// Authoritative morphisms must not use lossy transforms.
    #[error("authoritative morphisms must use lossless transforms")]
    AuthoritativeTransformMustBeLossless,
    /// Authoritative prefix rewrites must stay literal-only.
    #[error("authoritative rename-prefix morphisms must use literal-only patterns")]
    AuthoritativeRenameMustBeLiteralOnly,
    /// Derived views must not require authority-bearing capabilities.
    #[error("derived-view morphisms must not require authority-bearing capabilities")]
    DerivedViewCannotRequireAuthorityCapability,
    /// Derived views must not rebind replies as authority.
    #[error("derived-view morphisms must not originate reply authority")]
    DerivedViewCannotOriginateReplyAuthority,
    /// Egress morphisms must strip reply semantics.
    #[error("egress morphisms must strip replies")]
    EgressMustStripReplies,
    /// Egress morphisms are intentionally one-way.
    #[error("egress morphisms must be irreversible")]
    EgressMustBeIrreversible,
    /// Egress morphisms must leave the private boundary.
    #[error("egress morphisms must cross a non-private sharing boundary")]
    EgressMustCrossBoundary,
    /// Delegation requires the delegation capability.
    #[error("delegation morphisms require delegate-namespace capability")]
    DelegationRequiresDelegateCapability,
    /// Delegation must declare a finite handoff duration.
    #[error("delegation morphisms must declare a bounded handoff duration")]
    DelegationMustBeTimeBounded,
    /// Delegation must be explicitly revocable.
    #[error("delegation morphisms must be revocable")]
    DelegationMustBeRevocable,
    /// Capability requirements must be unique.
    #[error("duplicate capability requirement `{0:?}`")]
    DuplicateCapability(FabricCapability),
    /// Rename-prefix transforms must actually change the namespace.
    #[error("rename-prefix transform must change the namespace")]
    RenamePrefixIdentity,
    /// Tail summarization must preserve at least one segment.
    #[error("summarize-tail transform must preserve at least one segment")]
    SummarizeTailMustPreserveSegments,
    /// Hash partitioning requires at least one bucket.
    #[error("hash-partition transform requires at least one bucket")]
    HashPartitionRequiresBuckets,
    /// Wildcard capture transforms must name a 1-based index.
    #[error("wildcard-capture transform requires a positive index")]
    WildcardCaptureRequiresIndex,
    /// Deterministic hash transforms require at least one bucket.
    #[error("deterministic-hash transform requires at least one bucket")]
    DeterministicHashRequiresBuckets,
    /// Deterministic hash transforms use 1-based token indices.
    #[error("deterministic-hash source indices must be positive")]
    DeterministicHashIndexMustBePositive,
    /// Split-and-slice transforms must name a 1-based token index.
    #[error("split-slice transform requires a positive token index")]
    SplitSliceRequiresIndex,
    /// Split-and-slice transforms need a non-empty delimiter.
    #[error("split-slice transform requires a non-empty delimiter")]
    SplitSliceRequiresDelimiter,
    /// Split-and-slice transforms must keep at least one piece.
    #[error("split-slice transform requires a positive slice length")]
    SplitSliceRequiresLength,
    /// Left-extract transforms must name a 1-based token index.
    #[error("left-extract transform requires a positive token index")]
    LeftExtractRequiresIndex,
    /// Left-extract transforms must keep at least one character.
    #[error("left-extract transform requires a positive length")]
    LeftExtractRequiresLength,
    /// Right-extract transforms must name a 1-based token index.
    #[error("right-extract transform requires a positive token index")]
    RightExtractRequiresIndex,
    /// Right-extract transforms must keep at least one character.
    #[error("right-extract transform requires a positive length")]
    RightExtractRequiresLength,
    /// Compose transforms must contain at least one step.
    #[error("compose transform requires at least one step")]
    ComposeRequiresSteps,
    /// Bijective requirements need an invertible transform.
    #[error("bijective reversibility requires an invertible transform")]
    TransformCannotSatisfyBijectiveRequirement,
    /// Expansion factor must be positive.
    #[error("quota max expansion factor must be greater than zero")]
    ZeroMaxExpansionFactor,
    /// Fanout must be positive.
    #[error("quota max fanout must be greater than zero")]
    ZeroMaxFanout,
    /// Observability budget must be positive.
    #[error("quota max observability bytes must be greater than zero")]
    ZeroMaxObservabilityBytes,
    /// Handoff duration must be positive when present.
    #[error("quota max handoff duration must be greater than zero")]
    ZeroMaxHandoffDuration,
    /// Certificate compilation failed while serializing the morphism.
    #[error("failed to serialize morphism certificate: {0}")]
    SerializeCertificate(String),
}

/// Evaluation failures while executing a deterministic transform pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MorphismEvaluationError {
    /// A transform referenced a token/capture index that is not present.
    #[error("token index {index} is out of range for {available} available tokens")]
    TokenIndexOutOfRange {
        /// 1-based index requested by the transform.
        index: usize,
        /// Number of tokens available to the transform.
        available: usize,
    },
    /// Prefix rewrites can only execute against literal-only patterns.
    #[error("pattern `{0}` must contain only literal segments for evaluation")]
    NonLiteralPattern(String),
    /// Deterministic hashing requires at least one bucket.
    #[error("deterministic bucket count must be greater than zero")]
    ZeroBuckets,
}

fn canonical_capabilities(capabilities: &[FabricCapability]) -> Vec<FabricCapability> {
    capabilities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn duplicate_capability(capabilities: &[FabricCapability]) -> Option<FabricCapability> {
    let mut seen = BTreeSet::new();
    for capability in capabilities {
        if !seen.insert(*capability) {
            return Some(*capability);
        }
    }
    None
}

fn literal_only_segments(pattern: &SubjectPattern) -> Result<Vec<String>, MorphismEvaluationError> {
    pattern
        .segments()
        .iter()
        .map(|segment| match segment {
            super::subject::SubjectToken::Literal(value) => Ok(value.clone()),
            super::subject::SubjectToken::One | super::subject::SubjectToken::Tail => Err(
                MorphismEvaluationError::NonLiteralPattern(pattern.canonical_key()),
            ),
        })
        .collect()
}

fn select_token(tokens: &[String], index: usize) -> Result<&str, MorphismEvaluationError> {
    let offset = index
        .checked_sub(1)
        .ok_or(MorphismEvaluationError::TokenIndexOutOfRange {
            index,
            available: tokens.len(),
        })?;
    tokens
        .get(offset)
        .map(String::as_str)
        .ok_or(MorphismEvaluationError::TokenIndexOutOfRange {
            index,
            available: tokens.len(),
        })
}

fn deterministic_bucket(
    tokens: &[String],
    source_indices: &[usize],
    buckets: u16,
) -> Result<u16, MorphismEvaluationError> {
    if buckets == 0 {
        return Err(MorphismEvaluationError::ZeroBuckets);
    }

    let mut hasher = DetHasher::default();
    if source_indices.is_empty() {
        for token in tokens {
            hasher.write(token.as_bytes());
            hasher.write_u8(0xff);
        }
    } else {
        for index in source_indices {
            hasher.write(select_token(tokens, *index)?.as_bytes());
            hasher.write_u8(0xff);
        }
    }

    Ok((hasher.finish() % u64::from(buckets)) as u16)
}

fn take_left(token: &str, len: usize) -> String {
    let limit = token.chars().count().min(len);
    token.chars().take(limit).collect()
}

fn take_right(token: &str, len: usize) -> String {
    let char_count = token.chars().count();
    let start = char_count.saturating_sub(len);
    token.chars().skip(start).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authoritative_morphism() -> Morphism {
        Morphism {
            source_language: SubjectPattern::new("tenant.orders"),
            dest_language: SubjectPattern::new("authority.orders"),
            class: MorphismClass::Authoritative,
            transform: SubjectTransform::RenamePrefix {
                from: SubjectPattern::new("tenant.orders"),
                to: SubjectPattern::new("authority.orders"),
            },
            reversibility: ReversibilityRequirement::Bijective,
            capability_requirements: vec![
                FabricCapability::CarryAuthority,
                FabricCapability::ReplyAuthority,
            ],
            response_policy: ResponsePolicy::ReplyAuthoritative,
            ..Morphism::default()
        }
    }

    #[test]
    fn authoritative_compile_produces_deterministic_certificate() {
        let morphism = authoritative_morphism();
        let first = morphism.compile().expect("compile certificate");
        let second = morphism.compile().expect("compile certificate twice");

        assert_eq!(first, second);
        assert_eq!(first.class, MorphismClass::Authoritative);
        assert_eq!(
            first.facets.authority.capability_requirements,
            vec![
                FabricCapability::CarryAuthority,
                FabricCapability::ReplyAuthority,
            ]
        );
    }

    #[test]
    fn authoritative_morphisms_reject_lossy_or_wildcard_rewrites() {
        let mut lossy = authoritative_morphism();
        lossy.transform = SubjectTransform::RedactLiterals;
        assert_eq!(
            lossy.validate(),
            Err(MorphismValidationError::AuthoritativeTransformMustBeLossless)
        );

        let mut wildcard = authoritative_morphism();
        wildcard.transform = SubjectTransform::RenamePrefix {
            from: SubjectPattern::new("tenant.*"),
            to: SubjectPattern::new("authority.orders"),
        };
        assert_eq!(
            wildcard.validate(),
            Err(MorphismValidationError::AuthoritativeRenameMustBeLiteralOnly)
        );
    }

    #[test]
    fn delegation_requires_delegate_capability_bounded_duration_and_revocation() {
        let mut delegation = Morphism {
            class: MorphismClass::Delegation,
            response_policy: ResponsePolicy::ForwardOpaque,
            ..Morphism::default()
        };

        assert_eq!(
            delegation.validate(),
            Err(MorphismValidationError::DelegationRequiresDelegateCapability)
        );

        delegation.capability_requirements = vec![FabricCapability::DelegateNamespace];
        assert_eq!(
            delegation.validate(),
            Err(MorphismValidationError::DelegationMustBeTimeBounded)
        );

        delegation.quota_policy.max_handoff_duration = Some(Duration::from_secs(30));
        assert_eq!(
            delegation.validate(),
            Err(MorphismValidationError::DelegationMustBeRevocable)
        );

        delegation.quota_policy.revocation_required = true;
        assert!(delegation.validate().is_ok());
    }

    #[test]
    fn egress_requires_stripped_replies_and_one_way_reversibility() {
        let mut egress = Morphism {
            class: MorphismClass::Egress,
            sharing_policy: SharingPolicy::Federated,
            reversibility: ReversibilityRequirement::Irreversible,
            ..Morphism::default()
        };

        assert_eq!(
            egress.validate(),
            Err(MorphismValidationError::EgressMustStripReplies)
        );

        egress.response_policy = ResponsePolicy::StripReplies;
        egress.reversibility = ReversibilityRequirement::EvidenceBacked;
        assert_eq!(
            egress.validate(),
            Err(MorphismValidationError::EgressMustBeIrreversible)
        );

        egress.reversibility = ReversibilityRequirement::Irreversible;
        egress.sharing_policy = SharingPolicy::Private;
        assert_eq!(
            egress.validate(),
            Err(MorphismValidationError::EgressMustCrossBoundary)
        );
    }

    #[test]
    fn facet_views_change_independently() {
        let base = Morphism::default();

        let mut cost_variant = base.clone();
        cost_variant.quota_policy.max_fanout = 8;
        assert_eq!(base.authority_facet(), cost_variant.authority_facet());
        assert_eq!(
            base.reversibility_facet(),
            cost_variant.reversibility_facet()
        );
        assert_eq!(base.secrecy_facet(), cost_variant.secrecy_facet());
        assert_ne!(base.cost_facet(), cost_variant.cost_facet());
        assert_eq!(
            base.observability_facet(),
            cost_variant.observability_facet()
        );

        let mut observability_variant = base.clone();
        observability_variant
            .evidence_policy
            .record_counterfactual_branches = true;
        assert_eq!(
            base.authority_facet(),
            observability_variant.authority_facet()
        );
        assert_eq!(base.cost_facet(), observability_variant.cost_facet());
        assert_ne!(
            base.observability_facet(),
            observability_variant.observability_facet()
        );
    }

    #[test]
    fn duplicate_capabilities_fail_closed() {
        let mut morphism = authoritative_morphism();
        morphism.capability_requirements = vec![
            FabricCapability::ReplyAuthority,
            FabricCapability::CarryAuthority,
            FabricCapability::ReplyAuthority,
        ];

        assert_eq!(
            morphism.validate(),
            Err(MorphismValidationError::DuplicateCapability(
                FabricCapability::ReplyAuthority
            ))
        );
    }

    #[test]
    fn wildcard_capture_and_compose_apply_deterministically() {
        let tokens = vec![
            String::from("tenant"),
            String::from("orders-eu"),
            String::from("priority"),
        ];
        let transform = SubjectTransform::Compose {
            steps: vec![
                SubjectTransform::WildcardCapture { index: 2 },
                SubjectTransform::SplitSlice {
                    index: 1,
                    delimiter: String::from("-"),
                    start: 0,
                    len: 1,
                },
            ],
        };

        assert_eq!(
            transform
                .apply_tokens(&tokens)
                .expect("compose should evaluate"),
            vec![String::from("orders")]
        );
        assert!(!transform.is_invertible());
    }

    #[test]
    fn deterministic_hash_is_stable_for_selected_tokens() {
        let tokens = vec![
            String::from("tenant"),
            String::from("region"),
            String::from("user"),
        ];
        let transform = SubjectTransform::DeterministicHash {
            buckets: 32,
            source_indices: vec![1, 3],
        };

        let first = transform
            .apply_tokens(&tokens)
            .expect("hash should evaluate deterministically");
        let second = transform
            .apply_tokens(&tokens)
            .expect("hash should evaluate deterministically twice");
        assert_eq!(first, second);
    }

    #[test]
    fn left_and_right_extract_project_expected_substrings() {
        let tokens = vec![String::from("priority")];
        assert_eq!(
            SubjectTransform::LeftExtract { index: 1, len: 4 }
                .apply_tokens(&tokens)
                .expect("left extract should evaluate"),
            vec![String::from("prio")]
        );
        assert_eq!(
            SubjectTransform::RightExtract { index: 1, len: 4 }
                .apply_tokens(&tokens)
                .expect("right extract should evaluate"),
            vec![String::from("rity")]
        );
    }

    #[test]
    fn bijective_reversibility_rejects_irreversible_transforms() {
        let mut morphism = authoritative_morphism();
        morphism.transform = SubjectTransform::DeterministicHash {
            buckets: 16,
            source_indices: vec![1],
        };
        assert_eq!(
            morphism.validate(),
            Err(MorphismValidationError::TransformCannotSatisfyBijectiveRequirement)
        );
    }

    #[test]
    fn reversible_compose_builds_inverse_in_reverse_order() {
        let transform = SubjectTransform::Compose {
            steps: vec![
                SubjectTransform::RenamePrefix {
                    from: SubjectPattern::new("tenant.orders"),
                    to: SubjectPattern::new("authority.orders"),
                },
                SubjectTransform::Identity,
            ],
        };

        let inverse = transform.inverse().expect("compose should be invertible");
        assert_eq!(
            inverse,
            SubjectTransform::Compose {
                steps: vec![
                    SubjectTransform::Identity,
                    SubjectTransform::RenamePrefix {
                        from: SubjectPattern::new("authority.orders"),
                        to: SubjectPattern::new("tenant.orders"),
                    },
                ],
            }
        );
    }
}
