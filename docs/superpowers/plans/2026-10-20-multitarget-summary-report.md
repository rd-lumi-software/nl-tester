# Multi-target Summary Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every multi-target run use the existing summary HTML renderer and show generic flow/command information plus optional HC device/readback details.

**Architecture:** Keep `MultiTargetRunReport` and the current raw JSON artifacts as the execution source of truth. Add a conversion layer to the existing `TestResults` model, attach optional multi-target metadata, and render `report.html` through `summary_html` without changing YAML execution semantics or control behavior.

**Tech Stack:** Rust 2021, serde/serde_json, Tokio, existing `summary_html` renderer, Cargo integration tests.

---

### Task 1: Extend the report model with optional multi-target metadata

**Files:**
- Modify: `lumi-tester/src/report/types.rs`
- Modify: `lumi-tester/src/runner/executor.rs`
- Modify: `lumi-tester/src/report/junit.rs`
- Modify: `lumi-tester/src/report/json.rs`
- Modify: `lumi-tester/src/report/html.rs`
- Test: existing Rust literals that construct `TestResults`

- [x] **Step 1: Add the failing compile/test expectation**

Add an optional `multi_target` field to `TestResults` and define serializable metadata structs for adapter, repeat, operation, and HC evidence summaries. Add `multi_target: None` to existing `TestResults` fixtures only after the initial compile failure confirms all construction sites that must be updated.

- [x] **Step 2: Run the focused compile check**

Run:

```powershell
cargo test --manifest-path .\lumi-tester\Cargo.toml --no-run
```

Expected: compilation fails only at `TestResults` struct literals missing the new optional field.

- [x] **Step 3: Add the backward-compatible field and fixture values**

Use `#[serde(default, skip_serializing_if = "Option::is_none")]` so old JSON without metadata still deserializes. Set `multi_target: None` in non-adapter report constructors and keep all existing JSON field names unchanged.

- [x] **Step 4: Run focused compile and report tests**

Run:

```powershell
cargo test --manifest-path .\lumi-tester\Cargo.toml report --lib
```

Expected: PASS.

### Task 2: Convert multi-target operations into summary-compatible results

**Files:**
- Modify: `lumi-tester/src/adapter/runner.rs`
- Modify: `lumi-tester/src/adapter/mod.rs`
- Test: `lumi-tester/tests/multi_target_runner.rs`

- [x] **Step 1: Write a failing artifact test**

Extend the existing artifact test with assertions that `report.html` contains the summary renderer markers and that `test-results.json` contains a `multiTarget` metadata object. Assert that a passing mock operation is rendered as a passed command and that its adapter/target/profile are retained.

- [x] **Step 2: Run the focused test and verify the expected failure**

Run:

```powershell
cargo test --manifest-path .\lumi-tester\Cargo.toml --test multi_target_runner report_html -- --nocapture
```

Expected: FAIL because the current `report.html` is the counter-only placeholder and `multiTarget` metadata is absent.

- [x] **Step 3: Implement the conversion layer**

Add a private conversion function that builds `TestResults` from `MultiTargetRunReport`: create a summary-compatible flow, convert each operation status to `CommandStatus`, preserve command index/adapter/target/profile/evidence, calculate `TestSummary`, and derive optional HC metadata defensively from normalized evidence. Missing or malformed evidence must produce unavailable fields rather than a report-generation error.

- [x] **Step 4: Run the focused test and verify it passes**

Run:

```powershell
cargo test --manifest-path .\lumi-tester\Cargo.toml --test multi_target_runner report_html -- --nocapture
```

Expected: PASS.

### Task 3: Reuse `summary_html` for the multi-target HTML artifact

**Files:**
- Modify: `lumi-tester/src/report/summary_html.rs`
- Modify: `lumi-tester/src/adapter/runner.rs`
- Modify: `lumi-tester/src/report/mod.rs` if renderer export is needed
- Test: `lumi-tester/tests/multi_target_runner.rs`

- [x] **Step 1: Write a failing renderer test**

Add a fixture containing an HC control and wait operation with normalized values and assert that rendered HTML contains `lumi_hc`, the target/device identifier, control/readback labels, and a missing-evidence label for an operation without normalized evidence.

- [x] **Step 2: Run the focused test and verify it fails**

Run:

```powershell
cargo test --manifest-path .\lumi-tester\Cargo.toml --test multi_target_runner hc_summary -- --nocapture
```

Expected: FAIL because the current summary renderer has no multi-target/HC section.

- [x] **Step 3: Add a synchronous renderer entry point**

Expose a synchronous `render_summary_html` function from `summary_html.rs`. Keep the existing async `generate` function as a wrapper that calls the same renderer, so the normal executor and multi-target runner share one HTML implementation.

- [x] **Step 4: Render the optional multi-target sections**

Add sections after the generic summary content: adapter/repeat metadata, operation rows with sequence/repeat/target/command/status/evidence, and an HC device/readback table. Render `Not captured` for absent optional data and HTML-escape all values. Do not infer missing HC mappings or initial/final snapshots.

- [x] **Step 5: Replace the placeholder writer**

In `write_artifacts_with_options`, build the converted `TestResults`, write its JSON form to `test-results.json`, and write `summary_html::render_summary_html` to `report.html`. Preserve the current JUnit and events writers.

- [x] **Step 6: Run the focused renderer tests**

Run:

```powershell
cargo test --manifest-path .\lumi-tester\Cargo.toml --test multi_target_runner report_html hc_summary -- --nocapture
```

Expected: PASS.

### Task 4: Verify all existing behavior and generated artifacts

**Files:**
- Modify: none unless a regression is found
- Test: Rust test suite and generated mock output

- [x] **Step 1: Run the full Rust test suite**

Run:

```powershell
cargo test --manifest-path .\lumi-tester\Cargo.toml
```

Expected: PASS with no test failures.

Observed: 161 tests passed; the pre-existing Windows path test
`camera::launcher::tests::discovers_env_profile_and_camera_yaml_from_workspace`
failed at `src/camera/launcher.rs:187`. No camera source was changed by this
implementation.

- [x] **Step 2: Generate a mock multi-target report**

Run the existing mock multi-target test with `--nocapture`, then inspect `run.json`, `test-results.json`, and `report.html`. Expected: JSON artifacts remain parseable, `report.html` is larger than the old counter-only page, and the HTML includes generic operation data.

- [ ] **Step 3: Verify the existing `report` command input contract**

Run:

```powershell
cargo run --manifest-path .\lumi-tester\Cargo.toml -- report .\output\mock-multi-target\test-results.json
```

Expected: the command accepts the generated JSON without a deserialization error and can regenerate the same summary format.

- [x] **Step 4: Review the diff and preserve user changes**

Run:

```powershell
git diff -- lumi-tester/src/report lumi-tester/src/adapter/runner.rs lumi-tester/tests/multi_target_runner.rs
git status --short
```

Confirm no unrelated files are reverted and do not commit or push.
