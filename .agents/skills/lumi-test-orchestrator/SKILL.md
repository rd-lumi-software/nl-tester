---
name: lumi-test-orchestrator
description: Use when a user asks to plan, expand, select, or coordinate inventory-driven Lumi tests across real HC devices, repeated or concurrent actions, device discovery, restoration, or future App-to-Cloud-to-HC evidence.
---

# Lumi Test Orchestrator

Use this skill as the planning and coordination layer above `lumi-tester-agent`.
Turn a natural-language test request into an explicit, reviewable test plan,
then delegate Lumi YAML authoring and execution to `lumi-tester-agent`.

## Mandatory intake gate

Before discovery, YAML authoring, or any device control, collect and show a
complete intake record. Do not silently fill missing execution policy with
`repeat: 1`, `mode: sequential`, `delay: 0`, or an assumed restoration policy.

For every request that includes control testing, ask for these fields unless
the user has already supplied them explicitly:

```text
Scope: HC-only or App → Cloud → HC
HC connection: scheme, host/IP, port, and profile path
Real-device control: yes/no
Target selection: all devices, exact entity IDs, or allowlist
Capabilities: all discovered supported capabilities, or explicit switch/RGB/CCT/dimmer list
Repeat: number of complete cycles and whether it is per target or per suite
Execution mode: sequential or parallel; if parallel, maximum batch size
Delay/rate limit: delay between actions/cycles
Timeout: control/readback timeout
Restore: restore initial state yes/no; if yes, restore after each cycle or at the end
Evidence: report/events/snapshots and required latency measurements
```

When the user gives only a partial request, respond with an intake table that
separates `provided`, `missing`, and `proposed-but-not-approved` values. Parse
an IPv4 address or hostname immediately into `target.host` and preserve it in
the table. Mark the connection `partial` when scheme, port, or profile is still
missing; never display the whole endpoint as blank and never ask for the host
again. Ask the user to fill or confirm only the missing values in one reply.
Never interpret “some devices can be controlled” as a capability list, and
never interpret an IP address alone as a complete endpoint/profile.

Normalize connection input as separate fields:

```yaml
connection:
  scheme: http
  host: 10.10.61.237
  port: 8080
  profile: profiles/lumi-iot-v3.yaml
  status: partial
```

If a profile or trusted project configuration supplies a default port, show it
as `proposed-from-profile` and ask for confirmation; do not silently turn it
into an approved endpoint. A bare IP is a known host, not an absent endpoint.

Do not start live discovery for a mutating test until the endpoint/profile,
target selection, capability scope, real-device permission, repeat, execution
mode, delay, timeout, and restoration policy are explicit. Read-only health or
inventory discovery may run earlier only when it is clearly labelled as
preflight and cannot mutate a device.

If the user explicitly says “use safe defaults”, propose and show the defaults
before execution: one cycle, sequential mode, no extra delay, bounded timeout,
and restore the captured state. Real-device permission and target selection
must still be explicit; they may never default to approval or “all”.

### Partial-request example

For a request such as:

```text
Test độc lập HC tại 10.10.61.237, điều khiển các thiết bị có thể điều khiển
được và tổng hợp kết quả.
```

The first response must not start control. It should show:

```text
Đã nhận: HC-only; host 10.10.61.237; mục tiêu là control + readback.
Thiếu cần xác nhận:
- scheme/port và profile; host đã được nhận, không hỏi lại IP;
- cho phép điều khiển thiết bị thật: yes/no;
- all discovered devices hay allowlist entity;
- capability: all supported hay switch/RGB/CCT/dimmer cụ thể;
- repeat và phạm vi repeat;
- sequential hay parallel, batch size nếu parallel;
- delay/rate limit và timeout;
- restore state: yes/no, sau mỗi cycle hay cuối suite;
- report/events/snapshot và latency cần đo.
Chưa chạy control hoặc sinh YAML cho tới khi các mục trên được xác nhận.
```

