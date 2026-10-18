# Developer evidence — 2026-10-18-14-56-25_implement-hc-http-ws-canonical-evidence

## Rework result

Execution-first TDD completed for the five requested gaps.

- HTTP control now performs one exact v3 control POST, bounded state readback, entity/index/value assertion, and required raw plus normalized evidence. Dispatch rejection, timeout, mismatch, transport/payload error, and blocked outcomes remain structured.
- WebSocket subscribe acknowledgement has a total deadline; `expect_state` propagates evidence-capture errors and cannot pass without captured evidence.
- `E2eResult::passed` requires explicit action, readback, assertion, bounded evidence, and normalized evidence. Failed, blocked, and error statuses remain distinct.
- The existing `lumi_e2e` / `system_journey` executor path, CLI entry, human event output, and JSON `test-results.json` boundary expose the structured E2E result. Legacy App YAML remains on the existing runner path.

## RED evidence

Focused RED filters were run before the implementation changes, after the required VS x64 environment setup.

- `http_control_requires_bounded_readback_and_normalized_evidence`: 0 passed, 1 failed, 0 ignored, 9 filtered.
- `http_control_status_true_cannot_hide_readback_mismatch`: 0 passed, 1 failed, 0 ignored, 9 filtered.
- `websocket_subscribe_ack_wait_is_bounded`: 0 passed, 1 failed, 0 ignored, 9 filtered.
- `websocket_expect_state_propagates_required_evidence_errors`: 0 passed, 1 failed, 0 ignored, 9 filtered.
- `passed_result_requires_explicit_normalized_evidence`: 0 passed, 1 failed, 0 ignored, 9 filtered.
- `e2e_runner_reaches_existing_report_boundary_with_structured_observation`: 0 passed, 1 failed, 0 ignored, 27 filtered.

Each RED failure was the intended pre-change behavior: missing HTTP readback, status-only control success, unbounded WS acknowledgement, swallowed evidence errors, permissive result construction, and the old `Unknown platform: lumi_e2e` runner boundary.

## GREEN verification

Commands were run from `D:\nl-tester\lumi-tester` with:

`call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64 && set "PATH=C:\Users\Admin\.cargo\bin;C:\Program Files\Git\cmd;%PATH%"`

- `cargo test --manifest-path Cargo.toml --test e2e_hc_contract -- --nocapture`: 10 passed, 0 failed, 0 ignored, 0 measured, 0 filtered.
- `cargo test --manifest-path Cargo.toml --test e2e_parser_profile -- --nocapture`: 28 passed, 0 failed, 0 ignored, 0 measured, 0 filtered.
- `cargo test --manifest-path Cargo.toml --lib e2e -- --nocapture`: 12 passed, 0 failed, 0 ignored, 0 measured, 150 filtered.
- `cargo test --manifest-path Cargo.toml --test e2e_parser_profile e2e_runner_reaches_existing_report_boundary_with_structured_observation -- --exact --nocapture`: 1 passed, 0 failed, 0 ignored, 0 measured, 27 filtered.

`cargo fmt --all -- --check` remains non-clean because the repository already contains broad unrelated rustfmt drift; no broad formatting rewrite was applied.

## Evidence tier and environment

- Mock tier: `MockHttpTransport` and `MockWebSocketTransport` contract/conformance coverage passed.
- Real tier: real HC lab execution remains blocked at `real_preflight` because the integration profile endpoint variables (`LUMI_HC_BASE_URL` and `LUMI_HC_WS_URL`) were not available. No hardware or real HC endpoint was contacted.

## Deferred scope

Automation/rule/scene/schedule/group/App/Cloud/MQTT/SSH/serial/hardware-jig scope remains deferred outside this single-device `system_journey` boundary.

## Changed files

Rework touched:

- `lumi-tester/src/e2e/http.rs`
- `lumi-tester/src/e2e/websocket.rs`
- `lumi-tester/src/e2e/result.rs`
- `lumi-tester/src/e2e/evidence.rs`
- `lumi-tester/src/e2e/mod.rs`
- `lumi-tester/src/runner/executor.rs`
- `lumi-tester/src/main.rs`
- `lumi-tester/tests/e2e_hc_contract.rs`
- `lumi-tester/tests/e2e_parser_profile.rs`
- `developer-last-message.md`

Pre-existing same-task changes in the workspace, including `src/e2e/canonical.rs`, `src/e2e/profile.rs`, `src/e2e/types.rs`, and `src/lib.rs`, were preserved.

## Git safety

No commit, push, tag, PR, reset, checkout, or other Git-history mutation was performed. `.process` was not edited. The pre-existing dirty worktree remains dirty.
