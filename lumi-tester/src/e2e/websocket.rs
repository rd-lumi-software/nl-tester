use super::canonical::{map_runtime_frame, map_snapshot, CanonicalObservation, DecimalId, EntityRef, SlotState};
use super::evidence::{EvidenceCollector, EvidenceKind, EvidenceRecord};
use super::result::{readback_matches, RequestedChange};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(Debug, Error, Clone, PartialEq)]
pub enum WsTransportError {
    #[error("WebSocket transport failed: {0}")]
    Transport(String),
    #[error("WebSocket closed")]
    Closed,
    #[error("WebSocket receive timed out")]
    Timeout,
}

#[async_trait]
pub trait WsConnection: Send {
    async fn send_text(&mut self, text: String) -> Result<(), WsTransportError>;
    async fn receive_text(&mut self) -> Result<Option<String>, WsTransportError>;
    async fn close(&mut self);
}

#[async_trait]
pub trait WsTransport: Send + Sync {
    async fn connect(&self, url: &str) -> Result<Box<dyn WsConnection>, WsTransportError>;
}

#[derive(Clone, Default)]
pub struct RealWsTransport;

#[async_trait]
impl WsTransport for RealWsTransport {
    async fn connect(&self, url: &str) -> Result<Box<dyn WsConnection>, WsTransportError> {
        let (stream, _) = connect_async(url)
            .await
            .map_err(|error| WsTransportError::Transport(error.to_string()))?;
        Ok(Box::new(RealWsConnection { stream }))
    }
}

struct RealWsConnection {
    stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

#[async_trait]
impl WsConnection for RealWsConnection {
    async fn send_text(&mut self, text: String) -> Result<(), WsTransportError> {
        self.stream
            .send(Message::Text(text.into()))
            .await
            .map_err(|error| WsTransportError::Transport(error.to_string()))
    }

    async fn receive_text(&mut self) -> Result<Option<String>, WsTransportError> {
        match self.stream.next().await {
            Some(Ok(Message::Text(text))) => Ok(Some(text.to_string())),
            Some(Ok(Message::Close(_))) | None => Ok(None),
            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => Ok(Some("{}".into())),
            Some(Ok(_)) => Err(WsTransportError::Transport("binary WebSocket frame is unsupported".into())),
            Some(Err(error)) => Err(WsTransportError::Transport(error.to_string())),
        }
    }

