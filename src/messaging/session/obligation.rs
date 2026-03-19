//! Session-obligation placeholders.

/// Stub anchor for future mechanically derived obligations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DerivedObligationPlaceholder {
    /// Human-readable obligation family.
    pub name: String,
}
