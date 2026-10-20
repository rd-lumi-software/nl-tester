# Multi-HC Protocol Adapter Refactor Design

## Goal

Create a protocol-neutral Home Controller execution boundary in Lumi Tester. Existing lumi-iot-v3 HTTP/WebSocket execution must continue to work with current YAML and report behavior, while a future HCG1, MQTT-based HC, or another HC can be added as a new backend without changing the YAML parser or multi-target runner.

This phase does not implement HCG1 transport, MQTT/TLS, HCG1 discovery, or production device mappings.

## Current problem

The multi-target layer already exposes TargetId::Gateway and the public platform lumi_hc, but the gateway implementation is coupled directly to the v3 E2E runtime. Profile validation also accepts only the v3 implementation and protocol-pack identifiers. Adding a second HC would therefore require changing the gateway adapter, profile parser, or runner dispatch instead of registering a separate protocol backend.

The following public behavior must remain stable:

- canonical gateway YAML with platform: lumi_hc;
- previous HC platform aliases are rejected;
- hcHealth, hcGetEntities, hcGetState, hcControl, and hcWaitState;
- current indexed state syntax: state: [{idx, value}];
- v3 HTTP/WebSocket routes, bounded retry behavior, readback, evidence, and report fields;
- blocked/error classification when a real HC profile or lab is unavailable.

## Proposed architecture

Keep the existing TargetAdapter as the lifecycle boundary used by the multi-target runner. Refactor only the inside of the gateway adapter into three layers:

    YAML / parser
        -> protocol-neutral HcCommand
        -> HcProtocolBackend selected from profile
        -> protocol transport and wire payloads
        -> normalized HcObservation / HcResult
        -> existing AdapterResult / report / evidence

### Protocol-neutral HC domain

Add a small hc domain module containing commands and observations meaningful across HC implementations:

- health check;
- entity discovery;
- entity state read;
- entity control;
- state wait/readback;
- normalized entity identifiers and state values;
- capability declarations and evidence metadata.

The domain model continues to accept indexed state updates so existing v3 YAML remains valid. It may also represent semantic capability updates, but a backend may reject a capability it does not advertise. This leaves room for a future HCG1 mapping without forcing a v3 wire format into the shared model.

### Backend interface

Define an internal HcProtocolBackend trait. It owns protocol-specific connection state, transport, payload conversion, retry policy, and raw evidence capture. Its public inputs and outputs are only protocol-neutral HC domain types.

    trait HcProtocolBackend: Send {
        fn implementation_id(&self) -> &str;
        fn protocol_pack(&self) -> &str;
        fn capabilities(&self) -> &[HcCapability];
        fn preflight(&mut self, context: &TestContext) -> HcBackendResult;
        fn open(&mut self, context: &TestContext) -> HcBackendResult;
        fn execute(&mut self, command: &HcCommand, scope: &HcExecutionScope) -> HcBackendResult;
        fn close(&mut self, context: &TestContext) -> HcBackendResult;
    }

HcGatewayAdapter remains the TargetAdapter implementation. It translates parsed
`TestCommand::V3` values to `HcCommand`, converts the runner scope to
`HcExecutionScope`, delegates lifecycle and execution to the selected backend,
and converts `HcBackendResult` to `AdapterResult`. The HC domain does not
depend on `TargetScope` or any adapter module. The runner does not learn about
v3, HCG1, HTTP, MQTT, or WebSocket.

### Profile and backend selection

Separate common profile fields from v3-only transport fields.

Common fields:

- implementation ID;
- protocol-pack ID/version;
- target metadata;
- capabilities;
- evidence policy;
- retry policy;
- backend configuration.

V3 fields:

- HTTP base URL;
- WebSocket URL;
- HTTP auth details;
- v3-specific protocol settings.

The registry/factory selects a backend from profile identity. This phase registers one production backend:

    lumi_iot_v3_home_controller
        -> V3HcProtocolBackend

An in-memory fake backend is registered only in test code. An unknown implementation must produce a deterministic unsupported-profile diagnostic and must never silently fall back to v3.

### Compatibility policy

No breaking YAML migration is required for existing v3 flows: they continue to
use `platform: lumi_hc`, the same profile reference, and the same command
names. The new domain model is an internal boundary first; semantic capability
YAML is enabled only when a backend and schema contract support it.

The v3 backend remains responsible for:

- GET /api/health_check;
- GET /api/devices;
- GET /api/devices/{id}/state;
- POST /api/devices/{id}/control with indexed {idx, value} updates;
- WebSocket subscription/readback;
- bounded safe-read retries and no automatic retry of control POST;
- raw and normalized evidence with redaction and evidence-tier classification.

## Testing strategy

1. Domain tests verify command conversion and normalized state behavior without a network.
2. Backend contract tests run the same command sequence against a fake backend and assert lifecycle, capability rejection, readback, and evidence behavior.
3. Existing v3 HTTP/WebSocket contract tests remain and run behind V3HcProtocolBackend without changing their wire assertions.
4. Registry tests verify profile identity selects v3, unknown identity is rejected, and a fake backend can be injected without parser or runner changes.
5. Existing parser, multi-target, report, and unsupported-platform tests remain green.
6. Documentation gives a future adapter author the exact files, trait methods, profile entry point, and required contract tests, but does not claim HCG1 support.

## Completion criteria

The refactor is complete when:

- current v3 HC YAML validates without modification;
- v3 contract tests pass through the new backend boundary;
- a fake backend executes a representative health/discovery/control/readback flow through the existing runner;
- adding a backend requires implementing the backend trait and registering its profile identity, with no parser or runner changes;
- unknown backend identities fail clearly;
- reports and evidence retain existing adapter/status/evidence fields;
- the architecture and future adapter procedure are documented;
- no HCG1 production transport code is added in this phase.
