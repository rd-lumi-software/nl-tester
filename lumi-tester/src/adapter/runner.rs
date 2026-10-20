use super::{
    AdapterRegistry, AdapterResult, AdapterStatus, CloudMockAdapter, SessionEvent, TargetAdapter,
    TargetScope, TargetSession,
};
use crate::parser::flow::{child_flow_path, run_flow_params, uses_inline_flow};
use crate::parser::types::{NormalizedFlow, TargetId, TestCommand};
use crate::parser::validation::validate_flow_graph;
use crate::report::types::{
    HcReadbackReport, HcReportMetadata, MultiTargetOperationReport, MultiTargetReportMetadata,
    TestResults,
};
use crate::runner::context::TestContext;
use crate::runner::state::{
    CommandStateReport, CommandStatus, FlowStateReport, FlowStatus, TestSummary,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub session_id: String,
    pub target: TargetId,
    pub platform: String,
    pub adapter: String,
    pub command: String,
    pub command_index: usize,
    #[serde(default)]
    pub command_path: Vec<usize>,
    pub depth: usize,
    pub correlation_id: String,
    #[serde(default)]
    pub repeat_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<PathBuf>,
    pub result: AdapterResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiTargetRunReport {
    pub schema_version: u32,
    pub session_id: String,
    pub status: AdapterStatus,
    #[serde(default)]
    pub passed: usize,
    #[serde(default)]
    pub failed: usize,
    #[serde(default)]
    pub blocked: usize,
    #[serde(default)]
    pub errors: usize,
    #[serde(default = "default_repeat_count")]
    pub repeat_count: u32,
    #[serde(default)]
    pub operations: Vec<OperationRecord>,
    #[serde(default)]
    pub lifecycle: Vec<SessionEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct MultiTargetRunOptions {
    pub continue_on_failure: bool,
    pub snapshot: bool,
    pub report: bool,
    pub events_jsonl: bool,
    pub command_index: Option<usize>,
    pub from_command_index: Option<usize>,
    pub command_name: Option<String>,
    pub repeat: u32,
    pub data_records: Vec<HashMap<String, String>>,
}

pub struct MultiTargetRunner {
    flow: NormalizedFlow,
    session: TargetSession,
    context: TestContext,
    operations: Vec<OperationRecord>,
    sequence: u64,
    options: MultiTargetRunOptions,
    stopped: bool,
    selected_root_index: Option<usize>,
    current_repeat_index: usize,
}

fn default_repeat_count() -> u32 {
    1
}

impl MultiTargetRunner {
    pub fn standard(flow: NormalizedFlow) -> anyhow::Result<Self> {
        Self::standard_with_adapter_registry(flow, AdapterRegistry::standard())
    }

    /// Build a runner with an explicitly supplied adapter and HC backend registry.
    pub fn standard_with_adapter_registry(
        flow: NormalizedFlow,
        registry: AdapterRegistry,
    ) -> anyhow::Result<Self> {
        validate_flow_graph(&flow)?;
        let adapters = flow
            .components
            .iter()
            .map(|(target, component)| {
                registry
                    .create_with_source(*target, component, Some(&flow.source_path))
                    .map_err(anyhow::Error::from)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let base_dir = flow
            .source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Ok(Self {
            flow,
            session: TargetSession::from_adapters(adapters)?,
            context: TestContext::new(&base_dir, None, false, None),
            operations: Vec::new(),
            sequence: 0,
            options: MultiTargetRunOptions::default(),
            stopped: false,
            selected_root_index: None,
            current_repeat_index: 0,
        })
    }

    pub fn standard_with_app_driver(
        flow: NormalizedFlow,
        driver: Box<dyn crate::driver::traits::PlatformDriver>,
        output: &Path,
    ) -> anyhow::Result<Self> {
        Self::standard_with_app_driver_and_options(
            flow,
            driver,
            output,
            MultiTargetRunOptions::default(),
        )
    }

    pub fn standard_with_app_driver_and_options(
        flow: NormalizedFlow,
        driver: Box<dyn crate::driver::traits::PlatformDriver>,
        output: &Path,
        options: MultiTargetRunOptions,
    ) -> anyhow::Result<Self> {
        validate_flow_graph(&flow)?;
        let registry = AdapterRegistry::standard();
        let base_dir = flow
            .source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut driver = Some(driver);
        let adapters = flow
            .components
            .iter()
            .map(|(target, component)| {
                if *target == TargetId::App {
                    registry
                        .create_app_with_driver_with_options(
                            component,
                            driver.take().expect("one app component"),
                            &base_dir,
                            output,
                            options.continue_on_failure,
                            options.snapshot,
                            options.report,
                            options.events_jsonl,
                        )
                        .map_err(anyhow::Error::from)
                } else {
                    registry
                        .create_with_source(*target, component, Some(&flow.source_path))
                        .map_err(anyhow::Error::from)
                }
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            flow,
            session: TargetSession::from_adapters(adapters)?,
            context: TestContext::new(&base_dir, Some(output), false, None),
            operations: Vec::new(),
            sequence: 0,
            options,
            stopped: false,
            selected_root_index: None,
            current_repeat_index: 0,
        })
    }

    pub fn mock_harness(flow: NormalizedFlow) -> anyhow::Result<Self> {
        validate_flow_graph(&flow)?;
        let world = Arc::new(Mutex::new(MockWorld::default()));
        let mut adapters: Vec<Box<dyn TargetAdapter>> = Vec::new();
        for target in flow.components.keys() {
            match target {
                TargetId::App => adapters.push(Box::new(MockAppAdapter::new(world.clone()))),
                TargetId::Gateway => {
                    adapters.push(Box::new(MockGatewayAdapter::new(world.clone())))
                }
                TargetId::Cloud => adapters.push(Box::new(CloudMockAdapter::new())),
            }
        }
        let base_dir = flow
            .source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Ok(Self {
            flow,
            session: TargetSession::from_adapters(adapters)?,
            context: TestContext::new(&base_dir, None, false, None),
            operations: Vec::new(),
            sequence: 0,
            options: MultiTargetRunOptions::default(),
            stopped: false,
            selected_root_index: None,
            current_repeat_index: 0,
        })
    }

    pub fn with_options(mut self, options: MultiTargetRunOptions) -> Self {
        self.options = options;
        self
    }

    pub fn run(&mut self) -> anyhow::Result<MultiTargetRunReport> {
        let sole_target = if self.flow.components.len() == 1 {
            self.flow.components.keys().next().copied()
        } else {
            None
        };
        let commands = self.flow.commands.clone();
        let source = self.flow.source_path.clone();
        if let Some(index) = self.options.command_index {
            if index >= commands.len() {
                anyhow::bail!(
                    "command index {index} is out of range ({} commands)",
                    commands.len()
                );
            }
            self.selected_root_index = Some(index);
        } else if let Some(index) = self.options.from_command_index {
            if index >= commands.len() {
                anyhow::bail!(
                    "from command index {index} is out of range ({} commands)",
                    commands.len()
                );
            }
        } else if let Some(name) = self.options.command_name.as_deref() {
            self.selected_root_index = commands.iter().position(|command| {
                command
                    .display_name()
                    .to_ascii_lowercase()
                    .starts_with(&name.to_ascii_lowercase())
            });
            if self.selected_root_index.is_none() {
                anyhow::bail!("command name '{name}' was not found in the root flow");
            }
        }
        let repeat = self.options.repeat.max(1);
        let records = if self.options.data_records.is_empty() {
            vec![None]
        } else {
            self.options
                .data_records
                .clone()
                .into_iter()
                .map(Some)
                .collect()
        };
        let run_result = records
            .into_iter()
            .try_for_each(|record| {
                if let Some(record) = record {
                    self.context.merge_vars(&record);
                }
                (0..repeat).try_for_each(|repeat_index| {
                    if self.stopped {
                        return Ok(());
                    }
                    self.current_repeat_index = repeat_index as usize;
                    self.execute_commands(&commands, &source, sole_target, 0, None, &[])
                })
            });
        let close_results = self.session.close_all(&self.context);
        run_result?;

        let passed = count_status(&self.operations, AdapterStatus::Passed);
        let failed = count_status(&self.operations, AdapterStatus::Failed);
        let blocked = count_status(&self.operations, AdapterStatus::Blocked);
        let errors = count_status(&self.operations, AdapterStatus::Error);
        let lifecycle_error = self
            .session
            .events()
            .iter()
            .any(|event| event.status == AdapterStatus::Error);
        let lifecycle_failed = self
            .session
            .events()
            .iter()
            .any(|event| matches!(event.status, AdapterStatus::Failed))
            || close_results
                .iter()
                .any(|result| result.status == AdapterStatus::Failed);
        let lifecycle_blocked = close_results
            .iter()
            .any(|result| result.status == AdapterStatus::Blocked);
        let status = if errors > 0 || lifecycle_error {
            AdapterStatus::Error
        } else if failed > 0 || lifecycle_failed {
            AdapterStatus::Failed
        } else if blocked > 0 || lifecycle_blocked {
            AdapterStatus::Blocked
        } else {
            AdapterStatus::Passed
        };
        Ok(MultiTargetRunReport {
            schema_version: 2,
            session_id: self.session.session_id().to_string(),
            status,
            passed,
            failed,
            blocked,
            errors,
            repeat_count: repeat,
            operations: self.operations.clone(),
            lifecycle: self.session.events().to_vec(),
        })
    }

    pub fn write_artifacts(report: &MultiTargetRunReport, output: &Path) -> anyhow::Result<()> {
        Self::write_artifacts_with_options(report, output, true, true)
    }

    pub fn write_artifacts_with_options(
        report: &MultiTargetRunReport,
        output: &Path,
        report_enabled: bool,
        events_enabled: bool,
    ) -> anyhow::Result<()> {
        std::fs::create_dir_all(output)?;
        let generated_at = chrono::Utc::now().to_rfc3339();
        let test_results = multi_target_test_results(report, generated_at.clone());
        let mut artifact = serde_json::to_value(report)?;
        let mut flows = serde_json::to_value(&test_results.flows)?;
        if let Some(flows) = flows.as_array_mut() {
            for (flow, operation) in flows.iter_mut().zip(&report.operations) {
                if let Some(evidence_tier) = operation.result.evidence_tier.as_ref() {
                    flow["evidenceTier"] = serde_json::json!(evidence_tier);
                }
            }
        }
        artifact["flows"] = flows;
        artifact["summary"] = serde_json::to_value(&test_results.summary)?;
        artifact["generatedAt"] = serde_json::json!(generated_at);
        artifact["multiTarget"] = serde_json::to_value(&test_results.multi_target)?;
        let pretty = serde_json::to_string_pretty(&artifact)?;
        std::fs::write(output.join("run.json"), &pretty)?;
        if report_enabled {
            std::fs::write(output.join("test-results.json"), &pretty)?;
        }
        if events_enabled {
            let mut events = String::new();
            for event in &report.lifecycle {
                events.push_str(&serde_json::to_string(&json!({
                    "type": "targetLifecycle",
                    "event": "targetLifecycle",
                    "sessionId": event.session_id,
                    "phase": event.phase,
                    "target": event.target,
                    "component": event.target,
                    "adapter": event.adapter,
                    "status": event.status,
                    "correlationId": event.correlation_id,
                }))?);
                events.push('\n');
            }
            for operation in &report.operations {
                events.push_str(&serde_json::to_string(&json!({
                    "type": "commandFinished",
                    "event": "commandFinished",
                    "sessionId": operation.session_id,
                    "target": operation.target,
                    "adapter": operation.adapter,
                    "profile": operation.profile,
                    "resultStatus": operation.result.status,
                    "evidenceTier": operation.result.evidence_tier,
                    "correlationId": operation.correlation_id,
                    "command": operation.command,
                    "commandIndex": operation.command_index,
                    "commandPath": operation.command_path,
                }))?);
                events.push('\n');
            }
            std::fs::write(output.join("events.jsonl"), events)?;
        }
        if report_enabled {
            let html = crate::report::summary_html::render_summary_html(
                &test_results,
                None,
                Some("multi-target"),
                Some("Lumi multi-target report"),
                None,
                output,
            );
            std::fs::write(output.join("report.html"), html)?;
            std::fs::write(
                output.join("junit.xml"),
                format!("<testsuite name=\"multi-target\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\"></testsuite>", report.operations.len(), report.failed, report.errors, report.blocked),
            )?;
        }
        Ok(())
    }

    fn execute_commands(
        &mut self,
        commands: &[TestCommand],
        source: &Path,
        active_target: Option<TargetId>,
        depth: usize,
        root_command_index: Option<usize>,
        parent_path: &[usize],
    ) -> anyhow::Result<()> {
        for (command_index, command) in commands.iter().enumerate() {
            if self.stopped {
                return Ok(());
            }
            if depth == 0 && !self.root_command_selected(command, command_index) {
                continue;
            }
            if depth > 0
                && self.options.command_name.as_deref().is_some_and(|name| {
                    !matches!(command, TestCommand::RunFlow(_))
                        && !command
                            .display_name()
                            .to_ascii_lowercase()
                            .starts_with(&name.to_ascii_lowercase())
                })
            {
                continue;
            }
            let mut command_path = parent_path.to_vec();
            command_path.push(command_index);
            if let TestCommand::RunFlow(params) = command {
                let params = run_flow_params(params);
                if let Some(condition) = params.when.as_ref() {
                    if !run_flow_condition_is_true(condition) {
                        continue;
                    }
                }
                let target = params.target.or(active_target).ok_or_else(|| {
                    anyhow::anyhow!("runFlow command at index {command_index} has no target")
                })?;
                if let Some(vars) = params.vars.as_ref() {
                    self.context.merge_vars(vars);
                }
                let child_result = (|| {
                    if uses_inline_flow(&params) {
                        let inline = params.commands.as_deref().expect("inline flow present");
                        self.execute_commands(
                            inline,
                            source,
                            Some(target),
                            depth + 1,
                            root_command_index.or(Some(command_index)),
                            &command_path,
                        )?;
                    } else if let Some(path) = params.path.as_deref() {
                        let path = self.context.substitute_vars(path);
                        let child_path = child_flow_path(source, &path);
                        if !child_path.exists() {
                            if params.optional.unwrap_or(false) {
                                return Ok::<(), anyhow::Error>(());
                            }
                            anyhow::bail!("Failed to read file: {}", child_path.display());
                        }
                        let child = crate::parser::yaml::parse_test_file(&child_path)?;
                        self.execute_commands(
                            &child.commands,
                            &child_path,
                            Some(target),
                            depth + 1,
                            root_command_index.or(Some(command_index)),
                            &command_path,
                        )?;
                    }
                    Ok::<(), anyhow::Error>(())
                })();
                if let Err(error) = child_result {
                    if !params.optional.unwrap_or(false) {
                        return Err(error);
                    }
                }
                continue;
            }

            let target = active_target.ok_or_else(|| {
                anyhow::anyhow!("command at index {command_index} has no target scope")
            })?;
            let component = &self.flow.components[&target];
            let operation_command_index = root_command_index.unwrap_or(command_index);
            self.sequence += 1;
            let correlation_id =
                format!("{}-{target}-{}", self.session.session_id(), self.sequence);
            let scope = TargetScope {
                target,
                depth,
                command_index: operation_command_index,
                correlation_id: correlation_id.clone(),
            };
            let result = self
                .session
                .execute(target, command, scope, &self.context)?;
            self.operations.push(OperationRecord {
                session_id: self.session.session_id().to_string(),
                target,
                platform: component.platform.clone(),
                adapter: result.adapter.clone(),
                command: command.display_name(),
                command_index: operation_command_index,
                command_path,
                depth,
                correlation_id,
                repeat_index: self.current_repeat_index,
                profile: component.profile.clone(),
                result,
            });
            if !self.options.continue_on_failure
                && self.operations.last().is_some_and(|operation| {
                    matches!(
                        operation.result.status,
                        AdapterStatus::Failed | AdapterStatus::Error
                    )
                })
            {
                self.stopped = true;
                return Ok(());
            }
        }
        Ok(())
    }

    fn root_command_selected(&self, command: &TestCommand, command_index: usize) -> bool {
        if let Some(index) = self.selected_root_index {
            return command_index == index;
        }
        if let Some(start) = self.options.from_command_index {
            if command_index < start {
                return false;
            }
        }
        if let Some(name) = self.options.command_name.as_deref() {
            if !matches!(command, TestCommand::RunFlow(_)) {
                return command.display_name() == name;
            }
        }
        true
    }
}

fn run_flow_condition_is_true(condition: &serde_json::Value) -> bool {
    match condition {
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Null => false,
        serde_json::Value::String(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "false" | "0" | "no"
        ),
        _ => true,
    }
}

fn multi_target_test_results(report: &MultiTargetRunReport, generated_at: String) -> TestResults {
    let flows = report
        .operations
        .iter()
        .map(|operation| {
            let status = match operation.result.status {
                AdapterStatus::Passed => CommandStatus::Passed,
                AdapterStatus::Blocked => CommandStatus::Skipped {
                    reason: operation.result.message.clone(),
                },
                AdapterStatus::Failed | AdapterStatus::Error => CommandStatus::Failed {
                    error: operation.result.message.clone(),
                },
            };
            let flow_status = if operation.result.status == AdapterStatus::Passed {
                FlowStatus::Passed
            } else {
                FlowStatus::Failed
            };
            FlowStateReport {
                flow_name: format!("{}:{}", operation.target, operation.command),
                flow_path: operation
                    .profile
                    .as_ref()
                    .map(|profile| profile.display().to_string())
                    .unwrap_or_else(|| "multi-target".to_string()),
                status: flow_status,
                commands: vec![CommandStateReport {
                    index: operation.command_index,
                    command_name: operation.command.clone(),
                    command_display: operation.command.clone(),
                    status,
                    duration_ms: None,
                    screenshot_path: None,
                    ui_hierarchy_path: None,
                    log_path: None,
                    retry_count: 0,
                }],
                total_duration_ms: None,
                error: if operation.result.status == AdapterStatus::Passed {
                    None
                } else {
                    Some(operation.result.message.clone())
                },
                video_path: None,
            }
        })
        .collect::<Vec<_>>();

    let mut adapters = BTreeSet::new();
    let metadata_operations = report
        .operations
        .iter()
        .enumerate()
        .map(|(sequence, operation)| {
            adapters.insert(operation.adapter.clone());
            MultiTargetOperationReport {
                sequence,
                target: operation.target.to_string(),
                platform: operation.platform.clone(),
                adapter: operation.adapter.clone(),
                command: operation.command.clone(),
                command_index: operation.command_index,
                repeat_index: operation.repeat_index,
                status: adapter_status_text(operation.result.status),
                category: operation.result.category.clone(),
                message: operation.result.message.clone(),
                profile: operation
                    .profile
                    .as_ref()
                    .map(|profile| profile.display().to_string()),
                evidence_tier: operation.result.evidence_tier.clone(),
                normalized_evidence: operation.result.normalized_evidence.clone(),
                raw_evidence: operation.result.raw_evidence.clone(),
            }
        })
        .collect::<Vec<_>>();

    let hc = hc_report_metadata(report);
    let metadata = MultiTargetReportMetadata {
        operation_count: report.operations.len() as u32,
        repeat_count: report.repeat_count.max(1),
        adapters: adapters.into_iter().collect(),
        operations: metadata_operations,
        hc,
    };
    let failed = report.failed + report.blocked + report.errors;
    TestResults {
        session_id: report.session_id.clone(),
        flows,
        summary: TestSummary {
            session_id: report.session_id.clone(),
            total_flows: report.operations.len() as u32,
            total_commands: report.operations.len() as u32,
            passed: report.passed as u32,
            failed: failed as u32,
            skipped: 0,
            total_duration_ms: None,
        },
        generated_at,
        multi_target: Some(metadata),
    }
}

fn adapter_status_text(status: AdapterStatus) -> String {
    match status {
        AdapterStatus::Passed => "passed",
        AdapterStatus::Failed => "failed",
        AdapterStatus::Blocked => "blocked",
        AdapterStatus::Error => "error",
    }
    .to_string()
}

fn hc_report_metadata(report: &MultiTargetRunReport) -> Option<HcReportMetadata> {
    let hc_operations = report
        .operations
        .iter()
        .filter(|operation| operation.platform == "lumi_hc" || operation.adapter == "lumi_hc")
        .collect::<Vec<_>>();
    if hc_operations.is_empty() {
        return None;
    }

    let mut device_ids = BTreeSet::new();
    let mut readbacks = Vec::new();
    for operation in hc_operations {
        let values = operation.result.normalized_evidence.clone();
        let device_id = values.as_ref().and_then(find_device_id).or_else(|| {
            operation
                .result
                .raw_evidence
                .as_ref()
                .and_then(find_device_id)
        });
        if let Some(device_id) = device_id.as_ref() {
            device_ids.insert(device_id.clone());
        }
        if matches!(
            operation.command.as_str(),
            "hcGetState" | "hcControl" | "hcWaitState"
        ) {
            readbacks.push(HcReadbackReport {
                device_id,
                repeat_index: operation.repeat_index,
                command: operation.command.clone(),
                command_index: operation.command_index,
                status: adapter_status_text(operation.result.status),
                values,
                message: operation.result.message.clone(),
            });
        }
    }

    Some(HcReportMetadata {
        device_ids: device_ids.into_iter().collect(),
        readbacks,
    })
}

fn find_device_id(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in ["device_id", "deviceId", "entity_id", "entityId", "entity"] {
                if let Some(Value::String(value)) = map.get(key) {
                    return Some(value.clone());
                }
            }
            map.values().find_map(find_device_id)
        }
        Value::Array(values) => values.iter().find_map(find_device_id),
        _ => None,
    }
}


fn count_status(operations: &[OperationRecord], status: AdapterStatus) -> usize {
    operations
        .iter()
        .filter(|operation| operation.result.status == status)
        .count()
}

#[derive(Default)]
struct MockWorld {
    device_on: bool,
    app_reads: usize,
}

struct MockAppAdapter {
    world: Arc<Mutex<MockWorld>>,
    capabilities: Vec<String>,
}

impl MockAppAdapter {
    fn new(world: Arc<Mutex<MockWorld>>) -> Self {
        Self {
            world,
            capabilities: vec!["deterministic_app_state".into()],
        }
    }
}

impl TargetAdapter for MockAppAdapter {
    fn target(&self) -> TargetId {
        TargetId::App
    }
    fn platform(&self) -> &str {
        "android"
    }
    fn adapter_name(&self) -> &str {
        "app_mock"
    }
    fn capabilities(&self) -> &[String] {
        &self.capabilities
    }
    fn preflight(&mut self, _context: &TestContext) -> AdapterResult {
        mock_pass("app_mock")
    }
    fn open(&mut self, _context: &TestContext) -> AdapterResult {
        mock_pass("app_mock")
    }
    fn execute(&mut self, command: &TestCommand, _scope: &TargetScope) -> AdapterResult {
        let mut world = self.world.lock().expect("mock world lock");
        world.app_reads += 1;
        let is_final = world.app_reads > 1;
        if is_final && !world.device_on {
            return AdapterResult::failed(
                "app_mock",
                "AssertionMismatch",
                "App did not reflect gateway readback",
            );
        }
        let evidence = json!({
            "command": command.display_name(),
            "phase": if is_final { "final" } else { "initial" },
            "deviceOn": world.device_on
        });
        with_evidence(mock_pass("app_mock"), evidence)
    }
    fn close(&mut self, _context: &TestContext) -> AdapterResult {
        mock_pass("app_mock")
    }
}

struct MockGatewayAdapter {
    world: Arc<Mutex<MockWorld>>,
    capabilities: Vec<String>,
}

impl MockGatewayAdapter {
    fn new(world: Arc<Mutex<MockWorld>>) -> Self {
        Self {
            world,
            capabilities: vec!["device_control".into(), "device_state".into()],
        }
    }
}

impl TargetAdapter for MockGatewayAdapter {
    fn target(&self) -> TargetId {
        TargetId::Gateway
    }
    fn platform(&self) -> &str {
        "lumi_hc"
    }
    fn adapter_name(&self) -> &str {
        "lumi_hc_mock"
    }
    fn capabilities(&self) -> &[String] {
        &self.capabilities
    }
    fn preflight(&mut self, _context: &TestContext) -> AdapterResult {
        mock_pass("lumi_hc_mock")
    }
    fn open(&mut self, _context: &TestContext) -> AdapterResult {
        mock_pass("lumi_hc_mock")
    }
    fn execute(&mut self, command: &TestCommand, _scope: &TargetScope) -> AdapterResult {
        let mut world = self.world.lock().expect("mock world lock");
        let (operation, requested) = match command {
            TestCommand::V3(crate::hc::backends::v3::runtime::types::V3Command::HcControl(
                params,
            )) => {
                let requested = params.state.iter().any(|change| {
                    change.idx == 0 && serde_json::to_value(&change.value).ok() == Some(json!(true))
                });
                world.device_on = requested;
                ("control", requested)
            }
            TestCommand::V3(crate::hc::backends::v3::runtime::types::V3Command::HcWaitState(
                params,
            )) => {
                let requested = params.state.iter().any(|change| {
                    change.idx == 0 && serde_json::to_value(&change.value).ok() == Some(json!(true))
                });
                if world.device_on != requested {
                    return AdapterResult::failed(
                        "lumi_hc_mock",
                        "AssertionMismatch",
                        "HC readback did not match requested idx/value",
                    );
                }
                ("readback", requested)
            }
            _ => {
                return AdapterResult::failed(
                    "lumi_hc_mock",
                    "UnsupportedCapability",
                    "mock gateway accepts HC control/readback commands only",
                )
            }
        };
        let raw = json!({"entity": "9007199254740993", "state": [{"idx": 0, "value": world.device_on}], "operation": operation});
        let normalized = json!({"entityId": "9007199254740993", "slotIdx": 0, "value": world.device_on, "requested": requested});
        let mut result = mock_pass("lumi_hc_mock");
        result.raw_evidence = Some(raw);
        result.normalized_evidence = Some(normalized);
        result
    }
    fn close(&mut self, _context: &TestContext) -> AdapterResult {
        mock_pass("lumi_hc_mock")
    }
}

fn mock_pass(adapter: &str) -> AdapterResult {
    let mut result = AdapterResult::passed(adapter);
    result.evidence_tier = Some("mock_contract".into());
    result
}

fn with_evidence(mut result: AdapterResult, evidence: serde_json::Value) -> AdapterResult {
    result.raw_evidence = Some(evidence.clone());
    result.normalized_evidence = Some(evidence);
    result
}
