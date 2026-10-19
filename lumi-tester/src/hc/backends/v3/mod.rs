pub mod runtime;

use crate::hc::backends::v3::runtime::{profile, result, types, V3Runtime};
use crate::hc::{
    HcBackendConfig, HcBackendResult, HcCapability, HcCommand, HcEntity, HcEvidence,
    HcEvidenceTier, HcExecutionScope, HcObservation, HcProtocolBackend, HcState, HcStateUpdate,
};
use crate::runner::context::TestContext;
use result::{V3Result, V3Status, EvidenceTier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use types::{
    V3Command, V3EntityParams, V3ExpectParams, V3IndexedValue, V3StateParams,
    V3SubscribeParams,
};

/// The protocol-neutral V3 backend backed by the existing V3 runtime.
pub struct V3HcProtocolBackend {
    config: HcBackendConfig,
    runtime: Option<V3Runtime>,
    capabilities: Vec<HcCapability>,
}

impl V3HcProtocolBackend {
    /// Construct a V3 backend without opening network or WebSocket resources.
    pub fn new(config: &HcBackendConfig) -> Self {
        Self {
            config: config.clone(),
            runtime: None,
            capabilities: vec![
                HcCapability::new("health", json!(true)),
                HcCapability::new("device_discovery", json!(true)),
                HcCapability::new("device_state", json!(true)),
                HcCapability::new("device_control", json!(true)),
                HcCapability::new("realtime_cell_readback", json!(true)),
            ],
        }
    }

    /// Convert one protocol-neutral command into its V3 command.
    pub fn map_command(command: &HcCommand) -> Result<V3Command, String> {
        match command {
            HcCommand::Health => Ok(V3Command::HcHealth),
            HcCommand::GetEntities => Ok(V3Command::HcGetEntities),
            HcCommand::GetState { entity } => Ok(V3Command::HcGetState(V3EntityParams {
                entity: entity.clone(),
            })),
            HcCommand::Control { entity, updates } => Ok(V3Command::HcControl(V3StateParams {
                entity: entity.clone(),
                state: map_updates(updates)?,
            })),
            HcCommand::WaitState { entity, expected } => {
                Ok(V3Command::HcWaitState(V3StateParams {
                    entity: entity.clone(),
                    state: map_updates(expected)?,
                }))
            }
            HcCommand::RealtimeSubscribe { channels } => {
                Ok(V3Command::WsSubscribe(V3SubscribeParams {
                    channels: channels.clone(),
                }))
            }
            HcCommand::RealtimeExpect { entity, event } => {
                Ok(V3Command::WsExpect(V3ExpectParams {
                    entity: entity.clone(),
                    event: serde_yaml::to_value(event).map_err(|error| error.to_string())?,
                }))
            }
            HcCommand::RealtimeSnapshot { entity } => Ok(V3Command::WsSnapshot(V3EntityParams {
                entity: entity.clone(),
            })),
        }
    }

    fn build_runtime(&self, context: &TestContext) -> Result<V3Runtime, String> {
        let source_path = self
            .config
            .config
            .get("source_path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| {
                "real HC endpoint environment is unresolved: flow source path is unavailable"
                    .to_string()
            })?;
        let profile_ref = self
            .config
            .config
            .get("profile_ref")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                "real HC endpoint environment is unresolved: no profile was declared".to_string()
            })?;
        let loaded = profile::load_profile(&source_path, profile_ref)
            .map_err(|error| format!("real HC runtime preflight is unavailable: {error}"))?;
        let base_url = context.substitute_vars(&loaded.profile.endpoint.base_url);
        let websocket_url = context.substitute_vars(&loaded.profile.endpoint.websocket_url);
        if base_url.contains("${") || websocket_url.contains("${") {
            return Err(
                "real HC endpoint environment is unresolved; real HC lab execution is blocked"
                    .into(),
            );
        }
        V3Runtime::new(
            &loaded,
            base_url,
            websocket_url,
            Duration::from_millis(context.default_timeout_ms),
        )
        .map_err(|error| format!("real HC runtime preflight is unavailable: {error}"))
    }
}

impl HcProtocolBackend for V3HcProtocolBackend {
    fn implementation_id(&self) -> &str {
        &self.config.implementation
    }

    fn protocol_pack(&self) -> &str {
        &self.config.protocol_pack
    }

    fn capabilities(&self) -> &[HcCapability] {
        &self.capabilities
    }

    fn preflight(&mut self, context: &TestContext) -> HcBackendResult {
        if self.runtime.is_none() {
            match self.build_runtime(context) {
                Ok(runtime) => self.runtime = Some(runtime),
                Err(message) => {
                    return HcBackendResult::blocked("NoLab", message)
                        .with_evidence_tier(HcEvidenceTier::RealPreflight)
                }
            }
        }
        HcBackendResult::passed("real HC runtime preflight completed")
            .with_evidence_tier(HcEvidenceTier::RealPreflight)
    }

    fn open(&mut self, context: &TestContext) -> HcBackendResult {
        self.preflight(context)
    }

