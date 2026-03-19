//! Delivery-policy placeholders.

/// Stub anchor for future FABRIC policy objects.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeliveryPolicyPlaceholder {
    /// Human-readable policy name.
    pub name: String,
}
