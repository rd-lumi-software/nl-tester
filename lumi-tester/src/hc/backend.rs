use super::command::HcCommand;
use super::result::{HcBackendResult, HcCapability};
use crate::runner::context::TestContext;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Identity and opaque runtime settings passed to a backend constructor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HcBackendConfig {
    pub implementation: String,
    pub protocol_pack: String,
    pub protocol_version: String,
    pub pack_version: String,
    pub config: Value,
}

impl HcBackendConfig {
    /// Construct a backend configuration while preserving opaque settings.
    pub fn new(
        implementation: impl Into<String>,
        protocol_pack: impl Into<String>,
        protocol_version: impl Into<String>,
        pack_version: impl Into<String>,
        config: Value,
    ) -> Self {
        Self {
            implementation: implementation.into(),
            protocol_pack: protocol_pack.into(),
            protocol_version: protocol_version.into(),
            pack_version: pack_version.into(),
            config,
        }
    }
}

/// HC-local execution metadata, independent from multi-target runner scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcExecutionScope {
    pub depth: usize,
    pub command_index: usize,
    pub correlation_id: String,
}

impl HcExecutionScope {
    /// Construct an HC execution scope from runner-provided metadata.
    pub fn new(depth: usize, command_index: usize, correlation_id: impl Into<String>) -> Self {
        Self {
            depth,
            command_index,
            correlation_id: correlation_id.into(),
        }
    }

    /// Return the nested execution depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Return the zero-based command index.
    pub fn command_index(&self) -> usize {
        self.command_index
    }

    /// Return the correlation identifier for this operation.
    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }
}

/// Extension boundary for protocol-specific HC implementations.
pub trait HcProtocolBackend: Send {
    /// Return the registered implementation identity.
    fn implementation_id(&self) -> &str;

    /// Return the registered protocol-pack identity.
    fn protocol_pack(&self) -> &str;

    /// Return capabilities advertised by this backend instance.
    fn capabilities(&self) -> &[HcCapability];

    /// Check availability before opening a session.
    fn preflight(&mut self, context: &TestContext) -> HcBackendResult;

    /// Open backend resources for a test flow.
    fn open(&mut self, context: &TestContext) -> HcBackendResult;

    /// Execute one protocol-neutral HC command.
    fn execute(&mut self, command: &HcCommand, scope: &HcExecutionScope) -> HcBackendResult;

    /// Release backend resources for a test flow.
    fn close(&mut self, context: &TestContext) -> HcBackendResult;
}
