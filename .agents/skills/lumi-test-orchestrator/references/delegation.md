# Lumi Tester Delegation

Use this reference to hand a semantic plan to `lumi-tester-agent` without
duplicating its YAML or runtime responsibilities.

## Delegation phases

Use one explicit phase at a time:

```text
discover → classify → author → validate → list → run → inspect → rerun
```

The orchestrator may request discovery and authoring separately so an inventory
or safety decision can be reviewed before any mutating YAML is run.

## Authoring handoff

Provide:

- the plan ID and scope;
- target profile and endpoint environment-variable names;
- immutable inventory snapshot and exact entity selection;
- capability and mapping sources;
- semantic actions, acceptance criteria, repeat/concurrency policy;
- template policy, cycle data source, and generated-flow layout;
- restoration and evidence requirements;
- requested output directory.

Ask the agent to reuse an existing capability template or create a reusable
template/generator when none exists, then materialize canonical `header ---
commands` YAML, not a new YAML dialect. Generated flows may be ephemeral or
ignored artifacts, but every executable flow must be validated. The agent must
use its own command/header references and must not infer unsupported indexes or
values.

For HC, the handoff contains semantic target/cycle data; it does not contain or
request an orchestrator-generated companion-action graph. The agent is
responsible for composing explicit atomic commands and `runFlow` subflows. For
example, WebSocket confirmation is represented by authored `wsSubscribe` and
`wsExpect` commands around an authored `hcControl`, followed by an authored
`hcWaitState` when the profile supports that API. The runner never inserts
these commands implicitly.

## Validation and execution handoff

Require the agent to return machine-readable results for:

```text
validate --json
list --json
run ... --report --events-jsonl [--snapshot]
```

The agent owns command indexes. The orchestrator must not count indexes by hand.
For a failure, ask the agent to inspect `run.json`, `test-results.json`, event
JSONL, and linked artifacts, then rerun only the smallest failing command before
rerunning the complete flow.

## Required delegation response

```yaml
planId: "<plan id>"
phase: run
status: passed
runtimeStatus: completed
validation: { valid: true }
listedCommands: []
flowPaths: []
outputDir: "output/orchestration/<run-id>/"
template: { status: created|reused|not_needed, path: "<path or null>" }
generatedFlowPaths: []
evidenceTier: real_preflight
artifactPaths: []
unresolved: []
```

The response must distinguish `passed`, `failed`, `blocked`, `error`, and
`not_run`. Missing artifacts or unresolved assumptions must remain visible in
the aggregate report. An `error` response with target results is a target-local
error that remains quarantined and does not stop independent targets; an
`error` response without target results is a delegation-level error and stops
the remaining run.

Validate the response mechanically before aggregation:

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate-delegation plan.yaml response.json --json
```

The validator rejects plan ID mismatches, unknown targets/evidence tiers,
duplicate command indexes, artifacts outside `outputDir`, unresolved
assumptions on a passed response, and false restoration claims. The
orchestrator consumes the agent's listed indexes; it never counts command
indexes itself.

Before aggregation, validate the response with `plan validate-delegation`.
The validator checks plan identity, phase/status, validation result, unique
listed command indexes, output-root containment for flow paths, artifacts, and
generated flows, evidence tier, required `run.json`, restoration verification,
and unresolved assumptions. It also checks runtime status and repetition
invariants: a run target's requested count must match `execution.repeat`,
`passed + failed` must equal `completed`, and a passed target must complete and
pass every requested repetition. Optional lifecycle records must have matching
identity, transition history, and terminal state. A response cannot claim
`passed` while evidence is incomplete, and a failed/error response that reached
mutation still needs honest restoration evidence when restoration is enabled.

`plan aggregate` repeats plan and response validation before creating the
aggregate report. It resolves relative artifact paths from the response file's
directory, refuses paths outside `outputDir`, and leaves no success report when
validation fails.
