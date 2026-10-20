# Multi-HC Protocol Adapter Refactor Implementation Plan

> For agentic workers: REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Refactor the HC gateway into a protocol-neutral command/backend boundary, preserve current lumi-iot-v3 behavior and YAML compatibility, and leave a tested extension point for HCG1 or another HC without implementing HCG1 in this phase.

**Architecture:** Keep TargetAdapter and the multi-target runner unchanged at the public boundary. Add protocol-neutral HC domain types and an HcProtocolBackend trait inside the gateway path. Move the existing v3 HTTP/WebSocket runtime behind V3HcProtocolBackend; select it from profile identity through a backend factory. Use a fake backend for deterministic contract tests.

**Tech Stack:** Rust 2021, Tokio, Reqwest, Tokio Tungstenite, Serde/Serde YAML/Serde JSON, existing Lumi Tester parser/runner/report/evidence code, Cargo unit and integration tests.

---

## File map

| File | Responsibility |
|---|---|
| lumi-tester/src/hc/mod.rs | HC domain module wiring and re-exports. |
| lumi-tester/src/hc/command.rs | Protocol-neutral health/discovery/state/control/wait commands. |
| lumi-tester/src/hc/result.rs | Normalized observations, backend result, capability, and evidence types. |
| lumi-tester/src/hc/backend.rs | HcProtocolBackend trait, HcExecutionScope, and factory errors. |
| lumi-tester/src/hc/factory.rs | Profile identity to backend registration and v3 construction. |
| lumi-tester/src/adapter/registry.rs | HC lifecycle facade and command/result conversion; no wire-format logic. |
| lumi-tester/src/hc/backends/v3/mod.rs | V3 backend owning the existing HTTP/WebSocket runtime. |
| lumi-tester/src/hc/backends/v3/runtime/profile.rs | V3 profile parsing and transport validation. |
| lumi-tester/src/hc/backends/v3/runtime/types.rs | V3 YAML command representation and conversion. |
| lumi-tester/tests/hc_backend_contract.rs | Fake backend and reusable backend contract tests. |
| lumi-tester/tests/e2e_hc_contract.rs | Existing v3 wire contract tests. |
| lumi-tester/tests/e2e_parser_profile.rs | Parser/profile compatibility and unknown-backend tests. |
| lumi-tester/tests/multi_target_runner.rs | Existing runner tests plus fake HC execution. |
| lumi-tester/schema/lumi-test.schema.json | Schema for the syntax actually accepted by the parser. |
| lumi-tester/docs/hc-adapter-development.md | Future backend author guide. |

Do not add HCG1 MQTT/TLS files in this phase. Do not change Android/iOS driver modules.

## Task 1: Freeze current behavior with regression tests

**Files:**
- Modify: lumi-tester/tests/e2e_hc_contract.rs
- Modify: lumi-tester/tests/e2e_parser_profile.rs
- Modify: lumi-tester/tests/multi_target_runner.rs

- [ ] Step 1: Add a v3 wire-contract test for control/readback.

Use the existing mock HTTP transport and ControlChange fixture. Assert one control POST, an indexed body containing idx and value, and a following state readback matching the requested value. Do not call a real HC.

- [ ] Step 2: Add an unsupported-platform assertion.

Assert that an unsupported old HC platform is rejected, while platform: lumi_hc with profiles/hc.yaml and hcHealth normalizes to TargetId::Gateway and preserves the profile reference.

- [ ] Step 3: Add a report-shape assertion.

Run the deterministic gateway fixture and assert the existing adapter identity, real_e2e success evidence tier, raw evidence, and normalized evidence fields.

- [ ] Step 4: Run the baseline.

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test e2e_hc_contract --test e2e_parser_profile --test multi_target_runner
~~~

Expected: all selected tests pass.

- [ ] Step 5: Commit.

~~~powershell
git add lumi-tester/tests/e2e_hc_contract.rs lumi-tester/tests/e2e_parser_profile.rs lumi-tester/tests/multi_target_runner.rs
git commit -m "test: freeze current HC gateway behavior"
~~~