    fn execute(&mut self, command: &HcCommand, _scope: &HcExecutionScope) -> HcBackendResult {
        let Some(runtime) = self.runtime.as_mut() else {
            return HcBackendResult::blocked(
                "no_lab",
                "real HC execution is unavailable in this session",
            )
            .with_evidence_tier(HcEvidenceTier::RealPreflight);
        };
        let legacy_command = match Self::map_command(command) {
            Ok(command) => command,
            Err(message) => {
                return HcBackendResult::error("invalid_payload", message)
                    .with_evidence_tier(HcEvidenceTier::RealE2e)
            }
        };
        let result = block_on_runtime(runtime.execute(&legacy_command));
        let mut translated = translate_v3_result(result);
        translated.observation = observation_for(command, translated.normalized.as_ref());
        translated
    }

    fn close(&mut self, _context: &TestContext) -> HcBackendResult {
        self.runtime = None;
        HcBackendResult::passed("HC backend closed")
    }
}

/// Translate a V3 result into the protocol-neutral HC result shape.
pub fn translate_v3_result(result: V3Result) -> HcBackendResult {
    let status = match result.status {
        V3Status::Passed => crate::hc::HcStatus::Passed,
        V3Status::Failed => crate::hc::HcStatus::Failed,
        V3Status::Blocked => crate::hc::HcStatus::Blocked,
        V3Status::Error => crate::hc::HcStatus::Error,
    };
    let category = serde_json::to_value(&result.category)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "e2e".into());
    let readback_match = result
        .assertion
        .as_ref()
        .and_then(|assertion| assertion.get("readbackMatches"))
        .and_then(Value::as_bool);
    let evidence = result
        .evidence
        .iter()
        .map(|record| {
            HcEvidence::raw(
                record.kind.clone(),
                serde_json::to_value(record).unwrap_or(Value::Null),
            )
        })
        .collect();
    HcBackendResult {
        status,
        category,
        message: result.message,
        action: result.action,
        readback: result.readback,
        assertion: result.assertion,
        evidence,
        normalized: result.normalized,
        evidence_tier: map_evidence_tier(result.evidence_tier),
        observation: None,
        readback_match,
    }
}

fn map_updates(updates: &[HcStateUpdate]) -> Result<Vec<V3IndexedValue>, String> {
    updates
        .iter()
        .map(|update| match update {
            HcStateUpdate::Indexed { idx, value } => Ok(V3IndexedValue {
                idx: *idx,
                value: serde_yaml::to_value(value).map_err(|error| error.to_string())?,
            }),
            HcStateUpdate::Capability { name, .. } => Err(format!(
                "V3 HC backend does not support capability state update '{name}'"
            )),
        })
        .collect()
}

fn map_evidence_tier(tier: EvidenceTier) -> HcEvidenceTier {
    match tier {
        EvidenceTier::MockContract => HcEvidenceTier::MockContract,
        EvidenceTier::MockConformance => HcEvidenceTier::MockConformance,
        EvidenceTier::RealPreflight => HcEvidenceTier::RealPreflight,
        EvidenceTier::RealE2e => HcEvidenceTier::RealE2e,
    }
}

fn block_on_runtime<F: std::future::Future>(future: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        tokio::runtime::Runtime::new()
            .expect("HC backend runtime")
            .block_on(future)
    }
}

fn observation_for(command: &HcCommand, normalized: Option<&Value>) -> Option<HcObservation> {
    let normalized = normalized?;
    match command {
        HcCommand::GetEntities => discovery_observation(normalized),
        HcCommand::GetState { entity }
        | HcCommand::Control { entity, .. }
        | HcCommand::WaitState { entity, .. }
        | HcCommand::RealtimeExpect { entity, .. }
        | HcCommand::RealtimeSnapshot { entity } => state_observation(entity, normalized),
        HcCommand::Health | HcCommand::RealtimeSubscribe { .. } => None,
    }
}

fn discovery_observation(normalized: &Value) -> Option<HcObservation> {
    let entities = normalized
        .as_array()?
        .iter()
        .map(|item| {
            let raw = item.get("raw").unwrap_or(item);
            let entity = item.get("entity").unwrap_or(item);
            let id = raw
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| entity.get("entity_id").and_then(Value::as_str))?
                .to_string();
            let name = raw.get("name").and_then(Value::as_str).map(str::to_owned);
            Some(HcEntity {
                id,
                name,
                capabilities: raw_capabilities(raw),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(HcObservation::discovery(entities))
}

fn state_observation(entity: &str, normalized: &Value) -> Option<HcObservation> {
    let updates = normalized
        .as_array()?
        .iter()
        .map(|item| {
            let idx = item
                .get("slot_idx")
                .and_then(Value::as_u64)
                .or_else(|| item.get("vector_index").and_then(Value::as_u64))?;
            Some(HcStateUpdate::Indexed {
                idx,
                value: item.get("value")?.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(HcObservation {
        entities: None,
        entity: None,
        state: Some(HcState {
            entity: entity.to_string(),
            updates,
        }),
        value: None,
    })
}

fn raw_capabilities(raw: &Value) -> Vec<HcCapability> {
    raw.get("capabilities")
        .and_then(Value::as_array)
        .map(|capabilities| {
            capabilities
                .iter()
                .filter_map(|capability| {
                    capability
                        .as_str()
                        .map(|name| HcCapability::new(name, json!(true)))
                        .or_else(|| {
                            Some(HcCapability::new(
                                capability.get("name")?.as_str()?,
                                capability.get("value").cloned().unwrap_or(Value::Null),
                            ))
                        })
                })
                .collect()
        })
        .unwrap_or_default()
}
