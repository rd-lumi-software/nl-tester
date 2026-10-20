use std::path::Path;

use lumi_tester::orchestrator::plan::validate_plan_yaml;

fn valid_plan() -> &'static str {
    r#"
schemaVersion: "1"
planId: "plan-001"
request: "exercise one switch"
scope: hc_only
target:
  platform: lumi_hc
  profile: profiles/lumi-iot-v3.yaml
  endpoint: "${LUMI_HC_URL}"
  mode: real
inventory:
  source: live_hc_discovery
  snapshotId: baseline-001
  capturedAt: "2026-10-20T10:00:00Z"
  complete: true
  expectedCount: 1
  entities:
    - entity: "entity-1"
      capabilities: [switch]
      fingerprint: sha256:entity
      mappingFingerprint: sha256:mapping
  allowlist: ["entity-1"]
authorization:
  real_device_control: true
  restore_state: false
execution:
  repeat: 1
  max_repeat: 2
  mode: sequential
  max_parallel: 1
  delay_ms: 0
  timeout_ms: 10000
  final_state_capture: true
failurePolicy:
  scope: target
  onTargetFailure: record_and_continue_independent_targets
  aggregateAtEnd: true
  stopOn: [health_failure, discovery_failure, inventory_drift, unresolved_system_safety]
targets:
  - entity: "entity-1"
    capability: switch
    status: eligible
    mapping:
      input_idx: 1
      state_idx: 1
      source: protocol_profile
    cycles:
      - { power: true }
acceptance: [dispatch, readback, final_state]
evidence:
  report: true
  events: true
  snapshots: true
  label: real_e2e
"#
}

#[test]
fn valid_real_hc_plan_is_ready_for_run() {
    let report = validate_plan_yaml(valid_plan(), true, Path::new("."));
    assert!(report.valid, "{report:#?}");
    assert!(report.ready_for_run, "{report:#?}");
    assert!(report.errors.is_empty(), "{report:#?}");
}

#[test]
fn repeat_above_limit_is_rejected_with_machine_path() {
    let yaml = valid_plan().replace("repeat: 1", "repeat: 3");
    let report = validate_plan_yaml(&yaml, true, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "repeat_exceeds_max" && issue.path == "/execution/repeat"));
}

#[test]
fn blocked_target_requires_reason() {
    let yaml = valid_plan().replace("status: eligible", "status: blocked");
    let report = validate_plan_yaml(&yaml, false, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "target_reason_required" && issue.path == "/targets/0/reason"));
}

#[test]
fn restore_enabled_requires_explicit_capture_and_verification_contract() {
    let yaml = valid_plan().replace("restore_state: false", "restore_state: true");
    let report = validate_plan_yaml(&yaml, true, Path::new("."));
    assert!(!report.valid);
    assert!(report.errors.iter().any(|issue| {
        issue.code == "restore_contract_required" && issue.path == "/restoration"
    }));
}

#[test]
fn numeric_entity_id_is_rejected_before_execution() {
    let yaml = valid_plan().replace("entity: \"entity-1\"", "entity: 1");
    let report = validate_plan_yaml(&yaml, false, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "entity_id_must_be_string"));
}

#[test]
fn planning_only_plan_can_be_structurally_valid_but_not_run_ready() {
    let yaml = valid_plan().replace("  entities:\n    - entity: \"entity-1\"\n      capabilities: [switch]\n      fingerprint: sha256:entity\n      mappingFingerprint: sha256:mapping\n", "");
    let report = validate_plan_yaml(&yaml, false, Path::new("."));
    assert!(report.valid, "{report:#?}");
    assert!(!report.ready_for_run, "{report:#?}");
}

#[test]
fn eligible_mapping_requires_a_source() {
    let yaml = valid_plan().replace("      source: protocol_profile\n", "");
    let report = validate_plan_yaml(&yaml, false, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "mapping_source_required"));
}

#[test]
fn parallel_execution_requires_explicit_batches() {
    let yaml = valid_plan().replace("mode: sequential", "mode: parallel");
    let report = validate_plan_yaml(&yaml, false, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "parallel_batches_required"));
}

#[test]
fn entity_ids_must_be_unique_even_when_capabilities_differ() {
    let yaml = valid_plan().replace(
        "      - { power: true }\n",
        "      - { power: true }\n  - entity: \"entity-1\"\n    capability: dimmer\n    status: eligible\n    mapping: {input_idx: 1, state_idx: 1, source: protocol_profile}\n    cycles: [{power: true}]\n",
    );
    let report = validate_plan_yaml(&yaml, false, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "duplicate_target"));
}

#[test]
fn empty_real_endpoint_is_not_run_ready() {
    let yaml = valid_plan().replace("endpoint: \"${LUMI_HC_URL}\"", "endpoint: \"\"");
    let report = validate_plan_yaml(&yaml, true, Path::new("."));
    assert!(!report.valid);
    assert!(report
        .errors
        .iter()
        .any(|issue| issue.code == "connection_required"));
}
