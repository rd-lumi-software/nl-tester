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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetLifecycle {
    pub entity: String,
    pub capability: String,
    pub state: TargetState,
    pub quarantined: bool,
    pub quarantine_reason: Option<String>,
    pub events: Vec<LifecycleEvent>,
}

impl Default for TargetLifecycle {
    fn default() -> Self {
        Self::new("", "")
    }
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
        self.transition_with_reason(next, None)
    }

    pub fn transition_with_reason(
        &mut self,
        next: TargetState,
        reason: Option<String>,
    ) -> Result<(), String> {
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
            reason,
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

    pub fn validate_history(&self) -> Result<(), String> {
        if self.events.is_empty() {
            if self.state == TargetState::Discovered
                && !self.quarantined
                && self.quarantine_reason.is_none()
            {
                return Ok(());
            }
            return Err("lifecycle history is empty for a non-discovered target".into());
        }

        let mut state = TargetState::Discovered;
        let mut quarantined = false;
        let mut quarantine_reason = None;
        for event in &self.events {
            if event.from != state {
                return Err(format!(
                    "lifecycle event starts at {:?}, expected {:?}",
                    event.from, state
                ));
            }
            if event.from == event.to {
                if quarantined {
                    return Err("lifecycle contains an event after quarantine".into());
                }
                let Some(reason) = event.reason.as_deref() else {
                    return Err("quarantine event requires a reason".into());
                };
                let Some(reason) = reason.strip_prefix("quarantined: ") else {
                    return Err("same-state lifecycle event is not a quarantine event".into());
                };
                quarantined = true;
                quarantine_reason = Some(reason.to_string());
            } else {
                if quarantined || !is_allowed(state, event.to) {
                    return Err(format!(
                        "invalid lifecycle transition {:?} -> {:?}",
                        event.from, event.to
                    ));
                }
                state = event.to;
            }
        }

        if state != self.state {
            return Err(format!(
                "lifecycle state {:?} does not match history {:?}",
                self.state, state
            ));
        }
        if self.quarantined != quarantined {
            return Err("lifecycle quarantine flag does not match history".into());
        }
        if self.quarantine_reason != quarantine_reason {
            return Err("lifecycle quarantine reason does not match history".into());
        }
        Ok(())
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
            | (TargetState::Discovered, TargetState::NotRun)
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
