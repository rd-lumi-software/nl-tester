# HC Adapter Development

This guide describes the protocol-neutral Home Controller (HC) backend boundary.
It covers the existing `lumi_hc` adapter and the instance-owned backend factory;
it does not introduce a new wire protocol.

## Registering a backend

Register a backend on an `HcBackendRegistry` instance. Registration is local to
that registry; there is no global mutable backend table.

```rust
let mut backends = HcBackendRegistry::empty();
backends.register(
    HcBackendDescriptor::new("my_controller", "my_pack", "1.0", "1.0"),
    |config| Box::new(MyBackend::new(config)),
)?;
```

The descriptor key is the complete tuple:

- `implementation`
- `protocol_pack`
- `protocol_version`
- `pack_version`

Resolution matches all four fields before invoking the constructor. Missing,
unknown, unsupported, duplicate, and constructor-less registrations return the
typed `HcBackendFactoryError` variants.

Production code normally uses `HcBackendRegistry::new()`, which registers the
selectable V3 descriptor. Tests and alternate integrations can inject a custom
registry with `AdapterRegistry::with_hc_backend_registry` and pass it through
`MultiTargetRunner::standard_with_adapter_registry`.

## Generic profile identity and configuration

`hc::profile::load_profile` is the generic HC profile-envelope loader. It reads
only these identity fields:

```yaml
implementation: my_controller
protocolPack: my_pack
protocolVersion: 1.0
packVersion: 1.0
```

The resulting `HcBackendConfig` also carries opaque JSON configuration containing
the flow `source_path` and relative `profile_ref`. The generic loader does not
assume or inspect `endpoint`, `auth`, `retry`, evidence, or transport fields.
Those fields belong to the selected backend. This lets a fake or future backend
use the same YAML identity envelope without requiring V3 profile fields.

For the existing V3 backend, `hc/backends/v3/mod.rs` consumes those opaque
references and performs the V3 profile load, endpoint substitution/unresolved
checks, and `V3Runtime::new` construction. The HTTP/WebSocket transport and
profile/result logic lives under `hc/backends/v3/runtime`; `lumi_hc` is the
only public HC platform accepted by the parser.

## Protocol-neutral commands

`HcCommand` represents all eight existing gateway operations:

| Domain variant | YAML/display name | Payload |
| --- | --- | --- |
| `Health` | `hcHealth` | none |
| `GetEntities` | `hcGetEntities` | none |
| `GetState` | `hcGetState` | string `entity` |
| `Control` | `hcControl` | string `entity`, indexed/capability updates |
| `WaitState` | `hcWaitState` | string `entity`, expected updates |
| `RealtimeSubscribe` | `wsSubscribe` | channel names |
| `RealtimeExpect` | `wsExpect` | string `entity`, opaque event JSON |
| `RealtimeSnapshot` | `wsSnapshot` | string `entity` |

`HcStateUpdate::Indexed` uses a `u64` index and `serde_json::Value`; entity IDs
remain `String` at this layer. `HcStateUpdate::Capability` carries a semantic
name and JSON value. A backend maps these values to its own protocol only after
receiving the `HcCommand`.

## Backend lifecycle and results

Implement `HcProtocolBackend` with this lifecycle:

1. `preflight(&TestContext)` validates availability and constructs backend
   runtime state without adding an operation-level health request.
2. `open(&TestContext)` opens backend resources. The gateway adapter delegates
   directly to this method.
3. `execute(&HcCommand, &HcExecutionScope)` handles one semantic command. The
   scope contains depth, command index, and correlation ID, without depending on
   runner target types.
4. `close(&TestContext)` releases resources and returns its result; close is
   non-destructive and its result is preserved by the adapter.

Return `HcBackendResult` with the appropriate `HcStatus`, category, message,
action, readback, assertion, normalized evidence, evidence tier, and raw
`HcEvidence` records. `readback_match` should be set when a control or wait
operation performs a readback assertion. The `lumi_hc` facade maps these fields
to the existing adapter/report shape and adds the resolved backend identity as
an additive raw-evidence `backend` block:

```json
{
  "implementation": "my_controller",
  "protocolPack": "my_pack",
  "protocolVersion": "1.0",
  "packVersion": "1.0"
}
```

Existing `action`, `readback`, `assertion`, `evidence`, and normalized evidence
fields remain unchanged. Each adapter-level evidence entry explicitly retains
its `HcEvidence.kind`: object payloads receive a sibling `kind` field, while a
non-object payload is represented as `{ "kind": ..., "raw": ... }`. V3
`EvidenceRecord` fields such as `payload`, `bytes`, `redacted`, and `truncated`
remain alongside that kind. Raw transport details may vary between backends;
the semantic result contract should not.

## Fake backend testing

Use an empty, instance-owned registry and register a fake constructor with the
same four-field identity contract as a real backend. A useful integration test
should run the same generic HC YAML through two distinct fake identities and
assert:

- lifecycle order is preflight, open, command execution, and close;
- every `HcCommand` and `HcExecutionScope` is semantic and stable;
- discovery returns normalized entities;
- get-state reads the fake entity map;
- control mutates the map and returns action/readback/assertion data;
- wait succeeds on matching readback and fails with a mismatch category when it
  does not match;
- both generated `run.json` and `test-results.json` preserve adapter status,
  category, message, evidence tier, backend identity, raw evidence, and
  normalized evidence.

## Scope boundaries

Adding an HC backend requires no parser grammar change and no runner/dispatch
change. The YAML command grammar continues to produce the existing eight E2E
gateway commands; the gateway facade owns compatibility conversion to
`HcCommand`.

This boundary does not implement HCG1, MQTT, TLS, or production mappings for
other protocols. Those are separate future protocol-pack/backend work.
