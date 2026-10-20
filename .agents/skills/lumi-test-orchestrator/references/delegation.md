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
the aggregate report.
