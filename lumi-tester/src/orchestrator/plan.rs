use crate::orchestrator::inventory::InventoryEntity;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationPlan {
    pub schema_version: String,
    pub plan_id: String,
    pub request: String,
    pub scope: String,
    pub target: PlanConnection,
    pub inventory: PlanInventory,
    pub authorization: AuthorizationPolicy,
    pub execution: ExecutionPolicy,
    pub failure_policy: FailurePolicy,
    #[serde(default)]
    pub targets: Vec<PlanTarget>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub evidence: EvidencePolicy,
    #[serde(default)]
    pub restoration: Option<RestorationPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanConnection {
    pub platform: String,
    pub profile: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub connection: Option<ConnectionDetails>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDetails {
    #[serde(default)]
    pub scheme: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInventory {
    pub source: String,
    #[serde(alias = "snapshot_id")]
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(alias = "baseline_snapshot_id")]
    #[serde(default)]
    pub baseline_snapshot_id: Option<String>,
    #[serde(alias = "snapshot_path")]
    #[serde(default)]
    pub snapshot_path: Option<String>,
    #[serde(alias = "captured_at")]
    #[serde(default)]
    pub captured_at: Option<String>,
    #[serde(default)]
    pub complete: bool,
    #[serde(alias = "expected_count")]
    #[serde(default)]
    pub expected_count: Option<usize>,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub exclusions: Vec<InventoryExclusion>,
    #[serde(default)]
    pub entities: Vec<InventoryEntity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryExclusion {
    pub entity: String,
    #[serde(default)]
    pub capability: Option<String>,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationPolicy {
    #[serde(rename = "real_device_control", default)]
    pub real_device_control: bool,
    #[serde(rename = "restore_state", default)]
    pub restore_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPolicy {
    pub repeat: u32,
    #[serde(alias = "max_repeat")]
    pub max_repeat: u32,
    pub mode: String,
    #[serde(alias = "max_parallel")]
    pub max_parallel: u32,
    #[serde(alias = "delay_ms")]
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(alias = "timeout_ms")]
    pub timeout_ms: u64,
    #[serde(alias = "flow_layout")]
    #[serde(default)]
    pub flow_layout: Option<String>,
    #[serde(alias = "template_policy")]
    #[serde(default)]
    pub template_policy: Option<String>,
    #[serde(alias = "restore_after_each_cycle")]
    #[serde(default)]
    pub restore_after_each_cycle: bool,
    #[serde(alias = "final_state_capture")]
    #[serde(default)]
    pub final_state_capture: bool,
    #[serde(alias = "parallel_batches")]
    #[serde(default)]
    pub parallel_batches: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailurePolicy {
    pub scope: String,
    #[serde(alias = "on_target_failure")]
    pub on_target_failure: String,
    #[serde(alias = "aggregate_at_end")]
    pub aggregate_at_end: bool,
    #[serde(alias = "stop_on")]
    #[serde(default)]
    pub stop_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EvidencePolicy {
    #[serde(default)]
    pub report: bool,
    #[serde(default)]
    pub events: bool,
    #[serde(default)]
    pub snapshots: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub redaction: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorationPolicy {
    pub mode: String,
    pub verify: bool,
    #[serde(alias = "final_state_capture")]
    #[serde(default)]
    pub final_state_capture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTarget {
    pub entity: String,
    pub capability: String,
    pub status: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub mapping: BTreeMap<String, Value>,
    #[serde(default)]
    pub classification: Option<Value>,
    #[serde(default)]
    pub actions: Vec<Value>,
    #[serde(default)]
    pub cycles: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanIssue {
    pub severity: String,
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanValidationReport {
    pub schema_version: String,
    pub valid: bool,
    pub ready_for_run: bool,
    pub errors: Vec<PlanIssue>,
    pub warnings: Vec<PlanIssue>,
    #[serde(skip)]
    pub plan: Option<OrchestrationPlan>,
}

impl PlanValidationReport {
    fn new() -> Self {
        Self {
            schema_version: "1".to_string(),
            valid: true,
            ready_for_run: false,
            errors: Vec::new(),
            warnings: Vec::new(),
            plan: None,
        }
    }

    fn error(&mut self, code: &str, path: &str, message: impl Into<String>) {
        self.errors.push(PlanIssue {
            severity: "error".to_string(),
            code: code.to_string(),
            path: path.to_string(),
            message: message.into(),
        });
        self.valid = false;
    }

    fn safety(&mut self, for_run: bool, code: &str, path: &str, message: impl Into<String>) {
        let issue = PlanIssue {
            severity: if for_run { "error" } else { "warning" }.to_string(),
            code: code.to_string(),
            path: path.to_string(),
            message: message.into(),
        };
        if for_run {
            self.errors.push(issue);
            self.valid = false;
        } else {
            self.warnings.push(issue);
        }
    }
}

pub fn validate_plan_yaml(content: &str, for_run: bool, base_dir: &Path) -> PlanValidationReport {
    let mut report = PlanValidationReport::new();
    let raw: Value = match serde_yaml::from_str(content) {
        Ok(value) => value,
        Err(error) => {
            report.error("plan_parse_error", "/", error.to_string());
            return report;
        }
    };

    inspect_raw_entity_types(&raw, &mut report);
    let plan: OrchestrationPlan = match serde_yaml::from_value(raw) {
        Ok(plan) => plan,
        Err(error) => {
            report.error("plan_schema_error", "/", error.to_string());
            return report;
        }
    };
    validate_plan_into(&plan, for_run, base_dir, &mut report);
    report.plan = Some(plan);
    report.ready_for_run = report.valid && report.warnings.is_empty();
    report
}

pub fn validate_plan(
    plan: &OrchestrationPlan,
    for_run: bool,
    base_dir: &Path,
) -> PlanValidationReport {
    let mut report = PlanValidationReport::new();
    validate_plan_into(plan, for_run, base_dir, &mut report);
    report.plan = Some(plan.clone());
    report.ready_for_run = report.valid && report.warnings.is_empty();
    report
}

fn inspect_raw_entity_types(raw: &Value, report: &mut PlanValidationReport) {
    for (collection, path) in [("targets", "/targets"), ("entities", "/inventory/entities")] {
        let source = if collection == "targets" {
            map_value(raw, collection)
        } else {
            map_value(raw, "inventory").and_then(|inventory| map_value(inventory, collection))
        };
        let Some(items) = source.and_then(Value::as_sequence) else {
            continue;
        };
        for (index, item) in items.iter().enumerate() {
            if let Some(entity) = map_value(item, "entity") {
                if !matches!(entity, Value::String(_)) {
                    report.error(
                        "entity_id_must_be_string",
                        &format!("{path}/{index}/entity"),
                        "entity IDs must remain exact strings; numeric coercion is unsafe",
                    );
                }
            }
        }
    }
}

fn validate_plan_into(
    plan: &OrchestrationPlan,
    for_run: bool,
    base_dir: &Path,
    report: &mut PlanValidationReport,
) {
    if plan.schema_version != "1" {
        report.error(
            "unsupported_schema_version",
            "/schemaVersion",
            "only schemaVersion 1 is supported",
        );
    }
    for (path, value) in [
        ("/planId", plan.plan_id.trim()),
        ("/request", plan.request.trim()),
        ("/scope", plan.scope.trim()),
        ("/target/profile", plan.target.profile.trim()),
    ] {
        if value.is_empty() {
            report.error("required_field_missing", path, "field must not be empty");
        }
    }
    if plan.scope != "hc_only" || plan.target.platform != "lumi_hc" {
        report.error(
            "unsupported_orchestrator_scope",
            "/scope",
            "v1 orchestration currently supports scope hc_only and platform lumi_hc",
        );
    }
    if !matches!(plan.target.mode.as_str(), "real" | "mock") {
        report.error(
            "invalid_target_mode",
            "/target/mode",
            "mode must be real or mock",
        );
    }
    let endpoint_present = plan
        .target
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some();
    if plan.target.mode == "real" && !endpoint_present && plan.target.connection.is_none() {
        report.safety(
            for_run,
            "connection_required",
            "/target",
            "real HC execution requires endpoint or connection details",
        );
    }
    if let Some(connection) = &plan.target.connection {
        let has_address = connection
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
            || (connection
                .scheme
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
                && connection
                    .host
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_some()
                && connection.port.is_some());
        if plan.target.mode == "real"
            && (connection.status.as_deref() != Some("complete") || !has_address)
        {
            report.safety(
                for_run,
                "connection_incomplete",
                "/target/connection/status",
                "real HC execution requires a complete connection record",
            );
        }
    }

    if plan.inventory.source != "live_hc_discovery" {
        report.error(
            "invalid_inventory_source",
            "/inventory/source",
            "v1 inventory must come from live_hc_discovery",
        );
    }
    if let Some(captured_at) = &plan.inventory.captured_at {
        if chrono::DateTime::parse_from_rfc3339(captured_at).is_err() {
            report.error(
                "invalid_timestamp",
                "/inventory/capturedAt",
                "capturedAt must be RFC3339",
            );
        }
    }
    if plan.inventory.complete
        && plan.inventory.expected_count == Some(0)
        && !plan.targets.is_empty()
    {
        report.error(
            "inventory_count_mismatch",
            "/inventory/expectedCount",
            "expectedCount cannot be zero when targets are selected",
        );
    }
    if let Some(expected_count) = plan.inventory.expected_count {
        if plan.inventory.complete
            && !plan.inventory.entities.is_empty()
            && expected_count != plan.inventory.entities.len()
        {
            report.error(
                "inventory_count_mismatch",
                "/inventory/expectedCount",
                "expectedCount must match the complete embedded inventory",
            );
        }
    }
    if plan.inventory.complete
        && plan
            .inventory
            .snapshot_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        report.safety(
            for_run,
            "inventory_snapshot_id_required",
            "/inventory/snapshotId",
            "complete inventory requires a stable snapshotId",
        );
    }
    if for_run && plan.inventory.captured_at.is_none() {
        report.safety(
            true,
            "inventory_timestamp_required",
            "/inventory/capturedAt",
            "run-ready plans require the inventory capture timestamp",
        );
    }
    if let Some(snapshot_path) = &plan.inventory.snapshot_path {
        let resolved = resolve_path(base_dir, Path::new(snapshot_path));
        if !path_is_within(base_dir, &resolved) {
            report.error(
                "inventory_snapshot_outside_base_dir",
                "/inventory/snapshotPath",
                "inventory snapshot must remain inside the plan directory",
            );
        } else if !resolved.is_file() {
            report.safety(
                for_run,
                "inventory_snapshot_missing",
                "/inventory/snapshotPath",
                format!("inventory snapshot does not exist: {}", resolved.display()),
            );
        }
    }
    if plan.inventory.entities.is_empty() && plan.inventory.snapshot_path.is_none() {
        report.safety(
            for_run,
            "inventory_snapshot_payload_missing",
            "/inventory",
            "drift detection needs embedded entities or an inventory snapshot artifact",
        );
    }

    if plan.execution.repeat == 0 {
        report.error(
            "repeat_must_be_positive",
            "/execution/repeat",
            "repeat must be at least 1",
        );
    }
    if plan.execution.max_repeat == 0 {
        report.error(
            "max_repeat_must_be_positive",
            "/execution/maxRepeat",
            "maxRepeat must be at least 1",
        );
    }
    if plan.execution.repeat > plan.execution.max_repeat {
        report.error(
            "repeat_exceeds_max",
            "/execution/repeat",
            "repeat must not exceed maxRepeat",
        );
    }
    if !matches!(plan.execution.mode.as_str(), "sequential" | "parallel") {
        report.error(
            "invalid_execution_mode",
            "/execution/mode",
            "mode must be sequential or parallel",
        );
    }
    if plan.execution.max_parallel == 0 {
        report.error(
            "max_parallel_must_be_positive",
            "/execution/maxParallel",
            "maxParallel must be at least 1",
        );
    }
    if plan.execution.mode == "parallel" && plan.execution.parallel_batches.is_empty() {
        report.error(
            "parallel_batches_required",
            "/execution/parallelBatches",
            "parallel execution requires explicit independent batches",
        );
    }
    if plan.execution.mode == "sequential" && plan.execution.max_parallel > 1 {
        report.safety(
            for_run,
            "sequential_parallel_limit_mismatch",
            "/execution/maxParallel",
            "sequential execution must use maxParallel 1",
        );
    }
    if plan.execution.timeout_ms == 0 {
        report.error(
            "timeout_must_be_positive",
            "/execution/timeoutMs",
            "timeoutMs must be positive",
        );
    }

    for required_stop in [
        "health_failure",
        "discovery_failure",
        "inventory_drift",
        "unresolved_system_safety",
    ] {
        if !plan
            .failure_policy
            .stop_on
            .iter()
            .any(|value| value == required_stop)
        {
            report.safety(
                for_run,
                "shared_stop_condition_missing",
                "/failurePolicy/stopOn",
                format!("stopOn must include {required_stop}"),
            );
        }
    }
    if plan.failure_policy.scope != "target" {
        report.error(
            "invalid_failure_scope",
            "/failurePolicy/scope",
            "v1 orchestration requires target-scoped failure isolation",
        );
    }
    if plan.failure_policy.on_target_failure != "record_and_continue_independent_targets" {
        report.error(
            "invalid_target_failure_policy",
            "/failurePolicy/onTargetFailure",
            "target failures must be recorded and quarantined while independent targets continue",
        );
    }
    if !plan.failure_policy.aggregate_at_end {
        report.error(
            "aggregate_at_end_required",
            "/failurePolicy/aggregateAtEnd",
            "v1 orchestration requires aggregateAtEnd=true",
        );
    }

    let mut seen = BTreeMap::new();
    for (index, target) in plan.targets.iter().enumerate() {
        let path = format!("/targets/{index}");
        if !matches!(target.status.as_str(), "eligible" | "blocked" | "skipped") {
            report.error(
                "invalid_target_status",
                &format!("{path}/status"),
                "status must be eligible, blocked, or skipped",
            );
        }
        if seen
            .insert((target.entity.clone(), target.capability.clone()), index)
            .is_some()
        {
            report.error(
                "duplicate_target",
                &format!("{path}/entity"),
                "entity/capability target is duplicated",
            );
        }
        if target.entity.trim().is_empty() {
            report.error(
                "required_field_missing",
                &format!("{path}/entity"),
                "entity must not be empty",
            );
        }
        if target.capability.trim().is_empty() {
            report.error(
                "required_field_missing",
                &format!("{path}/capability"),
                "capability must not be empty",
            );
        }
        if matches!(target.status.as_str(), "blocked" | "skipped")
            && target.reason.as_deref().unwrap_or("").trim().is_empty()
        {
            report.error(
                "target_reason_required",
                &format!("{path}/reason"),
                "blocked/skipped targets require a reason",
            );
        }
        if target.status == "eligible" {
            if target.mapping.is_empty() {
                report.error(
                    "target_mapping_required",
                    &format!("{path}/mapping"),
                    "eligible targets require explicit mapping",
                );
            } else if !target
                .mapping
                .keys()
                .any(|key| key.eq_ignore_ascii_case("source"))
            {
                report.error(
                    "mapping_source_required",
                    &format!("{path}/mapping/source"),
                    "eligible target mapping must name its trusted source",
                );
            }
            if target
                .mapping
                .get("source")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                report.error(
                    "mapping_source_required",
                    &format!("{path}/mapping/source"),
                    "eligible targets require a mapping source",
                );
            }
            if plan.inventory.complete
                && !plan.inventory.allowlist.is_empty()
                && !plan
                    .inventory
                    .allowlist
                    .iter()
                    .any(|entity| entity == &target.entity)
            {
                report.error(
                    "target_not_allowlisted",
                    &format!("{path}/entity"),
                    "eligible target is outside inventory allowlist",
                );
            }
            if for_run && target.actions.is_empty() && target.cycles.is_empty() {
                report.safety(
                    true,
                    "target_actions_missing",
                    &format!("{path}/cycles"),
                    "eligible target needs actions or cycle data before mutation",
                );
            }
            if plan.execution.mode == "parallel"
                && !plan
                    .execution
                    .parallel_batches
                    .iter()
                    .any(|batch| batch.iter().any(|entity| entity == &target.entity))
            {
                report.error(
                    "parallel_batch_membership_required",
                    &format!("{path}/entity"),
                    "every eligible parallel target must belong to an explicit independent batch",
                );
            }
        }
    }

    let eligible_entities = plan
        .targets
        .iter()
        .filter(|target| target.status == "eligible")
        .map(|target| target.entity.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if plan.execution.mode == "parallel" {
        let mut batch_members = BTreeMap::new();
        for (batch_index, batch) in plan.execution.parallel_batches.iter().enumerate() {
            if batch.is_empty() {
                report.error(
                    "parallel_batch_empty",
                    &format!("/execution/parallelBatches/{batch_index}"),
                    "parallel batches must contain at least one eligible target",
                );
            }
            if batch.len() > plan.execution.max_parallel as usize {
                report.error(
                    "parallel_batch_exceeds_limit",
                    &format!("/execution/parallelBatches/{batch_index}"),
                    "parallel batch size must not exceed maxParallel",
                );
            }
            for (member_index, entity) in batch.iter().enumerate() {
                if !eligible_entities.contains(entity) {
                    report.error(
                        "parallel_batch_target_invalid",
                        &format!("/execution/parallelBatches/{batch_index}/{member_index}"),
                        "parallel batches may contain only eligible target entity IDs",
                    );
                }
                if batch_members
                    .insert(entity.clone(), (batch_index, member_index))
                    .is_some()
                {
                    report.error(
                        "parallel_batch_target_duplicated",
                        &format!("/execution/parallelBatches/{batch_index}/{member_index}"),
                        "an eligible target must belong to exactly one independent parallel batch",
                    );
                }
            }
        }
        for target in &eligible_entities {
            if !batch_members.contains_key(target) {
                report.error(
                    "parallel_batch_membership_required",
                    "/execution/parallelBatches",
                    format!("eligible target {target} is missing from parallel batches"),
                );
            }
        }
    }

    let eligible = plan
        .targets
        .iter()
        .any(|target| target.status == "eligible");
    if plan.target.mode == "real" && eligible && !plan.authorization.real_device_control {
        report.safety(
            for_run,
            "real_control_authorization_required",
            "/authorization/real_device_control",
            "real target control requires explicit authorization",
        );
    }
    if plan.authorization.restore_state {
        let valid_restore = plan
            .restoration
            .as_ref()
            .map(|restoration| {
                restoration.mode == "captured_state"
                    && restoration.verify
                    && restoration.final_state_capture
            })
            .unwrap_or(false);
        if !valid_restore {
            report.error(
                "restore_contract_required",
                "/restoration",
                "restore_state=true requires captured_state mode and verification",
            );
        }
        for (index, target) in plan
            .targets
            .iter()
            .enumerate()
            .filter(|(_, target)| target.status == "eligible")
        {
            let has_capture = target
                .actions
                .iter()
                .any(|action| action.as_str() == Some("capture_initial_state"))
                || target
                    .actions
                    .iter()
                    .any(|action| action.get("capture_initial_state").is_some());
            let has_restore = target
                .actions
                .iter()
                .any(|action| action.as_str() == Some("restore_initial_state"))
                || target
                    .actions
                    .iter()
                    .any(|action| action.get("restore_initial_state").is_some());
            if for_run && (!has_capture || !has_restore) {
                report.error(
                    "restore_actions_required",
                    &format!("/targets/{index}/actions"),
                    "restore-enabled eligible targets require capture_initial_state and restore_initial_state actions",
                );
            }
        }
    } else if eligible && !plan.execution.final_state_capture {
        report.safety(
            for_run,
            "final_state_capture_required",
            "/execution/finalStateCapture",
            "when restoration is not requested, final state capture is required",
        );
    }
}

fn map_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_mapping()?.get(Value::String(key.to_string()))
}

fn resolve_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        lexical_absolute(path)
    } else {
        lexical_absolute(&base_dir.join(path))
    }
}

fn path_is_within(base: &Path, candidate: &Path) -> bool {
    let base = canonical_or_lexical(base);
    let candidate = canonical_or_lexical(candidate);
    candidate.starts_with(&base)
        && !candidate
            .components()
            .any(|component| component == Component::ParentDir)
}

fn canonical_or_lexical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| lexical_absolute(path))
}

fn lexical_absolute(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
