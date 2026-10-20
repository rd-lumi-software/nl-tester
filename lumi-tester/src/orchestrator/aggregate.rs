use crate::orchestrator::delegation::{DelegatedActionResult, DelegatedTargetResult, DelegationResponse};
use crate::orchestrator::inventory::{compare_inventory, InventoryComparison, InventorySnapshot};
use crate::orchestrator::lifecycle::{TargetLifecycle, TargetState};
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
    pub initial_state: Option<Value>,
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
    #[serde(default)]
    pub lifecycle: Option<TargetLifecycle>,
    #[serde(default)]
    pub action_results: Vec<DelegatedActionResult>,
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

    pub fn from_delegated(target: &DelegatedTargetResult, output_dir: Option<&Path>) -> Self {
        let artifact_paths = target
            .artifact_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
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
            dispatch_latency_ms: target.dispatch_latency_ms.clone(),
            readback_latency_ms: target.readback_latency_ms.clone(),
            initial_state: target.initial_state.clone(),
            expected_state: target.expected_state.clone(),
            observed_state: target.observed_state.clone(),
            final_state: target.final_state.clone(),
            evidence: EvidencePaths {
                output_dir: output_dir.map(|path| path.display().to_string()),
                artifacts: artifact_paths.clone(),
            },
            artifact_paths,
            reason: target.reason.clone(),
            restore_status: target
                .restoration
                .as_ref()
                .and_then(|restoration| restoration.status.clone()),
            lifecycle: target.lifecycle.clone(),
            action_results: target.action_results.clone(),
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
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub top_level_status: Option<String>,
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
    pub initial_state: Option<Value>,
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
    #[serde(default)]
    pub lifecycle: TargetLifecycle,
    #[serde(default)]
    pub action_results: Vec<DelegatedActionResult>,
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
    pub lifecycle: Vec<TargetLifecycle>,
    #[serde(default)]
    pub inventory: Option<InventoryComparison>,
    #[serde(default)]
    pub restoration_status: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
}

