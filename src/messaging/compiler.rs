//! Deterministic FABRIC validation and cost-estimation compiler scaffolding.

use super::class::DeliveryClass;
use super::ir::{CostVector, FabricIr, FabricIrValidationError, SubjectFamily};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Deterministic compiler for FABRIC IR declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FabricCompiler;

/// Cost estimate emitted for one compiled subject declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledSubjectCost {
    /// Canonical subject pattern.
    pub pattern: String,
    /// Semantic family attached to the subject.
    pub family: SubjectFamily,
    /// Delivery class used as the baseline cost envelope.
    pub delivery_class: DeliveryClass,
    /// Estimated cost envelope for this subject.
    pub estimated_cost: CostVector,
}

impl CompiledSubjectCost {
    fn from_subject(schema: &super::ir::SubjectSchema) -> Self {
        Self {
            pattern: schema.pattern.as_str().to_owned(),
            family: schema.family,
            delivery_class: schema.delivery_class,
            estimated_cost: CostVector::estimate_subject(schema),
        }
    }
}

/// Deterministic compiler output for one FABRIC IR configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FabricCompileReport {
    /// The schema version validated by the compiler.
    pub schema_version: u16,
    /// Per-subject cost estimates in declaration order.
    pub subject_costs: Vec<CompiledSubjectCost>,
    /// Worst-case cost envelope across all declared subjects.
    pub aggregate_cost: CostVector,
}

/// Compiler failures.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum FabricCompilerError {
    /// Structural validation failed before cost estimation could run.
    #[error("FABRIC IR validation failed with {errors_len} error(s)")]
    Validation {
        /// Validation errors reported by the IR validator.
        errors: Vec<FabricIrValidationError>,
        /// Stable count for diagnostics without parsing the error vector.
        errors_len: usize,
    },
}

impl FabricCompiler {
    /// Validate a FABRIC IR document and emit deterministic subject-cost
    /// estimates for explain-plan and operator reporting surfaces.
    pub fn compile(ir: &FabricIr) -> Result<FabricCompileReport, FabricCompilerError> {
        let errors = ir.validate();
        if !errors.is_empty() {
            return Err(FabricCompilerError::Validation {
                errors_len: errors.len(),
                errors,
            });
        }

        let subject_costs = ir
            .subjects
            .iter()
            .map(CompiledSubjectCost::from_subject)
            .collect::<Vec<_>>();
        let aggregate_cost =
            CostVector::max_dimensions(subject_costs.iter().map(|subject| subject.estimated_cost));

        Ok(FabricCompileReport {
            schema_version: ir.schema_version,
            subject_costs,
            aggregate_cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::class::DeliveryClass;
    use crate::messaging::ir::{
        EvidencePolicy, MobilityPermission, PrivacyPolicy, ReplySpaceRule, SubjectFamily,
        SubjectPattern, SubjectSchema,
    };

    #[test]
    fn compiler_rejects_invalid_ir_before_estimating_costs() {
        let ir = FabricIr {
            schema_version: 999,
            ..FabricIr::default()
        };

        let err = FabricCompiler::compile(&ir).expect_err("invalid schema version should fail");
        match err {
            FabricCompilerError::Validation { errors_len, errors } => {
                assert_eq!(errors_len, errors.len());
                assert!(!errors.is_empty());
            }
        }
    }

    #[test]
    fn compiler_emits_subject_costs_and_aggregate_envelope() {
        let subject = SubjectSchema {
            pattern: SubjectPattern::new("tenant.orders.command"),
            family: SubjectFamily::Command,
            delivery_class: DeliveryClass::ObligationBacked,
            evidence_policy: EvidencePolicy::default(),
            privacy_policy: PrivacyPolicy::default(),
            reply_space: Some(ReplySpaceRule::CallerInbox),
            mobility: MobilityPermission::Federated,
            quantitative_obligation: None,
        };
        let ir = FabricIr {
            subjects: vec![subject.clone()],
            ..FabricIr::default()
        };

        let report = FabricCompiler::compile(&ir).expect("valid fabric ir should compile");
        assert_eq!(report.subject_costs.len(), 1);
        assert_eq!(report.subject_costs[0].pattern, subject.pattern.as_str());
        assert_eq!(report.subject_costs[0].family, subject.family);
        assert_eq!(
            report.subject_costs[0].delivery_class,
            subject.delivery_class
        );
        assert_eq!(
            report.subject_costs[0].estimated_cost,
            CostVector::estimate_subject(&subject)
        );
        assert_eq!(
            report.aggregate_cost,
            report.subject_costs[0].estimated_cost
        );
    }
}
