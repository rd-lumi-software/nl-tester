pub mod evidence;
pub mod canonical;
pub mod http;
pub mod profile;
pub mod result;
pub mod types;
pub mod websocket;

pub use evidence::{EvidenceBoundaryError, EvidenceRoot, EvidenceWriter};
pub use profile::{validate_flow, V3FlowMetadata, V3ValidationError};
pub use types::{V3Capability, V3Command, V3CommandRequirement};

use anyhow::Result as AnyResult;
use canonical::{CanonicalEntity, DecimalId, EntityRef};
use evidence::{EvidenceConfig, EvidenceRecord};
use http::{HcHttpAdapter, HcHttpError, ReqwestHttpTransport};
use result::{readback_matches, V3Category, V3Result, V3Status, EvidenceTier, RequestedChange};
use serde_json::{json, Value};
use std::io::Write;
use std::time::Duration;
use types::{V3ExpectParams, V3StateParams};
use websocket::{HcWsAdapter, HcWsError, RealWsTransport};

/// The V3 HTTP/WebSocket runtime used by the HC backend.
pub struct V3Runtime {
    http: HcHttpAdapter<ReqwestHttpTransport>,
    websocket: Option<HcWsAdapter<RealWsTransport>>,
    entities: Vec<CanonicalEntity>,
    evidence_writer: EvidenceWriter,
    sequence: u64,
}

impl V3Runtime {
    pub fn new(
        loaded: &profile::LoadedV3Profile,
        base_url: String,
        websocket_url: Option<String>,
        timeout: Duration,
    ) -> AnyResult<Self> {
        if loaded.profile.retry.control_max_attempts != 0 {
            anyhow::bail!(
                "V3 HC controlMaxAttempts must be 0 because control POST is non-retried"
            );
        }
        let config = EvidenceConfig {
            max_raw_bytes: loaded.profile.evidence.max_raw_bytes as usize,
            max_websocket_frames: loaded.profile.evidence.max_websocket_frames as usize,
            redact_fields: loaded.profile.evidence.redact_fields.clone(),
        };
        let http = HcHttpAdapter::new(
            ReqwestHttpTransport::new()?,
            base_url,
            timeout,
            loaded.profile.retry.safe_http_max_attempts,
            evidence::EvidenceCollector::new(config.clone())?,
        )?;
        let websocket = websocket_url
            .map(|websocket_url| {
                HcWsAdapter::new(
                    RealWsTransport,
                    websocket_url,
                    timeout,
                    loaded.profile.retry.websocket_reconnect_max_attempts,
                    evidence::EvidenceCollector::new(config)
                        .map_err(|error| HcWsError::Evidence(error.to_string()))?,
                )
            })
            .transpose()?;
        Ok(Self {
            http,
            websocket,
            entities: Vec::new(),
            evidence_writer: loaded.open_evidence_writer()?,
            sequence: 0,
        })
    }

    pub async fn execute(&mut self, command: &V3Command) -> V3Result {
        let result = match command {
            V3Command::HcHealth => self.health().await,
            V3Command::HcGetEntities => self.discover().await,
            V3Command::HcGetState(params) => self.get_state(params).await,
            V3Command::HcControl(params) => self.control(params).await,
            V3Command::HcWaitState(params) => self.wait_state(params).await,
            V3Command::WsSubscribe(params) => self.subscribe(params).await,
            V3Command::WsExpect(params) => self.expect(params).await,
            V3Command::WsSnapshot(params) => self.snapshot(params).await,
        };
        match result {
            Ok(result) => {
                if result.status == V3Status::Passed {
                    if let Err(error) = self.persist(&result) {
                        return V3Result::error(
                            V3Category::EvidenceError,
                            error.to_string(),
                            EvidenceTier::RealE2e,
                        );
                    }
                }
                result
            }
            Err(result) => result,
        }
    }

    async fn health(&mut self) -> std::result::Result<V3Result, V3Result> {
        let response = self.http.health().await.map_err(|error| http_result(error))?;
        let normalized = json!({ "status": response.status });
        if !response.status {
            return Err(V3Result::failed(
                V3Category::AssertionMismatch,
                "HC health/readiness returned status=false",
                EvidenceTier::RealE2e,
            ));
        }
        Ok(self.pass(
            V3Category::Preflight,
            json!({ "operation": "health" }),
            normalized.clone(),
            json!({ "status": true }),
            vec![response.evidence.request, response.evidence.response],
            normalized,
        ))
    }

    async fn discover(&mut self) -> std::result::Result<V3Result, V3Result> {
        let response = self.http.discover().await.map_err(|error| http_result(error))?;
        self.entities = response.entities.clone();
        let normalized = serde_json::to_value(&response.entities).unwrap_or(Value::Null);
        Ok(self.pass(
            V3Category::Preflight,
            json!({ "operation": "device_discovery" }),
            normalized.clone(),
            json!({ "entitiesFound": response.entities.len() }),
            vec![response.evidence.request, response.evidence.response],
            normalized,
        ))
    }

