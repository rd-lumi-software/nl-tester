use super::types::{
    ComponentSource, ComponentSpec, Diagnostic, LegacyMetadata, NormalizedFlow, Platform, TargetId,
    TestFlow,
};
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NormalizationError {
    #[error("legacy platform/appId conflicts with canonical app component")]
    ConflictingLegacyAppDefinition,
}

impl NormalizationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ConflictingLegacyAppDefinition => "ConflictingLegacyAppDefinition",
        }
    }
}

pub fn normalize_flow(flow: TestFlow, source_path: &Path) -> Result<NormalizedFlow> {
    let mut components = BTreeMap::new();
    let mut diagnostics = flow
        .journey
        .as_ref()
        .map(|_| {
            vec![Diagnostic {
                code: "DeprecatedJourney".to_string(),
                severity: "warning".to_string(),
                message: "journey is deprecated metadata and does not select an adapter"
                    .to_string(),
                source_path: source_path.to_path_buf(),
            }]
        })
        .unwrap_or_default();

    if let (Some(app), Some(platform)) = (flow.app.as_ref(), flow.platform.as_ref()) {
        if !matches!(platform, Platform::LumiHc | Platform::LumiCloud)
            && (app.platform != platform_name(platform)
                || flow
                    .app_id
                    .as_ref()
                    .is_some_and(|legacy_app_id| app.app_id.as_ref() != Some(legacy_app_id)))
        {
            return Err(NormalizationError::ConflictingLegacyAppDefinition.into());
        }
    }

    if let Some(app) = flow.app.as_ref() {
        components.insert(
            TargetId::App,
            ComponentSpec {
                target: TargetId::App,
                platform: app.platform.clone(),
                app_id: app.app_id.clone(),
                profile: app
                    .profile
                    .as_ref()
                    .map(|path| resolve_profile(source_path, path)),
                capabilities: app.capabilities.clone(),
                source: ComponentSource::Canonical,
            },
        );
    }

    if let Some(gateway) = flow.gateway.as_ref() {
        components.insert(
            TargetId::Gateway,
            component_from_declaration(TargetId::Gateway, gateway, source_path),
        );
    }

    if let Some(cloud) = flow.cloud.as_ref() {
        components.insert(
            TargetId::Cloud,
            component_from_declaration(TargetId::Cloud, cloud, source_path),
        );
    }

    if let Some(platform) = flow.platform.as_ref() {
        if matches!(
            platform,
            Platform::Android
                | Platform::AndroidAuto
                | Platform::Ios
                | Platform::Web
                | Platform::Macos
                | Platform::Windows
        ) && !components.contains_key(&TargetId::App)
        {
            components.insert(
                TargetId::App,
                ComponentSpec {
                    target: TargetId::App,
                    platform: platform_name(platform).to_string(),
                    app_id: flow.app_id.clone(),
                    profile: None,
                    capabilities: Vec::new(),
                    source: ComponentSource::LegacyApp,
                },
            );
        } else if platform == &Platform::LumiHc
            && !components.contains_key(&TargetId::Gateway)
        {
            components.insert(
                TargetId::Gateway,
                ComponentSpec {
                    target: TargetId::Gateway,
                    platform: "lumi_hc".to_string(),
                    app_id: None,
                    profile: flow
                        .profile
                        .as_ref()
                        .map(|profile| resolve_profile(source_path, profile)),
                    capabilities: Vec::new(),
                    source: ComponentSource::LegacyGateway,
                },
            );
            if flow.profile.is_some() {
                diagnostics.push(Diagnostic {
                    code: "DeprecatedLegacyProfile".to_string(),
                    severity: "warning".to_string(),
                    message: "top-level profile is deprecated; use gateway.profile".to_string(),
                    source_path: source_path.to_path_buf(),
                });
            }
        } else if platform == &Platform::LumiCloud && !components.contains_key(&TargetId::Cloud) {
            components.insert(
                TargetId::Cloud,
                ComponentSpec {
                    target: TargetId::Cloud,
                    platform: "lumi_cloud".to_string(),
                    app_id: None,
                    profile: flow
                        .profile
                        .as_ref()
                        .map(|profile| resolve_profile(source_path, profile)),
                    capabilities: Vec::new(),
                    source: ComponentSource::Canonical,
                },
            );
        }
    }

    Ok(NormalizedFlow {
        components,
        commands: flow.commands,
        legacy_metadata: LegacyMetadata {
            journey: flow.journey,
            legacy_platform: flow
                .platform
                .as_ref()
                .map(platform_name)
                .map(str::to_string),
            legacy_profile: flow.profile.map(Into::into),
            diagnostics,
        },
        source_path: source_path.to_path_buf(),
    })
}

fn resolve_profile(source_path: &Path, profile: &str) -> std::path::PathBuf {
    source_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(profile)
}

fn component_from_declaration(
    target: TargetId,
    declaration: &super::types::ComponentDeclaration,
    source_path: &Path,
) -> ComponentSpec {
    ComponentSpec {
        target,
        platform: declaration.platform.clone(),
        app_id: declaration.app_id.clone(),
        profile: declaration
            .profile
            .as_ref()
            .map(|profile| resolve_profile(source_path, profile)),
        capabilities: declaration.capabilities.clone(),
        source: ComponentSource::Canonical,
    }
}

pub fn platform_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Android => "android",
        Platform::AndroidAuto => "android_auto",
        Platform::Ios => "ios",
        Platform::Web => "web",
        Platform::Macos => "macos",
        Platform::Windows => "windows",
        Platform::LumiHc => "lumi_hc",
        Platform::LumiCloud => "lumi_cloud",
    }
}
