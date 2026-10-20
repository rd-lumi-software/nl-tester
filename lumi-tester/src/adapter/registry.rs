use super::{AdapterResult, AdapterStatus, CloudMockAdapter, TargetAdapter, TargetScope};
use crate::driver::traits::PlatformDriver;
use crate::hc::{
    HcBackendDescriptor, HcBackendRegistry, HcBackendResult, HcCommand, HcEvidence,
    HcExecutionScope, HcProtocolBackend, HcStateUpdate, HcStatus,
};
use crate::parser::types::{ComponentSpec, TargetId, TestCommand};
use crate::runner::context::TestContext;
use crate::runner::executor::TestExecutor;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterRegistryError {
    #[error("platform '{platform}' is unsupported for target '{target}'")]
    UnsupportedTargetPlatform { target: TargetId, platform: String },
}

impl AdapterRegistryError {
    pub fn code(&self) -> &'static str {
        "UnsupportedTargetPlatform"
    }
}

pub struct AdapterRegistry {
    hc_backends: HcBackendRegistry,
}

impl AdapterRegistry {
    /// Construct an adapter registry with an explicitly supplied HC factory.
    pub fn with_hc_backend_registry(hc_backends: HcBackendRegistry) -> Self {
        Self { hc_backends }
    }

    pub fn standard() -> Self {
        Self::with_hc_backend_registry(HcBackendRegistry::new())
    }

    pub fn create(
        &self,
        target: TargetId,
        component: &ComponentSpec,
    ) -> Result<Box<dyn TargetAdapter>, AdapterRegistryError> {
        self.create_with_source(target, component, None)
    }

    pub fn create_with_source(
        &self,
        target: TargetId,
        component: &ComponentSpec,
        source_path: Option<&Path>,
    ) -> Result<Box<dyn TargetAdapter>, AdapterRegistryError> {
        let adapter: Box<dyn TargetAdapter> = match (target, component.platform.as_str()) {
            (TargetId::App, "android" | "android_auto" | "ios" | "web" | "macos" | "windows") => {
                Box::new(AppDriverAdapter::new(component.platform.clone()))
            }
            (TargetId::Gateway, "lumi_hc") => Box::new(HcGatewayAdapter::new(
                component.profile.clone(),
                source_path.map(Path::to_path_buf),
                self.hc_backends.clone(),
            )),
            (TargetId::Cloud, "lumi_cloud") if cloud_profile_is_real(component) => {
                Box::new(UnavailableCloudAdapter::new())
            }
            (TargetId::Cloud, "lumi_cloud") => Box::new(CloudMockAdapter::new()),
            _ => {
                return Err(AdapterRegistryError::UnsupportedTargetPlatform {
                    target,
                    platform: component.platform.clone(),
                })
            }
        };
        Ok(adapter)
    }

    pub fn create_app_with_driver(
        &self,
        component: &ComponentSpec,
        driver: Box<dyn PlatformDriver>,
        base_dir: &Path,
        output: &Path,
    ) -> Result<Box<dyn TargetAdapter>, AdapterRegistryError> {
        self.create_app_with_driver_with_options(
            component, driver, base_dir, output, false, false, false, false,
        )
    }

    pub fn create_app_with_driver_with_options(
        &self,
        component: &ComponentSpec,
        driver: Box<dyn PlatformDriver>,
        base_dir: &Path,
        output: &Path,
        continue_on_failure: bool,
        snapshot: bool,
        report: bool,
        _events_jsonl: bool,
    ) -> Result<Box<dyn TargetAdapter>, AdapterRegistryError> {
        if component.target != TargetId::App
            || !matches!(
                component.platform.as_str(),
                "android" | "android_auto" | "ios" | "web" | "macos" | "windows"
            )
        {
            return Err(AdapterRegistryError::UnsupportedTargetPlatform {
                target: component.target,
                platform: component.platform.clone(),
            });
        }
        Ok(Box::new(AppDriverAdapter::with_driver(
            component,
            driver,
            base_dir,
            output,
            continue_on_failure,
            snapshot,
            report,
            false,
        )))
    }
}

fn cloud_profile_is_real(component: &ComponentSpec) -> bool {
    let Some(profile) = component.profile.as_deref() else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string(profile) else {
        return false;
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&content) else {
        return false;
    };
    value.get("mode").and_then(serde_yaml::Value::as_str) == Some("real")
        || value.get("mock").and_then(serde_yaml::Value::as_bool) == Some(false)
}

