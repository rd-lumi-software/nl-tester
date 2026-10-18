use lumi_tester::adapter::{
    AdapterRegistry, AdapterResult, AdapterStatus, MultiTargetRunner, TargetAdapter, TargetScope,
    TargetSession,
};
use lumi_tester::parser::normalize::normalize_flow;
use lumi_tester::parser::types::{ComponentSource, ComponentSpec, TargetId, TestCommand};
use lumi_tester::runner::context::TestContext;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn component(platform: &str) -> ComponentSpec {
    ComponentSpec {
        target: match platform {
            "lumi_hc" => TargetId::Gateway,
            "lumi_cloud" => TargetId::Cloud,
            _ => TargetId::App,
        },
        platform: platform.to_string(),
        app_id: None,
        profile: None,
        capabilities: Vec::new(),
        source: ComponentSource::Canonical,
    }
}

#[test]
fn registry_creates_existing_app_adapter() {
    let registry = AdapterRegistry::standard();
    let adapter = registry
        .create(TargetId::App, &component("android"))
        .unwrap();

    assert_eq!(adapter.target(), TargetId::App);
    assert_eq!(adapter.adapter_name(), "app_platform_driver");
}

#[test]
fn registry_creates_hc_gateway_adapter() {
    let registry = AdapterRegistry::standard();
    let adapter = registry
        .create(TargetId::Gateway, &component("lumi_hc"))
        .unwrap();

    assert_eq!(adapter.target(), TargetId::Gateway);
    assert_eq!(adapter.adapter_name(), "lumi_hc");
}

#[test]
fn registry_creates_cloud_mock_adapter_without_network() {
    let registry = AdapterRegistry::standard();
    let mut adapter = registry
        .create(TargetId::Cloud, &component("lumi_cloud"))
        .unwrap();
    let context = TestContext::new(Path::new("."), None, false, None);
    let command = command("setVar: { name: service, value: ready }");

    assert_eq!(adapter.preflight(&context).status, AdapterStatus::Passed);
    assert_eq!(adapter.open(&context).status, AdapterStatus::Passed);
    let result = adapter.execute(&command, &TargetScope::root(TargetId::Cloud));

    assert_eq!(result.status, AdapterStatus::Passed);
    assert_eq!(result.evidence_tier.as_deref(), Some("mock_contract"));
    assert_eq!(result.adapter, "cloud_mock");
}

#[test]
fn real_cloud_profile_is_blocked_as_unsupported_adapter() {
    let registry = AdapterRegistry::standard();
    let mut cloud = component("lumi_cloud");
    cloud.profile = Some(fixture_dir().join("profiles").join("cloud-real.yaml"));
    let mut adapter = registry.create(TargetId::Cloud, &cloud).unwrap();
    let context = TestContext::new(Path::new("."), None, false, None);

    let result = adapter.preflight(&context);

    assert_eq!(result.status, AdapterStatus::Blocked);
    assert_eq!(result.category, "UnsupportedAdapter");
    assert_eq!(result.evidence_tier.as_deref(), Some("real_preflight"));
}

