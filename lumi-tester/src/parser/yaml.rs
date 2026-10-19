use super::types::{
    AssertColorParams, AssertParams, AssertParamsInput, AssertVarParams, BuildGifParams,
    CaptureGifFrameParamsInput, ConditionalParams, ForEachParams, GenerateParams,
    HttpRequestParams, InputAtParams, LaunchAppParams, MatchParams, MockLocationParamsInput,
    Platform, RepeatParams, ReportParams, RetryParams, ScrollUntilVisibleInput,
    ScrollUntilVisibleParams, SetVarParams, TapAtParams, TapParams, TapParamsInput, TestCommand,
    TestFlow, WaitParams, WaitParamsInput, WhenParams,
};
use crate::hc::backends::v3::runtime::types::{deferred_command_message, parse_v3_command};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Parse a YAML test file into a TestFlow
pub fn parse_test_file(path: &Path) -> Result<TestFlow> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    parse_yaml_content(&content, path)
}

/// Parse YAML content into a TestFlow
pub fn parse_yaml_content(content: &str, _source_path: &Path) -> Result<TestFlow> {
    // 1. Check for custom "---" separator format
    if content.contains("---") {
        let parts: Vec<&str> = content.split("---").collect();
        let (header, commands_yaml) = if parts.len() >= 2 {
            (parts[0].trim(), parts[1..].join("---"))
        } else {
            ("", content.to_string()) // Should not happen if contains ---
        };

        // Parse header
        let mut flow = if !header.is_empty() {
            parse_header(header, _source_path)?
        } else {
            TestFlow {
                app: None,
                gateway: None,
                cloud: None,
                app_id: None,
                url: None,
                platform: Some(Platform::default()),
                journey: None,
                profile: None,
                env: None,
                vars: None,
                data: None,
                default_timeout_ms: None,
                commands: Vec::new(),
                tags: Vec::new(),
                speed: None,
                browser: None,
                close_when_finish: None,
                desktop_state: None,
                cameras: None,
                window_size: None,
                jig: None,
                skip: None,
            }
        };
        // Parse commands
        flow.commands = parse_commands(&commands_yaml)?;
        return Ok(flow);
    }

    // 2. Try parsing entire content as a list of commands (legacy simple format)
    let legacy_commands = match parse_commands(content) {
        Ok(commands) => {
            return Ok(TestFlow {
                app: None,
                gateway: None,
                cloud: None,
                app_id: None,
                url: None,
                platform: Some(Platform::default()),
                journey: None,
                profile: None,
                env: None,
                vars: None,
                data: None,
                default_timeout_ms: None,
                commands,
                tags: Vec::new(),
                speed: None,
                browser: None,
                close_when_finish: None,
                desktop_state: None,
                cameras: None,
                window_size: None,
                jig: None,
                skip: None,
            });
        }
        Err(error) => error,
    };

    // 3. Try parsing entire content as TestFlow struct (Map with commands field)
    // We need to deserialize into a Value first to check structure/fields manually or use TestFlow struct directly
    // But TestFlow struct uses `TestCommand` type which has custom logic?
    // Actually TestFlow derives Deserialize via serde, and TestCommand also does.
    // BUT TestCommand deserializer might not handle the short syntax if serde default is used.
    // My TestCommand parses from Map {key:val} correctly via normal serde.
    // The issue is `parse_command_value` logic handles string shortcuts like "- stopApp" which serde default enum deserializer might not (it expects String variant name).

    // To support `TestFlow` struct with shortcuts, we'd need custom deserializer for TestFlow or TestCommand.
    // For now, let's manually parse the map.

    let value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse YAML content")?;

    if matches!(&value, serde_yaml::Value::Sequence(_)) {
        return Err(legacy_commands);
    }

    if let serde_yaml::Value::Mapping(map) = value {
        reject_unknown_components(&map)?;
        let mut flow = TestFlow {
            app: None,
            gateway: None,
            cloud: None,
            app_id: None,
            url: None,
            platform: Some(Platform::default()),
            journey: None,
            profile: None,
            env: None,
            vars: None,
            data: None,
            default_timeout_ms: None,
            commands: Vec::new(),
            tags: Vec::new(),
            speed: None,
            browser: None,
            close_when_finish: None,
            desktop_state: None,
            cameras: None,
            window_size: None,
            jig: None,
            skip: None,
        };

        if let Some(val) = map.get(&serde_yaml::Value::String("data".to_string())) {
            if let Some(s) = val.as_str() {
                flow.data = Some(s.to_string());
            }
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("tags".to_string())) {
            if let Ok(tags) = serde_yaml::from_value(val.clone()) {
                flow.tags = tags;
            }
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("appId".to_string())) {
            if let Some(s) = val.as_str() {
                flow.app_id = Some(s.to_string());
            }
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("url".to_string())) {
            if let Some(s) = val.as_str() {
                flow.url = Some(s.to_string());
            }
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("platform".to_string())) {
            flow.platform = Some(
                serde_yaml::from_value(val.clone())
                    .context("Failed to parse header field: platform")?,
            );
        }

        let explicit_top_level_platform = map
            .contains_key(&serde_yaml::Value::String("platform".to_string()));
        flow.app = parse_component_from_map(&map, "app")?;
        flow.gateway = parse_component_from_map(&map, "gateway")?;
        flow.cloud = parse_component_from_map(&map, "cloud")?;

        if let Some(app) = flow.app.as_ref() {
            if flow.app_id.is_none() {
                flow.app_id = app.app_id.clone();
            }
            if !explicit_top_level_platform {
                // Keep an unsupported canonical component platform as raw component
                // data so normalization can return the stable UnsupportedPlatform
                // diagnostic instead of failing while mirroring legacy metadata.
                if let Ok(platform) =
                    serde_yaml::from_value(serde_yaml::Value::String(app.platform.clone()))
                {
                    flow.platform = Some(platform);
                }
            }
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("journey".to_string())) {
            flow.journey = Some(
                val.as_str()
                    .ok_or_else(|| anyhow::anyhow!("Failed to parse header field: journey"))?
                    .to_string(),
            );
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("profile".to_string())) {
            flow.profile = Some(
                val.as_str()
                    .ok_or_else(|| anyhow::anyhow!("Failed to parse header field: profile"))?
                    .to_string(),
            );
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("speed".to_string())) {
            if let Some(s) = val.as_str() {
                flow.speed = Some(s.to_string());
            }
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("browser".to_string())) {
            if let Some(s) = val.as_str() {
                flow.browser = Some(s.to_string());
            }
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("defaultTimeout".to_string())) {
            flow.default_timeout_ms = val.as_u64();
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("closeWhenFinish".to_string())) {
            flow.close_when_finish = val.as_bool();
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("desktopState".to_string())) {
            flow.desktop_state = Some(
                serde_yaml::from_value(val.clone())
                    .context("Failed to parse header field: desktopState")?,
            );
        }

        if let Some(val) = map
            .get(&serde_yaml::Value::String("cameras".to_string()))
            .or_else(|| map.get(&serde_yaml::Value::String("camera".to_string())))
        {
            flow.cameras = Some(
                serde_yaml::from_value(val.clone())
                    .context("Failed to parse camera/cameras header")?,
            );
        }

        let env_val = map.get(&serde_yaml::Value::String("env".to_string()));
        if let Some(val) = env_val {
            flow.env = serde_yaml::from_value(val.clone()).ok();
        }
        let vars_val = map
            .get(&serde_yaml::Value::String("vars".to_string()))
            .or_else(|| map.get(&serde_yaml::Value::String("var".to_string())));
        if let Some(val) = vars_val {
            flow.vars = serde_yaml::from_value(val.clone()).ok();
        }

        if let Some(val) = map.get(&serde_yaml::Value::String("commands".to_string())) {
            // Parse commands using our custom parser helper
            if let serde_yaml::Value::Sequence(seq) = val {
                let mut cmds = Vec::new();
                for (index, item) in seq.iter().enumerate() {
                    if let Some(cmd) = parse_command_at_index(item, index)? {
                        cmds.push(cmd);
                    }
                }
                flow.commands = cmds;
            }
        } else if let Some(val) = map.get(&serde_yaml::Value::String("steps".to_string())) {
            // Support 'steps' alias
            if let serde_yaml::Value::Sequence(seq) = val {
                let mut cmds = Vec::new();
                for (index, item) in seq.iter().enumerate() {
                    if let Some(cmd) = parse_command_at_index(item, index)? {
                        cmds.push(cmd);
                    }
                }
                flow.commands = cmds;
            }
        }
        return Ok(flow);
    }

    anyhow::bail!("Invalid YAML format: expected sequence or TestFlow map");
}

