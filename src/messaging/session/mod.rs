//! Session-contract surfaces for FABRIC.

pub mod conformance;
pub mod contract;
pub mod obligation;
pub mod projection;
pub mod synthesis;

pub use conformance::ConformanceMonitorPlaceholder;
pub use contract::{
    CompensationPath, CutoffPath, EvidenceCheckpoint, GlobalSessionType, Label, MessageType,
    ProtocolContract, ProtocolContractValidationError, RoleName, SessionBranch, SessionPath,
    SessionType, TimeoutLaw, TimeoutOverride,
};
pub use obligation::DerivedObligationPlaceholder;
pub use projection::ProjectionPlanPlaceholder;
pub use synthesis::SynthesizedHandlerPlaceholder;
