use super::{AdapterResult, AdapterStatus, CloudMockAdapter, TargetAdapter, TargetScope};
use crate::e2e;
use crate::parser::types::{ComponentSpec, TargetId, TestCommand};
use crate::runner::context::TestContext;
use crate::driver::traits::PlatformDriver;
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

#[derive(Default)]
pub struct AdapterRegistry;

impl AdapterRegistry {
    pub fn standard() -> Self {
        Self
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
            (TargetId::Gateway, "lumi_hc") => {
                Box::new(HcGatewayAdapter::new(
                    component.profile.clone(),
                    source_path.map(Path::to_path_buf),
                ))
            }
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
            component,
            driver,
            base_dir,
            output,
            false,
            false,
            false,
            false,
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
            Err(error) => AdapterResult::failed(
                "app_platform_driver",
                "AppCommandFailed",
                error.to_string(),
            ),
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
    runtime: Option<e2e::E2eRuntime>,
    context: Option<TestContext>,
}

impl HcGatewayAdapter {
    fn new(profile: Option<PathBuf>, source_path: Option<PathBuf>) -> Self {
        Self {
            capabilities: vec!["existing_hc_http_websocket".into()],
            profile,
            source_path,
            runtime: None,
            context: None,
        }
    }

    fn preflight_message(&self) -> String {
        let Some(profile_path) = self.profile.as_deref() else {
            return "real HC endpoint environment is unresolved: no profile was declared".into();
        };
        let Ok(content) = std::fs::read_to_string(profile_path) else {
            return format!(
                "real HC endpoint environment is unresolved for profile '{}'",
                profile_path.display()
            );
        };
        let Ok(profile) = serde_yaml::from_str::<serde_yaml::Value>(&content) else {
            return format!(
                "real HC endpoint environment is unresolved for malformed profile '{}'",
                profile_path.display()
            );
        };
        let endpoint = profile.get("endpoint");
        let base_url = endpoint
            .and_then(|value| value.get("baseUrl"))
            .and_then(serde_yaml::Value::as_str);
        let websocket_url = endpoint
            .and_then(|value| value.get("websocketUrl"))
            .and_then(serde_yaml::Value::as_str);
        if endpoint_value_is_unresolved(base_url) || endpoint_value_is_unresolved(websocket_url) {
            return format!(
                "real HC endpoint environment is unresolved for profile '{}'",
                profile_path.display()
            );
        }
        format!(
            "real HC endpoint environment from profile '{}' is unavailable in this session",
            profile_path.display()
        )
    }

    fn prepare_runtime(&mut self, context: &TestContext) -> AdapterResult {
        let Some(profile_path) = self.profile.as_deref() else {
            return AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                self.preflight_message(),
            );
        };
        let Some(source_path) = self.source_path.as_deref() else {
            return AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                "real HC endpoint environment is unresolved: flow source path is unavailable",
            );
        };
        let source_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
        let Ok(profile_ref) = profile_path.strip_prefix(source_dir) else {
            return AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                format!("HC profile '{}' is outside the flow directory", profile_path.display()),
            );
        };
        let profile_ref = profile_ref.to_string_lossy().replace('\\', "/");
        let loaded = match e2e::profile::load_profile(source_path, &profile_ref) {
            Ok(loaded) => loaded,
            Err(error) => {
                return AdapterResult::blocked(
                    self.adapter_name(),
                    "NoLab",
                    format!("{} ({error})", self.preflight_message()),
                )
            }
        };
        let base_url = context.substitute_vars(&loaded.profile.endpoint.base_url);
        let websocket_url = context.substitute_vars(&loaded.profile.endpoint.websocket_url);
        if base_url.contains("${") || websocket_url.contains("${") {
            return AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                "real HC endpoint environment is unresolved; real HC lab execution is blocked",
            );
        }
        match e2e::E2eRuntime::new(
            &loaded,
            base_url,
            websocket_url,
            std::time::Duration::from_millis(context.default_timeout_ms),
        ) {
            Ok(runtime) => {
                self.runtime = Some(runtime);
                let mut result = AdapterResult::passed(self.adapter_name());
                result.evidence_tier = Some("real_preflight".into());
                result
            }
            Err(error) => AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                format!("real HC runtime preflight is unavailable: {error}"),
            ),
        }
    }
}

fn endpoint_value_is_unresolved(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let Some(variable) = value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return false;
    };
    std::env::var(variable).map_or(true, |value| value.trim().is_empty())
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
    fn preflight(&mut self, _context: &TestContext) -> AdapterResult {
        if self.runtime.is_some() {
            let mut result = AdapterResult::passed(self.adapter_name());
            result.evidence_tier = Some("real_preflight".into());
            result
        } else {
            self.prepare_runtime(_context)
        }
    }
    fn open(&mut self, context: &TestContext) -> AdapterResult {
        self.preflight(context)
    }
    fn update_context(&mut self, context: &TestContext) {
        self.context = Some(context.clone());
    }
    fn execute(&mut self, command: &TestCommand, _scope: &TargetScope) -> AdapterResult {
        let Some(runtime) = self.runtime.as_mut() else {
            return AdapterResult::blocked(
                self.adapter_name(),
                "NoLab",
                "real HC execution is unavailable in this session",
            );
        };
        let TestCommand::E2e(command) = command else {
            return AdapterResult::failed(
                self.adapter_name(),
                "UnsupportedCapability",
                "HC adapter accepts E2E gateway commands only",
            );
        };
        let command = match substitute_e2e_command(command, self.context.as_ref()) {
            Ok(command) => command,
            Err(error) => {
                return AdapterResult::error(self.adapter_name(), error);
            }
        };
        let result = block_on_adapter(runtime.execute(&command));
        let status = match result.status {
            e2e::result::E2eStatus::Passed => AdapterStatus::Passed,
            e2e::result::E2eStatus::Failed => AdapterStatus::Failed,
            e2e::result::E2eStatus::Blocked => AdapterStatus::Blocked,
            e2e::result::E2eStatus::Error => AdapterStatus::Error,
        };
        let evidence_tier = serde_json::to_value(result.evidence_tier)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string));
        let category = serde_json::to_value(result.category)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "e2e".into());
        AdapterResult {
            status,
            adapter: self.adapter_name().into(),
            category,
            message: result.message,
            evidence_tier,
            raw_evidence: Some(serde_json::json!({
                "action": result.action,
                "readback": result.readback,
                "assertion": result.assertion,
                "evidence": result.evidence,
            })),
            normalized_evidence: result.normalized,
        }
    }
    fn close(&mut self, _context: &TestContext) -> AdapterResult {
        AdapterResult::passed(self.adapter_name())
    }
}

fn substitute_e2e_command(
    command: &crate::e2e::types::E2eCommand,
    context: Option<&TestContext>,
) -> Result<crate::e2e::types::E2eCommand, String> {
    let Some(context) = context else {
        return Ok(command.clone());
    };
    let mut value = serde_json::to_value(command).map_err(|error| error.to_string())?;
    substitute_json_strings(&mut value, context);
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn substitute_json_strings(value: &mut serde_json::Value, context: &TestContext) {
    match value {
        serde_json::Value::String(text) => *text = context.substitute_vars(text),
        serde_json::Value::Array(items) => {
            for item in items {
                substitute_json_strings(item, context);
            }
        }
        serde_json::Value::Object(fields) => {
            for field in fields.values_mut() {
                substitute_json_strings(field, context);
            }
        }
        _ => {}
    }
}
