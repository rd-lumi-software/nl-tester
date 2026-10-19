use super::backend::{HcBackendConfig, HcProtocolBackend};
use super::backends::v3::V3HcProtocolBackend;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

/// The production v3 implementation identity reserved by the existing HC profile.
pub const V3_IMPLEMENTATION_ID: &str = "lumi_iot_v3_home_controller";
/// The production v3 protocol-pack identity reserved by the existing HC profile.
pub const V3_PROTOCOL_PACK_ID: &str = "lumi_iot_v3_home_controller";
/// The production v3 protocol version reserved by the existing HC profile.
pub const V3_PROTOCOL_VERSION: &str = "0.2.0";
/// The production v3 pack version reserved by the existing HC profile.
pub const V3_PACK_VERSION: &str = "0.1.0";

/// An implementation, protocol-pack, and version identity tuple.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HcBackendDescriptor {
    pub implementation_id: String,
    pub protocol_pack: String,
    pub protocol_version: String,
    pub pack_version: String,
}

impl HcBackendDescriptor {
    /// Construct a complete backend identity descriptor.
    pub fn new(
        implementation_id: impl Into<String>,
        protocol_pack: impl Into<String>,
        protocol_version: impl Into<String>,
        pack_version: impl Into<String>,
    ) -> Self {
        Self {
            implementation_id: implementation_id.into(),
            protocol_pack: protocol_pack.into(),
            protocol_version: protocol_version.into(),
            pack_version: pack_version.into(),
        }
    }

    /// Return the existing v3 identity descriptor without constructing a backend.
    pub fn v3() -> Self {
        Self::new(
            V3_IMPLEMENTATION_ID,
            V3_PROTOCOL_PACK_ID,
            V3_PROTOCOL_VERSION,
            V3_PACK_VERSION,
        )
    }
}

/// Factory errors are stable and independent of any transport implementation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HcBackendFactoryError {
    #[error("HC backend implementation is required")]
    MissingImplementation,
    #[error("HC protocol pack is required for implementation '{implementation}'")]
    MissingProtocolPack { implementation: String },
    #[error("HC protocol version is required for implementation '{implementation}'")]
    MissingProtocolVersion { implementation: String },
    #[error("HC pack version is required for implementation '{implementation}'")]
    MissingPackVersion { implementation: String },
    #[error("unsupported HC backend implementation '{implementation}'")]
    UnsupportedImplementation { implementation: String },
    #[error(
        "unsupported HC protocol pack '{protocol_pack}' for implementation '{implementation}'"
    )]
    UnsupportedProtocolPack {
        implementation: String,
        protocol_pack: String,
    },
    #[error(
        "unsupported HC protocol version '{actual}' for implementation '{implementation}' and pack '{protocol_pack}' (expected '{expected}')"
    )]
    UnsupportedProtocolVersion {
        implementation: String,
        protocol_pack: String,
        expected: String,
        actual: String,
    },
    #[error(
        "unsupported HC pack version '{actual}' for implementation '{implementation}' and pack '{protocol_pack}' (expected '{expected}')"
    )]
    UnsupportedPackVersion {
        implementation: String,
        protocol_pack: String,
        expected: String,
        actual: String,
    },
    #[error("HC backend descriptor field '{field}' must not be empty")]
    MalformedDescriptor { field: &'static str },
    #[error(
        "HC backend '{implementation}' with protocol pack '{protocol_pack}' has no constructor"
    )]
    MissingConstructor {
        implementation: String,
        protocol_pack: String,
        protocol_version: String,
        pack_version: String,
    },
    #[error(
        "HC backend '{implementation}' with protocol pack '{protocol_pack}' is already registered"
    )]
    DuplicateDescriptor {
        implementation: String,
        protocol_pack: String,
        protocol_version: String,
        pack_version: String,
    },
}

/// A constructor stored by one registry instance.
pub type HcBackendConstructor =
    Arc<dyn Fn(&HcBackendConfig) -> Box<dyn HcProtocolBackend> + Send + Sync>;

