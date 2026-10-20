use std::fs;
use std::path::{Path, PathBuf};

use lumi_tester::orchestrator::aggregate::{aggregate_plan, aggregate_plan_with_baseline};
use lumi_tester::orchestrator::delegation::{
    validate_delegation, validate_delegation_batch, DelegationResponse, RestorationEvidence,
    TemplateSummary,
};
use lumi_tester::orchestrator::inventory::{compare_inventory, InventoryEntity, InventorySnapshot};
use lumi_tester::orchestrator::lifecycle::{LifecycleEvent, TargetLifecycle, TargetState};
use lumi_tester::orchestrator::plan::OrchestrationPlan;

fn snapshot(id: &str, fingerprint: &str) -> InventorySnapshot {
    InventorySnapshot {
        schema_version: "1".to_string(),
        snapshot_id: id.to_string(),
        source: "live_hc_discovery".to_string(),
        captured_at: "2026-10-20T10:00:00Z".to_string(),
        complete: true,
        entities: vec![InventoryEntity {
            entity: "entity-1".to_string(),
            capabilities: vec!["switch".to_string()],
            fingerprint: fingerprint.to_string(),
            mapping_fingerprint: Some("sha256:mapping".to_string()),
        }],
    }
}

#[test]
fn failed_target_is_quarantined_and_cannot_mutate_again() {
    let mut target = TargetLifecycle::new("entity-1", "switch");
    target.transition(TargetState::Eligible).unwrap();
    target.transition(TargetState::Queued).unwrap();
    target.transition(TargetState::Running).unwrap();
    target.transition(TargetState::Failed).unwrap();
    target.quarantine("readback mismatch");

    assert_eq!(target.state(), TargetState::Failed);
    assert!(target.is_quarantined());
    assert!(!target.can_mutate());
    assert!(target.transition(TargetState::Queued).is_err());
}

#[test]
fn shared_stop_marks_only_unstarted_targets_not_run() {
    let mut targets = vec![
        TargetLifecycle::new("entity-1", "switch"),
        TargetLifecycle::new("entity-2", "switch"),
    ];
    targets[0].transition(TargetState::Eligible).unwrap();
    targets[0].transition(TargetState::Queued).unwrap();
    targets[0].transition(TargetState::Running).unwrap();
    targets[0].transition(TargetState::Passed).unwrap();
    targets[1].transition(TargetState::Eligible).unwrap();

    TargetLifecycle::apply_shared_stop(&mut targets, "inventory drift");

    assert_eq!(targets[0].state(), TargetState::Passed);
    assert_eq!(targets[1].state(), TargetState::NotRun);
    assert!(!targets[1].can_mutate());
    assert!(targets[1].validate_history().is_ok());
}

#[test]
fn shared_stop_marks_discovered_target_with_valid_history() {
    let mut targets = vec![TargetLifecycle::new("entity-1", "switch")];

    TargetLifecycle::apply_shared_stop(&mut targets, "health failure");

    assert_eq!(targets[0].state(), TargetState::NotRun);
    assert!(targets[0].validate_history().is_ok());
}

#[test]
fn shared_stop_history_accepts_an_untouched_discovered_target() {
    let mut targets = vec![TargetLifecycle::new("entity-1", "switch")];

    TargetLifecycle::apply_shared_stop(&mut targets, "inventory drift");

    assert_eq!(targets[0].state(), TargetState::NotRun);
    assert!(targets[0].validate_history().is_ok());
}

#[test]
fn inventory_fingerprint_change_is_run_level_drift() {
    let baseline = snapshot("baseline", "sha256:old");
    let current = snapshot("current", "sha256:new");
    let comparison = compare_inventory(&baseline, &current);

    assert!(comparison.drifted);
    assert!(!comparison.unverifiable);
    assert!(comparison
        .changes
        .iter()
        .any(|change| change.kind == "changed"));
}

#[test]
fn missing_mapping_fingerprint_is_unverifiable_not_safe() {
    let baseline = snapshot("baseline", "sha256:old");
    let mut current = snapshot("current", "sha256:old");
    current.entities[0].mapping_fingerprint = None;
    let comparison = compare_inventory(&baseline, &current);

    assert!(comparison.drifted);
    assert!(!comparison.mapping_verifiable);
}

