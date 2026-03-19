//! Protocol-adapter placeholders.

/// Stub anchor for future protocol compatibility adapters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtocolAdapterPlaceholder {
    /// Human-readable adapter name.
    pub name: String,
}