    async fn close(&mut self) {
        let _ = self.stream.close(None).await;
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum HcWsError {
    #[error("WebSocket transport error: {0}")]
    Transport(String),
    #[error("WebSocket closed")]
    Closed,
    #[error("WebSocket frame timed out")]
    Timeout,
    #[error("WebSocket frame is malformed: {0}")]
    Malformed(String),
    #[error("WebSocket evidence capture failed: {0}")]
    Evidence(String),
    #[error("WebSocket reconnect bound exhausted")]
    ReconnectExhausted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WsReadback {
    pub states: Vec<SlotState>,
    pub replayed: bool,
    pub fallback: bool,
    pub evidence: Vec<EvidenceRecord>,
}

pub struct HcWsAdapter<T> {
    transport: T,
    websocket_url: String,
    timeout: Duration,
    reconnect_max_attempts: u64,
    connection: Option<Box<dyn WsConnection>>,
    subscribed: Vec<String>,
    next_request_id: u64,
    evidence: EvidenceCollector,
}

impl<T> HcWsAdapter<T>
where
    T: WsTransport,
{
    pub fn new(
        transport: T,
        websocket_url: impl Into<String>,
        timeout: Duration,
        reconnect_max_attempts: u64,
        evidence: EvidenceCollector,
    ) -> Result<Self, HcWsError> {
        if reconnect_max_attempts == 0 {
            return Err(HcWsError::ReconnectExhausted);
        }
        Ok(Self {
            transport,
            websocket_url: websocket_url.into(),
            timeout,
            reconnect_max_attempts,
            connection: None,
            subscribed: Vec::new(),
            next_request_id: 1,
            evidence,
        })
    }

    pub fn evidence(&self) -> &[EvidenceRecord] {
        self.evidence.records()
    }

    pub async fn connect(&mut self) -> Result<(), HcWsError> {
        let url = self.session_url();
        self.connection = Some(self.transport.connect(&url).await.map_err(map_transport_error)?);
        Ok(())
    }

    pub async fn subscribe(&mut self, channels: &[String]) -> Result<(), HcWsError> {
        if self.connection.is_none() {
            self.connect().await?;
        }
        self.subscribed = channels.to_vec();
        let request = json!({ "cmd": "subscribe", "payload": channels });
        self.send_json(&request).await?;
        let started = tokio::time::Instant::now();
        loop {
            let remaining = self.timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(HcWsError::Timeout);
            }
            let frame = match tokio::time::timeout(remaining, self.receive_json()).await {
                Ok(frame) => frame?,
                Err(_) => return Err(HcWsError::Timeout),
            };
            if frame.get("type").and_then(Value::as_str) == Some("subscribe_response") {
                let accepted = frame.get("payload").and_then(Value::as_array).ok_or_else(|| HcWsError::Malformed("subscribe_response payload must be an array".into()))?;
                if channels.iter().all(|channel| accepted.iter().any(|value| value.as_str() == Some(channel))) {
                    return Ok(());
                }
                return Err(HcWsError::Malformed("subscribe_response did not acknowledge every channel".into()));
            }
        }
    }

    pub async fn expect_state(
        &mut self,
        entity: &EntityRef,
        requested: &[RequestedChange],
        deadline: Duration,
    ) -> Result<WsReadback, HcWsError> {
        let started = tokio::time::Instant::now();
        let mut evidence = Vec::new();
        loop {
            let remaining = deadline.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(HcWsError::Timeout);
            }
            match tokio::time::timeout(remaining, self.receive_json()).await {
                Ok(Ok(frame)) => {
                    let record = self.capture(EvidenceKind::WebsocketFrame, &frame)?;
                    evidence.push(record);
                    if let Ok(CanonicalObservation::State(states)) = map_runtime_frame(entity, &frame, chrono::Utc::now().to_rfc3339()) {
                        if readback_matches(entity, requested, &states) {
                            evidence.push(self.capture(
                                EvidenceKind::NormalizedState,
                                &json!({
                                    "entity": entity.entity_id.as_str(),
                                    "states": states.clone(),
                                }),
                            )?);
                            return Ok(WsReadback { states, replayed: false, fallback: false, evidence });
                        }
                    }
                }
                Ok(Err(HcWsError::Closed)) | Ok(Err(HcWsError::Transport(_))) => {
                    self.reconnect_and_resubscribe().await?;
                    // Reconnect has no replay guarantee. Keep waiting for a new event;
                    // state/snapshot fallback is a separate operation.
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => return Err(HcWsError::Timeout),
            }
        }
    }

    pub async fn snapshot(&mut self, entity: &EntityRef) -> Result<WsReadback, HcWsError> {
        if self.connection.is_none() {
            self.connect().await?;
        }
        let request_id = DecimalId::from_u64(self.next_request_id);
        self.next_request_id = self.next_request_id.checked_add(1).ok_or_else(|| HcWsError::Malformed("RPC request id overflow".into()))?;
        let request = json!({
            "cmd": "rpc",
            "payload": {
                "request_id": request_id.as_str(),
                "data": {"cell_snapshot": entity.cell_id.as_str()}
            }
        });
        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut reconnects = 0;
        loop {
            match self.send_json(&request).await {
                Ok(()) => {}
                Err(HcWsError::Closed | HcWsError::Transport(_)) => {
                    reconnects += 1;
                    if reconnects > self.reconnect_max_attempts {
                        return Err(HcWsError::ReconnectExhausted);
                    }
                    self.reconnect_and_resubscribe().await?;
                    continue;
                }
                Err(error) => return Err(error),
            }
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err(HcWsError::Timeout);
                }
                match tokio::time::timeout(remaining, self.receive_json()).await {
                    Ok(Ok(frame)) => {
                        let record = self.capture(EvidenceKind::WebsocketFrame, &frame)?;
                        let Some(payload) = frame.get("payload").and_then(Value::as_object) else { continue };
                        if frame.get("type").and_then(Value::as_str) != Some("rpc") { continue }
                        if payload.get("request_id").and_then(Value::as_str) != Some(request_id.as_str()) { continue }
                        let data = payload.get("data").and_then(Value::as_object).ok_or_else(|| HcWsError::Malformed("RPC response data missing".into()))?;
                        if let Some(error) = data.get("error") {
                            return Err(HcWsError::Malformed(format!("RPC error: {error}")));
                        }
                        let values = data.get("cell_snapshot").ok_or_else(|| HcWsError::Malformed("RPC response missing cell_snapshot".into()))?;
                        let states = map_snapshot(entity, values, chrono::Utc::now().to_rfc3339(), request_id.clone())
                            .map_err(|error| HcWsError::Malformed(error.to_string()))?;
                        let mut evidence = vec![record];
                        let normalized = self.capture(
                            EvidenceKind::NormalizedState,
                            &json!({
                                "entity": entity.entity_id.as_str(),
                                "states": states.clone(),
                            }),
                        )?;
                        evidence.push(normalized);
                        return Ok(WsReadback { states, replayed: false, fallback: true, evidence });
                    }
                    Ok(Err(HcWsError::Closed | HcWsError::Transport(_))) => {
                        reconnects += 1;
                        if reconnects > self.reconnect_max_attempts {
                            return Err(HcWsError::ReconnectExhausted);
                        }
                        self.reconnect_and_resubscribe().await?;
                        break;
                    }
                    Ok(Err(error)) => return Err(error),
                    Err(_) => return Err(HcWsError::Timeout),
                }
            }
        }
    }

