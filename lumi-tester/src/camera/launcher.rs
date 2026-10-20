use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CameraLauncherConfig {
    pub root: PathBuf,
    pub rtsp: Option<String>,
    pub profile: Option<PathBuf>,
    pub test_yaml: Option<PathBuf>,
    pub env_file: Option<PathBuf>,
}

impl CameraLauncherConfig {
    pub fn discover(root: &Path) -> Result<Self> {
        let root = root.to_path_buf();
        let env_file = find_env_file(&root);
        let rtsp = env_file.as_deref().and_then(read_camera_rtsp);
        let profile = find_first_matching_file(&root, |path| {
            path.extension().is_some_and(|ext| ext == "json")
                && path
                    .components()
                    .any(|component| component.as_os_str() == "profiles")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("camera"))
        })?;
        let test_yaml = find_first_matching_file(&root, |path| {
            path.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("camera"))
        })?;

        Ok(Self {
            root,
            rtsp,
            profile,
            test_yaml,
            env_file,
        })
    }

    pub fn require_rtsp(&self, action: &str) -> Result<String> {
        self.rtsp.clone().with_context(|| {
            format!(
                "Missing CAMERA_RTSP for camera {action}. Add CAMERA_RTSP=rtsp://... to .env, or run: lumi-tester camera {action} --rtsp rtsp://..."
            )
        })
    }

    pub fn require_profile(&self, action: &str) -> Result<PathBuf> {
        self.profile.clone().with_context(|| {
            format!(
                "Missing camera profile for camera {action}. Create one with: lumi-tester camera profile --profile e2e/workspaces/lumi_life/profiles/lab_switch4_camera.json"
            )
        })
    }

    pub fn require_test_yaml(&self) -> Result<PathBuf> {
        self.test_yaml.clone().with_context(|| {
            "Missing camera YAML test. Expected a file like e2e/workspaces/lumi_life/camera_hardware_sample.yaml".to_string()
        })
    }

    pub fn render_doctor_summary(&self) -> String {
        let rtsp = if self.rtsp.is_some() {
            "found"
        } else {
            "missing"
        };
        let profile = self
            .profile
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "missing".to_string());
        let test_yaml = self
            .test_yaml
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "missing".to_string());

        format!(
            "Camera setup\n  CAMERA_RTSP: {rtsp}\n  camera profile: {profile}\n  camera YAML: {test_yaml}\n\nShort commands:\n  lumi-tester camera profile\n  lumi-tester camera observe\n  lumi-tester camera check\n  lumi-tester camera test\n"
        )
    }
}

fn find_env_file(root: &Path) -> Option<PathBuf> {
    [root.join(".env"), root.join("..").join(".env")]
        .into_iter()
        .find(|path| path.exists())
}

fn read_camera_rtsp(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let line = line.trim();
        let value = line.strip_prefix("CAMERA_RTSP=")?;
        Some(value.trim_matches('"').trim_matches('\'').to_string())
    })
}

fn find_first_matching_file<F>(root: &Path, matches: F) -> Result<Option<PathBuf>>
where
    F: Fn(&Path) -> bool,
{
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        if path.components().any(|part| part.as_os_str() == "target") {
            continue;
        }
        if matches(path) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files.into_iter().next())
}

pub fn camera_failure_hint(error: &str) -> Option<String> {
    if !error.contains("camera evidence") && !error.contains("device check failed") {
        return None;
    }

    let region = error
        .split("button '")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .unwrap_or("the failed region");

    let actual = error
        .split(" is '")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .unwrap_or("UNKNOWN");

    Some(format!(
        "Camera next steps:\n  {region} is {actual}. Open the profile editor and re-check detection:\n    lumi-tester camera profile\n    lumi-tester camera check\n  If the profile is correct, confirm the physical device LED state matches the YAML expectation."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumi-camera-launcher-{}-{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn discovers_env_profile_and_camera_yaml_from_workspace() {
        let root = temp_root("workspace");
        fs::write(
            root.join(".env"),
            "CAMERA_RTSP=rtsp://user:pass@10.0.0.5/live\n",
        )
        .unwrap();
        let workspace = root.join("e2e/workspaces/lumi_life");
        fs::create_dir_all(workspace.join("profiles")).unwrap();
        fs::write(workspace.join("profiles/lab_switch4_camera.json"), "{}").unwrap();
        fs::write(
            workspace.join("camera_hardware_sample.yaml"),
            "platform: android\ncamera:\n  profile: profiles/lab_switch4_camera.json\n---\n- getDeviceState:\n    saveAs: state\n",
        )
        .unwrap();

        let discovered = CameraLauncherConfig::discover(&root).unwrap();

        assert_eq!(
            discovered.rtsp.as_deref(),
            Some("rtsp://user:pass@10.0.0.5/live")
        );
        assert_eq!(
            discovered.profile.unwrap(),
            workspace.join("profiles/lab_switch4_camera.json")
        );
        assert_eq!(
            discovered.test_yaml.unwrap(),
            workspace.join("camera_hardware_sample.yaml")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reports_missing_rtsp_with_actionable_message() {
        let root = temp_root("missing-rtsp");
        let workspace = root.join("e2e/workspaces/lumi_life");
        fs::create_dir_all(workspace.join("profiles")).unwrap();
        fs::write(workspace.join("profiles/lab_switch4_camera.json"), "{}").unwrap();

        let discovered = CameraLauncherConfig::discover(&root).unwrap();
        let error = discovered.require_rtsp("profile").unwrap_err().to_string();

        assert!(error.contains("CAMERA_RTSP"));
        assert!(error.contains(".env"));
        assert!(error.contains("lumi-tester camera profile --rtsp"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn renders_doctor_summary_with_short_commands() {
        let root = temp_root("doctor");
        let workspace = root.join("e2e/workspaces/lumi_life");
        fs::create_dir_all(workspace.join("profiles")).unwrap();
        fs::write(root.join(".env"), "CAMERA_RTSP=rtsp://10.0.0.5/live\n").unwrap();
        fs::write(workspace.join("profiles/lab_switch4_camera.json"), "{}").unwrap();
        fs::write(
            workspace.join("camera_lab_read_state.yaml"),
            "platform: android\n---\n- launchApp\n",
        )
        .unwrap();

        let discovered = CameraLauncherConfig::discover(&root).unwrap();
        let summary = discovered.render_doctor_summary();

        assert!(summary.contains("camera profile"));
        assert!(summary.contains("camera observe"));
        assert!(summary.contains("camera check"));
        assert!(summary.contains("camera test"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn builds_hint_for_unknown_camera_state_failure() {
        let error = "device check failed: button 'device_2.button_1' is 'UNKNOWN', expected 'BLUE'\ncamera evidence: output/run/camera_evidence/default_123\nstate timeline: output/run/camera_evidence/default_123/timeline.json";

        let hint = camera_failure_hint(error).unwrap();

        assert!(hint.contains("device_2.button_1"));
        assert!(hint.contains("UNKNOWN"));
        assert!(hint.contains("lumi-tester camera profile"));
        assert!(hint.contains("lumi-tester camera check"));
    }
}