    async fn get_state(&mut self, params: &types::V3EntityParams) -> std::result::Result<V3Result, V3Result> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let response = self.http.state(&id).await.map_err(|error| http_result(error))?;
        let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
        Ok(self.pass(
            V3Category::Preflight,
            json!({ "operation": "state_read", "entity": params.entity }),
            normalized.clone(),
            json!({ "stateRead": true }),
            vec![response.evidence.request, response.evidence.response],
            normalized,
        ))
    }

    async fn control(&mut self, params: &V3StateParams) -> std::result::Result<V3Result, V3Result> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let changes = params
            .state
            .iter()
            .map(|value| {
                Ok(http::ControlChange {
                    idx: value.idx,
                    value: serde_json::to_value(&value.value)
                        .map_err(|error| V3Result::error(V3Category::InvalidPayload, error.to_string(), EvidenceTier::RealE2e))?,
                })
            })
            .collect::<std::result::Result<Vec<_>, V3Result>>()?;
        let response = self
            .http
            .control(&id, &changes)
            .await
            .map_err(|error| http_result(error))?;
        let normalized = serde_json::to_value(&response.readback.states).unwrap_or(Value::Null);
        let action = json!({
            "operation": "control",
            "entity": params.entity,
            "changes": changes.iter().map(|change| json!({ "idx": change.idx, "value": change.value })).collect::<Vec<_>>(),
        });
        Ok(self.pass(
            V3Category::ControlDispatch,
            action,
            normalized.clone(),
            json!({ "readbackMatches": response.assertion }),
            response.required_evidence,
            normalized,
        ))
    }

    async fn wait_state(&mut self, params: &V3StateParams) -> std::result::Result<V3Result, V3Result> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let requested = requested_changes(params).map_err(|error| invalid_result(error))?;
        for _ in 0..3 {
            let response = self.http.state(&id).await.map_err(|error| http_result(error))?;
            let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
            if readback_matches(&entity_for(&id), &requested, &response.states) {
                return Ok(self.pass(
                    V3Category::AssertionMismatch,
                    json!({ "operation": "wait_state", "entity": params.entity }),
                    normalized.clone(),
                    json!({ "readbackMatches": true }),
                    vec![response.evidence.request, response.evidence.response],
                    normalized,
                ));
            }
        }
        Err(V3Result::failed(
            V3Category::AssertionMismatch,
            "bounded state readback did not match requested state",
            EvidenceTier::RealE2e,
        ))
    }

    async fn subscribe(&mut self, params: &types::V3SubscribeParams) -> std::result::Result<V3Result, V3Result> {
        let websocket = self.websocket.as_mut().ok_or_else(|| {
            V3Result::blocked(
                V3Category::UnsupportedCapability,
                "WebSocket transport is not configured for this HC profile",
            )
        })?;
        websocket
            .subscribe(&params.channels)
            .await
            .map_err(ws_result)?;
        let evidence = websocket.evidence().to_vec();
        let normalized = json!({ "channels": params.channels });
        Ok(self.pass(
            V3Category::Preflight,
            json!({ "operation": "subscribe", "channels": params.channels }),
            normalized.clone(),
            json!({ "acknowledged": true }),
            evidence,
            normalized,
        ))
    }

    async fn expect(&mut self, params: &V3ExpectParams) -> std::result::Result<V3Result, V3Result> {
        let websocket = self.websocket.as_mut().ok_or_else(|| {
            V3Result::blocked(
                V3Category::UnsupportedCapability,
                "WebSocket transport is not configured for this HC profile",
            )
        })?;
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let requested = requested_changes_from_event(&params.event).map_err(|error| invalid_result(error))?;
        let response = websocket
            .expect_state(&entity_for(&id), &requested, Duration::from_secs(10))
            .await
            .map_err(ws_result)?;
        let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
        Ok(self.pass(
            V3Category::AssertionMismatch,
            json!({ "operation": "expect_state", "entity": params.entity, "event": params.event }),
            normalized.clone(),
            json!({ "readbackMatches": true }),
            response.evidence,
            normalized,
        ))
    }

    async fn snapshot(&mut self, params: &types::V3EntityParams) -> std::result::Result<V3Result, V3Result> {
        let websocket = self.websocket.as_mut().ok_or_else(|| {
            V3Result::blocked(
                V3Category::UnsupportedCapability,
                "WebSocket transport is not configured for this HC profile",
            )
        })?;
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let response = websocket
            .snapshot(&entity_for(&id))
            .await
            .map_err(ws_result)?;
        let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
        Ok(self.pass(
            V3Category::ReconnectFallback,
            json!({ "operation": "snapshot", "entity": params.entity }),
            normalized.clone(),
            json!({ "snapshotRead": true }),
            response.evidence,
            normalized,
        ))
    }

    fn pass(
        &self,
        category: V3Category,
        action: Value,
        readback: Value,
        assertion: Value,
        evidence: Vec<EvidenceRecord>,
        normalized: Value,
    ) -> V3Result {
        V3Result::passed(
            category,
            EvidenceTier::RealE2e,
            action,
            readback,
            assertion,
            evidence,
            Some(normalized),
        )
    }

    fn persist(&mut self, result: &V3Result) -> AnyResult<()> {
        self.sequence += 1;
        let stem = format!("e2e-{:04}", self.sequence);
        let raw = serde_json::to_vec_pretty(&result.evidence)?;
        let normalized = serde_json::to_vec_pretty(
            result.normalized.as_ref().unwrap_or(&Value::Null),
        )?;
        let mut raw_file = self.evidence_writer.create_raw_file(format!("{stem}.json"))?;
        raw_file.write_all(&raw)?;
        let mut normalized_file = self
            .evidence_writer
            .create_normalized_file(format!("{stem}.json"))?;
        normalized_file.write_all(&normalized)?;
        Ok(())
    }
}

