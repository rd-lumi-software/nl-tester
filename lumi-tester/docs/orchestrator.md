# Lumi Orchestrator Contracts

The orchestrator is a semantic planning and evidence layer above
`lumi-tester-agent`. It validates scope, inventory, authorization, target
selection, lifecycle outcomes, and delegation responses. It does not call HC
HTTP endpoints and does not replace Lumi YAML execution.

## CLI

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --for-run --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate-delegation plan.yaml response.json --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan aggregate plan.yaml --response response.json --inventory-current current.json --output ./output/orchestrator --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan schema --json
```

`plan validate` performs structural validation. `--for-run` additionally
requires an explicit endpoint/connection, authorization, complete inventory
evidence, target mappings, shared stop conditions, and honest restoration or
final-state-capture policy.

The bundled JSON schema is the structural interchange contract. Semantic
constraints—path containment and existence, runtime/repetition invariants,
delegation evidence, lifecycle transitions, and restoration honesty—are
authoritatively enforced by the Rust `plan validate` and
`plan validate-delegation` commands.

`plan aggregate` repeats the run-ready plan check and delegation validation
before writing `aggregate.json`. Relative evidence paths are resolved from the
response file directory and must point to existing files below `outputDir`.
Invalid responses never produce a success aggregate. `inventory.snapshotPath`
is resolved relative to the plan file; if no embedded or readable baseline is
available, inventory status is `unverifiable` and the run is stopped.

## HC safety

`doctor` is an app/browser/desktop dependency check. It is not HC readiness.
For HC, use a read-only executable flow containing `hcHealth`, optionally
`hcGetEntities` and `hcGetState`, through
`lumi_agent.py agent-hc-preflight`. A preflight flow must not contain
`hcControl`; mutation belongs to an explicitly authorized delegated run.

## Restoration

`restore_state: false` is reported as `not_requested`, never as restored. When
restoration is enabled, the agent must capture initial state, materialize a
typed restore flow, execute it, and return verified initial/final readback
evidence. The Rust orchestrator validates that evidence but does not synthesize
HC control payloads.

Evidence tiers are explicit: `mock_contract`, `mock_conformance`,
`real_preflight`, `mixed`, or `real_e2e`. They must not be upgraded merely
because a transport request succeeded.

## Inventory and lifecycle

Inventory snapshots contain exact string entity IDs, capabilities, entity
fingerprints, and mapping fingerprints when mutation depends on mapping. Added,
removed, changed, incomplete, or unverifiable inventory causes a run-level stop;
untouched targets become `not_run`.

An executed target failure is `failed` and quarantined. Independent targets may
continue. Pre-execution missing prerequisites are `blocked`, unsupported targets
are `skipped`, and shared safety failures stop the remaining run.

The aggregate report is written to `aggregate.json` and preserves target
outcomes, lifecycle events, evidence tier, artifact paths, state observations,
restoration status, and missing measurements without inventing values.
Each target also carries its derived lifecycle/quarantine record, including
shared-stop `not_run` reasons and post-execution quarantine.
