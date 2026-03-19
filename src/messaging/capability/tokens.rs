//! Capability token placeholders.

/// Stub anchor for future publish-permit and lease tokens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublishPermitPlaceholder {
    /// Human-readable token family.
    pub family: String,
}
