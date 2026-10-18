# Lumi E2E Device Control Implementation Plan

> **For agentic workers:** This plan is executed through the sequential planner workflow required by `D:\lumi-iot-v3\AGENTS.md` and `D:\lumi-iot-v3\.harness\ORCHESTRATION.md`. Do not commit, push, or create a pull request.

**Goal:** Extend the existing `lumi-tester` product with a reusable semantic E2E vertical slice for single-device control against the local `lumi-iot-v3` Home Controller implementation.

**Architecture:** Preserve the existing YAML parser, runner, App drivers, hardware support, reports, and backward-compatible flows. Add a separate E2E domain with profile resolution, capability preflight, protocol-pack mapping, HC HTTP/JSON and WebSocket/JSON adapters, a canonical device/Cell/state/event model, structured results, and raw/normalized evidence. Keep physical paths out of the semantic contract; profiles and environment configuration resolve local paths and endpoints.

**Tech Stack:** Rust 2021, Tokio, Reqwest, Tokio Tungstenite, Serde/Serde YAML/JSON, existing Clap CLI, existing report/event pipeline, Rust unit/contract tests, and deterministic mock HTTP/WebSocket fixtures where a real HC lab is unavailable.

---

## Scope and acceptance boundary

In scope:

- `platform: lumi_e2e` and `journey: system_journey` for single-device control.
- Profile loading and capability preflight.
- HC health, device discovery, state readback, control, and WebSocket realtime readback.
- Canonical mapping for v3 `cell_id`, `cell_model_id`, `cell_idx`, slot state, and events.
- `passed`, `failed`, `blocked`, and `error` result semantics with raw/normalized evidence.
- Unit, contract, conformance, and deterministic smoke coverage.
- SRS corrections and explicit deferred scope.

Out of scope for this goal:

- `user_journey` and App integration.
- Automation/rule, scene, schedule, and group control.
- Cloud, MQTT, SSH, serial, hardware jig, and production credentials.
- Automatic commit, push, tag, or pull request operations.

The SRS remains the source of product requirements, while `D:\\lumi-iot-v3` is the only implementation contract/reference for v3. The SRS's old absolute reference paths are documentation references only and must not become runtime paths.

## Task 1: Baseline current product and environment

**Files:**

- Read: `D:\\nl-tester\\AGENTS.md`
- Read: `D:\\nl-tester\\lumi-tester\\README.md`
- Read: `D:\\nl-tester\\lumi-tester\\docs\\architecture.md`
- Read: `D:\\nl-tester\\lumi-tester\\docs\\flows\\test_execution_flow.md`
- Read: `D:\\nl-tester\\2026-10-05-lumi-e2e-hc-srs.md`
- Read: `D:\\lumi-iot-v3\\AGENTS.md`
- Read: `D:\\lumi-iot-v3\\docs\\protocols\\realtime-websocket.md`
- Read: `D:\\lumi-iot-v3\\protocol\\docs\\core-spec\\v0.2.0\\protocol-core-spec-0.2.0.md`

- [ ] Record installed tool versions for Rust/Cargo, Node/npm, and any optional runtime tools.
- [ ] Record the current CLI commands and output artifacts that must remain backward compatible.
- [ ] Verify v3 routes and WebSocket frames from repository documentation/source; do not infer them from legacy projects.
- [ ] Record unresolved SRS ambiguities in the epic QA section instead of silently choosing behavior.

Verification commands:

```powershell
$env:Path = "C:\\Users\\Admin\\.cargo\\bin;C:\\Program Files\\nodejs;C:\\Program Files\\Git\\cmd;" + $env:Path
cargo metadata --manifest-path D:\\nl-tester\\lumi-tester\\Cargo.toml --no-deps --format-version 1
cargo test --manifest-path D:\\nl-tester\\lumi-tester\\Cargo.toml --lib
node --version
npm.cmd --version
```

## Task 2: Update SRS and freeze the device-control contract

