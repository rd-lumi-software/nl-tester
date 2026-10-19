use super::canonical::{EntityRef, SlotState};
use super::evidence::EvidenceRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3Status {
    Passed,
    Failed,
    Blocked,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3Category {
    Preflight,
    NoLab,
    UnsupportedCapability,
    HttpError,
    InvalidPayload,
    Timeout,
    NotConnected,
    WebsocketClosed,
    ProtocolError,
    EventMismatch,
    AssertionMismatch,
    EvidenceError,
    ControlDispatch,
    ReconnectFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceTier {
    MockContract,
    MockConformance,
    RealPreflight,
    RealE2e,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V3Result {
    pub status: V3Status,
    pub category: V3Category,
    pub message: String,
    pub retry_count: u32,
    pub evidence_tier: EvidenceTier,
    pub action: Option<Value>,
    pub readback: Option<Value>,
    pub assertion: Option<Value>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<Value>,
}

impl V3Result {
    pub fn passed(
        category: V3Category,
        tier: EvidenceTier,
        action: Value,
        readback: Value,
        assertion: Value,
        evidence: Vec<EvidenceRecord>,
        normalized: Option<Value>,
    ) -> Self {
        let evidence_is_bounded = evidence
            .iter()
            .all(|record| record.payload.as_bytes().len() <= record.bytes);
        if action.is_null()
            || readback.is_null()
            || assertion.is_null()
            || evidence.is_empty()
            || !evidence_is_bounded
            || normalized.as_ref().map(|value| value.is_null()).unwrap_or(true)
        {
            return Self::error(
                V3Category::EvidenceError,
                "a passed E2E result requires explicit action, readback, assertion, bounded evidence, and normalized evidence",
                tier,
            );
        }

        Self {
            status: V3Status::Passed,
            category,
            message: "operation, bounded readback, assertion, and required evidence completed".to_string(),
            retry_count: 0,
            evidence_tier: tier,
            action: Some(action),
            readback: Some(readback),
            assertion: Some(assertion),
            evidence,
            normalized,
        }
    }

    pub fn failed(category: V3Category, message: impl Into<String>, tier: EvidenceTier) -> Self {
        Self {
            status: V3Status::Failed,
            category,
            message: message.into(),
            retry_count: 0,
            evidence_tier: tier,
            action: None,
            readback: None,
            assertion: None,
            evidence: Vec::new(),
            normalized: None,
        }
    }

    pub fn blocked(category: V3Category, message: impl Into<String>) -> Self {
        Self {
            status: V3Status::Blocked,
            category,
            message: message.into(),
            retry_count: 0,
            evidence_tier: EvidenceTier::RealPreflight,
            action: None,
            readback: None,
            assertion: None,
            evidence: Vec::new(),
            normalized: None,
        }
    }

    pub fn error(category: V3Category, message: impl Into<String>, tier: EvidenceTier) -> Self {
        Self {
            status: V3Status::Error,
            category,
            message: message.into(),
            retry_count: 0,
            evidence_tier: tier,
            action: None,
            readback: None,
            assertion: None,
            evidence: Vec::new(),
            normalized: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedChange {
    pub idx: u64,
    pub expected: Value,
}

/// Match only a readback carrying the same entity, explicit index, and value.
/// HTTP vectors use `vector_index`; WS events/snapshots use `slot_idx`.
pub fn readback_matches(
    entity: &EntityRef,
    requested: &[RequestedChange],
    states: &[SlotState],
) -> bool {
    requested.iter().all(|change| {
        states.iter().any(|state| {
            state.entity.entity_id == entity.entity_id
                && state.entity.device_id == entity.device_id
                && state.entity.cell_id == entity.cell_id
                && (state.slot_idx == Some(change.idx)
                    || state.vector_index == Some(change.idx))
                && state.value == change.expected
        })
    })
}

pub fn classify_health(status: bool, tier: EvidenceTier) -> V3Result {
    if status {
        let normalized = serde_json::json!({"status": true});
        V3Result::passed(
            V3Category::Preflight,
            tier,
            serde_json::json!({"operation": "health"}),
            normalized.clone(),
            serde_json::json!({"status": true}),
            vec![
                EvidenceRecord {
                    kind: "http_response".into(),
                    payload: normalized.to_string(),
                    bytes: normalized.to_string().len(),
                    redacted: false,
                    truncated: false,
                },
                EvidenceRecord {
                    kind: "normalized_event".into(),
                    payload: normalized.to_string(),
                    bytes: normalized.to_string().len(),
                    redacted: false,
                    truncated: false,
                },
            ],
            Some(normalized),
        )
    } else {
        V3Result::failed(
            V3Category::AssertionMismatch,
            "HC health/readiness returned status=false",
            tier,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hc::backends::v3::runtime::canonical::{DecimalId, EntityRef, SlotState, StateSource};

    fn entity(id: &str) -> EntityRef {
        EntityRef {
            entity_id: DecimalId::parse(&Value::String(id.into()), "id").unwrap(),
            entity_kind: "device_cell".into(),
            cell_id: DecimalId::parse(&Value::String(id.into()), "id").unwrap(),
            device_id: DecimalId::parse(&Value::String(id.into()), "id").unwrap(),
            cell_model_id: DecimalId::from_u64(1),
            cell_idx: 0,
            protocol: "virtual".into(),
        }
    }

    #[test]
    fn classification_never_turns_dispatch_status_into_physical_pass() {
        assert_eq!(classify_health(true, EvidenceTier::MockContract).status, V3Status::Passed);
        assert_eq!(classify_health(false, EvidenceTier::MockContract).status, V3Status::Failed);
        assert_eq!(V3Result::blocked(V3Category::NoLab, "missing lab").status, V3Status::Blocked);
        assert_eq!(V3Result::error(V3Category::Timeout, "timed out", EvidenceTier::RealE2e).status, V3Status::Error);
    }

    #[test]
    fn readback_requires_entity_index_and_value() {
        let target = entity("42");
        let state = SlotState {
            entity: target.clone(),
            slot_idx: Some(2),
            vector_index: None,
            r#ref: None,
            value: Value::Bool(true),
            source: StateSource::WebsocketEvent,
            observed_at: "2026-10-18T00:00:00Z".into(),
            wire_timestamp: None,
            correlation_id: None,
        };
        let request = RequestedChange { idx: 2, expected: Value::Bool(true) };
        assert!(readback_matches(&target, &[request.clone()], &[state.clone()]));
        assert!(!readback_matches(&target, &[RequestedChange { idx: 1, ..request.clone() }], &[state.clone()]));
        assert!(!readback_matches(&entity("43"), &[request], &[state]));
    }
}