## Task 2: Add protocol-neutral HC domain types

**Files:**
- Create: lumi-tester/src/hc/mod.rs
- Create: lumi-tester/src/hc/command.rs
- Create: lumi-tester/src/hc/result.rs
- Modify: lumi-tester/src/lib.rs
- Test: lumi-tester/tests/hc_backend_contract.rs

- [ ] Step 1: Define the semantic command model.

Create types equivalent to:

~~~rust
pub enum HcCommand {
    Health,
    GetEntities,
    GetState { entity: String },
    Control { entity: String, updates: Vec<HcStateUpdate> },
    WaitState { entity: String, expected: Vec<HcStateUpdate> },
}

pub enum HcStateUpdate {
    Indexed { idx: u32, value: serde_json::Value },
    Capability { name: String, value: serde_json::Value },
}
~~~

Keep Indexed as the representation of the current YAML contract. Capability is an extension point; a backend must advertise support before accepting it. The v3 backend must not silently reinterpret a capability update.

- [ ] Step 2: Define normalized observations and results.

Create HcEntity, HcState, HcObservation, HcEvidence, HcCapability, and HcBackendResult. Include status, message, raw evidence, normalized evidence, evidence tier, and optional readback match. Keep the HC domain independent from the multi-target adapter module; convert to AdapterResult only in the facade.

- [ ] Step 3: Add transport-free conversion tests.

Test that an existing indexed control representation becomes HcCommand::Control with HcStateUpdate::Indexed, and a capability representation remains HcStateUpdate::Capability. Test that the two variants cannot be confused.

- [ ] Step 4: Run focused tests.

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test hc_backend_contract
~~~

Expected: domain tests pass without HTTP, WebSocket, MQTT, or device dependencies.

- [ ] Step 5: Commit.

~~~powershell
git add lumi-tester/src/hc lumi-tester/src/lib.rs lumi-tester/tests/hc_backend_contract.rs
git commit -m "refactor: add protocol-neutral HC domain types"
~~~

## Task 3: Define the backend trait and factory

**Files:**
- Create: lumi-tester/src/hc/backend.rs
- Create: lumi-tester/src/hc/factory.rs
- Modify: lumi-tester/src/hc/mod.rs
- Modify: lumi-tester/src/hc/backends/v3/runtime/profile.rs
- Test: lumi-tester/tests/hc_backend_contract.rs
- Test: lumi-tester/tests/e2e_parser_profile.rs

- [ ] Step 1: Define the backend trait.

Use this boundary with imports from `crate::hc` and `crate::runner::context::TestContext`. The HC module owns HcExecutionScope so it does not depend on adapter modules:

~~~rust
pub trait HcProtocolBackend: Send {
    fn implementation_id(&self) -> &str;
    fn protocol_pack(&self) -> &str;
    fn capabilities(&self) -> &[HcCapability];
    fn preflight(&mut self, context: &TestContext) -> HcBackendResult;
    fn open(&mut self, context: &TestContext) -> HcBackendResult;
    fn execute(&mut self, command: &HcCommand, scope: &HcExecutionScope) -> HcBackendResult;
    fn close(&mut self, context: &TestContext) -> HcBackendResult;
}
~~~

The trait must not expose reqwest, tokio_tungstenite, MQTT types, HTTP paths, or v3 entity structs.

- [ ] Step 2: Add explicit factory errors.

Define errors for missing implementation identity, unsupported implementation, unsupported protocol pack, and malformed backend configuration. Preserve the profile field/value needed for stable diagnostics. Unknown identities must never select v3 by default.

- [ ] Step 3: Separate profile identity from transport configuration.

Retain current v3 values exactly:

~~~yaml
implementation: lumi_iot_v3_home_controller
protocolPack: lumi_iot_v3_home_controller
protocolVersion: 0.2.0
packVersion: 0.1.0
~~~

Do not add an HCG1 identity or pretend HCG1 configuration is supported.

- [ ] Step 4: Test factory selection and rejection.

