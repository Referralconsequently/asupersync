//! Federation-role placeholders.

/// Top-level federation roles reserved by the FABRIC design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FederationRole {
    /// Constrained leaf fabric.
    LeafFabric,
    /// Gateway between fabrics.
    GatewayFabric,
    /// Replication-oriented bridge.
    ReplicationLink,
    /// Replay- and evidence-oriented bridge.
    EdgeReplayLink,
}