fn plan() -> OrchestrationPlan {
    serde_yaml::from_str(
        r#"
schemaVersion: "1"
planId: plan-001
request: switch
scope: hc_only
target:
  platform: lumi_hc
  profile: profile.yaml
  endpoint: http://hc
  mode: real
inventory:
  source: live_hc_discovery
  snapshotId: baseline
  complete: true
  entities: []
authorization:
  real_device_control: true
  restore_state: false
execution:
  repeat: 1
  max_repeat: 1
  mode: sequential
  max_parallel: 1
  delay_ms: 0
  timeout_ms: 1000
  final_state_capture: true
failurePolicy:
  scope: target
  onTargetFailure: record_and_continue_independent_targets
  aggregateAtEnd: true
  stopOn: [health_failure, discovery_failure, inventory_drift, unresolved_system_safety]
targets:
  - entity: entity-1
    capability: switch
    status: eligible
    mapping: {input_idx: 1, state_idx: 1}
    cycles: [{power: true}]
  - entity: entity-2
    capability: switch
    status: eligible
    mapping: {input_idx: 1, state_idx: 1}
    cycles: [{power: false}]
evidence:
  report: true
  events: true
  snapshots: true
  label: real_e2e
"#,
    )
    .unwrap()
}

fn response(status: &str, entity: &str, target_status: &str) -> DelegationResponse {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": "1",
        "planId": "plan-001",
        "phase": "run",
        "status": status,
        "runtimeStatus": "completed",
        "validation": {"valid": true, "exitCode": 0},
        "listedCommands": [{"index": 0, "name": "hcHealth"}],
        "flowPaths": [],
        "outputDir": "output/run-001",
        "template": {"status": "reused", "path": null},
        "evidenceTier": "real_e2e",
        "artifactPaths": ["output/run-001/run.json"],
        "unresolved": [],
        "restoration": {"required": false, "verified": false, "status": "not_requested"},
        "targetResults": [{
            "entity": entity,
            "capability": "switch",
            "scenario": "control_readback",
            "status": target_status,
            "repetitions": {"requested": 1, "completed": 1, "passed": if target_status == "passed" {1} else {0}, "failed": if target_status == "failed" {1} else {0}},
            "artifactPaths": ["output/run-001/run.json"],
            "evidenceTier": "real_e2e"
        }]
    }))
    .unwrap()
}

fn temporary_contract_dir(label: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("lumi-orchestrator-{label}-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn delegation_rejects_passed_response_when_run_manifest_is_missing() {
    let root = temporary_contract_dir("missing-manifest");
    let output = root.join("output").join("run-001");
    let mut response = response("passed", "entity-1", "passed");
    response.output_dir = Some(PathBuf::from("output/run-001"));
    response.artifact_paths = vec![PathBuf::from("output/run-001/run.json")];
    let mut second = response.target_results[0].clone();
    second.entity = "entity-2".to_string();
    response.target_results.push(second);

    let report = validate_delegation(&plan(), &response, &root);

    assert!(
        !report.valid,
        "missing evidence must not validate: {report:#?}"
    );
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "artifact_missing"));
    assert!(!output.join("run.json").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn delegation_resolves_relative_artifacts_from_response_base_dir() {
    let root = temporary_contract_dir("relative-artifacts");
    let output = root.join("output").join("run-001");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("run.json"), "{}\n").unwrap();
    let mut response = response("failed", "entity-1", "failed");
    response.runtime_status = "failed".into();
    response.output_dir = Some(PathBuf::from("output/run-001"));
    response.artifact_paths = vec![PathBuf::from("output/run-001/run.json")];

    let report = validate_delegation(&plan(), &response, &root);

    assert!(
        report.valid,
        "relative evidence should resolve from base: {report:#?}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn delegation_rejects_target_artifact_outside_output_dir() {
    let root = temporary_contract_dir("target-artifact-escape");
    let output = root.join("output").join("run-001");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("run.json"), "{}\n").unwrap();
    let outside = root.join("secret.json");
    fs::write(&outside, "secret\n").unwrap();
    let mut response = response("passed", "entity-1", "passed");
    response.output_dir = Some(PathBuf::from("output/run-001"));
    response.artifact_paths = vec![PathBuf::from("output/run-001/run.json")];
    response.target_results[0].artifact_paths = vec![PathBuf::from("secret.json")];

    let report = validate_delegation(&plan(), &response, &root);

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "target_artifact_outside_output_dir"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn delegation_requires_restore_evidence_even_when_run_failed() {
    let restore_plan: OrchestrationPlan = serde_yaml::from_str(&plan_yaml_with_restore()).unwrap();
    let mut response = response("failed", "entity-1", "failed");
    response.restoration = None;

    let report = validate_delegation(&restore_plan, &response, Path::new("."));

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "restore_not_verified"));
}

