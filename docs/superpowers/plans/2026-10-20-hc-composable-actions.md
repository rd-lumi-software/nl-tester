# HC Composable Actions Implementation Plan

> **For agentic workers:** Use `subagent-driven-development` and
> `test-driven-development`. Do not commit or push; the user explicitly
> requested a working tree only.

**Goal:** Align HC with the existing Android/iOS/Web model: atomic YAML
commands are executed by the common runner and reusable behavior is composed by
`runFlow`; no orchestrator or backend layer silently inserts companion actions.

**Design:** `docs/superpowers/specs/2026-10-20-hc-composable-actions-design.md`

## Task 1: Capture the regression contract with tests

**Files:**

- Modify `lumi-tester/tests/orchestrator_plan.rs` only where existing tests
  currently assert the rejected action-catalog behavior.
- Add focused parser/runner tests in the existing test modules or a new
  `lumi-tester/tests/hc_composable_flow.rs` if no suitable module exists.

**Tests to add or preserve:**

1. Parsing a single `hcControl` produces one `TestCommand::V3` command.
2. Parsing the explicit four-layer sequence preserves the authored command
   order and count.
3. `runFlow` with inline HC commands is accepted by flow validation.
4. `runFlow` with a child HC YAML file is accepted and uses the same target
   scope; do not invent a second HC composition path.
5. `list --json` command entries do not require or expose `actionContract`.
6. No orchestrator plan validation expands `cycles` into hidden command nodes.

Run the smallest relevant tests and confirm failures before production edits.

## Task 2: Remove hidden HC action composition

**Files:**

- `lumi-tester/src/hc/action_contract.rs` (remove if it exists only for the
  in-progress auto-composition design).
- `lumi-tester/src/hc/mod.rs`.
- `lumi-tester/src/hc/backends/v3/runtime/types.rs`.
- `lumi-tester/src/main.rs`.
- `lumi-tester/src/orchestrator/plan.rs`.
- `lumi-tester/src/orchestrator/delegation.rs`.
- `.agents/skills/lumi-tester-agent/scripts/lumi_agent.py` and its synchronized
  Codex/Antigravity copy.
- Related tests/schema only where they describe the removed hidden contract.

**Implementation:**

1. Remove `AtomicActionContract`, `ActionCatalog`, `ExpandedAction`,
   `normalize_target_actions`, and automatic cycle expansion introduced by the
   rejected design.
2. Restore `V3CommandRequirement` to command name + capability metadata only.
3. Restore `list --json` to the existing generic command metadata; do not add
   a dependency graph to the public list contract.
4. Remove the Python pre-run gate that scans list output for action dependency
   metadata. Keep normal `validate`, `list`, and `run` behavior.
5. Preserve unrelated lifecycle, inventory, path-safety, failure-isolation,
   and aggregation hardening already present in the worktree.
6. Ensure plan `actions`/`cycles` remain semantic input data for delegation and
   are not converted by Rust into executable HC command nodes. Do not weaken
   existing safety checks unrelated to command composition.

Use `apply_patch` and compare each affected file against `HEAD` plus the
unrelated existing diff; do not broad-reset the worktree.

## Task 3: Make the explicit flow path the documented extension point

**Files:**

- `lumi-tester/e2e/hc/v3/` reusable flow examples as appropriate.
- `lumi-tester/docs/hc-v3-standalone-testing.md`.
- `lumi-tester/docs/orchestrator.md`.
- `.agents/skills/lumi-test-orchestrator/references/*.md` only for stale
  auto-composition claims.
- `.agents/skills/lumi-tester-agent/references/hc-execution.md` and mirrored
  skill copies if they mention hidden action insertion.

**Implementation:**

1. Add or update a reusable HC control subflow that explicitly lists
   `wsSubscribe`, `hcControl`, `wsExpect`, and `hcWaitState` when all layers are
   available.
2. Explain that `hcControl`'s internal HTTP readback is backend behavior, while
   independent WS/final-HTTP confirmation is authored as separate commands.
3. Add a small automation-rule composition example using placeholder future
   primitives, clearly marked as illustrative rather than implemented.
4. Document that a profile without WS or final API readback uses only supported
   atomic actions and relies on normal capability validation.

## Task 4: Verify and review

**Verification:**

1. Run `cargo fmt --check` for the Rust workspace.
2. Run focused parser/adapter/orchestrator tests with one job if needed.
3. Run `cargo test --manifest-path lumi-tester/Cargo.toml` if disk space
   permits; if not, report the exact limitation and run the focused suite.
4. Run `cargo run --manifest-path lumi-tester/Cargo.toml -- validate` and
   `list --json` against offline HC examples.
5. Run the Python syntax/test checks for both synchronized agent helper copies.
6. Inspect `git diff` and `git status`; confirm no commit/push occurred and no
   unrelated files were reverted.

The final report must distinguish source changes from pre-existing worktree
changes and must not claim a real HC run without a connected authorized device.