struct UnavailableCloudAdapter {
    capabilities: Vec<String>,
}

impl UnavailableCloudAdapter {
    fn new() -> Self {
        Self {
            capabilities: Vec::new(),
        }
    }

    fn blocked(&self) -> AdapterResult {
        AdapterResult::blocked(
            "cloud_unavailable",
            "UnsupportedAdapter",
            "real Cloud adapter is not implemented; use an explicit mock profile",
        )
    }
}

impl TargetAdapter for UnavailableCloudAdapter {
    fn target(&self) -> TargetId {
        TargetId::Cloud
    }
    fn platform(&self) -> &str {
        "lumi_cloud"
    }
    fn adapter_name(&self) -> &str {
        "cloud_unavailable"
    }
    fn capabilities(&self) -> &[String] {
        &self.capabilities
    }
    fn preflight(&mut self, _context: &TestContext) -> AdapterResult {
        self.blocked()
    }
    fn open(&mut self, _context: &TestContext) -> AdapterResult {
        self.blocked()
    }
    fn execute(&mut self, _command: &TestCommand, _scope: &TargetScope) -> AdapterResult {
        self.blocked()
    }
    fn close(&mut self, _context: &TestContext) -> AdapterResult {
        AdapterResult::passed(self.adapter_name())
    }
}

struct AppDriverAdapter {
    platform: String,
    capabilities: Vec<String>,
    executor: Option<TestExecutor>,
}

impl AppDriverAdapter {
    fn new(platform: String) -> Self {
        Self {
            platform,
            capabilities: vec!["existing_platform_driver".into()],
            executor: None,
        }
    }

    fn with_driver(
        component: &ComponentSpec,
        driver: Box<dyn PlatformDriver>,
        base_dir: &Path,
        output: &Path,
        continue_on_failure: bool,
        snapshot: bool,
        report: bool,
        _events_jsonl: bool,
    ) -> Self {
        let mut executor = TestExecutor::new_with_events(
            driver,
            Some(output),
            continue_on_failure,
            false,
            snapshot,
            report,
            None,
            // The aggregate multi-target writer owns events.jsonl. Keeping the
            // delegated App executor listener disabled avoids two writers racing
            // on the same artifact while preserving the CLI flag at aggregate level.
            false,
        );
        executor.context.base_dir = base_dir.to_path_buf();
        executor.context.app_id = component.app_id.clone();
        Self {
            platform: component.platform.clone(),
            capabilities: vec!["existing_platform_driver".into()],
            executor: Some(executor),
        }
    }
}

impl TargetAdapter for AppDriverAdapter {
    fn target(&self) -> TargetId {
        TargetId::App
    }
    fn platform(&self) -> &str {
        &self.platform
    }
    fn adapter_name(&self) -> &str {
        "app_platform_driver"
    }
    fn capabilities(&self) -> &[String] {
        &self.capabilities
    }
    fn preflight(&mut self, _context: &TestContext) -> AdapterResult {
        if self.executor.is_some() {
            let mut result = AdapterResult::passed(self.adapter_name());
            result.evidence_tier = Some("real_preflight".into());
            result
        } else {
            AdapterResult::blocked(
                self.adapter_name(),
                "DriverNotAttached",
                "App adapter requires the existing runner PlatformDriver",
            )
        }
    }
    fn open(&mut self, context: &TestContext) -> AdapterResult {
        self.preflight(context)
    }
    fn update_context(&mut self, context: &TestContext) {
        if let Some(executor) = self.executor.as_mut() {
            executor.context.vars = context.vars.clone();
            executor.context.env = context.env.clone();
            executor.context.app_id = context.app_id.clone();
            executor.context.url = context.url.clone();
            executor.context.default_timeout_ms = context.default_timeout_ms;
        }
    }
    fn execute(&mut self, command: &TestCommand, scope: &TargetScope) -> AdapterResult {
        let Some(executor) = self.executor.as_mut() else {
            return AdapterResult::blocked(
                self.adapter_name(),
                "DriverNotAttached",
                "App execution remains owned by the existing PlatformDriver",
            );
        };
        match block_on_adapter(executor.execute_command_with_failure_artifacts(
            command,
            "multi-target",
            scope.command_index,
        )) {
            Ok(()) => {
                let mut result = AdapterResult::passed("app_platform_driver");
                result.evidence_tier = Some("real_e2e".into());
                result.normalized_evidence = Some(serde_json::json!({
                    "source": "app_driver",
                    "command": command.display_name(),
                }));
                result
            }
            Err(error) => {
                AdapterResult::failed("app_platform_driver", "AppCommandFailed", error.to_string())
            }
        }
    }
    fn close(&mut self, _context: &TestContext) -> AdapterResult {
        if let Some(executor) = self.executor.as_mut() {
            if let Err(error) = block_on_adapter(executor.finish_without_artifacts()) {
                return AdapterResult::error("app_platform_driver", error.to_string());
            }
        }
        AdapterResult::passed(self.adapter_name())
    }
}