**Files:**

- Modify: `D:\\nl-tester\\2026-10-05-lumi-e2e-hc-srs.md`
- Create: `D:\\nl-tester\\docs\\superpowers\\specs\\2026-10-18-lumi-e2e-device-control-contract.md`

- [ ] Add a current-product integration section mapping the existing CLI parser/runner/report pipeline to the new E2E domain.
- [ ] Define the exact Phase 1 header fields: `platform`, `journey`, `profile`, `defaultTimeout`, and `tags`.
- [ ] Define profile fields for implementation identity, protocol-pack identity, endpoint configuration, enabled capabilities, evidence root, and secret references without embedding credentials.
- [ ] Define semantic operations for health, discovery, state read, control, and realtime event/readback only.
- [ ] Define canonical device/Cell/state/event fields and the v3 mapping rules, including safe handling of v3 numeric IDs.
- [ ] Define result behavior: missing required capability/environment is `blocked`; product assertion mismatch is `failed`; runner/adapter inability to conclude is `error`; successful action/readback/evidence is `passed`.
- [ ] Mark App/user journey, rules, scenes, schedules, groups, Cloud, MQTT, SSH, serial, and hardware jig as deferred for this goal.
- [ ] Replace old absolute reference paths with logical identifiers or clearly label them as documentation-only examples.
- [ ] Add acceptance criteria that can be verified without a physical lab by using mock HTTP/WebSocket fixtures, while keeping real-lab E2E as a separate evidence tier.

The contract document must include at least one YAML example:

```yaml
platform: lumi_e2e
journey: system_journey
profile: profiles/lumi-iot-v3-home-controller.yaml
defaultTimeout: 15000
tags: [smoke, device-control]
---
- hcHealth
- hcGetEntities
- hcGetState:
    entity: "${CELL_ID}"
- hcControl:
    entity: "${CELL_ID}"
    capability: power
    value: true
- wsExpect:
    entity: "${CELL_ID}"
    event: state_changed
- hcWaitState:
    entity: "${CELL_ID}"
    state:
      - idx: 1
        value: true
```

## Task 3: Design the E2E domain boundary

**Files:**

- Read: `D:\\nl-tester\\lumi-tester\\src\\main.rs`
- Read: `D:\\nl-tester\\lumi-tester\\src\\parser\\types.rs`
- Read: `D:\\nl-tester\\lumi-tester\\src\\runner\\executor.rs`
- Read: `D:\\nl-tester\\lumi-tester\\src\\runner\\state.rs`
- Read: `D:\\nl-tester\\lumi-tester\\src\\report\\types.rs`
- Create design evidence: the active `.process/<timestamp>_.../tasks/...` file in `D:\\lumi-iot-v3`

- [ ] Decide whether E2E commands are represented as an extension of `TestCommand` or dispatched through a dedicated `E2eCommand` branch while preserving old commands.
- [ ] Define the adapter traits and ownership boundaries before implementation:
  - profile resolver;
  - capability preflight;
  - HC HTTP adapter;
  - HC WebSocket adapter;
  - protocol-pack mapping;
  - canonical model normalizer;
  - evidence collector;
  - result classifier.
- [ ] Define how E2E session state is carried without leaking HC-specific state into App drivers.
- [ ] Define timeout, retry, reconnect, cancellation, and correlation behavior.
- [ ] Define how E2E statuses are serialized into existing JSON/HTML/JUnit reports and machine-readable events.
- [ ] Reject design alternatives that hard-code v3 routes into generic semantic commands or create a second incompatible YAML runner.

## Task 4: Add parser/profile and CLI support

**Files:**

