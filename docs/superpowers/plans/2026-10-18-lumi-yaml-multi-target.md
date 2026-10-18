# Lumi YAML Multi-Target Implementation Plan

> **For agentic workers:** Use the repository's TDD workflow for every behavior change. Work in the stage order planner -> developer -> reviewer -> tester. Do not commit, push, create a PR, reset, checkout, rewrite history, or create target-codex/target-review directories.

**Goal:** Add canonical YAML components, target-aware recursive composition, shared App/HC/Cloud sessions, and backward-compatible reporting without breaking existing App flows or HC evidence contracts.

**Architecture:** Normalize legacy and canonical headers into one component map. Build one parent adapter registry/session, retain the existing App PlatformDriver, place the HC implementation behind a lumi_hc gateway adapter, and expose only a deterministic Cloud mock. Route recursive flows by explicit runFlow.target, with child-header isolation and additive target/status/evidence metadata.

**Tech Stack:** Rust workspace, Cargo tests, serde YAML/JSON, existing Lumi Tester parser/runner/report/event infrastructure, existing HC HTTP/WebSocket mocks, deterministic fixture harness, JSONL/JSON/HTML/JUnit artifacts.

---

## 0. Stage contract and exact file map

This plan is executed only after the approved design document:

docs/superpowers/specs/2026-10-18-lumi-yaml-multi-target-design.md

No source changes are made during the planner stage that writes this plan. The
later developer stage must preserve all pre-existing dirty changes listed by
git status.

### Files to modify

- lumi-tester/src/parser/types.rs — normalized component types, target IDs,
  target-aware RunFlowParams, legacy metadata.
- lumi-tester/src/parser/yaml.rs — legacy/new header parsing, target parsing,
  child-header isolation input.
- lumi-tester/src/parser/mod.rs — expose normalization/validation modules when
  required by the existing module layout.
- lumi-tester/src/main.rs — normalized validate/list output, deprecation
  diagnostics, multi-target platform inference, status reporting.
- lumi-tester/src/runner/context.rs — target scope and shared session context.
- lumi-tester/src/runner/mod.rs — create one registry/session while preserving
  App driver creation.
- lumi-tester/src/runner/executor.rs — recursive target dispatch, lifecycle,
  blocked/error preservation, child-header isolation.
- lumi-tester/src/runner/state.rs — target-aware command/flow/session states and
  Blocked/Error distinctions.
- lumi-tester/src/runner/events.rs — additive target lifecycle and target fields.
- lumi-tester/src/report/types.rs — component, target, evidence, and result
  status fields.
- lumi-tester/src/report/json.rs — additive JSON mapping and blocked/error
  summaries.
- lumi-tester/src/report/html.rs — target sections and status rendering.
- lumi-tester/src/report/junit.rs — preserve standard failures and represent
  blocked results without converting them to product failures.
- lumi-tester/src/report/summary_html.rs — target/evidence summary.
- lumi-tester/schema/lumi-test.schema.json — component keys, target enum,
  profiles, deprecated journey, and runFlow target.
- 2026-10-05-lumi-e2e-hc-srs.md — component model, recursive composition,
  dispatch, migration, evidence, and report contract.

### Files to create

- lumi-tester/src/parser/normalize.rs — legacy/new schema normalization.
- lumi-tester/src/parser/validation.rs — component, target, recursive graph,
  and command compatibility validation.
- lumi-tester/src/adapter/mod.rs — adapter module boundary.
- lumi-tester/src/adapter/traits.rs — adapter and lifecycle interfaces.
- lumi-tester/src/adapter/registry.rs — (target, platform) factories.
- lumi-tester/src/adapter/session.rs — shared parent session and target scopes.
- lumi-tester/src/adapter/lifecycle.rs — open/preflight/close state handling.
- lumi-tester/src/adapter/cloud_mock.rs — deterministic Cloud boundary.
- lumi-tester/tests/multi_target_yaml.rs — parser, normalization, schema, and
  validation tests.
- lumi-tester/tests/multi_target_runner.rs — mock lifecycle, routing, report,
  event, and evidence tests.
- lumi-tester/tests/fixtures/multi_target/app.yaml — App child fixture.
- lumi-tester/tests/fixtures/multi_target/gateway.yaml — HC child fixture.
- lumi-tester/tests/fixtures/multi_target/cloud.yaml — Cloud mock child fixture.
- lumi-tester/tests/fixtures/multi_target/parent.yaml — App/gateway/Cloud
  orchestration fixture.