#[test]
fn aggregate_emits_lifecycle_and_preserves_initial_state_evidence() {
    let mut delegated = response("passed", "entity-1", "passed");
    delegated.target_results[0].initial_state = Some(serde_json::json!({"power": false}));
    let mut lifecycle = TargetLifecycle::new("entity-1", "switch");
    lifecycle.transition(TargetState::Eligible).unwrap();
    lifecycle.transition(TargetState::Queued).unwrap();
    lifecycle.transition(TargetState::Running).unwrap();
    lifecycle.transition(TargetState::Passed).unwrap();
    delegated.target_results[0].lifecycle = Some(lifecycle);
    let aggregate = aggregate_plan(&plan(), &[delegated], None);
    let value = serde_json::to_value(&aggregate).unwrap();

    assert!(value.get("lifecycle").is_some_and(|value| value.is_array()));
    assert_eq!(
        value["targets"][0]["lifecycle"]["events"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(value["targets"][0]["initialState"]["power"], false);
    assert_eq!(
        value["targets"][0]["evidence"]["outputDir"],
        "output/run-001"
    );
}

#[test]
fn aggregate_reports_not_requested_restore_when_response_omits_restore_object() {
    let mut delegated = response("failed", "entity-1", "failed");
    delegated.restoration = None;
    let aggregate = aggregate_plan(&plan(), &[delegated], None);

    assert_eq!(
        aggregate.restoration_status.as_deref(),
        Some("not_requested")
    );
}

#[test]
fn aggregate_can_compare_current_inventory_to_an_external_baseline() {
    let baseline = snapshot("baseline", "sha256:stable");
    let current = snapshot("current", "sha256:stable");
    let aggregate = aggregate_plan_with_baseline(
        &plan(),
        &[response("failed", "entity-1", "failed")],
        Some(&baseline),
        Some(&current),
    );

    assert!(!aggregate.inventory.as_ref().unwrap().drifted);
}

#[test]
fn aggregate_marks_missing_baseline_as_unverifiable() {
    let aggregate = aggregate_plan(
        &plan(),
        &[response("failed", "entity-1", "failed")],
        Some(&snapshot("current", "sha256:stable")),
    );

    let inventory = aggregate.inventory.as_ref().unwrap();
    assert!(inventory.unverifiable);
    assert!(inventory
        .changes
        .iter()
        .any(|change| change.kind == "unverifiable"));
}

#[test]
fn delegation_rejects_paths_that_escape_or_are_missing_from_response_base() {
    let root = temporary_contract_dir("path-boundaries");
    let output = root.join("output").join("run-001");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("run.json"), "{}\n").unwrap();
    let outside = root
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("lumi-orchestrator-outside.json");
    fs::write(&outside, "outside\n").unwrap();

    let mut delegated = response("passed", "entity-1", "passed");
    delegated.output_dir = Some(outside.with_extension("dir"));
    delegated.artifact_paths = vec![outside.clone()];
    delegated.generated_flow_paths = vec![outside.clone()];
    delegated.flow_paths = vec![outside.clone()];
    delegated.template = Some(TemplateSummary {
        status: "reused".to_string(),
        path: Some(PathBuf::from("output/run-001/missing-template.yaml")),
    });
    delegated.restoration = Some(RestorationEvidence {
        required: false,
        verified: false,
        initial_state_path: Some(PathBuf::from("output/run-001/missing-initial.json")),
        restore_flow_path: Some(PathBuf::from("output/run-001/missing-restore.yaml")),
        final_state_path: Some(PathBuf::from("output/run-001/missing-final.json")),
        status: Some("not_requested".to_string()),
    });

    let report = validate_delegation(&plan(), &delegated, &root);

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "output_dir_outside_base_dir"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "artifact_outside_output_dir"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "flow_outside_output_dir"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "template_missing"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "restoration_path_missing"));
    let _ = fs::remove_file(outside);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn delegation_rejects_inconsistent_run_repetition_invariants() {
    let mut delegated = response("passed", "entity-1", "passed");
    delegated.target_results[0].repetitions.requested = 2;
    delegated.target_results[0].repetitions.completed = 1;
    delegated.target_results[0].repetitions.passed = 0;
    delegated.target_results[0].repetitions.failed = 0;

    let report = validate_delegation(&plan(), &delegated, Path::new("."));

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "repetitions_outcome_mismatch"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "repetitions_requested_mismatch"));

    delegated.target_results[0].repetitions.requested = 1;
    delegated.target_results[0].repetitions.completed = 0;
    delegated.target_results[0].repetitions.passed = 0;
    delegated.target_results[0].repetitions.failed = 0;
    let zero_completed = validate_delegation(&plan(), &delegated, Path::new("."));
    assert!(!zero_completed.valid);
    assert!(zero_completed
        .errors
        .iter()
        .any(|issue| issue.code == "passed_target_without_completed_repetitions"));
}

