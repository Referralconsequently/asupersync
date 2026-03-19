//! Explain-plan placeholders.

/// Stub anchor for future explain-plan output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExplainPlanPlaceholder {
    /// Human-readable explain-plan label.
    pub name: String,
}