Cover these cases:

1. the v3 profile selects the v3 registration;
2. an unknown implementation returns an unsupported-backend error;
3. a fake test registration can be selected without parser or runner changes;
4. an unsupported pack is rejected before runtime.

- [ ] Step 5: Run focused tests.

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test hc_backend_contract --test e2e_parser_profile
~~~

Expected: current v3 profile tests and factory tests pass.

- [ ] Step 6: Commit.

~~~powershell
git add lumi-tester/src/hc lumi-tester/tests/hc_backend_contract.rs lumi-tester/tests/e2e_parser_profile.rs
git commit -m "refactor: define extensible HC backend contract"
~~~

## Task 4: Move v3 execution behind the backend boundary

**Files:**
- Create: lumi-tester/src/hc/backends/v3/mod.rs
- Modify: lumi-tester/src/hc/backends/v3/runtime/mod.rs
- Modify: lumi-tester/src/hc/backends/v3/runtime/http.rs
- Modify: lumi-tester/src/hc/backends/v3/runtime/websocket.rs
- Modify: lumi-tester/src/hc/backends/v3/runtime/result.rs
- Modify: lumi-tester/src/hc/backends/v3/runtime/evidence.rs
- Modify: lumi-tester/src/hc/factory.rs
- Test: lumi-tester/tests/e2e_hc_contract.rs

- [ ] Step 1: Create V3HcProtocolBackend as the owner of V3Runtime.

Move or wrap existing runtime construction, profile loading, endpoint substitution, evidence writer setup, and HTTP/WebSocket execution in V3HcProtocolBackend. Keep v3 wire details inside this file or the existing transport modules. Implement the new trait and return HcBackendResult.

- [ ] Step 2: Preserve the v3 mapping.

~~~text
HcCommand::Health       -> GET /api/health_check
HcCommand::GetEntities  -> GET /api/devices
HcCommand::GetState     -> GET /api/devices/{id}/state
HcCommand::Control      -> POST /api/devices/{id}/control, indexed updates only
HcCommand::WaitState    -> bounded state polling and normalized assertion
~~~

Reject HcStateUpdate::Capability with a deterministic unsupported-capability result until a profile mapping exists. Preserve the rule that the control POST is not automatically retried and readback is bounded separately.

- [ ] Step 3: Preserve evidence and result semantics.

Successful v3 operations must still write raw and normalized evidence under the profile evidence root and report real_e2e. Blocked preflight must still report real_preflight and unavailable lab conditions must remain blocked rather than falsely passed.

- [ ] Step 4: Run all v3 contract tests.

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test e2e_hc_contract --test e2e_parser_profile
~~~

Expected: route, payload, retry, WebSocket, redaction, evidence, and parser/profile assertions pass with unchanged wire values.

- [ ] Step 5: Commit.

~~~powershell
git add lumi-tester/src/hc/backends/v3 lumi-tester/src/hc/factory.rs lumi-tester/tests/e2e_hc_contract.rs lumi-tester/tests/e2e_parser_profile.rs
git commit -m "refactor: route v3 HC execution through backend"
~~~

## Task 5: Convert the gateway adapter into a protocol-neutral facade

**Files:**
- Modify: lumi-tester/src/adapter/registry.rs
- Test: lumi-tester/tests/multi_target_runner.rs
- Test: lumi-tester/tests/multi_target_yaml.rs

- [ ] Step 1: Replace direct V3Runtime ownership.

Change HcGatewayAdapter to own a selected Box<dyn HcProtocolBackend> plus common profile/context state. It may retain profile and source paths for loading, but it must not call v3 HTTP or WebSocket methods directly.

- [ ] Step 2: Add one command conversion boundary.

Implement one private conversion function:

~~~text
TestCommand::V3(HcHealth)             -> HcCommand::Health
TestCommand::V3(HcGetEntities)       -> HcCommand::GetEntities
TestCommand::V3(HcGetState(params))  -> HcCommand::GetState { entity }
TestCommand::V3(HcControl(params))   -> HcCommand::Control { entity, updates }
TestCommand::V3(HcWaitState(params)) -> HcCommand::WaitState { entity, expected }
~~~

