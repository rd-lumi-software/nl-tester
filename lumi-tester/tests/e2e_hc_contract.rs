use lumi_tester::e2e::canonical::{
    map_device_entity, map_snapshot, map_state_vector, AliasMapping, DecimalId, EntityRef,
    StateSource,
};
use lumi_tester::e2e::evidence::{
    EvidenceCaptureError, EvidenceCollector, EvidenceConfig, EvidenceKind, EvidenceRecord,
};
use lumi_tester::e2e::http::{
    default_evidence, HcHttpAdapter, HttpMethod, HttpResponse, MockHttpTransport,
};
use lumi_tester::e2e::result::{
    classify_health, readback_matches, E2eCategory, E2eResult, E2eStatus, EvidenceTier,
    RequestedChange,
};
use lumi_tester::e2e::websocket::{
    HcWsAdapter, HcWsError, MockWebSocketTransport, WsConnection, WsTransport,
    WsTransportError,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

struct NeverAckConnection;

#[async_trait]
impl WsConnection for NeverAckConnection {
    async fn send_text(&mut self, _text: String) -> Result<(), WsTransportError> {
        Ok(())
    }

    async fn receive_text(&mut self) -> Result<Option<String>, WsTransportError> {
        std::future::pending().await
    }

    async fn close(&mut self) {}
}

#[derive(Clone)]
struct NeverAckTransport;

#[async_trait]
impl WsTransport for NeverAckTransport {
    async fn connect(&self, _url: &str) -> Result<Box<dyn WsConnection>, WsTransportError> {
        Ok(Box::new(NeverAckConnection))
    }
}

struct ContinuousIrrelevantConnection;

#[async_trait]
impl WsConnection for ContinuousIrrelevantConnection {
    async fn send_text(&mut self, _text: String) -> Result<(), WsTransportError> {
        Ok(())
    }

    async fn receive_text(&mut self) -> Result<Option<String>, WsTransportError> {
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok(Some(r#"{"type":"event","payload":{"unrelated":true}}"#.into()))
    }

    async fn close(&mut self) {}
}

#[derive(Clone)]
struct ContinuousIrrelevantTransport;

#[async_trait]
impl WsTransport for ContinuousIrrelevantTransport {
    async fn connect(&self, _url: &str) -> Result<Box<dyn WsConnection>, WsTransportError> {
        Ok(Box::new(ContinuousIrrelevantConnection))
    }
}

fn entity(id: &str) -> EntityRef {
    let id = DecimalId::parse(&Value::String(id.to_string()), "id").unwrap();
    EntityRef {
        entity_id: id.clone(),
        entity_kind: "device_cell".into(),
        cell_id: id.clone(),
        device_id: id,
        cell_model_id: DecimalId::from_u64(7),
        cell_idx: 0,
        protocol: "virtual".into(),
    }
}

#[test]
fn canonical_mapping_preserves_v3_identity_and_explicit_indexes() {
    let raw = json!({
        "id": "9007199254740993",
        "cell_model_id": 18446744073709551615u64,
        "cell_idx": 0,
        "protocol": "virtual",
        "network_data": {"dev_key": "secret"}
    });
    let mapped = map_device_entity(&raw).unwrap();

    assert_eq!(mapped.entity.entity_id.as_str(), "9007199254740993");
    assert_eq!(mapped.entity.cell_id.as_str(), "9007199254740993");
    assert_eq!(mapped.entity.device_id.as_str(), "9007199254740993");
    assert_eq!(mapped.entity.cell_model_id.as_str(), "18446744073709551615");
    assert_eq!(mapped.entity.cell_idx, 0);

    let vector = map_state_vector(
        &mapped.entity,
        &json!([false, true]),
        "2026-10-18T00:00:00Z",
        StateSource::HttpState,
    )
    .unwrap();
    assert_eq!(
        vector
            .iter()
            .map(|state| state.vector_index)
            .collect::<Vec<_>>(),
        [Some(0), Some(1)]
    );
    assert_eq!(vector[0].slot_idx, None);

    let snapshot = map_snapshot(
        &mapped.entity,
        &json!([{"idx": 0, "value": false}, {"idx": 3, "value": true}]),
        "2026-10-18T00:00:00Z",
        DecimalId::from_u64(1),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .iter()
            .map(|state| state.slot_idx)
            .collect::<Vec<_>>(),
        [Some(0), Some(3)]
    );
    assert!(AliasMapping {
        alias: "power".into(),
        input_idx: 0,
        state_idx: 0
    }
    .resolve("brightness")
    .is_none());
}

#[test]
fn status_classification_distinguishes_passed_failed_blocked_and_error() {
    assert_eq!(
        classify_health(true, EvidenceTier::MockContract).status,
        E2eStatus::Passed
    );
    assert_eq!(
        classify_health(false, EvidenceTier::MockContract).status,
        E2eStatus::Failed
    );
    assert_eq!(
        E2eResult::blocked(E2eCategory::NoLab, "runtime capability unavailable").status,
        E2eStatus::Blocked
    );
    assert_eq!(
        E2eResult::error(
            E2eCategory::ProtocolError,
            "fixture protocol error",
            EvidenceTier::MockConformance
        )
        .status,
        E2eStatus::Error
    );
    assert_eq!(
        E2eResult::error(
            E2eCategory::ProtocolError,
            "fixture protocol error",
            EvidenceTier::MockConformance
        )
        .evidence_tier,
        EvidenceTier::MockConformance
    );
}

#[test]
fn readback_correlation_requires_entity_index_and_expected_value() {
    let target = entity("42");
    let matching = map_snapshot(
        &target,
        &json!([{"idx": 2, "value": true}]),
        "2026-10-18T00:00:00Z",
        DecimalId::from_u64(1),
    )
    .unwrap();
    let request = RequestedChange {
        idx: 2,
        expected: Value::Bool(true),
    };

    assert!(readback_matches(
        &target,
        std::slice::from_ref(&request),
        &matching
    ));
    assert!(!readback_matches(
        &target,
        &[RequestedChange {
            idx: 1,
            ..request.clone()
        }],
        &matching
    ));
    assert!(!readback_matches(
        &target,
        &[RequestedChange {
            expected: Value::Bool(false),
            ..request.clone()
        }],
        &matching
    ));
    assert!(!readback_matches(&entity("43"), &[request], &matching));
}

#[test]
fn evidence_redaction_and_bounds_are_enforced() {
    let mut collector = EvidenceCollector::new(EvidenceConfig {
        max_raw_bytes: 48,
        max_websocket_frames: 2,
        redact_fields: vec!["token".into(), "dev_key".into()],
    })
    .unwrap();

    let record = collector
        .capture_json(
            EvidenceKind::HttpResponse,
            &json!({
                "token": "secret-token",
                "nested": {"dev_key": "secret-key"},
                "payload": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
            }),
        )
        .unwrap();
    assert!(record.redacted);
    assert!(record.truncated);
    assert!(record.bytes > 48);
    assert!(record.payload.as_bytes().len() <= 48);
    assert!(!record.payload.contains("secret-token"));
    assert!(!record.payload.contains("secret-key"));

    collector
        .capture_json(EvidenceKind::WebsocketFrame, &Value::Null)
        .unwrap();
    collector
        .capture_json(EvidenceKind::WebsocketFrame, &Value::Null)
        .unwrap();
    assert_eq!(
        collector.capture_json(EvidenceKind::WebsocketFrame, &Value::Null),
        Err(EvidenceCaptureError::FrameLimitExceeded)
    );
}

#[tokio::test]
async fn mock_wire_shapes_match_v3_http_and_websocket_contract() {
    let http_mock = MockHttpTransport::new([
        Ok(HttpResponse {
            status: 200,
            body: r#"{"status":true}"#.into(),
        }),
        Ok(HttpResponse {
            status: 200,
            body:
                r#"[{"id":"9007199254740993","cell_model_id":7,"cell_idx":0,"protocol":"virtual"}]"#
                    .into(),
        }),
        Ok(HttpResponse {
            status: 200,
            body: r#"[false,true]"#.into(),
        }),
        Ok(HttpResponse {
            status: 200,
            body: r#"{"status":true}"#.into(),
        }),
        Ok(HttpResponse {
            status: 200,
            body: r#"[true,true]"#.into(),
        }),
    ]);
    let http_inspect = http_mock.clone();
    let mut http = HcHttpAdapter::new(
        http_mock,
        "http://hc.test/",
        Duration::from_millis(50),
        2,
        default_evidence(4096, 20).unwrap(),
    )
    .unwrap();

    assert!(http.health().await.unwrap().status);
    let discovery = http.discover().await.unwrap();
    let device_id = discovery.entities[0].entity.device_id.clone();
    assert_eq!(device_id.as_str(), "9007199254740993");
    let state = http.state(&device_id).await.unwrap();
    assert_eq!(state.states[0].vector_index, Some(0));
    assert_eq!(state.states[1].vector_index, Some(1));
    assert!(
        http.control(
            &device_id,
            &[lumi_tester::e2e::http::ControlChange {
                idx: 0,
                value: Value::Bool(true),
            }]
        )
        .await
        .unwrap()
        .dispatched
    );

    let requests = http_inspect.requests();
    assert_eq!(requests[0].method, HttpMethod::Get);
    assert_eq!(requests[0].url, "http://hc.test/api/health_check");
    assert_eq!(requests[1].url, "http://hc.test/api/devices/");
    assert_eq!(
        requests[2].url,
        "http://hc.test/api/devices/9007199254740993/state"
    );
    assert_eq!(requests[3].method, HttpMethod::Post);
    assert_eq!(
        requests[3].url,
        "http://hc.test/api/devices/9007199254740993/control"
    );
    assert_eq!(requests[3].body, Some(json!([{"idx": 0, "value": true}])));
    assert_eq!(requests[4].method, HttpMethod::Get);

    let ws_mock = MockWebSocketTransport::new([
        vec![r#"{"type":"subscribe_response","payload":["cell:42"]}"#.into()],
        vec![
            r#"{"type":"subscribe_response","payload":["cell:42"]}"#.into(),
            r#"{"type":"data","payload":{"channel":"cell_runtime_data","data":{"cell_id":"42","content":{"type":"cell_state_changed","payload":[{"idx":0,"value":true}]}}}}"#.into(),
        ],
    ]);
    let ws_inspect = ws_mock.clone();
    let mut websocket = HcWsAdapter::new(
        ws_mock,
        "ws://hc.test/ws",
        Duration::from_millis(100),
        1,
        default_evidence(4096, 20).unwrap(),
    )
    .unwrap();
    websocket.subscribe(&["cell:42".into()]).await.unwrap();
    let runtime = websocket
        .expect_state(
            &entity("42"),
            &[RequestedChange {
                idx: 0,
                expected: Value::Bool(true),
            }],
            Duration::from_millis(100),
        )
        .await
        .unwrap();
    assert!(!runtime.fallback);
    assert_eq!(runtime.states[0].slot_idx, Some(0));
    assert_eq!(
        ws_inspect
            .sent()
            .iter()
            .filter(|frame| frame["cmd"] == "subscribe")
            .count(),
        2
    );
    assert_eq!(ws_inspect.urls().len(), 2);

    let snapshot_mock = MockWebSocketTransport::new([vec![
        r#"{"type":"rpc","payload":{"request_id":"1","data":{"cell_snapshot":[{"idx":0,"value":true}]}}}"#.into(),
    ]]);
    let snapshot_inspect = snapshot_mock.clone();
    let mut snapshot = HcWsAdapter::new(
        snapshot_mock,
        "ws://hc.test/ws",
        Duration::from_millis(100),
        1,
        default_evidence(4096, 20).unwrap(),
    )
    .unwrap();
    let fallback = snapshot.snapshot(&entity("42")).await.unwrap();
    assert!(fallback.fallback);
    assert_eq!(fallback.states[0].slot_idx, Some(0));
    assert_eq!(
        snapshot_inspect.sent()[0]["payload"]["data"]["cell_snapshot"],
        "42"
    );
}

#[tokio::test]
async fn http_control_requires_bounded_readback_and_normalized_evidence() {
    let mock = MockHttpTransport::new([
        Ok(HttpResponse {
            status: 200,
            body: r#"{"status":true}"#.into(),
        }),
        Ok(HttpResponse {
            status: 200,
            body: r#"[true]"#.into(),
        }),
    ]);
    let inspect = mock.clone();
    let mut http = HcHttpAdapter::new(
        mock,
        "http://hc.test",
        Duration::from_millis(50),
        2,
        default_evidence(4096, 20).unwrap(),
    )
    .unwrap();

    let response = http
        .control(
            &DecimalId::from_u64(42),
            &[lumi_tester::e2e::http::ControlChange {
                idx: 0,
                value: Value::Bool(true),
            }],
        )
        .await
        .unwrap();

    assert!(response.dispatched);
    assert_eq!(inspect.requests().len(), 2);
    assert_eq!(inspect.requests()[1].method, HttpMethod::Get);
    assert!(http
        .evidence()
        .iter()
        .any(|record| record.kind == "normalized_state"));
}

#[tokio::test]
async fn http_control_status_true_cannot_hide_readback_mismatch() {
    let mock = MockHttpTransport::new([
        Ok(HttpResponse {
            status: 200,
            body: r#"{"status":true}"#.into(),
        }),
        Ok(HttpResponse {
            status: 200,
            body: r#"[false]"#.into(),
        }),
        Ok(HttpResponse {
            status: 200,
            body: r#"[false]"#.into(),
        }),
    ]);
    let mut http = HcHttpAdapter::new(
        mock,
        "http://hc.test",
        Duration::from_millis(50),
        2,
        default_evidence(4096, 20).unwrap(),
    )
    .unwrap();

    let result = http
        .control(
            &DecimalId::from_u64(42),
            &[lumi_tester::e2e::http::ControlChange {
                idx: 0,
                value: Value::Bool(true),
            }],
        )
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("readback"));
}

#[tokio::test]
async fn websocket_subscribe_ack_wait_is_bounded() {
    let mut websocket = HcWsAdapter::new(
        NeverAckTransport,
        "ws://hc.test/ws",
        Duration::from_millis(20),
        1,
        default_evidence(4096, 20).unwrap(),
    )
    .unwrap();

    let result = tokio::time::timeout(
        Duration::from_millis(100),
        websocket.subscribe(&["cell:42".into()]),
    )
    .await;

    assert!(matches!(result, Ok(Err(HcWsError::Timeout))));
}

#[tokio::test]
async fn websocket_snapshot_timeout_is_shared_across_irrelevant_frames() {
    let mut websocket = HcWsAdapter::new(
        ContinuousIrrelevantTransport,
        "ws://hc.test/ws",
        Duration::from_millis(20),
        1,
        default_evidence(4096, 1024).unwrap(),
    )
    .unwrap();

    let result = tokio::time::timeout(
        Duration::from_millis(100),
        websocket.snapshot(&entity("42")),
    )
    .await;

    assert!(matches!(result, Ok(Err(HcWsError::Timeout))));
}

#[tokio::test]
async fn websocket_snapshot_reconnect_propagates_evidence_failure() {
    let mock = MockWebSocketTransport::new([
        vec![r#"{"type":"subscribe_response","payload":["cell:42"]}"#.into()],
        vec![],
    ]);
    let mut websocket = HcWsAdapter::new(
        mock,
        "ws://hc.test/ws",
        Duration::from_millis(100),
        1,
        default_evidence(4096, 2).unwrap(),
    )
    .unwrap();
    websocket.subscribe(&["cell:42".into()]).await.unwrap();

    let result = websocket.snapshot(&entity("42")).await;

    assert!(matches!(result, Err(HcWsError::Evidence(_))));
}

#[tokio::test]
async fn websocket_expect_state_propagates_required_evidence_errors() {
    let mock = MockWebSocketTransport::new([vec![
        r#"{"type":"subscribe_response","payload":["cell:42"]}"#.into(),
        r#"{"type":"data","payload":{"channel":"cell_runtime_data","data":{"cell_id":"42","content":{"type":"cell_state_changed","payload":[{"idx":0,"value":true}]}}}}"#.into(),
    ]]);
    let mut websocket = HcWsAdapter::new(
        mock,
        "ws://hc.test/ws",
        Duration::from_millis(100),
        1,
        default_evidence(4096, 1).unwrap(),
    )
    .unwrap();
    websocket.subscribe(&["cell:42".into()]).await.unwrap();

    let result = websocket
        .expect_state(
            &entity("42"),
            &[RequestedChange {
                idx: 0,
                expected: Value::Bool(true),
            }],
            Duration::from_millis(100),
        )
        .await;

    assert!(matches!(result, Err(HcWsError::Evidence(_))));
}

#[test]
fn passed_result_requires_explicit_normalized_evidence() {
    let result = E2eResult::passed(
        E2eCategory::ControlDispatch,
        EvidenceTier::MockContract,
        json!({"operation":"control"}),
        json!({"state":true}),
        json!({"matches":true}),
        vec![EvidenceRecord {
            kind: "http_response".into(),
            payload: "{}".into(),
            bytes: 2,
            redacted: false,
            truncated: false,
        }],
        None,
    );
    assert_eq!(result.status, E2eStatus::Error);
    assert_eq!(result.category, E2eCategory::EvidenceError);
}

#[test]
fn passed_result_rejects_null_normalized_evidence() {
    let result = E2eResult::passed(
        E2eCategory::ControlDispatch,
        EvidenceTier::MockContract,
        json!({"operation":"control"}),
        json!({"state":true}),
        json!({"matches":true}),
        vec![EvidenceRecord {
            kind: "http_response".into(),
            payload: "{}".into(),
            bytes: 2,
            redacted: false,
            truncated: false,
        }],
        Some(Value::Null),
    );
    assert_eq!(result.status, E2eStatus::Error);
    assert_eq!(result.category, E2eCategory::EvidenceError);
}
