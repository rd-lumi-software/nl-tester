use super::canonical::{map_device_entity, map_state_vector, CanonicalEntity, DecimalId, SlotState, StateSource};
use super::evidence::{EvidenceCaptureError, EvidenceCollector, EvidenceKind, EvidenceRecord};
use super::result::{readback_matches, RequestedChange};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub body: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum HttpTransportError {
    #[error("HTTP transport failed: {0}")]
    Transport(String),
    #[error("HTTP request timed out")]
    Timeout,
}

#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn send(&self, request: HttpRequest, timeout: Duration) -> Result<HttpResponse, HttpTransportError>;
}

#[derive(Clone, Default)]
pub struct ReqwestHttpTransport {
    client: reqwest::Client,
}

impl ReqwestHttpTransport {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self { client: reqwest::Client::builder().build()? })
    }
}

#[async_trait]
impl HttpTransport for ReqwestHttpTransport {
    async fn send(&self, request: HttpRequest, timeout: Duration) -> Result<HttpResponse, HttpTransportError> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
        };
        let mut builder = self.client.request(method, &request.url);
        if let Some(body) = request.body {
            builder = builder.json(&body);
        }
        let response = tokio::time::timeout(timeout, builder.send())
            .await
            .map_err(|_| HttpTransportError::Timeout)?
            .map_err(|error| HttpTransportError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|error| HttpTransportError::Transport(error.to_string()))?;
        Ok(HttpResponse { status, body })
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum HcHttpError {
    #[error("HTTP transport error: {0}")]
    Transport(String),
    #[error("HTTP response timed out")]
    Timeout,
    #[error("HTTP status {status} returned {code}: {message}")]
    HttpStatus { status: u16, code: String, message: String },
    #[error("HTTP status {status} returned malformed JSON")]
    MalformedJson { status: u16 },
    #[error("HTTP response payload is invalid: {0}")]
    InvalidPayload(String),
    #[error("HTTP control was blocked: {0}")]
    Blocked(String),
    #[error("HTTP control readback mismatch for entity {entity} after {attempts} bounded attempt(s)")]
    ReadbackMismatch { entity: String, attempts: u64 },
    #[error("evidence capture failed: {0}")]
    Evidence(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpCallEvidence {
    pub request: EvidenceRecord,
    pub response: EvidenceRecord,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HealthResponse {
    pub status: bool,
    pub evidence: HttpCallEvidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryResponse {
    pub entities: Vec<CanonicalEntity>,
    pub evidence: HttpCallEvidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateResponse {
    pub states: Vec<SlotState>,
    pub evidence: HttpCallEvidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlChange {
    pub idx: u64,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlResponse {
    pub dispatched: bool,
    pub evidence: HttpCallEvidence,
    pub readback: StateResponse,
    pub assertion: bool,
    pub required_evidence: Vec<EvidenceRecord>,
}

pub struct HcHttpAdapter<T> {
    transport: T,
    base_url: String,
    timeout: Duration,
    safe_get_max_attempts: u64,
    evidence: EvidenceCollector,
}

impl<T> HcHttpAdapter<T> {
    pub fn new(
        transport: T,
        base_url: impl Into<String>,
        timeout: Duration,
        safe_get_max_attempts: u64,
        evidence: EvidenceCollector,
    ) -> Result<Self, HcHttpError> {
        if safe_get_max_attempts == 0 {
            return Err(HcHttpError::InvalidPayload("safe GET attempts must be positive".into()));
        }
        Ok(Self {
            transport,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            timeout,
            safe_get_max_attempts,
            evidence,
        })
    }

    pub fn evidence(&self) -> &[EvidenceRecord] {
        self.evidence.records()
    }

    pub async fn health(&mut self) -> Result<HealthResponse, HcHttpError>
    where
        T: HttpTransport,
    {
        let (response, evidence) = self.request_json(HttpMethod::Get, "/api/health_check", None, true).await?;
        let status = response
            .get("status")
            .and_then(Value::as_bool)
            .ok_or_else(|| HcHttpError::InvalidPayload("health response requires boolean status".into()))?;
        Ok(HealthResponse { status, evidence })
    }

    pub async fn discover(&mut self) -> Result<DiscoveryResponse, HcHttpError>
    where
        T: HttpTransport,
    {
        // v3 source registration is route("/") nested under /api/devices.
        let (response, evidence) = self.request_json(HttpMethod::Get, "/api/devices/", None, true).await?;
        let values = response
            .as_array()
            .ok_or_else(|| HcHttpError::InvalidPayload("discovery response must be an array".into()))?;
        let entities = values
            .iter()
            .map(map_device_entity)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| HcHttpError::InvalidPayload(error.to_string()))?;
        Ok(DiscoveryResponse { entities, evidence })
    }

    pub async fn state(&mut self, device_id: &DecimalId) -> Result<StateResponse, HcHttpError>
    where
        T: HttpTransport,
    {
        let path = format!("/api/devices/{}/state", device_id.as_str());
        let (response, evidence) = self.request_json(HttpMethod::Get, &path, None, true).await?;
        let entity = minimal_entity(device_id);
        let observed_at = chrono::Utc::now().to_rfc3339();
        let states = map_state_vector(&entity.entity, &response, observed_at, StateSource::HttpState)
            .map_err(|error| HcHttpError::InvalidPayload(error.to_string()))?;
        Ok(StateResponse { states, evidence })
    }

    pub async fn control(
        &mut self,
        device_id: &DecimalId,
        changes: &[ControlChange],
    ) -> Result<ControlResponse, HcHttpError>
    where
        T: HttpTransport,
    {
        if changes.is_empty() {
            return Err(HcHttpError::InvalidPayload("control changes must not be empty".into()));
        }
        let body = changes
            .iter()
            .map(|change| json!({ "idx": change.idx, "value": change.value }))
            .collect::<Vec<_>>();
        let path = format!("/api/devices/{}/control", device_id.as_str());
        // This call deliberately passes safe_get=false: control POST is never retried.
        let (response, evidence) = self.request_json(HttpMethod::Post, &path, Some(Value::Array(body)), false).await?;
        let dispatched = response
            .get("status")
            .and_then(Value::as_bool)
            .ok_or_else(|| HcHttpError::InvalidPayload("control response requires boolean status".into()))?;
        if !dispatched {
            return Err(HcHttpError::Blocked(
                "control response returned status=false".into(),
            ));
        }

        let entity = minimal_entity(device_id).entity;
        let requested = changes
            .iter()
            .map(|change| RequestedChange {
                idx: change.idx,
                expected: change.value.clone(),
            })
            .collect::<Vec<_>>();
        let mut required_evidence = vec![evidence.request.clone(), evidence.response.clone()];
        for attempt in 1..=self.safe_get_max_attempts {
            let readback = self.state(device_id).await?;
            required_evidence.push(readback.evidence.request.clone());
            required_evidence.push(readback.evidence.response.clone());
            let normalized = self.capture(
                EvidenceKind::NormalizedState,
                &json!({
                    "entity": entity.entity_id.as_str(),
                    "states": readback.states.clone(),
                }),
            )?;
            required_evidence.push(normalized);

            if readback_matches(&entity, &requested, &readback.states) {
                return Ok(ControlResponse {
                    dispatched,
                    evidence,
                    readback,
                    assertion: true,
                    required_evidence,
                });
            }
            if attempt == self.safe_get_max_attempts {
                return Err(HcHttpError::ReadbackMismatch {
                    entity: device_id.as_str().to_string(),
                    attempts: self.safe_get_max_attempts,
                });
            }
        }

        Err(HcHttpError::ReadbackMismatch {
            entity: device_id.as_str().to_string(),
            attempts: self.safe_get_max_attempts,
        })
    }

    async fn request_json(
        &mut self,
        method: HttpMethod,
        path: &str,
        body: Option<Value>,
        safe_get: bool,
    ) -> Result<(Value, HttpCallEvidence), HcHttpError>
    where
        T: HttpTransport,
    {
        let request = HttpRequest {
            method,
            url: format!("{}{}", self.base_url, path),
            body: body.clone(),
        };
        let request_value = json!({
            "method": method.as_str(),
            "url": request.url,
            "body": body,
        });
        let request_evidence = self.capture(EvidenceKind::HttpRequest, &request_value)?;
        let attempts = if safe_get { self.safe_get_max_attempts } else { 1 };
        let mut last_transport = None;
        for _ in 0..attempts {
            match self.transport.send(request.clone(), self.timeout).await {
                Ok(response) => {
                    let parsed = serde_json::from_str::<Value>(&response.body);
                    let response_value = match &parsed {
                        Ok(value) => value.clone(),
                        Err(_) => Value::String(response.body.clone()),
                    };
                    let response_evidence = self.capture(EvidenceKind::HttpResponse, &json!({
                        "status": response.status,
                        "body": response_value,
                    }))?;
                    let response_json = parsed.map_err(|_| HcHttpError::MalformedJson { status: response.status })?;
                    if !(200..300).contains(&response.status) {
                        return Err(domain_error(response.status, &response_json));
                    }
                    return Ok((response_json, HttpCallEvidence { request: request_evidence, response: response_evidence }));
                }
                Err(HttpTransportError::Timeout) => return Err(HcHttpError::Timeout),
                Err(HttpTransportError::Transport(error)) => last_transport = Some(error),
            }
        }
        Err(HcHttpError::Transport(last_transport.unwrap_or_else(|| "request failed".into())))
    }

    fn capture(&mut self, kind: EvidenceKind, value: &Value) -> Result<EvidenceRecord, HcHttpError> {
        self.evidence.capture_json(kind, value).map_err(|error| HcHttpError::Evidence(error.to_string()))
    }
}

fn domain_error(status: u16, value: &Value) -> HcHttpError {
    if let Some(object) = value.as_object() {
        if let (Some(code), Some(message)) = (object.get("code").and_then(Value::as_str), object.get("msg").and_then(Value::as_str)) {
            return HcHttpError::HttpStatus { status, code: code.into(), message: message.into() };
        }
    }
    HcHttpError::HttpStatus { status, code: "HTTP_ERROR".into(), message: value.to_string() }
}

fn minimal_entity(device_id: &DecimalId) -> CanonicalEntity {
    CanonicalEntity {
        entity: super::canonical::EntityRef {
            entity_id: device_id.clone(),
            entity_kind: "device_cell".into(),
            cell_id: device_id.clone(),
            device_id: device_id.clone(),
            cell_model_id: DecimalId::from_u64(0),
            cell_idx: 0,
            protocol: "v3".into(),
        },
        raw: Value::Null,
    }
}

#[derive(Clone)]
pub struct MockHttpTransport {
    responses: Arc<Mutex<VecDeque<Result<HttpResponse, HttpTransportError>>>>,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
}

impl MockHttpTransport {
    pub fn new(responses: impl IntoIterator<Item = Result<HttpResponse, HttpTransportError>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl HttpTransport for MockHttpTransport {
    async fn send(&self, request: HttpRequest, _timeout: Duration) -> Result<HttpResponse, HttpTransportError> {
        self.requests.lock().unwrap().push(request);
        self.responses.lock().unwrap().pop_front().unwrap_or_else(|| {
            Err(HttpTransportError::Transport("mock response queue exhausted".into()))
        })
    }
}

pub fn default_evidence(max_raw_bytes: usize, max_websocket_frames: usize) -> Result<EvidenceCollector, EvidenceCaptureError> {
    EvidenceCollector::new(super::evidence::EvidenceConfig {
        max_raw_bytes,
        max_websocket_frames,
        redact_fields: vec!["dev_key", "nwk_key", "app_key", "authorization", "cookie", "token"].into_iter().map(String::from).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> EvidenceCollector {
        default_evidence(4096, 10).unwrap()
    }

    #[tokio::test]
    async fn uses_exact_discovery_route_and_never_retries_control_post() {
        let mock = MockHttpTransport::new([
            Ok(HttpResponse { status: 200, body: r#"[{"id":"9007199254740993","cell_model_id":1,"cell_idx":0,"protocol":"virtual"}]"#.into() }),
            Ok(HttpResponse { status: 200, body: r#"{"status":true}"#.into() }),
            Ok(HttpResponse { status: 200, body: r#"[true]"#.into() }),
        ]);
        let inspect = mock.clone();
        let mut adapter = HcHttpAdapter::new(mock, "http://hc.test", Duration::from_millis(50), 2, evidence()).unwrap();
        let discovery = adapter.discover().await.unwrap();
        assert_eq!(discovery.entities[0].entity.device_id.as_str(), "9007199254740993");
        let control = adapter.control(&DecimalId::from_u64(42), &[ControlChange { idx: 0, value: Value::Bool(true) }]).await.unwrap();
        assert!(control.dispatched);
        let requests = inspect.requests();
        assert_eq!(requests[0].url, "http://hc.test/api/devices/");
        assert_eq!(requests[1].url, "http://hc.test/api/devices/42/control");
        assert_eq!(requests[1].method, HttpMethod::Post);
    }

    #[tokio::test]
    async fn malformed_json_is_error_with_bounded_raw_evidence() {
        let mock = MockHttpTransport::new([Ok(HttpResponse { status: 200, body: "not-json".into() })]);
        let mut adapter = HcHttpAdapter::new(mock, "http://hc.test", Duration::from_millis(50), 1, evidence()).unwrap();
        assert!(matches!(adapter.health().await, Err(HcHttpError::MalformedJson { .. })));
        assert_eq!(adapter.evidence().len(), 2);
    }
}