#[derive(Clone)]
struct Registration {
    descriptor: HcBackendDescriptor,
    constructor: Option<HcBackendConstructor>,
}

type HcBackendKey = (String, String, String, String);

/// Instance-owned HC backend registry and factory.
#[derive(Default, Clone)]
pub struct HcBackendRegistry {
    registrations: BTreeMap<HcBackendKey, Registration>,
}

impl HcBackendRegistry {
    /// Create a registry containing the selectable v3 descriptor.
    pub fn new() -> Self {
        let mut registry = Self::empty();
        registry
            .register(HcBackendDescriptor::v3(), |config| {
                Box::new(V3HcProtocolBackend::new(config))
            })
            .expect("the built-in v3 descriptor is valid");
        registry
    }

    /// Create a registry with no registrations, useful for isolated fake backends.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Register a constructible backend on this registry instance.
    pub fn register<F>(
        &mut self,
        descriptor: HcBackendDescriptor,
        constructor: F,
    ) -> Result<(), HcBackendFactoryError>
    where
        F: Fn(&HcBackendConfig) -> Box<dyn HcProtocolBackend> + Send + Sync + 'static,
    {
        self.register_constructor(descriptor, Arc::new(constructor))
    }

    /// Register a descriptor without a constructor, for a backend implemented later.
    pub fn register_descriptor(
        &mut self,
        descriptor: HcBackendDescriptor,
    ) -> Result<(), HcBackendFactoryError> {
        self.register_inner(descriptor, None)
    }

    /// Register a backend constructor explicitly on this registry instance.
    pub fn register_constructor(
        &mut self,
        descriptor: HcBackendDescriptor,
        constructor: HcBackendConstructor,
    ) -> Result<(), HcBackendFactoryError> {
        self.register_inner(descriptor, Some(constructor))
    }

    /// Resolve all implementation, pack, and version identities.
    pub fn resolve(
        &self,
        config: &HcBackendConfig,
    ) -> Result<HcBackendDescriptor, HcBackendFactoryError> {
        validate_config(config)?;

        if !self
            .registrations
            .values()
            .any(|registration| registration.descriptor.implementation_id == config.implementation)
        {
            return Err(HcBackendFactoryError::UnsupportedImplementation {
                implementation: config.implementation.clone(),
            });
        }

        let matching_pack = self
            .registrations
            .values()
            .filter(|registration| {
                registration.descriptor.implementation_id == config.implementation
                    && registration.descriptor.protocol_pack == config.protocol_pack
            })
            .collect::<Vec<_>>();
        if matching_pack.is_empty() {
            return Err(HcBackendFactoryError::UnsupportedProtocolPack {
                implementation: config.implementation.clone(),
                protocol_pack: config.protocol_pack.clone(),
            });
        }

        let key = config_key(config);
        if let Some(registration) = self.registrations.get(&key) {
            return Ok(registration.descriptor.clone());
        }

        if let Some(registration) = matching_pack.iter().find(|registration| {
            registration.descriptor.protocol_version == config.protocol_version
        }) {
            return Err(HcBackendFactoryError::UnsupportedPackVersion {
                implementation: config.implementation.clone(),
                protocol_pack: config.protocol_pack.clone(),
                expected: registration.descriptor.pack_version.clone(),
                actual: config.pack_version.clone(),
            });
        }

        if let Some(registration) = matching_pack
            .iter()
            .find(|registration| registration.descriptor.pack_version == config.pack_version)
        {
            return Err(HcBackendFactoryError::UnsupportedProtocolVersion {
                implementation: config.implementation.clone(),
                protocol_pack: config.protocol_pack.clone(),
                expected: registration.descriptor.protocol_version.clone(),
                actual: config.protocol_version.clone(),
            });
        }

        let expected = matching_pack[0];
        Err(HcBackendFactoryError::UnsupportedProtocolVersion {
            implementation: config.implementation.clone(),
            protocol_pack: config.protocol_pack.clone(),
            expected: expected.descriptor.protocol_version.clone(),
            actual: config.protocol_version.clone(),
        })
    }