    async fn reconnect_and_resubscribe(&mut self) -> Result<(), HcWsError> {
        if let Some(connection) = &mut self.connection {
            connection.close().await;
        }
        for _ in 0..self.reconnect_max_attempts {
            match self.connect().await {
                Ok(()) => {}
                Err(HcWsError::Transport(_) | HcWsError::Closed) => continue,
                Err(error) => return Err(error),
            }
            let channels = self.subscribed.clone();
            match self.subscribe(&channels).await {
                Ok(()) => return Ok(()),
                Err(HcWsError::Transport(_) | HcWsError::Closed) => {}
                Err(error) => return Err(error),
            }
        }
        Err(HcWsError::ReconnectExhausted)
    }

    async fn send_json(&mut self, value: &Value) -> Result<(), HcWsError> {
        let text = serde_json::to_string(value).map_err(|error| HcWsError::Malformed(error.to_string()))?;
        self.capture(EvidenceKind::WebsocketFrame, value)?;
        self.connection.as_mut().ok_or(HcWsError::Closed)?.send_text(text).await.map_err(map_transport_error)
    }

    async fn receive_json(&mut self) -> Result<Value, HcWsError> {
        let text = self.connection.as_mut().ok_or(HcWsError::Closed)?.receive_text().await.map_err(map_transport_error)?.ok_or(HcWsError::Closed)?;
        serde_json::from_str(&text).map_err(|error| HcWsError::Malformed(error.to_string()))
    }

    fn capture(&mut self, kind: EvidenceKind, value: &Value) -> Result<EvidenceRecord, HcWsError> {
        self.evidence.capture_json(kind, value).map_err(|error| HcWsError::Evidence(error.to_string()))
    }

    fn session_url(&self) -> String {
        let separator = if self.websocket_url.contains('?') { '&' } else { '?' };
        format!("{}{}clientId=lumi-tester-{}", self.websocket_url, separator, uuid::Uuid::new_v4())
    }
}

fn map_transport_error(error: WsTransportError) -> HcWsError {
    match error {
        WsTransportError::Transport(error) => HcWsError::Transport(error),
        WsTransportError::Closed => HcWsError::Closed,
        WsTransportError::Timeout => HcWsError::Timeout,
    }
}

#[derive(Clone)]
pub struct MockWebSocketTransport {
    sessions: Arc<Mutex<VecDeque<Vec<String>>>>,
    sent: Arc<Mutex<Vec<String>>>,
    urls: Arc<Mutex<Vec<String>>>,
}

impl MockWebSocketTransport {
    pub fn new(sessions: impl IntoIterator<Item = Vec<String>>) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(sessions.into_iter().collect())),
            sent: Arc::new(Mutex::new(Vec::new())),
            urls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn sent(&self) -> Vec<Value> {
        self.sent.lock().unwrap().iter().filter_map(|value| serde_json::from_str(value).ok()).collect()
    }

