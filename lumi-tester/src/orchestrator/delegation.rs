use crate::orchestrator::plan::{OrchestrationPlan, PlanIssue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
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

pub fn validate_delegation(
    plan: &OrchestrationPlan,
    response: &DelegationResponse,
    _base_dir: &Path,
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
    }

    if matches!(response.phase.as_str(), "run" | "rerun") {
        let mut seen_targets = BTreeSet::new();
        for (position, target) in response.target_results.iter().enumerate() {
            let path = format!("/targetResults/{position}");
            if !seen_targets.insert((target.entity.as_str(), target.capability.as_str())) {
                error(
                    &mut report,
                    "duplicate_target_result",
                    &format!("{path}/entity"),
                    "target result must be unique per entity/capability",
                );
            }
        }
    }

    if matches!(response.phase.as_str(), "run" | "rerun") {
        let Some(output_dir) = response.output_dir.as_deref() else {
            error(
                &mut report,
                "output_dir_required",
                "/outputDir",
                "run response requires outputDir",
            );
            return report;
        };
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
            if !path_is_within(output_dir, path) {
                error(
                    &mut report,
                    "artifact_outside_output_dir",
                    &format!("/artifactPaths/{position}"),
                    "artifact path must remain inside outputDir",
                );
            }
        }
        for (position, path) in response.generated_flow_paths.iter().enumerate() {
            if !path_is_within(output_dir, path) {
                error(
                    &mut report,
                    "generated_flow_outside_output_dir",
                    &format!("/generatedFlowPaths/{position}"),
                    "generated flow must remain inside outputDir",
                );
            }
        }
    }

    if matches!(response.phase.as_str(), "run" | "rerun") {
        if plan.authorization.restore_state && response.status == "passed" {
            let verified = response
                .restoration
                .as_ref()
                .map(|value| value.required)
                .unwrap_or(false)
                && response
                    .restoration
                    .as_ref()
                    .map(|value| value.verified)
                    .unwrap_or(false)
                && response
                    .restoration
                    .as_ref()
                    .and_then(|value| value.initial_state_path.as_ref())
                    .is_some()
                && response
                    .restoration
                    .as_ref()
                    .and_then(|value| value.final_state_path.as_ref())
                    .is_some();
            if !verified {
                error(&mut report, "restore_not_verified", "/restoration", "restore_state requires required=true, initial/final evidence, and verified=true");
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
    } else if !plan.authorization.restore_state
        && response.status == "passed"
        && response
            .restoration
            .as_ref()
            .and_then(|value| value.status.as_deref())
            != Some("not_requested")
    {
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
    let base = lexical_absolute(base);
    let candidate = if candidate.is_absolute() {
        lexical_absolute(candidate)
    } else {
        lexical_absolute(candidate)
    };
    candidate.starts_with(&base)
        && !candidate
            .components()
            .any(|component| component == Component::ParentDir)
}

fn lexical_absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}
