use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lumi-tester"))
}

#[test]
fn plan_schema_prints_json() {
    let output = Command::new(binary())
        .args(["plan", "schema", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["title"], "Lumi Orchestrator Test Plan");
}

#[test]
fn plan_validate_prints_machine_readable_report() {
    let root = std::env::temp_dir().join(format!("lumi-plan-cli-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let plan = root.join("plan.yaml");
    fs::write(&plan, include_str!("fixtures/orchestrator/valid-plan.yaml")).unwrap();
    let output = Command::new(binary())
        .args(["plan", "validate", plan.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["readyForRun"], true);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn delegation_validation_and_aggregate_are_offline_cli_operations() {
    let root = std::env::temp_dir().join(format!("lumi-plan-cli-contract-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let plan = root.join("plan.yaml");
    let response = root.join("response.json");
    let inventory = root.join("inventory.json");
    let output_dir = root.join("aggregate");
    fs::write(&plan, include_str!("fixtures/orchestrator/valid-plan.yaml")).unwrap();
    fs::write(
        root.join("flow.yaml"),
        "platform: lumi_hc\n---\n- hcHealth\n",
    )
    .unwrap();
    fs::write(
        &response,
        r#"{
          "schemaVersion": "1",
          "planId": "plan-001",
          "phase": "run",
          "status": "passed",
          "runtimeStatus": "completed",
          "validation": {"valid": true, "exitCode": 0},
          "listedCommands": [{"index": 0, "name": "hcHealth"}],
          "flowPaths": ["output/run-001/flow.yaml"],
          "outputDir": "output/run-001",
          "template": {"status": "reused", "path": null},
          "evidenceTier": "real_e2e",
          "artifactPaths": ["output/run-001/run.json"],
          "unresolved": [],
          "restoration": {"required": false, "verified": false, "status": "not_requested"},
          "targetResults": [{
            "entity": "entity-1",
            "capability": "switch",
            "scenario": "control_readback",
            "status": "passed",
            "repetitions": {"requested": 1, "completed": 1, "passed": 1, "failed": 0},
            "artifactPaths": ["output/run-001/run.json"],
            "evidenceTier": "real_e2e"
          }]
        }"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("output/run-001")).unwrap();
    fs::write(root.join("output/run-001/run.json"), "{}\n").unwrap();
    fs::write(root.join("output/run-001/flow.yaml"), "- hcHealth\n").unwrap();
    fs::write(
        &inventory,
        r#"{
          "schemaVersion": "1",
          "snapshotId": "current-001",
          "source": "live_hc_discovery",
          "capturedAt": "2026-10-20T10:05:00Z",
          "complete": true,
          "entities": [{
            "entity": "entity-1",
            "capabilities": ["switch"],
            "fingerprint": "sha256:entity",
            "mappingFingerprint": "sha256:mapping"
          }]
        }"#,
    )
    .unwrap();

    let validated = Command::new(binary())
        .args([
            "plan",
            "validate-delegation",
            plan.to_str().unwrap(),
            response.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        validated.status.success(),
        "{}",
        String::from_utf8_lossy(&validated.stderr)
    );
    let validation_json: serde_json::Value = serde_json::from_slice(&validated.stdout).unwrap();
    assert_eq!(validation_json["valid"], true);

    let aggregated = Command::new(binary())
        .args([
            "plan",
            "aggregate",
            plan.to_str().unwrap(),
            "--response",
            response.to_str().unwrap(),
            "--inventory-current",
            inventory.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        aggregated.status.success(),
        "{}",
        String::from_utf8_lossy(&aggregated.stderr)
    );
    assert!(
        aggregated.stderr.is_empty(),
        "aggregate emitted unexpected stderr: {}",
        String::from_utf8_lossy(&aggregated.stderr)
    );
    let aggregate_json: serde_json::Value = serde_json::from_slice(&aggregated.stdout).unwrap();
    assert_eq!(aggregate_json["status"], "passed");
    assert!(output_dir.join("aggregate.json").is_file());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn aggregate_rejects_invalid_delegation_without_writing_success() {
    let root =
        std::env::temp_dir().join(format!("lumi-plan-cli-invalid-new-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let plan = root.join("plan.yaml");
    let response = root.join("response.json");
    let inventory = root.join("inventory.json");
    let output_dir = root.join("aggregate");
    fs::write(&plan, include_str!("fixtures/orchestrator/valid-plan.yaml")).unwrap();
    fs::write(
        &response,
        r#"{
          "schemaVersion": "1",
          "planId": "plan-001",
          "phase": "run",
          "status": "passed",
          "runtimeStatus": "completed",
          "validation": {"valid": true, "exitCode": 0},
          "listedCommands": [{"index": 0, "name": "hcHealth"}],
          "outputDir": "output/run-001",
          "evidenceTier": "real_e2e",
          "artifactPaths": ["output/run-001/missing-run.json"],
          "restoration": {"required": false, "verified": false, "status": "not_requested"},
          "targetResults": [{
            "entity": "entity-1",
            "capability": "switch",
            "scenario": "control_readback",
            "status": "passed",
            "repetitions": {"requested": 1, "completed": 1, "passed": 1, "failed": 0}
          }]
        }"#,
    )
    .unwrap();
    fs::write(
        &inventory,
        r#"{
          "schemaVersion": "1",
          "snapshotId": "current-001",
          "source": "live_hc_discovery",
          "capturedAt": "2026-10-20T10:05:00Z",
          "complete": true,
          "entities": [{
            "entity": "entity-1",
            "capabilities": ["switch"],
            "fingerprint": "sha256:entity",
            "mappingFingerprint": "sha256:mapping"
          }]
        }"#,
    )
    .unwrap();

    let result = Command::new(binary())
        .args([
            "plan",
            "aggregate",
            plan.to_str().unwrap(),
            "--response",
            response.to_str().unwrap(),
            "--inventory-current",
            inventory.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["valid"], false);
    assert!(value["errors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty()));
    assert!(!output_dir.join("aggregate.json").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn aggregate_loads_plan_snapshot_relative_to_plan_directory() {
    let root = std::env::temp_dir().join(format!("lumi-plan-cli-baseline-{}", std::process::id()));
    let run_dir = root.join("output").join("run-001");
    fs::create_dir_all(&run_dir).unwrap();
    fs::write(run_dir.join("run.json"), "{}\n").unwrap();

    let plan = root.join("plan.yaml");
    let response = root.join("response.json");
    let baseline = root.join("baseline.json");
    let inventory = root.join("inventory.json");
    let output_dir = root.join("aggregate");
    let plan_content = include_str!("fixtures/orchestrator/valid-plan.yaml")
        .replace("\r\n", "\n")
        .replace(
        "  entities:\n    - entity: \"entity-1\"\n      capabilities: [switch]\n      fingerprint: \"sha256:entity\"\n      mappingFingerprint: \"sha256:mapping\"\n",
        "  snapshotPath: baseline.json\n",
    );
    fs::write(&plan, plan_content).unwrap();
    fs::write(
        &baseline,
        r#"{
          "schemaVersion": "1",
          "snapshotId": "baseline-001",
          "source": "live_hc_discovery",
          "capturedAt": "2026-10-20T10:00:00Z",
          "complete": true,
          "entities": [{
            "entity": "entity-1",
            "capabilities": ["switch"],
            "fingerprint": "sha256:entity",
            "mappingFingerprint": "sha256:mapping"
          }]
        }"#,
    )
    .unwrap();
    fs::write(
        &response,
        r#"{
          "schemaVersion": "1",
          "planId": "plan-001",
          "phase": "run",
          "status": "failed",
          "runtimeStatus": "failed",
          "validation": {"valid": true, "exitCode": 0},
          "listedCommands": [{"index": 0, "name": "hcHealth"}],
          "outputDir": "output/run-001",
          "evidenceTier": "real_e2e",
          "artifactPaths": ["output/run-001/run.json"],
          "restoration": {"required": false, "verified": false, "status": "not_requested"},
          "targetResults": [{
            "entity": "entity-1",
            "capability": "switch",
            "scenario": "control_readback",
            "status": "failed",
            "repetitions": {"requested": 1, "completed": 1, "passed": 0, "failed": 1}
          }]
        }"#,
    )
    .unwrap();
    fs::write(
        &inventory,
        fs::read_to_string(&baseline)
            .unwrap()
            .replace("baseline-001", "current-001"),
    )
    .unwrap();

    let result = Command::new(binary())
        .args([
            "plan",
            "aggregate",
            plan.to_str().unwrap(),
            "--response",
            response.to_str().unwrap(),
            "--inventory-current",
            inventory.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["inventory"]["drifted"], false);
    assert!(output_dir.join("aggregate.json").is_file());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn aggregate_rejects_invalid_delegation_response_before_writing_report() {
    let root = std::env::temp_dir().join(format!("lumi-plan-cli-invalid-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let plan = root.join("plan.yaml");
    let response = root.join("response.json");
    let inventory = root.join("inventory.json");
    let output_dir = root.join("aggregate");
    fs::write(&plan, include_str!("fixtures/orchestrator/valid-plan.yaml")).unwrap();
    fs::write(
        &response,
        r#"{
          "schemaVersion": "1",
          "planId": "plan-001",
          "phase": "run",
          "status": "passed",
          "runtimeStatus": "completed",
          "validation": {"valid": true, "exitCode": 0},
          "listedCommands": [{"index": 0, "name": "hcHealth"}],
          "outputDir": "output/run-001",
          "evidenceTier": "real_e2e",
          "artifactPaths": ["output/run-001/run.json"],
          "restoration": {"required": false, "verified": false, "status": "not_requested"},
          "targetResults": [{
            "entity": "unknown-entity",
            "capability": "switch",
            "scenario": "control_readback",
            "status": "passed",
            "repetitions": {"requested": 1, "completed": 1, "passed": 1, "failed": 0},
            "artifactPaths": ["output/run-001/run.json"],
            "evidenceTier": "real_e2e"
          }]
        }"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("output/run-001")).unwrap();
    fs::write(root.join("output/run-001/run.json"), "{}\n").unwrap();
    fs::write(
        &inventory,
        r#"{
          "schemaVersion": "1",
          "snapshotId": "current-001",
          "source": "live_hc_discovery",
          "capturedAt": "2026-10-20T10:05:00Z",
          "complete": true,
          "entities": [{
            "entity": "entity-1",
            "capabilities": ["switch"],
            "fingerprint": "sha256:entity",
            "mappingFingerprint": "sha256:mapping"
          }]
        }"#,
    )
    .unwrap();

    let aggregated = Command::new(binary())
        .args([
            "plan",
            "aggregate",
            plan.to_str().unwrap(),
            "--response",
            response.to_str().unwrap(),
            "--inventory-current",
            inventory.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!aggregated.status.success());
    assert!(!output_dir.join("aggregate.json").exists());
    let _ = fs::remove_dir_all(root);
}