## Version 1 scope

Version 1 is HC-focused: discover a real or deterministic HC inventory, plan
capability-specific control/readback cases, and coordinate their execution.
Existing single-app Android/iOS/Web/Desktop flows still go directly through
`lumi-tester-agent`. App → Cloud → HC is documented as an extension boundary;
the current project supports a Cloud mock, not an unqualified real Cloud claim.

## Role boundary

| Responsibility | Owner |
| --- | --- |
| Interpret requested product behavior and test scope | Orchestrator |
| Discover and classify target devices/capabilities | Orchestrator, using delegated Lumi flows |
| Choose scenarios, repetitions, timing, concurrency, acceptance, restoration | Orchestrator |
| Define template policy, cycle data, and generated-flow layout | Orchestrator |
| Author canonical Lumi YAML and choose Lumi commands | `lumi-tester-agent` |
| Create or reuse templates/generators and materialize executable YAML | `lumi-tester-agent` |
| Validate, list, run, debug, and collect raw evidence | `lumi-tester-agent` |
| Call HC/App/Cloud APIs directly | Delegated Lumi flow/backend, never this skill directly |
| Aggregate business-level coverage and outcomes | Orchestrator |

Do not turn this skill into a second Lumi runner. Do not issue raw HC control
requests, invent command names, or write command-level YAML from memory.

## Result semantics: technical checks versus requested scenarios

An action result is a technical fact, not automatically the business verdict.
The delegated runner reports what its commands observed; the orchestrator
decides whether the requested scenarios were executable and whether the
requested coverage was achieved. Keep these result layers separate in the
plan, delegation response, aggregate, and HTML report.

Classify each requested check before delegation:

- **Technical check:** has no target requirement. A successful response and
  its declared assertion may be `passed` even when the returned inventory is
  empty. For example, a connectivity or inventory API check can pass with a
  valid empty collection.
- **Target-dependent scenario:** requires one or more resolved eligible
  targets. It must remain visible as a planned scenario even when discovery
  returns zero eligible targets.

An empty inventory or zero eligible targets is data, not a failure by itself.
Never make a health/discovery action fail solely because the collection is
empty, and never make every plan fail solely for that reason. For a required
target-dependent scenario, however, do not silently drop the scenario or
report the suite as passed. Use this fixed distinction: the scenario outcome
is `blocked`, its execution/lifecycle state is `not_run`, and the reason is
`no_eligible_targets`. If a response schema has only one terminal outcome
field, use `blocked` and retain `not_run` in the scenario matrix or report
metadata. Optional target-dependent scenarios must be marked `skipped` only
when the plan explicitly allows that outcome.

Persist this intent before delegation. In the current v1 plan shape, state the
technical checks and required target-dependent scenarios in `request` and
`acceptance`, then retain the scenario matrix in the delegation/aggregate
artifacts. Do not invent adapter-specific command names or unsupported plan
fields merely to encode this distinction.

The v1 target list is target-centric. When a required scenario has no entity,
do not manufacture a fake entity ID just to create a result row. Retain a
suite-level scenario record in the coverage matrix/report instead. If the
available aggregate command cannot represent that record, report the
aggregate as incomplete/blocked and preserve the companion coverage artifact;
never omit the scenario and claim business success.

The final report must show both layers, for example: technical discovery
`passed`; target-dependent coverage `blocked`/`not_run`; overall business
result not `passed`. A technical-only plan may still have an overall `passed`
result. Do not infer intent from an adapter command name, an HTTP status, or
the number of returned entities; use the persisted semantic plan and its
acceptance criteria.

## Machine-verifiable orchestration gate

