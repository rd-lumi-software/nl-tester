# Lumi YAML Multi-Target Design

**Status:** design approved for implementation planning

**Scope:** YAML parsing, normalization, target dispatch, adapter/session lifecycle,
reports, events, evidence, validation, migration, and deterministic mock
execution for App, Lumi HC gateway, and Cloud.

**Implementation constraint:** this document defines the target architecture. It
does not authorize changing the source tree, tests, schema, SRS, or Git history
by itself. The implementation plan is the authority for the later developer,
reviewer, and tester stages.

## 1. Decisions

The framework will use a canonical component model and an adapter registry. App,
gateway, and cloud are peer components. The existing App PlatformDriver remains
the App execution implementation. The existing HC implementation under
D:\lumi-iot-v3 is treated only as the reference for the lumi_hc gateway
boundary. Cloud is a mock extension boundary; no real Cloud service is integrated
in this scope.

The parent flow owns the normalized component registry and execution session.
Recursive child flows contribute commands to that session; a child YAML header
cannot replace the parent session or select a different driver.

journey is accepted as deprecated metadata for migration and diagnostics. It is
never an adapter selector, target selector, lifecycle selector, or capability
selector.

## 2. YAML contract

### 2.1 Legacy App YAML

The existing form remains valid:

~~~yaml
platform: android
appId: com.lumi.lumilife
tags:
  - smoke
---
- launchApp
- see: "Home"
~~~

It normalizes to exactly one app component. Existing App command syntax,
selectors, variables, setup/teardown, reports, and event JSONL retain their
current behavior.

### 2.2 New component YAML

The canonical form is:

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
    path: flows/app-open.yaml
    target: app
- runFlow:
    path: flows/gateway-control.yaml
    target: gateway
- runFlow:
    path: flows/cloud-check.yaml
    target: cloud
~~~

The recognized peer component keys are app, gateway, and cloud. Unknown component
keys are validation errors. A component has:

    target       app | gateway | cloud
    platform     adapter platform identifier
    appId        App-only application identifier
    profile      gateway/cloud adapter profile path

app.platform uses the existing App platform values such as android, ios, web,
macos, and windows. gateway.platform: lumi_hc selects the HC adapter.
cloud.platform: lumi_cloud selects the Cloud mock adapter in this scope.

### 2.3 Mixing legacy and canonical fields

Top-level platform and appId remain a compatibility shorthand for app. When a
document contains both an app component and legacy App fields, the legacy values
are accepted only when they agree with the normalized app component. A
disagreement is ConflictingLegacyAppDefinition, not a silent override.

Legacy top-level profile is accepted only for migration of the existing
HC-oriented YAML. It maps to gateway.profile when no gateway component exists.
The migration produces a deprecation diagnostic. A canonical document must use
gateway.profile or cloud.profile.

The public HC platform is `lumi_hc`. An old platform alias is rejected instead
of being normalized, so a flow cannot silently select a different target. It is
not a new component type and must not remain the internal adapter-selection model.

### 2.4 journey

journey may appear in old or child YAML for compatibility. The parser stores it
as legacy_metadata.journey and emits a deprecation warning. It does not select
App, gateway, Cloud, an adapter, a profile, a driver, or a session lifecycle.

Missing journey is valid for every component combination. A child with
journey: system_journey and a parent with journey: user_journey still uses the
parent session and explicit target scopes.

## 3. Canonical data model

The parser produces a normalized flow before validation or execution:

~~~rust
enum TargetId {
    App,
    Gateway,
    Cloud,
}

struct ComponentSpec {
    target: TargetId,
    platform: String,
    app_id: Option<String>,
    profile: Option<PathBuf>,
    capabilities: Vec<String>,
    source: ComponentSource,
}

struct LegacyMetadata {
    journey: Option<String>,
    legacy_platform: Option<String>,
    legacy_profile: Option<PathBuf>,
    diagnostics: Vec<Diagnostic>,
}

struct NormalizedFlow {
    components: BTreeMap<TargetId, ComponentSpec>,
    commands: Vec<TestCommand>,
    legacy_metadata: LegacyMetadata,
    source_path: PathBuf,
}

struct RunFlowParams {
    path: PathBuf,
    target: Option<TargetId>,
    vars: Map<String, Value>,
    env: Map<String, Value>,
    commands: Option<Vec<TestCommand>>,
    when: Option<Condition>,
    label: Option<String>,
    optional: bool,
}
~~~

The exact Rust names may follow repository conventions, but the following
invariants are fixed:

1. TargetId is the only logical route identity used by dispatch.
2. platform identifies an adapter implementation within a component; it does
   not identify a flow or journey.
3. A normalized legacy App flow has one app component.
4. A normalized canonical flow has one entry per declared peer component.
5. Profiles resolve relative to the YAML file that declares the component.
6. Child component declarations cannot replace the parent component registry.
7. The normalized model is the input to validation, list output, execution,
   report generation, and event metadata.

## 4. Adapter registry and shared session

