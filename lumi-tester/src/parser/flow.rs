use super::types::{RunFlowParams, RunFlowParamsInput};
use std::path::{Path, PathBuf};

pub fn run_flow_params(input: &RunFlowParamsInput) -> RunFlowParams {
    input.clone().into_inner()
}

pub fn child_flow_path(source: &Path, path: &str) -> PathBuf {
    source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

pub fn resolve_flow_path(base_dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

pub fn uses_inline_flow(params: &RunFlowParams) -> bool {
    params.commands.is_some()
}