    /// Resolve and construct a registered backend instance with its config.
    pub fn create(
        &self,
        config: &HcBackendConfig,
    ) -> Result<Box<dyn HcProtocolBackend>, HcBackendFactoryError> {
        let descriptor = self.resolve(config)?;
        let registration = self
            .registrations
            .get(&descriptor_key(&descriptor))
            .expect("resolved HC descriptor must remain registered");
        registration
            .constructor
            .as_ref()
            .map(|constructor| constructor(config))
            .ok_or_else(|| HcBackendFactoryError::MissingConstructor {
                implementation: descriptor.implementation_id,
                protocol_pack: descriptor.protocol_pack,
                protocol_version: descriptor.protocol_version,
                pack_version: descriptor.pack_version,
            })
    }

    fn register_inner(
        &mut self,
        descriptor: HcBackendDescriptor,
        constructor: Option<HcBackendConstructor>,
    ) -> Result<(), HcBackendFactoryError> {
        validate_descriptor(&descriptor)?;
        let key = descriptor_key(&descriptor);
        if self.registrations.contains_key(&key) {
            return Err(HcBackendFactoryError::DuplicateDescriptor {
                implementation: descriptor.implementation_id,
                protocol_pack: descriptor.protocol_pack,
                protocol_version: descriptor.protocol_version,
                pack_version: descriptor.pack_version,
            });
        }
        self.registrations.insert(
            key,
            Registration {
                descriptor,
                constructor,
            },
        );
        Ok(())
    }
}

/// Alias emphasizing that the registry is also the backend factory.
pub type HcBackendFactory = HcBackendRegistry;

fn descriptor_key(descriptor: &HcBackendDescriptor) -> HcBackendKey {
    (
        descriptor.implementation_id.clone(),
        descriptor.protocol_pack.clone(),
        descriptor.protocol_version.clone(),
        descriptor.pack_version.clone(),
    )
}

fn config_key(config: &HcBackendConfig) -> HcBackendKey {
    (
        config.implementation.clone(),
        config.protocol_pack.clone(),
        config.protocol_version.clone(),
        config.pack_version.clone(),
    )
}

fn validate_config(config: &HcBackendConfig) -> Result<(), HcBackendFactoryError> {
    if config.implementation.trim().is_empty() {
        return Err(HcBackendFactoryError::MissingImplementation);
    }
    if config.protocol_pack.trim().is_empty() {
        return Err(HcBackendFactoryError::MissingProtocolPack {
            implementation: config.implementation.clone(),
        });
    }
    if config.protocol_version.trim().is_empty() {
        return Err(HcBackendFactoryError::MissingProtocolVersion {
            implementation: config.implementation.clone(),
        });
    }
    if config.pack_version.trim().is_empty() {
        return Err(HcBackendFactoryError::MissingPackVersion {
            implementation: config.implementation.clone(),
        });
    }
    Ok(())
}

fn validate_descriptor(descriptor: &HcBackendDescriptor) -> Result<(), HcBackendFactoryError> {
    if descriptor.implementation_id.trim().is_empty() {
        return Err(HcBackendFactoryError::MalformedDescriptor {
            field: "implementation",
        });
    }
    if descriptor.protocol_pack.trim().is_empty() {
        return Err(HcBackendFactoryError::MalformedDescriptor {
            field: "protocol_pack",
        });
    }
    if descriptor.protocol_version.trim().is_empty() {
        return Err(HcBackendFactoryError::MalformedDescriptor {
            field: "protocol_version",
        });
    }
    if descriptor.pack_version.trim().is_empty() {
        return Err(HcBackendFactoryError::MalformedDescriptor {
            field: "pack_version",
        });
    }
    Ok(())
}