fn block_on_adapter<F: std::future::Future>(future: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        tokio::runtime::Runtime::new()
            .expect("adapter runtime")
            .block_on(future)
    }
}

struct HcGatewayAdapter {
    capabilities: Vec<String>,
    profile: Option<PathBuf>,
    source_path: Option<PathBuf>,
    backend_registry: HcBackendRegistry,
    backend: Option<Box<dyn HcProtocolBackend>>,
    backend_identity: Option<HcBackendDescriptor>,
    context: Option<TestContext>,
}

impl HcGatewayAdapter {
    fn new(
        profile: Option<PathBuf>,
        source_path: Option<PathBuf>,
        backend_registry: HcBackendRegistry,
    ) -> Self {
        Self {
            capabilities: vec!["existing_hc_http_websocket".into()],
            profile,
            source_path,
            backend_registry,
            backend: None,
            backend_identity: None,
            context: None,
        }
    }

    fn select_backend(&mut self) -> Result<(), AdapterResult> {
        let Some(profile_path) = self.profile.as_deref() else {
            return Err(AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                "HC profile is required",
            ));
        };
        let Some(source_path) = self.source_path.as_deref() else {
            return Err(AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                "real HC endpoint environment is unresolved: flow source path is unavailable",
            ));
        };
        let source_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
        let Ok(profile_ref) = profile_path.strip_prefix(source_dir) else {
            return Err(AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                format!(
                    "HC profile '{}' is outside the flow directory",
                    profile_path.display()
                ),
            ));
        };
        let profile_ref = profile_ref.to_string_lossy().replace('\\', "/");
        let config = match crate::hc::profile::load_profile(source_path, &profile_ref) {
            Ok(config) => config,
            Err(error) => {
                return Err(AdapterResult::blocked(
                    self.adapter_name(),
                    "NoLab",
                    error.to_string(),
                ))
            }
        };
        if self.backend.is_none() {
            let descriptor = match self.backend_registry.resolve(&config) {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    return Err(AdapterResult::blocked(
                        self.adapter_name(),
                        "NoLab",
                        format!("real HC backend preflight is unavailable: {error}"),
                    ))
                }
            };
            self.backend = match self.backend_registry.create(&config) {
                Ok(backend) => Some(backend),
                Err(error) => {
                    return Err(AdapterResult::blocked(
                        self.adapter_name(),
                        "NoLab",
                        format!("real HC backend preflight is unavailable: {error}"),
                    ))
                }
            };
            self.backend_identity = Some(descriptor);
        }
        Ok(())
    }
}

