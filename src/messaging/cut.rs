//! Certified-cut placeholders.

/// Stub anchor for future cut and branch coordination policy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CutPolicyPlaceholder {
    /// Human-readable cut-policy name.
    pub name: String,
}
