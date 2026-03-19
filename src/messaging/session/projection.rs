//! Session-projection placeholders.

/// Stub anchor for future local-type projection plans.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectionPlanPlaceholder {
    /// Human-readable projection name.
    pub name: String,
}