impl TargetAdapter for HcGatewayAdapter {
    fn target(&self) -> TargetId {
        TargetId::Gateway
    }
    fn platform(&self) -> &str {
        "lumi_hc"
    }
    fn adapter_name(&self) -> &str {
        "lumi_hc"
    }
    fn capabilities(&self) -> &[String] {
        &self.capabilities
    }
    fn preflight(&mut self, context: &TestContext) -> AdapterResult {
        if let Err(result) = self.select_backend() {
            return result;
        }
        let result = self
            .backend
            .as_mut()
            .expect("HC backend is selected before lifecycle preflight")
            .preflight(context);
        adapter_result_from_hc(self.adapter_name(), result, self.backend_identity.as_ref())
    }
    fn open(&mut self, context: &TestContext) -> AdapterResult {
        if let Err(result) = self.select_backend() {
            return result;
        }
        let result = self
            .backend
            .as_mut()
            .expect("HC backend is selected before lifecycle open")
            .open(context);
        adapter_result_from_hc(self.adapter_name(), result, self.backend_identity.as_ref())
    }
    fn update_context(&mut self, context: &TestContext) {
        self.context = Some(context.clone());
    }
    fn execute(&mut self, command: &TestCommand, scope: &TargetScope) -> AdapterResult {
        let adapter_name = self.adapter_name().to_string();
        let Some(backend) = self.backend.as_mut() else {
            return AdapterResult::blocked(
                &adapter_name,
                "NoLab",
                "real HC execution is unavailable in this session",
            );
        };
        let TestCommand::V3(command) = command else {
            return AdapterResult::failed(
                &adapter_name,
                "UnsupportedCapability",
                "HC adapter accepts V3 gateway commands only",
            );
        };
        let command = hc_command_from_v3(command);
        let command = match substitute_hc_command(&command, self.context.as_ref()) {
            Ok(command) => command,
            Err(error) => {
                return AdapterResult::error(&adapter_name, error);
            }
        };
        let scope = HcExecutionScope::new(
            scope.depth,
            scope.command_index,
            scope.correlation_id.clone(),
        );
        let result = backend.execute(&command, &scope);
        adapter_result_from_hc(&adapter_name, result, self.backend_identity.as_ref())
    }
    fn close(&mut self, context: &TestContext) -> AdapterResult {
        let result = match self.backend.as_mut() {
            Some(backend) => backend.close(context),
            None => HcBackendResult::passed("HC backend was not opened"),
        };
        adapter_result_from_hc(self.adapter_name(), result, self.backend_identity.as_ref())
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

fn hc_command_from_v3(
    command: &crate::hc::backends::v3::runtime::types::V3Command,
) -> HcCommand {
    use crate::hc::backends::v3::runtime::types::V3Command;

    match command {
        V3Command::HcHealth => HcCommand::Health,
        V3Command::HcGetEntities => HcCommand::GetEntities,
        V3Command::HcGetState(params) => HcCommand::GetState {
            entity: params.entity.clone(),
        },
        V3Command::HcControl(params) => HcCommand::Control {
            entity: params.entity.clone(),
            updates: hc_state_updates(&params.state),
        },
        V3Command::HcWaitState(params) => HcCommand::WaitState {
            entity: params.entity.clone(),
            expected: hc_state_updates(&params.state),
        },
        V3Command::WsSubscribe(params) => HcCommand::RealtimeSubscribe {
            channels: params.channels.clone(),
        },
        V3Command::WsExpect(params) => HcCommand::RealtimeExpect {
            entity: params.entity.clone(),
            event: serde_json::to_value(&params.event).unwrap_or(serde_json::Value::Null),
        },
        V3Command::WsSnapshot(params) => HcCommand::RealtimeSnapshot {
            entity: params.entity.clone(),
        },
    }
}

fn hc_state_updates(
    values: &[crate::hc::backends::v3::runtime::types::V3IndexedValue],
) -> Vec<HcStateUpdate> {
    values
        .iter()
        .map(|value| HcStateUpdate::Indexed {
            idx: value.idx,
            value: serde_json::to_value(&value.value).unwrap_or(serde_json::Value::Null),
        })
        .collect()
}

fn adapter_result_from_hc(
    adapter: &str,
    result: HcBackendResult,
    backend_identity: Option<&HcBackendDescriptor>,
) -> AdapterResult {
    let status = match result.status {
        HcStatus::Passed => AdapterStatus::Passed,
        HcStatus::Failed => AdapterStatus::Failed,
        HcStatus::Blocked => AdapterStatus::Blocked,
        HcStatus::Error => AdapterStatus::Error,
    };
    let evidence_tier = serde_json::to_value(result.evidence_tier)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string));
    let mut raw_evidence = serde_json::json!({
        "action": result.action,
        "readback": result.readback,
        "assertion": result.assertion,
        "evidence": result
            .evidence
            .into_iter()
            .map(hc_evidence_payload)
            .collect::<Vec<_>>(),
    });
    if let Some(identity) = backend_identity {
        raw_evidence["backend"] = serde_json::json!({
            "implementation": identity.implementation_id,
            "protocolPack": identity.protocol_pack,
            "protocolVersion": identity.protocol_version,
            "packVersion": identity.pack_version,
        });
    }
    AdapterResult {
        status,
        adapter: adapter.into(),
        category: result.category,
        message: result.message,
        evidence_tier,
        raw_evidence: Some(raw_evidence),
        normalized_evidence: result.normalized,
    }
}

fn hc_evidence_payload(evidence: HcEvidence) -> serde_json::Value {
    let HcEvidence { kind, raw } = evidence;
    match raw {
        serde_json::Value::Object(mut fields) => {
            fields.insert("kind".into(), serde_json::Value::String(kind));
            serde_json::Value::Object(fields)
        }
        raw => serde_json::json!({
            "kind": kind,
            "raw": raw,
        }),
    }
}

