use super::evidence::FlowRootAuthority;
pub use super::evidence::{EvidenceBoundaryError, EvidenceRoot, EvidenceWriter};
use super::types::{V3Capability, V3CommandRequirement};
use crate::parser::types::{Platform, TestCommand, TestFlow};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

const SUPPORTED_IMPLEMENTATION: &str = "lumi_iot_v3_home_controller";
const SUPPORTED_PROTOCOL_PACK: &str = "lumi_iot_v3_home_controller";
const SUPPORTED_PROTOCOL_VERSION: &str = "0.2.0";
const SUPPORTED_PACK_VERSION: &str = "0.1.0";

const REQUIRED_REDACT_FIELDS: [&str; 6] = [
    "dev_key",
    "nwk_key",
    "app_key",
    "authorization",
    "cookie",
    "token",
];

#[derive(Debug, Error)]
pub enum V3ValidationError {
    #[error("V3 HC commands require a gateway component with platform: lumi_hc")]
    UnsupportedPlatform,
    #[error("App command '{command}' is outside the HC gateway flow scope")]
    DeferredCommand { command: String },
    #[error("V3 HC flow requires a relative profile path")]
    MissingProfile,
    #[error("profile path '{profile}' must be relative to the flow file")]
    InvalidProfilePath { profile: String },
    #[error("profile '{profile}' was not found relative to the flow")]
    ProfileNotFound { profile: String },
    #[error("profile '{profile}' could not be read: {reason}")]
    ProfileRead { profile: String, reason: String },
    #[error("profile '{profile}' is malformed: {reason}")]
    ProfileParse { profile: String, reason: String },
    #[error("profile field '{field}' is required")]
    MissingProfileField { field: String },
    #[error("profile field '{field}' has unsupported value '{value}'")]
    UnsupportedProfileValue { field: String, value: String },
    #[error("profile endpoint '{field}' is invalid: expected {expected}, got '{value}'")]
    InvalidEndpoint {
        field: String,
        expected: String,
        value: String,
    },
    #[error("evidence root '{root}' must remain a relative path within the flow directory")]
    InvalidEvidenceRoot { root: String },
    #[error("profile capability '{capability}' is unsupported or deferred in Phase 1")]
    DeferredCapability { capability: String },
    #[error("profile is missing required capability '{capability}' for command '{command}'")]
    MissingCapability {
        capability: String,
        command: String,
    },
}

