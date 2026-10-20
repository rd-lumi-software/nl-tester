use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshot {
    pub schema_version: String,
    pub snapshot_id: String,
    pub source: String,
    pub captured_at: String,
    pub complete: bool,
    #[serde(default)]
    pub entities: Vec<InventoryEntity>,
}

impl InventorySnapshot {
    pub fn new(
        snapshot_id: impl Into<String>,
        complete: bool,
        expected_count: Option<usize>,
        entities: Vec<InventoryEntity>,
    ) -> Self {
        Self {
            schema_version: "1".into(),
            snapshot_id: snapshot_id.into(),
            source: "live_hc_discovery".into(),
            captured_at: chrono::Utc::now().to_rfc3339(),
            complete: complete && expected_count.map_or(true, |count| count == entities.len()),
            entities,
        }
    }

    pub fn stable(&self) -> bool {
        self.complete && !self.entities.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InventoryEntity {
    pub entity: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub fingerprint: String,
    #[serde(default)]
    pub mapping_fingerprint: Option<String>,
}

impl InventoryEntity {
    pub fn new<I, S>(
        entity: impl Into<String>,
        capabilities: I,
        fingerprint: Option<impl Into<String>>,
        mapping_fingerprint: Option<impl Into<String>>,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            entity: entity.into(),
            capabilities: capabilities.into_iter().map(Into::into).collect(),
            fingerprint: fingerprint.map(Into::into).unwrap_or_default(),
            mapping_fingerprint: mapping_fingerprint.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InventoryChange {
    pub entity: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InventoryComparison {
    pub baseline_snapshot_id: String,
    pub current_snapshot_id: String,
    pub drifted: bool,
    pub unverifiable: bool,
    pub mapping_verifiable: bool,
    pub changes: Vec<InventoryChange>,
}

impl InventoryComparison {
    pub fn stable(&self) -> bool {
        !self.drifted && !self.unverifiable && self.mapping_verifiable
    }

    pub fn drift(&self) -> &[InventoryChange] {
        &self.changes
    }
}

pub fn compare_inventory(
    baseline: &InventorySnapshot,
    current: &InventorySnapshot,
) -> InventoryComparison {
    let mut changes = Vec::new();
    let mut baseline_by_id = BTreeMap::new();
    let mut current_by_id = BTreeMap::new();

    if !baseline.complete {
        changes.push(InventoryChange {
            entity: "*".to_string(),
            kind: "unverifiable".to_string(),
            detail: "baseline inventory is incomplete".to_string(),
        });
    }
    if !current.complete {
        changes.push(InventoryChange {
            entity: "*".to_string(),
            kind: "unverifiable".to_string(),
            detail: "current inventory is incomplete".to_string(),
        });
    }

    for entity in &baseline.entities {
        if baseline_by_id
            .insert(entity.entity.clone(), entity)
            .is_some()
        {
            changes.push(InventoryChange {
                entity: entity.entity.clone(),
                kind: "unverifiable".to_string(),
                detail: "baseline contains duplicate entity IDs".to_string(),
            });
        }
        if entity.fingerprint.trim().is_empty() {
            changes.push(InventoryChange {
                entity: entity.entity.clone(),
                kind: "unverifiable".to_string(),
                detail: "baseline entity fingerprint is missing".to_string(),
            });
        }
    }
    for entity in &current.entities {
        if current_by_id
            .insert(entity.entity.clone(), entity)
            .is_some()
        {
            changes.push(InventoryChange {
                entity: entity.entity.clone(),
                kind: "unverifiable".to_string(),
                detail: "current inventory contains duplicate entity IDs".to_string(),
            });
        }
        if entity.fingerprint.trim().is_empty() {
            changes.push(InventoryChange {
                entity: entity.entity.clone(),
                kind: "unverifiable".to_string(),
                detail: "current entity fingerprint is missing".to_string(),
            });
        }
    }

    for entity in baseline_by_id.keys() {
        if !current_by_id.contains_key(entity) {
            changes.push(InventoryChange {
                entity: entity.clone(),
                kind: "removed".to_string(),
                detail: "entity was present in baseline but is missing currently".to_string(),
            });
        }
    }
    for entity in current_by_id.keys() {
        if !baseline_by_id.contains_key(entity) {
            changes.push(InventoryChange {
                entity: entity.clone(),
                kind: "added".to_string(),
                detail: "entity is present currently but was absent from baseline".to_string(),
            });
        }
    }

    let mut mapping_verifiable = true;
    for (entity_id, baseline_entity) in &baseline_by_id {
        let Some(current_entity) = current_by_id.get(entity_id) else {
            continue;
        };
        let mut baseline_capabilities = baseline_entity.capabilities.clone();
        let mut current_capabilities = current_entity.capabilities.clone();
        baseline_capabilities.sort();
        current_capabilities.sort();
        if baseline_entity.fingerprint != current_entity.fingerprint
            || baseline_capabilities != current_capabilities
        {
            changes.push(InventoryChange {
                entity: entity_id.clone(),
                kind: "changed".to_string(),
                detail: "entity fingerprint or capabilities changed".to_string(),
            });
        }
        match (
            baseline_entity.mapping_fingerprint.as_deref(),
            current_entity.mapping_fingerprint.as_deref(),
        ) {
            (Some(baseline_mapping), Some(current_mapping))
                if baseline_mapping != current_mapping =>
            {
                changes.push(InventoryChange {
                    entity: entity_id.clone(),
                    kind: "mapping_changed".to_string(),
                    detail: "resolved target mapping fingerprint changed".to_string(),
                });
            }
            (Some(_), None) | (None, Some(_)) | (None, None) => {
                mapping_verifiable = false;
                changes.push(InventoryChange {
                    entity: entity_id.clone(),
                    kind: "unverifiable".to_string(),
                    detail: "mapping fingerprint is missing from one or both snapshots".to_string(),
                });
            }
            _ => {}
        }
    }

    let unverifiable = changes.iter().any(|change| change.kind == "unverifiable");
    InventoryComparison {
        baseline_snapshot_id: baseline.snapshot_id.clone(),
        current_snapshot_id: current.snapshot_id.clone(),
        drifted: !changes.is_empty(),
        unverifiable,
        mapping_verifiable,
        changes,
    }
}

pub fn unverifiable_comparison(
    baseline_snapshot_id: &str,
    current_snapshot_id: &str,
    detail: impl Into<String>,
) -> InventoryComparison {
    InventoryComparison {
        baseline_snapshot_id: baseline_snapshot_id.to_string(),
        current_snapshot_id: current_snapshot_id.to_string(),
        drifted: true,
        unverifiable: true,
        mapping_verifiable: false,
        changes: vec![InventoryChange {
            entity: "*".to_string(),
            kind: "unverifiable".to_string(),
            detail: detail.into(),
        }],
    }
}
