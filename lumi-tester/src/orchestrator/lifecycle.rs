use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetState {
    Discovered,
    Eligible,
    Blocked,
    Skipped,
    Queued,
    Running,
    Passed,
    Failed,
    Error,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleEvent {
    pub from: TargetState,
    pub to: TargetState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetLifecycle {
    pub entity: String,
    pub capability: String,
    pub state: TargetState,
    pub quarantined: bool,
    pub quarantine_reason: Option<String>,
    pub events: Vec<LifecycleEvent>,
}

impl TargetLifecycle {
    pub fn new(entity: impl Into<String>, capability: impl Into<String>) -> Self {
        Self {
            entity: entity.into(),
            capability: capability.into(),
            state: TargetState::Discovered,
            quarantined: false,
            quarantine_reason: None,
            events: Vec::new(),
        }
    }

    pub fn transition(&mut self, next: TargetState) -> Result<(), String> {
        if self.quarantined {
            return Err(format!("target {} is quarantined", self.entity));
        }
        if !is_allowed(self.state, next) {
            return Err(format!(
                "invalid target transition {:?} -> {:?}",
                self.state, next
            ));
        }
        let from = self.state;
        self.state = next;
        self.events.push(LifecycleEvent {
            from,
            to: next,
            reason: None,
        });
        Ok(())
    }

    pub fn state(&self) -> TargetState {
        self.state
    }

    pub fn is_quarantined(&self) -> bool {
        self.quarantined
    }

    pub fn can_mutate(&self) -> bool {
        matches!(self.state, TargetState::Queued | TargetState::Running) && !self.quarantined
    }

    pub fn apply_shared_stop(targets: &mut [TargetLifecycle], reason: impl Into<String>) {
        let reason = reason.into();
        for target in targets {
            if matches!(
                target.state,
                TargetState::Discovered | TargetState::Eligible | TargetState::Queued
            ) {
                let from = target.state;
                target.state = TargetState::NotRun;
                target.events.push(LifecycleEvent {
                    from,
                    to: TargetState::NotRun,
                    reason: Some(reason.clone()),
                });
            }
        }
    }

    pub fn quarantine(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.quarantined = true;
        self.quarantine_reason = Some(reason.clone());
        self.events.push(LifecycleEvent {
            from: self.state,
            to: self.state,
            reason: Some(format!("quarantined: {reason}")),
        });
    }
}

fn is_allowed(from: TargetState, to: TargetState) -> bool {
    matches!(
        (from, to),
        (TargetState::Discovered, TargetState::Eligible)
            | (TargetState::Discovered, TargetState::Blocked)
            | (TargetState::Discovered, TargetState::Skipped)
            | (TargetState::Eligible, TargetState::Queued)
            | (TargetState::Eligible, TargetState::Blocked)
            | (TargetState::Eligible, TargetState::Skipped)
            | (TargetState::Eligible, TargetState::NotRun)
            | (TargetState::Queued, TargetState::Running)
            | (TargetState::Queued, TargetState::NotRun)
            | (TargetState::Running, TargetState::Passed)
            | (TargetState::Running, TargetState::Failed)
            | (TargetState::Running, TargetState::Error)
            | (TargetState::Running, TargetState::NotRun)
    )
}
