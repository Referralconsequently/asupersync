//! Snapshot coordination placeholders.

/// Stub anchor for future snapshot and restore coordination.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotCoordinationPlaceholder {
    /// Human-readable snapshot label.
    pub name: String,
}
