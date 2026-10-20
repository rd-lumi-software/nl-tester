# Test Plan Contract

The orchestrator-to-agent handoff is a planning contract. It is intentionally
separate from Lumi Tester's executable `header --- commands` YAML.

The handoff must not be created for a mutating run while required intake values
are unknown. Use an intake record first and mark unresolved fields explicitly;
do not rely on Lumi CLI defaults for safety-sensitive policy.

Connection fields are independent. A supplied host/IP must be retained even
when scheme, port, or profile is missing. Use `status: partial` and ask only
for the missing fields. A profile-derived port is a proposal until the user or
trusted configuration confirms it.

## Required fields

```yaml
schemaVersion: "1"
planId: "<unique plan id>"
request: "<user's test intent>"
scope: hc_only
target:
  platform: lumi_hc
  profile: profiles/lumi-iot-v3.yaml
  connection:
    scheme: http
    host: "${LUMI_HC_HOST}"
    port: 8080
    baseUrl: "${LUMI_HC_URL}"
    status: complete
  mode: real
inventory:
  source: live_hc_discovery
  snapshotId: "<discovery run id or hash>"
  capturedAt: "<ISO-8601 timestamp>"
  complete: false
  expectedCount: null
  allowlist: []
  exclusions: []
authorization:
  real_device_control: false
  restore_state: true
execution:
  repeat: 1
  max_repeat: 5
  mode: sequential
  max_parallel: 1
  delay_ms: 0
  timeout_ms: 10000
  flow_layout: generated_from_plan
  template_policy: create_if_missing
  restore_after_each_cycle: true
failurePolicy:
  scope: target
  onTargetFailure: record_and_continue_independent_targets
  aggregateAtEnd: true
  stopOn: [health_failure, discovery_failure, inventory_drift, unresolved_system_safety]
targets: []
acceptance: []
evidence:
  report: true
  events: true
  label: real_preflight
  redaction: [dev_key, nwk_key, app_key, authorization, cookie, token]
```

## Target contract

Each target should contain:

```yaml
- entity: "<stable entity ID as a string>"
  name: "<human-readable name>"
  capability: switch
  status: eligible
  classification:
    status: observed
    source: live_entity_and_protocol_profile
  mapping:
    input_idx: 0
    state_idx: 0
    source: protocol_profile
  actions:
    - capture_initial_state
    - set: { input_idx: 0, value: true }
    - readback: { state_idx: 0, expected: true }
    - restore_initial_state
```

Allowed planning statuses are `eligible`, `blocked`, and `skipped`. A target
with `blocked` or `skipped` status must include `reason` and must not be passed
to a mutating execution flow. The selection must contain exact entity IDs or a
deterministic allowlist; “first discovered device” is not a valid selection.

## Repeat and concurrency semantics

`execution.repeat` means the number of complete action/readback cycles per
eligible target unless the plan explicitly says `scope: suite`. `mode` is one
of `sequential` or `parallel`. A parallel plan must include independent batch
membership and a maximum batch size. Do not use parallel mode for two channels
of the same device unless the target backend documents that it is safe. The
requested repeat must not exceed `max_repeat` without explicit approval.

## Template and generated-flow semantics

When repeated cycles use different control values, the plan is the source of
truth. Use:

```yaml
execution:
  flow_layout: generated_from_plan
  template_policy: create_if_missing
```

The orchestrator defines the cycle data, capability mappings, and acceptance
rules. `lumi-tester-agent` owns creating or reusing the capability template or
generator and materializing executable `header --- commands` YAML with literal
typed values. Generated flows belong under the requested output or an ignored
generated directory and must not become duplicated maintained source files.

Use one template per capability when switch, RGB, CCT, or dimmer command shape
differs. Preserve one evidence directory per generated cycle when the run
requires per-cycle readback or target quarantine. `one_cycle_per_target_flow`
may describe this runtime artifact layout, but it is not a requirement to
hand-author one YAML file per cycle.

If the target backend has not verified typed variable substitution inside its
control command, do not put `${cycle.value}` placeholders directly in indexed
HC state updates. Materialize boolean, numeric, and string values before
validation and execution.

## Acceptance contract

Acceptance criteria must distinguish:

- request dispatch: the command was sent and the backend returned a usable
  response;
- device readback: the state endpoint reports the requested state;
- latency: record dispatch-to-response and/or dispatch-to-readback timing when
  the backend emits timestamps;
- restoration: the final state matches the captured initial state when enabled.

A dispatch success without correct readback is not a passing control test.

## Safety contract

For real-device control, include an approval record, endpoint/environment
allowlist, entity/index/value allowlists, repeat and concurrency limits, timeout,
control retry policy, initial-state capture, and restore-and-verify policy. If
one of these is unresolved, mark the target `blocked` rather than guessing.

For independent real-device targets, declare failure scope explicitly. The
default is target-scoped continuation: a target that executed and failed is
`failed`, is quarantined from further mutation, and does not block independent
targets. A shared health, discovery, inventory, endpoint/session, or safety
failure is run-level and makes untouched targets `not_run`.

## Result contract

The aggregate result should retain one record per target and scenario:

```yaml
- entity: "<entity>"
  capability: switch
  scenario: control_readback_restore
  repetitions:
    requested: 3
    completed: 3
    passed: 2
    failed: 1
  outcome: failed
  evidenceTier: real_e2e
  dispatch_latency_ms: [120, 98, 101]
  readback_latency_ms: [340, 290, 10000]
  expected_state: true
  observed_state: unknown
  final_state: false
  evidence:
    output_dir: "output/orchestration/<run-id>/<entity>/"
  reason: "readback timeout on repetition 3"
```

Allowed outcomes are `passed`, `failed`, `blocked`, `error`, and `not_run`.
The delegation response must retain the phase, generated flow paths, exact
validate/list/run results, output directory, runtime status, evidence tier,
and unresolved assumptions.

Do not place access tokens, passwords, or secret headers in this contract.

## Machine validation

Use the Rust semantic validator before delegating a plan:

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --for-run --json
```

The first command checks structure and returns `valid`, `readyForRun`,
`errors`, and `warnings`. `--for-run` additionally requires safety-sensitive
data such as a complete connection, real-control authorization, immutable
inventory evidence, explicit mapping sources, shared stop conditions, and a
restoration or final-state-capture contract. A plan with only an inventory ID
but no snapshot payload is not drift-verifiable.

## Machine validation and lifecycle

Use `lumi-tester plan validate` for structural validation and `--for-run` for
the mutation safety gate. The validator requires string, unique entity IDs,
reasons for `blocked`/`skipped` targets, bounded repeats, explicit parallel
batches, a complete real-HC endpoint or connection, a live inventory snapshot
payload or artifact, mapping source, and an explicit captured-state restoration
contract when restoration is enabled.

Each selected target has an explicit lifecycle:
`discovered -> eligible|blocked|skipped -> queued -> running -> passed|failed|error`.
Shared health, discovery, inventory, or safety stops move untouched targets to
`not_run`; post-execution failures/errors are quarantined without blocking
independent targets.

`aggregate.json` retains outcomes, repetitions, state observations, final state,
restore status, captured latency, and artifact paths. Missing measurements stay
null or not-captured; they are never inferred.