#[tokio::test(flavor = "multi_thread")]
async fn attached_app_adapter_delegates_to_existing_executor() {
    let source = Path::new("attached-app.yaml");
    let flow = lumi_tester::parser::yaml::parse_yaml_content(
        "app:\n  platform: windows\n  appId: com.lumi.test\n---\n- setVar: { name: delegated, value: yes }\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();
    let output = unique_temp_dir("attached-app");
    let driver: Box<dyn lumi_tester::driver::traits::PlatformDriver> =
        Box::new(lumi_tester::driver::windows::WindowsDriver::new());
    let mut runner = MultiTargetRunner::standard_with_app_driver(
        normalized,
        driver,
        &output,
    )
    .unwrap();

    let report = runner.run().unwrap();

    assert_eq!(report.status, AdapterStatus::Passed);
    assert_eq!(report.operations[0].adapter, "app_platform_driver");
    assert_eq!(
        report.operations[0].result.evidence_tier.as_deref(),
        Some("real_e2e")
    );
    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn gateway_preflight_uses_profile_and_reports_unresolved_environment() {
    let source = fixture_dir().join("parent-real-blocked.yaml");
    let flow = lumi_tester::parser::yaml::parse_test_file(&source).unwrap();
    let normalized = normalize_flow(flow, &source).unwrap();
    let mut runner = MultiTargetRunner::standard(normalized).unwrap();

    let report = runner.run().unwrap();
    let gateway = report
        .operations
        .iter()
        .find(|operation| operation.target == TargetId::Gateway)
        .unwrap();

    assert_eq!(gateway.result.status, AdapterStatus::Blocked);
    assert_eq!(gateway.result.category, "NoLab");
    assert!(gateway.result.message.contains("endpoint environment"));
}

#[test]
fn unsupported_target_platform_is_rejected() {
    let registry = AdapterRegistry::standard();
    let error = match registry.create(TargetId::Gateway, &component("lumi_cloud")) {
        Ok(_) => panic!("unsupported target/platform pair was accepted"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "UnsupportedTargetPlatform");
}

#[test]
fn one_session_reuses_adapter_across_nested_flows() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let adapter = RecordingAdapter::new(TargetId::App, "android", calls.clone());
    let mut session = TargetSession::from_adapters(vec![Box::new(adapter)]).unwrap();
    let context = TestContext::new(Path::new("."), None, false, None);
    let command = command("launchApp");

    session
        .execute(
            TargetId::App,
            &command,
            TargetScope::root(TargetId::App),
            &context,
        )
        .unwrap();
    session
        .execute(
            TargetId::App,
            &command,
            TargetScope::nested(TargetId::App, 1),
            &context,
        )
        .unwrap();
    session.close_all(&context);

    let calls = calls.lock().unwrap();
    assert_eq!(calls.iter().filter(|call| *call == "app:open").count(), 1);
    assert_eq!(
        calls.iter().filter(|call| *call == "app:execute").count(),
        2
    );
}

#[test]
fn adapters_close_once_in_reverse_open_order() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let adapters: Vec<Box<dyn TargetAdapter>> = vec![
        Box::new(RecordingAdapter::new(
            TargetId::App,
            "android",
            calls.clone(),
        )),
        Box::new(RecordingAdapter::new(
            TargetId::Gateway,
            "lumi_hc",
            calls.clone(),
        )),
        Box::new(RecordingAdapter::new(
            TargetId::Cloud,
            "lumi_cloud",
            calls.clone(),
        )),
    ];
    let mut session = TargetSession::from_adapters(adapters).unwrap();
    let context = TestContext::new(Path::new("."), None, false, None);
    let app = command("launchApp");
    let gateway = command("hcHealth");
    let cloud = command("setVar: { name: x, value: y }");

    session
        .execute(
            TargetId::App,
            &app,
            TargetScope::root(TargetId::App),
            &context,
        )
        .unwrap();
    session
        .execute(
            TargetId::Gateway,
            &gateway,
            TargetScope::root(TargetId::Gateway),
            &context,
        )
        .unwrap();
    session
        .execute(
            TargetId::Cloud,
            &cloud,
            TargetScope::root(TargetId::Cloud),
            &context,
        )
        .unwrap();
    session.close_all(&context);
    session.close_all(&context);

    let closes: Vec<_> = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.ends_with(":close"))
        .cloned()
        .collect();
    assert_eq!(closes, ["cloud:close", "gateway:close", "app:close"]);
}

#[test]
fn blocked_preflight_is_not_failed_or_passed() {
    let result = AdapterResult::blocked("lumi_hc", "NoLab", "HC environment is unavailable");

    assert_eq!(result.status, AdapterStatus::Blocked);
    assert_ne!(result.status, AdapterStatus::Failed);
    assert_ne!(result.status, AdapterStatus::Passed);
}

