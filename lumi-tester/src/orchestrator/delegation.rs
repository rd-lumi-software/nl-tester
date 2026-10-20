use crate::orchestrator::lifecycle::{TargetLifecycle, TargetState};
use crate::orchestrator::plan::{OrchestrationPlan, PlanIssue};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationResponse {
    pub schema_version: String,
    pub plan_id: String,
    pub phase: String,
    pub status: String,
    pub runtime_status: String,
    #[serde(default)]
    pub validation: Option<ValidationSummary>,
    #[serde(default)]
    pub listed_commands: Vec<ListedCommand>,
    #[serde(default)]
    pub flow_paths: Vec<PathBuf>,
    #[serde(default)]
    pub output_dir: Option<PathBuf>,
    #[serde(default)]
    pub template: Option<TemplateSummary>,
    pub evidence_tier: String,
    #[serde(default)]
    pub artifact_paths: Vec<PathBuf>,
    #[serde(default)]
    pub generated_flow_paths: Vec<PathBuf>,
    #[serde(default)]
    pub unresolved: Vec<String>,
    #[serde(default)]
    pub restoration: Option<RestorationEvidence>,
    #[serde(default)]
    pub target_results: Vec<DelegatedTargetResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationSummary {
    pub valid: bool,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedCommand {
    pub index: usize,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSummary {
    pub status: String,
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorationEvidence {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub initial_state_path: Option<PathBuf>,
    #[serde(default)]
    pub restore_flow_path: Option<PathBuf>,
    #[serde(default)]
    pub final_state_path: Option<PathBuf>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegatedTargetResult {
    pub entity: String,
    pub capability: String,
    pub scenario: String,
    pub status: String,
    pub repetitions: RepetitionSummary,
    #[serde(default)]
    pub artifact_paths: Vec<PathBuf>,
    #[serde(default)]
    pub dispatch_latency_ms: Vec<u64>,
    #[serde(default)]
    pub readback_latency_ms: Vec<u64>,
    #[serde(default)]
    pub evidence_tier: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub initial_state: Option<serde_json::Value>,
    #[serde(default)]
    pub expected_state: Option<serde_json::Value>,
    #[serde(default)]
    pub observed_state: Option<serde_json::Value>,
    #[serde(default)]
    pub final_state: Option<serde_json::Value>,
    #[serde(default)]
    pub restoration: Option<RestorationEvidence>,
    #[serde(default)]
    pub lifecycle: Option<TargetLifecycle>,
    #[serde(default, alias = "actions", alias = "actionEvidence")]
    pub action_results: Vec<DelegatedActionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DelegatedActionResult {
    pub action_id: String,
    pub cycle_id: String,
    pub entity: String,
    pub stage: String,
    pub command: String,
    pub action_index: usize,
    pub transport: String,
    pub status: String,
    #[serde(default)]
    pub artifact_paths: Vec<PathBuf>,
    #[serde(default)]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepetitionSummary {
    pub requested: u32,
    pub completed: u32,
    pub passed: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationValidationReport {
    pub valid: bool,
    pub errors: Vec<PlanIssue>,
    pub warnings: Vec<PlanIssue>,
}

/// Validate a batch of delegation responses and reject target identities that
/// are repeated across response documents. Per-response validation alone
/// cannot detect that aggregation would otherwise silently discard one result.
pub fn validate_delegation_batch(
    plan: &OrchestrationPlan,
    responses: &[DelegationResponse],
    base_dir: &Path,
) -> Vec<DelegationValidationReport> {
    let mut reports = responses
        .iter()
        .map(|response| validate_delegation(plan, response, base_dir))
        .collect::<Vec<_>>();
    let mut seen = BTreeMap::new();
    for (response_index, response) in responses.iter().enumerate() {
        for (target_index, target) in response.target_results.iter().enumerate() {
            let key = (
                target.entity.clone(),
                target.capability.clone(),
                target.scenario.clone(),
            );
            if let Some((first_response, first_target)) =
                seen.insert(key, (response_index, target_index))
            {
                add_error(
                    &mut reports[response_index],
                    "duplicate_target_identity_across_responses",
                    &format!("/targetResults/{target_index}"),
                    format!(
                        "target identity is already present in response {first_response} at target result {first_target}"
                    ),
                );
            }
        }
    }
    reports
}

pub fn validate_delegation(
    plan: &OrchestrationPlan,
    response: &DelegationResponse,
    base_dir: &Path,
) -> DelegationValidationReport {
    let mut report = DelegationValidationReport {
        valid: true,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let error = |report: &mut DelegationValidationReport, code: &str, path: &str, message: &str| {
        report.errors.push(PlanIssue {
            severity: "error".to_string(),
            code: code.to_string(),
            path: path.to_string(),
            message: message.to_string(),
        });
        report.valid = false;
    };

    if response.schema_version != "1" {
        error(
            &mut report,
            "unsupported_schema_version",
            "/schemaVersion",
            "only response schemaVersion 1 is supported",
        );
    }
    if response.plan_id != plan.plan_id {
        error(
            &mut report,
            "plan_id_mismatch",
            "/planId",
            "delegation response planId does not match the plan",
        );
    }
    if !matches!(
        response.phase.as_str(),
        "discover"
            | "classify"
            | "author"
            | "validate"
            | "list"
            | "run"
            | "inspect"
            | "rerun"
            | "preflight"
    ) {
        error(
            &mut report,
            "invalid_phase",
            "/phase",
            "unsupported delegation phase",
        );
    }
    if !matches!(
        response.status.as_str(),
        "passed" | "failed" | "blocked" | "error" | "not_run"
    ) {
        error(
            &mut report,
            "invalid_status",
            "/status",
            "status must be passed, failed, blocked, error, or not_run",
        );
    }
    if !matches!(
        response.runtime_status.as_str(),
        "completed" | "failed" | "blocked" | "error" | "not_run"
    ) {
        error(
            &mut report,
            "runtime_status_invalid",
            "/runtimeStatus",
            "runtimeStatus must be completed, failed, blocked, error, or not_run",
        );
    }
    let expected_runtime_status = match response.status.as_str() {
        "passed" => Some("completed"),
        "failed" => Some("failed"),
        "blocked" => Some("blocked"),
        "error" => Some("error"),
        "not_run" => Some("not_run"),
        _ => None,
    };
    if expected_runtime_status.is_some_and(|expected| response.runtime_status != expected) {
        error(
            &mut report,
            "runtime_status_mismatch",
            "/runtimeStatus",
            "runtimeStatus must agree with the response status",
        );
    }
    if !is_allowed_evidence_tier(&response.evidence_tier) {
        error(
            &mut report,
            "invalid_evidence_tier",
            "/evidenceTier",
            "evidenceTier must be mock_contract, mock_conformance, real_preflight, mixed, or real_e2e",
        );
    }
    if response.status == "passed" && !response.unresolved.is_empty() {
        error(
            &mut report,
            "unresolved_assumptions",
            "/unresolved",
            "passed response cannot hide unresolved assumptions",
        );
    }
    if response.status == "passed" {
        if let Some(validation) = &response.validation {
            if !validation.valid {
                error(
                    &mut report,
                    "validation_failed",
                    "/validation/valid",
                    "passed response requires valid=true",
                );
            }
        } else if matches!(
            response.phase.as_str(),
            "validate" | "list" | "run" | "rerun"
        ) {
            error(
                &mut report,
                "validation_result_missing",
                "/validation",
                "validation result is required",
            );
        }
    }

    let mut indexes = BTreeSet::new();
    for (position, command) in response.listed_commands.iter().enumerate() {
        if command.name.trim().is_empty() {
            error(
                &mut report,
                "command_name_missing",
                &format!("/listedCommands/{position}/name"),
                "listed command name is required",
            );
        }
        if !indexes.insert(command.index) {
            error(
                &mut report,
                "duplicate_command_index",
                &format!("/listedCommands/{position}/index"),
                "listed command indexes must be unique",
            );
        }
    }

    let known_targets = plan
        .targets
        .iter()
        .map(|target| format!("{}::{}", target.entity, target.capability))
        .collect::<BTreeSet<_>>();
    for (position, target) in response.target_results.iter().enumerate() {
        if !known_targets.contains(&format!("{}::{}", target.entity, target.capability)) {
            error(
                &mut report,
                "unknown_target",
                &format!("/targetResults/{position}"),
                "target result is not present in the validated plan",
            );
        }
        if !matches!(
            target.status.as_str(),
            "passed" | "failed" | "blocked" | "skipped" | "error" | "not_run"
        ) {
            error(
                &mut report,
                "invalid_target_status",
                &format!("/targetResults/{position}/status"),
                "target result status is invalid",
            );
        }
        if let Some(tier) = &target.evidence_tier {
            if !matches!(
                tier.as_str(),
                "mock_contract" | "mock_conformance" | "real_preflight" | "real_e2e" | "mixed"
            ) {
                error(
                    &mut report,
                    "invalid_evidence_tier",
                    &format!("/targetResults/{position}/evidenceTier"),
                    "target evidenceTier is invalid",
                );
            }
        }
        if let Some(lifecycle) = &target.lifecycle {
            let expected_state = match target.status.as_str() {
                "passed" => Some(TargetState::Passed),
                "failed" => Some(TargetState::Failed),
                "blocked" => Some(TargetState::Blocked),
                "skipped" => Some(TargetState::Skipped),
                "error" => Some(TargetState::Error),
                "not_run" => Some(TargetState::NotRun),
                _ => None,
            };
            if lifecycle.entity != target.entity
                || lifecycle.capability != target.capability
                || lifecycle.validate_history().is_err()
                || expected_state.is_some_and(|state| lifecycle.state != state)
            {
                error(
                    &mut report,
                    "lifecycle_invalid",
                    &format!("/targetResults/{position}/lifecycle"),
                    "lifecycle identity, transition history, and terminal state must match the target result",
                );
            }
        }
        let repetitions = &target.repetitions;
        if repetitions.completed > repetitions.requested {
            error(
                &mut report,
                "repetitions_completed_exceed_requested",
                &format!("/targetResults/{position}/repetitions/completed"),
                "completed repetitions must not exceed requested repetitions",
            );
        }
        if repetitions.passed.saturating_add(repetitions.failed) > repetitions.completed {
            error(
                &mut report,
                "repetitions_outcome_exceed_completed",
                &format!("/targetResults/{position}/repetitions"),
                "passed plus failed repetitions must not exceed completed repetitions",
            );
        }
        if matches!(response.phase.as_str(), "run" | "rerun") {
            if repetitions.passed.saturating_add(repetitions.failed) != repetitions.completed {
                error(
                    &mut report,
                    "repetitions_outcome_mismatch",
                    &format!("/targetResults/{position}/repetitions"),
                    "passed plus failed repetitions must equal completed repetitions",
                );
            }
            if let Some(plan_target) = plan.targets.iter().find(|plan_target| {
                plan_target.entity == target.entity && plan_target.capability == target.capability
            }) {
                if plan_target.status == "eligible"
                    && repetitions.requested != plan.execution.repeat
                {
                    error(
                        &mut report,
                        "repetitions_requested_mismatch",
                        &format!("/targetResults/{position}/repetitions/requested"),
                        "requested repetitions must match plan execution.repeat for eligible targets",
                    );
                }
            }
            if target.status == "passed"
                && (repetitions.completed != repetitions.requested
                    || repetitions.passed != repetitions.requested
                    || repetitions.failed != 0)
            {
                error(
                    &mut report,
                    "passed_target_without_completed_repetitions",
                    &format!("/targetResults/{position}/repetitions/completed"),
                    "a passed target must report at least one completed repetition",
                );
            }
        }
    }

    if matches!(response.phase.as_str(), "run" | "rerun") {
        let mut seen_targets = BTreeSet::new();
        for (position, target) in response.target_results.iter().enumerate() {
            let path = format!("/targetResults/{position}");
            if !seen_targets.insert((
                target.entity.as_str(),
                target.capability.as_str(),
                target.scenario.as_str(),
            )) {
                error(
                    &mut report,
                    "duplicate_target_result",
                    &format!("{path}/entity"),
                    "target result must be unique per entity/capability/scenario",
                );
            }
        }

        if !response.target_results.is_empty() {
            let has_failed_target = response
                .target_results
                .iter()
                .any(|target| target.status == "failed");
            let has_error_target = response
                .target_results
                .iter()
                .any(|target| target.status == "error");
            if response.status == "failed" && !has_failed_target {
                error(
                    &mut report,
                    "response_status_inconsistent",
                    "/status",
                    "failed response with target results must include a failed target result",
                );
            }
            if response.status == "error" && !has_error_target {
                error(
                    &mut report,
                    "response_status_inconsistent",
                    "/status",
                    "error response with target results must include an error target result",
                );
            }
        }

        if response.status == "passed" {
            if response
                .target_results
                .iter()
                .any(|target| target.status != "passed")
            {
                error(
                    &mut report,
                    "passed_target_not_allowed",
                    "/targetResults",
                    "passed response requires every eligible target result to be passed",
                );
            }
            for target in plan
                .targets
                .iter()
                .filter(|target| target.status == "eligible")
            {
                if !response.target_results.iter().any(|result| {
                    result.entity == target.entity && result.capability == target.capability
                }) {
                    add_error(
                        &mut report,
                        "eligible_target_result_missing",
                        "/targetResults",
                        format!(
                            "passed response is missing eligible target {}::{}",
                            target.entity, target.capability
                        ),
                    );
                }
            }
        }
    }

    let output_root = if matches!(response.phase.as_str(), "run" | "rerun") {
        match response.output_dir.as_deref() {
            None => {
                error(
                    &mut report,
                    "output_dir_required",
                    "/outputDir",
                    "run response requires outputDir",
                );
                None
            }
            Some(output_dir) if output_dir.as_os_str().is_empty() => {
                error(
                    &mut report,
                    "output_dir_invalid",
                    "/outputDir",
                    "outputDir must not be empty",
                );
                None
            }
            Some(output_dir) => Some(resolve_path(base_dir, output_dir)),
        }
    } else {
        None
    };

    if let Some(output_root) = output_root.as_deref() {
        if !path_is_within(base_dir, output_root) {
            add_error(
                &mut report,
                "output_dir_outside_base_dir",
                "/outputDir",
                "outputDir must remain inside the supplied response base directory",
            );
        }
        if !output_root.is_dir() {
            add_error(
                &mut report,
                "output_dir_missing",
                "/outputDir",
                format!("outputDir does not exist: {}", output_root.display()),
            );
        }
        if response.status == "passed"
            && !response
                .artifact_paths
                .iter()
                .any(|path| path.file_name().and_then(|name| name.to_str()) == Some("run.json"))
        {
            error(
                &mut report,
                "run_manifest_missing",
                "/artifactPaths",
                "passed run response must include run.json",
            );
        }
        for (position, path) in response.artifact_paths.iter().enumerate() {
            validate_evidence_path(
                &mut report,
                base_dir,
                output_root,
                path,
                &format!("/artifactPaths/{position}"),
                "artifact_outside_output_dir",
                "artifact_missing",
            );
        }
        for (position, path) in response.flow_paths.iter().enumerate() {
            validate_evidence_path(
                &mut report,
                base_dir,
                output_root,
                path,
                &format!("/flowPaths/{position}"),
                "flow_outside_output_dir",
                "flow_missing",
            );
        }
        for (position, path) in response.generated_flow_paths.iter().enumerate() {
            validate_evidence_path(
                &mut report,
                base_dir,
                output_root,
                path,
                &format!("/generatedFlowPaths/{position}"),
                "generated_flow_outside_output_dir",
                "generated_flow_missing",
            );
        }
        for (target_position, target) in response.target_results.iter().enumerate() {
            for (path_position, path) in target.artifact_paths.iter().enumerate() {
                validate_evidence_path(
                    &mut report,
                    base_dir,
                    output_root,
                    path,
                    &format!("/targetResults/{target_position}/artifactPaths/{path_position}"),
                    "target_artifact_outside_output_dir",
                    "target_artifact_missing",
                );
            }
            for (action_position, action) in target.action_results.iter().enumerate() {
                for (path_position, path) in action.artifact_paths.iter().enumerate() {
                    validate_evidence_path(
                        &mut report,
                        base_dir,
                        output_root,
                        path,
                        &format!(
                            "/targetResults/{target_position}/actionResults/{action_position}/artifactPaths/{path_position}"
                        ),
                        "action_artifact_outside_output_dir",
                        "action_artifact_missing",
                    );
                }
            }
            if let Some(restoration) = target.restoration.as_ref() {
                for (name, path) in [
                    ("initialStatePath", restoration.initial_state_path.as_ref()),
                    ("restoreFlowPath", restoration.restore_flow_path.as_ref()),
                    ("finalStatePath", restoration.final_state_path.as_ref()),
                ] {
                    if let Some(path) = path {
                        validate_evidence_path(
                            &mut report,
                            base_dir,
                            output_root,
                            path,
                            &format!("/targetResults/{target_position}/restoration/{name}"),
                            "target_restoration_path_outside_output_dir",
                            "target_restoration_path_missing",
                        );
                    }
                }
            }
        }
    }

    if let Some(template) = response
        .template
        .as_ref()
        .and_then(|template| template.path.as_ref())
    {
        let resolved = resolve_path(base_dir, template);
        if !path_is_within(base_dir, &resolved) {
            add_error(
                &mut report,
                "template_outside_base_dir",
                "/template/path",
                "template path must remain inside the supplied response base directory",
            );
        }
        if !resolved.is_file() {
            add_error(
                &mut report,
                "template_missing",
                "/template/path",
                format!("template file does not exist: {}", resolved.display()),
            );
        }
    }

    if output_root.is_none() {
        if let Some(output_dir) = response.output_dir.as_deref() {
            let resolved = resolve_path(base_dir, output_dir);
            if !path_is_within(base_dir, &resolved) {
                add_error(
                    &mut report,
                    "output_dir_outside_base_dir",
                    "/outputDir",
                    "outputDir must remain inside the supplied response base directory",
                );
            }
            if !resolved.is_dir() {
                add_error(
                    &mut report,
                    "output_dir_missing",
                    "/outputDir",
                    format!("outputDir does not exist: {}", resolved.display()),
                );
            }
        }
        for (position, path) in response.artifact_paths.iter().enumerate() {
            validate_base_path(
                &mut report,
                base_dir,
                path,
                &format!("/artifactPaths/{position}"),
                "artifact_outside_base_dir",
                "artifact_missing",
            );
        }
        for (position, path) in response.flow_paths.iter().enumerate() {
            validate_base_path(
                &mut report,
                base_dir,
                path,
                &format!("/flowPaths/{position}"),
                "flow_outside_base_dir",
                "flow_missing",
            );
        }
        for (position, path) in response.generated_flow_paths.iter().enumerate() {
            validate_base_path(
                &mut report,
                base_dir,
                path,
                &format!("/generatedFlowPaths/{position}"),
                "generated_flow_outside_base_dir",
                "generated_flow_missing",
            );
        }
        for (target_position, target) in response.target_results.iter().enumerate() {
            for (path_position, path) in target.artifact_paths.iter().enumerate() {
                validate_base_path(
                    &mut report,
                    base_dir,
                    path,
                    &format!("/targetResults/{target_position}/artifactPaths/{path_position}"),
                    "target_artifact_outside_base_dir",
                    "target_artifact_missing",
                );
            }
            for (action_position, action) in target.action_results.iter().enumerate() {
                for (path_position, path) in action.artifact_paths.iter().enumerate() {
                    validate_base_path(
                        &mut report,
                        base_dir,
                        path,
                        &format!(
                            "/targetResults/{target_position}/actionResults/{action_position}/artifactPaths/{path_position}"
                        ),
                        "action_artifact_outside_base_dir",
                        "action_artifact_missing",
                    );
                }
            }
            if let Some(restoration) = target.restoration.as_ref() {
                for (name, path) in [
                    ("initialStatePath", restoration.initial_state_path.as_ref()),
                    ("restoreFlowPath", restoration.restore_flow_path.as_ref()),
                    ("finalStatePath", restoration.final_state_path.as_ref()),
                ] {
                    if let Some(path) = path {
                        validate_base_path(
                            &mut report,
                            base_dir,
                            path,
                            &format!("/targetResults/{target_position}/restoration/{name}"),
                            "target_restoration_path_outside_base_dir",
                            "target_restoration_path_missing",
                        );
                    }
                }
            }
        }
        if let Some(restoration) = response.restoration.as_ref() {
            for (name, path) in [
                ("initialStatePath", restoration.initial_state_path.as_ref()),
                ("restoreFlowPath", restoration.restore_flow_path.as_ref()),
                ("finalStatePath", restoration.final_state_path.as_ref()),
            ] {
                if let Some(path) = path {
                    validate_base_path(
                        &mut report,
                        base_dir,
                        path,
                        &format!("/restoration/{name}"),
                        "restoration_path_outside_base_dir",
                        "restoration_path_missing",
                    );
                }
            }
        }
    }

    if matches!(response.phase.as_str(), "run" | "rerun") {
        let execution_reached_mutation = !matches!(response.status.as_str(), "blocked" | "not_run");
        if plan.authorization.restore_state && execution_reached_mutation {
            let Some(restoration) = response.restoration.as_ref() else {
                error(
                    &mut report,
                    "restore_not_verified",
                    "/restoration",
                    "restore_state requires restoration evidence after execution",
                );
                return report;
            };
            let paths_present = restoration.initial_state_path.is_some()
                && restoration.restore_flow_path.is_some()
                && restoration.final_state_path.is_some();
            let verified = restoration.required
                && restoration.verified
                && restoration.status.as_deref() == Some("verified")
                && paths_present;
            if !verified {
                error(
                    &mut report,
                    "restore_not_verified",
                    "/restoration",
                    "restore_state requires required=true, initial/restore/final evidence, status=verified, and verified=true",
                );
            }
        } else if !plan.authorization.restore_state {
            let not_requested = response
                .restoration
                .as_ref()
                .map(|value| {
                    !value.required
                        && !value.verified
                        && value.status.as_deref() == Some("not_requested")
                })
                .unwrap_or(false);
            if !not_requested {
                error(
                    &mut report,
                    "restore_status_invalid",
                    "/restoration/status",
                    "restore_state=false requires restoration status not_requested",
                );
            }
        }
    }

    if let (Some(output_root), Some(restoration)) =
        (output_root.as_deref(), response.restoration.as_ref())
    {
        for (name, path) in [
            ("initialStatePath", restoration.initial_state_path.as_ref()),
            ("restoreFlowPath", restoration.restore_flow_path.as_ref()),
            ("finalStatePath", restoration.final_state_path.as_ref()),
        ] {
            if let Some(path) = path {
                validate_evidence_path(
                    &mut report,
                    base_dir,
                    output_root,
                    path,
                    &format!("/restoration/{name}"),
                    "restoration_path_outside_output_dir",
                    "restoration_path_missing",
                );
            }
        }
    }

    if !matches!(response.phase.as_str(), "run" | "rerun")
        && !plan.authorization.restore_state
        && response.status == "passed"
    {
        let not_requested = response
            .restoration
            .as_ref()
            .map(|value| {
                !value.required
                    && !value.verified
                    && value.status.as_deref() == Some("not_requested")
            })
            .unwrap_or(false);
        if not_requested {
            return report;
        }
        error(
            &mut report,
            "restore_status_invalid",
            "/restoration/status",
            "restoration must be reported as not_requested when the plan disables restoration",
        );
    }

    report
}

fn is_allowed_evidence_tier(value: &str) -> bool {
    matches!(
        value,
        "mock_contract" | "mock_conformance" | "real_preflight" | "mixed" | "real_e2e"
    )
}

fn path_is_within(base: &Path, candidate: &Path) -> bool {
    let base = canonical_or_lexical(base);
    let candidate = canonical_or_lexical(candidate);
    candidate.starts_with(&base)
        && !candidate
            .components()
            .any(|component| component == Component::ParentDir)
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

fn canonical_or_lexical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| lexical_absolute(path))
}

fn resolve_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        lexical_absolute(path)
    } else {
        lexical_absolute(&base_dir.join(path))
    }
}

fn validate_evidence_path(
    report: &mut DelegationValidationReport,
    base_dir: &Path,
    output_root: &Path,
    path: &Path,
    json_path: &str,
    outside_code: &str,
    missing_code: &str,
) {
    let resolved = resolve_path(base_dir, path);
    if !path_is_within(output_root, &resolved) {
        add_error(
            report,
            outside_code,
            json_path,
            "evidence path must remain inside outputDir",
        );
    }
    if !resolved.is_file() {
        add_error(
            report,
            missing_code,
            json_path,
            format!("evidence file does not exist: {}", resolved.display()),
        );
    }
}

fn validate_base_path(
    report: &mut DelegationValidationReport,
    base_dir: &Path,
    path: &Path,
    json_path: &str,
    outside_code: &str,
    missing_code: &str,
) {
    let resolved = resolve_path(base_dir, path);
    if !path_is_within(base_dir, &resolved) {
        add_error(
            report,
            outside_code,
            json_path,
            "path must remain inside the supplied response base directory",
        );
    }
    if !resolved.is_file() {
        add_error(
            report,
            missing_code,
            json_path,
            format!("path does not exist: {}", resolved.display()),
        );
    }
}

fn add_error(
    report: &mut DelegationValidationReport,
    code: &str,
    path: &str,
    message: impl Into<String>,
) {
    report.errors.push(PlanIssue {
        severity: "error".to_string(),
        code: code.to_string(),
        path: path.to_string(),
        message: message.into(),
    });
    report.valid = false;
}