Before delegating a plan, validate its semantic contract with the Rust
orchestrator. Use `--for-run` only after the intake, endpoint, authorization,
inventory, mapping, repeat, failure, evidence, and restoration decisions are
explicit:

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --for-run --json
```

After `lumi-tester-agent` returns, validate the delegation response before
aggregating it:

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate-delegation plan.yaml response.json --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan aggregate plan.yaml --response response.json --inventory-current current.json --output ./output/orchestrator --json
```

The orchestrator only validates contracts and combines evidence; it never
calls HC directly or synthesizes a restore payload. Inventory drift or an
unverifiable mapping is a run-level stop, while a target-local failure remains
failed and quarantined. A response with `restore_state: false` must explicitly
report `restoration.status: not_requested`; enabled restoration requires
captured initial state, restore evidence, and verified final state.

The machine-verifiable contract is exposed by the repo-local CLI:

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --for-run --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate-delegation plan.yaml response.json --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan aggregate plan.yaml --response response.json --inventory-current inventory.json --output ./output/orchestrator --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan schema --json
```

The Rust `orchestrator` module validates plans, lifecycle transitions,
inventory drift, delegation responses, and aggregate evidence only. It does
not call HC HTTP/WebSocket endpoints, spawn Lumi, create command indexes, or
synthesize restore payloads.

## Template ownership and repeated-flow policy

The orchestrator owns the semantic plan; `lumi-tester-agent` owns the template
or generator and the executable YAML. For repeated HC actions with changing
values, keep the cycle data in the handoff plan as the source of truth and set
the layout to `generated_from_plan`. Ask the agent to reuse a capability
template or create one when none exists, then materialize typed literal YAML
flows under the run output or another ignored generated directory.

Do not hand-author duplicate `cycle-N/control.yaml` files in the maintained
test source merely to represent data already present in the plan. Preserve
per-cycle evidence and target-scoped quarantine even when the executable flows
are generated. Use `one_cycle_per_target_flow` only as a runtime/evidence
layout when the backend requires it, not as a reason to duplicate source YAML.

## Failure isolation policy

Use target-scoped failure handling by default for independent targets. A target
that already ran and failed control, readback, restore, or acceptance is
`failed`, not `blocked`; quarantine it, preserve its evidence, and continue
with the remaining independent targets when the shared HC safety gates remain
healthy.

Keep these meanings distinct:

| Condition | Outcome | Queue behavior |
| --- | --- | --- |
| Missing permission, unsafe/unknown mapping, or unresolved precondition before mutation | `blocked` | Do not enqueue that target; evaluate independent targets |
| Unsupported/read-only capability excluded by policy | `skipped` | Do not enqueue that target; evaluate independent targets |
| Target ran and an assertion, readback, or restore failed | `failed` | Stop mutation for that target; continue independent targets |
| Transport/runtime ambiguity without enough product evidence | `error` | Preserve evidence; isolate target and continue only if shared safety is intact |
| HC health/discovery failure, inventory drift, or shared safety failure | run-level stop | Stop remaining mutations and report `not_run` targets |

The plan must declare this policy, normally with
`scope: target`, `onTargetFailure: record_and_continue_independent_targets`,
and `aggregateAtEnd: true`. A target-local failure must never be silently
promoted to a run-level block, and a target that executed must never be
relabelled `blocked` merely because its final state is non-passing.

## Required sub-skill

Use `lumi-tester-agent` for every Lumi-specific operation:

- discovery (`hcHealth`, `hcGetEntities`, or the target platform equivalent);
- converting the approved test plan into canonical YAML;
- `validate` before any real-device run;
- `list` before referring to command indexes;
- execution with reports, snapshots/events where supported;
- reading evidence and rerunning the smallest failing command.

Pass the orchestrator's structured plan to that skill. If a capability or
mapping is unknown, stop and mark it `blocked` or `skipped`; do not guess.

## Orchestration loop

Follow these phases in order.

1. **Clarify scope.** Run the mandatory intake gate first. Classify the request
   as HC-only, app-only, Cloud-only, or App → Cloud → HC. Record target
   environment, endpoint/profile, authorization to affect real devices, target
   selection, capability scope, repeat count, delay, concurrency, timeout,
   evidence level, and restoration policy. Stop and ask for missing values
   before continuing to discovery or mutation.
2. **Discover targets.** Ask `lumi-tester-agent` to run the appropriate
   read-only health and inventory flow. For HC, live `hcGetEntities` output is
   the source of truth; a profile list is a hint and may be stale. Preserve
   large entity IDs as strings. If the request says “all”, require a complete
   inventory signal, trusted expected count, or explicit allowlist before
   claiming complete coverage.
3. **Classify capabilities.** Map each discovered entity to a supported
   capability such as `switch`, `rgb`, `cct`, `dimmer`, `read_only`, or
   `unknown`. Record control inputs and readback state fields separately.
   Do not infer an input index from a display name, position, or `cell_idx`.
4. **Design scenarios.** Define the smallest meaningful action/readback loop
   for each capability. Add repeated, sequential, or parallel variants only
   when requested and only when device interference is understood. Capture the
   initial state and define restoration before control actions. For repeated HC
   actions with different values, put the ordered cycles in the plan instead
   of copying nearly identical executable YAML files.
5. **Gate execution.** Before any mutating action, verify exact endpoint,
   entity, mapping, value, repeat/concurrency policy, real-device permission,
   and final-state policy. A missing or conflicting item blocks that target.
   This gate is evaluated per target; it does not invalidate independent
   targets that have their own verified mappings and restoration plans.
6. **Delegate implementation.** Give `lumi-tester-agent` the plan, template
   policy, and requested output directory. Ask it to reuse or create a
   capability template/generator, materialize canonical YAML with literal HC
   values, validate it, list indexes, and run it. That agent owns YAML shape
   and Lumi command syntax.
7. **Aggregate results.** After each target, record its outcome and quarantine
   a failed target without removing independent targets from the queue. Combine
   discovery, planned scenarios, executed, skipped, blocked, passed, failed,
   error, and `not_run` targets/scenarios at the end. Preserve per-action
   evidence paths,
   dispatch/readback timing when available, observed state, expected state,
   final state, and failure reason. Do not collapse `blocked` or `skipped`
   into `passed`, and do not turn a target-local failure into a global block.
8. **Close the run.** Confirm restoration where requested. Report any target
   whose final state could not be verified and stop further mutation for that
   target. Continue independent targets unless a declared global stop condition
   is present; stop the whole run if the inventory or mapping changed globally,
   HC health/discovery failed, or system-wide safety became unresolved.

Read the focused references when needed:

- `references/workflow.md` for the phase contract and failure transitions;
- `references/test-plan-contract.md` for the structured handoff format;
- `references/hc-orchestration.md` for live HC discovery and capability rules;
- `references/safety-and-evidence.md` for approval, restoration, and evidence;
- `references/delegation.md` for the `lumi-tester-agent` handoff/response.

## Test-plan handoff

Before delegating, produce a plan containing at least:

```yaml
schemaVersion: "1"
planId: "<unique run plan id>"
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
  complete: true
  expectedCount: 2
  allowlist: ["9007199254740993"]
