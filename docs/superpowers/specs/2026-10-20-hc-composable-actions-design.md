# HC Composable Actions Design

## Status

Approved for implementation on 2026-10-20. This document is intentionally
uncommitted because the user requested no automatic git commit or push.

## Goal

Make the Home Controller path follow the existing Android, iOS, and Web
execution model: parsed atomic commands are executed sequentially by the common
runner, while reusable behavior is composed with `runFlow`. Future HC features
must be addable as new atomic commands and reusable flows without adding
feature-specific branches to the orchestrator.

## Existing architecture to preserve

The canonical path is:

```text
YAML header + commands
        -> parser (`TestCommand`)
        -> `MultiTargetRunner`
        -> `TargetAdapter`
        -> platform driver or HC protocol backend
```

`runFlow` is the existing composition primitive. It supports inline commands or
a child YAML path, carries target scope and variables, and participates in the
same command index/report lifecycle. The HC adapter already uses this path; its
backend differs from an app driver only at the final dispatch boundary.

## Design rules

1. Every HC operation is an atomic command. In the current V3 backend these are
   `hcHealth`, `hcGetEntities`, `hcGetState`, `hcControl`, `hcWaitState`,
   `wsSubscribe`, `wsExpect`, and `wsSnapshot`.
2. `hcControl` may keep its internal HTTP readback because that is a backend
   operation needed to verify the control request. It must not cause the
   runner or orchestrator to inject other commands.
3. Independent confirmation layers are explicit flow commands. A flow that
   needs control plus WebSocket and final API confirmation writes those actions
   in order, for example `wsSubscribe -> hcControl -> wsExpect -> hcWaitState`.
4. `lumi-test-orchestrator` owns semantic planning: inventory, target selection,
   repeat/delay/concurrency, restoration policy, safety gates, failure
   isolation, and aggregation. It does not synthesize command sequences or
   become a second Lumi runner.
5. `lumi-tester-agent` owns canonical YAML materialization, reusable subflows,
   `validate`, `list`, `run`, and evidence inspection. Repeated values remain
   data in a plan or data file; the generated flow remains an ordinary Lumi
   flow.
6. A future feature such as automation rules is composed from primitives (for
   example `hcCreateRule`, `hcControl`, `wsExpect`, `hcWaitState`, and
   `hcDeleteRule`). Adding a missing primitive requires the normal parser,
   command enum, backend mapping, and capability updates, but does not require
   an orchestrator action graph or automatic companion insertion.
7. A profile without WebSocket or a separate API readback uses only the actions
   its capabilities support. Unsupported actions are rejected by normal flow
   validation/preflight; no universal HC sequence is assumed.

## Scope of implementation

- Remove the in-progress HC action-catalog/action-contract auto-composition
  layer and its `list --json`/agent gate coupling.
- Keep the existing atomic HC command and backend boundaries intact.
- Keep existing orchestrator lifecycle, inventory, restoration, and aggregation
  hardening that is independent of action composition.
- Add regression tests proving HC commands stay explicit and `runFlow` remains
  the composition mechanism.
- Update the HC authoring/orchestrator documentation with a reusable control
  subflow and a future automation-rule composition example.

## Out of scope

- Implementing new HC protocol operations such as rule creation or scheduling.
- Changing Android/iOS/Web runner semantics.
- Introducing a dynamic plugin registry for arbitrary unknown YAML commands.
- Running a real HC mutation during offline verification.

## Acceptance criteria

- `list --json` exposes the same command metadata shape for HC as for other
  platforms; it does not publish action dependency metadata.
- A YAML flow containing only `hcControl` remains an explicit one-command flow
  (subject to its normal backend-internal readback).
- A YAML flow containing the four-layer control sequence lists and executes all
  four commands in authored order.
- A `runFlow` inline/path composition is validated and executed through the
  existing common runner for HC.
- The orchestrator does not normalize `cycles` into hidden HC command nodes.
- Offline parser, adapter, and orchestrator tests pass; no git commit or push is
  performed.
