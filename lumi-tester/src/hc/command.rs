use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A protocol-neutral Home Controller operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HcCommand {
    /// Check that the Home Controller is ready.
    Health,
    /// Discover the entities exposed by the Home Controller.
    GetEntities,
    /// Read the current state for one entity identifier.
    GetState { entity: String },
    /// Apply state changes to one entity identifier.
    Control {
        entity: String,
        updates: Vec<HcStateUpdate>,
    },
    /// Wait for the requested state changes to be observed.
    WaitState {
        entity: String,
        expected: Vec<HcStateUpdate>,
    },
    /// Subscribe to protocol-neutral realtime channels.
    RealtimeSubscribe { channels: Vec<String> },
    /// Wait for a protocol-neutral realtime event for an entity.
    RealtimeExpect { entity: String, event: Value },
    /// Request a protocol-neutral realtime snapshot for an entity.
    RealtimeSnapshot { entity: String },
}

impl HcCommand {
    /// Return the stable YAML command name represented by this operation.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Health => "hcHealth",
            Self::GetEntities => "hcGetEntities",
            Self::GetState { .. } => "hcGetState",
            Self::Control { .. } => "hcControl",
            Self::WaitState { .. } => "hcWaitState",
            Self::RealtimeSubscribe { .. } => "wsSubscribe",
            Self::RealtimeExpect { .. } => "wsExpect",
            Self::RealtimeSnapshot { .. } => "wsSnapshot",
        }
    }

    /// Construct the protocol-neutral operation represented by `wsSubscribe`.
    pub fn realtime_subscribe(channels: Vec<String>) -> Self {
        Self::RealtimeSubscribe { channels }
    }

    /// Construct the protocol-neutral operation represented by `wsExpect`.
    pub fn realtime_expect(entity: impl Into<String>, event: Value) -> Self {
        Self::RealtimeExpect {
            entity: entity.into(),
            event,
        }
    }

    /// Construct the protocol-neutral operation represented by `wsSnapshot`.
    pub fn realtime_snapshot(entity: impl Into<String>) -> Self {
        Self::RealtimeSnapshot {
            entity: entity.into(),
        }
    }
}

/// A state update that remains meaningful independently of a wire protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HcStateUpdate {
    /// An indexed state value from the current YAML/parser contract.
    Indexed { idx: u64, value: Value },
    /// A semantic capability value for a backend that advertises its support.
    Capability { name: String, value: Value },
}
