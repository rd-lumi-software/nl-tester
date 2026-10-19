use super::types::{ComponentSpec, NormalizedFlow, RunFlowParamsInput, TargetId, TestCommand};
use anyhow::Result;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FlowValidationError {
    #[error("flow does not declare an app, gateway, or cloud component")]
    MissingComponent,
    #[error("component '{component}' is unknown; expected app, gateway, or cloud")]
    UnknownComponent { component: String },
    #[error("platform '{platform}' is unsupported")]
    UnsupportedPlatform { platform: String },
    #[error("app component for platform '{platform}' requires appId")]
    MissingAppId { platform: String },
    #[error("component '{target}' requires profile")]
    MissingProfile { target: TargetId },
    #[error("platform '{platform}' cannot be bound to target '{target}'")]
    TargetPlatformMismatch { target: TargetId, platform: String },
    #[error("profile '{profile}' for target '{target}' is invalid or missing")]
    InvalidProfile { target: TargetId, profile: String },
    #[error("multi-target flow command at index {command_index} requires an explicit target")]
    MissingTarget { command_index: usize },
    #[error("runFlow target '{target}' is unknown")]
    UnknownTarget { target: String },
    #[error("runFlow target '{target}' is not declared by the root flow")]
    TargetRequired { target: TargetId },
    #[error("child flow '{path}' attempts to rebind target '{target}'")]
    ChildHeaderDriverConflict { path: String, target: TargetId },
    #[error("recursive flow cycle detected at '{path}'")]
    RecursiveFlowCycle { path: String },
    #[error("recursive flow depth {depth} exceeds maximum {max_depth}")]
    RecursiveFlowDepthExceeded { depth: usize, max_depth: usize },
    #[error("command '{command}' is unsupported for target '{target}'")]
    UnsupportedCommandForTarget { command: String, target: TargetId },
}

