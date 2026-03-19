//! Region-owned FABRIC stream placeholders.

/// Stub anchor for future region-owned stream state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FabricStreamPlaceholder {
    /// Human-readable stream name.
    pub name: String,
}
