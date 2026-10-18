use lumi_tester::parser::normalize::normalize_flow;
use lumi_tester::parser::types::TargetId;
use lumi_tester::parser::validation::{validate_flow_graph, validate_flow_graph_with_limit};
use lumi_tester::parser::yaml::parse_yaml_content;
use std::path::Path;
use std::path::PathBuf;

#[test]
fn legacy_app_header_normalizes_to_one_app_component() {
    let source = Path::new("fixtures/legacy-app.yaml");
    let flow = parse_yaml_content(
        "platform: android\nappId: com.lumi.lumilife\n---\n- launchApp\n",
        source,
    )
    .unwrap();

    let normalized = normalize_flow(flow, source).unwrap();

    assert_eq!(normalized.components.len(), 1);
    let app = normalized.components.get(&TargetId::App).unwrap();
    assert_eq!(app.platform, "android");
    assert_eq!(app.app_id.as_deref(), Some("com.lumi.lumilife"));
    assert_eq!(app.profile, None);
}

#[test]
fn new_app_component_normalizes_to_the_same_app_adapter_shape() {
    let source = Path::new("fixtures/canonical-app.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.lumilife\n---\n- launchApp\n",
        source,
    )
    .unwrap();

    let normalized = normalize_flow(flow, source).unwrap();

    assert_eq!(normalized.components.len(), 1);
    let app = normalized.components.get(&TargetId::App).unwrap();
    assert_eq!(app.platform, "android");
    assert_eq!(app.app_id.as_deref(), Some("com.lumi.lumilife"));
    assert_eq!(app.profile, None);
}

#[test]
fn peer_components_normalize_to_app_gateway_cloud() {
    let source = Path::new("tests/fixtures/multi_target/parent.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.lumilife\n\
gateway:\n  platform: lumi_hc\n  profile: profiles/hc.yaml\n\
cloud:\n  platform: lumi_cloud\n  profile: profiles/cloud.yaml\n---\n[]\n",
        source,
    )
    .unwrap();

    let normalized = normalize_flow(flow, source).unwrap();

    assert_eq!(normalized.components.len(), 3);
    assert_eq!(normalized.components[&TargetId::App].platform, "android");
    assert_eq!(
        normalized.components[&TargetId::Gateway].platform,
        "lumi_hc"
    );
    assert_eq!(
        normalized.components[&TargetId::Cloud].platform,
        "lumi_cloud"
    );
    assert_eq!(
        normalized.components[&TargetId::Gateway].profile.as_deref(),
        Some(Path::new("tests/fixtures/multi_target/profiles/hc.yaml"))
    );
    assert_eq!(
        normalized.components[&TargetId::Cloud].profile.as_deref(),
        Some(Path::new("tests/fixtures/multi_target/profiles/cloud.yaml"))
    );
}

#[test]
fn journey_is_warning_metadata_and_not_adapter_selection() {
    let source = Path::new("fixtures/legacy-journey.yaml");
    let flow = parse_yaml_content(
        "platform: android\nappId: com.lumi.lumilife\njourney: system_journey\n---\n- launchApp\n",
        source,
    )
    .unwrap();

    let normalized = normalize_flow(flow, source).unwrap();

    assert!(normalized.components.contains_key(&TargetId::App));
    assert!(!normalized.components.contains_key(&TargetId::Gateway));
    assert_eq!(
        normalized.legacy_metadata.journey.as_deref(),
        Some("system_journey")
    );
    assert_eq!(normalized.legacy_metadata.diagnostics.len(), 1);
    assert_eq!(
        normalized.legacy_metadata.diagnostics[0].code,
        "DeprecatedJourney"
    );
    assert_eq!(
        normalized.legacy_metadata.diagnostics[0].severity,
        "warning"
    );
}

