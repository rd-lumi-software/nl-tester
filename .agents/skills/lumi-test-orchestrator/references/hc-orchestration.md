# HC Orchestration Rules

Use this reference when the target is a real HC gateway. Execution details
remain in `lumi-tester-agent`'s `references/hc-execution.md`; this file covers
higher-level decisions around discovery and coverage.

## Discovery is live

1. Read the selected profile for the endpoint and backend identity.
2. Run the HC health check through `lumi-tester-agent`.
3. Run `hcGetEntities` and retain the raw inventory as evidence.
4. Compare live inventory with any static profile list.
5. Use live entities for eligibility; treat missing/static-only entries as
   `not_discovered`, not as executable targets. For an “all devices” request,
   require a complete discovery signal, trusted expected count, or explicit
   allowlist; otherwise block the completeness claim.

The discovery route is the backend-defined `/api/devices` route. Do not replace
it with a guessed trailing-slash variant or a different endpoint merely because
another HC version uses a similar route.

## Capability classification

Classify from documented entity metadata and supported input/state mappings:

- `switch`: on/off control and boolean readback;
- `rgb`: color or power control with matching color/readback fields;
- `cct`: color-temperature and/or power control with matching readback fields;
- `dimmer`: numeric brightness control and numeric readback;
- `read_only`: discoverable state but no safe control mapping;
- `unknown`: metadata or mapping is insufficient.

Do not assume every entity has only one control channel. A device may expose
multiple channels, and `cell_idx` is not automatically the input/state `idx`.
The plan must preserve this distinction:

```text
entity    = device identity
cell_idx  = backend/device cell address when present
input_idx = index used by the control command
state_idx = index used by the state/readback command
```

## Safe value selection

Choose values that are reversible and observable:

- switch: the opposite boolean of the captured state;
- RGB: a documented test color different from the captured color, then restore;
- CCT: a documented safe temperature within the device range, then restore;
- dimmer: a documented safe level within the device range, then restore.

If the range, mapping, or restoration value is unknown, block the target. Do
not use a guessed value on a real device. Bound repeat counts with an explicit
profile or test-policy limit; do not interpret an unbounded repeat request as
permission for hardware stress testing.

## HC-only control loop

```text
hcHealth
  → hcGetEntities
  → classify eligible entities
  → hcGetState (capture)
  → hcControl
  → hcWaitState / hcGetState (readback)
  → restore captured state
  → hcGetState (final verification)
```

Only the delegated `lumi-tester-agent` should turn this loop into executable
Lumi YAML. The orchestrator records required stages and interprets evidence.

## Repeated controls and template generation

For repeated HC controls, keep ordered cycle values in the orchestrator plan.
Set `execution.flow_layout: generated_from_plan` and
`template_policy: create_if_missing` when values differ between cycles. The
orchestrator owns the semantic cycle data; `lumi-tester-agent` owns the
capability template/generator and materializes typed literal YAML.

Do not create duplicated maintained `cycle-N/control.yaml` files when the only
difference is data already present in the plan. Generated flows may still be
one per cycle at runtime to preserve delay, evidence, and target quarantine.
Do not assume generic `forEach`, `repeat`, or `${...}` substitution is safe for
HC indexed values until the target runner has validated that behavior.

## Failure handling

- Health failure: stop the run; do not control any entity, and report remaining
  targets as `not_run`.
- Discovery failure: stop mutation and block coverage claims; do not synthesize
  IDs from a profile list.
- Unsupported capability: skip with a reason and continue only for independent
  eligible entities.
- Unknown mapping: block that entity; do not try adjacent indexes.
- Control response without readback: outcome is `failed` or `blocked` according
  to whether the readback endpoint was unavailable or contradicted the state.
- Restore failure: report `failed` even if the control assertion passed, stop
  further mutations for that target, quarantine it, and preserve the
  restore/readback evidence. Continue independent targets when the declared
  failure policy is target-scoped and shared HC health, discovery, inventory,
  and safety checks remain valid. Do not blindly retry the failed target.
- Transport/runtime failure with no product assertion: report `error`, preserve
  raw/normalized evidence, isolate the target, and do not silently retry a
  mutating request. Continue independent targets only when the failure is not a
  shared gateway/session safety failure.

An eligible target that executed and failed is never `blocked` merely because
its final state is non-passing. `blocked` is reserved for targets that were not
safely executable before mutation because a prerequisite, permission, mapping,
or required evidence was missing or ambiguous.
