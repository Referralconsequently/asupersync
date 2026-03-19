//! System-subject control-plane placeholders.

/// Stub anchor for future control-subject namespaces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ControlSubjectPlaceholder {
    /// Human-readable control family name.
    pub family: String,
}