pub fn aggregate_run(input: &AggregateInput) -> AggregateReport {
    let inventory_stop_reason = input.inventory.as_ref().and_then(|inventory| {
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
    let stop_reason = input.stop_reason.clone().or(inventory_stop_reason);
    let targets = input
        .targets
        .iter()
        .map(|target| aggregate_target(target, input.restore_required, stop_reason.as_deref()))
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
    let run_level_stop = stop_reason
        .as_deref()
        .is_some_and(|reason| reason != "duplicate_target_identity");
    let status = overall_status(&summary, run_level_stop, input.top_level_status.as_deref());
    let lifecycle = targets
        .iter()
        .map(|target| target.lifecycle.clone())
        .collect();
    let restoration_status = aggregate_restoration_status(
        input.restore_required,
        input.restoration_status.clone(),
        &targets,
    );
    AggregateReport {
        schema_version: "1".into(),
        plan_id: input.plan_id.clone(),
        run_id: input.run_id.clone(),
        status,
        evidence_tier: input.evidence_tier.clone(),
        stop_reason,
        summary,
        targets,
        lifecycle,
        inventory: input.inventory.clone(),
        restoration_status,
        artifacts: input.artifacts.clone(),
        output_dir: input.output_dir.clone(),
    }
}

pub fn aggregate_response(
    plan: &OrchestrationPlan,
    response: &DelegationResponse,
    current_inventory: &InventorySnapshot,
) -> anyhow::Result<AggregateReport> {
    Ok(aggregate_plan_with_baseline_at(
        plan,
        &[response.clone()],
        None,
        Some(current_inventory),
        Path::new("."),
    ))
}

pub fn aggregate_plan(
    plan: &OrchestrationPlan,
    responses: &[DelegationResponse],
    current_inventory: Option<&InventorySnapshot>,
) -> AggregateReport {
    aggregate_plan_with_baseline_at(plan, responses, None, current_inventory, Path::new("."))
}

pub fn aggregate_plan_with_baseline(
    plan: &OrchestrationPlan,
    responses: &[DelegationResponse],
    external_baseline: Option<&InventorySnapshot>,
    current_inventory: Option<&InventorySnapshot>,
) -> AggregateReport {
    aggregate_plan_with_baseline_at(
        plan,
        responses,
        external_baseline,
        current_inventory,
        Path::new("."),
    )
}

pub fn aggregate_plan_with_baseline_at(
    plan: &OrchestrationPlan,
    responses: &[DelegationResponse],
    external_baseline: Option<&InventorySnapshot>,
    current_inventory: Option<&InventorySnapshot>,
    base_dir: &Path,
) -> AggregateReport {
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();
    let mut covered_capabilities = BTreeSet::new();
    let mut artifacts = Vec::new();
    let mut evidence_tier = String::new();
    let mut restoration_status = None;
    let mut stop_reason = None;
    let mut duplicate_target_identity = false;
    let mut output_dir = None;
    let mut top_level_status = None;
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
        if restoration_status.is_none() {
            restoration_status = response
                .restoration
                .as_ref()
                .and_then(|restoration| restoration.status.clone());
        }
        if restoration_status.is_none() && !plan.authorization.restore_state {
            restoration_status = Some("not_requested".into());
        }
        if output_dir.is_none() {
            output_dir = response
                .output_dir
                .as_ref()
                .map(|path| path.display().to_string());
        }
        if stop_reason.is_none() {
            stop_reason = response_stop_reason(response);
        }
        top_level_status = merge_top_level_status(top_level_status, &response.status);
        for target in &response.target_results {
            let is_new = seen.insert(target_key(
                target.entity.as_str(),
                target.capability.as_str(),
                target.scenario.as_str(),
            ));
            if !is_new {
                duplicate_target_identity = true;
            }
            covered_capabilities.insert(capability_key(
                target.entity.as_str(),
                target.capability.as_str(),
            ));
            targets.push(TargetResultInput::from_delegated(
                target,
                response.output_dir.as_deref(),
            ));
        }
    }
    if duplicate_target_identity {
        stop_reason = Some("duplicate_target_identity".into());
        top_level_status = Some("error".into());
    }
    for target in &plan.targets {
        if !covered_capabilities.contains(&capability_key(&target.entity, &target.capability)) {
            targets.push(planned_target_input(target));
        }
    }
    let baseline = external_baseline
        .cloned()
        .or_else(|| embedded_baseline(plan))
        .or_else(|| load_snapshot_baseline(plan, base_dir).ok().flatten());
    let inventory = current_inventory.map(|current| match baseline.as_ref() {
        Some(baseline) => compare_inventory(baseline, current),
        None => crate::orchestrator::inventory::unverifiable_comparison(
            plan.inventory
                .snapshot_id
                .as_deref()
                .unwrap_or("missing-baseline"),
            &current.snapshot_id,
            "baseline inventory snapshot is unavailable; drift cannot be determined",
        ),
    });
    if restoration_status.is_none() && !plan.authorization.restore_state {
        restoration_status = Some("not_requested".into());
    }
    if plan.authorization.restore_state {
        if restoration_status.as_deref() != Some("verified") {
            restoration_status = Some("not_verified".into());
        }
    } else {
        restoration_status = Some("not_requested".into());
    }
    aggregate_run(&AggregateInput {
        plan_id: plan.plan_id.clone(),
        run_id: "orchestrator-run".into(),
        evidence_tier,
        inventory,
        targets,
        artifacts,
        restoration_status,
        restore_required: plan.authorization.restore_state,
        stop_reason,
        output_dir,
        top_level_status,
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

fn aggregate_target(
    target: &TargetResultInput,
    restore_required: bool,
    stop_reason: Option<&str>,
) -> AggregateTarget {
    let latency_ms = target
        .readback_latency_ms
        .iter()
        .copied()
        .chain(target.dispatch_latency_ms.iter().copied())
        .max();
    let restore_missing = restore_required
        && matches!(target.outcome.as_str(), "passed" | "failed" | "error")
        && target.restore_status.as_deref() != Some("verified");
    let outcome = if restore_missing {
        "failed".to_string()
    } else {
        target.outcome.clone()
    };
    let reason = if restore_missing {
        target
            .reason
            .clone()
            .or_else(|| Some("required initial-state restoration was not verified".to_string()))
    } else {
        target.reason.clone()
    };
    let quarantine_requested =
        restore_missing || matches!(target.outcome.as_str(), "failed" | "error");
    let lifecycle = lifecycle_for_target(
        &target.entity,
        &target.capability,
        &outcome,
        reason.clone(),
        quarantine_requested,
        stop_reason,
    );
    let quarantined = quarantine_requested || lifecycle.quarantined;
    let restore_status = if !restore_required {
        Some("not_requested".to_string())
    } else if restore_missing {
        Some("not_verified".to_string())
    } else {
        target.restore_status.clone()
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
        initial_state: target.initial_state.clone(),
        final_state: target.final_state.clone(),
        evidence: target.evidence.clone(),
        artifact_paths: target.artifact_paths.clone(),
        reason,
        restore_status,
        quarantined,
        lifecycle,
        action_results: target.action_results.clone(),
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

fn target_key(entity: &str, capability: &str, scenario: &str) -> String {
    format!("{entity}\u{1f}{}\u{1f}{scenario}", capability)
}

fn capability_key(entity: &str, capability: &str) -> String {
    format!("{entity}\u{1f}{capability}")
}

fn embedded_baseline(plan: &OrchestrationPlan) -> Option<InventorySnapshot> {
    if plan.inventory.entities.is_empty() {
        return None;
    }
    Some(InventorySnapshot {
        schema_version: "1".into(),
        snapshot_id: plan
            .inventory
            .snapshot_id
            .clone()
            .or_else(|| plan.inventory.baseline_snapshot_id.clone())
            .unwrap_or_else(|| "plan-inventory".into()),
        source: plan.inventory.source.clone(),
        captured_at: plan.inventory.captured_at.clone().unwrap_or_default(),
        complete: plan.inventory.complete,
        entities: plan.inventory.entities.clone(),
    })
}

fn load_snapshot_baseline(
    plan: &OrchestrationPlan,
    base_dir: &Path,
) -> anyhow::Result<Option<InventorySnapshot>> {
    let Some(snapshot_path) = plan.inventory.snapshot_path.as_deref() else {
        return Ok(None);
    };
    let path = if Path::new(snapshot_path).is_absolute() {
        Path::new(snapshot_path).to_path_buf()
    } else {
        base_dir.join(snapshot_path)
    };
    let content = std::fs::read_to_string(&path)?;
    Ok(Some(serde_yaml::from_str(&content)?))
}

fn overall_status(
    summary: &AggregateSummary,
    run_level_stop: bool,
    top_level_status: Option<&str>,
) -> String {
    if run_level_stop {
        "blocked".into()
    } else if let Some(status) = top_level_status {
        match status {
            "error" => "error".into(),
            "failed" => "failed".into(),
            "blocked" => "blocked".into(),
            _ => overall_status_from_summary(summary),
        }
    } else if summary.errors > 0 {
        overall_status_from_summary(summary)
    } else {
        overall_status_from_summary(summary)
    }
}

fn overall_status_from_summary(summary: &AggregateSummary) -> String {
    if summary.errors > 0 {
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

fn merge_top_level_status(current: Option<String>, status: &str) -> Option<String> {
    let priority = |value: &str| match value {
        "error" => 3,
        "failed" => 2,
        "blocked" => 1,
        _ => 0,
    };
    if priority(status) == 0 {
        return current;
    }
    if current
        .as_deref()
        .map(|existing| priority(status) > priority(existing))
        .unwrap_or(true)
    {
        Some(status.to_string())
    } else {
        current
    }
}

fn aggregate_restoration_status(
    required: bool,
    reported: Option<String>,
    targets: &[AggregateTarget],
) -> Option<String> {
    if !required {
        return Some("not_requested".into());
    }
    let executed = targets
        .iter()
        .filter(|target| matches!(target.outcome.as_str(), "passed" | "failed" | "error"));
    let mut saw_executed = false;
    for target in executed {
        saw_executed = true;
        if target.restore_status.as_deref() != Some("verified") {
            return Some("not_verified".into());
        }
    }
    if saw_executed {
        Some("verified".into())
    } else {
        reported
    }
}

fn response_stop_reason(response: &DelegationResponse) -> Option<String> {
    match response.status.as_str() {
        "blocked" => Some("delegation_blocked".into()),
        "error" if response.target_results.is_empty() => Some("delegation_error".into()),
        "failed" if response.target_results.is_empty() => Some("delegation_failed".into()),
        _ => None,
    }
}

fn lifecycle_for_target(
    entity: &str,
    capability: &str,
    outcome: &str,
    reason: Option<String>,
    quarantine_requested: bool,
    stop_reason: Option<&str>,
) -> TargetLifecycle {
    let mut lifecycle = TargetLifecycle::new(entity, capability);
    match outcome {
        "blocked" => {
            let _ = lifecycle.transition_with_reason(TargetState::Blocked, reason);
        }
        "skipped" => {
            let _ = lifecycle.transition_with_reason(TargetState::Skipped, reason);
        }
        "not_run" => {
            let _ = lifecycle.transition(TargetState::Eligible);
            let reason = stop_reason
                .map(|value| format!("run stopped: {value}"))
                .or(reason)
                .or_else(|| Some("target was not executed".into()));
            let _ = lifecycle.transition_with_reason(TargetState::NotRun, reason);
        }
        "passed" | "failed" | "error" => {
            let _ = lifecycle.transition(TargetState::Eligible);
            let _ = lifecycle.transition(TargetState::Queued);
            let _ = lifecycle.transition(TargetState::Running);
            let state = match outcome {
                "passed" => TargetState::Passed,
                "failed" => TargetState::Failed,
                _ => TargetState::Error,
            };
            let quarantine_reason = reason.clone();
            let _ = lifecycle.transition_with_reason(state, reason);
            if matches!(state, TargetState::Failed | TargetState::Error) || quarantine_requested {
                lifecycle.quarantine(
                    quarantine_reason.unwrap_or_else(|| "target execution failed".into()),
                );
            }
        }
        _ => {
            lifecycle.quarantine("aggregate contained an unsupported target outcome");
        }
    }
    lifecycle
}