- lumi-tester/tests/fixtures/multi_target/profiles/hc.yaml — deterministic HC
  mock profile.
- lumi-tester/tests/fixtures/multi_target/profiles/cloud.yaml — deterministic
  Cloud mock profile.
- lumi-tester/tests/fixtures/multi_target/invalid-missing-target.yaml — static
  validation failure fixture.
- lumi-tester/tests/fixtures/multi_target/invalid-unknown-target.yaml — static
  validation failure fixture.
- lumi-tester/tests/fixtures/multi_target/child-header-conflict.yaml — driver
  isolation fixture.

Existing lumi-tester/src/e2e/*.rs and current HC tests are modified only when
needed to implement the generic gateway adapter boundary. Their HC wire facts are
checked only against D:\lumi-iot-v3.

## 1. Planner gate — freeze the approved contract

**Owner:** planner

**Files:**

- Read: docs/superpowers/specs/2026-10-18-lumi-yaml-multi-target-design.md
- Read: AGENTS.md
- Read: 2026-10-05-lumi-e2e-hc-srs.md
- Read: D:\lumi-iot-v3\AGENTS.md
- Read: D:\lumi-iot-v3\bin\home-controller\src\transport\http\*.rs
- Read: D:\lumi-iot-v3\bin\home-controller\src\transport\websocket\*.rs

- [ ] Verify that the design contains all requested semantics: legacy/new App,
  optional journey, recursive target routing, child-header isolation, shared
  session, HC-only reference boundary, Cloud mock, lifecycle, report/event/
  evidence, validation, harness, migration, and no-real-environment policy.
- [ ] Record the current dirty state without changing it:

~~~powershell
git status --short --untracked-files=all
git diff --stat
~~~

Expected evidence: a status snapshot showing the pre-existing changes and no
planner-created source/test/schema modifications.

## 2. Developer RED — write normalization tests first

**Owner:** developer

**Files:**

- Create: lumi-tester/tests/multi_target_yaml.rs
- Create: lumi-tester/tests/fixtures/multi_target/app.yaml
- Create: lumi-tester/tests/fixtures/multi_target/gateway.yaml
- Create: lumi-tester/tests/fixtures/multi_target/cloud.yaml
- Create: lumi-tester/tests/fixtures/multi_target/parent.yaml
- Create: lumi-tester/tests/fixtures/multi_target/profiles/hc.yaml
- Create: lumi-tester/tests/fixtures/multi_target/profiles/cloud.yaml

Write focused failing tests before adding production normalization code. The test
names and required assertions are:

~~~text
legacy_app_header_normalizes_to_one_app_component
new_app_component_normalizes_to_the_same_app_adapter_shape
peer_components_normalize_to_app_gateway_cloud
journey_is_warning_metadata_and_not_adapter_selection
legacy_lumi_e2e_profile_normalizes_to_gateway_hc
conflicting_legacy_app_fields_are_rejected
~~~

Use these exact fixture behaviors:

~~~yaml
platform: android
appId: com.lumi.lumilife
---
- launchApp
~~~

~~~yaml
app:
  platform: android
  appId: com.lumi.lumilife
gateway:
  platform: lumi_hc
  profile: profiles/hc.yaml
cloud:
  platform: lumi_cloud
  profile: profiles/cloud.yaml
---
- runFlow:
    path: app.yaml
    target: app
~~~

Run the RED test command:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_yaml legacy_app_header_normalizes_to_one_app_component -- --exact
~~~

Expected result: compilation/test failure because the normalized component API is
not implemented yet. Correct fixture or test syntax errors until the failure is
specifically about the missing behavior.

## 3. Developer GREEN — implement canonical normalization

**Owner:** developer

**Files:**

- Modify: lumi-tester/src/parser/types.rs
- Modify: lumi-tester/src/parser/yaml.rs
- Create: lumi-tester/src/parser/normalize.rs
- Modify: lumi-tester/src/parser/mod.rs

Implement the smallest API that satisfies the RED tests:

~~~rust
pub enum TargetId { App, Gateway, Cloud }

pub struct ComponentSpec {
    pub target: TargetId,
    pub platform: String,
    pub app_id: Option<String>,
    pub profile: Option<PathBuf>,
}

pub struct NormalizedFlow {
    pub components: BTreeMap<TargetId, ComponentSpec>,
    pub commands: Vec<TestCommand>,
    pub legacy_metadata: LegacyMetadata,
}
~~~

Implement these exact normalization rules:

1. Legacy platform/appId creates TargetId::App.
2. Canonical app, gateway, and cloud create peer entries.
3. journey is retained as metadata and emits a deprecation diagnostic.
4. lumi_e2e is accepted only as a legacy HC alias.
5. Matching duplicate App declarations are accepted; conflicting values return a
   structured parser/validation error.
6. Component profile paths resolve relative to the declaring YAML file.

Run the focused GREEN command and then all parser tests:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_yaml
cargo test --manifest-path lumi-tester/Cargo.toml --lib parser
~~~

Expected result: all normalization tests pass and existing parser tests remain
green. Refactor only after these tests are green.

## 4. Developer RED/GREEN — schema and static validation

**Owner:** developer

**Files:**

- Create: lumi-tester/src/parser/validation.rs
- Modify: lumi-tester/src/parser/yaml.rs
- Modify: lumi-tester/src/main.rs
- Modify: lumi-tester/schema/lumi-test.schema.json
- Modify: lumi-tester/tests/multi_target_yaml.rs
- Create: lumi-tester/tests/fixtures/multi_target/invalid-missing-target.yaml
- Create: lumi-tester/tests/fixtures/multi_target/invalid-unknown-target.yaml
- Create: lumi-tester/tests/fixtures/multi_target/child-header-conflict.yaml

First add failing tests for:

~~~text
single_target_runflow_may_omit_target
multi_target_root_route_requires_target
multi_target_nested_runflow_requires_explicit_target
unknown_target_is_rejected
target_must_name_declared_component
child_header_cannot_rebind_parent_driver
recursive_flow_cycle_is_rejected
recursive_flow_depth_is_bounded
unsupported_command_for_target_is_rejected
~~~

Implement validation in this order:

1. Parse target strings only as app, gateway, or cloud.
2. Compute the root component set.
3. Require an explicit target for every runFlow in a multi-target root.
4. Permit omitted target only for a single-target root.
5. Reject root-level commands without a target scope in a multi-target flow.
6. Resolve child paths, detect cycles, and enforce the configured depth limit.
7. Validate child headers as metadata only and reject a conflicting attempted
   driver rebind.
8. Validate command capability against the effective target.

Extend the JSON schema with app, gateway, cloud, target, profile, and deprecated
journey metadata. Keep the existing legacy header schema valid.

Run:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_yaml
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/parent.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/invalid-missing-target.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/invalid-unknown-target.yaml --json
~~~

Expected results: canonical parent validates; missing-target and unknown-target
fixtures return structured validation errors; legacy App fixture remains valid.

## 5. Developer RED/GREEN — adapter traits, registry, and lifecycle

**Owner:** developer

**Files:**

- Create: lumi-tester/src/adapter/mod.rs
- Create: lumi-tester/src/adapter/traits.rs
- Create: lumi-tester/src/adapter/registry.rs
- Create: lumi-tester/src/adapter/session.rs
- Create: lumi-tester/src/adapter/lifecycle.rs
- Create: lumi-tester/src/adapter/cloud_mock.rs
- Modify: lumi-tester/src/lib.rs
- Modify: lumi-tester/src/e2e/mod.rs
- Modify: lumi-tester/src/e2e/profile.rs
- Create/modify: lumi-tester/tests/multi_target_runner.rs

Write failing unit/integration tests before implementation:

~~~text
registry_creates_existing_app_adapter
registry_creates_hc_gateway_adapter
registry_creates_cloud_mock_adapter_without_network
unsupported_target_platform_is_rejected
one_session_reuses_adapter_across_nested_flows
adapters_close_once_in_reverse_open_order
blocked_preflight_is_not_failed_or_passed
~~~

Implement the boundary with these behaviors:

~~~rust
pub trait TargetAdapter {
    fn target(&self) -> TargetId;
    fn platform(&self) -> &str;
    fn capabilities(&self) -> &[String];
    fn preflight(&mut self, context: &TestContext) -> AdapterResult;
    fn open(&mut self, context: &TestContext) -> AdapterResult;
    fn execute(&mut self, command: &TestCommand, scope: &TargetScope) -> AdapterResult;
    fn close(&mut self, context: &TestContext) -> AdapterResult;
}
~~~

The App implementation wraps the current PlatformDriver. The gateway
implementation wraps current HC HTTP/WebSocket/evidence code. The Cloud adapter
is deterministic and rejects real network access. Keep E2eResult status and
evidence tier intact instead of collapsing it into an ordinary executor error.

Run:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner registry_creates_cloud_mock_adapter_without_network -- --exact
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner one_session_reuses_adapter_across_nested_flows -- --exact
cargo test --manifest-path lumi-tester/Cargo.toml --test e2e_hc_contract
~~~

Expected result: registry/lifecycle tests and all existing HC contract tests pass;
no test opens a real Cloud connection.

## 6. Developer RED/GREEN — runner target dispatch and recursive session reuse

**Owner:** developer

**Files:**

- Modify: lumi-tester/src/runner/context.rs
- Modify: lumi-tester/src/runner/mod.rs
- Modify: lumi-tester/src/runner/executor.rs
- Modify: lumi-tester/src/parser/types.rs
- Modify: lumi-tester/tests/multi_target_runner.rs
- Modify: lumi-tester/tests/fixtures/multi_target/parent.yaml
- Modify: lumi-tester/tests/fixtures/multi_target/child-header-conflict.yaml

Write failing tests first:

~~~text
single_target_flow_uses_implicit_target
nested_runflow_dispatches_gateway_target
nested_runflow_dispatches_cloud_target
app_gateway_cloud_app_sequence_uses_one_session
child_platform_header_does_not_switch_app_driver
nested_runflow_with_missing_target_fails_before_execution
~~~

Implement TargetScope in the shared context and make runFlow do the following:

1. Evaluate when and optional using current behavior.
2. Resolve the target from validated RunFlowParams.target or the sole component.
3. Reuse the parent TargetSession.
4. Push the target scope while executing child commands.
5. Parse child commands without applying child header driver/profile selection.
6. Restore the parent scope after child completion, including failure paths.
7. Keep existing command indexes and variable merging behavior compatible.

Move HC preparation from flow-platform selection to target adapter lifecycle. A
child with platform: android cannot replace an already-open App adapter, and a
child with platform: lumi_hc cannot replace the Cloud mock.

Run:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner
cargo test --manifest-path lumi-tester/Cargo.toml --lib runner
~~~

Expected result: target routing tests pass, existing App runner tests pass, and a
single run manifest shows one session with multiple target operations.

## 7. Developer RED/GREEN — state, report, event, and evidence compatibility

**Owner:** developer

**Files:**

- Modify: lumi-tester/src/runner/state.rs
- Modify: lumi-tester/src/runner/events.rs
- Modify: lumi-tester/src/report/types.rs
- Modify: lumi-tester/src/report/json.rs
- Modify: lumi-tester/src/report/html.rs
- Modify: lumi-tester/src/report/junit.rs
- Modify: lumi-tester/src/report/summary_html.rs
- Modify: lumi-tester/tests/multi_target_runner.rs

Write failing tests first:

~~~text
target_lifecycle_events_are_emitted_in_jsonl
command_events_include_effective_target
blocked_target_is_preserved_in_run_and_test_results
error_target_is_distinct_from_product_failure
legacy_app_report_shape_remains_readable
mock_evidence_contains_adapter_and_evidence_tier
~~~

Implement additive fields/variants. Existing App event consumers must continue to
deserialize old events. Add target lifecycle events and optional target,
adapter, profile, result status, evidence tier, and correlation fields. Update
summary counts so Blocked is visible and is not counted as a product pass or
product failure.

Run:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner target_lifecycle_events_are_emitted_in_jsonl -- --exact
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner legacy_app_report_shape_remains_readable -- --exact
cargo test --manifest-path lumi-tester/Cargo.toml --lib report
~~~

Expected result: fresh mock artifacts contain target/component/evidence metadata;
legacy App report fixtures still deserialize and render.

## 8. Developer RED/GREEN — deterministic App -> HC -> readback -> App harness

**Owner:** developer

**Files:**

- Modify: lumi-tester/src/adapter/cloud_mock.rs
- Modify: lumi-tester/src/e2e/http.rs only where injection is required
- Modify: lumi-tester/src/e2e/websocket.rs only where injection is required
- Create: lumi-tester/tests/fixtures/multi_target/parent.yaml
- Create: lumi-tester/tests/fixtures/multi_target/app.yaml
- Create: lumi-tester/tests/fixtures/multi_target/gateway.yaml
- Create: lumi-tester/tests/fixtures/multi_target/cloud.yaml
- Modify: lumi-tester/tests/multi_target_runner.rs

Write a failing fixture test named
mock_app_gateway_readback_returns_to_app_in_one_session.

The fixture must execute this exact logical sequence:

    target app: launch/read initial App state
    target gateway: control device idx/value
    target gateway: read state and assert requested idx/value
    target app: read/assert App state after gateway operation

The harness must record dispatch order, session ID, target scopes, HC raw and
normalized evidence, and mock evidence tier. It must not require a device,
network, HC process, Cloud service, or environment secret.

Run:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner mock_app_gateway_readback_returns_to_app_in_one_session -- --exact
cargo test --manifest-path lumi-tester/Cargo.toml --test e2e_hc_contract
~~~

Expected result: deterministic mock test passes, readback is required, and the
artifact identifies all target transitions as mock evidence.

## 9. Developer — update CLI, schema, SRS, and migration documentation

**Owner:** developer

**Files:**

- Modify: lumi-tester/src/main.rs
- Modify: lumi-tester/schema/lumi-test.schema.json
- Modify: 2026-10-05-lumi-e2e-hc-srs.md
- Read/check: docs/architecture.md, docs/cli-reference.md, existing README

Update CLI validation/list/schema output to expose normalized components, target
diagnostics, deprecated journey warnings, and Blocked/Error statuses. Ensure a
multi-target root does not require a single --platform value. Preserve the legacy
App platform option.

Update the SRS sections for component model, recursive YAML composition, target
dispatch, child-header isolation, migration, shared session, HC boundary, Cloud
mock, evidence, report, and acceptance gates. Do not claim real HC or Cloud PASS
in documentation without fresh runtime evidence.

Run:

~~~powershell
cargo run --manifest-path lumi-tester/Cargo.toml -- schema --json
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/app.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/parent.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- list lumi-tester/tests/fixtures/multi_target/parent.yaml --json
~~~

Expected result: schema contains the new component/target contract; old and new
App documents validate; the parent lists target-aware nested commands.

## 10. Developer verification gate

**Owner:** developer

Run formatting, unit, integration, parser, existing App, and HC tests before
handing off:

~~~powershell
cargo fmt --manifest-path lumi-tester/Cargo.toml -- --check
cargo test --manifest-path lumi-tester/Cargo.toml --lib
cargo test --manifest-path lumi-tester/Cargo.toml --tests
cargo test --manifest-path lumi-tester/Cargo.toml --test e2e_hc_contract
cargo test --manifest-path lumi-tester/Cargo.toml --test e2e_parser_profile
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_yaml
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner
~~~

Store fresh command output and generated JSONL/JSON/report artifacts under an
ordinary evidence location such as
lumi-tester/output/multi-target-gates/developer/; do not create any
target-codex or target-review directory. If a test fails, the developer returns
to the relevant RED/GREEN task and adds a reproducing test before fixing.

## 11. Reviewer gate — architecture and compatibility review

**Owner:** reviewer

The reviewer does not change code during the review pass. Inspect:

- only the approved exact file map was changed;
- App driver construction and existing App flows remain intact;
- journey is never read as an adapter selector;
- runFlow.target is validated and propagated recursively;
- child headers never mutate the parent registry/session;
- HC references are limited to the documented v3 boundary and no source was
  copied from D:\lumi-iot-v3;
- Cloud has no real network/service implementation;
- Blocked is not converted to Failed or Passed;
- old JSON/report/event consumers remain compatible;
- no unapproved generated/build directories exist.

Run:

~~~powershell
git diff --check
git diff --stat
rg -n "journey.*platform|platform.*journey|lumi_e2e|target-codex|target-review" lumi-tester/src lumi-tester/schema
cargo fmt --manifest-path lumi-tester/Cargo.toml -- --check
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_yaml
cargo test --manifest-path lumi-tester/Cargo.toml --test multi_target_runner
~~~

Expected evidence: reviewer report with file map, compatibility findings, target
dispatch findings, HC boundary findings, and explicit confirmation that no Git
history operation was performed. Any review defect returns to the developer task
that owns the behavior; it is not silently waived.

## 12. Tester gate — fresh mock and compatibility output

**Owner:** tester

Run the complete fresh test set from a clean process state without modifying
fixtures during execution:

~~~powershell
cargo test --manifest-path lumi-tester/Cargo.toml --lib
cargo test --manifest-path lumi-tester/Cargo.toml --tests
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/app.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/parent.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- list lumi-tester/tests/fixtures/multi_target/parent.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- run lumi-tester/tests/fixtures/multi_target/parent.yaml --report --events-jsonl --output lumi-tester/output/multi-target-gates/tester
~~~

Inspect these fresh artifacts:

    lumi-tester/output/multi-target-gates/tester/run.json
    lumi-tester/output/multi-target-gates/tester/test-results.json
    lumi-tester/output/multi-target-gates/tester/events.jsonl
    lumi-tester/output/multi-target-gates/tester/*.html
    lumi-tester/output/multi-target-gates/tester/*.xml

The tester must verify old App YAML, new App YAML, HC-only, nested target
routing, single-adapter, multi-target mock, missing/wrong target, child-header
isolation, backward compatibility, and the App -> gateway -> readback -> App
harness. A PASS claim is permitted only when these outputs are fresh and match
the expected statuses.

## 13. Tester gate — real-runtime and blocked policy

**Owner:** tester

Check environment availability without inventing it:

~~~powershell
cargo run --manifest-path lumi-tester/Cargo.toml -- doctor --json
cargo run --manifest-path lumi-tester/Cargo.toml -- validate lumi-tester/tests/fixtures/multi_target/parent.yaml --json
~~~

If the HC profile endpoint, WebSocket endpoint, required credentials, or device
lab is unavailable, record a structured BLOCKED result in the tester report and
retain the evidence showing the missing preflight requirement. Do not change the
result to PASS because mock tests passed. Cloud real-runtime testing is not
permitted in this scope; only Cloud mock contract/conformance output is valid.

If a real HC environment is supplied, run only the approved fixture with bounded
timeouts and full artifacts:

~~~powershell
cargo run --manifest-path lumi-tester/Cargo.toml -- run lumi-tester/tests/fixtures/multi_target/parent.yaml --report --snapshot --events-jsonl --output lumi-tester/output/multi-target-gates/real-hc
~~~

Inspect run.json, test-results.json, events.jsonl, raw/normalized HC evidence,
and the target lifecycle sequence. A real PASS requires fresh real evidence, not
a prior developer message or a mock result.

## 14. Requirement coverage matrix

| User requirement | Plan coverage |
|---|---|
| 1. Optional/deprecated journey | Tasks 3, 4, 9; design sections 1, 2.4 |
| 2. Legacy App YAML | Tasks 2, 3, 9, 12 |
| 3. Common normalized adapter model | Tasks 2, 3, 5 |
| 4. Recursive runFlow.target | Tasks 4, 6, 12 |
| 5. Single/multi-target routing | Tasks 4, 6, 12 |
| 6. Child header isolation | Tasks 4, 6, 11, 12 |
| 7. Shared App/HC/Cloud session | Tasks 5, 6, 8 |
| 8. Preserve App/HC/report/events/flows | Tasks 3, 5, 7, 10, 11, 12 |
| 9. HC reference only from v3 | Tasks 1, 5, 11 |
| 10. Cloud extension/mock only | Tasks 5, 8, 13 |
| 11. SRS update | Task 9 |
| 12. Parser/schema/validation/registry/lifecycle/runner/report/events | Tasks 3-9 |
| 13. Required tests | Tasks 2, 4, 5, 6, 7, 8, 12 |
| 14. Mock fixture harness | Task 8 |
| 15. Blocked policy and fresh PASS evidence | Tasks 7, 10, 12, 13 |

## 15. Self-review checklist

Before implementation begins, the planner verifies:

- [x] Exact requested design and plan paths are used.
- [x] The plan is TDD-first: each behavior starts with a named RED test and a
  specific command before production implementation.
- [x] Legacy App, canonical App, HC-only, nested flow, target routing,
  single-adapter, multi-target mock, invalid target, child isolation, and
  backward compatibility are explicit tests.
- [x] Exact source, schema, SRS, fixture, report, event, and evidence paths are
  listed.
- [x] HC facts are limited to D:\lumi-iot-v3 and the existing local HC boundary.
- [x] Cloud real integration is excluded.
- [x] Missing real environment produces Blocked, never a fake Passed.
- [x] No source, tests, schema, SRS, or Git history are changed by this planning
  stage.
- [x] No target-codex or target-review directory is introduced.
- [x] No commit step appears because the user explicitly forbids commits.

The next authorized action is the developer RED stage. It must not begin until
the controller starts the implementation stage.

