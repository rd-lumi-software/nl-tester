use super::command::HcStateUpdate;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// HC-owned outcome status, kept independent from adapter status types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcStatus {
    Passed,
    Failed,
    Blocked,
    Error,
}

/// A capability advertised by an HC backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HcCapability {
    pub name: String,
    pub value: Value,
}

impl HcCapability {
    /// Create a named capability declaration.
    pub fn new(name: impl Into<String>, value: Value) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

/// Evidence captured by a backend for a protocol-neutral operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HcEvidence {
    pub kind: String,
    pub raw: Value,
}

impl HcEvidence {
    /// Wrap one raw evidence payload with its normalized kind.
    pub fn raw(kind: impl Into<String>, raw: Value) -> Self {
        Self {
            kind: kind.into(),
            raw,
        }
    }
}

/// Evidence confidence retained by the HC boundary for later report mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcEvidenceTier {
    MockContract,
    MockConformance,
    RealPreflight,
    RealE2e,
}

/// A normalized HC entity with a protocol-neutral string identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HcEntity {
    pub id: String,
    pub name: Option<String>,
    pub capabilities: Vec<HcCapability>,
}

/// A normalized state observation for an entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HcState {
    pub entity: String,
    pub updates: Vec<HcStateUpdate>,
}

/// Normalized data optionally returned by a backend operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HcObservation {
    pub entities: Option<Vec<HcEntity>>,
    pub entity: Option<HcEntity>,
    pub state: Option<HcState>,
    pub value: Option<Value>,
}

impl HcObservation {
    /// Create a discovery observation containing normalized entities.
    pub fn discovery(entities: Vec<HcEntity>) -> Self {
        Self {
            entities: Some(entities),
            entity: None,
            state: None,
            value: None,
        }
    }

    /// Attach one normalized entity state observation.
    pub fn with_state(mut self, state: HcState) -> Self {
        self.state = Some(state);
        self
    }
}

/// Protocol-neutral backend output consumed by a later adapter facade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HcBackendResult {
    pub status: HcStatus,
    pub category: String,
    pub message: String,
    pub action: Option<Value>,
    pub readback: Option<Value>,
    pub assertion: Option<Value>,
    pub evidence: Vec<HcEvidence>,
    pub normalized: Option<Value>,
    pub evidence_tier: HcEvidenceTier,
    pub observation: Option<HcObservation>,
    pub readback_match: Option<bool>,
}

impl HcBackendResult {
    /// Build a successful backend result.
    pub fn passed(message: impl Into<String>) -> Self {
        Self::new(HcStatus::Passed, "completed", message)
    }

    /// Build a failed backend result.
    pub fn failed(category: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HcStatus::Failed, category, message)
    }

    /// Build a blocked backend result.
    pub fn blocked(category: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HcStatus::Blocked, category, message)
    }

    /// Build an error backend result.
    pub fn error(category: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HcStatus::Error, category, message)
    }

    /// Attach normalized observation data.
    pub fn with_observation(mut self, observation: HcObservation) -> Self {
        self.observation = Some(observation);
        self
    }

    /// Attach one raw evidence wrapper.
    pub fn with_evidence(mut self, evidence: HcEvidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    /// Attach the action sent to the backend.
    pub fn with_action(mut self, action: Value) -> Self {
        self.action = Some(action);
        self
    }

    /// Attach the backend readback.
    pub fn with_readback(mut self, readback: Value) -> Self {
        self.readback = Some(readback);
        self
    }

    /// Attach the normalized assertion result.
    pub fn with_assertion(mut self, assertion: Value) -> Self {
        self.assertion = Some(assertion);
        self
    }

    /// Attach normalized evidence.
    pub fn with_normalized(mut self, normalized: Value) -> Self {
        self.normalized = Some(normalized);
        self
    }

    /// Set the evidence confidence tier.
    pub fn with_evidence_tier(mut self, evidence_tier: HcEvidenceTier) -> Self {
        self.evidence_tier = evidence_tier;
        self
    }

    /// Attach the result of a bounded readback assertion.
    pub fn with_readback_match(mut self, matched: bool) -> Self {
        self.readback_match = Some(matched);
        self
    }

    fn new(status: HcStatus, category: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            category: category.into(),
            message: message.into(),
            action: None,
            readback: None,
            assertion: None,
            evidence: Vec::new(),
            normalized: None,
            evidence_tier: if status == HcStatus::Blocked {
                HcEvidenceTier::RealPreflight
            } else {
                HcEvidenceTier::MockContract
            },
            observation: None,
            readback_match: None,
        }
    }
}
