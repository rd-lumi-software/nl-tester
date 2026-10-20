# Orchestration Workflow

Use this reference when a request spans discovery, test generation, execution,
and result interpretation. The orchestrator owns decisions; the delegated
`lumi-tester-agent` owns Lumi command syntax and runtime operations.

## Phase contract

| Phase | Required input | Output | Stop condition |
| --- | --- | --- | --- |
| Clarify | User intent and environment | Scope record | Target or mutation permission is missing |
| Discover | Read-only target access | Live inventory | Health or inventory cannot be verified |
| Classify | Inventory plus capability metadata | Capability map | Mapping is unknown or contradictory |
| Plan | Scope, map, policy | Test-plan handoff | Restoration or acceptance is undefined |
| Delegate | Approved plan | Executable YAML and indexes | YAML is invalid or command shape is uncertain |
| Execute | Validated YAML and authorization | Raw run artifacts | Safety gate fails or target changes |
| Aggregate | Run artifacts | Coverage/result report | Evidence is incomplete for a claim |
| Restore | Captured initial state | Final-state evidence | State cannot be verified |

The Clarify phase is a hard intake gate for mutating tests. It must produce a
complete record for endpoint/profile, real-device permission, target selection,
capability scope, repeat semantics, execution mode, delay/rate limit, timeout,
evidence, and restoration. Missing values are presented to the user; they are
not replaced with runner defaults. A read-only health/inventory preflight may
run before the gate only when it is explicitly labelled non-mutating.

## Scope decision

Use the narrowest scope that satisfies the request:

- `hc_only`: direct HC discovery, control, and readback.
- `app_only`: app flow only; do not infer Cloud or HC success.
- `cloud_only`: API/Cloud behavior only; do not infer device actuation.
- `app_cloud_hc`: explicit end-to-end stages with correlation IDs.

When the user asks for “all devices” without naming an app or Cloud path, use
`hc_only` and confirm that real-device mutation is permitted before control.

For a partial request such as “test controllable devices at this IP”, parse and
retain the IP as `host` first, then ask only for the missing scheme/port/profile,
target selection, capability scope, repeat count, sequential/parallel mode,
delay, timeout, evidence, and restore policy. Show the host as `provided` and
the connection as `partial`; never render the complete endpoint as empty or ask
for the same IP again. An IP address and a statement that some devices are
controllable are not enough to authorize a control suite.

## Scenario policy

Every mutating scenario must describe:

1. target entity and capability;
2. initial-state read;
3. control value and input mapping;
4. expected readback field/value;
5. timeout and retry policy;
6. restoration action and expected final state.

The orchestrator can request a family of scenarios, but it must not silently
expand a simple smoke request into destructive stress testing. Expansion such
as boundary values, rapid toggles, repeat cycles, or concurrent batches must be
listed as additional scenarios and approved by the stated test scope.

## Result classification

- `passed`: action and readback acceptance criteria passed, and restoration
  passed when required;
- `failed`: the test ran and an acceptance criterion failed;
- `blocked`: the test was not safely executable because a prerequisite,
  mapping, permission, or evidence requirement was missing;
- `skipped`: the target was intentionally excluded by an explicit policy, such
  as unsupported capability or read-only entity;
- `error`: execution failed at the transport/runtime boundary without enough
  evidence to decide whether the product assertion passed or failed.

Never convert `blocked` or `skipped` to `failed` merely to make a summary look
complete. Report coverage as discovered, eligible, executed, and unexecuted.
Never claim “all” coverage when discovery completeness or an explicit allowlist
is absent.

For a real-device run, restoration must be attempted after a failure or
cancellation as well as after a pass. If restoration cannot be verified, keep
the target non-passing, classify it as `failed`, quarantine it, and stop
further mutations for that target. Under the default target-scoped failure
policy, continue independent targets when shared HC health, discovery,
inventory, and safety checks remain valid. A run-level stop is reserved for
shared failures; targets not reached after such a stop are `not_run`.

## Delegation prompt

Give `lumi-tester-agent` a compact handoff containing:

```text
Target scope: <hc_only | app_only | cloud_only | app_cloud_hc>
Environment: <profile, endpoint variable, app/device identity>
Approved plan: <structured test plan>
Authoring request: materialize canonical YAML; do not guess unsupported fields
Validation: validate then list before execution
Execution: report/events/snapshots as supported; use a unique output directory
Debug: inspect artifacts and rerun only the smallest failing command
```

Ask for executable YAML and its validation/list output before treating the plan
as runnable. For App → Cloud → HC, require explicit `app`, `cloud`, and
`gateway` declarations and label mock, mixed, and real evidence separately.
