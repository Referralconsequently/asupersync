//! Session-contract placeholders for FABRIC.

pub mod conformance;
pub mod contract;
pub mod obligation;
pub mod projection;
pub mod synthesis;

pub use conformance::ConformanceMonitorPlaceholder;
pub use contract::ProtocolContractPlaceholder;
pub use obligation::DerivedObligationPlaceholder;
pub use projection::ProjectionPlanPlaceholder;
pub use synthesis::SynthesizedHandlerPlaceholder;