#[test]
fn aggregate_derives_lifecycle_when_delegated_history_is_empty_or_invalid() {
    let mut empty_history = response("passed", "entity-1", "passed");
    empty_history.target_results[0].lifecycle = Some(TargetLifecycle {
        entity: "entity-1".to_string(),
        capability: "switch".to_string(),
        state: TargetState::Passed,
        quarantined: false,
        quarantine_reason: None,
        events: Vec::new(),
    });
    let aggregate = aggregate_plan(&plan(), &[empty_history], None);
    assert!(aggregate.targets[0].lifecycle.events.len() >= 4);

    let mut invalid_history = response("passed", "entity-1", "passed");
    invalid_history.target_results[0].lifecycle = Some(TargetLifecycle {
        entity: "entity-1".to_string(),
        capability: "switch".to_string(),
        state: TargetState::Passed,
        quarantined: false,
        quarantine_reason: None,
        events: vec![LifecycleEvent {
            from: TargetState::Discovered,
            to: TargetState::Passed,
            reason: None,
        }],
    });
    let aggregate = aggregate_plan(&plan(), &[invalid_history], None);
    assert!(aggregate.targets[0].lifecycle.events.len() >= 4);
    assert_eq!(
        aggregate.targets[0].lifecycle.events[0].to,
        TargetState::Eligible
    );
}

#[test]
fn aggregate_uses_canonical_lifecycle_even_for_valid_agent_history() {
    let mut delegated = response("passed", "entity-1", "passed");
    let mut lifecycle = TargetLifecycle::new("entity-1", "switch");
    lifecycle
        .transition_with_reason(TargetState::Eligible, Some("agent supplied".into()))
        .unwrap();
    lifecycle
        .transition_with_reason(TargetState::Queued, Some("agent supplied".into()))
        .unwrap();
    lifecycle
        .transition_with_reason(TargetState::Running, Some("agent supplied".into()))
        .unwrap();
    lifecycle
        .transition_with_reason(TargetState::Passed, Some("agent supplied".into()))
        .unwrap();
    delegated.target_results[0].lifecycle = Some(lifecycle);

    let aggregate = aggregate_plan(&plan(), &[delegated], None);

    assert!(aggregate.targets[0]
        .lifecycle
        .events
        .iter()
        .all(|event| event.reason.is_none()));
}