    pub fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }
}

struct MockWebSocketConnection {
    inbound: VecDeque<String>,
    sent: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl WsConnection for MockWebSocketConnection {
    async fn send_text(&mut self, text: String) -> Result<(), WsTransportError> {
        self.sent.lock().unwrap().push(text);
        Ok(())
    }

    async fn receive_text(&mut self) -> Result<Option<String>, WsTransportError> {
        Ok(self.inbound.pop_front())
    }

    async fn close(&mut self) {}
}

#[async_trait]
impl WsTransport for MockWebSocketTransport {
    async fn connect(&self, url: &str) -> Result<Box<dyn WsConnection>, WsTransportError> {
        self.urls.lock().unwrap().push(url.to_string());
        let frames = self.sessions.lock().unwrap().pop_front().ok_or_else(|| WsTransportError::Transport("mock session queue exhausted".into()))?;
        Ok(Box::new(MockWebSocketConnection { inbound: frames.into(), sent: self.sent.clone() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2e::canonical::EntityRef;
    use crate::e2e::http::default_evidence;

    fn entity() -> EntityRef {
        EntityRef {
            entity_id: DecimalId::from_u64(42), entity_kind: "device_cell".into(),
            cell_id: DecimalId::from_u64(42), device_id: DecimalId::from_u64(42),
            cell_model_id: DecimalId::from_u64(1), cell_idx: 0, protocol: "virtual".into(),
        }
    }

    #[tokio::test]
    async fn subscribes_on_explicit_url_and_matches_runtime_frame() {
        let mock = MockWebSocketTransport::new([vec![
            r#"{"type":"subscribe_response","payload":["cell:42"]}"#.into(),
            r#"{"type":"data","payload":{"channel":"cell_runtime_data","data":{"cell_id":"42","content":{"type":"cell_state_changed","payload":[{"idx":0,"value":true}]}}}}"#.into(),
        ]]);
        let inspect = mock.clone();
        let mut adapter = HcWsAdapter::new(mock, "ws://hc.test/ws", Duration::from_millis(100), 1, default_evidence(4096, 10).unwrap()).unwrap();
        adapter.subscribe(&["cell:42".into()]).await.unwrap();
        let result = adapter.expect_state(&entity(), &[RequestedChange { idx: 0, expected: Value::Bool(true) }], Duration::from_millis(100)).await.unwrap();
        assert!(!result.replayed);
        assert!(!result.fallback);
        assert!(inspect.urls()[0].starts_with("ws://hc.test/ws?clientId="));
    }

    #[tokio::test]
    async fn reconnect_resubscribes_then_snapshot_is_explicit_fallback() {
        let mock = MockWebSocketTransport::new([
            vec![r#"{"type":"subscribe_response","payload":["cell:42"]}"#.into()],
            vec![
                r#"{"type":"subscribe_response","payload":["cell:42"]}"#.into(),
                r#"{"type":"rpc","payload":{"request_id":"1","data":{"cell_snapshot":[{"idx":0,"value":true}]}}}"#.into(),
            ],
        ]);
        let inspect = mock.clone();
        let mut adapter = HcWsAdapter::new(mock, "ws://hc.test/ws", Duration::from_millis(100), 1, default_evidence(4096, 20).unwrap()).unwrap();
        adapter.subscribe(&["cell:42".into()]).await.unwrap();
        let result = adapter.snapshot(&entity()).await.unwrap();
        assert!(result.fallback);
        assert!(!result.replayed);
        let sent = inspect.sent();
        assert_eq!(sent.iter().filter(|value| value["cmd"] == "subscribe").count(), 2);
        assert_eq!(sent.last().unwrap()["payload"]["data"]["cell_snapshot"], "42");
    }
}
