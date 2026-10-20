# HC Gateway Execution

Use this reference when a Lumi Tester flow targets a real or mocked Home
Controller with `platform: lumi_hc`. This is an execution guide: it explains
how to author, validate, run, and debug an explicit HC YAML flow. It does not
define product coverage or automatically choose a complete test suite.

## Target and profile

HC flows use a gateway component and an HC profile. Keep endpoint values out of
the YAML by resolving them from environment variables:

```yaml
platform: lumi_hc
gateway:
  platform: lumi_hc
  profile: profiles/lumi-iot-v3.yaml
tags:
  - hc
  - smoke
defaultTimeout: 15000
---
- hcHealth
- hcGetEntities
```

The profile normally contains:

```yaml
implementation: lumi_iot_v3_home_controller
protocolPack: lumi_iot_v3_home_controller
endpoint:
  baseUrl: "${LUMI_HC_URL}"
  websocketUrl: "${LUMI_HC_WS_URL}"
auth:
  mode: none
```

Run with explicit endpoints supplied by the environment:

```powershell
$env:LUMI_HC_URL = "http://<hc-host>:<port>"
$env:LUMI_HC_WS_URL = "ws://<hc-host>:<port>/ws"
```

Do not print or commit profile secrets such as `dev_key`, `nwk_key`, or
`app_key`. The profile evidence redaction list must include those fields.

## HC command contract

| Command | Purpose | Required rule |
| --- | --- | --- |
| `hcHealth` | Check HTTP readiness | Run before real control. |
| `hcGetEntities` | Discover live entities | Use the live result as the source of truth. |
| `hcGetState` | Read an entity state vector | Use the entity ID as a decimal string. |
| `hcControl` | Send indexed `{idx, value}` updates | Do not retry a control POST automatically. |
| `hcWaitState` | Wait for bounded state readback | A control passes only when the requested state matches. |

For the current v3 backend, discovery uses the exact route
`GET /api/devices` without a trailing slash. Do not replace discovery with a
manual HTTP call when the requested test is meant to verify Lumi Tester.

## Entity and index rules

- `entity` is the device/entity ID used in the URL; keep large IDs as strings.
- `cell_idx` identifies the HC cell and is not automatically a control index.
- Control `idx` must come from the entity's `spec.input` mapping.
- Readback `idx` must come from the entity's `spec.state` mapping.
- Never infer an index from a device name, list position, or `cell_idx`.
- If input/state mapping is missing or ambiguous, stop with `blocked` or
  `unsupported`; do not guess.

For a real device, the safe baseline sequence is:

```yaml
- hcHealth
- hcGetEntities
- hcGetState:
    entity: "<entity-id>"
- hcControl:
    entity: "<entity-id>"
    state:
      - idx: <control-idx>
        value: <valid-value>
- hcWaitState:
    entity: "<entity-id>"
    state:
      - idx: <state-idx>
        value: <expected-value>
```

Read the initial state before control and restore it after the scenario unless
the user explicitly requests a different final state. For multiple real
devices, run sequentially by default so a failed readback identifies one
entity. Do not treat `{status: true}` from `hcControl` as physical confirmation;
it only confirms dispatch unless the subsequent readback matches.

Before the first real `hcControl`, require all of these to be explicit or
unambiguously present in the flow/profile:

- the intended HC endpoint (`LUMI_HC_URL`);
- the exact entity ID to control;
- the input/state index mapping;
- the control value and desired final state.

If any item is missing, run only health, discovery, and state reads and report
the flow as not ready for control. Do not turn a discovery result into a
control target by guesswork.

The executor cannot claim dynamic state restoration unless the flow or runtime
actually stores the initial value. A hard-coded cleanup value is an explicit
target state, not proof that the initial state was restored; report that
distinction in the result.

## Repeated cycles and template materialization

When an orchestrator plan contains repeated cycles, treat its ordered cycle
data as the source of truth. If a reusable capability template is absent, this
agent must create one or a small generator before materializing the executable
flows. Use separate templates when switch, RGB, CCT, and dimmer state shapes
differ.

For HC indexed commands, materialize boolean, numeric, and string values as
literal YAML values before `validate`. Do not assume that generic `forEach`,
`repeat`, or `${cycle.value}` substitution preserves types or is dispatched by
the HC multi-target runner. `--repeat` is appropriate only when every repeated
execution is the same literal flow; it does not express alternating values.

Generated flows should live under the requested output or an ignored generated
directory, not beside maintained source as duplicated `cycle-N/control.yaml`
files. Validate and list every generated executable flow. The orchestrator owns
the delay between generated cycle runs; do not remove a required delay merely
to collapse files into one flow. Preserve per-cycle evidence and failure
isolation when requested.

## Preflight and execution

Always validate and list before a real run:

```powershell
cargo run --manifest-path .\lumi-tester\Cargo.toml -- validate <flow.yaml> --json
cargo run --manifest-path .\lumi-tester\Cargo.toml -- list <flow.yaml> --json
cargo run --manifest-path .\lumi-tester\Cargo.toml -- run <flow.yaml> `
  --platform lumi_hc `
  --report `
  --events-jsonl `
  --output .\output\<unique-run>
```

`doctor --platform lumi_hc` is not a substitute for HC readiness; the current
doctor command checks local app/browser/desktop dependencies and does not
probe the gateway. Use `hcHealth` and the endpoint environment variables for
HC preflight.

## Evidence and failure handling

With `--report --events-jsonl`, inspect:

- `run.json` for operation order, status, and command indexes;
- `test-results.json` for report-level pass/fail details;
- `events.jsonl` for chronological command events;
- profile evidence `raw` and `normalized` artifacts for HTTP/WS request,
  response, dispatch, and readback data.

When a flow fails:

1. Find the first failed command in `run.json` or `test-results.json`.
2. Check whether the failure is health, discovery, mapping, control dispatch,
   or readback.
3. Rerun the smallest failing command with `--command-index` only when the
   operation is safe to repeat. Never rerun a control POST merely to obtain
   evidence.
4. After fixing the YAML or environment, rerun the full flow.

HC evidence must distinguish `dispatched`, `readback matched`, `blocked`, and
`unsupported`; a generic command pass without matching state is insufficient
for a physical-control claim.
