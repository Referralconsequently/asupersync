//! FABRIC consumer and cursor-lease placeholders.

/// Stub anchor for future consumer-policy and cursor-lease state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FabricConsumerPlaceholder {
    /// Human-readable consumer name.
    pub name: String,
}
