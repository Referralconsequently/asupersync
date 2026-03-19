//! Capability-aware routing placeholders.

/// Stub anchor for future capability-checked routing policies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilityRoutingPlaceholder {
    /// Human-readable routing policy name.
    pub name: String,
}
