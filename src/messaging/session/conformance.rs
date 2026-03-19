//! Session-conformance placeholders.

/// Stub anchor for future runtime protocol monitors.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConformanceMonitorPlaceholder {
    /// Human-readable monitor name.
    pub name: String,
}