### 4.1 Registry

The registry maps a normalized component to an adapter factory:

    TargetId::App     + android/ios/web/... -> existing App PlatformDriver adapter
    TargetId::Gateway + lumi_hc            -> HC gateway adapter
    TargetId::Cloud   + lumi_cloud         -> deterministic Cloud mock adapter

The registry rejects an unsupported (target, platform) pair before command
execution. It does not infer a target from journey, child platform, YAML file
name, or command order.

The adapter boundary has these responsibilities:

    adapter_id()
    capabilities()
    preflight(context)
    open(session_context)
    execute(command, target_scope)
    observe(query)
    close(session_context)

The App adapter wraps the existing PlatformDriver lifecycle rather than
replacing it. HC-specific HTTP/WS behavior remains behind the gateway adapter.
Cloud exposes the same boundary through a deterministic in-memory mock.

### 4.2 Session

One parent invocation owns one TargetSession:

    TargetSession
      shared TestContext and variables
      shared run_id/correlation IDs
      shared evidence collector
      active target scope
      App PlatformDriver, when app is declared
      gateway adapter, when gateway is declared
      Cloud mock adapter, when cloud is declared

Adapters remain available while recursive child flows run. A child flow does not
close, replace, or reinitialize another component's adapter. All adapters close
once during parent finalization, in reverse successful-open order.

Lifecycle states are:

    Created -> Preflight -> Ready -> Closing -> Closed
                        \-> Blocked
                        \-> Failed

Blocked means the environment or required capability is unavailable. It is not
converted into Failed merely because the executor cannot continue.

Adapters may be opened lazily at the first command for their target, but the
target must pass preflight before its first command. The registry and session are
created once from the root normalized flow.

## 5. Recursive runFlow and target dispatch

### 5.1 Scope rules

runFlow.target is the explicit dispatch field:

~~~yaml
- runFlow:
    path: gateway-control.yaml
    target: gateway
~~~

The target scope applies to commands in the child flow and is recorded in nested
events and reports.

Rules:

1. A flow with exactly one component may omit target on runFlow; the sole
   component is the effective target.
2. A flow with multiple components must put every target-specific child route in
   an explicit target scope. A root-level adapter command without a target scope
   is MissingTarget.
3. In a multi-target flow, every runFlow call must declare target, including a
   nested call that remains on the same target. This makes every recursive route
   auditable and avoids implicit cross-target behavior.
4. Commands inside a target-scoped child inherit that scope. They do not need a
   second target field.
5. runFlow.target must name a component declared by the parent root session.
6. A child runFlow may target another declared component, allowing App ->
   gateway -> Cloud -> App orchestration without ending the session.
7. target is validated before child execution. Unknown, missing, or incompatible
   targets never reach an adapter.
8. Existing single-target App flows preserve their current command indexes and
   behavior.

### 5.2 Child-header isolation

Child YAML headers are parsed for syntax, diagnostics, and migration metadata,
but they do not select a driver. The parent session is authoritative.

Examples:

- A child header with platform: ios invoked under target: app cannot switch an
  Android parent App driver.
- A child header with platform: lumi_hc invoked under target: cloud cannot
  switch the Cloud mock into HC.
- A child legacy journey cannot alter target, adapter, lifecycle, or report
  classification.

If a child declares a component definition that conflicts with the parent target,
validation reports ChildHeaderDriverConflict. The child is not allowed to
silently rebind the session.

### 5.3 Recursive safety

The parser/validator builds a flow graph using resolved paths. It rejects cycles
and enforces a finite maximum recursion depth before execution. Inline commands
and path-based runFlow use the same target-scope rules and event model.

## 6. HC gateway boundary

Only D:\lumi-iot-v3 is the implementation reference for HC facts. The framework
does not copy HC source code or import it as a dependency.

The gateway adapter must preserve these v3 wire facts:

    GET  /api/health_check
    GET  /api/devices/
    GET  /api/devices/{device_id}/state
    POST /api/devices/{device_id}/control
    WS   /ws?clientId=<id>

The v3 contract uses decimal-string-safe device IDs, state/control values indexed
by idx, direct JSON success payloads, and {code,msg} error payloads. Control
acknowledgement is not proof of device state; the adapter must perform bounded
state readback and correlate the requested idx/value.

The v3 WebSocket has subscribe/data/RPC envelopes, supports device and cell
channels, has no replay guarantee, and requires HTTP resynchronization after
reconnect. These details stay inside the gateway adapter and evidence normalizer.

Existing HC canonical, HTTP, WebSocket, profile, result, redaction, and
evidence code is owned by `src/hc/backends/v3/runtime` and refactored behind
the generic adapter interface where needed.

## 7. Cloud mock boundary

lumi_cloud is a contract-only adapter in this release:

- no real Cloud SDK, endpoint, credential, or service call;
- deterministic in-memory command/state/readback behavior;
- explicit mock evidence tier;
- capability declarations and validation identical in shape to other adapters;
- an attempted real Cloud operation is Blocked/UnsupportedAdapter, never Passed.

