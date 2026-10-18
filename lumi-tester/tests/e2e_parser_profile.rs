use lumi_tester::e2e::profile::{
    load_profile, validate_flow, E2eValidationError, EvidenceBoundaryError,
};
use lumi_tester::e2e::types::{E2eCapability, E2eCommand};
use lumi_tester::parser::types::{Platform, TestCommand};
use lumi_tester::parser::yaml::parse_yaml_content;
use std::fs;
use std::io::{self, Write};
use std::panic::catch_unwind;
use std::path::{Path, PathBuf};
use std::process::Command;

const VALID_PROFILE: &str = r#"
implementation: lumi_iot_v3_home_controller
protocolPack: lumi_iot_v3_home_controller
protocolVersion: "0.2.0"
packVersion: "0.1.0"
target:
  chip: unknown
  firmware: unknown
endpoint:
  baseUrl: "${LUMI_HC_BASE_URL}"
  websocketUrl: "${LUMI_HC_WS_URL}"
auth:
  mode: none
capabilities:
  - health
  - device_discovery
  - device_state
  - device_control
  - realtime_cell_readback
evidence:
  root: ./output
  maxRawBytes: 65536
  maxWebSocketFrames: 200
  redactFields: [dev_key, nwk_key, app_key, authorization, cookie, token]
retry:
  safeHttpMaxAttempts: 2
  controlMaxAttempts: 0
  websocketReconnectMaxAttempts: 2
"#;

fn valid_flow(profile: &str) -> String {
    format!(
        r#"
platform: lumi_e2e
journey: system_journey
profile: {profile}
defaultTimeout: 15000
tags: [smoke, device-control]
---
- hcHealth
- hcGetEntities
- hcGetState:
    entity: "123456789012345678"
- wsSubscribe:
    channels: ["cell:123456789012345678"]
- hcControl:
    entity: "123456789012345678"
    state:
      - idx: 0
        value: true
- wsExpect:
    entity: "123456789012345678"
    event:
      type: cell_state_changed
      state:
        - idx: 0
          value: true
- hcWaitState:
    entity: "123456789012345678"
    state:
      - idx: 0
        value: true
- wsSnapshot:
    entity: "123456789012345678"
"#,
    )
}

fn temp_flow_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("lumi-tester-{name}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn try_create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let output = Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &target.to_string_lossy(),
            ])
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "mklink /J failed (status {}): {}{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
            ))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (target, link);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory-link regression requires Windows",
        ))
    }
}

fn handle_directory_link_setup_error(error: io::Error) -> ! {
    if error.raw_os_error() == Some(1314) {
        eprintln!(
            "unsupported_environment: Windows directory junction prerequisite unavailable: {error}"
        );
        panic!(
            "unavailable_environment: Windows directory junction prerequisite unavailable: {error}"
        );
    }
    panic!("failed to replace flow path with a junction: {error}");
}

#[test]
fn junction_privilege_failure_is_not_silent() {
    let outcome = catch_unwind(|| {
        handle_directory_link_setup_error(io::Error::from_raw_os_error(1314));
    });
    assert!(
        outcome.is_err(),
        "junction setup privilege failure must not be reported as passed"
    );
}

