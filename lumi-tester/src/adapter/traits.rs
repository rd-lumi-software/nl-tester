use crate::parser::types::{TargetId, TestCommand};
use crate::runner::context::TestContext;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterStatus {
    Passed,
    Failed,
    Blocked,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterResult {
    pub status: AdapterStatus,
    pub adapter: String,
    pub category: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_evidence: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_evidence: Option<serde_json::Value>,
}

impl AdapterResult {
    pub fn passed(adapter: impl Into<String>) -> Self {
        Self {
            status: AdapterStatus::Passed,
            adapter: adapter.into(),
            category: "completed".into(),
            message: "adapter operation completed".into(),
            evidence_tier: None,
            raw_evidence: None,
            normalized_evidence: None,
        }
    }

    pub fn blocked(
        adapter: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status: AdapterStatus::Blocked,
            adapter: adapter.into(),
            category: category.into(),
            message: message.into(),
            evidence_tier: Some("real_preflight".into()),
            raw_evidence: None,
            normalized_evidence: None,
        }
    }

    pub fn failed(
        adapter: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status: AdapterStatus::Failed,
            adapter: adapter.into(),
            category: category.into(),
            message: message.into(),
            evidence_tier: None,
            raw_evidence: None,
            normalized_evidence: None,
        }
    }

    pub fn error(adapter: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: AdapterStatus::Error,
            adapter: adapter.into(),
            category: "adapter_error".into(),
            message: message.into(),
            evidence_tier: None,
            raw_evidence: None,
            normalized_evidence: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetScope {
    pub target: TargetId,
    pub depth: usize,
    pub command_index: usize,
    pub correlation_id: String,
}

impl TargetScope {
    pub fn root(target: TargetId) -> Self {
        Self::nested(target, 0)
    }

    pub fn nested(target: TargetId, depth: usize) -> Self {
        Self {
            target,
            depth,
            command_index: 0,
            correlation_id: format!("{target}-{depth}-0"),
        }
    }
}

pub trait TargetAdapter: Send {
    fn target(&self) -> TargetId;
    fn platform(&self) -> &str;
    fn adapter_name(&self) -> &str;
    fn capabilities(&self) -> &[String];
    fn preflight(&mut self, context: &TestContext) -> AdapterResult;
    fn open(&mut self, context: &TestContext) -> AdapterResult;
    fn update_context(&mut self, _context: &TestContext) {}
    fn execute(&mut self, command: &TestCommand, scope: &TargetScope) -> AdapterResult;
    fn close(&mut self, context: &TestContext) -> AdapterResult;
}
