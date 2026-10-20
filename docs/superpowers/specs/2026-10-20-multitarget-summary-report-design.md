# Multi-target Summary Report Design

## Goal

Make every multi-target Lumi Tester run produce a useful human-readable summary
report by reusing `report::summary_html`, while preserving the existing
machine-readable artifacts and adding adapter-specific detail for `lumi_hc`.

## Problem

`MultiTargetRunner::write_artifacts_with_options` currently writes `run.json`,
`test-results.json`, events, and a very small `report.html` containing only the
overall counters. The operation-level data is present in
`operations[].result.rawEvidence`, but it is not rendered as a flow, repeat,
device, control, or readback view. The normal executor already has a richer
summary renderer in `src/report/summary_html.rs`, but the multi-target path does
not use it.

## Scope

In scope:

- all multi-target runs, regardless of adapter;
- reuse of the existing summary HTML layout and report command input format;
- preserving current `run.json`, `test-results.json`, `events.jsonl`, and JUnit
  output contracts;
- generic operation/flow information for every adapter;
- an optional HC section derived only from captured operation evidence;
- unit/integration tests for report conversion and generated HTML.

Out of scope:

- changing device control behavior or HC protocol mappings;
- adding a new CLI command;
- changing existing YAML semantics;
- replacing raw evidence with a lossy summary;
- committing or pushing repository changes.

## Design

### Report data model

`MultiTargetRunReport` remains the source of truth for adapter execution. A
small conversion layer will produce the existing `report::types::TestResults`
shape so the existing summary renderer can be reused. The conversion will
preserve every executed operation as a summary flow/command row and retain the
source target, repeat number, profile, adapter status, and captured evidence in
the multi-target metadata table.

The report model will gain optional multi-target metadata so existing JSON
consumers remain compatible. The metadata will contain adapter names, the
requested repeat count, target/command operation rows, captured evidence, and
optional HC device/readback summaries. Absent metadata must deserialize as
`None`.

### HC-specific summary

When the operation set contains `lumi_hc` records, the renderer will derive an
HC section from captured evidence without issuing any new HC requests. It will
show:

- entity/device identifier when present in normalized evidence;
- operation sequence and repeat number;
- `hcControl`, `hcWaitState`, and `hcGetState` operation rows when captured;
- normalized readback values and operation messages;
- a clear `Not captured` label when optional evidence is absent.

The renderer does not infer control/readback mappings or invent initial/final
snapshots. The complete raw evidence remains in `run.json` and
`test-results.json` for deeper inspection.

The generic report remains usable for non-HC adapters and does not assume HC
fields exist.

### Artifact behavior

With `--report`, the multi-target path will continue to write:

- `run.json` as the complete machine-readable execution manifest;
- `test-results.json` as the report input accepted by `lumi-tester report`;
- `junit.xml` with aggregate operation counters;
- `report.html` as the rich summary page generated through
  `summary_html::render_summary_html`.

With `--events-jsonl`, `events.jsonl` remains unchanged. No raw evidence is
removed or rewritten; the HTML only adds a presentation layer.

### Error handling

Report generation failures are returned as command errors, matching the current
artifact writer behavior. Missing optional fields are rendered as unavailable
data and do not fail a successful run. A malformed evidence payload must not
panic the renderer; it is shown as an unavailable readback value while the raw
JSON remains available for debugging.

## Verification

- Add a failing test proving multi-target artifact generation produces a rich
  summary report rather than the current counter-only placeholder.
- Add tests for generic operation conversion and optional HC metadata.
- Add a test that malformed/missing HC evidence is rendered safely.
- Run focused Rust tests for adapter/report code, then the full Rust test suite.
- Run a local multi-target mock report generation and inspect the resulting
  `report.html`, `test-results.json`, and `run.json`.