Keep WebSocket-specific commands either mapped to backend-neutral realtime commands or rejected at this boundary with the existing deterministic error. Do not add protocol checks to the multi-target runner.

- [ ] Step 3: Convert backend results to AdapterResult.

Preserve AdapterStatus, adapter name, category, message, evidence tier, raw evidence, and normalized evidence. The adapter name may include backend identity, but current v3 report expectations must remain compatible.

- [ ] Step 4: Test a fake backend through the existing runner.

Inject a fake backend through a test-only constructor. Run health, discovery, control, state, and wait. Assert that the runner sees one gateway target, the fake backend sees semantic commands rather than HTTP payloads, and the report retains existing operation/evidence fields.

- [ ] Step 5: Run gateway and multi-target tests.

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_yaml --test multi_target_runner --test e2e_hc_contract --test e2e_parser_profile
~~~

Expected: canonical/legacy routing, lifecycle, report, and HC contract tests pass.

- [ ] Step 6: Commit.

~~~powershell
git add lumi-tester/src/adapter/registry.rs lumi-tester/src/adapter/traits.rs lumi-tester/src/adapter/runner.rs lumi-tester/src/parser/validation.rs lumi-tester/tests/multi_target_yaml.rs lumi-tester/tests/multi_target_runner.rs
git commit -m "refactor: make HC gateway adapter protocol neutral"
~~~

## Task 6: Add the reusable backend contract harness

**Files:**
- Modify: lumi-tester/tests/hc_backend_contract.rs
- Create: lumi-tester/tests/fixtures/multi_target/profiles/hc-fake.yaml
- Create: lumi-tester/tests/fixtures/multi_target/hc-fake.yaml
- Modify: lumi-tester/src/hc/factory.rs for test-only registration hooks
- Test: lumi-tester/tests/hc_backend_contract.rs
- Test: lumi-tester/tests/multi_target_runner.rs

- [ ] Step 1: Implement a deterministic fake backend.

The fake backend records semantic commands, returns a fixed entity list, maintains an in-memory state map, and returns raw/normalized evidence. It supports the same lifecycle methods as a real backend and never requires a network, SSH, UART, MQTT broker, or physical HC.

- [ ] Step 2: Add shared contract tests.

Assert that:

1. preflight, open, execute, and close are called in order;
2. health and discovery return normalized observations;
3. control updates state and wait reads back the expected state;
4. unsupported capabilities produce a failure or blocked result according to the profile contract, never a false pass;
5. raw and normalized evidence are attached;
6. backend identity and capabilities are exposed to the facade.

- [ ] Step 3: Add a canonical fake HC flow.

~~~yaml
gateway:
  platform: lumi_hc
  profile: profiles/hc-fake.yaml
---
- hcHealth
- hcGetEntities
- hcControl:
    entity: switch-001
    state:
      - idx: 0
        value: true
- hcWaitState:
    entity: switch-001
    state:
      - idx: 0
        value: true
~~~

- [ ] Step 4: Prove extension behavior.

Register a second fake backend with a different implementation ID and run the same semantic flow. Parser and runner test code must remain unchanged; only backend/profile registration differs.

- [ ] Step 5: Run the contract suite.

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test hc_backend_contract --test multi_target_runner
~~~

Expected: both fake backends execute the same semantic sequence and produce normalized reports.

- [ ] Step 6: Commit.

~~~powershell
git add lumi-tester/src/hc/factory.rs lumi-tester/tests/hc_backend_contract.rs lumi-tester/tests/multi_target_runner.rs lumi-tester/tests/fixtures/multi_target
git commit -m "test: add reusable HC backend contract harness"
~~~

## Task 7: Update schema and developer documentation

**Files:**
- Create: lumi-tester/docs/hc-adapter-development.md
- Modify: lumi-tester/schema/lumi-test.schema.json
- Modify: lumi-tester/docs/writing_tests.md: add the HC compatibility note beside the existing HC YAML/profile guidance