- Modify: `D:\\nl-tester\\lumi-tester\\src\\parser\\types.rs`
- Modify: `D:\\nl-tester\\lumi-tester\\src\\parser\\yaml.rs`
- Modify: `D:\\nl-tester\\lumi-tester\\src\\main.rs`
- Create/modify: `D:\\nl-tester\\lumi-tester\\src\\e2e\\profile.rs`
- Create/modify: `D:\\nl-tester\\lumi-tester\\src\\e2e\\types.rs`
- Create: `D:\\nl-tester\\lumi-tester\\e2e\\workspaces\\lumi_e2e\\profiles\\lumi-iot-v3-home-controller.yaml`
- Create: `D:\\nl-tester\\lumi-tester\\e2e\\workspaces\\lumi_e2e\\system\\device_control_smoke.yaml`

- [ ] Parse `platform: lumi_e2e` without changing existing platform parsing.
- [ ] Parse `journey: system_journey` and reject unsupported journey/command combinations with actionable validation errors.
- [ ] Resolve profile paths relative to the YAML flow, never relative to a hard-coded checkout path.
- [ ] Validate required profile fields and capability declarations before connecting to a target.
- [ ] Make `validate --json` and `list --json` report E2E commands and profile/capability requirements.
- [ ] Preserve existing App-only YAML behavior and existing command indexes.
- [ ] Add parser tests for valid device-control flow, missing profile, unsupported capability, malformed entity/state values, and legacy YAML compatibility.

## Task 5: Implement the canonical model and protocol pack

**Files:**

- Create/modify: `D:\\nl-tester\\lumi-tester\\src\\e2e\\canonical.rs`
- Create/modify: `D:\\nl-tester\\lumi-tester\\src\\e2e\\protocol_pack.rs`
- Create/modify: `D:\\nl-tester\\lumi-tester\\src\\e2e\\result.rs`
- Create/modify: `D:\\nl-tester\\lumi-tester\\src\\e2e\\evidence.rs`

- [ ] Represent entity identity, Cell metadata, indexed state, event type, timestamp, source, and optional correlation ID.
- [ ] Preserve raw JSON values and normalize only fields declared by the v3 pack.
- [ ] Treat v3 identifiers as safe integer/string-compatible values; do not coerce them through JavaScript numbers.
- [ ] Map v3 Cell/input/state/command/config/event structures to canonical references without guessing aliases such as `power`.
- [ ] Map HTTP errors `{code,msg}`, WebSocket close/parse errors, unsupported capabilities, timeouts, and assertion mismatches to structured error categories.
- [ ] Redact configured secrets before writing evidence.

## Task 6: Implement HC HTTP/JSON and WebSocket/JSON adapters

**Files:**

- Create: `D:\\nl-tester\\lumi-tester\\src\\e2e\\adapter\\mod.rs`
- Create: `D:\\nl-tester\\lumi-tester\\src\\e2e\\adapter\\http.rs`
- Create: `D:\\nl-tester\\lumi-tester\\src\\e2e\\adapter\\websocket.rs`
- Create/modify: `D:\\nl-tester\\lumi-tester\\src\\e2e\\session.rs`
- Modify: `D:\\nl-tester\\lumi-tester\\src\\runner\\executor.rs`

- [ ] Implement health/readiness against the configured v3 route.
- [ ] Implement discovery, state readback, and single-device control through the v3 HTTP contract.
- [ ] Implement WebSocket connect, subscribe, frame parsing, event matching, timeout, reconnect, resubscribe, and close.
- [ ] Keep HTTP/WS raw request/response/frame evidence bounded and redacted.
- [ ] Normalize successful responses/events through the v3 protocol pack.
- [ ] Classify unavailable required capability or unreachable lab as `blocked` only during preflight; classify runtime adapter failures as `error` unless the product assertion itself fails.
- [ ] Keep App, Cloud, MQTT, group, scene, rule, schedule, SSH, serial, and hardware-jig code paths untouched except for shared interfaces required by the design.

## Task 7: Integrate result, event, and report output

**Files:**