fn parse_id(value: &str) -> AnyResult<DecimalId> {
    DecimalId::parse(&Value::String(value.to_string()), "entity")
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn entity_for(id: &DecimalId) -> EntityRef {
    EntityRef {
        entity_id: id.clone(),
        entity_kind: "device_cell".into(),
        cell_id: id.clone(),
        device_id: id.clone(),
        cell_model_id: DecimalId::from_u64(0),
        cell_idx: 0,
        protocol: "v3".into(),
    }
}

fn requested_changes(params: &V3StateParams) -> AnyResult<Vec<RequestedChange>> {
    params
        .state
        .iter()
        .map(|value| {
            Ok(RequestedChange {
                idx: value.idx,
                expected: serde_json::to_value(&value.value)?,
            })
        })
        .collect()
}

fn requested_changes_from_event(event: &serde_yaml::Value) -> AnyResult<Vec<RequestedChange>> {
    let state = event
        .get("state")
        .and_then(serde_yaml::Value::as_sequence)
        .ok_or_else(|| anyhow::anyhow!("V3 HC wsExpect event requires state"))?;
    state
        .iter()
        .map(|item| {
            let idx = item
                .get("idx")
                .and_then(serde_yaml::Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("V3 HC wsExpect state requires idx"))?;
            let value = item
                .get("value")
                .ok_or_else(|| anyhow::anyhow!("V3 HC wsExpect state requires value"))?;
            Ok(RequestedChange {
                idx,
                expected: serde_json::to_value(value)?,
            })
        })
        .collect()
}

fn invalid_result(error: anyhow::Error) -> V3Result {
    V3Result::error(V3Category::InvalidPayload, error.to_string(), EvidenceTier::RealE2e)
}

fn http_result(error: HcHttpError) -> V3Result {
    match error {
        HcHttpError::Blocked(message) => V3Result::blocked(V3Category::ControlDispatch, message),
        HcHttpError::ReadbackMismatch { .. } => V3Result::failed(V3Category::AssertionMismatch, error.to_string(), EvidenceTier::RealE2e),
        HcHttpError::Timeout => V3Result::error(V3Category::Timeout, error.to_string(), EvidenceTier::RealE2e),
        HcHttpError::InvalidPayload(message) => V3Result::error(V3Category::InvalidPayload, message, EvidenceTier::RealE2e),
        HcHttpError::Evidence(message) => V3Result::error(V3Category::EvidenceError, message, EvidenceTier::RealE2e),
        other => V3Result::error(V3Category::HttpError, other.to_string(), EvidenceTier::RealE2e),
    }
}

fn ws_result(error: HcWsError) -> V3Result {
    match error {
        HcWsError::Timeout => V3Result::error(V3Category::Timeout, error.to_string(), EvidenceTier::RealE2e),
        HcWsError::Evidence(message) => V3Result::error(V3Category::EvidenceError, message, EvidenceTier::RealE2e),
        HcWsError::Closed => V3Result::error(V3Category::WebsocketClosed, error.to_string(), EvidenceTier::RealE2e),
        HcWsError::Malformed(message) => V3Result::error(V3Category::ProtocolError, message, EvidenceTier::RealE2e),
        HcWsError::ReconnectExhausted => V3Result::blocked(V3Category::NotConnected, error.to_string()),
        HcWsError::Transport(message) => V3Result::error(V3Category::HttpError, message, EvidenceTier::RealE2e),
    }
}