impl FlowValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingComponent => "MissingComponent",
            Self::UnknownComponent { .. } => "UnknownComponent",
            Self::UnsupportedPlatform { .. } => "UnsupportedPlatform",
            Self::MissingAppId { .. } => "MissingAppId",
            Self::MissingProfile { .. } => "MissingProfile",
            Self::TargetPlatformMismatch { .. } => "TargetPlatformMismatch",
            Self::InvalidProfile { .. } => "InvalidProfile",
            Self::MissingTarget { .. } => "MissingTarget",
            Self::UnknownTarget { .. } => "UnknownTarget",
            Self::TargetRequired { .. } => "TargetRequired",
            Self::ChildHeaderDriverConflict { .. } => "ChildHeaderDriverConflict",
            Self::RecursiveFlowCycle { .. } => "RecursiveFlowCycle",
            Self::RecursiveFlowDepthExceeded { .. } => "RecursiveFlowDepthExceeded",
            Self::UnsupportedCommandForTarget { .. } => "UnsupportedCommandForTarget",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRoute {
    pub command_index: usize,
    pub effective_target: TargetId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidatedFlow {
    pub routes: Vec<ValidatedRoute>,
}

pub fn validate_flow_graph(flow: &NormalizedFlow) -> Result<ValidatedFlow> {
    validate_flow_graph_with_limit(flow, 32)
}

pub fn validate_flow_graph_with_limit(
    flow: &NormalizedFlow,
    max_depth: usize,
) -> Result<ValidatedFlow> {
    let sole_target = if flow.components.len() == 1 {
        flow.components.keys().next().copied()
    } else {
        None
    };
    let mut routes = Vec::new();
    let mut stack = vec![path_key(&flow.source_path)];
    collect_routes(
        &flow.commands,
        flow.components.len() > 1,
        sole_target,
        &flow.components,
        &flow.source_path,
        sole_target,
        0,
        max_depth,
        &mut stack,
        &mut routes,
    )?;
    validate_components(flow)?;

    Ok(ValidatedFlow { routes })
}

fn validate_components(flow: &NormalizedFlow) -> Result<()> {
    if flow.components.is_empty() {
        return Err(FlowValidationError::MissingComponent.into());
    }
    for (target, component) in &flow.components {
        let known_platform = matches!(
            component.platform.as_str(),
            "android"
                | "android_auto"
                | "ios"
                | "web"
                | "macos"
                | "windows"
                | "lumi_hc"
                | "lumi_cloud"
        );
        if !known_platform {
            return Err(FlowValidationError::UnsupportedPlatform {
                platform: component.platform.clone(),
            }
            .into());
        }
        let target_matches = match target {
            TargetId::App => matches!(
                component.platform.as_str(),
                "android" | "android_auto" | "ios" | "web" | "macos" | "windows"
            ),
            TargetId::Gateway => component.platform == "lumi_hc",
            TargetId::Cloud => component.platform == "lumi_cloud",
        };
        if !target_matches {
            return Err(FlowValidationError::TargetPlatformMismatch {
                target: *target,
                platform: component.platform.clone(),
            }
            .into());
        }
        if *target == TargetId::App
            && component.platform != "web"
            && component.app_id.as_deref().map_or(true, str::is_empty)
        {
            return Err(FlowValidationError::MissingAppId {
                platform: component.platform.clone(),
            }
            .into());
        }
        if matches!(target, TargetId::Gateway | TargetId::Cloud) {
            let Some(profile) = component.profile.as_deref() else {
                return Err(FlowValidationError::MissingProfile { target: *target }.into());
            };
            if !profile.is_file() {
                return Err(FlowValidationError::InvalidProfile {
                    target: *target,
                    profile: profile.display().to_string(),
                }
                .into());
            }
        }
    }
    Ok(())
}

fn collect_routes(
    commands: &[TestCommand],
    multi_target: bool,
    sole_target: Option<TargetId>,
    components: &BTreeMap<TargetId, ComponentSpec>,
    source_path: &std::path::Path,
    active_target: Option<TargetId>,
    depth: usize,
    max_depth: usize,
    stack: &mut Vec<std::path::PathBuf>,
    routes: &mut Vec<ValidatedRoute>,
) -> Result<()> {
    for (command_index, command) in commands.iter().enumerate() {
        if let TestCommand::RunFlow(params) = command {
            let params = params_ref(params);
            if multi_target && params.target.is_none() {
                return Err(FlowValidationError::MissingTarget { command_index }.into());
            }
            let effective_target = params.target.or(sole_target);
            if let Some(effective_target) = effective_target {
                if !components.contains_key(&effective_target) {
                    return Err(FlowValidationError::TargetRequired {
                        target: effective_target,
                    }
                    .into());
                }
                routes.push(ValidatedRoute {
                    command_index,
                    effective_target,
                });
            }
            if let Some(inline_commands) = params.commands.as_deref() {
                check_depth(depth + 1, max_depth)?;
                collect_routes(
                    inline_commands,
                    multi_target,
                    sole_target,
                    components,
                    source_path,
                    effective_target,
                    depth + 1,
                    max_depth,
                    stack,
                    routes,
                )?;
            }
            if let (Some(path), Some(effective_target)) = (params.path.as_deref(), effective_target)
            {
                let child_path = source_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(path);
                if params.optional.unwrap_or(false) && !child_path.is_file() {
                    continue;
                }
                check_depth(depth + 1, max_depth)?;
                let child_key = path_key(&child_path);
                if stack.contains(&child_key) {
                    return Err(FlowValidationError::RecursiveFlowCycle {
                        path: child_path.display().to_string(),
                    }
                    .into());
                }
                let child = super::yaml::parse_test_file(&child_path)?;
                let child = super::normalize::normalize_flow(child, &child_path)?;
                validate_child_header(components, &child, effective_target, &child_path)?;
                stack.push(child_key);
                collect_routes(
                    &child.commands,
                    multi_target,
                    sole_target,
                    components,
                    &child_path,
                    Some(effective_target),
                    depth + 1,
                    max_depth,
                    stack,
                    routes,
                )?;
                stack.pop();
            }
        } else if let Some(target) = active_target {
            validate_command_target(command, target)?;
        } else if multi_target {
            return Err(FlowValidationError::MissingTarget { command_index }.into());
        }
    }
    Ok(())
}

fn validate_command_target(command: &TestCommand, target: TargetId) -> Result<()> {
    let supported = match target {
        TargetId::Gateway => matches!(command, TestCommand::V3(_)),
        TargetId::App => !matches!(command, TestCommand::V3(_)),
        TargetId::Cloud => !matches!(command, TestCommand::V3(_)),
    };
    if !supported {
        return Err(FlowValidationError::UnsupportedCommandForTarget {
            command: command.display_name(),
            target,
        }
        .into());
    }
    Ok(())
}

fn check_depth(depth: usize, max_depth: usize) -> Result<()> {
    if depth > max_depth {
        return Err(FlowValidationError::RecursiveFlowDepthExceeded { depth, max_depth }.into());
    }
    Ok(())
}

fn path_key(path: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn validate_child_header(
    root_components: &BTreeMap<TargetId, ComponentSpec>,
    child: &NormalizedFlow,
    effective_target: TargetId,
    child_path: &std::path::Path,
) -> Result<()> {
    if child.components.is_empty() {
        return Ok(());
    }
    // A list-only legacy child is parsed with the historical Android default so
    // it can still run as a root App flow. Under an explicit parent target that
    // default is not a driver declaration and must not rebind the parent scope.
    if effective_target != TargetId::App
        && child.components.len() == 1
        && child
            .components
            .get(&TargetId::App)
            .is_some_and(|component| {
                matches!(component.source, super::types::ComponentSource::LegacyApp)
            })
    {
        return Ok(());
    }
    let Some(child_target) = child.components.get(&effective_target) else {
        return Err(FlowValidationError::ChildHeaderDriverConflict {
            path: child_path.display().to_string(),
            target: effective_target,
        }
        .into());
    };
    let Some(root_target) = root_components.get(&effective_target) else {
        return Ok(());
    };
    let app_id_conflicts = child_target.app_id.is_some()
        && root_target.app_id.is_some()
        && child_target.app_id != root_target.app_id;
    let profile_conflicts = child_target.profile.is_some()
        && root_target.profile.is_some()
        && child_target.profile != root_target.profile;
    if child_target.platform != root_target.platform || app_id_conflicts || profile_conflicts {
        return Err(FlowValidationError::ChildHeaderDriverConflict {
            path: child_path.display().to_string(),
            target: effective_target,
        }
        .into());
    }
    Ok(())
}

fn params_ref(params: &RunFlowParamsInput) -> super::types::RunFlowParams {
    params.clone().into_inner()
}
