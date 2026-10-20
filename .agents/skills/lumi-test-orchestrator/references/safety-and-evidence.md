# Safety and Evidence Contract

Use this reference before coordinating any real-device mutation. The plan is
not executable until the safety gate is complete.

## Safety gate

Require all of the following:

- explicit approval to affect real hardware;
- endpoint/profile and environment allowlist;
- exact entity IDs, capability, input/state indexes, and allowed values;
- inventory snapshot ID, timestamp, and freshness check;
- repeat, delay, timeout, concurrency, rate, and control-retry limits;
- initial-state capture and field-by-field restore-and-verify policy;
- target-local abort behavior for readback mismatch or restore failure;
- run-level abort behavior for inventory drift or shared safety failure.

If a selected entity, mapping, or endpoint no longer matches the discovery
snapshot, stop and rediscover. Do not silently reuse stale mappings.

## Evidence tiers

Use exactly one label for each result:

- `mock_contract`: deterministic mock only;
- `mock_conformance`: deterministic backend exercised against the HC contract,
  without a live gateway;
- `real_preflight`: real target health/discovery/state inspection, no mutation;
- `mixed`: some components are real and another required component is mocked;
- `real_e2e`: every required component and assertion ran against the declared
  real target.

Never upgrade `mock_contract`, `real_preflight`, or `mixed` to `real_e2e`.

## Required control evidence

For each mutating scenario retain:

1. initial state and mapping source;
2. exact semantic action and wire input;
3. dispatch request/response with secrets redacted;
4. dispatch-to-response timing;
5. state/readback request and observed value;
6. dispatch-to-readback timing and bounded timeout outcome;
7. restore request and final state verification;
8. output paths for raw and normalized artifacts.

An HTTP 2xx response proves dispatch only. A product control assertion passes
only when the expected readback matches and required restoration is verified.

## Stop conditions

Stop further mutation for the target when:

- readback contradicts the requested state;
- a repeat exceeds the approved limit;
- the target inventory or mapping changes;
- restore cannot be verified;
- a mutating request times out without a definitive outcome.

Classify transport ambiguity as `error` and read the state before any manually
approved follow-up. Do not blindly retry a non-idempotent control request.

A target-local stop is not a suite-wide stop. Quarantine the failed target,
retain its evidence, and continue independent targets when the declared plan
uses target-scoped failure handling and shared HC health, discovery, inventory,
and safety checks remain valid.

Stop the whole run before the next mutation when:

- HC health or discovery fails;
- the live inventory or selected mapping drifts from the discovery snapshot;
- the endpoint/session failure could affect multiple targets; or
- system-wide safety cannot be established.

Report targets not reached after a run-level stop as `not_run`. Do not call an
eligible target `blocked` after it has already executed and failed an assertion
or restore check.

When restoration is disabled, aggregate `restoreStatus: not_requested`; this is
not equivalent to restored. When restoration is enabled, the response must
include captured initial state, a materialized typed restore flow, final-state
readback, and `verified: true`. The orchestrator validates these artifacts but
does not call HC or invent a restore payload.

When `restore_state` is false, the aggregate must report an explicit
not-requested restore status and retain final-state capture. When restoration
is enabled, the response must contain captured initial state, restore evidence,
final-state evidence, and a verified result. The Rust validator checks these
claims; it never calls HC and never invents a cleanup value.