impl V3ValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::DeferredCommand { .. } => "deferred_command",
            Self::MissingProfile => "missing_profile",
            Self::InvalidProfilePath { .. } => "invalid_profile_path",
            Self::ProfileNotFound { .. } => "missing_profile",
            Self::ProfileRead { .. } => "profile_read_error",
            Self::ProfileParse { .. } => "profile_parse_error",
            Self::MissingProfileField { .. } => "missing_profile_field",
            Self::UnsupportedProfileValue { .. } => "unsupported_profile_value",
            Self::InvalidEndpoint { .. } => "invalid_endpoint",
            Self::InvalidEvidenceRoot { .. } => "invalid_evidence_root",
            Self::DeferredCapability { .. } => "deferred_capability",
            Self::MissingCapability { .. } => "missing_capability",
        }
    }

    pub fn field(&self) -> Option<&str> {
        match self {
            Self::MissingProfileField { field } => Some(field),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V3Profile {
    pub implementation: String,
    pub protocol_pack: String,
    pub protocol_version: String,
    pub pack_version: String,
    pub target: V3Target,
    pub endpoint: V3Endpoint,
    pub auth: V3Auth,
    pub capabilities: Vec<V3Capability>,
    pub evidence: V3Evidence,
    pub retry: V3Retry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V3Target {
    pub chip: String,
    pub firmware: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V3Endpoint {
    pub base_url: String,
    pub websocket_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Auth {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V3Evidence {
    pub root: String,
    pub max_raw_bytes: u64,
    #[serde(rename = "maxWebSocketFrames")]
    pub max_websocket_frames: u64,
    pub redact_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V3Retry {
    pub safe_http_max_attempts: u64,
    pub control_max_attempts: u64,
    pub websocket_reconnect_max_attempts: u64,
}

#[derive(Debug, Clone)]
pub struct LoadedV3Profile {
    pub path: PathBuf,
    pub profile: V3Profile,
    pub evidence_root: EvidenceRoot,
}

impl LoadedV3Profile {
    pub fn open_evidence_writer(&self) -> Result<EvidenceWriter, EvidenceBoundaryError> {
        self.evidence_root.open_writer()
    }
}

#[derive(Debug, Clone)]
pub struct V3FlowMetadata {
    pub profile_ref: String,
    pub profile_path: PathBuf,
    pub required_capabilities: Vec<V3Capability>,
    pub command_requirements: Vec<V3CommandRequirement>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProfile {
    implementation: Option<String>,
    protocol_pack: Option<String>,
    protocol_version: Option<String>,
    pack_version: Option<String>,
    target: Option<V3Target>,
    endpoint: Option<RawEndpoint>,
    auth: Option<RawAuth>,
    capabilities: Option<Vec<String>>,
    evidence: Option<RawEvidence>,
    retry: Option<RawRetry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEndpoint {
    base_url: Option<String>,
    websocket_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAuth {
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEvidence {
    root: Option<String>,
    max_raw_bytes: Option<u64>,
    #[serde(rename = "maxWebSocketFrames", alias = "maxWebsocketFrames")]
    max_websocket_frames: Option<u64>,
    redact_fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRetry {
    safe_http_max_attempts: Option<u64>,
    control_max_attempts: Option<u64>,
    websocket_reconnect_max_attempts: Option<u64>,
}

pub fn load_profile(
    flow_path: &Path,
    profile_ref: &str,
) -> Result<LoadedV3Profile, V3ValidationError> {
    let profile_ref = profile_ref.trim();
    if profile_ref.is_empty() {
        return Err(V3ValidationError::MissingProfile);
    }

    let profile_path = Path::new(profile_ref);
    if profile_path.is_absolute() || profile_ref.contains("${") {
        return Err(V3ValidationError::InvalidProfilePath {
            profile: profile_ref.to_string(),
        });
    }

    let flow_dir = flow_path.parent().unwrap_or_else(|| Path::new("."));
    let resolved_path = flow_dir.join(profile_path);
    if !resolved_path.is_file() {
        return Err(V3ValidationError::ProfileNotFound {
            profile: profile_ref.to_string(),
        });
    }

    let content = std::fs::read_to_string(&resolved_path).map_err(|error| {
        V3ValidationError::ProfileRead {
            profile: profile_ref.to_string(),
            reason: error.to_string(),
        }
    })?;
    let raw: RawProfile = serde_yaml::from_str(&content).map_err(|error| {
        V3ValidationError::ProfileParse {
            profile: profile_ref.to_string(),
            reason: error.to_string(),
        }
    })?;
    let mut profile = build_profile(raw)?;
    let evidence_root = resolve_evidence_root(flow_dir, &profile.evidence.root)?;
    profile.evidence.root = evidence_root.path().to_string_lossy().into_owned();

    Ok(LoadedV3Profile {
        path: resolved_path,
        profile,
        evidence_root,
    })
}

pub fn validate_flow(
    flow_path: &Path,
    flow: &TestFlow,
) -> Result<V3FlowMetadata, V3ValidationError> {
    let has_v3_commands = contains_v3_commands(flow_path, &flow.commands, 0)?;

    let gateway_component = flow
        .gateway
        .as_ref()
        .filter(|component| component.platform == "lumi_hc");
    let direct_hc_flow = matches!(flow.platform, Some(Platform::LumiHc));

    if !direct_hc_flow && gateway_component.is_none() {
        if has_v3_commands {
            return Err(V3ValidationError::UnsupportedPlatform);
        }
        return Ok(V3FlowMetadata {
            profile_ref: String::new(),
            profile_path: PathBuf::new(),
            required_capabilities: Vec::new(),
            command_requirements: Vec::new(),
        });
    }

    let mut v3_commands = Vec::new();
    collect_v3_commands(flow_path, &flow.commands, &mut v3_commands, 0)?;

    let profile_ref = flow
        .profile
        .as_deref()
        .or_else(|| gateway_component.and_then(|component| component.profile.as_deref()))
        .ok_or(V3ValidationError::MissingProfile)?;
    let loaded = load_profile(flow_path, profile_ref)?;
    let command_requirements = v3_commands
        .iter()
        .enumerate()
        .map(|(index, command)| V3CommandRequirement::for_command(index, command))
        .collect::<Vec<_>>();

    let mut required_capabilities = Vec::new();
    let mut seen = HashSet::new();
    for command in &v3_commands {
        for capability in command.required_capabilities() {
            if seen.insert(capability) {
                required_capabilities.push(capability);
            }
        }
    }

    for requirement in &command_requirements {
        for capability in &requirement.capabilities {
            if !loaded.profile.capabilities.contains(capability) {
                return Err(V3ValidationError::MissingCapability {
                    capability: capability.as_str().to_string(),
                    command: requirement.name.clone(),
                });
            }
        }
    }

    if v3_commands.iter().any(|command| {
        matches!(
            command,
            super::types::V3Command::WsSubscribe(_)
                | super::types::V3Command::WsExpect(_)
                | super::types::V3Command::WsSnapshot(_)
        )
    }) && loaded.profile.endpoint.websocket_url.is_none()
    {
        return Err(V3ValidationError::MissingProfileField {
            field: "endpoint.websocketUrl".to_string(),
        });
    }

    Ok(V3FlowMetadata {
        profile_ref: profile_ref.to_string(),
        profile_path: loaded.path,
        required_capabilities,
        command_requirements,
    })
}

fn contains_v3_commands(
    source: &Path,
    commands: &[TestCommand],
    depth: usize,
) -> Result<bool, V3ValidationError> {
    if depth > 32 {
        return Err(V3ValidationError::ProfileParse {
            profile: source.display().to_string(),
            reason: "runFlow nesting exceeds maximum depth 32".to_string(),
        });
    }

    for command in commands {
        match command {
            TestCommand::V3(_) => return Ok(true),
            TestCommand::RunFlow(params) => {
                let params = crate::parser::flow::run_flow_params(params);
                if let Some(inline) = params.commands.as_deref() {
                    if contains_v3_commands(source, inline, depth + 1)? {
                        return Ok(true);
                    }
                } else if let Some(path) = params.path.as_deref() {
                    let child_path = crate::parser::flow::child_flow_path(source, path);
                    if !child_path.is_file() {
                        if params.optional.unwrap_or(false) {
                            continue;
                        }
                        return Err(V3ValidationError::ProfileRead {
                            profile: child_path.display().to_string(),
                            reason: "runFlow child file was not found".to_string(),
                        });
                    }
                    let child = crate::parser::yaml::parse_test_file(&child_path).map_err(|error| {
                        V3ValidationError::ProfileParse {
                            profile: child_path.display().to_string(),
                            reason: error.to_string(),
                        }
                    })?;
                    if contains_v3_commands(&child_path, &child.commands, depth + 1)? {
                        return Ok(true);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(false)
}

fn collect_v3_commands(
    source: &Path,
    commands: &[TestCommand],
    output: &mut Vec<super::types::V3Command>,
    depth: usize,
) -> Result<(), V3ValidationError> {
    if depth > 32 {
        return Err(V3ValidationError::ProfileParse {
            profile: source.display().to_string(),
            reason: "runFlow nesting exceeds maximum depth 32".to_string(),
        });
    }
    for command in commands {
        match command {
            TestCommand::V3(command) => output.push(command.clone()),
            TestCommand::RunFlow(params) => {
                let params = crate::parser::flow::run_flow_params(params);
                if let Some(inline) = params.commands.as_deref() {
                    collect_v3_commands(source, inline, output, depth + 1)?;
                } else if let Some(path) = params.path.as_deref() {
                    let child_path = crate::parser::flow::child_flow_path(source, path);
                    if !child_path.is_file() {
                        if params.optional.unwrap_or(false) {
                            continue;
                        }
                        return Err(V3ValidationError::ProfileRead {
                            profile: child_path.display().to_string(),
                            reason: "runFlow child file was not found".to_string(),
                        });
                    }
                    let child = crate::parser::yaml::parse_test_file(&child_path).map_err(|error| {
                        V3ValidationError::ProfileParse {
                            profile: child_path.display().to_string(),
                            reason: error.to_string(),
                        }
                    })?;
                    collect_v3_commands(&child_path, &child.commands, output, depth + 1)?;
                }
            }
            command => {
                return Err(V3ValidationError::DeferredCommand {
                    command: command.display_name(),
                });
            }
        }
    }
    Ok(())
}

fn build_profile(raw: RawProfile) -> Result<V3Profile, V3ValidationError> {
    let implementation = required_value(raw.implementation, "implementation")?;
    let protocol_pack = required_value(raw.protocol_pack, "protocolPack")?;
    let protocol_version = required_value(raw.protocol_version, "protocolVersion")?;
    let pack_version = required_value(raw.pack_version, "packVersion")?;
    validate_supported_value("implementation", &implementation, SUPPORTED_IMPLEMENTATION)?;
    validate_supported_value("protocolPack", &protocol_pack, SUPPORTED_PROTOCOL_PACK)?;
    validate_supported_value(
        "protocolVersion",
        &protocol_version,
        SUPPORTED_PROTOCOL_VERSION,
    )?;
    validate_supported_value("packVersion", &pack_version, SUPPORTED_PACK_VERSION)?;
    let target = raw
        .target
        .ok_or_else(|| missing("target"))
        .and_then(|target| {
            Ok(V3Target {
                chip: required_value(Some(target.chip), "target.chip")?,
                firmware: required_value(Some(target.firmware), "target.firmware")?,
            })
        })?;
    let endpoint = raw
        .endpoint
        .ok_or_else(|| missing("endpoint"))
        .and_then(|endpoint| {
            let base_url = required_value(endpoint.base_url, "endpoint.baseUrl")?;
            validate_endpoint(&base_url, "endpoint.baseUrl", false)?;
            if let Some(websocket_url) = endpoint.websocket_url.as_deref() {
                validate_endpoint(websocket_url, "endpoint.websocketUrl", true)?;
            }
            Ok(V3Endpoint {
                base_url,
                websocket_url: endpoint.websocket_url,
            })
        })?;
    let auth = raw
        .auth
        .ok_or_else(|| missing("auth"))
        .and_then(|auth| {
            Ok(V3Auth {
                mode: required_value(auth.mode, "auth.mode")?,
            })
        })?;
    let capability_names = raw
        .capabilities
        .ok_or_else(|| missing("capabilities"))?;
    let capabilities = capability_names
        .into_iter()
        .map(|capability| {
            V3Capability::parse(&capability).ok_or(V3ValidationError::DeferredCapability {
                capability,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if capabilities.is_empty() {
        return Err(missing("capabilities"));
    }

    let evidence = raw
        .evidence
        .ok_or_else(|| missing("evidence"))
        .and_then(|evidence| {
            let max_raw_bytes = evidence
                .max_raw_bytes
                .ok_or_else(|| missing("evidence.maxRawBytes"))?;
            let max_websocket_frames = evidence.max_websocket_frames.unwrap_or(1);
            if max_raw_bytes == 0 {
                return Err(missing("evidence.maxRawBytes"));
            }
            if max_websocket_frames == 0 {
                return Err(missing("evidence.maxWebSocketFrames"));
            }
            Ok(V3Evidence {
                root: required_value(evidence.root, "evidence.root")?,
                max_raw_bytes,
                max_websocket_frames,
                redact_fields: validate_redact_fields(
                    evidence
                        .redact_fields
                        .ok_or_else(|| missing("evidence.redactFields"))?,
                )?,
            })
        })?;

    let retry = raw
        .retry
        .ok_or_else(|| missing("retry"))
        .and_then(|retry| {
            Ok(V3Retry {
                safe_http_max_attempts: retry
                    .safe_http_max_attempts
                    .ok_or_else(|| missing("retry.safeHttpMaxAttempts"))?,
                control_max_attempts: retry
                    .control_max_attempts
                    .ok_or_else(|| missing("retry.controlMaxAttempts"))?,
                websocket_reconnect_max_attempts: retry
                    .websocket_reconnect_max_attempts
                    .ok_or_else(|| missing("retry.websocketReconnectMaxAttempts"))?,
            })
        })?;

    Ok(V3Profile {
        implementation,
        protocol_pack,
        protocol_version,
        pack_version,
        target,
        endpoint,
        auth,
        capabilities,
        evidence,
        retry,
    })
}

fn required_value(value: Option<String>, field: &str) -> Result<String, V3ValidationError> {
    let value = value.ok_or_else(|| missing(field))?;
    if value.trim().is_empty() {
        return Err(missing(field));
    }
    Ok(value)
}

fn validate_supported_value(
    field: &str,
    value: &str,
    supported: &str,
) -> Result<(), V3ValidationError> {
    if value != supported {
        return Err(V3ValidationError::UnsupportedProfileValue {
            field: field.to_string(),
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_endpoint(
    value: &str,
    field: &str,
    websocket: bool,
) -> Result<(), V3ValidationError> {
    if is_environment_reference(value) {
        return Ok(());
    }

    let expected = if websocket { "ws:// or wss://" } else { "http:// or https://" };
    let parsed = reqwest::Url::parse(value).map_err(|_| V3ValidationError::InvalidEndpoint {
        field: field.to_string(),
        expected: expected.to_string(),
        value: value.to_string(),
    })?;
    let scheme_valid = if websocket {
        matches!(parsed.scheme(), "ws" | "wss")
    } else {
        matches!(parsed.scheme(), "http" | "https")
    };
    if !scheme_valid || parsed.host_str().is_none() {
        return Err(V3ValidationError::InvalidEndpoint {
            field: field.to_string(),
            expected: expected.to_string(),
            value: value.to_string(),
        });
    }
    Ok(())
}

fn is_environment_reference(value: &str) -> bool {
    value.starts_with("${") && value.ends_with('}') && value.len() > 3
}

fn resolve_evidence_root(flow_dir: &Path, root: &str) -> Result<EvidenceRoot, V3ValidationError> {
    let root_path = Path::new(root);
    if root_path.is_absolute() || root.contains("${") {
        return Err(V3ValidationError::InvalidEvidenceRoot {
            root: root.to_string(),
        });
    }

    let mut relative = PathBuf::new();
    let mut relative_components = Vec::new();
    for component in root_path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                if !valid_evidence_component(value) {
                    return Err(V3ValidationError::InvalidEvidenceRoot {
                        root: root.to_string(),
                    });
                }
                relative.push(value);
                relative_components.push(value.to_os_string());
            }
            Component::ParentDir => {
                if !relative.pop() || relative_components.pop().is_none() {
                    return Err(V3ValidationError::InvalidEvidenceRoot {
                        root: root.to_string(),
                    });
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(V3ValidationError::InvalidEvidenceRoot {
                    root: root.to_string(),
                })
            }
        }
    }

    let lexical_flow_dir = absolute_lexical_path(flow_dir, root)?;
    reject_existing_reparse_components(&lexical_flow_dir, root)?;
    let authority = FlowRootAuthority::establish(&lexical_flow_dir).map_err(|_| {
        V3ValidationError::InvalidEvidenceRoot {
            root: root.to_string(),
        }
    })?;
    let base = authority.path().to_path_buf();
    let resolved = base.join(relative);
    if !resolved.starts_with(&base) {
        return Err(V3ValidationError::InvalidEvidenceRoot {
            root: root.to_string(),
        });
    }
    reject_existing_reparse_components(&resolved, root)?;
    let existing_ancestor = nearest_existing_ancestor(&resolved, root)?;
    let canonical_ancestor = std::fs::canonicalize(existing_ancestor).map_err(|_| {
        V3ValidationError::InvalidEvidenceRoot {
            root: root.to_string(),
        }
    })?;
    if !canonical_ancestor.starts_with(&base) {
        return Err(V3ValidationError::InvalidEvidenceRoot {
            root: root.to_string(),
        });
    }
    Ok(EvidenceRoot::new(authority, relative_components, resolved))
}

fn valid_evidence_component(value: &std::ffi::OsStr) -> bool {
    let Some(value) = value.to_str() else {
        return false;
    };
    !value.is_empty() && !value.contains('\0') && (!cfg!(windows) || !value.contains(':'))
}

fn absolute_lexical_path(path: &Path, root: &str) -> Result<PathBuf, V3ValidationError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(|_| V3ValidationError::InvalidEvidenceRoot {
            root: root.to_string(),
        })
}

fn nearest_existing_ancestor(path: &Path, root: &str) -> Result<PathBuf, V3ValidationError> {
    let mut candidate = path;
    loop {
        match std::fs::symlink_metadata(candidate) {
            Ok(_) => return Ok(candidate.to_path_buf()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                candidate = candidate.parent().ok_or_else(|| {
                    V3ValidationError::InvalidEvidenceRoot {
                        root: root.to_string(),
                    }
                })?;
            }
            Err(_) => {
                return Err(V3ValidationError::InvalidEvidenceRoot {
                    root: root.to_string(),
                })
            }
        }
    }
}

fn reject_existing_reparse_components(path: &Path, root: &str) -> Result<(), V3ValidationError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                    return Err(V3ValidationError::InvalidEvidenceRoot {
                        root: root.to_string(),
                    });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => {
                return Err(V3ValidationError::InvalidEvidenceRoot {
                    root: root.to_string(),
                })
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn missing(field: &str) -> V3ValidationError {
    V3ValidationError::MissingProfileField {
        field: field.to_string(),
    }
}

fn validate_redact_fields(fields: Vec<String>) -> Result<Vec<String>, V3ValidationError> {
    let mut normalized = Vec::with_capacity(fields.len());
    let mut seen = HashSet::new();

    for field in fields {
        let field = field.trim().to_ascii_lowercase();
        if field.is_empty() {
            return Err(missing("evidence.redactFields"));
        }
        if seen.insert(field.clone()) {
            normalized.push(field);
        }
    }

    for required in REQUIRED_REDACT_FIELDS {
        if !seen.contains(required) {
            return Err(missing(&format!("evidence.redactFields.{required}")));
        }
    }

    Ok(normalized)
}
