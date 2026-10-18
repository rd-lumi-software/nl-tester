pub mod evidence;
pub mod canonical;
pub mod http;
pub mod profile;
pub mod result;
pub mod types;
pub mod websocket;

pub use evidence::{EvidenceBoundaryError, EvidenceRoot, EvidenceWriter};
pub use profile::{validate_flow, E2eFlowMetadata, E2eValidationError};
pub use types::{E2eCapability, E2eCommand, E2eCommandRequirement};

use anyhow::Result as AnyResult;
use canonical::{CanonicalEntity, DecimalId, EntityRef};
use evidence::{EvidenceConfig, EvidenceRecord};
use http::{HcHttpAdapter, HcHttpError, ReqwestHttpTransport};
use result::{readback_matches, E2eCategory, E2eResult, E2eStatus, EvidenceTier, RequestedChange};
use serde_json::{json, Value};
use std::io::Write;
use std::time::Duration;
use types::{E2eExpectParams, E2eStateParams};
use websocket::{HcWsAdapter, HcWsError, RealWsTransport};

/// The one runtime boundary used by the existing Lumi runner for
/// legacy `platform: lumi_e2e` flows. `journey` is metadata only.
pub struct E2eRuntime {
    http: HcHttpAdapter<ReqwestHttpTransport>,
    websocket: HcWsAdapter<RealWsTransport>,
    entities: Vec<CanonicalEntity>,
    evidence_writer: EvidenceWriter,
    sequence: u64,
}

