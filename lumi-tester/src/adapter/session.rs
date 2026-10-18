use super::{AdapterResult, AdapterStatus, TargetAdapter, TargetScope};
use crate::parser::types::{TargetId, TestCommand};
use crate::runner::context::TestContext;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub session_id: String,
    pub phase: String,
    pub target: TargetId,
    pub adapter: String,
    pub status: AdapterStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

pub struct TargetSession {
    session_id: String,
    adapters: BTreeMap<TargetId, Box<dyn TargetAdapter>>,
    opened: BTreeSet<TargetId>,
    open_order: Vec<TargetId>,
    events: Vec<SessionEvent>,
}

impl TargetSession {
    pub fn from_adapters(adapters: Vec<Box<dyn TargetAdapter>>) -> anyhow::Result<Self> {
        let mut by_target = BTreeMap::new();
        for adapter in adapters {
            let target = adapter.target();
            if by_target.insert(target, adapter).is_some() {
                anyhow::bail!("duplicate adapter for target '{target}'");
            }
        }
        Ok(Self {
            session_id: format!("multi-target-{}", std::process::id()),
            adapters: by_target,
            opened: BTreeSet::new(),
            open_order: Vec::new(),
            events: Vec::new(),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn events(&self) -> &[SessionEvent] {
        &self.events
    }

    pub fn execute(
        &mut self,
        target: TargetId,
        command: &TestCommand,
        scope: TargetScope,
        context: &TestContext,
    ) -> anyhow::Result<AdapterResult> {
        let adapter = self
            .adapters
            .get_mut(&target)
            .ok_or_else(|| anyhow::anyhow!("no adapter registered for target '{target}'"))?;
        adapter.update_context(context);
        if !self.opened.contains(&target) {
            let preflight = adapter.preflight(context);
            self.events.push(event(
                &self.session_id,
                "preflight",
                target,
                adapter.adapter_name(),
                &preflight,
                None,
            ));
            if preflight.status != AdapterStatus::Passed {
                return Ok(preflight);
            }
            let opened = adapter.open(context);
            self.events.push(event(
                &self.session_id,
                "open",
                target,
                adapter.adapter_name(),
                &opened,
                None,
            ));
            if opened.status != AdapterStatus::Passed {
                return Ok(opened);
            }
            self.opened.insert(target);
            self.open_order.push(target);
        }
        let result = adapter.execute(command, &scope);
        self.events.push(event(
            &self.session_id,
            "execute",
            target,
            adapter.adapter_name(),
            &result,
            Some(scope.correlation_id),
        ));
        Ok(result)
    }

    pub fn close_all(&mut self, context: &TestContext) -> Vec<AdapterResult> {
        let mut results = Vec::new();
        while let Some(target) = self.open_order.pop() {
            if !self.opened.remove(&target) {
                continue;
            }
            if let Some(adapter) = self.adapters.get_mut(&target) {
                let result = adapter.close(context);
                self.events.push(event(
                    &self.session_id,
                    "close",
                    target,
                    adapter.adapter_name(),
                    &result,
                    None,
                ));
                results.push(result);
            }
        }
        results
    }
}

fn event(
    session_id: &str,
    phase: &str,
    target: TargetId,
    adapter: &str,
    result: &AdapterResult,
    correlation_id: Option<String>,
) -> SessionEvent {
    SessionEvent {
        session_id: session_id.into(),
        phase: phase.into(),
        target,
        adapter: adapter.into(),
        status: result.status,
        correlation_id,
    }
}
