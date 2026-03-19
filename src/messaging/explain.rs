//! Deterministic explain-plan output for FABRIC cost estimation.

use super::compiler::FabricCompileReport;
use super::ir::CostVector;
use serde::{Deserialize, Serialize};

/// One operator-facing cost breakdown row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostBreakdown {
    /// Human-readable entry label.
    pub label: String,
    /// Estimated cost envelope for the entry.
    pub cost: CostVector,
    /// Short rationale explaining the dominant cost drivers.
    pub reasons: Vec<String>,
}

/// Explain-plan payload emitted from a compiled FABRIC IR report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ExplainPlan {
    /// Human-readable explain summary.
    pub summary: String,
    /// Conservative aggregate envelope across all costed entries.
    pub aggregate_cost: CostVector,
    /// Per-entry breakdown in deterministic declaration order.
    pub breakdown: Vec<CostBreakdown>,
}

impl ExplainPlan {
    /// Build an explain plan from a compiler report.
    #[must_use]
    pub fn from_compile_report(report: &FabricCompileReport) -> Self {
        let breakdown = report
            .subject_costs
            .iter()
            .map(|subject| CostBreakdown {
                label: subject.pattern.clone(),
                cost: subject.estimated_cost,
                reasons: vec![
                    format!("family={}", subject.family.as_str()),
                    format!("delivery_class={}", subject.delivery_class),
                ],
            })
            .collect::<Vec<_>>();

        Self {
            summary: format!(
                "Compiled {} FABRIC subject declaration(s) into deterministic cost envelopes",
                report.subject_costs.len()
            ),
            aggregate_cost: report.aggregate_cost,
            breakdown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::class::DeliveryClass;
    use crate::messaging::compiler::{CompiledSubjectCost, FabricCompileReport};
    use crate::messaging::ir::{CostVector, SubjectFamily};

    #[test]
    fn explain_plan_includes_cost_breakdown_for_every_subject() {
        let cost = CostVector::baseline_for_delivery_class(DeliveryClass::DurableOrdered);
        let report = FabricCompileReport {
            schema_version: 1,
            subject_costs: vec![CompiledSubjectCost {
                pattern: "tenant.orders.stream".to_owned(),
                family: SubjectFamily::Event,
                delivery_class: DeliveryClass::DurableOrdered,
                estimated_cost: cost,
            }],
            aggregate_cost: cost,
        };

        let plan = ExplainPlan::from_compile_report(&report);
        assert_eq!(plan.aggregate_cost, cost);
        assert_eq!(plan.breakdown.len(), 1);
        assert_eq!(plan.breakdown[0].label, "tenant.orders.stream");
        assert!(
            plan.breakdown[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("delivery_class=durable-ordered"))
        );
    }
}
