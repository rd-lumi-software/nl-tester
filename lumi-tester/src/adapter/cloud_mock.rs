use super::{AdapterResult, TargetAdapter, TargetScope};
use crate::parser::types::{TargetId, TestCommand};
use crate::runner::context::TestContext;
use std::collections::BTreeMap;

pub struct CloudMockAdapter {
    state: BTreeMap<String, String>,
    capabilities: Vec<String>,
}

impl CloudMockAdapter {
    pub fn new() -> Self {
        Self {
            state: BTreeMap::new(),
            capabilities: vec!["deterministic_state".into()],
        }
    }

    fn mock_result(&self, command: &TestCommand) -> AdapterResult {
        let normalized = serde_json::json!({
            "command": command.display_name(),
            "state": self.state,
            "networkAccess": false
        });
        let mut result = AdapterResult::passed("cloud_mock");
        result.evidence_tier = Some("mock_contract".into());
        result.raw_evidence = Some(normalized.clone());
        result.normalized_evidence = Some(normalized);
        result
    }

    fn lifecycle_result(&self) -> AdapterResult {
        let mut result = AdapterResult::passed("cloud_mock");
        result.evidence_tier = Some("mock_contract".into());
        result
    }
}

impl Default for CloudMockAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TargetAdapter for CloudMockAdapter {
    fn target(&self) -> TargetId {
        TargetId::Cloud
    }

    fn platform(&self) -> &str {
        "lumi_cloud"
    }

    fn adapter_name(&self) -> &str {
        "cloud_mock"
    }

    fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    fn preflight(&mut self, _context: &TestContext) -> AdapterResult {
        self.lifecycle_result()
    }

    fn open(&mut self, _context: &TestContext) -> AdapterResult {
        self.lifecycle_result()
    }

    fn execute(&mut self, command: &TestCommand, _scope: &TargetScope) -> AdapterResult {
        if let TestCommand::SetVar(params) = command {
            self.state.insert(params.name.clone(), params.value.clone());
        }
        self.mock_result(command)
    }

    fn close(&mut self, _context: &TestContext) -> AdapterResult {
        self.lifecycle_result()
    }
}