#[test]
fn parses_valid_lumi_e2e_flow_with_stable_commands_and_requirements() {
    let dir = temp_flow_dir("valid");
    let profile_path = dir.join("profiles");
    fs::create_dir_all(&profile_path).unwrap();
    fs::write(profile_path.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = dir.join("device-control.yaml");
    fs::write(&flow_path, valid_flow("profiles/hc.yaml")).unwrap();

    let flow = parse_yaml_content(&valid_flow("profiles/hc.yaml"), &flow_path).unwrap();
    assert_eq!(flow.platform, Some(Platform::LumiE2e));
    assert_eq!(flow.journey.as_deref(), Some("system_journey"));
    assert_eq!(flow.profile.as_deref(), Some("profiles/hc.yaml"));
    assert_eq!(flow.default_timeout_ms, Some(15000));
    assert_eq!(flow.tags, vec!["smoke", "device-control"]);

    let names: Vec<_> = flow.commands.iter().map(TestCommand::display_name).collect();
    assert_eq!(
        names,
        vec![
            "hcHealth",
            "hcGetEntities",
            "hcGetState",
            "wsSubscribe",
            "hcControl",
            "wsExpect",
            "hcWaitState",
            "wsSnapshot"
        ]
    );
    assert!(matches!(flow.commands[0], TestCommand::E2e(E2eCommand::HcHealth)));

    let metadata = validate_flow(&flow_path, &flow).unwrap();
    assert_eq!(metadata.required_capabilities.len(), 5);
    assert_eq!(metadata.command_requirements.len(), flow.commands.len());
    assert_eq!(metadata.profile_ref, "profiles/hc.yaml");
    let control_requirements = metadata
        .command_requirements
        .iter()
        .find(|requirement| requirement.name == "hcControl")
        .unwrap();
    assert_eq!(
        control_requirements.capabilities,
        vec![
            E2eCapability::DeviceControl,
            E2eCapability::DeviceState,
            E2eCapability::RealtimeCellReadback,
        ]
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn rejects_missing_profile_without_fallback_to_checkout_path() {
    let dir = temp_flow_dir("missing-profile");
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("profiles/missing.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "missing_profile");
    assert!(error.to_string().contains("profiles/missing.yaml"));
    assert!(!error.to_string().contains("lumi-iot-v3"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn resolves_profile_relative_to_flow_file() {
    let dir = temp_flow_dir("relative-profile");
    let nested = dir.join("flows").join("system");
    fs::create_dir_all(nested.join("profiles")).unwrap();
    fs::write(nested.join("profiles/hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = nested.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("profiles/hc.yaml"), &flow_path).unwrap();

    let metadata = validate_flow(&flow_path, &flow).unwrap();
    assert_eq!(metadata.profile_path, nested.join("profiles/hc.yaml"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn requires_websocket_url_independently_from_base_url() {
    let dir = temp_flow_dir("missing-ws");
    let profile_path = dir.join("hc.yaml");
    fs::write(
        &profile_path,
        VALID_PROFILE.replace("  websocketUrl: \"${LUMI_HC_WS_URL}\"\n", ""),
    )
    .unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "missing_profile_field");
    assert!(error.to_string().contains("endpoint.websocketUrl"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn rejects_unsupported_protocol_profile_identity_or_version() {
    let dir = temp_flow_dir("unsupported-pack");
    let profile_path = dir.join("hc.yaml");
    let unsupported = VALID_PROFILE.replace("packVersion: \"0.1.0\"", "packVersion: \"9.9.9\"");
    fs::write(&profile_path, unsupported).unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "unsupported_profile_value");
    assert!(error.to_string().contains("packVersion"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn rejects_non_decimal_or_overflowing_entity_ids() {
    for invalid_id in ["-1", "1.5", "1e3", "18446744073709551616"] {
        let flow = valid_flow("hc.yaml").replace("123456789012345678", invalid_id);
        let error = parse_yaml_content(&flow, Path::new("flow.yaml")).unwrap_err();
        assert!(
            error.to_string().contains("entity")
                || error.to_string().contains("decimal")
                || error.to_string().contains("u64"),
            "unexpected error for {invalid_id}: {error}"
        );
    }
}

#[test]
fn rejects_evidence_root_escape() {
    let dir = temp_flow_dir("evidence-root");
    fs::write(
        dir.join("hc.yaml"),
        VALID_PROFILE.replace("root: ./output", "root: ../outside"),
    )
    .unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "invalid_evidence_root");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn resolves_missing_nested_evidence_leaf_inside_flow_directory() {
    let dir = temp_flow_dir("nested-evidence-root");
    let nested = dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        dir.join("hc.yaml"),
        VALID_PROFILE.replace("root: ./output", "root: nested/new-output"),
    )
    .unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    validate_flow(&flow_path, &flow).unwrap();
    assert!(!nested.join("new-output").exists());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn accepts_lexically_normalized_flow_directory_for_evidence_root() {
    let dir = temp_flow_dir("lexical-flow-root");
    fs::create_dir_all(dir.join("nested")).unwrap();
    fs::write(dir.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = dir
        .join("nested")
        .join("..")
        .join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    validate_flow(&flow_path, &flow).unwrap();

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn evidence_writer_uses_validated_root_for_raw_and_normalized_outputs() {
    let dir = temp_flow_dir("evidence-writer");
    fs::write(dir.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = dir.join("device-control.yaml");

    let loaded = load_profile(&flow_path, "hc.yaml").unwrap();
    let result = loaded.open_evidence_writer();

    #[cfg(windows)]
    {
        let writer = result.unwrap();
        let mut raw = writer.create_raw_file(Path::new("capture.json")).unwrap();
        raw.write_all(b"raw").unwrap();
        let mut normalized = writer
            .create_normalized_file(Path::new("state.json"))
            .unwrap();
        normalized.write_all(b"normalized").unwrap();

        let root = PathBuf::from(&loaded.profile.evidence.root);
        assert_eq!(fs::read(root.join("raw/capture.json")).unwrap(), b"raw");
        assert_eq!(
            fs::read(root.join("normalized/state.json")).unwrap(),
            b"normalized"
        );
        assert!(matches!(
            writer.create_raw_file(Path::new("C:\\outside.json")),
            Err(EvidenceBoundaryError::InvalidRelativePath { .. })
        ));
    }

    #[cfg(not(windows))]
    assert!(matches!(
        result,
        Err(EvidenceBoundaryError::UnsupportedPlatform)
    ));

    let _ = fs::remove_dir_all(dir);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires Windows directory junction support")]
fn evidence_writer_keeps_flow_root_authority_after_path_replacement() {
    let parent = temp_flow_dir("retained-flow-authority");
    let flow_dir = parent.join("flow");
    let moved_flow_dir = parent.join("moved-flow");
    let outside = temp_flow_dir("retained-flow-authority-outside");
    fs::create_dir_all(&flow_dir).unwrap();
    fs::write(flow_dir.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = flow_dir.join("device-control.yaml");

    let loaded = load_profile(&flow_path, "hc.yaml").unwrap();
    fs::rename(&flow_dir, &moved_flow_dir).unwrap();

    if let Err(error) = try_create_directory_link(&outside, &flow_dir) {
        if error.raw_os_error() == Some(1314) {
            let _ = fs::remove_dir_all(&moved_flow_dir);
            let _ = fs::remove_dir_all(&outside);
            let _ = fs::remove_dir_all(&parent);
            handle_directory_link_setup_error(error);
        }
        panic!("failed to replace flow path with a junction: {error}");
    }

    let writer = loaded.open_evidence_writer().unwrap();
    let mut raw = writer.create_raw_file(Path::new("captured.json")).unwrap();
    raw.write_all(b"retained").unwrap();

    assert_eq!(
        fs::read(moved_flow_dir.join("output/raw/captured.json")).unwrap(),
        b"retained"
    );
    assert!(!outside.join("output/raw/captured.json").exists());

    fs::remove_dir(&flow_dir).unwrap();
    let _ = fs::remove_dir_all(&moved_flow_dir);
    let _ = fs::remove_dir_all(&outside);
    let _ = fs::remove_dir_all(&parent);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires Windows directory junction support")]
fn evidence_writer_rejects_reparse_point_at_create_boundary() {
    let dir = temp_flow_dir("evidence-boundary-link");
    let outside = temp_flow_dir("evidence-boundary-outside");
    fs::write(dir.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = dir.join("device-control.yaml");
    let loaded = load_profile(&flow_path, "hc.yaml").unwrap();
    let writer = loaded.open_evidence_writer().unwrap();
    let raw_directory = PathBuf::from(&loaded.profile.evidence.root).join("raw");

    if let Err(error) = try_create_directory_link(&outside, &raw_directory) {
        panic!(
            "Windows directory-junction prerequisite unavailable; boundary assertion did not run: {error}"
        );
    }

    let error = writer.create_raw_file(Path::new("escaped.json")).unwrap_err();
    assert!(matches!(error, EvidenceBoundaryError::Io(_)));
    assert!(!outside.join("escaped.json").exists());

    fs::remove_dir(&raw_directory).unwrap();
    let _ = fs::remove_dir_all(dir);
    let _ = fs::remove_dir_all(outside);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires Windows directory junction support"
)]
fn rejects_existing_ancestor_directory_link_before_new_evidence_leaf() {
    let dir = temp_flow_dir("linked-evidence-root");
    let target = dir.join("target");
    fs::create_dir_all(&target).unwrap();
    let link = dir.join("linked");
    let link_result = try_create_directory_link(&target, &link);
    if let Err(error) = link_result {
        panic!(
            "Windows directory-junction prerequisite unavailable; security assertion did not run: {error}"
        );
    }

    fs::write(
        dir.join("hc.yaml"),
        VALID_PROFILE.replace("root: ./output", "root: linked/new-output"),
    )
    .unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "invalid_evidence_root");
    assert!(!target.join("new-output").exists());

    fs::remove_dir(&link).unwrap();
    let _ = fs::remove_dir_all(dir);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires Windows directory junction support"
)]
fn rejects_dangling_ancestor_directory_link_before_new_evidence_leaf() {
    let dir = temp_flow_dir("dangling-linked-evidence-root");
    let target = dir.join("target");
    fs::create_dir_all(&target).unwrap();
    let link = dir.join("linked");
    if let Err(error) = try_create_directory_link(&target, &link) {
        panic!(
            "Windows directory-junction prerequisite unavailable; security assertion did not run: {error}"
        );
    }
    fs::remove_dir_all(&target).unwrap();

    fs::write(
        dir.join("hc.yaml"),
        VALID_PROFILE.replace("root: ./output", "root: linked/new-output"),
    )
    .unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "invalid_evidence_root");
    assert!(!dir.join("new-output").exists());

    fs::remove_dir(&link).unwrap();
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn rejects_control_profile_without_state_or_realtime_readback() {
    let dir = temp_flow_dir("control-readback");
    fs::write(
        dir.join("hc.yaml"),
        VALID_PROFILE
            .replace("  - device_state\n", "")
            .replace("  - realtime_cell_readback\n", ""),
    )
    .unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow_yaml = r#"
platform: lumi_e2e
journey: system_journey
profile: hc.yaml
---
- hcControl:
    entity: "123"
    state:
      - idx: 0
        value: true
"#;
    let flow = parse_yaml_content(flow_yaml, &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "missing_capability");
    assert!(error.to_string().contains("readback"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn accepts_control_with_either_readback_capability_and_advertises_both() {
    let control_flow = r#"
platform: lumi_e2e
journey: system_journey
profile: hc.yaml
---
- hcControl:
    entity: "123"
    state:
      - idx: 0
        value: true
"#;

    for capability_to_remove in ["  - device_state\n", "  - realtime_cell_readback\n"] {
        let dir = temp_flow_dir("control-readback-alternative");
        let profile = VALID_PROFILE.replace(capability_to_remove, "");
        fs::write(dir.join("hc.yaml"), profile).unwrap();
        let flow_path = dir.join("device-control.yaml");
        let flow = parse_yaml_content(control_flow, &flow_path).unwrap();

        let metadata = validate_flow(&flow_path, &flow).unwrap();
        assert_eq!(
            metadata.command_requirements[0].capabilities,
            vec![
                E2eCapability::DeviceControl,
                E2eCapability::DeviceState,
                E2eCapability::RealtimeCellReadback,
            ]
        );

        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn rejects_invalid_endpoint_schemes() {
    for (field, replacement) in [
        ("baseUrl", "ftp://example.test"),
        ("websocketUrl", "http://example.test"),
    ] {
        let dir = temp_flow_dir("endpoint-scheme");
        let profile = VALID_PROFILE.replace(
            &format!("  {field}: \"${{LUMI_HC_{}}}\"\n", if field == "baseUrl" { "BASE_URL" } else { "WS_URL" }),
            &format!("  {field}: \"{replacement}\"\n"),
        );
        fs::write(dir.join("hc.yaml"), profile).unwrap();
        let flow_path = dir.join("device-control.yaml");
        let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

        let error = validate_flow(&flow_path, &flow).unwrap_err();
        assert_eq!(error.code(), "invalid_endpoint");
        assert!(error.to_string().contains(field));
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn requires_target_metadata_in_profile() {
    let dir = temp_flow_dir("target-metadata");
    let profile = VALID_PROFILE.replace(
        "target:\n  chip: unknown\n  firmware: unknown\n",
        "",
    );
    fs::write(dir.join("hc.yaml"), profile).unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "missing_profile_field");
    assert!(error.to_string().contains("target"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn preserves_journey_metadata_on_legacy_app_flow() {
    let flow_yaml = r#"
platform: android
journey: legacy_metadata
appId: com.example.app
---
- launchApp
"#;
    let flow = parse_yaml_content(flow_yaml, Path::new("legacy.yaml")).unwrap();
    validate_flow(Path::new("legacy.yaml"), &flow).unwrap();
}

#[test]
fn rejects_parameters_on_parameterless_e2e_commands() {
    let flow_yaml = r#"
platform: lumi_e2e
journey: system_journey
profile: hc.yaml
---
- hcHealth:
    unexpected: true
"#;
    let error = parse_yaml_content(flow_yaml, Path::new("flow.yaml")).unwrap_err();
    assert!(error.to_string().contains("must not accept parameters"));
}

#[test]
fn rejects_incomplete_evidence_redaction_fields() {
    let dir = temp_flow_dir("incomplete-redaction");
    let profile_path = dir.join("hc.yaml");
    let incomplete_profile =
        VALID_PROFILE.replace(", token", "").replace("authorization, cookie", "authorization");
    fs::write(
        &profile_path,
        incomplete_profile,
    )
    .unwrap();
    let flow_path = dir.join("device-control.yaml");
    let flow = parse_yaml_content(&valid_flow("hc.yaml"), &flow_path).unwrap();

    let error = validate_flow(&flow_path, &flow).unwrap_err();
    assert_eq!(error.code(), "missing_profile_field");
    assert!(
        error.to_string().contains("evidence.redactFields.cookie"),
        "unexpected validation error: {error}"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn journey_is_metadata_and_deferred_capability_is_still_rejected() {
    let dir = temp_flow_dir("deferred");
    fs::write(dir.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = dir.join("device-control.yaml");
    let journey_yaml = valid_flow("hc.yaml").replace("system_journey", "user_journey");
    let flow = parse_yaml_content(&journey_yaml, &flow_path).unwrap();
    validate_flow(&flow_path, &flow).unwrap();

    let no_journey_yaml = valid_flow("hc.yaml").replace("journey: system_journey\n", "");
    let flow = parse_yaml_content(&no_journey_yaml, &flow_path).unwrap();
    validate_flow(&flow_path, &flow).unwrap();

    let deferred = r#"
platform: lumi_e2e
journey: system_journey
profile: hc.yaml
---
- hcControl:
    entity: "1"
    capability: power
    value: true
"#;
    let parse_error = parse_yaml_content(deferred, &flow_path).unwrap_err();
    assert!(format!("{parse_error:#}").contains("deferred"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn rejects_malformed_explicit_idx_and_value() {
    let fractional = r#"
platform: lumi_e2e
journey: system_journey
profile: hc.yaml
---
- hcControl:
    entity: "1"
    state:
      - idx: 1.5
        value: true
"#;
    let error = parse_yaml_content(fractional, Path::new("flow.yaml")).unwrap_err();
    assert!(error.to_string().contains("idx"));

    let missing_value = r#"
platform: lumi_e2e
journey: system_journey
profile: hc.yaml
---
- hcControl:
    entity: "1"
    state:
      - idx: 0
"#;
    let error = parse_yaml_content(missing_value, Path::new("flow.yaml")).unwrap_err();
    assert!(error.to_string().contains("value"));
}

#[test]
fn preserves_legacy_yaml_without_e2e_fields() {
    let yaml = r#"
platform: android
appId: com.example.app
---
- launchApp
- tap: "Login"
"#;
    let flow = parse_yaml_content(yaml, Path::new("legacy.yaml")).unwrap();
    assert_eq!(flow.platform, Some(Platform::Android));
    assert!(flow.journey.is_none());
    assert!(flow.profile.is_none());
    assert_eq!(flow.commands[0].display_name(), "launchApp");
    assert_eq!(flow.commands[1].display_name(), "tapOn(text: \"Login\")");
}

#[test]
fn emits_structured_e2e_cli_json() {
    let dir = temp_flow_dir("cli-json");
    fs::write(dir.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = dir.join("device-control.yaml");
    fs::write(&flow_path, valid_flow("hc.yaml")).unwrap();

    let binary = env!("CARGO_BIN_EXE_lumi-tester");
    let validate = std::process::Command::new(binary)
        .args(["validate", flow_path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(validate.status.success(), "{}", String::from_utf8_lossy(&validate.stderr));
    let validate_json: serde_json::Value = serde_json::from_slice(&validate.stdout).unwrap();
    assert_eq!(validate_json["valid"], true);
    assert_eq!(validate_json["files"][0]["platform"], "lumi_e2e");
    assert_eq!(validate_json["files"][0]["journey"], "system_journey");
    assert_eq!(validate_json["files"][0]["profile"], "hc.yaml");
    assert_eq!(validate_json["files"][0]["commands"][0]["index"], 0);
    assert_eq!(validate_json["files"][0]["commands"][0]["name"], "hcHealth");
    assert!(validate_json["files"][0]["commands"][0]["capabilities"].is_array());

    let list = std::process::Command::new(binary)
        .args(["list", flow_path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(list.status.success(), "{}", String::from_utf8_lossy(&list.stderr));
    let list_json: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(list_json["files"][0]["commandCount"], 8);
    assert_eq!(list_json["files"][0]["commands"][7]["name"], "wsSnapshot");
    assert!(list_json["files"][0]["requiredCapabilities"].is_array());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn e2e_runner_reaches_existing_report_boundary_with_structured_observation() {
    let dir = temp_flow_dir("runner-report-boundary");
    fs::write(dir.join("hc.yaml"), VALID_PROFILE).unwrap();
    let flow_path = dir.join("device-control.yaml");
    fs::write(
        &flow_path,
        r#"
platform: lumi_e2e
journey: system_journey
profile: hc.yaml
---
- hcHealth
"#,
    )
    .unwrap();
    let output_dir = dir.join("out");

    let binary = env!("CARGO_BIN_EXE_lumi-tester");
    let output = std::process::Command::new(binary)
        .args([
            "run",
            flow_path.to_str().unwrap(),
            "--platform",
            "lumi_e2e",
            "--continue-on-failure",
            "--report",
            "--events-jsonl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("action"), "{combined}");
    assert!(combined.contains("readback"), "{combined}");
    assert!(combined.contains("assertion"), "{combined}");
    assert!(combined.contains("evidence"), "{combined}");
    assert!(output_dir.join("test-results.json").is_file());
    assert!(output_dir.join("events.jsonl").is_file());

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(output_dir.join("test-results.json")).unwrap())
            .unwrap();
    assert_eq!(report["flows"][0]["commands"][0]["commandName"], "hcHealth");
    assert_eq!(report["flows"][0]["status"]["type"], "failed");
    let command_error = report["flows"][0]["commands"][0]["status"]["error"]
        .as_str()
        .unwrap();
    assert!(command_error.contains("action"), "{command_error}");
    assert!(command_error.contains("readback"), "{command_error}");
    assert!(command_error.contains("assertion"), "{command_error}");
    assert!(command_error.contains("evidence"), "{command_error}");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn prints_human_list_errors_with_paths_and_reasons() {
    let dir = temp_flow_dir("list-human-errors");
    let flow_path = dir.join("device-control.yaml");
    fs::write(&flow_path, valid_flow("missing.yaml")).unwrap();

    let binary = env!("CARGO_BIN_EXE_lumi-tester");
    let output = std::process::Command::new(binary)
        .args(["list", flow_path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(flow_path.to_str().unwrap()), "{stdout}");
    assert!(stdout.contains("not found") || stdout.contains("missing"), "{stdout}");
    assert!(!output.status.success());

    let _ = fs::remove_dir_all(dir);
}

#[allow(dead_code)]
fn _assert_typed_error(error: E2eValidationError) {
    assert!(!error.code().is_empty());
}