#[test]
fn legacy_lumi_e2e_profile_normalizes_to_gateway_hc() {
    let source = Path::new("tests/fixtures/multi_target/legacy-hc.yaml");
    let flow = parse_yaml_content(
        "platform: lumi_e2e\nprofile: profiles/hc.yaml\n---\n- hcHealth\n",
        source,
    )
    .unwrap();

    let normalized = normalize_flow(flow, source).unwrap();

    assert_eq!(normalized.components.len(), 1);
    let gateway = normalized.components.get(&TargetId::Gateway).unwrap();
    assert_eq!(gateway.platform, "lumi_hc");
    assert_eq!(
        gateway.profile.as_deref(),
        Some(Path::new("tests/fixtures/multi_target/profiles/hc.yaml"))
    );
    assert!(normalized
        .legacy_metadata
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DeprecatedLegacyProfile"));
}

#[test]
fn conflicting_legacy_app_fields_are_rejected() {
    let source = Path::new("fixtures/conflicting-app.yaml");
    let flow = parse_yaml_content(
        "platform: android\nappId: com.legacy.app\n\
app:\n  platform: ios\n  appId: com.canonical.app\n---\n- launchApp\n",
        source,
    )
    .unwrap();

    let error = normalize_flow(flow, source).unwrap_err();
    let error = error
        .downcast_ref::<lumi_tester::parser::normalize::NormalizationError>()
        .unwrap();

    assert_eq!(error.code(), "ConflictingLegacyAppDefinition");
}

#[test]
fn single_target_runflow_may_omit_target() {
    let source = Path::new("fixtures/single-target.yaml");
    let flow = parse_yaml_content(
        "platform: android\nappId: com.lumi.lumilife\n---\n\
- runFlow:\n    commands:\n      - launchApp\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();

    let validated = validate_flow_graph(&normalized).unwrap();

    assert_eq!(validated.routes.len(), 1);
    assert_eq!(validated.routes[0].effective_target, TargetId::App);
}

#[test]
fn multi_target_root_route_requires_target() {
    let source = Path::new("fixtures/missing-target.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.lumilife\n\
gateway:\n  platform: lumi_hc\n  profile: hc.yaml\n---\n\
- runFlow:\n    commands:\n      - launchApp\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();

    let error = validate_flow_graph(&normalized).unwrap_err();

    assert_eq!(validation_code(&error), "MissingTarget");
}

#[test]
fn multi_target_nested_runflow_requires_explicit_target() {
    let source = Path::new("fixtures/nested-missing-target.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.lumilife\n\
gateway:\n  platform: lumi_hc\n  profile: hc.yaml\n---\n\
- runFlow:\n    target: app\n    commands:\n      - runFlow:\n          commands:\n            - launchApp\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();

    let error = validate_flow_graph(&normalized).unwrap_err();

    assert_eq!(validation_code(&error), "MissingTarget");
}

#[test]
fn unknown_target_is_rejected() {
    let source = Path::new("fixtures/unknown-target.yaml");
    let error = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.lumilife\n---\n\
- runFlow:\n    target: satellite\n    commands:\n      - launchApp\n",
        source,
    )
    .unwrap_err();

    assert_eq!(validation_code(&error), "UnknownTarget");
}

#[test]
fn target_must_name_declared_component() {
    let source = Path::new("fixtures/undeclared-target.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.lumilife\n\
gateway:\n  platform: lumi_hc\n  profile: hc.yaml\n---\n\
- runFlow:\n    target: cloud\n    commands:\n      - launchApp\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();

    let error = validate_flow_graph(&normalized).unwrap_err();

    assert_eq!(validation_code(&error), "TargetRequired");
}

#[test]
fn child_header_cannot_rebind_parent_driver() {
    let source = fixture_dir().join("root-conflict.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.lumilife\n\
gateway:\n  platform: lumi_hc\n  profile: profiles/hc.yaml\n---\n\
- runFlow:\n    path: child-header-conflict.yaml\n    target: app\n",
        &source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, &source).unwrap();

    let error = validate_flow_graph(&normalized).unwrap_err();

    assert_eq!(validation_code(&error), "ChildHeaderDriverConflict");
}

#[test]
fn recursive_flow_cycle_is_rejected() {
    let temp = unique_temp_dir("cycle");
    std::fs::create_dir_all(&temp).unwrap();
    let root_path = temp.join("root.yaml");
    let child_path = temp.join("child.yaml");
    std::fs::write(
        &root_path,
        "platform: android\nappId: com.lumi.app\n---\n- runFlow: child.yaml\n",
    )
    .unwrap();
    std::fs::write(
        &child_path,
        "platform: android\nappId: com.lumi.app\n---\n- runFlow: root.yaml\n",
    )
    .unwrap();
    let flow = lumi_tester::parser::yaml::parse_test_file(&root_path).unwrap();
    let normalized = normalize_flow(flow, &root_path).unwrap();

    let error = validate_flow_graph_with_limit(&normalized, 8).unwrap_err();

    assert_eq!(validation_code(&error), "RecursiveFlowCycle");
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn recursive_flow_depth_is_bounded() {
    let temp = unique_temp_dir("depth");
    std::fs::create_dir_all(&temp).unwrap();
    for index in 0..4 {
        let body = if index == 3 {
            "platform: android\nappId: com.lumi.app\n---\n- launchApp\n".to_string()
        } else {
            format!(
                "platform: android\nappId: com.lumi.app\n---\n- runFlow: flow-{}.yaml\n",
                index + 1
            )
        };
        std::fs::write(temp.join(format!("flow-{index}.yaml")), body).unwrap();
    }
    let root_path = temp.join("flow-0.yaml");
    let flow = lumi_tester::parser::yaml::parse_test_file(&root_path).unwrap();
    let normalized = normalize_flow(flow, &root_path).unwrap();

    let error = validate_flow_graph_with_limit(&normalized, 2).unwrap_err();

    assert_eq!(validation_code(&error), "RecursiveFlowDepthExceeded");
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn unsupported_command_for_target_is_rejected() {
    let source = Path::new("fixtures/unsupported-command.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.app\n\
gateway:\n  platform: lumi_hc\n  profile: hc.yaml\n---\n\
- runFlow:\n    target: gateway\n    commands:\n      - launchApp\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();

    let error = validate_flow_graph(&normalized).unwrap_err();

    assert_eq!(validation_code(&error), "UnsupportedCommandForTarget");
}

#[test]
fn multi_target_root_command_requires_target_scope() {
    let source = Path::new("fixtures/root-command-without-target.yaml");
    let flow = parse_yaml_content(
        "app:\n  platform: android\n  appId: com.lumi.app\n\
gateway:\n  platform: lumi_hc\n  profile: hc.yaml\n---\n\
- launchApp\n",
        source,
    )
    .unwrap();
    let normalized = normalize_flow(flow, source).unwrap();

    let error = validate_flow_graph(&normalized).unwrap_err();

    assert_eq!(validation_code(&error), "MissingTarget");
}

#[test]
fn bundled_schema_describes_components_and_runflow_targets() {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join("lumi-test.schema.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(schema_path).unwrap()).unwrap();

    for component in ["app", "gateway", "cloud"] {
        assert_eq!(
            schema["properties"][component]["$ref"],
            serde_json::json!("#/$defs/component")
        );
    }
    assert_eq!(
        schema["$defs"]["runFlow"]["properties"]["target"]["enum"],
        serde_json::json!(["app", "gateway", "cloud"])
    );
    assert!(schema["properties"].get("journey").is_some());
}

#[test]
fn component_contract_errors_have_stable_codes() {
    let cases = [
        ("tags: []\n---\n- wait: 0\n", "MissingComponent"),
        (
            "app:\n  platform: android\n---\n- launchApp\n",
            "MissingAppId",
        ),
        (
            "gateway:\n  platform: lumi_hc\n---\n- hcHealth\n",
            "MissingProfile",
        ),
        (
            "gateway:\n  platform: android\n  profile: hc.yaml\n---\n- hcHealth\n",
            "TargetPlatformMismatch",
        ),
        (
            "app:\n  platform: amiga\n  appId: com.lumi.test\n---\n- launchApp\n",
            "UnsupportedPlatform",
        ),
        (
            "gateway:\n  platform: lumi_hc\n  profile: absent.yaml\n---\n- hcHealth\n",
            "InvalidProfile",
        ),
    ];

    for (yaml, expected) in cases {
        let source = Path::new("fixtures/component-contract.yaml");
        let flow = parse_yaml_content(yaml, source).unwrap();
        let normalized = normalize_flow(flow, source).unwrap();
        let error = validate_flow_graph(&normalized).unwrap_err();
        assert_eq!(validation_code(&error), expected, "YAML:\n{yaml}");
    }
}

#[test]
fn unknown_component_name_is_rejected() {
    let error = parse_yaml_content(
        "satellite:\n  platform: lumi_cloud\n---\n- wait: 0\n",
        Path::new("fixtures/unknown-component.yaml"),
    )
    .unwrap_err();

    assert_eq!(validation_code(&error), "UnknownComponent");
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("multi_target")
}

fn unique_temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lumi-multi-target-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn validation_code(error: &anyhow::Error) -> &'static str {
    error
        .downcast_ref::<lumi_tester::parser::validation::FlowValidationError>()
        .unwrap()
        .code()
}