#[test]
fn single_target_flow_uses_implicit_target() {
    let source = Path::new("single-target-runner.yaml");
    let flow = lumi_tester::parser::yaml::parse_yaml_content(
        "platform: android\nappId: com.lumi.test\n---\n- launchApp\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();
    let mut runner = MultiTargetRunner::mock_harness(normalized).unwrap();

    let report = runner.run().unwrap();

    assert_eq!(report.operations.len(), 1);
    assert_eq!(report.operations[0].target, TargetId::App);
}

#[test]
fn nested_runflow_dispatches_gateway_target() {
    let mut runner = fixture_runner("parent.yaml");

    let report = runner.run().unwrap();

    assert!(report
        .operations
        .iter()
        .any(|operation| operation.target == TargetId::Gateway));
}

#[test]
fn nested_runflow_dispatches_cloud_target() {
    let mut runner = fixture_runner("parent.yaml");

    let report = runner.run().unwrap();

    assert!(report
        .operations
        .iter()
        .any(|operation| operation.target == TargetId::Cloud));
}

#[test]
fn command_index_and_repeat_options_route_only_selected_root_flow() {
    let mut runner = fixture_runner("parent.yaml").with_options(
        lumi_tester::adapter::MultiTargetRunOptions {
            command_index: Some(1),
            repeat: 2,
            continue_on_failure: true,
            ..Default::default()
        },
    );

    let report = runner.run().unwrap();

    assert_eq!(report.operations.len(), 4);
    assert!(report
        .operations
        .iter()
        .all(|operation| operation.target == TargetId::Gateway));
}

#[test]
fn missing_command_filter_is_rejected() {
    let mut runner = fixture_runner("parent.yaml").with_options(
        lumi_tester::adapter::MultiTargetRunOptions {
            command_name: Some("does-not-exist".into()),
            ..Default::default()
        },
    );

    let error = runner.run().unwrap_err();
    assert!(error.to_string().contains("was not found"));
}

#[test]
fn app_gateway_cloud_app_sequence_uses_one_session() {
    let mut runner = fixture_runner("parent.yaml");

    let report = runner.run().unwrap();

    assert!(report
        .operations
        .iter()
        .all(|operation| operation.session_id == report.session_id));
    let targets: Vec<_> = report
        .operations
        .iter()
        .map(|operation| operation.target)
        .collect();
    assert_eq!(targets.first(), Some(&TargetId::App));
    assert_eq!(targets.last(), Some(&TargetId::App));
}

#[test]
fn child_platform_header_does_not_switch_app_driver() {
    let mut runner = fixture_runner("parent.yaml");

    let report = runner.run().unwrap();

    let app_operations: Vec<_> = report
        .operations
        .iter()
        .filter(|operation| operation.target == TargetId::App)
        .collect();
    assert!(!app_operations.is_empty());
    assert!(app_operations
        .iter()
        .all(|operation| operation.platform == "android"));
}

#[test]
fn nested_runflow_with_missing_target_fails_before_execution() {
    let source = fixture_dir().join("invalid-missing-target.yaml");
    let flow = lumi_tester::parser::yaml::parse_test_file(&source).unwrap();
    let normalized = normalize_flow(flow, &source).unwrap();
    let error = match MultiTargetRunner::mock_harness(normalized) {
        Ok(_) => panic!("invalid graph was accepted"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("explicit target"));
}

#[test]
fn runflow_when_false_skips_target_dispatch() {
    let source = Path::new("conditional-target.yaml");
    let flow = lumi_tester::parser::yaml::parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.test\ncloud:\n  platform: lumi_cloud\n  profile: tests/fixtures/multi_target/profiles/cloud.yaml\n---\n- runFlow:\n    target: app\n    when: false\n    commands:\n      - launchApp\n- runFlow:\n    target: cloud\n    commands:\n      - setVar: { name: reached, value: yes }\n",
        source,
    )
    .unwrap();
    let mut normalized = normalize_flow(flow, source).unwrap();
    normalized.components.get_mut(&TargetId::Cloud).unwrap().profile =
        Some(fixture_dir().join("profiles").join("cloud.yaml"));
    let mut runner = MultiTargetRunner::mock_harness(normalized).unwrap();

    let report = runner.run().unwrap();

    assert_eq!(report.operations.len(), 1);
    assert_eq!(report.operations[0].target, TargetId::Cloud);
}

#[test]
fn optional_missing_child_is_skipped_without_opening_adapter() {
    let source = Path::new("optional-child.yaml");
    let flow = lumi_tester::parser::yaml::parse_yaml_content(
        "platform: android\nappId: com.lumi.test\n---\n- runFlow:\n    path: missing-child.yaml\n    optional: true\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();
    let mut runner = MultiTargetRunner::mock_harness(normalized).unwrap();

    let report = runner.run().unwrap();

    assert_eq!(report.status, AdapterStatus::Passed);
    assert!(report.operations.is_empty());
    assert!(report.lifecycle.is_empty());
}

#[test]
fn mock_app_gateway_readback_returns_to_app_in_one_session() {
    let mut runner = fixture_runner("parent.yaml");

    let report = runner.run().unwrap();

    let targets: Vec<_> = report
        .operations
        .iter()
        .map(|operation| operation.target)
        .collect();
    let gateway = report
        .operations
        .iter()
        .filter(|operation| operation.target == TargetId::Gateway)
        .collect::<Vec<_>>();
    assert!(targets.starts_with(&[TargetId::App]));
    assert!(targets.ends_with(&[TargetId::App]));
    assert_eq!(gateway.len(), 2);
    assert_eq!(gateway[0].command, "hcControl");
    assert_eq!(gateway[1].command, "hcWaitState");
    assert_eq!(
        gateway[1].result.evidence_tier.as_deref(),
        Some("mock_contract")
    );
    assert!(gateway[1].result.raw_evidence.is_some());
    assert!(gateway[1].result.normalized_evidence.is_some());
    assert_eq!(report.status, AdapterStatus::Passed);
}

#[test]
fn target_lifecycle_events_are_emitted_in_jsonl() {
    let mut runner = fixture_runner("parent.yaml");
    let report = runner.run().unwrap();
    let output = unique_temp_dir("lifecycle-events");
    MultiTargetRunner::write_artifacts(&report, &output).unwrap();

    let events = std::fs::read_to_string(output.join("events.jsonl")).unwrap();
    let lifecycle: Vec<serde_json::Value> = events
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|event: &serde_json::Value| event["event"] == "targetLifecycle")
        .collect();

    assert!(!lifecycle.is_empty());
    assert!(lifecycle
        .iter()
        .all(|event| event.get("component").is_some()));
    assert!(lifecycle.iter().any(|event| event["phase"] == "close"));
    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn command_events_include_effective_target() {
    let mut runner = fixture_runner("parent.yaml");
    let report = runner.run().unwrap();
    let output = unique_temp_dir("command-events");
    MultiTargetRunner::write_artifacts(&report, &output).unwrap();

    let events = std::fs::read_to_string(output.join("events.jsonl")).unwrap();
    let commands: Vec<serde_json::Value> = events
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|event: &serde_json::Value| event["event"] == "commandFinished")
        .collect();

    assert!(commands.iter().all(|event| event.get("target").is_some()));
    assert!(commands.iter().all(|event| event.get("adapter").is_some()));
    assert!(commands
        .iter()
        .all(|event| event.get("correlationId").is_some()));
    let correlations = commands
        .iter()
        .map(|event| event["correlationId"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(correlations.len(), commands.len());
    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn blocked_target_is_preserved_in_run_and_test_results() {
    let report = report_with_result(AdapterResult::blocked("lumi_hc", "NoLab", "unavailable"));
    let output = unique_temp_dir("blocked-report");
    MultiTargetRunner::write_artifacts(&report, &output).unwrap();

    for name in ["run.json", "test-results.json"] {
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(output.join(name)).unwrap()).unwrap();
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["blocked"], 1);
        assert_eq!(value["failed"], 0);
    }
    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn error_target_is_distinct_from_product_failure() {
    let mut result = AdapterResult::error("lumi_hc", "transport crashed");
    result.status = AdapterStatus::Error;
    let report = report_with_result(result);

    assert_eq!(report.status, AdapterStatus::Error);
    assert_eq!(report.errors, 1);
    assert_eq!(report.failed, 0);
}

#[test]
fn legacy_app_report_shape_remains_readable() {
    let legacy = serde_json::json!({
        "sessionId": "legacy-app-session",
        "flows": [],
        "summary": {
            "sessionId": "legacy-app-session",
            "totalFlows": 0,
            "totalCommands": 0,
            "passed": 0,
            "failed": 0,
            "skipped": 0,
            "totalDurationMs": 0
        },
        "generatedAt": "2026-10-18T00:00:00Z"
    });

    let parsed: lumi_tester::report::types::TestResults = serde_json::from_value(legacy).unwrap();

    assert_eq!(parsed.session_id, "legacy-app-session");
}

#[test]
fn multi_target_artifacts_keep_legacy_report_reader_shape_additively() {
    let mut runner = fixture_runner("parent.yaml");
    let report = runner.run().unwrap();
    let output = unique_temp_dir("legacy-report-shape");
    MultiTargetRunner::write_artifacts(&report, &output).unwrap();

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("test-results.json")).unwrap())
            .unwrap();
    let parsed: lumi_tester::report::types::TestResults =
        serde_json::from_value(value.clone()).unwrap();

    assert_eq!(parsed.session_id, report.session_id);
    assert_eq!(parsed.summary.total_commands, report.operations.len() as u32);
    assert_eq!(value["status"], "passed");
    assert!(value["operations"].is_array());
    assert!(value["flows"].is_array());
    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn multi_target_artifact_flags_do_not_force_optional_outputs() {
    let mut runner = fixture_runner("parent.yaml");
    let report = runner.run().unwrap();
    let output = unique_temp_dir("artifact-flags");
    MultiTargetRunner::write_artifacts_with_options(&report, &output, false, false).unwrap();

    assert!(output.join("run.json").is_file());
    assert!(!output.join("test-results.json").exists());
    assert!(!output.join("events.jsonl").exists());
    assert!(!output.join("report.html").exists());
    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn mock_evidence_contains_adapter_and_evidence_tier() {
    let mut runner = fixture_runner("parent.yaml");
    let report = runner.run().unwrap();

    assert!(report.operations.iter().all(|operation| {
        !operation.adapter.is_empty()
            && operation.result.evidence_tier.as_deref() == Some("mock_contract")
    }));
}

fn report_with_result(result: AdapterResult) -> lumi_tester::adapter::MultiTargetRunReport {
    let status = result.status;
    lumi_tester::adapter::MultiTargetRunReport {
        schema_version: 2,
        session_id: "session-test".into(),
        status,
        passed: usize::from(status == AdapterStatus::Passed),
        failed: usize::from(status == AdapterStatus::Failed),
        blocked: usize::from(status == AdapterStatus::Blocked),
        errors: usize::from(status == AdapterStatus::Error),
        operations: vec![lumi_tester::adapter::OperationRecord {
            session_id: "session-test".into(),
            target: TargetId::Gateway,
            platform: "lumi_hc".into(),
            adapter: result.adapter.clone(),
            command: "hcHealth".into(),
            command_index: 0,
            depth: 0,
            correlation_id: "correlation-test".into(),
            profile: None,
            result,
        }],
        lifecycle: Vec::new(),
    }
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lumi-multi-target-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn fixture_runner(name: &str) -> MultiTargetRunner {
    let source = fixture_dir().join(name);
    let flow = lumi_tester::parser::yaml::parse_test_file(&source).unwrap();
    let normalized = normalize_flow(flow, &source).unwrap();
    MultiTargetRunner::mock_harness(normalized).unwrap()
}

fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("multi_target")
}

struct RecordingAdapter {
    target: TargetId,
    platform: String,
    calls: Arc<Mutex<Vec<String>>>,
}

fn command(yaml: &str) -> TestCommand {
    let source = Path::new("adapter-command.yaml");
    let document = format!("platform: android\nappId: com.lumi.test\n---\n- {yaml}\n");
    lumi_tester::parser::yaml::parse_yaml_content(&document, source)
        .unwrap()
        .commands
        .into_iter()
        .next()
        .unwrap()
}

impl RecordingAdapter {
    fn new(target: TargetId, platform: &str, calls: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            target,
            platform: platform.to_string(),
            calls,
        }
    }

    fn record(&self, operation: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:{operation}", self.target));
    }
}

impl TargetAdapter for RecordingAdapter {
    fn target(&self) -> TargetId {
        self.target
    }

    fn platform(&self) -> &str {
        &self.platform
    }

    fn adapter_name(&self) -> &str {
        "recording"
    }

    fn capabilities(&self) -> &[String] {
        &[]
    }

    fn preflight(&mut self, _context: &TestContext) -> AdapterResult {
        self.record("preflight");
        AdapterResult::passed("recording")
    }

    fn open(&mut self, _context: &TestContext) -> AdapterResult {
        self.record("open");
        AdapterResult::passed("recording")
    }

    fn execute(&mut self, _command: &TestCommand, _scope: &TargetScope) -> AdapterResult {
        self.record("execute");
        AdapterResult::passed("recording")
    }

    fn close(&mut self, _context: &TestContext) -> AdapterResult {
        self.record("close");
        AdapterResult::passed("recording")
    }
}