#[test]
fn aggregate_honors_top_level_failed_status_even_when_target_passed() {
    let mut delegated = response("failed", "entity-1", "passed");
    delegated.runtime_status = "failed".to_string();

    let aggregate = aggregate_plan(&plan(), &[delegated], None);

    assert_eq!(aggregate.status, "failed");
}

#[test]
fn delegation_rejects_failed_response_with_only_passed_target_results() {
    let mut delegated = response("failed", "entity-1", "passed");
    delegated.runtime_status = "failed".to_string();

    let report = validate_delegation(&plan(), &delegated, Path::new("."));

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "response_status_inconsistent"));
}

#[test]
fn delegation_rejects_invalid_optional_lifecycle_history() {
    let mut delegated = response("passed", "entity-1", "passed");
    delegated.target_results[0].lifecycle = Some(TargetLifecycle {
        entity: "entity-1".to_string(),
        capability: "switch".to_string(),
        state: TargetState::Passed,
        quarantined: false,
        quarantine_reason: None,
        events: Vec::new(),
    });

    let report = validate_delegation(&plan(), &delegated, Path::new("."));

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "lifecycle_invalid"));
}

#[test]
fn delegation_rejects_invalid_runtime_status_and_repetition_counts() {
    let mut delegated = response("passed", "entity-1", "passed");
    delegated.runtime_status = "running".to_string();
    delegated.target_results[0].repetitions.completed = 2;
    delegated.target_results[0].repetitions.passed = 2;
    delegated.target_results[0].repetitions.failed = 1;

    let report = validate_delegation(&plan(), &delegated, Path::new("."));

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "runtime_status_invalid"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "repetitions_completed_exceed_requested"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "repetitions_outcome_exceed_completed"));
}

#[test]
fn passed_delegation_requires_every_eligible_target_and_no_failed_target() {
    let mut delegated = response("passed", "entity-1", "passed");
    let mut missing = delegated.target_results[0].clone();
    missing.entity = "entity-2".to_string();
    missing.status = "not_run".to_string();
    delegated.target_results.push(missing);

    let report = validate_delegation(&plan(), &delegated, Path::new("."));

    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "passed_target_not_allowed"));
}

#[test]
fn aggregate_preserves_same_entity_across_capability_and_scenario() {
    let mut delegated = response("passed", "entity-1", "passed");
    let mut second = delegated.target_results[0].clone();
    second.capability = "rgb".to_string();
    second.scenario = "rgb_control".to_string();
    delegated.target_results.push(second);

    let aggregate = aggregate_plan(&plan(), &[delegated], None);
    let entity_targets = aggregate
        .targets
        .iter()
        .filter(|target| target.entity == "entity-1")
        .collect::<Vec<_>>();

    assert_eq!(entity_targets.len(), 2);
    assert!(entity_targets
        .iter()
        .any(|target| target.capability == "switch"));
    assert!(entity_targets
        .iter()
        .any(|target| target.capability == "rgb"));
}

#[test]
fn aggregate_rejects_duplicate_target_identity_across_responses() {
    let first = response("passed", "entity-1", "passed");
    let duplicate = response("passed", "entity-1", "passed");

    let aggregate = aggregate_plan(&plan(), &[first, duplicate], None);

    assert_eq!(aggregate.status, "error");
    assert_eq!(
        aggregate.stop_reason.as_deref(),
        Some("duplicate_target_identity")
    );
}

#[test]
fn delegation_batch_rejects_duplicate_target_identity_before_aggregate() {
    let first = response("passed", "entity-1", "passed");
    let duplicate = response("passed", "entity-1", "passed");

    let reports = validate_delegation_batch(&plan(), &[first, duplicate], Path::new("."));

    assert!(reports.iter().any(|report| {
        report
            .errors
            .iter()
            .any(|issue| issue.code == "duplicate_target_identity_across_responses")
    }));
}

