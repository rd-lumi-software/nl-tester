//! Protocol-neutral Home Controller domain and backend extension boundary.

mod backend;
pub mod backends;
mod command;
mod factory;
pub mod profile;
mod result;

pub use backend::{HcBackendConfig, HcExecutionScope, HcProtocolBackend};
pub use command::{HcCommand, HcStateUpdate};
pub use factory::{
    HcBackendConstructor, HcBackendDescriptor, HcBackendFactory, HcBackendFactoryError,
    HcBackendRegistry, V3_IMPLEMENTATION_ID, V3_PACK_VERSION, V3_PROTOCOL_PACK_ID,
    V3_PROTOCOL_VERSION,
};
pub use result::{
    HcBackendResult, HcCapability, HcEntity, HcEvidence, HcEvidenceTier, HcObservation, HcState,
    HcStatus,
};
