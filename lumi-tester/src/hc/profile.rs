//! Generic HC profile-envelope loading independent of any protocol backend.

use super::HcBackendConfig;
use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

/// Errors raised while loading the protocol-neutral portion of an HC profile.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HcProfileError {
    #[error("HC profile is required")]
    MissingProfile,
    #[error("HC profile path '{profile}' must be relative to the flow file")]
    InvalidProfilePath { profile: String },
    #[error("HC profile '{profile}' was not found relative to the flow")]
    ProfileNotFound { profile: String },
    #[error("HC profile '{profile}' could not be read: {reason}")]
    ProfileRead { profile: String, reason: String },
    #[error("HC profile '{profile}' is malformed: {reason}")]
    ProfileParse { profile: String, reason: String },
    #[error("HC profile field '{field}' is required")]
    MissingProfileField { field: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawHcProfile {
    implementation: Option<String>,
    protocol_pack: Option<String>,
    protocol_version: Option<String>,
    pack_version: Option<String>,
}

/// Load only the HC identity envelope and opaque flow/profile references.
///
/// Unknown profile fields are intentionally ignored. Endpoint, auth, retry,
/// evidence, and capability semantics belong to the selected backend.
pub fn load_profile(
    flow_path: &Path,
    profile_ref: &str,
) -> Result<HcBackendConfig, HcProfileError> {
    let profile_ref = profile_ref.trim();
    if profile_ref.is_empty() {
        return Err(HcProfileError::MissingProfile);
    }

    let profile_path = Path::new(profile_ref);
    if profile_path.is_absolute() || profile_ref.contains("${") {
        return Err(HcProfileError::InvalidProfilePath {
            profile: profile_ref.to_string(),
        });
    }

    let flow_dir = flow_path.parent().unwrap_or_else(|| Path::new("."));
    let resolved_path = flow_dir.join(profile_path);
    if !resolved_path.is_file() {
        return Err(HcProfileError::ProfileNotFound {
            profile: profile_ref.to_string(),
        });
    }

    let content =
        std::fs::read_to_string(&resolved_path).map_err(|error| HcProfileError::ProfileRead {
            profile: profile_ref.to_string(),
            reason: error.to_string(),
        })?;
    let raw: RawHcProfile =
        serde_yaml::from_str(&content).map_err(|error| HcProfileError::ProfileParse {
            profile: profile_ref.to_string(),
            reason: error.to_string(),
        })?;

    Ok(HcBackendConfig::new(
        required(raw.implementation, "implementation")?,
        required(raw.protocol_pack, "protocolPack")?,
        required(raw.protocol_version, "protocolVersion")?,
        required(raw.pack_version, "packVersion")?,
        serde_json::json!({
            "source_path": flow_path.to_string_lossy(),
            "profile_ref": profile_ref,
        }),
    ))
}

fn required(value: Option<String>, field: &str) -> Result<String, HcProfileError> {
    let value = value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| HcProfileError::MissingProfileField {
            field: field.to_string(),
        })?;
    Ok(value.trim().to_string())
}
