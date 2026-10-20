use std::path::Path;

use lumi_tester::orchestrator::aggregate::aggregate_plan;
use lumi_tester::orchestrator::delegation::{validate_delegation, DelegationResponse};
use lumi_tester::orchestrator::inventory::{compare_inventory, InventoryEntity, InventorySnapshot};
use lumi_tester::orchestrator::lifecycle::{TargetLifecycle, TargetState};
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
        "flowPaths": ["flow.yaml"],
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
    let aggregate = aggregate_plan(
        &restore_plan,
        &[response("passed", "entity-1", "passed")],
        None,
    );

    assert_eq!(aggregate.status, "failed");
    assert_eq!(aggregate.summary.quarantined, 1);
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
