# Lumi Orchestrator Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven development and test-driven development to implement this plan task-by-task.

**Goal:** Make Lumi HC orchestration machine-verifiable without moving HC transport or YAML execution into the orchestrator.

**Architecture:** Add a pure Rust `orchestrator` module for semantic plan validation, target lifecycle, inventory comparison, delegation-response validation, and evidence aggregation. Keep `lumi-tester-agent` responsible for YAML authoring and Lumi execution. Add an HC-safe mode to the Python helper that uses `hcHealth` instead of app/browser `doctor`.

**Tech Stack:** Rust 2021, `serde`, `serde_yaml`, `serde_json`, Clap; Python `argparse`; existing Lumi CLI artifacts and test fixtures.

---

### Task 1: Add HC-safe helper behavior

**Files:**
- Modify: `.agents/skills/lumi-tester-agent/scripts/lumi_agent.py`
- Modify: `lumi-tester/ai/codex-skill/lumi-tester-agent/scripts/lumi_agent.py`
- Modify: `lumi-tester/ai/antigravity-skill/lumi-tester-agent/scripts/lumi_agent.py`
- Modify: matching AI skill docs and `lumi-tester/scripts/check-ai-reference.py` only where required by existing sync checks

- [x] Add separate run and doctor platform lists; allow `lumi_hc` for run/debug but reject it for doctor with an actionable message.
- [x] Add `agent-hc-preflight` which runs validate/list, requires `hcHealth`, rejects mutation commands, executes with report/events/snapshot artifacts, and emits an explicit `hcPreflight` summary.
- [x] Add deterministic helper-level checks for accepted HC run, rejected HC doctor, missing health, and mutation in preflight.
- [x] Keep the three helper copies behaviorally synchronized without removing platform-specific commands.

### Task 2: Implement semantic plan validation

**Files:**
- Create: `lumi-tester/src/orchestrator/plan.rs`
- Create: `lumi-tester/schema/orchestrator-test-plan.schema.json`
- Create: `lumi-tester/tests/orchestrator_plan.rs`
- Create: `lumi-tester/tests/fixtures/orchestrator/valid-plan.yaml`
- Create: invalid plan fixtures for duplicate entities, numeric entities, invalid repeat, missing reasons, invalid parallel policy, and unsafe restoration.

- [x] Define typed plan structures with forward-compatible mapping/cycle fields.
- [x] Validate entity IDs as strings, unique selection, target reasons, repeat limits, parallel limits, endpoint/connection completeness, mapping source, inventory readiness, and restoration contract.
- [x] Return `valid`, `readyForRun`, structured errors, and warnings; keep `--for-run` stricter than authoring validation.
- [x] Add red/green tests for each safety rule.

### Task 3: Implement lifecycle, inventory, delegation, and aggregate models

**Files:**
- Create: `lumi-tester/src/orchestrator/lifecycle.rs`
- Create: `lumi-tester/src/orchestrator/inventory.rs`
- Create: `lumi-tester/src/orchestrator/delegation.rs`
- Create: `lumi-tester/src/orchestrator/aggregate.rs`
- Create: `lumi-tester/tests/orchestrator_runtime.rs`
- Create: JSON fixtures under `lumi-tester/tests/fixtures/orchestrator/`

- [x] Implement explicit target transitions and quarantine rules.
- [x] Implement pure inventory comparison with added/removed/capability/fingerprint/mapping drift and unverifiable snapshots.
- [x] Validate delegation responses against plan ID, phase, statuses, command indexes, artifact paths, evidence tier, restore verification, and unresolved assumptions.
- [x] Aggregate target outcomes without collapsing `blocked`, `failed`, `error`, `skipped`, or `not_run`; preserve missing measurements as null/not-captured.
- [x] Add tests for independent target continuation and run-level stop behavior.

### Task 4: Wire the orchestration CLI

**Files:**
- Create/modify: `lumi-tester/src/orchestrator/mod.rs`
- Modify: `lumi-tester/src/lib.rs`
- Modify: `lumi-tester/src/main.rs`
- Create: `lumi-tester/tests/orchestrator_cli.rs`

- [x] Add `lumi-tester plan validate <plan> [--for-run] --json`.
- [x] Add `lumi-tester plan validate-delegation <plan> <response> --json`.
- [x] Add `lumi-tester plan aggregate <plan> --response <file> --inventory-current <file> --output <dir> --json`.
- [x] Add `lumi-tester plan schema --json`.
- [x] Preserve existing CLI behavior and add command-level tests.

### Task 5: Make restoration and evidence semantics explicit

**Files:**
- Modify: `.agents/skills/lumi-test-orchestrator/SKILL.md`
- Modify: orchestrator reference contracts under `.agents/skills/lumi-test-orchestrator/references/`
- Create/modify: `lumi-tester/docs/orchestrator.md`

- [x] Document that the Rust module validates evidence/contracts but does not call HC or synthesize restore payloads.
- [x] Require `restoreStatus: not_requested` when restoration is disabled and require captured-state evidence when enabled.
- [x] Document inventory snapshot/fingerprint format and aggregate output.
- [x] Document HC helper usage and the distinction between `doctor` and `hcHealth`.

### Task 6: Verification

- [x] Run targeted Rust tests for all orchestrator modules and CLI tests.
- [ ] Run helper reference synchronization checks (blocked: Python is unavailable in this environment).
- [x] Run HC fixture validate/list offline through the existing Rust contract tests.
- [ ] Run the repository pre-commit verification pipeline (blocked: Bash/WSL is unavailable in this environment).
- [x] Review the final diff and ensure no real-device mutation was performed.