/// Parse the header section of a YAML test file
fn parse_header(header: &str, base_path: &Path) -> Result<TestFlow> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        #[serde(default)]
        app: Option<crate::parser::types::ComponentDeclaration>,
        #[serde(default)]
        gateway: Option<crate::parser::types::ComponentDeclaration>,
        #[serde(default)]
        cloud: Option<crate::parser::types::ComponentDeclaration>,
        #[serde(default)]
        app_id: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        platform: Option<Platform>,
        #[serde(default)]
        journey: Option<String>,
        #[serde(default)]
        profile: Option<String>,
        #[serde(default)]
        env: Option<serde_yaml::Value>,
        #[serde(default, alias = "var")]
        vars: Option<HashMap<String, String>>,
        #[serde(default)]
        data: Option<String>,
        #[serde(default, alias = "defaultTimeout")]
        default_timeout: Option<u64>,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default)]
        speed: Option<String>,
        #[serde(default)]
        browser: Option<String>,
        #[serde(default)]
        close_when_finish: Option<bool>,
        #[serde(default)]
        desktop_state: Option<crate::parser::types::DesktopState>,
        #[serde(default, alias = "camera")]
        cameras: Option<crate::parser::types::CamerasConfig>,
        #[serde(default, alias = "window", alias = "windowSize")]
        window_size: Option<crate::parser::types::WindowSizeConfig>,
        #[serde(default)]
        jig: Option<crate::parser::types::JigConfig>,
        #[serde(default, alias = "disabled", alias = "manual", alias = "ignore")]
        skip: Option<bool>,
    }

    let header_value: serde_yaml::Value =
        serde_yaml::from_str(header).context("Failed to parse YAML header")?;
    if let Some(map) = header_value.as_mapping() {
        reject_unknown_components(map)?;
    }
    let parsed: Header =
        serde_yaml::from_value(header_value).context("Failed to parse YAML header")?;

    // Process env
    let mut env_map = std::collections::HashMap::new();

    if let Some(env_val) = parsed.env {
        match env_val {
            serde_yaml::Value::Mapping(map) => {
                // Check if it's the special syntax: env: { file: "..." }
                if let Some(file_val) = map.get(&serde_yaml::Value::String("file".to_string())) {
                    if let Some(file_path_str) = file_val.as_str() {
                        // Resolve path relative to base_path
                        let env_path = if let Some(parent) = base_path.parent() {
                            parent.join(file_path_str)
                        } else {
                            Path::new(file_path_str).to_path_buf()
                        };

                        // Read .env file
                        let content = std::fs::read_to_string(&env_path).with_context(|| {
                            format!("Failed to read env file: {}", env_path.display())
                        })?;

                        env_map.extend(parse_env_file_content(&content));
                    }
                } else {
                    // Normal map syntax
                    for (k, v) in map {
                        if let (Some(k_str), Some(v_str)) = (k.as_str(), v.as_str()) {
                            env_map.insert(k_str.to_string(), v_str.to_string());
                        } else if let (Some(k_str), Some(v_num)) = (k.as_str(), v.as_u64()) {
                            env_map.insert(k_str.to_string(), v_num.to_string());
                        } else if let (Some(k_str), Some(v_bool)) = (k.as_str(), v.as_bool()) {
                            env_map.insert(k_str.to_string(), v_bool.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let env = if env_map.is_empty() {
        None
    } else {
        Some(env_map)
    };

    let mut flow = TestFlow {
        app: parsed.app,
        gateway: parsed.gateway,
        cloud: parsed.cloud,
        app_id: parsed.app_id,
        url: parsed.url,
        platform: parsed.platform,
        journey: parsed.journey,
        profile: parsed.profile,
        env,
        vars: parsed.vars,
        data: parsed.data,
        default_timeout_ms: parsed.default_timeout,
        commands: Vec::new(),
        tags: parsed.tags,
        speed: parsed.speed,
        browser: parsed.browser,
        close_when_finish: parsed.close_when_finish,
        desktop_state: parsed.desktop_state,
        cameras: parsed.cameras,
        window_size: parsed.window_size,
        jig: parsed.jig,
        skip: parsed.skip,
    };
    if let Some(app) = flow.app.as_ref() {
        if flow.app_id.is_none() {
            flow.app_id = app.app_id.clone();
        }
        if flow.platform.is_none() {
            // Do not make an unsupported canonical component platform a parser
            // error. The normalized component validator owns that diagnostic.
            if let Ok(platform) =
                serde_yaml::from_value(serde_yaml::Value::String(app.platform.clone()))
            {
                flow.platform = Some(platform);
            }
        }
    }
    Ok(flow)
}

fn reject_unknown_components(map: &serde_yaml::Mapping) -> Result<()> {
    for (key, value) in map {
        let (Some(key), Some(component)) = (key.as_str(), value.as_mapping()) else {
            continue;
        };
        let has_platform = component.contains_key(serde_yaml::Value::String("platform".into()));
        if has_platform && !matches!(key, "app" | "gateway" | "cloud") {
            return Err(
                crate::parser::validation::FlowValidationError::UnknownComponent {
                    component: key.to_string(),
                }
                .into(),
            );
        }
    }
    Ok(())
}

fn parse_component_from_map(
    map: &serde_yaml::Mapping,
    key: &str,
) -> Result<Option<crate::parser::types::ComponentDeclaration>> {
    map.get(&serde_yaml::Value::String(key.to_string()))
        .cloned()
        .map(serde_yaml::from_value)
        .transpose()
        .with_context(|| format!("Failed to parse header field: {key}"))
}

fn parse_env_file_content(content: &str) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    for raw_line in content.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            continue;
        }
        env.insert(key.to_string(), parse_env_value(raw_value));
    }
    env
}

fn parse_env_value(raw: &str) -> String {
    let value = raw.trim();
    if value.len() >= 2 {
        let first = value.as_bytes()[0] as char;
        let last = value.as_bytes()[value.len() - 1] as char;
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    if let Some(idx) = value.find(" #") {
        value[..idx].trim_end().to_string()
    } else {
        value.to_string()
    }
}

/// Parse the commands section of a YAML test file
fn parse_commands(yaml: &str) -> Result<Vec<TestCommand>> {
    let yaml = yaml.trim();
    if yaml.is_empty() {
        return Ok(Vec::new());
    }

    // Parse as a list of YAML values
    let values: Vec<serde_yaml::Value> =
        serde_yaml::from_str(yaml).context("Failed to parse YAML commands")?;

    let mut commands = Vec::new();

    for (index, value) in values.into_iter().enumerate() {
        if let Some(cmd) = parse_command_at_index(&value, index)? {
            commands.push(cmd);
        }
    }

    Ok(commands)
}

/// Parse a list of commands from a YAML value
pub fn parse_commands_from_value(value: &serde_yaml::Value) -> Result<Vec<TestCommand>> {
    match value {
        serde_yaml::Value::Sequence(seq) => {
            let mut cmds = Vec::new();
            for (index, item) in seq.iter().enumerate() {
                if let Some(cmd) = parse_command_at_index(item, index)? {
                    cmds.push(cmd);
                }
            }
            Ok(cmds)
        }
        _ => {
            if let Some(cmd) = parse_command_value(value)? {
                Ok(vec![cmd])
            } else {
                Ok(Vec::new())
            }
        }
    }
}

fn parse_command_at_index(value: &serde_yaml::Value, index: usize) -> Result<Option<TestCommand>> {
    match parse_command_value(value) {
        Ok(command) => Ok(command),
        Err(error) => {
            let detail = error.to_string();
            Err(error.context(format!(
                "Failed to parse command #{}: {detail}",
                index + 1
            )))
        }
    }
}

/// Parse a single command from a YAML value
pub fn parse_command_value(value: &serde_yaml::Value) -> Result<Option<TestCommand>> {
    match value {
        // Simple string command like "- stopApp" or "- hideKeyboard"
        serde_yaml::Value::String(s) => parse_simple_command(s)?
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("Unknown command: {}", s)),

        // Command with parameters like "- tapOn:\n    text: 'Login'"
        serde_yaml::Value::Mapping(map) => {
            // Check for flattened match with cases:
            // - match: "${ROLE}"
            //   cases: { ... }
            let match_key = serde_yaml::Value::String("match".to_string());
            let cases_key = serde_yaml::Value::String("cases".to_string());
            if let Some(match_val) = map.get(&match_key) {
                if let Some(cases_val) = map.get(&cases_key) {
                    let value_str = match match_val {
                        serde_yaml::Value::String(s) => s.clone(),
                        serde_yaml::Value::Number(n) => n.to_string(),
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        _ => serde_json::to_string(match_val).unwrap_or_default(),
                    };
                    let cases_map = cases_val
                        .as_mapping()
                        .ok_or_else(|| anyhow::anyhow!("match 'cases' must be a mapping"))?;
                    let mut cases = HashMap::new();
                    for (k, v) in cases_map {
                        let case_key = match k {
                            serde_yaml::Value::String(s) => s.clone(),
                            serde_yaml::Value::Number(n) => n.to_string(),
                            serde_yaml::Value::Bool(b) => b.to_string(),
                            _ => anyhow::bail!("case key must be string, number, or bool"),
                        };
                        let cmds = parse_commands_from_value(v)?;
                        cases.insert(case_key, cmds);
                    }
                    let default_cmds = if let Some(def_val) =
                        map.get(&serde_yaml::Value::String("default".to_string()))
                    {
                        Some(parse_commands_from_value(def_val)?)
                    } else {
                        None
                    };
                    return Ok(Some(TestCommand::Match(MatchParams {
                        value: value_str,
                        cases,
                        default: default_cmds,
                    })));
                }
            }

            // Check for modifier inline 'when' with other command keys:
            // - tap: "Accept"
            //   when: { visible: "Accept" }
            let when_key = serde_yaml::Value::String("when".to_string());
            if let Some(when_val) = map.get(&when_key) {
                if map.len() > 1 {
                    let mut remaining_map = map.clone();
                    remaining_map.remove(&when_key);
                    let inner_cmd =
                        parse_command_value(&serde_yaml::Value::Mapping(remaining_map))?
                            .ok_or_else(|| {
                                anyhow::anyhow!("Failed to parse command with 'when' modifier")
                            })?;
                    let condition_json: serde_json::Value =
                        serde_yaml::from_value(when_val.clone())?;
                    return Ok(Some(TestCommand::When(WhenParams {
                        condition: condition_json,
                        command: Box::new(inner_cmd),
                    })));
                }
            }

            if map.len() != 1 {
                anyhow::bail!("Invalid command format: expected single key mapping");
            }

            let (key, params) = map.iter().next().unwrap();
            let cmd_name = key
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Command name must be a string"))?;

            if let Some(message) = deferred_command_message(cmd_name) {
                anyhow::bail!("{message}");
            }

            if let Some(command) = parse_v3_command(cmd_name, params)? {
                return Ok(Some(TestCommand::V3(command)));
            }

            // If params is null or empty map, try simple command first
            if params.is_null() || (params.is_mapping() && params.as_mapping().unwrap().is_empty())
            {
                if let Some(cmd) = parse_simple_command(cmd_name)? {
                    return Ok(Some(cmd));
                }
            }

            parse_command_with_params(cmd_name, params)?
                .map(Some)
                .ok_or_else(|| anyhow::anyhow!("Unknown command: {}", cmd_name))
        }

        _ => {
            anyhow::bail!("Invalid command format: {:?}", value);
        }
    }
}

/// Parse a simple command without parameters
fn parse_simple_command(name: &str) -> Result<Option<TestCommand>> {
    if let Some(message) = deferred_command_message(name) {
        anyhow::bail!("{message}");
    }
    if let Some(command) = parse_v3_command(name, &serde_yaml::Value::Null)? {
        return Ok(Some(TestCommand::V3(command)));
    }

    let cmd = match name {
        "launchApp" | "open" => TestCommand::LaunchApp(None),
        "stopApp" | "stop" => TestCommand::StopApp,
        "hideKeyboard" | "hideKbd" => TestCommand::HideKeyboard,
        "swipeLeft" => TestCommand::SwipeLeft,
        "swipeRight" => TestCommand::SwipeRight,
        "swipeUp" => TestCommand::SwipeUp,
        "swipeDown" => TestCommand::SwipeDown,
        "waitForAnimationToEnd" => TestCommand::WaitForAnimationToEnd,
        "stopRecording" | "stopRecord" => TestCommand::StopRecording,
        "back" => TestCommand::Back,
        "pressHome" | "home" => TestCommand::PressHome,
        "eraseText" | "clear" => TestCommand::EraseText(None),
        "stopMockLocation" | "stopGps" => TestCommand::StopMockLocation,
        "stopMedia" => TestCommand::StopMedia,
        "stopAudioCapture" => TestCommand::StopAudioCapture,
        "pasteText" => TestCommand::PasteText,
        "inputRandomEmail" => TestCommand::InputRandomEmail,
        "inputRandomNumber" | "inputRandomPhoneNumber" => TestCommand::InputRandomNumber(None),
        "inputRandomPersonName" => TestCommand::InputRandomPersonName,
        "inputRandomText" => TestCommand::InputRandomText(None),
        "airplaneMode" | "toggleAirplaneMode" => TestCommand::ToggleAirplaneMode,
        "openNotifications" => TestCommand::OpenNotifications,
        "openQuickSettings" => TestCommand::OpenQuickSettings,
        "lockDevice" => TestCommand::LockDevice,
        "unlockDevice" => TestCommand::UnlockDevice,
        "hwDisconnect" => TestCommand::HwDisconnect,
        "hwPowerOffAll" => TestCommand::HwPowerOffAll,
        "hwReleaseAll" => TestCommand::HwReleaseAll,
        "hwReadSensorLight" => TestCommand::HwReadSensorLight(None),
        "hwSaveCalibration" => TestCommand::HwSaveCalibration,
        "hwLoadCalibration" => TestCommand::HwLoadCalibration,
        "hwResetCalibration" => TestCommand::HwResetCalibration,
        "hwEraseCalibration" => TestCommand::HwEraseCalibration,
        "hwSafeState" => TestCommand::HwSafeState,
        "hwDiagnostics" => TestCommand::HwDiagnostics,
        "click" => TestCommand::Click(crate::parser::types::ClickParams {
            selector: None,
            text: None,
        }),
        _ => return Ok(None),
    };

    Ok(Some(cmd))
}

/// Parse a command with parameters
fn parse_command_with_params(
    name: &str,
    params: &serde_yaml::Value,
) -> Result<Option<TestCommand>> {
    let cmd = match name {
        "launchApp" | "open" => {
            if params.is_string() {
                let s = params.as_str().unwrap().to_string();
                TestCommand::LaunchApp(Some(crate::parser::types::LaunchAppParamsInput::String(s)))
            } else {
                let p: LaunchAppParams =
                    serde_yaml::from_value(params.clone()).unwrap_or(LaunchAppParams {
                        clear_state: false,
                        clear_keychain: false,
                        stop_app: None,
                        permissions: None,
                        app_id: None,
                        label: None,
                    });
                TestCommand::LaunchApp(Some(crate::parser::types::LaunchAppParamsInput::Struct(p)))
            }
        }

        "find" => {
            let p: crate::parser::types::FindParams = serde_yaml::from_value(params.clone())?;
            TestCommand::Find(p)
        }

        "tapOn" | "tap" => {
            let p: TapParamsInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: TapParams = serde_yaml::from_value(params.clone())?;
                TapParamsInput::Struct(inner)
            };
            TestCommand::TapOn(p)
        }

        "longPressOn" | "longPress" => {
            let p: TapParamsInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: TapParams = serde_yaml::from_value(params.clone())?;
                TapParamsInput::Struct(inner)
            };
            TestCommand::LongPressOn(p)
        }

        "doubleTapOn" | "doubleTap" => {
            let p: TapParamsInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: TapParams = serde_yaml::from_value(params.clone())?;
                TapParamsInput::Struct(inner)
            };
            TestCommand::DoubleTapOn(p)
        }

        "inputText" | "write" | "type" => {
            // "type" can be InputText(String) or Type(TypeParams)
            // Try as simple string first (InputText)
            if let Ok(text) = serde_yaml::from_value::<String>(params.clone()) {
                TestCommand::InputText(crate::parser::types::InputTextParamsInput::String(text))
            } else if let Ok(p) =
                serde_yaml::from_value::<crate::parser::types::InputTextParams>(params.clone())
            {
                // If it parses as InputTextParams (struct with text/unicode), use it
                TestCommand::InputText(crate::parser::types::InputTextParamsInput::Struct(p))
            } else if let Ok(p) =
                serde_yaml::from_value::<crate::parser::types::TypeParams>(params.clone())
            {
                // If it parses as TypeParams (struct), use Type command
                TestCommand::Type(p)
            } else {
                // Fallback to error from string parsing
                let text: String = serde_yaml::from_value(params.clone())?;
                TestCommand::InputText(crate::parser::types::InputTextParamsInput::String(text))
            }
        }

        "eraseText" => {
            let p = serde_yaml::from_value(params.clone()).ok();
            TestCommand::EraseText(p)
        }

        "swipe" => {
            let p = serde_yaml::from_value(params.clone()).ok();
            TestCommand::ManualScroll(p)
        }

        "drag" | "dragFromTo" => {
            let p: crate::parser::types::DragParams = serde_yaml::from_value(params.clone())?;
            TestCommand::Drag(p)
        }

        "pinch" => {
            let p: crate::parser::types::PinchParams = serde_yaml::from_value(params.clone())?;
            TestCommand::Pinch(p)
        }

        "shove" => {
            let p: crate::parser::types::ShoveParams = serde_yaml::from_value(params.clone())?;
            TestCommand::Shove(p)
        }

        "scrollUntilVisible" | "scrollTo" => {
            let p: ScrollUntilVisibleInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: ScrollUntilVisibleParams = serde_yaml::from_value(params.clone())?;
                ScrollUntilVisibleInput::Struct(inner)
            };
            TestCommand::ScrollUntilVisible(p)
        }

        "assertVisible" | "see" => {
            let p: AssertParamsInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: AssertParams = serde_yaml::from_value(params.clone())?;
                AssertParamsInput::Struct(inner)
            };
            TestCommand::AssertVisible(p)
        }

        "assertNotVisible" | "notSee" => {
            let p: AssertParamsInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: AssertParams = serde_yaml::from_value(params.clone())?;
                AssertParamsInput::Struct(inner)
            };
            TestCommand::AssertNotVisible(p)
        }

        "waitUntilVisible" | "waitSee" => {
            let p: AssertParamsInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: AssertParams = serde_yaml::from_value(params.clone())?;
                AssertParamsInput::Struct(inner)
            };
            TestCommand::WaitUntilVisible(p)
        }

        "waitUntilNotVisible" | "waitNotSee" => {
            let p: AssertParamsInput = if params.is_string() {
                serde_yaml::from_value(params.clone())?
            } else {
                let inner: AssertParams = serde_yaml::from_value(params.clone())?;
                AssertParamsInput::Struct(inner)
            };
            TestCommand::WaitUntilNotVisible(p)
        }

        "wait" | "await" => {
            let p_input = if let Some(ms) = params.as_u64() {
                WaitParamsInput::Number(ms)
            } else {
                let inner: WaitParams = serde_yaml::from_value(params.clone())?;
                WaitParamsInput::Struct(inner)
            };
            TestCommand::Wait(p_input)
        }

        "repeat" => {
            let map = params
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("repeat requires a mapping"))?;
            let times = map
                .get(&serde_yaml::Value::String("times".to_string()))
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);

            let while_condition = map
                .get(&serde_yaml::Value::String("while".to_string()))
                .and_then(|v| serde_yaml::from_value(v.clone()).ok());

            let cmds_val = map
                .get(&serde_yaml::Value::String("commands".to_string()))
                .ok_or_else(|| anyhow::anyhow!("repeat requires commands"))?;
            let commands = parse_commands_from_value(cmds_val)?;
            TestCommand::Repeat(RepeatParams {
                times,
                while_condition,
                commands,
            })
        }

        "retry" => {
            let map = params
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("retry requires a mapping"))?;
            let max_retries = map
                .get(&serde_yaml::Value::String("maxRetries".to_string()))
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u32;
            let cmds_val = map
                .get(&serde_yaml::Value::String("commands".to_string()))
                .ok_or_else(|| anyhow::anyhow!("retry requires commands"))?;
            let commands = parse_commands_from_value(cmds_val)?;
            TestCommand::Retry(RetryParams {
                max_retries,
                commands,
            })
        }

        "forEach" | "foreach" | "for_each" => {
            let map = params
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("forEach requires a mapping"))?;
            let item = map
                .get(&serde_yaml::Value::String("item".to_string()))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("forEach requires 'item' name"))?
                .to_string();
            let in_val = map
                .get(&serde_yaml::Value::String("in".to_string()))
                .or_else(|| map.get(&serde_yaml::Value::String("items".to_string())))
                .ok_or_else(|| anyhow::anyhow!("forEach requires 'in' collection"))?
                .clone();
            let cmds_val = map
                .get(&serde_yaml::Value::String("commands".to_string()))
                .or_else(|| map.get(&serde_yaml::Value::String("do".to_string())))
                .ok_or_else(|| anyhow::anyhow!("forEach requires 'commands' or 'do'"))?;
            let commands = parse_commands_from_value(cmds_val)?;
            TestCommand::ForEach(ForEachParams {
                item,
                r#in: in_val,
                commands,
            })
        }

        "match" => {
            let map = params
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("match requires a mapping"))?;
            let value = map
                .get(&serde_yaml::Value::String("value".to_string()))
                .and_then(|v| match v {
                    serde_yaml::Value::String(s) => Some(s.clone()),
                    serde_yaml::Value::Number(n) => Some(n.to_string()),
                    serde_yaml::Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .unwrap_or_default();
            let cases_map = map
                .get(&serde_yaml::Value::String("cases".to_string()))
                .and_then(|v| v.as_mapping())
                .ok_or_else(|| anyhow::anyhow!("match requires 'cases' mapping"))?;
            let mut cases = HashMap::new();
            for (k, v) in cases_map {
                let case_key = match k {
                    serde_yaml::Value::String(s) => s.clone(),
                    serde_yaml::Value::Number(n) => n.to_string(),
                    serde_yaml::Value::Bool(b) => b.to_string(),
                    _ => anyhow::bail!("case key must be string, number, or bool"),
                };
                let cmds = parse_commands_from_value(v)?;
                cases.insert(case_key, cmds);
            }
            let default_cmds =
                if let Some(def_val) = map.get(&serde_yaml::Value::String("default".to_string())) {
                    Some(parse_commands_from_value(def_val)?)
                } else {
                    None
                };
            TestCommand::Match(MatchParams {
                value,
                cases,
                default: default_cmds,
            })
        }

        "when" => {
            let map = params
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("when requires a mapping"))?;
            let cond_val = map
                .get(&serde_yaml::Value::String("condition".to_string()))
                .or_else(|| map.get(&serde_yaml::Value::String("if".to_string())))
                .ok_or_else(|| anyhow::anyhow!("when requires 'condition'"))?;
            let condition_json: serde_json::Value = serde_yaml::from_value(cond_val.clone())?;
            let cmd_val = map
                .get(&serde_yaml::Value::String("command".to_string()))
                .or_else(|| map.get(&serde_yaml::Value::String("then".to_string())))
                .or_else(|| map.get(&serde_yaml::Value::String("commands".to_string())))
                .ok_or_else(|| anyhow::anyhow!("when requires 'command' or 'then'"))?;
            let cmds = parse_commands_from_value(cmd_val)?;
            let inner_cmd = if cmds.len() == 1 {
                cmds.into_iter().next().unwrap()
            } else {
                TestCommand::RunFlow(super::types::RunFlowParamsInput::Struct(
                    super::types::RunFlowParams {
                        path: None,
                        target: None,
                        vars: None,
                        commands: Some(cmds),
                        when: None,
                        label: Some("When block".to_string()),
                        optional: None,
                    },
                ))
            };
            TestCommand::When(WhenParams {
                condition: condition_json,
                command: Box::new(inner_cmd),
            })
        }

        "runFlow" => {
            use super::types::{RunFlowParams, RunFlowParamsInput};
            match params {
                serde_yaml::Value::String(s) => {
                    TestCommand::RunFlow(RunFlowParamsInput::String(s.clone()))
                }
                serde_yaml::Value::Mapping(map) => {
                    let path = map
                        .get(&serde_yaml::Value::String("path".to_string()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let target = map
                        .get(&serde_yaml::Value::String("target".to_string()))
                        .map(|value| {
                            let raw = value.as_str().ok_or_else(|| {
                                crate::parser::validation::FlowValidationError::UnknownTarget {
                                    target: value.as_str().unwrap_or("<non-string>").to_string(),
                                }
                            })?;
                            crate::parser::types::TargetId::parse(raw).ok_or_else(|| {
                                crate::parser::validation::FlowValidationError::UnknownTarget {
                                    target: raw.to_string(),
                                }
                            })
                        })
                        .transpose()?;

                    let vars = map
                        .get(&serde_yaml::Value::String("vars".to_string()))
                        .or_else(|| map.get(&serde_yaml::Value::String("env".to_string())))
                        .and_then(|v| serde_yaml::from_value(v.clone()).ok());

                    let commands = if let Some(cmds_val) =
                        map.get(&serde_yaml::Value::String("commands".to_string()))
                    {
                        Some(parse_commands_from_value(cmds_val)?)
                    } else {
                        None
                    };

                    let when = map
                        .get(&serde_yaml::Value::String("when".to_string()))
                        .and_then(|v| serde_yaml::from_value(v.clone()).ok());

                    let label = map
                        .get(&serde_yaml::Value::String("label".to_string()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let optional = map
                        .get(&serde_yaml::Value::String("optional".to_string()))
                        .and_then(|v| v.as_bool());

                    TestCommand::RunFlow(RunFlowParamsInput::Struct(RunFlowParams {
                        path,
                        target,
                        vars,
                        commands,
                        when,
                        label,
                        optional,
                    }))
                }
                _ => anyhow::bail!("Invalid runFlow params"),
            }
        }

        "sendLarkMessage" | "sendlarkmessage" | "lark" => {
            TestCommand::SendLarkMessage(serde_yaml::from_value(params.clone())?)
        }

        "tapAt" => {
            let p: TapAtParams = serde_yaml::from_value(params.clone())?;
            TestCommand::TapAt(p)
        }

        "inputAt" => {
            let p: InputAtParams = serde_yaml::from_value(params.clone())?;
            TestCommand::InputAt(p)
        }

        "setVar" => {
            let p: SetVarParams = serde_yaml::from_value(params.clone())?;
            TestCommand::SetVar(p)
        }

        "assertVar" => {
            let p: AssertVarParams = serde_yaml::from_value(params.clone())?;
            TestCommand::AssertVar(p)
        }

        "screenshot" | "takeScreenshot" | "takeSnapshot" | "snapshot" => {
            let p: crate::parser::types::ScreenshotParamsInput =
                serde_yaml::from_value(params.clone())?;
            TestCommand::TakeScreenshot(p)
        }

        "assertScreenshot" => {
            let path = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::AssertScreenshot(path)
        }

        "startRecording" => {
            let p: crate::parser::types::RecordingParamsInput =
                serde_yaml::from_value(params.clone())?;
            TestCommand::StartRecording(p)
        }

        "exportReport" => {
            let p: ReportParams = serde_yaml::from_value(params.clone())?;
            TestCommand::ExportReport(p)
        }

        "generate" => {
            let p: GenerateParams = serde_yaml::from_value(params.clone())?;
            TestCommand::Generate(p)
        }

        "httpRequest" => {
            let p: HttpRequestParams = serde_yaml::from_value(params.clone())?;
            TestCommand::HttpRequest(p)
        }

        "runScript" => {
            let p: crate::parser::types::RunScriptParamsInput =
                serde_yaml::from_value(params.clone())?;
            TestCommand::RunScript(p)
        }

        "conditional" => {
            let p: ConditionalParams = serde_yaml::from_value(params.clone())?;
            TestCommand::Conditional(p)
        }

        "rightClick" | "contextClick" => {
            let p: TapParams = if params.is_string() {
                let text = params.as_str().unwrap().to_string();
                TapParams {
                    text: Some(text),
                    ..Default::default()
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::RightClick(p)
        }

        "back" => TestCommand::Back,
        "stopRecording" | "stopRecord" => TestCommand::StopRecording,
        "stopApp" | "stop" => TestCommand::StopApp,
        "pressHome" | "home" => TestCommand::PressHome,
        "hideKeyboard" | "hideKbd" => TestCommand::HideKeyboard,

        "mockLocation" | "gps" => {
            let p: MockLocationParamsInput = serde_yaml::from_value(params.clone())?;
            TestCommand::MockLocation(p)
        }
        "stopMockLocation" | "stopGps" => TestCommand::StopMockLocation,

        "assertColor" | "checkColor" => {
            let p: AssertColorParams = serde_yaml::from_value(params.clone())?;
            TestCommand::AssertColor(p)
        }

        // GIF Recording
        "captureGifFrame" | "captureFrame" => {
            let p: CaptureGifFrameParamsInput = serde_yaml::from_value(params.clone())?;
            TestCommand::CaptureGifFrame(p)
        }

        "buildGif" | "createGif" => {
            let p: BuildGifParams = serde_yaml::from_value(params.clone())?;
            TestCommand::BuildGif(p)
        }

        "startGifCapture" => {
            let p: crate::parser::types::StartGifCaptureParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::StartGifCapture(p)
        }

        "stopGifCapture" => {
            let p: crate::parser::types::StopGifCaptureParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::StopGifCapture(p)
        }

        "rotate" | "rotateScreen" => {
            let p: crate::parser::types::RotationParamsInput =
                serde_yaml::from_value(params.clone())?;
            TestCommand::RotateScreen(p)
        }

        "pressKey" => {
            let p: crate::parser::types::PressKeyParamsInput =
                serde_yaml::from_value(params.clone())?;
            TestCommand::PressKey(p)
        }

        "press" => {
            let p: crate::parser::types::PressKeyParamsInput =
                serde_yaml::from_value(params.clone())?;
            TestCommand::PressKey(p)
        }

        "pushFile" => {
            let p: crate::parser::types::FileTransferParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::PushFile(p)
        }

        "pullFile" => {
            let p: crate::parser::types::FileTransferParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::PullFile(p)
        }

        "waitForLocation" => {
            let p: crate::parser::types::WaitForLocationParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::WaitForLocation(p)
        }

        "waitForMockCompletion" => {
            let p: crate::parser::types::WaitForMockCompletionParams = if params.is_null() {
                // Handle empty params case e.g. - waitForMockCompletion:
                crate::parser::types::WaitForMockCompletionParams {
                    name: None,
                    timeout: None,
                }
            } else if let Ok(timeout) = serde_yaml::from_value::<u64>(params.clone()) {
                crate::parser::types::WaitForMockCompletionParams {
                    name: None,
                    timeout: Some(timeout),
                }
            } else {
                // Try parsing struct
                serde_yaml::from_value(params.clone()).unwrap_or(
                    crate::parser::types::WaitForMockCompletionParams {
                        name: None,
                        timeout: None,
                    },
                )
            };
            TestCommand::WaitForMockCompletion(p)
        }

        "startProfiling" => {
            let p: Option<crate::parser::types::StartProfilingParams> = if params.is_null() {
                None
            } else {
                serde_yaml::from_value(params.clone()).ok()
            };
            TestCommand::StartProfiling(p)
        }

        "stopProfiling" => {
            let p: Option<crate::parser::types::StopProfilingParams> = if params.is_null() {
                None
            } else {
                serde_yaml::from_value(params.clone()).ok()
            };
            TestCommand::StopProfiling(p)
        }

        "assertPerformance" => {
            let p: crate::parser::types::AssertPerformanceParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::AssertPerformance(p)
        }

        "setCpuThrottling" => {
            let value = if params.is_number() {
                params.as_f64().unwrap_or(1.0)
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::SetCpuThrottling(value)
        }

        "setNetworkConditions" => {
            let profile = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::SetNetworkConditions(profile)
        }

        "mockLocationControl" => {
            let p: crate::parser::types::MockLocationControlParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::MockLocationControl(p)
        }

        "clearAppData" => {
            let pkg = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::ClearAppData(pkg)
        }

        "setClipboard" => {
            let val = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::SetClipboard(val)
        }

        "getClipboard" => {
            // Support simple string as variable name
            let p = if params.is_string() {
                crate::parser::types::SetVarParams {
                    name: params.as_str().unwrap().to_string(),
                    value: String::new(),
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::GetClipboard(p)
        }

        "assertClipboard" => {
            let val = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::AssertClipboard(val)
        }

        "assertTrue" | "assert" => {
            let p = if params.is_string() {
                crate::parser::types::AssertTrueParams::Expression(
                    params.as_str().unwrap().to_string(),
                )
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::AssertTrue(p)
        }

        "evalScript" => {
            let expr = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::EvalScript(expr)
        }

        "copyTextFrom" => {
            let p: crate::parser::types::CopyTextFromParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::CopyTextFrom(p)
        }

        "pasteText" => TestCommand::PasteText,

        "inputRandomEmail" => TestCommand::InputRandomEmail,

        "inputRandomNumber" | "inputRandomPhoneNumber" => {
            let p: Option<crate::parser::types::RandomNumberParams> =
                serde_yaml::from_value(params.clone()).ok();
            TestCommand::InputRandomNumber(p)
        }

        "inputRandomPersonName" => TestCommand::InputRandomPersonName,

        "inputRandomText" => {
            let p: Option<crate::parser::types::RandomTextParams> =
                serde_yaml::from_value(params.clone()).ok();
            TestCommand::InputRandomText(p)
        }

        "extendedWaitUntil" => {
            let p: crate::parser::types::ExtendedWaitParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::ExtendedWaitUntil(p)
        }

        "setNetwork" => {
            let p: crate::parser::types::NetworkParams = serde_yaml::from_value(params.clone())?;
            TestCommand::SetNetwork(p)
        }

        "airplaneMode" | "toggleAirplaneMode" => TestCommand::ToggleAirplaneMode,

        "openNotifications" => TestCommand::OpenNotifications,
        "openQuickSettings" => TestCommand::OpenQuickSettings,

        "setVolume" => {
            let level = if params.is_number() {
                params.as_u64().unwrap() as u8
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::SetVolume(level)
        }

        "lockDevice" => TestCommand::LockDevice,
        "unlockDevice" => TestCommand::UnlockDevice,

        "installApp" => {
            let path = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::InstallApp(path)
        }

        "uninstallApp" => {
            let pkg = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::UninstallApp(pkg)
        }

        "backgroundApp" => {
            let p: crate::parser::types::BackgroundAppParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::BackgroundApp(p)
        }

        "setOrientation" => {
            let p: crate::parser::types::OrientationParams = if params.is_string() {
                // Handle "LANDSCAPE" etc string alias
                let mode: crate::parser::types::Orientation =
                    serde_yaml::from_value(params.clone())?;
                crate::parser::types::OrientationParams { mode }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::SetOrientation(p)
        }

        "dbQuery" => {
            let p: crate::parser::types::DbQueryParams = serde_yaml::from_value(params.clone())?;
            TestCommand::DbQuery(p)
        }

        "openLink" | "deepLink" => {
            let s = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::OpenLink(s)
        }

        "focusApp" | "switchApp" => {
            let s = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::FocusApp(s)
        }

        "navigate" => {
            let p = if params.is_string() {
                crate::parser::types::NavigateParams {
                    url: params.as_str().unwrap().to_string(),
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::Navigate(p)
        }

        "click" => {
            let p = if params.is_string() {
                crate::parser::types::ClickParams {
                    text: Some(params.as_str().unwrap().to_string()),
                    selector: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::Click(p)
        }

        "setLocale" | "locale" => {
            let locale = match params {
                serde_yaml::Value::String(s) => s.clone(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::SetLocale(locale)
        }

        "selectDisplay" | "display" => {
            let id = match params {
                serde_yaml::Value::String(s) => s.clone(),
                serde_yaml::Value::Number(n) => n.to_string(),
                _ => serde_yaml::from_value(params.clone())?,
            };
            TestCommand::SelectDisplay(id)
        }

        "playMedia" => {
            let p: crate::parser::types::PlayMediaParams = serde_yaml::from_value(params.clone())?;
            TestCommand::PlayMedia(p)
        }

        "stopMedia" => TestCommand::StopMedia,

        "startAudioCapture" => {
            let p: crate::parser::types::StartAudioCaptureParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::StartAudioCapture(p)
        }

        "stopAudioCapture" => TestCommand::StopAudioCapture,

        "verifyAudioDucking" => {
            let p: crate::parser::types::VerifyAudioDuckingParams = if params.is_null() {
                serde_yaml::from_str("{}")?
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::VerifyAudioDucking(p)
        }

        // Hardware Automation Commands (Standardized hw* Commands)
        "hwConnect" => {
            let p: crate::parser::types::HardwareConnectParams = if params.is_string() {
                crate::parser::types::HardwareConnectParams {
                    port: params.as_str().unwrap().to_string(),
                    auto_power_off: Some(true),
                    ..Default::default()
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwConnect(p)
        }
        "hwPing" => {
            let p: crate::parser::types::HardwarePingParams = if params.is_number() {
                crate::parser::types::HardwarePingParams {
                    port: None,
                    baudrate: None,
                    node_id: params.as_u64().map(|n| n as u8),
                    save_as: None,
                }
            } else if let Some(s) = params.as_str() {
                if s.chars().all(|c| c.is_ascii_digit()) {
                    crate::parser::types::HardwarePingParams {
                        port: None,
                        baudrate: None,
                        node_id: s.parse::<u8>().ok(),
                        save_as: None,
                    }
                } else {
                    crate::parser::types::HardwarePingParams {
                        port: Some(s.to_string()),
                        baudrate: None,
                        node_id: None,
                        save_as: None,
                    }
                }
            } else if params.is_null() {
                crate::parser::types::HardwarePingParams::default()
            } else {
                serde_yaml::from_value(params.clone()).unwrap_or_default()
            };
            TestCommand::HwPing(p)
        }
        "hwDisconnect" => TestCommand::HwDisconnect,

        "hwClick" => {
            let p: crate::parser::types::ServoClickParams = if params.is_number() {
                crate::parser::types::ServoClickParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    hold_ms: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoClickParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    hold_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone()).unwrap_or(
                    crate::parser::types::ServoClickParams {
                        channel: 1,
                        button: None,
                        hold_ms: None,
                    },
                )
            };
            TestCommand::HwClick(p)
        }
        "hwPress" => {
            let p: crate::parser::types::ServoActionParams = if params.is_number() {
                crate::parser::types::ServoActionParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    save_as: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoActionParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    save_as: None,
                }
            } else {
                serde_yaml::from_value(params.clone()).unwrap_or(
                    crate::parser::types::ServoActionParams {
                        channel: 1,
                        button: None,
                        save_as: None,
                    },
                )
            };
            TestCommand::HwPress(p)
        }
        "hwRelease" => {
            let p: crate::parser::types::ServoActionParams = if params.is_number() {
                crate::parser::types::ServoActionParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    save_as: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoActionParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    save_as: None,
                }
            } else {
                serde_yaml::from_value(params.clone()).unwrap_or(
                    crate::parser::types::ServoActionParams {
                        channel: 1,
                        button: None,
                        save_as: None,
                    },
                )
            };
            TestCommand::HwRelease(p)
        }
        "hwRotate" => {
            let p: crate::parser::types::ServoRotateParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::HwRotate(p)
        }
        "hwReadServo" => {
            let p: crate::parser::types::ServoActionParams = if params.is_number() {
                crate::parser::types::ServoActionParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    save_as: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoActionParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    save_as: None,
                }
            } else {
                serde_yaml::from_value(params.clone()).unwrap_or(
                    crate::parser::types::ServoActionParams {
                        channel: 1,
                        button: None,
                        save_as: None,
                    },
                )
            };
            TestCommand::HwReadServo(p)
        }
        "hwReadRelay" => {
            let p: crate::parser::types::ServoActionParams = if params.is_number() {
                crate::parser::types::ServoActionParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    save_as: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoActionParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    save_as: None,
                }
            } else {
                serde_yaml::from_value(params.clone()).unwrap_or(
                    crate::parser::types::ServoActionParams {
                        channel: 1,
                        button: None,
                        save_as: None,
                    },
                )
            };
            TestCommand::HwReadRelay(p)
        }
        "hwReadColor" => {
            let p: crate::parser::types::ServoActionParams = if params.is_number() {
                crate::parser::types::ServoActionParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    save_as: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoActionParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    save_as: None,
                }
            } else {
                serde_yaml::from_value(params.clone()).unwrap_or(
                    crate::parser::types::ServoActionParams {
                        channel: 1,
                        button: None,
                        save_as: None,
                    },
                )
            };
            TestCommand::HwReadColor(p)
        }
        "hwReadSensorLight" => {
            let p: Option<crate::parser::types::ServoActionParams> = if params.is_number() {
                Some(crate::parser::types::ServoActionParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    save_as: None,
                })
            } else if let Some(s) = params.as_str() {
                Some(crate::parser::types::ServoActionParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    save_as: None,
                })
            } else if params.is_null() {
                None
            } else {
                serde_yaml::from_value(params.clone()).ok()
            };
            TestCommand::HwReadSensorLight(p)
        }

        "hwPowerOn" => {
            let p: crate::parser::types::RelaySetParams = if params.is_number() {
                crate::parser::types::RelaySetParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    channels: None,
                    state: "on".to_string(),
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::RelaySetParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    channels: None,
                    state: "on".to_string(),
                }
            } else {
                let mut parsed: crate::parser::types::RelaySetParams =
                    serde_yaml::from_value(params.clone())?;
                parsed.state = "on".to_string();
                parsed
            };
            TestCommand::HwPowerOn(p)
        }
        "hwPowerOff" => {
            let p: crate::parser::types::RelaySetParams = if params.is_number() {
                crate::parser::types::RelaySetParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    channels: None,
                    state: "off".to_string(),
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::RelaySetParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    channels: None,
                    state: "off".to_string(),
                }
            } else {
                let mut parsed: crate::parser::types::RelaySetParams =
                    serde_yaml::from_value(params.clone())?;
                parsed.state = "off".to_string();
                parsed
            };
            TestCommand::HwPowerOff(p)
        }
        "hwPowerOffAll" => TestCommand::HwPowerOffAll,

        "hwPowerCycle" => {
            let p: crate::parser::types::PowerCycleParams = if params.is_number() {
                crate::parser::types::PowerCycleParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    channels: None,
                    off_ms: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::PowerCycleParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    channels: None,
                    off_ms: None,
                }
            } else if params.is_null() {
                crate::parser::types::PowerCycleParams {
                    channel: 1,
                    button: None,
                    channels: None,
                    off_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwPowerCycle(p)
        }

        "hwSeeLed" => {
            let p: crate::parser::types::SeeColorParams = if let Some(s) = params.as_str() {
                if let Some((btn, col)) = s.split_once(',') {
                    crate::parser::types::SeeColorParams {
                        channel: crate::parser::types::parse_channel_str(btn),
                        button: Some(btn.trim().to_string()),
                        color: Some(col.trim().to_string()),
                        expected: Some(vec![col.trim().to_string()]),
                        timeout_ms: None,
                    }
                } else if ["NC1", "NC2", "NC3", "BUTTON1", "BUTTON2", "BUTTON3"]
                    .contains(&s.trim().to_uppercase().as_str())
                {
                    crate::parser::types::SeeColorParams {
                        channel: crate::parser::types::parse_channel_str(s),
                        button: Some(s.trim().to_string()),
                        color: None,
                        expected: None,
                        timeout_ms: None,
                    }
                } else {
                    crate::parser::types::SeeColorParams {
                        channel: 1,
                        button: None,
                        color: Some(s.to_string()),
                        expected: Some(vec![s.to_string()]),
                        timeout_ms: None,
                    }
                }
            } else if params.is_number() {
                crate::parser::types::SeeColorParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    color: None,
                    expected: None,
                    timeout_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwSeeLed(p)
        }
        "hwSeeLedBlink" => {
            let p: crate::parser::types::SeeBlinkParams = if params.is_number() {
                crate::parser::types::SeeBlinkParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    color: None,
                    count: None,
                    timeout_ms: None,
                    min_pulse_ms: None,
                    max_pulse_ms: None,
                    max_gap_ms: None,
                }
            } else if let Some(s) = params.as_str() {
                if let Some((btn, col)) = s.split_once(',') {
                    crate::parser::types::SeeBlinkParams {
                        channel: crate::parser::types::parse_channel_str(btn),
                        button: Some(btn.trim().to_string()),
                        color: Some(col.trim().to_string()),
                        count: None,
                        timeout_ms: None,
                        min_pulse_ms: None,
                        max_pulse_ms: None,
                        max_gap_ms: None,
                    }
                } else {
                    crate::parser::types::SeeBlinkParams {
                        channel: 1,
                        button: None,
                        color: Some(s.to_string()),
                        count: None,
                        timeout_ms: None,
                        min_pulse_ms: None,
                        max_pulse_ms: None,
                        max_gap_ms: None,
                    }
                }
            } else if params.is_null() {
                crate::parser::types::SeeBlinkParams {
                    channel: 1,
                    button: None,
                    color: None,
                    count: None,
                    timeout_ms: None,
                    min_pulse_ms: None,
                    max_pulse_ms: None,
                    max_gap_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwSeeLedBlink(p)
        }

        "hwSeeNativeLedBlink" => {
            let p: crate::parser::types::SeeBlinkParams = if params.is_number() {
                crate::parser::types::SeeBlinkParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    color: None,
                    count: None,
                    timeout_ms: None,
                    min_pulse_ms: None,
                    max_pulse_ms: None,
                    max_gap_ms: None,
                }
            } else if let Some(s) = params.as_str() {
                if let Some((btn, col)) = s.split_once(',') {
                    crate::parser::types::SeeBlinkParams {
                        channel: crate::parser::types::parse_channel_str(btn),
                        button: Some(btn.trim().to_string()),
                        color: Some(col.trim().to_string()),
                        count: None,
                        timeout_ms: None,
                        min_pulse_ms: None,
                        max_pulse_ms: None,
                        max_gap_ms: None,
                    }
                } else {
                    crate::parser::types::SeeBlinkParams {
                        channel: 1,
                        button: None,
                        color: Some(s.to_string()),
                        count: None,
                        timeout_ms: None,
                        min_pulse_ms: None,
                        max_pulse_ms: None,
                        max_gap_ms: None,
                    }
                }
            } else if params.is_null() {
                crate::parser::types::SeeBlinkParams {
                    channel: 1,
                    button: None,
                    color: None,
                    count: None,
                    timeout_ms: None,
                    min_pulse_ms: None,
                    max_pulse_ms: None,
                    max_gap_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwSeeNativeLedBlink(p)
        }

        "hwSeeLedOff" => {
            let p: crate::parser::types::SeeBlinkParams = if params.is_number() {
                crate::parser::types::SeeBlinkParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    color: None,
                    count: None,
                    timeout_ms: None,
                    min_pulse_ms: None,
                    max_pulse_ms: None,
                    max_gap_ms: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::SeeBlinkParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    color: None,
                    count: None,
                    timeout_ms: None,
                    min_pulse_ms: None,
                    max_pulse_ms: None,
                    max_gap_ms: None,
                }
            } else if params.is_null() {
                crate::parser::types::SeeBlinkParams {
                    channel: 1,
                    button: None,
                    color: None,
                    count: None,
                    timeout_ms: None,
                    min_pulse_ms: None,
                    max_pulse_ms: None,
                    max_gap_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwSeeLedOff(p)
        }

        "hwRepeatClick" => {
            let p: crate::parser::types::ServoRepeatParams = if params.is_number() {
                crate::parser::types::ServoRepeatParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    count: 2,
                    press_ms: None,
                    release_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwRepeatClick(p)
        }

        "hwConfigureServo" => {
            let p: crate::parser::types::ServoConfigParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::HwConfigureServo(p)
        }

        "hwReleaseAll" => TestCommand::HwReleaseAll,

        "hwStartRepeatClick" => {
            let p: crate::parser::types::ServoStartRepeatParams = if params.is_number() {
                crate::parser::types::ServoStartRepeatParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    period_ms: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoStartRepeatParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    period_ms: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwStartRepeatClick(p)
        }

        "hwStopRepeatClick" => {
            let p: crate::parser::types::ServoActionParams = if params.is_number() {
                crate::parser::types::ServoActionParams {
                    channel: params.as_u64().unwrap_or(1) as u8,
                    button: None,
                    save_as: None,
                }
            } else if let Some(s) = params.as_str() {
                crate::parser::types::ServoActionParams {
                    channel: crate::parser::types::parse_channel_str(s),
                    button: Some(s.to_string()),
                    save_as: None,
                }
            } else if params.is_null() {
                crate::parser::types::ServoActionParams {
                    channel: 1,
                    button: None,
                    save_as: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwStopRepeatClick(p)
        }

        "hwSensorLight" => {
            let p: crate::parser::types::SensorLightParams = if params.is_bool() {
                crate::parser::types::SensorLightParams {
                    channel: 1,
                    button: None,
                    state: None,
                    enabled: params.as_bool(),
                }
            } else if params.is_string() {
                crate::parser::types::SensorLightParams {
                    channel: 1,
                    button: None,
                    state: Some(params.as_str().unwrap().to_string()),
                    enabled: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::HwSensorLight(p)
        }

        "hwSetBrightnessThresholds" => {
            let p: crate::parser::types::BrightnessThresholdsParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::HwSetBrightnessThresholds(p)
        }

        "hwWaitForBrightness" => {
            let p: crate::parser::types::WaitForBrightnessParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::HwWaitForBrightness(p)
        }

        "hwWaitForCct" => {
            let p: crate::parser::types::WaitForCctParams = serde_yaml::from_value(params.clone())?;
            TestCommand::HwWaitForCct(p)
        }

        "hwCalibrateColor" => {
            let p: crate::parser::types::CalibrateColorParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::HwCalibrateColor(p)
        }

        "hwCalibrateBrightness" => {
            let p: crate::parser::types::CalibrateBrightnessParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::HwCalibrateBrightness(p)
        }

        "hwAddCctPoint" => {
            let p: crate::parser::types::AddCctPointParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::HwAddCctPoint(p)
        }

        "hwSaveCalibration" => TestCommand::HwSaveCalibration,
        "hwLoadCalibration" => TestCommand::HwLoadCalibration,
        "hwResetCalibration" => TestCommand::HwResetCalibration,
        "hwEraseCalibration" => TestCommand::HwEraseCalibration,
        "hwSafeState" => TestCommand::HwSafeState,
        "hwDiagnostics" => TestCommand::HwDiagnostics,

        "runPython" | "execPython" | "python" => {
            let p: crate::parser::types::RunPythonParams = if params.is_string() {
                crate::parser::types::RunPythonParams {
                    script: Some(params.as_str().unwrap().to_string()),
                    code: None,
                    args: vec![],
                    env: HashMap::new(),
                    timeout_ms: None,
                    python_path: None,
                    save_var: None,
                    save_vars: None,
                }
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::RunPython(p)
        }

        "assertDeviceState" | "checkDevice" => {
            let p: crate::parser::types::DeviceStateParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::AssertDeviceState(p)
        }

        "getDeviceState" => {
            let p: crate::parser::types::GetDeviceStateParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::GetDeviceState(p)
        }

        "waitDeviceState" => {
            let p: crate::parser::types::WaitDeviceStateParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::WaitDeviceState(p)
        }

        "assertDeviceTransition" | "assertDeviceChange" => {
            let p: crate::parser::types::DeviceTransitionParams =
                serde_yaml::from_value(params.clone())?;
            TestCommand::AssertDeviceTransition(p)
        }

        "waitLedPattern" | "assertDevicePattern" => {
            let p: crate::parser::types::LedPatternParams = serde_yaml::from_value(params.clone())?;
            TestCommand::WaitLedPattern(p)
        }

        "setWindowSize" | "resizeWindow" | "windowSize" | "setWindow" => {
            let p: crate::parser::types::SetWindowSizeParamsInput = if let Some(s) = params.as_str()
            {
                crate::parser::types::SetWindowSizeParamsInput::String(s.to_string())
            } else {
                serde_yaml::from_value(params.clone())?
            };
            TestCommand::SetWindowSize(p)
        }

        _ => return Ok(None),
    };

    Ok(Some(cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_flow() {
        let yaml = r#"
appId: com.example.app
---
- launchApp:
    clearState: true
- tapOn:
    text: "Login"
- inputText: "test@example.com"
- assertVisible:
    text: "Dashboard"
"#;

        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        assert_eq!(flow.app_id, Some("com.example.app".to_string()));
        assert_eq!(flow.commands.len(), 4);
    }

    #[test]
    fn unknown_parameterized_command_is_rejected() {
        let yaml = r#"
appId: com.example.app
---
- tappp:
    text: "Login"
"#;

        let err = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap_err();
        assert!(err.to_string().contains("Unknown command"));
        assert!(err.to_string().contains("tappp"));
    }

    #[test]
    fn map_format_preserves_supported_header_fields() {
        let yaml = r#"
appId: com.example.app
url: "https://example.com"
platform: web
browser: Firefox
speed: fast
defaultTimeout: 3000
closeWhenFinish: false
steps:
  - see: "Welcome"
"#;

        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        assert_eq!(flow.app_id, Some("com.example.app".to_string()));
        assert_eq!(flow.url, Some("https://example.com".to_string()));
        assert_eq!(flow.platform, Some(Platform::Web));
        assert_eq!(flow.browser, Some("Firefox".to_string()));
        assert_eq!(flow.speed, Some("fast".to_string()));
        assert_eq!(flow.default_timeout_ms, Some(3000));
        assert_eq!(flow.close_when_finish, Some(false));
        assert_eq!(flow.commands.len(), 1);
    }

    #[test]
    fn parses_desktop_platform_headers() {
        let macos = r#"
platform: macos
appId: /Applications/Calculator.app
---
- launchApp
"#;

        let windows = r#"
platform: windows
appId: C:\Program Files\Example\Example.exe
---
- launchApp
"#;

        let macos_flow = parse_yaml_content(macos, Path::new("macos.yaml")).unwrap();
        let windows_flow = parse_yaml_content(windows, Path::new("windows.yaml")).unwrap();

        assert_eq!(macos_flow.platform, Some(Platform::Macos));
        assert_eq!(windows_flow.platform, Some(Platform::Windows));
    }

    #[test]
    fn parses_and_serializes_android_auto_platform_header() {
        let yaml = r#"
platform: android_auto
appId: com.example.auto
---
- launchApp
"#;

        let flow = parse_yaml_content(yaml, Path::new("android-auto.yaml")).unwrap();
        assert_eq!(flow.platform, Some(Platform::AndroidAuto));
        assert_eq!(
            serde_json::to_value(flow.platform).unwrap(),
            serde_json::Value::String("android_auto".to_string())
        );
    }

    #[test]
    fn parses_desktop_state_clear_header() {
        let yaml = r#"
platform: macos
appId: /Applications/MyApp.app
desktopState:
  clear:
    mode: autoSafe
    paths:
      - "~/Library/Application Support/MyApp"
    keychainServices:
      - "com.example.MyApp"
---
- launchApp:
    clearState: true
"#;

        let flow = parse_yaml_content(yaml, Path::new("macos.yaml")).unwrap();
        let desktop_state = flow.desktop_state.expect("desktopState should parse");
        let clear = desktop_state
            .clear
            .expect("desktopState.clear should parse");

        assert_eq!(
            clear.mode,
            Some(crate::parser::types::DesktopClearMode::AutoSafe)
        );
        assert_eq!(
            clear.paths,
            vec!["~/Library/Application Support/MyApp".to_string()]
        );
        assert_eq!(
            clear.keychain_services,
            vec!["com.example.MyApp".to_string()]
        );
    }

    #[test]
    fn parses_windows_desktop_state_clear_header() {
        let yaml = r#"
platform: windows
appId: C:\Program Files\Example\Example.exe
desktopState:
  clear:
    mode: manual
    paths:
      - "%APPDATA%\\Example"
    registryKeys:
      - "HKCU:\\Software\\Example"
---
- launchApp:
    clearState: true
"#;

        let flow = parse_yaml_content(yaml, Path::new("windows.yaml")).unwrap();
        let desktop_state = flow.desktop_state.expect("desktopState should parse");
        let clear = desktop_state
            .clear
            .expect("desktopState.clear should parse");

        assert_eq!(
            clear.mode,
            Some(crate::parser::types::DesktopClearMode::Manual)
        );
        assert_eq!(clear.paths, vec![r"%APPDATA%\Example".to_string()]);
        assert_eq!(
            clear.registry_keys,
            vec![r"HKCU:\Software\Example".to_string()]
        );
    }

    #[test]
    fn bundled_schema_allows_desktop_platforms() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../schema/lumi-test.schema.json")).unwrap();
        let platforms = schema["properties"]["platform"]["enum"]
            .as_array()
            .expect("platform enum should be an array");

        assert!(platforms.contains(&serde_json::Value::String("macos".to_string())));
        assert!(platforms.contains(&serde_json::Value::String("windows".to_string())));
        assert!(platforms.contains(&serde_json::Value::String("android_auto".to_string())));
    }

    #[test]
    fn parses_performance_and_screenshot_commands_from_custom_parser() {
        let yaml = r#"
platform: android
---
- assertScreenshot: baseline.png
- startProfiling:
    package: com.example.app
- stopProfiling:
    savePath: profile.trace
- assertPerformance:
    metric: memory
    limit: 200MB
- setCpuThrottling: 4
- setNetworkConditions: 3g
"#;

        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        assert_eq!(flow.commands.len(), 6);
        assert!(matches!(flow.commands[0], TestCommand::AssertScreenshot(_)));
        assert!(matches!(flow.commands[1], TestCommand::StartProfiling(_)));
        assert!(matches!(flow.commands[2], TestCommand::StopProfiling(_)));
        assert!(matches!(
            flow.commands[3],
            TestCommand::AssertPerformance(_)
        ));
        assert!(matches!(flow.commands[4], TestCommand::SetCpuThrottling(_)));
        assert!(matches!(
            flow.commands[5],
            TestCommand::SetNetworkConditions(_)
        ));
    }

    #[test]
    fn parses_get_device_state_command_from_custom_parser() {
        let yaml = r#"
platform: android
---
- getDeviceState:
    saveAs: device_state
"#;

        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        match &flow.commands[0] {
            TestCommand::GetDeviceState(params) => {
                assert_eq!(params.save_as, "device_state");
            }
            _ => panic!("Expected GetDeviceState command"),
        }
    }

    #[test]
    fn invalid_camera_header_fails_clearly() {
        let yaml = r#"
platform: android
camera:
  rtsp:
    nested: invalid
  profile: switch4.json
commands:
  - launchApp
"#;

        let err = parse_yaml_content(yaml, Path::new("camera.yaml")).unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to parse camera/cameras header")
                || format!("{err:#}").contains("Failed to parse camera/cameras header"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn env_file_parser_handles_non_shell_safe_values() {
        let camera_rtsp_key = format!("{}{}", "CAMERA_", "RTSP");
        let content = format!(
            r#"
# comments are ignored
export {}="{}{}:{}{}10.0.0.5/live"
HOME==Nhà 3D
INLINE=value # comment
PASSWORD='abc#123'
not a shell assignment
            "#,
            camera_rtsp_key, "rtsp://", "user", "pass", "@"
        );
        let parsed = parse_env_file_content(&content);

        let expected_rtsp = format!("{}{}:{}{}10.0.0.5/live", "rtsp://", "user", "pass", "@");
        assert_eq!(
            parsed.get(&camera_rtsp_key).map(String::as_str),
            Some(expected_rtsp.as_str())
        );
        assert_eq!(parsed.get("HOME").map(String::as_str), Some("=Nhà 3D"));
        assert_eq!(parsed.get("INLINE").map(String::as_str), Some("value"));
        assert_eq!(parsed.get("PASSWORD").map(String::as_str), Some("abc#123"));
        assert!(!parsed.contains_key("not a shell assignment"));
    }

    #[test]
    fn test_hardware_commands_parsing() {
        let yaml = r#"
platform: android
---
- hwClick: 1
- hwClick: { channel: 2, hold_ms: 500 }
- hwPress: 1
- hwRelease: 1
- hwSeeLedBlink:
    channel: 1
    color: "BLUE"
    count: 2
    minPulseMs: 50
    maxPulseMs: 800
"#;
        let flow = parse_yaml_content(yaml, Path::new("hardware.yaml")).unwrap();
        assert_eq!(flow.commands.len(), 5);

        match &flow.commands[0] {
            TestCommand::HwClick(p) => assert_eq!(p.channel, 1),
            _ => panic!("Expected HwClick for command 0"),
        }
        match &flow.commands[1] {
            TestCommand::HwClick(p) => {
                assert_eq!(p.channel, 2);
                assert_eq!(p.hold_ms, Some(500));
            }
            _ => panic!("Expected HwClick for command 1"),
        }
        match &flow.commands[2] {
            TestCommand::HwPress(p) => assert_eq!(p.channel, 1),
            _ => panic!("Expected HwPress for command 2"),
        }
        match &flow.commands[3] {
            TestCommand::HwRelease(p) => assert_eq!(p.channel, 1),
            _ => panic!("Expected HwRelease for command 3"),
        }
        match &flow.commands[4] {
            TestCommand::HwSeeLedBlink(p) => {
                assert_eq!(p.channel, 1);
                assert_eq!(p.color, Some("BLUE".to_string()));
                assert_eq!(p.count, Some(2));
                assert_eq!(p.min_pulse_ms, Some(50));
                assert_eq!(p.max_pulse_ms, Some(800));
            }
            _ => panic!("Expected HwSeeLedBlink for command 4"),
        }
    }

    #[test]
    fn test_hardware_commands_variables_parsing() {
        let yaml = r#"
platform: android
jig:
  port: "${JIG_PORT:-COM5}"
  wireFormat: "${WIRE_FORMAT:-framed}"
---
- hwClick: "${MY_BTN}"
- hwClick:
    channel: "${MY_BTN}"
    holdMs: 300
- hwReadColor:
    button: "${SENSOR}"
    saveAs: "detected_color"
- hwReadRelay:
    button: "${RELAY_CH}"
    saveAs: "relay_state"
- hwPing:
    port: "${PING_PORT:-COM1}"
    saveAs: "ping_ok"
- hwSeeLed:
    button: "${SENSOR}"
    color: "${EXPECTED_COLOR}"
"#;
        let flow = parse_yaml_content(yaml, Path::new("hw_vars.yaml")).unwrap();
        assert_eq!(flow.commands.len(), 6);

        // Header jig port & wireFormat preserve expressions
        if let Some(ref jig) = flow.jig {
            let params = jig.resolve(None).unwrap();
            assert_eq!(params.port, "${JIG_PORT:-COM5}");
            assert_eq!(
                params.wire_format,
                Some("${WIRE_FORMAT:-framed}".to_string())
            );
        } else {
            panic!("Expected jig header");
        }

        // hwClick string shorthand
        match &flow.commands[0] {
            TestCommand::HwClick(p) => assert_eq!(p.button.as_deref(), Some("${MY_BTN}")),
            _ => panic!("Expected HwClick for command 0"),
        }

        // hwClick map with channel expression
        match &flow.commands[1] {
            TestCommand::HwClick(p) => {
                assert_eq!(p.button.as_deref(), Some("${MY_BTN}"));
                assert_eq!(p.hold_ms, Some(300));
            }
            _ => panic!("Expected HwClick for command 1"),
        }

        // hwReadColor with saveAs
        match &flow.commands[2] {
            TestCommand::HwReadColor(p) => {
                assert_eq!(p.button.as_deref(), Some("${SENSOR}"));
                assert_eq!(p.save_as.as_deref(), Some("detected_color"));
            }
            _ => panic!("Expected HwReadColor for command 2"),
        }

        // hwReadRelay with saveAs
        match &flow.commands[3] {
            TestCommand::HwReadRelay(p) => {
                assert_eq!(p.button.as_deref(), Some("${RELAY_CH}"));
                assert_eq!(p.save_as.as_deref(), Some("relay_state"));
            }
            _ => panic!("Expected HwReadRelay for command 3"),
        }

        // hwPing with port default & saveAs
        match &flow.commands[4] {
            TestCommand::HwPing(p) => {
                assert_eq!(p.port.as_deref(), Some("${PING_PORT:-COM1}"));
                assert_eq!(p.save_as.as_deref(), Some("ping_ok"));
            }
            _ => panic!("Expected HwPing for command 4"),
        }

        // hwSeeLed with button & color expressions
        match &flow.commands[5] {
            TestCommand::HwSeeLed(p) => {
                assert_eq!(p.button.as_deref(), Some("${SENSOR}"));
                assert_eq!(p.color.as_deref(), Some("${EXPECTED_COLOR}"));
            }
            _ => panic!("Expected HwSeeLed for command 5"),
        }
    }

    #[test]
    fn test_jig_profile_and_servos_parsing() {
        let yaml = r#"
platform: android
jig:
  port: "COM5"
  baudrate: 115200
  servos:
    - channel: 1
      pressAngle: 75
      releaseAngle: 15
    - channel: 2
      pressAngle: 70
---
- hwClick: 1
"#;
        let flow = parse_yaml_content(yaml, Path::new("jig_test.yaml")).unwrap();
        assert!(flow.jig.is_some());
        let jig = flow.jig.unwrap();
        let params = jig.resolve(None).unwrap();
        assert_eq!(params.port, "COM5");
        assert_eq!(params.baudrate, Some(115200));
        let servos = params.servos.unwrap();
        assert_eq!(servos.len(), 2);
        assert_eq!(servos[0].channel, 1);
        assert_eq!(servos[0].press_angle, Some(75));
        assert_eq!(servos[0].release_angle, Some(15));
        assert_eq!(servos[1].channel, 2);
        assert_eq!(servos[1].press_angle, Some(70));
    }

    #[test]
    fn test_jig_buttons_and_relays_profile_parsing() {
        let yaml = r#"
platform: android
jig:
  port: "COM5"
  buttons:
    NC1:
      servo: 5
      sensor: 5
    NC3:
      servo: 7
      sensor: 6
  relays:
    mainPower: [3, 4]
    220V: [3, 4]
---
- hwClick: "NC3"
- hwReadColor: "NC3"
"#;
        let flow = parse_yaml_content(yaml, Path::new("jig_btn_test.yaml")).unwrap();
        let jig = flow.jig.unwrap();
        let params = jig.resolve(None).unwrap();
        let buttons = params.buttons.unwrap();
        assert_eq!(buttons.get("NC3").unwrap().servo, Some(7));
        assert_eq!(buttons.get("NC3").unwrap().sensor, Some(6));
        let relays = params.relays.unwrap();
        assert_eq!(relays.get("220V").unwrap(), &vec![3, 4]);

        let mut hw_config = crate::hardware::HardwareConfig::default();
        hw_config.button_mappings = buttons;
        hw_config.relay_mappings = relays;
        let ctrl = crate::hardware::HardwareController::new(Some(hw_config));
        assert_eq!(ctrl.resolve_servo_channel("NC3"), 7);
        assert_eq!(ctrl.resolve_sensor_channel("NC3"), 6);
        assert_eq!(ctrl.resolve_relay_channels("220V"), vec![3, 4]);
    }

    #[test]
    fn test_run_python_parsing() {
        let yaml = r#"
platform: android
---
- runPython: "./scripts/test.py"
- execPython:
    script: "./scripts/run.py"
    args: ["--mode", "fast"]
    saveVars:
      token: "data.token"
      status: "status"
- python:
    code: "print('hello')"
    saveVar: "out"
"#;
        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        assert_eq!(flow.commands.len(), 3);

        match &flow.commands[0] {
            TestCommand::RunPython(p) => assert_eq!(p.script.as_deref(), Some("./scripts/test.py")),
            _ => panic!("Expected RunPython for command 0"),
        }
        match &flow.commands[1] {
            TestCommand::RunPython(p) => {
                assert_eq!(p.script.as_deref(), Some("./scripts/run.py"));
                assert_eq!(p.args, vec!["--mode", "fast"]);
                assert!(p.save_vars.is_some());
            }
            _ => panic!("Expected RunPython for command 1"),
        }
        match &flow.commands[2] {
            TestCommand::RunPython(p) => {
                assert_eq!(p.code.as_deref(), Some("print('hello')"));
                assert_eq!(p.save_var.as_deref(), Some("out"));
            }
            _ => panic!("Expected RunPython for command 2"),
        }
    }

    #[test]
    fn test_jig_wire_format_parsing() {
        let yaml = r#"
platform: android
appId: com.example.app
jig:
  port: "COM7"
  nodeId: 2
  wireFormat: "[NODE:{node}] {command}\r\n"
---
- hwClick: 1
"#;
        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        assert!(flow.jig.is_some());
        let params = flow.jig.unwrap().resolve(None).unwrap();
        assert_eq!(params.port, "COM7");
        assert_eq!(params.node_id, Some(2));
        assert_eq!(
            params.wire_format,
            Some("[NODE:{node}] {command}\r\n".to_string())
        );
    }

    #[test]
    fn test_when_modifier_parsing() {
        let yaml = r#"
platform: android
---
- tap: "Accept Cookies"
  when:
    visible: "Accept Cookies"
- inputText: "staging_user"
  when: "${ENV} == 'staging'"
- back:
  when:
    notVisible: "Welcome"
"#;
        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        assert_eq!(flow.commands.len(), 3);

        match &flow.commands[0] {
            TestCommand::When(p) => {
                assert_eq!(
                    p.condition.get("visible").and_then(|v| v.as_str()),
                    Some("Accept Cookies")
                );
                match p.command.as_ref() {
                    TestCommand::TapOn(t) => assert_eq!(
                        t.clone().into_inner().text.as_deref(),
                        Some("Accept Cookies")
                    ),
                    _ => panic!("Expected TapOn inner command"),
                }
            }
            _ => panic!("Expected When command for #0"),
        }

        match &flow.commands[1] {
            TestCommand::When(p) => {
                assert_eq!(p.condition.as_str(), Some("${ENV} == 'staging'"));
                match p.command.as_ref() {
                    TestCommand::InputText(it) => {
                        assert_eq!(it.clone().into_inner().text.as_str(), "staging_user")
                    }
                    _ => panic!("Expected InputText inner command"),
                }
            }
            _ => panic!("Expected When command for #1"),
        }

        match &flow.commands[2] {
            TestCommand::When(p) => {
                assert_eq!(
                    p.condition.get("notVisible").and_then(|v| v.as_str()),
                    Some("Welcome")
                );
                match p.command.as_ref() {
                    TestCommand::Back => {}
                    _ => panic!("Expected Back inner command"),
                }
            }
            _ => panic!("Expected When command for #2"),
        }
    }

    #[test]
    fn test_for_each_and_match_parsing() {
        let yaml = r#"
platform: android
---
- forEach:
    item: ch
    in: [1, 2, 3]
    commands:
      - hwClick:
          channel: "${ch}"
- match: "${ROLE}"
  cases:
    admin:
      - tap: "Admin Panel"
    user:
      - tap: "User Profile"
  default:
    - see: "Login"
"#;
        let flow = parse_yaml_content(yaml, Path::new("test.yaml")).unwrap();
        assert_eq!(flow.commands.len(), 2);

        match &flow.commands[0] {
            TestCommand::ForEach(p) => {
                assert_eq!(p.item, "ch");
                assert_eq!(p.commands.len(), 1);
            }
            _ => panic!("Expected ForEach command for #0"),
        }

        match &flow.commands[1] {
            TestCommand::Match(p) => {
                assert_eq!(p.value, "${ROLE}");
                assert_eq!(p.cases.len(), 2);
                assert!(p.cases.contains_key("admin"));
                assert!(p.cases.contains_key("user"));
                assert!(p.default.is_some());
            }
            _ => panic!("Expected Match command for #1"),
        }
    }
}