- Modify: `D:\\nl-tester\\lumi-tester\\src\\runner\\state.rs`
- Modify: `D:\\nl-tester\\lumi-tester\\src\\runner\\events.rs`
- Modify: `D:\\nl-tester\\lumi-tester\\src\\report\\types.rs`
- Modify: `D:\\nl-tester\\lumi-tester\\src\\report\\json.rs`
- Modify: `D:\\nl-tester\\lumi-tester\\src\\report\\html.rs`

- [ ] Add first-class E2E result categories without changing the meaning of existing App flow statuses.
- [ ] Include profile, implementation, protocol pack, capability preflight, raw evidence references, normalized readback, retry count, and blocker/error category.
- [ ] Keep `run.json`, `test-results.json`, HTML, JUnit, and `events.jsonl` machine-readable and backward-compatible for old flows.
- [ ] Add report tests for passed, failed, blocked, and error E2E cases.

## Task 8: Build deterministic test harness and coverage

**Files:**

- Create: `D:\\nl-tester\\lumi-tester\\tests\\e2e_device_control.rs`
- Create: `D:\\nl-tester\\lumi-tester\\tests\\fixtures\\lumi_iot_v3\\...`
- Create: `D:\\nl-tester\\lumi-tester\\e2e\\workspaces\\lumi_e2e\\system\\device_control_smoke.yaml`
- Modify: `D:\\nl-tester\\lumi-tester\\docs\\writing_tests.md`
- Modify: `D:\\nl-tester\\lumi-tester\\docs\\cli-reference.md`

- [ ] Test parser/profile validation with no external service.
- [ ] Test HTTP success/error response normalization with a local deterministic mock server.
- [ ] Test WebSocket subscribe, event matching, reconnect, resubscribe, timeout, and malformed frame behavior with a local deterministic mock server.
- [ ] Test control round-trip and canonical state assertions.
- [ ] Test that absent real lab capability yields `blocked`, not `passed` or `failed`.
- [ ] Add a real-lab command recipe that uses environment/profile values and is not run unless the target is available.
- [ ] Document validate → list → doctor → run → inspect evidence flow for `system_journey`.

Expected local verification:

```powershell
$env:Path = "C:\\Users\\Admin\\.cargo\\bin;C:\\Program Files\\nodejs;C:\\Program Files\\Git\\cmd;" + $env:Path
cargo fmt --manifest-path D:\\nl-tester\\lumi-tester\\Cargo.toml -- --check
cargo test --manifest-path D:\\nl-tester\\lumi-tester\\Cargo.toml --lib
cargo test --manifest-path D:\\nl-tester\\lumi-tester\\Cargo.toml --test e2e_device_control
cargo run --manifest-path D:\\nl-tester\\lumi-tester\\Cargo.toml -- validate D:\\nl-tester\\lumi-tester\\e2e\\workspaces\\lumi_e2e\\system\\device_control_smoke.yaml --json
cargo run --manifest-path D:\\nl-tester\\lumi-tester\\Cargo.toml -- list D:\\nl-tester\\lumi-tester\\e2e\\workspaces\\lumi_e2e\\system\\device_control_smoke.yaml --json
```

## Task 9: Review, integration, and completion audit

**Files:**

- Read/verify all changed files in `D:\\nl-tester`.
- Read/verify evidence under the active `.process` epic in `D:\\lumi-iot-v3`.

- [ ] Run the code-reviewer after every implementation task as required by the harness.
- [ ] Run the complete Rust test suite and targeted E2E harness tests.
- [ ] Run existing App/hardware parser and report tests to prove backward compatibility.
- [ ] Run validation/listing for legacy YAML and new `lumi_e2e` YAML.
- [ ] Attempt real v3 preflight only if a local HC endpoint/profile is available; otherwise record `blocked` with the missing capability and keep mock evidence separate.
- [ ] Verify no source, test, docs, or script performs commit, push, tag, or PR operations.
- [ ] Verify SRS acceptance criteria one by one against concrete test/report evidence before marking the goal complete.