The mock must be injectable through the registry and usable by integration tests
without network access. Its evidence must identify adapter: cloud_mock and
evidence_tier: mock_contract.

## 8. Validation and error model

Validation is split into syntax, normalization, static flow graph, component, and
runtime preflight checks.

Required diagnostics include:

    MissingComponent
    UnknownComponent
    UnsupportedPlatform
    MissingAppId
    MissingProfile
    ConflictingLegacyAppDefinition
    MissingTarget
    UnknownTarget
    TargetRequired
    TargetPlatformMismatch
    UnsupportedCommandForTarget
    ChildHeaderDriverConflict
    RecursiveFlowCycle
    RecursiveFlowDepthExceeded
    InvalidProfile
    AdapterUnavailable

journey produces a deprecation warning only. It never changes validation from
App to HC or Cloud.

Runtime result categories are distinct:

    Passed  product action/assertion/evidence complete
    Failed  product behavior or assertion failed
    Blocked environment/capability/real service unavailable
    Error   runner/adapter failure prevents a reliable conclusion

The CLI JSON validation/list output exposes normalized component and target data,
diagnostic code, severity, source path, and child-flow location.

## 9. Reports, events, and evidence

Existing report fields and event variants remain readable. New data is additive.

Every target-aware command/result may include:

    target
    adapter
    platform
    profile
    component capabilities
    effective target scope
    result status
    evidence tier
    correlation ID

Target lifecycle events are added for started, preflight, ready, blocked,
failed, and finished. Existing command and flow events gain optional target
metadata rather than changing the meaning of old App events.

run.json, test-results.json, HTML, JUnit, and events.jsonl must preserve the
distinction between Passed, Failed, Blocked, and Error. A blocked target must be
visible in summary counts and must not be reported as an App failure.

Evidence is bounded and redacted. Each evidence record identifies its source:

    App UI/driver
    HC HTTP
    HC WebSocket
    Cloud mock
    normalized readback

Evidence tiers are explicit:

    mock_contract
    mock_conformance
    real_preflight
    real_e2e

Mock evidence can prove adapter contract behavior but cannot be labeled real
device or real Cloud evidence. Missing real environment produces Blocked.

## 10. Deterministic test harness

The harness models this sequence without physical equipment:

    Mock App
      -> invoke gateway target
      -> Mock HC control
      -> Mock HC device readback
      -> return to App target

It uses an in-memory App driver, existing/injected HC mock transports, a fixture
state machine keyed by decimal device ID and idx, and the Cloud mock adapter.
The fixture records dispatch order, target scope, shared session identity, raw
messages, normalized state, and evidence tier.

The harness must prove that:

1. App, gateway, and Cloud share one parent session.
2. Nested flow targets route to the requested adapter.
3. Child headers do not switch the driver.
4. HC control is followed by readback.
5. Mock evidence is never classified as real PASS evidence.
6. Missing target and invalid target fail before adapter execution.

## 11. Migration

Migration is additive:

1. Existing App header YAML is normalized to app with no file rewrite.
2. Existing journey continues to parse with a deprecation warning.
3. HC flows use `platform: lumi_hc`; unsupported old platform aliases are
   rejected during validation.
4. New multi-target flows use explicit app, gateway, and cloud components.
5. Existing App driver, command names, command indexes, output artifacts, and
   event JSONL remain compatible.
6. The SRS and schema are updated in the implementation stage to describe this
   model and migration.
7. No automatic rewrite or destructive migration is performed.

## 12. Constraints and non-goals

- No real Cloud service integration in this scope.
- No HC implementation source copied from D:\lumi-iot-v3.
- No requirement for physical devices in parser, mock, or contract tests.
- No adapter selection from journey.
- No driver selection from a child YAML header.
- No replacement of the existing App driver architecture.
- No fake PASS when a real environment is unavailable.
- No commit, push, PR, reset, checkout, or Git-history rewrite.
- No target-codex, target-review, or equivalent auxiliary build directory.

## 13. Acceptance criteria

The design is considered implemented only when the later tester gate has fresh
output proving all of the following:

1. Old App YAML validates, lists, and runs through the existing App path.
2. New App component YAML normalizes to the same App adapter model.
3. HC-only flow routes to the gateway adapter without requiring journey.
4. Nested runFlow dispatches recursively with explicit target semantics.
5. Single-target flows may omit target.
6. Multi-target flows reject missing or unknown target routes.
7. Child headers cannot switch the parent driver.
8. App, gateway, and Cloud mock share one session.
9. HC behavior follows the v3 boundary above.
10. Cloud remains mock-only.
11. Validation, adapter lifecycle, runner, report, and event output include target
    and status information.
12. Evidence is bounded, redacted, source-labelled, and tiered.
13. The App -> HC -> readback -> App harness passes with mock evidence.
14. Existing App flows/tests remain compatible.
15. Real runtime absence is reported as Blocked, never Passed.

