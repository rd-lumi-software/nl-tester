use lumi_tester::adapter::{
    AdapterRegistry, AdapterResult, AdapterStatus, MultiTargetRunner, TargetAdapter, TargetScope,
    TargetSession,
};
use lumi_tester::hc::{
    HcBackendConfig, HcBackendDescriptor, HcBackendRegistry, HcBackendResult, HcCapability,
    HcCommand, HcEntity, HcEvidence, HcEvidenceTier, HcExecutionScope, HcObservation,
    HcProtocolBackend, HcState, HcStateUpdate,
};
use lumi_tester::parser::normalize::normalize_flow;
use lumi_tester::parser::types::{ComponentSource, ComponentSpec, TargetId, TestCommand};
use lumi_tester::parser::validation::validate_flow_graph;
use lumi_tester::runner::context::TestContext;
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
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
    let mut runner =
        MultiTargetRunner::standard_with_app_driver(normalized, driver, &output).unwrap();

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
fn custom_hc_backend_supports_generic_validate_list_run_and_artifacts() {
    let dir = unique_temp_dir("custom-hc-backend");
    fs::create_dir_all(dir.join("profiles")).unwrap();
    let source = dir.join("generic-hc.yaml");
    fs::write(
        dir.join("profiles/fake.yaml"),
        "implementation: fake-home-controller\nprotocolPack: fake-pack\nprotocolVersion: 1.0\npackVersion: 2.0\n",
    )
    .unwrap();
    fs::write(
        dir.join("profiles/second.yaml"),
        "implementation: second-home-controller\nprotocolPack: second-pack\nprotocolVersion: 3.0\npackVersion: 4.0\n",
    )
    .unwrap();
    fs::write(
        &source,
        "platform: lumi_hc\nprofile: profiles/fake.yaml\n---\n- hcHealth\n- hcGetState:\n    entity: \"123\"\n",
    )
    .unwrap();
    let second_source = dir.join("second-hc.yaml");
    fs::write(
        &second_source,
        "platform: lumi_hc\nprofile: profiles/second.yaml\n---\n- hcHealth\n- hcGetState:\n    entity: \"123\"\n",
    )
    .unwrap();

    let flow = lumi_tester::parser::yaml::parse_test_file(&source).unwrap();
    let normalized = normalize_flow(flow, &source).unwrap();
    validate_flow_graph(&normalized).unwrap();
    let listed = normalized
        .commands
        .iter()
        .map(TestCommand::display_name)
        .collect::<Vec<_>>();
    assert_eq!(listed, vec!["hcHealth", "hcGetState"]);

    let calls = Arc::new(Mutex::new(Vec::new()));
    let constructor_calls = calls.clone();
    let backend_calls = calls.clone();
    let mut hc_registry = HcBackendRegistry::empty();
    hc_registry
        .register(
            HcBackendDescriptor::new("fake-home-controller", "fake-pack", "1.0", "2.0"),
            move |config| {
                constructor_calls
                    .lock()
                    .unwrap()
                    .push(format!("construct:{}", config.implementation));
                Box::new(ContractHcBackend::new(config, backend_calls.clone()))
            },
        )
        .unwrap();
    let second_constructor_calls = calls.clone();
    let second_backend_calls = calls.clone();
    hc_registry
        .register(
            HcBackendDescriptor::new("second-home-controller", "second-pack", "3.0", "4.0"),
            move |config| {
                second_constructor_calls
                    .lock()
                    .unwrap()
                    .push(format!("construct:{}", config.implementation));
                Box::new(ContractHcBackend::new(config, second_backend_calls.clone()))
            },
        )
        .unwrap();

    let registry = AdapterRegistry::with_hc_backend_registry(hc_registry.clone());
    let output = dir.join("out");
    let mut runner =
        MultiTargetRunner::standard_with_adapter_registry(normalized, registry).unwrap();
    let report = runner.run().unwrap();

    assert_eq!(report.status, AdapterStatus::Passed);
    assert_eq!(report.operations.len(), 2);
    assert!(report
        .operations
        .iter()
        .all(|operation| operation.adapter == "lumi_hc"));
    assert!(report.operations.iter().all(|operation| {
        operation.result.evidence_tier.as_deref() == Some("mock_conformance")
    }));
    MultiTargetRunner::write_artifacts(&report, &output).unwrap();
    assert!(output.join("run.json").is_file());
    assert!(output.join("test-results.json").is_file());

    {
        let calls = calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|call| call == "construct:fake-home-controller"));
        assert!(calls
            .iter()
            .any(|call| call.starts_with("execute:hcGetState:123:")));
        assert!(calls.iter().any(|call| call == "lifecycle:preflight"));
        assert!(calls.iter().any(|call| call == "lifecycle:open"));
        assert!(calls.iter().any(|call| call == "lifecycle:close"));
    }

    let second_flow = lumi_tester::parser::yaml::parse_test_file(&second_source).unwrap();
    let second_normalized = normalize_flow(second_flow, &second_source).unwrap();
    validate_flow_graph(&second_normalized).unwrap();
    let second_registry = AdapterRegistry::with_hc_backend_registry(hc_registry);
    let mut second_runner =
        MultiTargetRunner::standard_with_adapter_registry(second_normalized, second_registry)
            .unwrap();
    let second_report = second_runner.run().unwrap();
    assert_eq!(second_report.status, AdapterStatus::Passed);
    assert_eq!(second_report.operations.len(), 2);
    assert!(calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call == "construct:second-home-controller"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn stateful_fake_backends_preserve_hc_semantics_and_artifacts() {
    let dir = unique_temp_dir("stateful-hc-backends");
    fs::create_dir_all(dir.join("profiles")).unwrap();
    fs::write(
        dir.join("profiles/fake-a.yaml"),
        "implementation: fake-state-a\nprotocolPack: fake-pack-a\nprotocolVersion: 1.0\npackVersion: 1.0\n",
    )
    .unwrap();
    fs::write(
        dir.join("profiles/fake-b.yaml"),
        "implementation: fake-state-b\nprotocolPack: fake-pack-b\nprotocolVersion: 2.0\npackVersion: 2.0\n",
    )
    .unwrap();

    let commands = "- hcHealth\n- hcGetEntities\n- hcGetState:\n    entity: \"123\"\n- hcControl:\n    entity: \"123\"\n    state:\n      - idx: 0\n        value: true\n- hcWaitState:\n    entity: \"123\"\n    state:\n      - idx: 0\n        value: true\n";
    let source_a = write_stateful_flow(&dir, "stateful-a.yaml", "fake-a.yaml", commands);
    let source_b = write_stateful_flow(&dir, "stateful-b.yaml", "fake-b.yaml", commands);

    let (report_a, calls_a) = run_stateful_flow(&source_a);
    let (report_b, calls_b) = run_stateful_flow(&source_b);
    assert_eq!(report_a.status, AdapterStatus::Passed);
    assert_eq!(report_b.status, AdapterStatus::Passed);
    assert_eq!(report_a.operations.len(), 5);
    assert_eq!(report_b.operations.len(), 5);

    assert_eq!(execute_calls(&calls_a), execute_calls(&calls_b));
    assert_eq!(
        lifecycle_calls(&calls_a),
        vec!["preflight", "open", "close"]
    );
    assert_eq!(
        lifecycle_calls(&calls_b),
        vec!["preflight", "open", "close"]
    );
    assert_eq!(report_a.operations[3].result.status, AdapterStatus::Passed);
    assert_eq!(report_a.operations[4].result.status, AdapterStatus::Passed);
    assert_eq!(
        report_a.operations[3].result.raw_evidence.as_ref().unwrap()["readback"],
        json!({"entity": "123", "state": [{"idx": 0, "value": true}]})
    );
    assert_eq!(
        report_a.operations[4].result.raw_evidence.as_ref().unwrap()["assertion"],
        json!({"readbackMatches": true})
    );
    assert_eq!(
        report_a.operations[4].result.raw_evidence.as_ref().unwrap()["readback"],
        json!({"entity": "123", "state": [{"idx": 0, "value": true}]})
    );

    for (left, right) in report_a.operations.iter().zip(&report_b.operations) {
        assert_eq!(
            hc_result_contract(&left.result),
            hc_result_contract(&right.result)
        );
        assert_eq!(left.result.evidence_tier, right.result.evidence_tier);
        assert!(left.result.raw_evidence.as_ref().unwrap()["evidence"]
            .as_array()
            .is_some_and(|evidence| !evidence.is_empty()));
    }

    let output_a = dir.join("out-a");
    let output_b = dir.join("out-b");
    MultiTargetRunner::write_artifacts(&report_a, &output_a).unwrap();
    MultiTargetRunner::write_artifacts(&report_b, &output_b).unwrap();
    assert_stateful_artifacts(&output_a, "fake-state-a", "fake-pack-a", "1.0", "1.0");
    assert_stateful_artifacts(&output_b, "fake-state-b", "fake-pack-b", "2.0", "2.0");

    let mismatch_source = write_stateful_flow(
        &dir,
        "stateful-mismatch.yaml",
        "fake-a.yaml",
        "- hcWaitState:\n    entity: \"123\"\n    state:\n      - idx: 0\n        value: true\n",
    );
    let mismatch_flow = lumi_tester::parser::yaml::parse_test_file(&mismatch_source).unwrap();
    let mismatch_normalized = normalize_flow(mismatch_flow, &mismatch_source).unwrap();
    validate_flow_graph(&mismatch_normalized).unwrap();
    let mismatch_calls = Arc::new(Mutex::new(Vec::new()));
    let mut mismatch_runner = MultiTargetRunner::standard_with_adapter_registry(
        mismatch_normalized,
        AdapterRegistry::with_hc_backend_registry(stateful_registry(mismatch_calls)),
    )
    .unwrap();
    let mismatch_report = mismatch_runner.run().unwrap();
    assert_eq!(mismatch_report.status, AdapterStatus::Failed);
    assert_eq!(
        mismatch_report.operations[0].result.category,
        "readback_mismatch"
    );
    assert_eq!(
        mismatch_report.operations[0]
            .result
            .raw_evidence
            .as_ref()
            .unwrap()["assertion"],
        json!({"readbackMatches": false})
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn hc_gateway_close_preserves_backend_result_fields() {
    let dir = unique_temp_dir("hc-close-result");
    fs::create_dir_all(dir.join("profiles")).unwrap();
    let source = dir.join("generic-hc.yaml");
    let profile = dir.join("profiles/fake.yaml");
    fs::write(
        &profile,
        "implementation: fake-home-controller\nprotocolPack: fake-pack\nprotocolVersion: 1.0\npackVersion: 2.0\n",
    )
    .unwrap();
    fs::write(
        &source,
        "platform: lumi_hc\nprofile: profiles/fake.yaml\n---\n- hcHealth\n",
    )
    .unwrap();
    let flow = lumi_tester::parser::yaml::parse_test_file(&source).unwrap();
    let normalized = normalize_flow(flow, &source).unwrap();
    let component = normalized.components.get(&TargetId::Gateway).unwrap();
    let close_result = HcBackendResult::failed("close_category", "close message")
        .with_evidence_tier(HcEvidenceTier::RealPreflight)
        .with_evidence(HcEvidence::raw("close_raw", json!({"closed": false})));
    let mut hc_registry = HcBackendRegistry::empty();
    hc_registry
        .register(
            HcBackendDescriptor::new("fake-home-controller", "fake-pack", "1.0", "2.0"),
            move |config| Box::new(CloseResultHcBackend::new(config, close_result.clone())),
        )
        .unwrap();

    let registry = AdapterRegistry::with_hc_backend_registry(hc_registry);
    let mut adapter = registry
        .create_with_source(TargetId::Gateway, component, Some(&source))
        .unwrap();
    let context = TestContext::new(&dir, None, false, None);
    assert_eq!(adapter.preflight(&context).status, AdapterStatus::Passed);
    assert_eq!(adapter.open(&context).status, AdapterStatus::Passed);

    let result = adapter.close(&context);
    assert_eq!(result.status, AdapterStatus::Failed);
    assert_eq!(result.category, "close_category");
    assert_eq!(result.message, "close message");
    assert_eq!(result.evidence_tier.as_deref(), Some("real_preflight"));
    let close_evidence = result.raw_evidence.unwrap()["evidence"][0].clone();
    assert_eq!(close_evidence["kind"], "close_raw");
    assert_eq!(close_evidence["closed"], false);

    let _ = fs::remove_dir_all(dir);
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
    let mut runner =
        fixture_runner("parent.yaml").with_options(lumi_tester::adapter::MultiTargetRunOptions {
            command_index: Some(1),
            repeat: 2,
            continue_on_failure: true,
            ..Default::default()
        });

    let report = runner.run().unwrap();

    assert_eq!(report.operations.len(), 4);
    assert_eq!(report.repeat_count, 2);
    assert_eq!(
        report
            .operations
            .iter()
            .map(|operation| operation.repeat_index)
            .collect::<Vec<_>>(),
        vec![0, 0, 1, 1]
    );
    assert!(report
        .operations
        .iter()
        .all(|operation| operation.target == TargetId::Gateway));
}

#[test]
fn missing_command_filter_is_rejected() {
    let mut runner =
        fixture_runner("parent.yaml").with_options(lumi_tester::adapter::MultiTargetRunOptions {
            command_name: Some("does-not-exist".into()),
            ..Default::default()
        });

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
    normalized
        .components
        .get_mut(&TargetId::Cloud)
        .unwrap()
        .profile = Some(fixture_dir().join("profiles").join("cloud.yaml"));
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
    assert_eq!(
        parsed.summary.total_commands,
        report.operations.len() as u32
    );
    assert_eq!(value["status"], "passed");
    assert!(value["operations"].is_array());
    assert!(value["flows"].is_array());
    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn multi_target_report_uses_summary_renderer_and_preserves_operation_metadata() {
    let mut runner = fixture_runner("parent.yaml");
    let report = runner.run().unwrap();
    let output = unique_temp_dir("summary-report");
    MultiTargetRunner::write_artifacts(&report, &output).unwrap();

    let artifact: Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("test-results.json")).unwrap())
            .unwrap();
    let html = std::fs::read_to_string(output.join("report.html")).unwrap();

    assert_eq!(
        artifact["multiTarget"]["operationCount"],
        report.operations.len()
    );
    assert_eq!(artifact["multiTarget"]["repeatCount"], 1);
    assert!(artifact["multiTarget"]["operations"].is_array());
    assert!(html.contains("Multi-target execution"));
    assert!(html.contains("Operation details"));
    assert!(html.contains("lumi_hc_mock"));
    assert!(html.contains("hcControl"));

    let _ = std::fs::remove_dir_all(output);
}

#[test]
fn multi_target_summary_handles_missing_evidence_without_failing() {
    let report = report_with_result(AdapterResult::passed("lumi_hc"));
    let output = unique_temp_dir("summary-missing-evidence");
    MultiTargetRunner::write_artifacts(&report, &output).unwrap();

    let html = std::fs::read_to_string(output.join("report.html")).unwrap();
    assert!(html.contains("Not captured"));

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
        repeat_count: 1,
        operations: vec![lumi_tester::adapter::OperationRecord {
            session_id: "session-test".into(),
            target: TargetId::Gateway,
            platform: "lumi_hc".into(),
            adapter: result.adapter.clone(),
            command: "hcHealth".into(),
            command_index: 0,
            depth: 0,
            correlation_id: "correlation-test".into(),
            repeat_index: 0,
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

#[derive(Debug, Clone, PartialEq)]
enum StatefulCall {
    Lifecycle(&'static str),
    Execute {
        command: HcCommand,
        scope: HcExecutionScope,
    },
}

fn write_stateful_flow(
    dir: &Path,
    file_name: &str,
    profile: &str,
    commands: &str,
) -> std::path::PathBuf {
    let source = dir.join(file_name);
    fs::write(
        &source,
        format!("platform: lumi_hc\nprofile: profiles/{profile}\n---\n{commands}"),
    )
    .unwrap();
    source
}

fn run_stateful_flow(
    source: &Path,
) -> (
    lumi_tester::adapter::MultiTargetRunReport,
    Arc<Mutex<Vec<StatefulCall>>>,
) {
    let flow = lumi_tester::parser::yaml::parse_test_file(source).unwrap();
    let normalized = normalize_flow(flow, source).unwrap();
    validate_flow_graph(&normalized).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let registry = AdapterRegistry::with_hc_backend_registry(stateful_registry(calls.clone()));
    let mut runner =
        MultiTargetRunner::standard_with_adapter_registry(normalized, registry).unwrap();
    (runner.run().unwrap(), calls)
}

fn stateful_registry(calls: Arc<Mutex<Vec<StatefulCall>>>) -> HcBackendRegistry {
    let mut registry = HcBackendRegistry::empty();
    registry
        .register(
            HcBackendDescriptor::new("fake-state-a", "fake-pack-a", "1.0", "1.0"),
            {
                let calls = calls.clone();
                move |config| Box::new(StatefulHcBackend::new(config, calls.clone()))
            },
        )
        .unwrap();
    registry
        .register(
            HcBackendDescriptor::new("fake-state-b", "fake-pack-b", "2.0", "2.0"),
            move |config| Box::new(StatefulHcBackend::new(config, calls.clone())),
        )
        .unwrap();
    registry
}

fn execute_calls(calls: &Arc<Mutex<Vec<StatefulCall>>>) -> Vec<(HcCommand, HcExecutionScope)> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|call| match call {
            StatefulCall::Execute { command, scope } => Some((command.clone(), scope.clone())),
            StatefulCall::Lifecycle(_) => None,
        })
        .collect()
}

fn lifecycle_calls(calls: &Arc<Mutex<Vec<StatefulCall>>>) -> Vec<String> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|call| match call {
            StatefulCall::Lifecycle(name) => Some((*name).to_string()),
            StatefulCall::Execute { .. } => None,
        })
        .collect()
}

fn hc_result_contract(result: &AdapterResult) -> Value {
    let raw = result.raw_evidence.as_ref().unwrap();
    json!({
        "status": result.status,
        "category": result.category,
        "message": result.message,
        "evidenceTier": result.evidence_tier,
        "action": raw["action"],
        "readback": raw["readback"],
        "assertion": raw["assertion"],
        "normalized": result.normalized_evidence,
    })
}

fn assert_stateful_artifacts(
    output: &Path,
    implementation: &str,
    protocol_pack: &str,
    protocol_version: &str,
    pack_version: &str,
) {
    for file_name in ["run.json", "test-results.json"] {
        let artifact: Value =
            serde_json::from_str(&fs::read_to_string(output.join(file_name)).unwrap()).unwrap();
        let operations = artifact["operations"].as_array().unwrap();
        assert_eq!(operations.len(), 5);
        for operation in operations {
            let result = &operation["result"];
            assert_eq!(result["status"], "passed");
            assert_eq!(result["adapter"], "lumi_hc");
            assert!(!result["category"].as_str().unwrap().is_empty());
            assert!(!result["message"].as_str().unwrap().is_empty());
            assert_eq!(result["evidenceTier"], "mock_conformance");
            let raw = &result["rawEvidence"];
            assert_eq!(raw["backend"]["implementation"], implementation);
            assert_eq!(raw["backend"]["protocolPack"], protocol_pack);
            assert_eq!(raw["backend"]["protocolVersion"], protocol_version);
            assert_eq!(raw["backend"]["packVersion"], pack_version);
            assert!(raw.get("action").is_some());
            assert!(raw.get("readback").is_some());
            assert!(raw.get("assertion").is_some());
            let evidence = raw["evidence"].as_array().unwrap();
            assert!(!evidence.is_empty());
            assert_eq!(evidence[0]["kind"], "fake_transport");
            assert_eq!(evidence[0]["implementation"], implementation);
            assert!(evidence[0]["command"].is_string());
            assert!(result.get("normalizedEvidence").is_some());
        }
    }
}

fn state_updates_json(updates: &[HcStateUpdate]) -> Value {
    Value::Array(
        updates
            .iter()
            .map(|update| match update {
                HcStateUpdate::Indexed { idx, value } => {
                    json!({"idx": idx, "value": value})
                }
                HcStateUpdate::Capability { name, value } => {
                    json!({"name": name, "value": value})
                }
            })
            .collect(),
    )
}

struct StatefulHcBackend {
    config: HcBackendConfig,
    calls: Arc<Mutex<Vec<StatefulCall>>>,
    entities: Vec<HcEntity>,
    states: BTreeMap<String, Vec<HcStateUpdate>>,
    capabilities: Vec<HcCapability>,
}

impl StatefulHcBackend {
    fn new(config: &HcBackendConfig, calls: Arc<Mutex<Vec<StatefulCall>>>) -> Self {
        let entity = HcEntity {
            id: "123".into(),
            name: Some("Fake light".into()),
            capabilities: vec![HcCapability::new("power", json!(true))],
        };
        Self {
            config: config.clone(),
            calls,
            entities: vec![entity],
            states: BTreeMap::from([(
                "123".into(),
                vec![HcStateUpdate::Indexed {
                    idx: 0,
                    value: json!(false),
                }],
            )]),
            capabilities: vec![HcCapability::new("health", json!(true))],
        }
    }

    fn record(&self, call: StatefulCall) {
        self.calls.lock().unwrap().push(call);
    }

    fn evidence(&self, command: &HcCommand) -> HcEvidence {
        HcEvidence::raw(
            "fake_transport",
            json!({
                "implementation": self.config.implementation,
                "command": command.display_name(),
            }),
        )
    }

    fn result(&self, command: &HcCommand, result: HcBackendResult) -> HcBackendResult {
        result
            .with_evidence_tier(HcEvidenceTier::MockConformance)
            .with_evidence(self.evidence(command))
    }
}

impl HcProtocolBackend for StatefulHcBackend {
    fn implementation_id(&self) -> &str {
        &self.config.implementation
    }

    fn protocol_pack(&self) -> &str {
        &self.config.protocol_pack
    }

    fn capabilities(&self) -> &[HcCapability] {
        &self.capabilities
    }

    fn preflight(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record(StatefulCall::Lifecycle("preflight"));
        HcBackendResult::passed("fake preflight")
    }

    fn open(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record(StatefulCall::Lifecycle("open"));
        HcBackendResult::passed("fake open")
    }

    fn execute(&mut self, command: &HcCommand, scope: &HcExecutionScope) -> HcBackendResult {
        self.record(StatefulCall::Execute {
            command: command.clone(),
            scope: scope.clone(),
        });
        match command {
            HcCommand::Health => self.result(
                command,
                HcBackendResult::passed("fake health").with_normalized(json!({"healthy": true})),
            ),
            HcCommand::GetEntities => self.result(
                command,
                HcBackendResult::passed("fake discovery")
                    .with_observation(HcObservation::discovery(self.entities.clone()))
                    .with_normalized(json!({"entities": self.entities})),
            ),
            HcCommand::GetState { entity } => {
                let state = self.states.get(entity).cloned().unwrap_or_default();
                let readback = json!({"entity": entity, "state": state_updates_json(&state)});
                self.result(
                    command,
                    HcBackendResult::passed("fake state")
                        .with_readback(readback.clone())
                        .with_observation(HcObservation {
                            entities: None,
                            entity: None,
                            state: Some(HcState {
                                entity: entity.clone(),
                                updates: self.states.get(entity).cloned().unwrap_or_default(),
                            }),
                            value: None,
                        })
                        .with_normalized(readback),
                )
            }
            HcCommand::Control { entity, updates } => {
                self.states.insert(entity.clone(), updates.clone());
                let updates_json = state_updates_json(updates);
                let readback = json!({"entity": entity, "state": updates_json.clone()});
                self.result(
                    command,
                    HcBackendResult::passed("fake control")
                        .with_action(json!({"entity": entity, "updates": updates_json}))
                        .with_readback(readback.clone())
                        .with_assertion(json!({"readbackMatches": true}))
                        .with_normalized(readback)
                        .with_readback_match(true),
                )
            }
            HcCommand::WaitState { entity, expected } => {
                let actual = self.states.get(entity).cloned().unwrap_or_default();
                let matched = actual == *expected;
                let readback = json!({"entity": entity, "state": state_updates_json(&actual)});
                let expected_json = state_updates_json(expected);
                let result = if matched {
                    HcBackendResult::passed("fake wait")
                } else {
                    HcBackendResult::failed("readback_mismatch", "fake state did not match")
                };
                self.result(
                    command,
                    result
                        .with_action(json!({"entity": entity, "expected": expected_json}))
                        .with_readback(readback.clone())
                        .with_assertion(json!({"readbackMatches": matched}))
                        .with_normalized(readback)
                        .with_readback_match(matched),
                )
            }
            _ => self.result(command, HcBackendResult::passed("fake realtime")),
        }
    }

    fn close(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record(StatefulCall::Lifecycle("close"));
        HcBackendResult::passed("fake close")
    }
}

struct ContractHcBackend {
    config: HcBackendConfig,
    calls: Arc<Mutex<Vec<String>>>,
    capabilities: Vec<HcCapability>,
}

impl ContractHcBackend {
    fn new(config: &HcBackendConfig, calls: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            config: config.clone(),
            calls,
            capabilities: vec![HcCapability::new("health", json!(true))],
        }
    }

    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

impl HcProtocolBackend for ContractHcBackend {
    fn implementation_id(&self) -> &str {
        &self.config.implementation
    }

    fn protocol_pack(&self) -> &str {
        &self.config.protocol_pack
    }

    fn capabilities(&self) -> &[HcCapability] {
        &self.capabilities
    }

    fn preflight(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record("lifecycle:preflight".into());
        HcBackendResult::passed("fake preflight")
            .with_evidence_tier(HcEvidenceTier::MockConformance)
    }

    fn open(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record("lifecycle:open".into());
        HcBackendResult::passed("fake open").with_evidence_tier(HcEvidenceTier::MockConformance)
    }

    fn execute(&mut self, command: &HcCommand, scope: &HcExecutionScope) -> HcBackendResult {
        let entity = match command {
            HcCommand::GetState { entity } => entity.as_str(),
            _ => "none",
        };
        self.record(format!(
            "execute:{}:{}:{}",
            command.display_name(),
            entity,
            scope.correlation_id()
        ));
        HcBackendResult::passed("fake execute")
            .with_evidence_tier(HcEvidenceTier::MockConformance)
            .with_evidence(HcEvidence::raw(
                "fake",
                json!({"command": command.display_name(), "entity": entity}),
            ))
            .with_normalized(json!({
                "command": command.display_name(),
                "entity": entity,
                "correlationId": scope.correlation_id()
            }))
    }

    fn close(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record("lifecycle:close".into());
        HcBackendResult::passed("fake close")
    }
}

struct CloseResultHcBackend {
    config: HcBackendConfig,
    close_result: HcBackendResult,
    capabilities: Vec<HcCapability>,
}

impl CloseResultHcBackend {
    fn new(config: &HcBackendConfig, close_result: HcBackendResult) -> Self {
        Self {
            config: config.clone(),
            close_result,
            capabilities: vec![HcCapability::new("health", json!(true))],
        }
    }
}

impl HcProtocolBackend for CloseResultHcBackend {
    fn implementation_id(&self) -> &str {
        &self.config.implementation
    }

    fn protocol_pack(&self) -> &str {
        &self.config.protocol_pack
    }

    fn capabilities(&self) -> &[HcCapability] {
        &self.capabilities
    }

    fn preflight(&mut self, _context: &TestContext) -> HcBackendResult {
        HcBackendResult::passed("fake preflight")
    }

    fn open(&mut self, _context: &TestContext) -> HcBackendResult {
        HcBackendResult::passed("fake open")
    }

    fn execute(&mut self, _command: &HcCommand, _scope: &HcExecutionScope) -> HcBackendResult {
        HcBackendResult::passed("fake execute")
    }

    fn close(&mut self, _context: &TestContext) -> HcBackendResult {
        self.close_result.clone()
    }
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