#[test]
fn aggregate_preserves_initial_state_latency_and_target_restore_evidence() {
    let mut raw = serde_json::to_value(response("passed", "entity-1", "passed")).unwrap();
    raw["targetResults"][0]["initialState"] = serde_json::json!({"power": false});
    raw["targetResults"][0]["dispatchLatencyMs"] = serde_json::json!([12]);
    raw["targetResults"][0]["readbackLatencyMs"] = serde_json::json!([34]);
    raw["targetResults"][0]["restoration"] = serde_json::json!({
        "required": false,
        "verified": false,
        "status": "not_requested"
    });
    let delegated: DelegationResponse = serde_json::from_value(raw).unwrap();

    let aggregate = aggregate_plan(&plan(), &[delegated], None);
    let value = serde_json::to_value(&aggregate).unwrap();

    assert_eq!(value["targets"][0]["initialState"]["power"], false);
    assert_eq!(value["targets"][0]["dispatchLatencyMs"][0], 12);
    assert_eq!(value["targets"][0]["readbackLatencyMs"][0], 34);
    assert_eq!(value["targets"][0]["restoreStatus"], "not_requested");
}

#[test]
fn restoration_is_required_for_erroring_mutations_and_disabled_restore_is_explicit() {
    let restore_plan: OrchestrationPlan = serde_yaml::from_str(&plan_yaml_with_restore()).unwrap();
    let mut error_response = response("error", "entity-1", "error");
    error_response.restoration = None;
    let report = validate_delegation(&restore_plan, &error_response, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "restore_not_verified"));

    let mut no_restore_response = response("failed", "entity-1", "failed");
    no_restore_response.restoration = None;
    let no_restore_report = validate_delegation(&plan(), &no_restore_response, Path::new("."));
    assert!(!no_restore_report.valid);
    assert!(no_restore_report
        .errors
        .iter()
        .any(|issue| issue.code == "restore_status_invalid"));
}

#[test]
fn delegation_response_requires_matching_plan_and_artifacts() {
    let mut response = response("passed", "entity-1", "passed");
    response.plan_id = "other-plan".to_string();
    let report = validate_delegation(&plan(), &response, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "plan_id_mismatch"));
}

#[test]
fn delegation_response_rejects_unknown_evidence_tier_and_target() {
    let mut response = response("passed", "unknown-entity", "passed");
    response.evidence_tier = "invented_tier".to_string();
    let report = validate_delegation(&plan(), &response, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "invalid_evidence_tier"));
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "unknown_target"));
}

#[test]
fn restoration_disabled_requires_not_requested_status() {
    let mut response = response("passed", "entity-1", "passed");
    response.restoration.as_mut().unwrap().status = Some("verified".to_string());
    let report = validate_delegation(&plan(), &response, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "restore_status_invalid"));
}

#[test]
fn aggregate_preserves_target_failure_and_quarantine() {
    let responses = vec![
        response("passed", "entity-1", "passed"),
        response("failed", "entity-2", "failed"),
    ];
    let aggregate = aggregate_plan(&plan(), &responses, None);
    assert_eq!(aggregate.summary.passed, 1);
    assert_eq!(aggregate.summary.failed, 1);
    assert_eq!(aggregate.summary.quarantined, 1);
    assert_eq!(aggregate.targets.len(), 2);
}

#[test]
fn aggregate_keeps_target_local_error_as_error_not_run_level_block() {
    let mut target_error = response("error", "entity-1", "error");
    target_error.runtime_status = "error".to_string();
    target_error.target_results[0].repetitions.failed = 1;

    let aggregate = aggregate_plan(&plan(), &[target_error], None);

    assert_eq!(aggregate.status, "error");
    assert_eq!(aggregate.stop_reason, None);
    assert_eq!(aggregate.targets[0].outcome, "error");
    assert!(aggregate.targets[0].quarantined);
    assert_eq!(aggregate.targets[1].outcome, "not_run");
}

#[test]
fn aggregate_stops_only_for_response_level_error_without_targets() {
    let mut response_level_error = response("error", "entity-1", "error");
    response_level_error.runtime_status = "error".to_string();
    response_level_error.target_results.clear();

    let aggregate = aggregate_plan(&plan(), &[response_level_error], None);

    assert_eq!(aggregate.stop_reason.as_deref(), Some("delegation_error"));
    assert_eq!(aggregate.status, "blocked");
    assert!(aggregate
        .targets
        .iter()
        .filter(|target| target.entity == "entity-2")
        .all(|target| target.outcome == "not_run"));
    assert!(aggregate.targets.iter().any(|target| {
        target.entity == "entity-2"
            && target
                .lifecycle
                .events
                .last()
                .and_then(|event| event.reason.as_deref())
                .is_some_and(|reason| reason.contains("delegation_error"))
    }));
}