- [ ] Step 1: Document the supported phase-1 boundary.

State that production support is lumi_iot_v3_home_controller over HTTP/WebSocket, platform: lumi_hc selects the gateway target, and old HC platform aliases are rejected. State explicitly that HCG1 is an extension target, not a supported runtime in this phase.

- [ ] Step 2: Document how to add a future backend.

Document the exact sequence:

1. add a backend module implementing HcProtocolBackend;
2. define transport configuration and identity;
3. register the identity in hc/factory.rs;
4. map protocol-neutral commands to wire operations;
5. normalize state and evidence;
6. add shared contract and wire-level tests;
7. update schema/profile documentation.

The future backend must not modify the parser or multi-target runner to support its transport.

- [ ] Step 3: Update schema only for implemented syntax.

Keep the current indexed state form valid. If capability updates are public in this implementation, add their exact object shape and validation examples; otherwise keep them internal and document that they are not public YAML syntax. Do not add HCG1-only keys.

- [ ] Step 4: Run schema and fixture validation.

~~~powershell
cargo run --manifest-path lumi-tester/Cargo.toml -- schema --json
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/hc-fake.yaml --json
~~~

Expected: schema generation succeeds and the fake HC fixture validates without a real device.

- [ ] Step 5: Commit.

~~~powershell
git add lumi-tester/docs/hc-adapter-development.md lumi-tester/schema/lumi-test.schema.json lumi-tester/docs/writing_tests.md README.md
git commit -m "docs: describe extensible HC adapter architecture"
~~~

## Task 8: Run final verification

**Files:**
- Modify: none unless verification exposes a regression

- [ ] Step 1: Run formatting and focused refactor tests.

~~~powershell
cargo fmt --manifest-path lumi-tester/Cargo.toml -- --check
cargo test --manifest-path lumi-tester/Cargo.toml --test hc_backend_contract --test e2e_hc_contract --test e2e_parser_profile --test multi_target_yaml --test multi_target_runner
~~~

Expected: formatting passes and every selected test passes.

- [ ] Step 2: Run the complete Rust suite.

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml
~~~

Expected: all required tests pass. Any unrelated pre-existing failure must be identified by test name and not relabeled as a refactor success.

- [ ] Step 3: Run repository pre-commit verification.

~~~powershell
./scripts/pre-commit-check.sh
~~~

Expected: every enabled gate completes successfully. If Windows cannot execute it directly, run it from the repository's supported Bash environment and record the exact failing gate if the environment is unavailable.

- [ ] Step 4: Verify fake-flow artifacts.

~~~powershell
cargo run --manifest-path lumi-tester/Cargo.toml -- run lumi-tester/tests/fixtures/multi_target/hc-fake.yaml --report --events-jsonl --output lumi-tester/output/hc-backend-refactor/fake
~~~

Inspect run.json, test-results.json, events.jsonl, and evidence files. Confirm semantic operation names, backend identity, normalized evidence, and a passing control/readback assertion.

- [ ] Step 5: Review the diff.

~~~powershell
git status --short
git diff --check
git log --oneline -8
~~~

Expected: only planned files changed, no credentials or real HC endpoints are committed, and the worktree contains intentional implementation commits only.

## Acceptance checklist

- [ ] Current v3 HC YAML validates without modification.
- [ ] Existing v3 HTTP/WebSocket contract tests pass behind V3HcProtocolBackend.
- [ ] Fake backend runs health, discovery, control, state, and readback through the existing runner.
- [ ] A second fake backend uses the same semantic flow with only backend/profile registration changed.
- [ ] Unknown backend identities fail deterministically.
- [ ] Existing report, evidence, status, and unsupported-platform behavior remains intact.
- [ ] Android/iOS code and YAML behavior are unchanged.
- [ ] No HCG1 MQTT/TLS or production protocol implementation is included.
- [ ] Future adapter instructions are documented at lumi-tester/docs/hc-adapter-development.md.
