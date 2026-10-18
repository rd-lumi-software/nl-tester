use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use thiserror::Error;

/// Precision-safe decimal representation of a v3 u64 identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecimalId(String);

impl DecimalId {
    pub fn parse(value: &Value, field: &str) -> Result<Self, CanonicalError> {
        let text = match value {
            Value::String(value) => value.clone(),
            Value::Number(value) if value.as_u64().is_some() => value.to_string(),
            _ => {
                return Err(CanonicalError::InvalidId {
                    field: field.to_string(),
                    value: value.to_string(),
                })
            }
        };
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(CanonicalError::InvalidId {
                field: field.to_string(),
                value: text,
            });
        }
        let parsed = text.parse::<u64>().map_err(|_| CanonicalError::InvalidId {
            field: field.to_string(),
            value: text.clone(),
        })?;
        Ok(Self(parsed.to_string()))
    }

    pub fn from_u64(value: u64) -> Self {
        Self(value.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_u64(&self) -> u64 {
        self.0.parse().expect("DecimalId is validated")
    }
}

impl fmt::Display for DecimalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error)]
pub enum CanonicalError {
    #[error("canonical payload must be an object")]
    ExpectedObject,
    #[error("canonical payload is missing '{field}'")]
    MissingField { field: String },
    #[error("canonical field '{field}' must be a non-negative decimal u64: {value}")]
    InvalidId { field: String, value: String },
    #[error("canonical field '{field}' must be a non-negative integer")]
    InvalidIndex { field: String },
    #[error("canonical state vector must be an array")]
    InvalidStateVector,
    #[error("canonical runtime frame has an unsupported shape")]
    InvalidRuntimeFrame,
    #[error("runtime frame cell_id '{actual}' does not match '{expected}'")]
    EntityMismatch { actual: String, expected: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    pub entity_id: DecimalId,
    pub entity_kind: String,
    pub cell_id: DecimalId,
    pub device_id: DecimalId,
    pub cell_model_id: DecimalId,
    pub cell_idx: u64,
    pub protocol: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalEntity {
    pub entity: EntityRef,
    pub raw: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateSource {
    HttpState,
    WebsocketEvent,
    WebsocketSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotState {
    pub entity: EntityRef,
    pub slot_idx: Option<u64>,
    pub vector_index: Option<u64>,
    pub r#ref: Option<String>,
    pub value: Value,
    pub source: StateSource,
    pub observed_at: String,
    pub wire_timestamp: Option<String>,
    pub correlation_id: Option<DecimalId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellEvent {
    pub entity: EntityRef,
    pub event_type: String,
    pub params: Value,
    pub source: StateSource,
    pub observed_at: String,
    pub wire_timestamp: Option<String>,
    pub correlation_id: Option<DecimalId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CanonicalObservation {
    State(Vec<SlotState>),
    Event(CellEvent),
}

pub fn map_device_entity(raw: &Value) -> Result<CanonicalEntity, CanonicalError> {
    let object = raw.as_object().ok_or(CanonicalError::ExpectedObject)?;
    let id = decimal_field(object, "id")?;
    let cell_model_id = decimal_field(object, "cell_model_id")?;
    let cell_idx = index_field(object, "cell_idx")?;
    let protocol = required_string(object, "protocol")?;
    Ok(CanonicalEntity {
        entity: EntityRef {
            entity_id: id.clone(),
            entity_kind: "device_cell".to_string(),
            cell_id: id.clone(),
            device_id: id,
            cell_model_id,
            cell_idx,
            protocol,
        },
        raw: raw.clone(),
    })
}

pub fn map_state_vector(
    entity: &EntityRef,
    raw: &Value,
    observed_at: impl Into<String>,
    source: StateSource,
) -> Result<Vec<SlotState>, CanonicalError> {
    let values = raw.as_array().ok_or(CanonicalError::InvalidStateVector)?;
    let observed_at = observed_at.into();
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            Ok(SlotState {
                entity: entity.clone(),
                slot_idx: None,
                vector_index: Some(
                    u64::try_from(index).map_err(|_| CanonicalError::InvalidIndex {
                        field: "vector_index".to_string(),
                    })?,
                ),
                r#ref: None,
                value: value.clone(),
                source,
                observed_at: observed_at.clone(),
                wire_timestamp: None,
                correlation_id: None,
            })
        })
        .collect()
}

pub fn map_snapshot(
    entity: &EntityRef,
    raw: &Value,
    observed_at: impl Into<String>,
    correlation_id: DecimalId,
) -> Result<Vec<SlotState>, CanonicalError> {
    let values = raw.as_array().ok_or(CanonicalError::InvalidStateVector)?;
    let observed_at = observed_at.into();
    values
        .iter()
        .map(|value| {
            let object = value.as_object().ok_or(CanonicalError::InvalidRuntimeFrame)?;
            let index = index_field(object, "idx")?;
            let state_value = object
                .get("value")
                .ok_or_else(|| CanonicalError::MissingField {
                    field: "value".to_string(),
                })?;
            Ok(SlotState {
                entity: entity.clone(),
                slot_idx: Some(index),
                vector_index: None,
                r#ref: None,
                value: state_value.clone(),
                source: StateSource::WebsocketSnapshot,
                observed_at: observed_at.clone(),
                wire_timestamp: None,
                correlation_id: Some(correlation_id.clone()),
            })
        })
        .collect()
}

pub fn map_runtime_frame(
    entity: &EntityRef,
    raw: &Value,
    observed_at: impl Into<String>,
) -> Result<CanonicalObservation, CanonicalError> {
    let envelope = raw.as_object().ok_or(CanonicalError::InvalidRuntimeFrame)?;
    if envelope.get("type").and_then(Value::as_str) != Some("data") {
        return Err(CanonicalError::InvalidRuntimeFrame);
    }
    let payload = envelope
        .get("payload")
        .and_then(Value::as_object)
        .ok_or(CanonicalError::InvalidRuntimeFrame)?;
    if payload.get("channel").and_then(Value::as_str) != Some("cell_runtime_data") {
        return Err(CanonicalError::InvalidRuntimeFrame);
    }
    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .ok_or(CanonicalError::InvalidRuntimeFrame)?;
    let frame_cell_id = DecimalId::parse(
        data.get("cell_id").ok_or_else(|| CanonicalError::MissingField {
            field: "cell_id".to_string(),
        })?,
        "cell_id",
    )?;
    if frame_cell_id != entity.cell_id {
        return Err(CanonicalError::EntityMismatch {
            actual: frame_cell_id.to_string(),
            expected: entity.cell_id.to_string(),
        });
    }
    let content = data
        .get("content")
        .and_then(Value::as_object)
        .ok_or(CanonicalError::InvalidRuntimeFrame)?;
    let content_type = content
        .get("type")
        .and_then(Value::as_str)
        .ok_or(CanonicalError::InvalidRuntimeFrame)?;
    let observed_at = observed_at.into();
    match content_type {
        "cell_state_changed" => map_snapshot_like_event(entity, content, observed_at),
        "cell_event" => {
            let payload = content
                .get("payload")
                .and_then(Value::as_object)
                .ok_or(CanonicalError::InvalidRuntimeFrame)?;
            let event_type = payload
                .get("event")
                .and_then(Value::as_str)
                .ok_or(CanonicalError::InvalidRuntimeFrame)?;
            let params = payload.get("params").cloned().unwrap_or(Value::Object(Map::new()));
            Ok(CanonicalObservation::Event(CellEvent {
                entity: entity.clone(),
                event_type: event_type.to_string(),
                params,
                source: StateSource::WebsocketEvent,
                observed_at,
                wire_timestamp: None,
                correlation_id: None,
            }))
        }
        _ => Err(CanonicalError::InvalidRuntimeFrame),
    }
}

fn map_snapshot_like_event(
    entity: &EntityRef,
    content: &Map<String, Value>,
    observed_at: String,
) -> Result<CanonicalObservation, CanonicalError> {
    let payload = content
        .get("payload")
        .ok_or_else(|| CanonicalError::MissingField {
            field: "payload".to_string(),
        })?;
    let values = payload.as_array().ok_or(CanonicalError::InvalidStateVector)?;
    let states = values
        .iter()
        .map(|value| {
            let object = value.as_object().ok_or(CanonicalError::InvalidRuntimeFrame)?;
            let index = index_field(object, "idx")?;
            let state_value = object
                .get("value")
                .ok_or_else(|| CanonicalError::MissingField {
                    field: "value".to_string(),
                })?;
            Ok(SlotState {
                entity: entity.clone(),
                slot_idx: Some(index),
                vector_index: None,
                r#ref: None,
                value: state_value.clone(),
                source: StateSource::WebsocketEvent,
                observed_at: observed_at.clone(),
                wire_timestamp: None,
                correlation_id: None,
            })
        })
        .collect::<Result<Vec<_>, CanonicalError>>()?;
    Ok(CanonicalObservation::State(states))
}

fn required_string(object: &Map<String, Value>, field: &str) -> Result<String, CanonicalError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CanonicalError::MissingField {
            field: field.to_string(),
        })
}

fn decimal_field(object: &Map<String, Value>, field: &str) -> Result<DecimalId, CanonicalError> {
    DecimalId::parse(
        object.get(field).ok_or_else(|| CanonicalError::MissingField {
            field: field.to_string(),
        })?,
        field,
    )
}

fn index_field(object: &Map<String, Value>, field: &str) -> Result<u64, CanonicalError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| CanonicalError::InvalidIndex {
            field: field.to_string(),
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasMapping {
    pub alias: String,
    pub input_idx: u64,
    pub state_idx: u64,
}

impl AliasMapping {
    pub fn resolve(&self, alias: &str) -> Option<(u64, u64)> {
        (self.alias == alias).then_some((self.input_idx, self.state_idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity() -> EntityRef {
        EntityRef {
            entity_id: DecimalId::from_u64(9_007_199_254_740_993),
            entity_kind: "device_cell".into(),
            cell_id: DecimalId::from_u64(9_007_199_254_740_993),
            device_id: DecimalId::from_u64(9_007_199_254_740_993),
            cell_model_id: DecimalId::from_u64(18_446_744_073_709_551_615),
            cell_idx: 0,
            protocol: "virtual".into(),
        }
    }

    #[test]
    fn decimal_ids_are_canonical_strings_without_precision_loss() {
        let id = DecimalId::parse(&serde_json::json!("9007199254740993"), "id").unwrap();
        assert_eq!(id.as_str(), "9007199254740993");
        assert!(DecimalId::parse(&serde_json::json!(1.5), "id").is_err());
        assert!(DecimalId::parse(&serde_json::json!("1e3"), "id").is_err());
    }

    #[test]
    fn runtime_state_keeps_entity_index_value_and_no_fake_event_correlation() {
        let entity = entity();
        let frame = serde_json::json!({
            "type":"data", "payload":{"channel":"cell_runtime_data", "data":{
                "cell_id":"9007199254740993",
                "content":{"type":"cell_state_changed","payload":[{"idx":0,"value":true}]}
            }}
        });
        let CanonicalObservation::State(states) =
            map_runtime_frame(&entity, &frame, "2026-10-18T00:00:00Z").unwrap()
        else { panic!("expected state") };
        assert_eq!(states[0].entity.cell_id.as_str(), "9007199254740993");
        assert_eq!(states[0].slot_idx, Some(0));
        assert_eq!(states[0].value, Value::Bool(true));
        assert_eq!(states[0].correlation_id, None);
    }

    #[test]
    fn aliases_only_resolve_when_a_pack_mapping_exists() {
        assert!(AliasMapping { alias: "power".into(), input_idx: 0, state_idx: 0 }
            .resolve("power").is_some());
        assert!(AliasMapping { alias: "power".into(), input_idx: 0, state_idx: 0 }
            .resolve("brightness").is_none());
    }
}