#[test]
fn aggregate_keeps_planned_blocked_target_and_stops_on_inventory_drift() {
    let mut blocked_plan = plan();
    blocked_plan.targets[1].status = "blocked".to_string();
    blocked_plan.targets[1].reason = Some("mapping is unresolved".to_string());
    let blocked = aggregate_plan(
        &blocked_plan,
        &[response("passed", "entity-1", "passed")],
        None,
    );
    assert_eq!(blocked.summary.blocked, 1);
    assert!(blocked
        .targets
        .iter()
        .any(|target| target.entity == "entity-2" && target.outcome == "blocked"));

    let drifted = aggregate_plan(
        &plan(),
        &[response("passed", "entity-1", "passed")],
        Some(&snapshot("current", "sha256:current")),
    );
    assert_eq!(drifted.status, "blocked");
    assert!(drifted
        .inventory
        .as_ref()
        .is_some_and(|inventory| inventory.drifted));
    assert!(drifted
        .targets
        .iter()
        .any(|target| target.entity == "entity-2" && target.outcome == "not_run"));

    let drifted_failure = aggregate_plan(
        &plan(),
        &[response("failed", "entity-1", "failed")],
        Some(&snapshot("current", "sha256:current")),
    );
    assert_eq!(drifted_failure.status, "blocked");
}

#[test]
fn aggregate_stops_on_inventory_drift_and_keeps_unreached_target_not_run() {
    let responses = vec![response("passed", "entity-1", "passed")];
    let current = snapshot("current", "sha256:changed");
    let aggregate = aggregate_plan(&plan(), &responses, Some(&current));

    assert_eq!(aggregate.status, "blocked");
    assert_eq!(aggregate.summary.not_run, 1);
    assert_eq!(aggregate.inventory.as_ref().unwrap().drifted, true);
}

#[test]
fn aggregate_does_not_claim_restore_when_restore_is_required() {
    let restore_plan = plan_yaml_with_restore();
    let restore_plan: OrchestrationPlan = serde_yaml::from_str(&restore_plan).unwrap();
    let mut delegated = response("passed", "entity-1", "passed");
    delegated.restoration = Some(RestorationEvidence {
        required: true,
        verified: true,
        initial_state_path: None,
        restore_flow_path: None,
        final_state_path: None,
        status: Some("verified".into()),
    });
    let aggregate = aggregate_plan(&restore_plan, &[delegated], None);

    assert_eq!(aggregate.status, "failed");
    assert_eq!(aggregate.summary.quarantined, 1);
    assert_eq!(
        aggregate.restoration_status.as_deref(),
        Some("not_verified")
    );
}

fn plan_yaml_with_restore() -> String {
    r#"
schemaVersion: "1"
planId: plan-001
request: switch
scope: hc_only
target: {platform: lumi_hc, profile: profile.yaml, endpoint: http://hc, mode: real}
inventory: {source: live_hc_discovery, snapshotId: baseline, complete: true, entities: []}
authorization: {real_device_control: true, restore_state: true}
restoration: {mode: captured_state, verify: true, final_state_capture: true}
execution: {repeat: 1, max_repeat: 1, mode: sequential, max_parallel: 1, timeout_ms: 1000, final_state_capture: true}
failurePolicy: {scope: target, onTargetFailure: record_and_continue_independent_targets, aggregateAtEnd: true, stopOn: [health_failure, discovery_failure, inventory_drift, unresolved_system_safety]}
targets:
  - {entity: entity-1, capability: switch, status: eligible, mapping: {source: protocol_profile}, cycles: [{power: true}]}
evidence: {report: true, events: true, snapshots: true, label: real_e2e}
"#
    .to_string()
}