authorization:
  real_device_control: true
  restore_state: true
execution:
  repeat: 1
  max_repeat: 5
  mode: sequential
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
targets:
  - entity: "9007199254740993"
    status: eligible
    capability: switch
    mapping_source: protocol_profile
    actions:
      - capture_initial_state
      - set: { input_idx: 0, value: true }
      - readback: { state_idx: 0, expected: true }
      - restore_initial_state
acceptance:
  - control request is accepted
  - readback matches the requested value
  - final state matches the captured initial state
evidence:
  report: true
  events: true
  label: real_e2e
```

This is a handoff plan, not a replacement for Lumi Tester's canonical
`header --- commands` YAML. `lumi-tester-agent` must materialize and validate
the executable YAML.

## Coverage policy

For a request to test all devices:

- include every discovered supported capability in the inventory summary;
- plan a separate case or clearly isolated case per entity/capability, then ask
  `lumi-tester-agent` to materialize it;
- keep repeated values in the plan and use `generated_from_plan` when cycles
  differ; do not create duplicated hand-authored cycle files;
- include `switch`, `rgb`, and `cct` only when mappings and safe values are
  known;
- mark unsupported, offline, read-only, or ambiguous entities as `skipped` or
  `blocked` with a reason;
- do not claim complete coverage from a static profile list alone.

For repeat or concurrency requests, state whether the count is per entity,
per scenario, or per whole suite. Use sequential execution by default. Use
parallel execution only when targets are independent and the user explicitly
allows it; record batch membership so timing and failures remain clear. Bound
repeat counts using an explicit test policy or profile limit; never turn an
unbounded request into hardware stress testing.

For App → Cloud → HC, require explicit `app`, `cloud`, and `gateway` component
declarations and create stage boundaries with correlation data:

```text
App action → Cloud request/ack → HC dispatch → HC readback → App observation
```

If only HC is in scope, omit App and Cloud stages rather than pretending they
were tested.

Real Cloud access is not available in the current project scope. A Cloud mock
must be labelled as mock evidence; a run with real HC and mocked Cloud is mixed
evidence, not full real end-to-end evidence. Do not silently substitute one for
the other.

## Safety and failure rules

- Treat real-device control as opt-in. Missing permission is `blocked`, not an
  implicit approval.
- Read the initial state before a mutation whenever restoration is requested.
- Restore every touched field, including color or temperature, in a guaranteed
  cleanup path after success, failure, or cancellation; verify final state.
- Never hardcode a cleanup state as a substitute for restoring captured state.
- Never retry a mutating request blindly after an unknown timeout; read state
  first and classify the outcome.
- If discovery, endpoint, entity, mapping, or state verification is ambiguous,
  stop that target and explain the missing evidence. A target-local stop is not
  a suite-wide stop when independent targets remain safe and executable.
- If a target has executed and control/readback/restore fails, classify it as
  `failed`, quarantine it, preserve evidence, and continue independent targets
  under the declared failure policy. Do not relabel it `blocked` and do not
  blindly retry its mutation.
- Stop the whole run only for a shared HC health/discovery failure, inventory
  drift, or unresolved system-wide safety condition; report remaining targets
  as `not_run`.
- Keep secrets out of plans and reports; reference environment variables.
- Use a unique output directory per orchestration run and retain raw delegated
  artifacts.

## Final report shape

Report these sections in order:

1. Scope and environment, including real-device mutation permission and whether
   evidence is `mock_contract`, `mock_conformance`, `real_preflight`, `mixed`,
   or `real_e2e`.
2. Discovery summary and source timestamp.
3. Capability/mapping decisions and skipped or blocked targets.
4. Scenario matrix: scenario intent, target requirement, target, action,
   repetition, execution mode, expected state, and whether execution occurred.
5. Results separated into technical checks and requested scenarios:
   pass/fail/skipped/blocked/error/not_run, dispatch latency, readback latency,
   observed state, final state, and evidence paths.
6. Overall business result, restoration status, and unresolved risks.

Do not report a target-dependent stateful scenario as successful merely because
an HTTP request returned 2xx; its expected state must be verified by the
readback contract. Technical checks use their own declared response/assertion
contract.

## Machine-verifiable orchestration

The repository provides semantic contract checks without duplicating HC
execution:

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate-delegation plan.yaml response.json --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan aggregate plan.yaml --response response.json --output ./output/orchestrator --json
```

The aggregate preserves lifecycle outcomes, quarantine, inventory drift,
artifact paths, evidence tier, and honest restoration status. The orchestrator
must not call HC endpoints or synthesize unsupported restore payloads.
