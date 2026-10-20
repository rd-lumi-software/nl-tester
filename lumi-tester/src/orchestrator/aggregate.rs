use crate::orchestrator::delegation::{DelegatedTargetResult, DelegationResponse};
use crate::orchestrator::inventory::{compare_inventory, InventoryComparison, InventorySnapshot};
use crate::orchestrator::plan::OrchestrationPlan;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepetitionSummary {
    #[serde(default)]
    pub requested: u32,
    #[serde(default)]
    pub completed: u32,
    #[serde(default)]
    pub passed: u32,
    #[serde(default)]
    pub failed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvidencePaths {
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TargetResultInput {
    pub entity: String,
    pub capability: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(alias = "status")]
    pub outcome: String,
    #[serde(default)]
    pub repetitions: RepetitionSummary,
    #[serde(default)]
    pub evidence_tier: String,
    #[serde(default)]
    pub dispatch_latency_ms: Vec<u64>,
    #[serde(default)]
    pub readback_latency_ms: Vec<u64>,
    #[serde(default)]
    pub expected_state: Option<Value>,
    #[serde(default)]
    pub observed_state: Option<Value>,
    #[serde(default)]
    pub final_state: Option<Value>,
    #[serde(default)]
    pub evidence: EvidencePaths,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub restore_status: Option<String>,
}

impl TargetResultInput {
    pub fn new(
        entity: impl Into<String>,
        capability: impl Into<String>,
        outcome: impl Into<String>,
    ) -> Self {
        Self {
            entity: entity.into(),
            capability: capability.into(),
            outcome: outcome.into(),
            ..Self::default()
        }
    }

    pub fn from_delegated(target: &DelegatedTargetResult) -> Self {
        Self {
            entity: target.entity.clone(),
            capability: target.capability.clone(),
            scenario: target.scenario.clone(),
            outcome: target.status.clone(),
            repetitions: RepetitionSummary {
                requested: target.repetitions.requested,
                completed: target.repetitions.completed,
                passed: target.repetitions.passed,
                failed: target.repetitions.failed,
            },
            evidence_tier: target.evidence_tier.clone().unwrap_or_default(),
            expected_state: target.expected_state.clone(),
            observed_state: target.observed_state.clone(),
            final_state: target.final_state.clone(),
            artifact_paths: target
                .artifact_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            reason: target.reason.clone(),
            restore_status: target
                .restoration
                .as_ref()
                .and_then(|restoration| restoration.status.clone()),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AggregateInput {
    pub plan_id: String,
    pub run_id: String,
    pub evidence_tier: String,
    #[serde(default)]
    pub inventory: Option<InventoryComparison>,
    #[serde(default)]
    pub targets: Vec<TargetResultInput>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub restoration_status: Option<String>,
    #[serde(default)]
    pub restore_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AggregateSummary {
    pub discovered: usize,
    pub eligible: usize,
    pub executed: usize,
    pub passed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub skipped: usize,
    pub errors: usize,
    pub not_run: usize,
    pub quarantined: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AggregateTarget {
    pub entity: String,
    pub capability: String,
    pub scenario: String,
    pub outcome: String,
    pub repetitions: RepetitionSummary,
    pub evidence_tier: String,
    #[serde(default)]
    pub dispatch_latency_ms: Vec<u64>,
    #[serde(default)]
    pub readback_latency_ms: Vec<u64>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub expected_state: Option<Value>,
    #[serde(default)]
    pub observed_state: Option<Value>,
    #[serde(default)]
    pub final_state: Option<Value>,
    pub evidence: EvidencePaths,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub restore_status: Option<String>,
    #[serde(default)]
    pub quarantined: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AggregateReport {
    pub schema_version: String,
    pub plan_id: String,
    pub run_id: String,
    pub status: String,
    pub evidence_tier: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
    pub summary: AggregateSummary,
    pub targets: Vec<AggregateTarget>,
    #[serde(default)]
    pub inventory: Option<InventoryComparison>,
    #[serde(default)]
    pub restoration_status: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

pub fn aggregate_run(input: &AggregateInput) -> AggregateReport {
    let targets = input
        .targets
        .iter()
        .map(|target| aggregate_target(target, input.restore_required))
        .collect::<Vec<_>>();
    let mut summary = AggregateSummary::default();
    summary.discovered = targets.len();
    summary.eligible = targets
        .iter()
        .filter(|target| !matches!(target.outcome.as_str(), "blocked" | "skipped"))
        .count();
    summary.executed = targets
        .iter()
        .filter(|target| matches!(target.outcome.as_str(), "passed" | "failed" | "error"))
        .count();
    for target in &targets {
        match target.outcome.as_str() {
            "passed" => summary.passed += 1,
            "failed" => summary.failed += 1,
            "blocked" => summary.blocked += 1,
            "skipped" => summary.skipped += 1,
            "error" => summary.errors += 1,
            "not_run" => summary.not_run += 1,
            _ => summary.errors += 1,
        }
        if target.quarantined {
            summary.quarantined += 1;
        }
    }
    let stop_reason = input.inventory.as_ref().and_then(|inventory| {
        if inventory.drifted || inventory.unverifiable || !inventory.mapping_verifiable {
            Some(if inventory.unverifiable {
                "inventory_unverifiable".to_string()
            } else {
                "inventory_drift".to_string()
            })
        } else {
            None
        }
    });
    let status = overall_status(&summary, stop_reason.is_some());
    AggregateReport {
        schema_version: "1".into(),
        plan_id: input.plan_id.clone(),
        run_id: input.run_id.clone(),
        status,
        evidence_tier: input.evidence_tier.clone(),
        stop_reason,
        summary,
        targets,
        inventory: input.inventory.clone(),
        restoration_status: input.restoration_status.clone(),
        artifacts: input.artifacts.clone(),
    }
}

pub fn aggregate_response(
    plan: &OrchestrationPlan,
    response: &DelegationResponse,
    current_inventory: &InventorySnapshot,
) -> anyhow::Result<AggregateReport> {
    let baseline = InventorySnapshot {
        schema_version: "1".into(),
        snapshot_id: plan
            .inventory
            .snapshot_id
            .clone()
            .unwrap_or_else(|| "plan-inventory".into()),
        source: plan.inventory.source.clone(),
        captured_at: plan.inventory.captured_at.clone().unwrap_or_default(),
        complete: plan.inventory.complete,
        entities: plan.inventory.entities.clone(),
    };
    let comparison = compare_inventory(&baseline, current_inventory);
    let mut seen = BTreeSet::new();
    let mut targets = response
        .target_results
        .iter()
        .filter(|target| seen.insert(target.entity.clone()))
        .map(TargetResultInput::from_delegated)
        .collect::<Vec<_>>();
    for target in &plan.targets {
        if !seen.contains(&target.entity) {
            targets.push(planned_target_input(target));
        }
    }
    Ok(aggregate_run(&AggregateInput {
        plan_id: plan.plan_id.clone(),
        run_id: response
            .output_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        evidence_tier: response.evidence_tier.clone(),
        inventory: Some(comparison),
        targets,
        artifacts: response
            .artifact_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        restoration_status: response
            .restoration
            .as_ref()
            .and_then(|restoration| restoration.status.clone()),
        restore_required: plan.authorization.restore_state,
    }))
}

pub fn aggregate_plan(
    plan: &OrchestrationPlan,
    responses: &[DelegationResponse],
    current_inventory: Option<&InventorySnapshot>,
) -> AggregateReport {
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();
    let mut artifacts = Vec::new();
    let mut evidence_tier = String::new();
    let mut restoration_status = None;
    for response in responses {
        if evidence_tier.is_empty() {
            evidence_tier = response.evidence_tier.clone();
        }
        artifacts.extend(
            response
                .artifact_paths
                .iter()
                .map(|path| path.display().to_string()),
        );
        restoration_status = response
            .restoration
            .as_ref()
            .and_then(|restoration| restoration.status.clone());
        for target in &response.target_results {
            if seen.insert(target.entity.clone()) {
                targets.push(TargetResultInput::from_delegated(target));
            }
        }
    }
    for target in &plan.targets {
        if seen.insert(target.entity.clone()) {
            targets.push(planned_target_input(target));
        }
    }
    let inventory = current_inventory.map(|current| {
        let baseline = InventorySnapshot {
            schema_version: "1".into(),
            snapshot_id: plan
                .inventory
                .snapshot_id
                .clone()
                .unwrap_or_else(|| "plan-inventory".into()),
            source: plan.inventory.source.clone(),
            captured_at: plan.inventory.captured_at.clone().unwrap_or_default(),
            complete: plan.inventory.complete,
            entities: plan.inventory.entities.clone(),
        };
        compare_inventory(&baseline, current)
    });
    aggregate_run(&AggregateInput {
        plan_id: plan.plan_id.clone(),
        run_id: "orchestrator-run".into(),
        evidence_tier,
        inventory,
        targets,
        artifacts,
        restoration_status,
        restore_required: plan.authorization.restore_state,
    })
}

pub fn write_aggregate(report: &AggregateReport, output_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(output_dir)?;
    std::fs::write(
        output_dir.join("aggregate.json"),
        serde_json::to_string_pretty(report)? + "\n",
    )?;
    Ok(())
}

fn aggregate_target(target: &TargetResultInput, restore_required: bool) -> AggregateTarget {
    let latency_ms = target
        .readback_latency_ms
        .iter()
        .copied()
        .chain(target.dispatch_latency_ms.iter().copied())
        .max();
    let restore_missing = restore_required
        && target.outcome == "passed"
        && target.restore_status.as_deref() != Some("verified");
    let outcome = if restore_missing {
        "failed".to_string()
    } else {
        target.outcome.clone()
    };
    let reason = if restore_missing {
        Some("required initial-state restoration was not verified".to_string())
    } else {
        target.reason.clone()
    };
    AggregateTarget {
        entity: target.entity.clone(),
        capability: target.capability.clone(),
        scenario: target.scenario.clone(),
        outcome,
        repetitions: target.repetitions.clone(),
        evidence_tier: target.evidence_tier.clone(),
        dispatch_latency_ms: target.dispatch_latency_ms.clone(),
        readback_latency_ms: target.readback_latency_ms.clone(),
        latency_ms,
        expected_state: target.expected_state.clone(),
        observed_state: target.observed_state.clone(),
        final_state: target.final_state.clone(),
        evidence: target.evidence.clone(),
        artifact_paths: target.artifact_paths.clone(),
        reason,
        restore_status: target.restore_status.clone(),
        quarantined: restore_missing || matches!(target.outcome.as_str(), "failed" | "error"),
    }
}

fn planned_target_input(target: &crate::orchestrator::plan::PlanTarget) -> TargetResultInput {
    TargetResultInput {
        entity: target.entity.clone(),
        capability: target.capability.clone(),
        outcome: if target.status == "eligible" {
            "not_run".into()
        } else {
            target.status.clone()
        },
        reason: target.reason.clone(),
        ..TargetResultInput::default()
    }
}

fn overall_status(summary: &AggregateSummary, run_level_stop: bool) -> String {
    if run_level_stop {
        "blocked".into()
    } else if summary.errors > 0 {
        "error".into()
    } else if summary.failed > 0 {
        "failed".into()
    } else if summary.blocked > 0 {
        "blocked".into()
    } else if summary.not_run > 0 {
        "not_run".into()
    } else if summary.skipped > 0 && summary.passed == 0 {
        "skipped".into()
    } else {
        "passed".into()
    }
}
