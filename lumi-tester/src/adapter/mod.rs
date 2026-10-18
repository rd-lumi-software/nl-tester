mod cloud_mock;
mod lifecycle;
mod registry;
mod runner;
mod session;
mod traits;

pub use cloud_mock::CloudMockAdapter;
pub use registry::{AdapterRegistry, AdapterRegistryError};
pub use runner::{MultiTargetRunOptions, MultiTargetRunReport, MultiTargetRunner, OperationRecord};
pub use session::{SessionEvent, TargetSession};
pub use traits::{AdapterResult, AdapterStatus, TargetAdapter, TargetScope};