impl E2eRuntime {
    pub fn new(
        loaded: &profile::LoadedE2eProfile,
        base_url: String,
        websocket_url: String,
        timeout: Duration,
    ) -> AnyResult<Self> {
        if loaded.profile.retry.control_max_attempts != 0 {
            anyhow::bail!(
                "E2E controlMaxAttempts must be 0 because control POST is non-retried"
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
        let websocket = HcWsAdapter::new(
            RealWsTransport,
            websocket_url,
            timeout,
            loaded.profile.retry.websocket_reconnect_max_attempts,
            evidence::EvidenceCollector::new(config)?,
        )?;
        Ok(Self {
            http,
            websocket,
            entities: Vec::new(),
            evidence_writer: loaded.open_evidence_writer()?,
            sequence: 0,
        })
    }

    pub async fn execute(&mut self, command: &E2eCommand) -> E2eResult {
        let result = match command {
            E2eCommand::HcHealth => self.health().await,
            E2eCommand::HcGetEntities => self.discover().await,
            E2eCommand::HcGetState(params) => self.get_state(params).await,
            E2eCommand::HcControl(params) => self.control(params).await,
            E2eCommand::HcWaitState(params) => self.wait_state(params).await,
            E2eCommand::WsSubscribe(params) => self.subscribe(params).await,
            E2eCommand::WsExpect(params) => self.expect(params).await,
            E2eCommand::WsSnapshot(params) => self.snapshot(params).await,
        };
        match result {
            Ok(result) => {
                if result.status == E2eStatus::Passed {
                    if let Err(error) = self.persist(&result) {
                        return E2eResult::error(
                            E2eCategory::EvidenceError,
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

    async fn health(&mut self) -> std::result::Result<E2eResult, E2eResult> {
        let response = self.http.health().await.map_err(|error| http_result(error))?;
        let normalized = json!({ "status": response.status });
        if !response.status {
            return Err(E2eResult::failed(
                E2eCategory::AssertionMismatch,
                "HC health/readiness returned status=false",
                EvidenceTier::RealE2e,
            ));
        }
        Ok(self.pass(
            E2eCategory::Preflight,
            json!({ "operation": "health" }),
            normalized.clone(),
            json!({ "status": true }),
            vec![response.evidence.request, response.evidence.response],
            normalized,
        ))
    }

    async fn discover(&mut self) -> std::result::Result<E2eResult, E2eResult> {
        let response = self.http.discover().await.map_err(|error| http_result(error))?;
        self.entities = response.entities.clone();
        let normalized = serde_json::to_value(&response.entities).unwrap_or(Value::Null);
        Ok(self.pass(
            E2eCategory::Preflight,
            json!({ "operation": "device_discovery" }),
            normalized.clone(),
            json!({ "entitiesFound": response.entities.len() }),
            vec![response.evidence.request, response.evidence.response],
            normalized,
        ))
    }

    async fn get_state(&mut self, params: &types::E2eEntityParams) -> std::result::Result<E2eResult, E2eResult> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let response = self.http.state(&id).await.map_err(|error| http_result(error))?;
        let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
        Ok(self.pass(
            E2eCategory::Preflight,
            json!({ "operation": "state_read", "entity": params.entity }),
            normalized.clone(),
            json!({ "stateRead": true }),
            vec![response.evidence.request, response.evidence.response],
            normalized,
        ))
    }

    async fn control(&mut self, params: &E2eStateParams) -> std::result::Result<E2eResult, E2eResult> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let changes = params
            .state
            .iter()
            .map(|value| {
                Ok(http::ControlChange {
                    idx: value.idx,
                    value: serde_json::to_value(&value.value)
                        .map_err(|error| E2eResult::error(E2eCategory::InvalidPayload, error.to_string(), EvidenceTier::RealE2e))?,
                })
            })
            .collect::<std::result::Result<Vec<_>, E2eResult>>()?;
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
            E2eCategory::ControlDispatch,
            action,
            normalized.clone(),
            json!({ "readbackMatches": response.assertion }),
            response.required_evidence,
            normalized,
        ))
    }

    async fn wait_state(&mut self, params: &E2eStateParams) -> std::result::Result<E2eResult, E2eResult> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let requested = requested_changes(params).map_err(|error| invalid_result(error))?;
        for _ in 0..3 {
            let response = self.http.state(&id).await.map_err(|error| http_result(error))?;
            let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
            if readback_matches(&entity_for(&id), &requested, &response.states) {
                return Ok(self.pass(
                    E2eCategory::AssertionMismatch,
                    json!({ "operation": "wait_state", "entity": params.entity }),
                    normalized.clone(),
                    json!({ "readbackMatches": true }),
                    vec![response.evidence.request, response.evidence.response],
                    normalized,
                ));
            }
        }
        Err(E2eResult::failed(
            E2eCategory::AssertionMismatch,
            "bounded state readback did not match requested state",
            EvidenceTier::RealE2e,
        ))
    }

    async fn subscribe(&mut self, params: &types::E2eSubscribeParams) -> std::result::Result<E2eResult, E2eResult> {
        self.websocket
            .subscribe(&params.channels)
            .await
            .map_err(ws_result)?;
        let normalized = json!({ "channels": params.channels });
        Ok(self.pass(
            E2eCategory::Preflight,
            json!({ "operation": "subscribe", "channels": params.channels }),
            normalized.clone(),
            json!({ "acknowledged": true }),
            self.websocket.evidence().to_vec(),
            normalized,
        ))
    }

    async fn expect(&mut self, params: &E2eExpectParams) -> std::result::Result<E2eResult, E2eResult> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let requested = requested_changes_from_event(&params.event).map_err(|error| invalid_result(error))?;
        let response = self
            .websocket
            .expect_state(&entity_for(&id), &requested, Duration::from_secs(10))
            .await
            .map_err(ws_result)?;
        let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
        Ok(self.pass(
            E2eCategory::AssertionMismatch,
            json!({ "operation": "expect_state", "entity": params.entity, "event": params.event }),
            normalized.clone(),
            json!({ "readbackMatches": true }),
            response.evidence,
            normalized,
        ))
    }

    async fn snapshot(&mut self, params: &types::E2eEntityParams) -> std::result::Result<E2eResult, E2eResult> {
        let id = parse_id(&params.entity).map_err(|error| invalid_result(error))?;
        let response = self
            .websocket
            .snapshot(&entity_for(&id))
            .await
            .map_err(ws_result)?;
        let normalized = serde_json::to_value(&response.states).unwrap_or(Value::Null);
        Ok(self.pass(
            E2eCategory::ReconnectFallback,
            json!({ "operation": "snapshot", "entity": params.entity }),
            normalized.clone(),
            json!({ "snapshotRead": true }),
            response.evidence,
            normalized,
        ))
    }

    fn pass(
        &self,
        category: E2eCategory,
        action: Value,
        readback: Value,
        assertion: Value,
        evidence: Vec<EvidenceRecord>,
        normalized: Value,
    ) -> E2eResult {
        E2eResult::passed(
            category,
            EvidenceTier::RealE2e,
            action,
            readback,
            assertion,
            evidence,
            Some(normalized),
        )
    }

    fn persist(&mut self, result: &E2eResult) -> AnyResult<()> {
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

fn requested_changes(params: &E2eStateParams) -> AnyResult<Vec<RequestedChange>> {
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
        .ok_or_else(|| anyhow::anyhow!("E2E wsExpect event requires state"))?;
    state
        .iter()
        .map(|item| {
            let idx = item
                .get("idx")
                .and_then(serde_yaml::Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("E2E wsExpect state requires idx"))?;
            let value = item
                .get("value")
                .ok_or_else(|| anyhow::anyhow!("E2E wsExpect state requires value"))?;
            Ok(RequestedChange {
                idx,
                expected: serde_json::to_value(value)?,
            })
        })
        .collect()
}

fn invalid_result(error: anyhow::Error) -> E2eResult {
    E2eResult::error(E2eCategory::InvalidPayload, error.to_string(), EvidenceTier::RealE2e)
}

fn http_result(error: HcHttpError) -> E2eResult {
    match error {
        HcHttpError::Blocked(message) => E2eResult::blocked(E2eCategory::ControlDispatch, message),
        HcHttpError::ReadbackMismatch { .. } => E2eResult::failed(E2eCategory::AssertionMismatch, error.to_string(), EvidenceTier::RealE2e),
        HcHttpError::Timeout => E2eResult::error(E2eCategory::Timeout, error.to_string(), EvidenceTier::RealE2e),
        HcHttpError::InvalidPayload(message) => E2eResult::error(E2eCategory::InvalidPayload, message, EvidenceTier::RealE2e),
        HcHttpError::Evidence(message) => E2eResult::error(E2eCategory::EvidenceError, message, EvidenceTier::RealE2e),
        other => E2eResult::error(E2eCategory::HttpError, other.to_string(), EvidenceTier::RealE2e),
    }
}

fn ws_result(error: HcWsError) -> E2eResult {
    match error {
        HcWsError::Timeout => E2eResult::error(E2eCategory::Timeout, error.to_string(), EvidenceTier::RealE2e),
        HcWsError::Evidence(message) => E2eResult::error(E2eCategory::EvidenceError, message, EvidenceTier::RealE2e),
        HcWsError::Closed => E2eResult::error(E2eCategory::WebsocketClosed, error.to_string(), EvidenceTier::RealE2e),
        HcWsError::Malformed(message) => E2eResult::error(E2eCategory::ProtocolError, message, EvidenceTier::RealE2e),
        HcWsError::ReconnectExhausted => E2eResult::blocked(E2eCategory::NotConnected, error.to_string()),
        HcWsError::Transport(message) => E2eResult::error(E2eCategory::HttpError, message, EvidenceTier::RealE2e),
    }
}
