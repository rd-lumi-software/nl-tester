use lumi_tester::hc::{
    HcBackendConfig, HcBackendDescriptor, HcBackendFactoryError, HcBackendRegistry,
    HcBackendResult, HcCapability, HcCommand, HcEntity, HcEvidence, HcEvidenceTier,
    HcExecutionScope, HcObservation, HcProtocolBackend, HcState, HcStateUpdate, HcStatus,
};
use lumi_tester::runner::context::TestContext;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const V3_IMPLEMENTATION: &str = "lumi_iot_v3_home_controller";
const V3_PROTOCOL_PACK: &str = "lumi_iot_v3_home_controller";
const V3_PROTOCOL_VERSION: &str = "0.2.0";
const V3_PACK_VERSION: &str = "0.1.0";

fn config(
    implementation: &str,
    protocol_pack: &str,
    protocol_version: &str,
    pack_version: &str,
) -> HcBackendConfig {
    HcBackendConfig::new(
        implementation,
        protocol_pack,
        protocol_version,
        pack_version,
        json!({"endpoint": "opaque-to-hc"}),
    )
}

#[test]
fn generic_hc_profile_loader_extracts_identity_without_transport_validation() {
    let dir = std::env::temp_dir().join(format!(
        "lumi-hc-profile-envelope-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("profiles")).unwrap();
    let flow_path = dir.join("flow.yaml");
    fs::write(
        dir.join("profiles/fake.yaml"),
        "implementation: fake-home-controller\nprotocolPack: fake-pack\nprotocolVersion: 1.0\npackVersion: 2.0\nendpoint: definitely-not-a-url\nauth: {mode: invalid}\nretry: {controlMaxAttempts: 99}\n",
    )
    .unwrap();

    let config = lumi_tester::hc::profile::load_profile(&flow_path, "profiles/fake.yaml").unwrap();

    assert_eq!(config.implementation, "fake-home-controller");
    assert_eq!(config.protocol_pack, "fake-pack");
    assert_eq!(config.protocol_version, "1.0");
    assert_eq!(config.pack_version, "2.0");
    assert_eq!(config.config["profile_ref"], "profiles/fake.yaml");
    assert_eq!(
        config.config["source_path"],
        flow_path.to_string_lossy().to_string()
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn protocol_neutral_commands_preserve_all_eight_exact_shapes() {
    let indexed = HcStateUpdate::Indexed {
        idx: u64::MAX,
        value: json!(true),
    };
    let capability = HcStateUpdate::Capability {
        name: "power".to_string(),
        value: json!("on"),
    };

    let commands = vec![
        HcCommand::Health,
        HcCommand::GetEntities,
        HcCommand::GetState {
            entity: "entity-42".to_string(),
        },
        HcCommand::Control {
            entity: "entity-42".to_string(),
            updates: vec![indexed.clone(), capability.clone()],
        },
        HcCommand::WaitState {
            entity: "entity-42".to_string(),
            expected: vec![indexed.clone()],
        },
        HcCommand::RealtimeSubscribe {
            channels: vec!["cell_runtime_data".to_string()],
        },
        HcCommand::RealtimeExpect {
            entity: "entity-42".to_string(),
            event: json!({"state": [{"idx": 0, "value": true}]}),
        },
        HcCommand::RealtimeSnapshot {
            entity: "entity-42".to_string(),
        },
    ];

    assert!(matches!(commands[0], HcCommand::Health));
    assert!(matches!(commands[1], HcCommand::GetEntities));
    assert!(matches!(
        &commands[2],
        HcCommand::GetState { entity } if entity == "entity-42"
    ));
    assert!(matches!(
        &commands[3],
        HcCommand::Control { entity, updates }
            if entity == "entity-42" && updates == &[indexed.clone(), capability.clone()]
    ));
    assert!(matches!(
        &commands[4],
        HcCommand::WaitState { entity, expected }
            if entity == "entity-42" && expected == &[indexed]
    ));
    assert!(matches!(
        &commands[5],
        HcCommand::RealtimeSubscribe { channels }
            if channels == &["cell_runtime_data".to_string()]
    ));
    assert!(matches!(
        &commands[6],
        HcCommand::RealtimeExpect { entity, event }
            if entity == "entity-42" && event == &json!({"state": [{"idx": 0, "value": true}]})
    ));
    assert!(matches!(
        &commands[7],
        HcCommand::RealtimeSnapshot { entity } if entity == "entity-42"
    ));
    assert_eq!(commands[5].display_name(), "wsSubscribe");
    assert_eq!(commands[6].display_name(), "wsExpect");
    assert_eq!(commands[7].display_name(), "wsSnapshot");
}

#[test]
fn backend_result_preserves_action_readback_assertion_and_evidence_contract() {
    let raw = json!({"status": 200, "payload": {"state": true}});
    let normalized = json!({"entity": "entity-42", "state": true});
    let result = HcBackendResult::passed("control completed")
        .with_action(json!({"operation": "control"}))
        .with_readback(json!({"state": true}))
        .with_assertion(json!({"matched": true}))
        .with_evidence(HcEvidence::raw("http_response", raw.clone()))
        .with_normalized(normalized.clone())
        .with_evidence_tier(HcEvidenceTier::RealE2e)
        .with_readback_match(true);

    assert_eq!(result.status, HcStatus::Passed);
    assert_eq!(result.action, Some(json!({"operation": "control"})));
    assert_eq!(result.readback, Some(json!({"state": true})));
    assert_eq!(result.assertion, Some(json!({"matched": true})));
    assert_eq!(result.evidence[0].kind, "http_response");
    assert_eq!(result.evidence[0].raw, raw);
    assert_eq!(result.normalized, Some(normalized));
    assert_eq!(result.evidence_tier, HcEvidenceTier::RealE2e);
    assert_eq!(result.readback_match, Some(true));

    let failed = HcBackendResult::failed("assertion_mismatch", "state did not match");
    let blocked = HcBackendResult::blocked("no_lab", "HC is unavailable");
    let error = HcBackendResult::error("protocol_error", "malformed response");
    assert_eq!(failed.status, HcStatus::Failed);
    assert_eq!(failed.category, "assertion_mismatch");
    assert_eq!(blocked.status, HcStatus::Blocked);
    assert_eq!(blocked.category, "no_lab");
    assert_eq!(error.status, HcStatus::Error);
    assert_eq!(error.category, "protocol_error");
}

#[test]
fn observation_represents_discovery_entities_and_state_semantics() {
    let entity = HcEntity {
        id: "entity-42".to_string(),
        name: Some("Kitchen light".to_string()),
        capabilities: vec![HcCapability::new("power", json!(true))],
    };
    let observation = HcObservation::discovery(vec![entity.clone()]).with_state(HcState {
        entity: "entity-42".to_string(),
        updates: vec![HcStateUpdate::Indexed {
            idx: u64::MAX,
            value: json!(true),
        }],
    });

    assert_eq!(observation.entities, Some(vec![entity]));
    assert_eq!(observation.state.unwrap().entity, "entity-42");
}

#[derive(Debug, Clone, PartialEq)]
enum FakeCall {
    Lifecycle(&'static str),
    Execute {
        command: HcCommand,
        scope: HcExecutionScope,
    },
}

#[derive(Clone)]
struct FakeBackend {
    calls: Arc<Mutex<Vec<FakeCall>>>,
    capabilities: Vec<HcCapability>,
    config: HcBackendConfig,
}

impl FakeBackend {
    fn record(&self, call: FakeCall) {
        self.calls.lock().unwrap().push(call);
    }
}

impl HcProtocolBackend for FakeBackend {
    fn implementation_id(&self) -> &str {
        &self.config.implementation
    }

    fn protocol_pack(&self) -> &str {
        &self.config.protocol_pack
    }

    fn capabilities(&self) -> &[HcCapability] {
        &self.capabilities
    }

    fn preflight(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record(FakeCall::Lifecycle("preflight"));
        HcBackendResult::passed("fake preflight")
    }

    fn open(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record(FakeCall::Lifecycle("open"));
        HcBackendResult::passed("fake open")
    }

    fn execute(&mut self, command: &HcCommand, scope: &HcExecutionScope) -> HcBackendResult {
        self.record(FakeCall::Execute {
            command: command.clone(),
            scope: scope.clone(),
        });
        HcBackendResult::passed("fake execute")
    }

    fn close(&mut self, _context: &TestContext) -> HcBackendResult {
        self.record(FakeCall::Lifecycle("close"));
        HcBackendResult::passed("fake close")
    }
}

#[test]
fn fake_backend_receives_semantic_payloads_and_hc_scope() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let capabilities = vec![HcCapability::new("health", json!(true))];
    let descriptor = HcBackendDescriptor::new("fake-home-controller", "fake-pack", "1.0", "2.0");
    let backend_config = config("fake-home-controller", "fake-pack", "1.0", "2.0");
    let mut registry = HcBackendRegistry::empty();
    let constructor_calls = calls.clone();
    let constructor_capabilities = capabilities.clone();
    registry
        .register(descriptor, move |config| {
            Box::new(FakeBackend {
                calls: constructor_calls.clone(),
                capabilities: constructor_capabilities.clone(),
                config: config.clone(),
            })
        })
        .unwrap();

    let mut backend = registry.create(&backend_config).unwrap();
    assert_eq!(backend.implementation_id(), "fake-home-controller");
    assert_eq!(backend.protocol_pack(), "fake-pack");
    assert_eq!(backend.capabilities(), capabilities.as_slice());

    let context = TestContext::new(Path::new("."), None, false, None);
    let scope = HcExecutionScope::new(2, 7, "hc-correlation-7");
    assert_eq!(backend.preflight(&context).status, HcStatus::Passed);
    assert_eq!(backend.open(&context).status, HcStatus::Passed);
    let control = HcCommand::Control {
        entity: "entity-42".to_string(),
        updates: vec![
            HcStateUpdate::Indexed {
                idx: u64::MAX,
                value: json!(true),
            },
            HcStateUpdate::Capability {
                name: "power".to_string(),
                value: json!("on"),
            },
        ],
    };
    assert_eq!(backend.execute(&control, &scope).status, HcStatus::Passed);
    assert_eq!(backend.close(&context).status, HcStatus::Passed);

    let calls = calls.lock().unwrap();
    assert_eq!(calls[0], FakeCall::Lifecycle("preflight"));
    assert_eq!(calls[1], FakeCall::Lifecycle("open"));
    assert_eq!(
        calls[2],
        FakeCall::Execute {
            command: control,
            scope,
        }
    );
    assert_eq!(calls[3], FakeCall::Lifecycle("close"));
}

#[test]
fn backend_registry_matches_all_identity_versions_before_construction() {
    let registry = HcBackendRegistry::new();
    let v3_config = config(
        V3_IMPLEMENTATION,
        V3_PROTOCOL_PACK,
        V3_PROTOCOL_VERSION,
        V3_PACK_VERSION,
    );
    assert_eq!(
        registry.resolve(&v3_config).unwrap(),
        HcBackendDescriptor::v3()
    );
    assert!(registry.create(&v3_config).is_ok());

    let mut fake_registry = HcBackendRegistry::empty();
    fake_registry
        .register_descriptor(HcBackendDescriptor::new(
            "fake-home-controller",
            "fake-pack",
            "1.0",
            "2.0",
        ))
        .unwrap();
    let fake_config = config("fake-home-controller", "fake-pack", "1.0", "2.0");
    assert!(matches!(
        fake_registry.create(&fake_config),
        Err(HcBackendFactoryError::MissingConstructor { .. })
    ));
}

#[test]
fn backend_registry_resolves_the_exact_registered_version_tuple() {
    let mut registry = HcBackendRegistry::empty();
    registry
        .register_descriptor(HcBackendDescriptor::new(
            "versioned-home-controller",
            "versioned-pack",
            "1.0",
            "1.0",
        ))
        .unwrap();
    registry
        .register_descriptor(HcBackendDescriptor::new(
            "versioned-home-controller",
            "versioned-pack",
            "2.0",
            "2.0",
        ))
        .unwrap();

    let resolved = registry
        .resolve(&config(
            "versioned-home-controller",
            "versioned-pack",
            "2.0",
            "2.0",
        ))
        .unwrap();
    assert_eq!(resolved.protocol_version, "2.0");
    assert_eq!(resolved.pack_version, "2.0");
}

#[test]
fn backend_registry_rejects_typed_identity_and_registration_errors() {
    let registry = HcBackendRegistry::new();

    assert!(matches!(
        registry.resolve(&config(
            "",
            V3_PROTOCOL_PACK,
            V3_PROTOCOL_VERSION,
            V3_PACK_VERSION
        )),
        Err(HcBackendFactoryError::MissingImplementation)
    ));
    assert!(matches!(
        registry.resolve(&config(
            V3_IMPLEMENTATION,
            "",
            V3_PROTOCOL_VERSION,
            V3_PACK_VERSION
        )),
        Err(HcBackendFactoryError::MissingProtocolPack { .. })
    ));
    assert!(matches!(
        registry.resolve(&config(
            "unknown",
            V3_PROTOCOL_PACK,
            V3_PROTOCOL_VERSION,
            V3_PACK_VERSION
        )),
        Err(HcBackendFactoryError::UnsupportedImplementation { .. })
    ));
    assert!(matches!(
        registry.resolve(&config(
            V3_IMPLEMENTATION,
            "wrong-pack",
            V3_PROTOCOL_VERSION,
            V3_PACK_VERSION
        )),
        Err(HcBackendFactoryError::UnsupportedProtocolPack { .. })
    ));
    assert!(matches!(
        registry.resolve(&config(
            V3_IMPLEMENTATION,
            V3_PROTOCOL_PACK,
            "wrong-version",
            V3_PACK_VERSION
        )),
        Err(HcBackendFactoryError::UnsupportedProtocolVersion { .. })
    ));
    assert!(matches!(
        registry.resolve(&config(
            V3_IMPLEMENTATION,
            V3_PROTOCOL_PACK,
            V3_PROTOCOL_VERSION,
            "wrong-version"
        )),
        Err(HcBackendFactoryError::UnsupportedPackVersion { .. })
    ));
    assert!(matches!(
        registry.resolve(&config(
            V3_IMPLEMENTATION,
            V3_PROTOCOL_PACK,
            "",
            V3_PACK_VERSION
        )),
        Err(HcBackendFactoryError::MissingProtocolVersion { .. })
    ));
    assert!(matches!(
        registry.resolve(&config(
            V3_IMPLEMENTATION,
            V3_PROTOCOL_PACK,
            V3_PROTOCOL_VERSION,
            ""
        )),
        Err(HcBackendFactoryError::MissingPackVersion { .. })
    ));

    let mut empty = HcBackendRegistry::empty();
    assert!(matches!(
        empty.register_descriptor(HcBackendDescriptor::new("", "pack", "1.0", "1.0")),
        Err(HcBackendFactoryError::MalformedDescriptor {
            field: "implementation"
        })
    ));
    assert!(matches!(
        empty.register_descriptor(HcBackendDescriptor::new("impl", "", "1.0", "1.0")),
        Err(HcBackendFactoryError::MalformedDescriptor {
            field: "protocol_pack"
        })
    ));
    assert!(matches!(
        empty.register_descriptor(HcBackendDescriptor::new("impl", "pack", "", "1.0")),
        Err(HcBackendFactoryError::MalformedDescriptor {
            field: "protocol_version"
        })
    ));
    assert!(matches!(
        empty.register_descriptor(HcBackendDescriptor::new("impl", "pack", "1.0", "")),
        Err(HcBackendFactoryError::MalformedDescriptor {
            field: "pack_version"
        })
    ));

    let descriptor = HcBackendDescriptor::new("impl", "pack", "1.0", "1.0");
    empty.register_descriptor(descriptor.clone()).unwrap();
    assert!(matches!(
        empty.register_descriptor(descriptor),
        Err(HcBackendFactoryError::DuplicateDescriptor { .. })
    ));
}