fn substitute_hc_command(
    command: &crate::hc::HcCommand,
    context: Option<&TestContext>,
) -> Result<crate::hc::HcCommand, String> {
    let Some(context) = context else {
        return Ok(command.clone());
    };
    use crate::hc::{HcCommand, HcStateUpdate};

    let substitute_updates = |updates: &[HcStateUpdate]| {
        updates
            .iter()
            .map(|update| match update {
                HcStateUpdate::Indexed { idx, value } => {
                    let mut value = value.clone();
                    substitute_json_value(&mut value, context);
                    HcStateUpdate::Indexed {
                        idx: *idx,
                        value,
                    }
                }
                HcStateUpdate::Capability { name, value } => {
                    let mut value = value.clone();
                    substitute_json_value(&mut value, context);
                    HcStateUpdate::Capability {
                        name: context.substitute_vars(name),
                        value,
                    }
                }
            })
            .collect::<Vec<_>>()
    };

    Ok(match command {
        HcCommand::Health => HcCommand::Health,
        HcCommand::GetEntities => HcCommand::GetEntities,
        HcCommand::GetState { entity } => HcCommand::GetState {
            entity: context.substitute_vars(entity),
        },
        HcCommand::Control { entity, updates } => HcCommand::Control {
            entity: context.substitute_vars(entity),
            updates: substitute_updates(updates),
        },
        HcCommand::WaitState { entity, expected } => HcCommand::WaitState {
            entity: context.substitute_vars(entity),
            expected: substitute_updates(expected),
        },
        HcCommand::RealtimeSubscribe { channels } => HcCommand::RealtimeSubscribe {
            channels: channels
                .iter()
                .map(|channel| context.substitute_vars(channel))
                .collect(),
        },
        HcCommand::RealtimeExpect { entity, event } => {
            let mut event = event.clone();
            substitute_json_value(&mut event, context);
            HcCommand::RealtimeExpect {
                entity: context.substitute_vars(entity),
                event,
            }
        }
        HcCommand::RealtimeSnapshot { entity } => HcCommand::RealtimeSnapshot {
            entity: context.substitute_vars(entity),
        },
    })
}

fn substitute_json_value(value: &mut serde_json::Value, context: &TestContext) {
    match value {
        serde_json::Value::String(text) => {
            let original = text.clone();
            let substituted = context.substitute_vars(&original);
            let whole_placeholder = original.starts_with("${") && original.ends_with('}')
                && original[2..original.len() - 1]
                    .chars()
                    .all(|character| character != '$' && character != '{' && character != '}');
            if whole_placeholder {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&substituted) {
                    *value = parsed;
                } else {
                    *text = substituted;
                }
            } else {
                *text = substituted;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                substitute_json_value(item, context);
            }
        }
        serde_json::Value::Object(fields) => {
            for field in fields.values_mut() {
                substitute_json_value(field, context);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod typed_value_tests {
    use super::*;
    use crate::hc::HcStateUpdate;
    use serde_json::json;

    #[test]
    fn whole_hc_placeholders_keep_json_types_but_embedded_text_stays_string() {
        let mut context = TestContext::new(Path::new("."), None, false, None);
        context.set_var("enabled", "true");
        context.set_var("level", "42");
        context.set_var("deviceId", "123");
        context.set_var("payload", r#"{"mode":"eco","limit":3}"#);
        let command = crate::hc::HcCommand::Control {
            entity: "${deviceId}".to_string(),
            updates: vec![
                HcStateUpdate::Indexed {
                    idx: 0,
                    value: json!("${enabled}"),
                },
                HcStateUpdate::Indexed {
                    idx: 1,
                    value: json!("${level}"),
                },
                HcStateUpdate::Indexed {
                    idx: 2,
                    value: json!("${payload}"),
                },
            ],
        };

        let substituted = substitute_hc_command(&command, Some(&context)).unwrap();
        let crate::hc::HcCommand::Control { entity, updates } = substituted else {
            panic!("expected control command");
        };
        assert_eq!(entity, "123");
        assert_eq!(
            updates,
            vec![
                HcStateUpdate::Indexed {
                    idx: 0,
                    value: json!(true),
                },
                HcStateUpdate::Indexed {
                    idx: 1,
                    value: json!(42),
                },
                HcStateUpdate::Indexed {
                    idx: 2,
                    value: json!({"mode": "eco", "limit": 3}),
                },
            ]
        );
    }
}
