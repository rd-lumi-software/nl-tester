use serde::{Deserialize, Serialize};
use serde_json;
use serde_yaml;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TargetId {
    App,
    Gateway,
    Cloud,
}

impl TargetId {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "app" => Some(Self::App),
            "gateway" => Some(Self::Gateway),
            "cloud" => Some(Self::Cloud),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Gateway => "gateway",
            Self::Cloud => "cloud",
        }
    }
}

impl std::fmt::Display for TargetId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentSource {
    LegacyApp,
    LegacyGateway,
    Canonical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSpec {
    pub target: TargetId,
    pub platform: String,
    pub app_id: Option<String>,
    pub profile: Option<PathBuf>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub source: ComponentSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMetadata {
    pub journey: Option<String>,
    pub legacy_platform: Option<String>,
    pub legacy_profile: Option<PathBuf>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedFlow {
    pub components: BTreeMap<TargetId, ComponentSpec>,
    pub commands: Vec<TestCommand>,
    pub legacy_metadata: LegacyMetadata,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDeclaration {
    pub platform: String,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Represents a parsed test flow from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestFlow {
    #[serde(default)]
    pub app: Option<ComponentDeclaration>,

    #[serde(default)]
    pub gateway: Option<ComponentDeclaration>,

    #[serde(default)]
    pub cloud: Option<ComponentDeclaration>,

    #[serde(default)]
    pub app_id: Option<String>,

    #[serde(default)]
    pub url: Option<String>,

    #[serde(default)]
    pub platform: Option<Platform>,

    /// Semantic journey for non-App domains such as lumi_e2e.
    #[serde(default)]
    pub journey: Option<String>,

    /// Profile path resolved relative to the flow file for semantic domains.
    #[serde(default)]
    pub profile: Option<String>,

    #[serde(default)]
    pub env: Option<HashMap<String, String>>,

    #[serde(default)]
    pub vars: Option<HashMap<String, String>>,

    #[serde(default)]
    pub data: Option<String>,

    #[serde(default, alias = "defaultTimeout")]
    pub default_timeout_ms: Option<u64>,

    #[serde(default)]
    pub commands: Vec<TestCommand>,

    #[serde(default)]
    pub tags: Vec<String>,

    /// Speed profile for this flow: "turbo", "fast", "normal", "safe"
    #[serde(default)]
    pub speed: Option<String>,

    /// Web browser type: "Chrome", "Firefox", "Webkit"
    #[serde(default)]
    pub browser: Option<String>,

    /// Whether to close browser when test finishes (default: true)
    #[serde(default)]
    pub close_when_finish: Option<bool>,

    /// Desktop app state clearing configuration for macOS and Windows.
    #[serde(default)]
    pub desktop_state: Option<DesktopState>,

    /// Camera(s) for hardware verification. Accepts a single camera or a map of
    /// named cameras (for devices at different positions). The `camera:` keyword
    /// is accepted as an alias for the single-camera form.
    #[serde(default, alias = "camera")]
    pub cameras: Option<CamerasConfig>,

    /// Window size for desktop or browser (e.g. `windowSize: { width: 1280, height: 800 }` or `window: "1280x800"`)
    #[serde(default, alias = "window", alias = "windowSize")]
    pub window_size: Option<WindowSizeConfig>,

    /// Global Hardware Jig configuration (e.g. `jig: "jig_profile.yaml"` or `hardware: "jig_profile.yaml"`).
    /// Automatically connects before test flow runs, and automatically disconnects when test finishes.
    #[serde(default, alias = "hardware")]
    pub jig: Option<JigConfig>,

    /// Skip or ignore this test flow when running directories in bulk (e.g. `skip: true`, `manual: true`)
    #[serde(default, alias = "disabled", alias = "manual", alias = "ignore")]
    pub skip: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum WindowSizeConfig {
    String(String),
    Struct { width: u32, height: u32 },
}

impl WindowSizeConfig {
    pub fn to_dimensions(&self) -> Option<(u32, u32)> {
        match self {
            WindowSizeConfig::Struct { width, height } => Some((*width, *height)),
            WindowSizeConfig::String(s) => {
                let parts: Vec<&str> = if s.contains('x') || s.contains('X') {
                    s.split(|c| c == 'x' || c == 'X').collect()
                } else {
                    s.split(',').collect()
                };
                if parts.len() == 2 {
                    if let (Ok(w), Ok(h)) = (
                        parts[0].trim().parse::<u32>(),
                        parts[1].trim().parse::<u32>(),
                    ) {
                        return Some((w, h));
                    }
                }
                None
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JigConfig {
    PortOrProfile(String),
    Struct(HardwareConnectParams),
}

impl JigConfig {
    pub fn resolve(
        &self,
        base_dir: Option<&std::path::Path>,
    ) -> Result<HardwareConnectParams, String> {
        match self {
            JigConfig::PortOrProfile(path_or_port) => {
                let is_file_ext = path_or_port.ends_with(".yaml")
                    || path_or_port.ends_with(".yml")
                    || path_or_port.ends_with(".json");

                let candidate_path = if let Some(base) = base_dir {
                    base.join(path_or_port)
                } else {
                    std::path::PathBuf::from(path_or_port)
                };

                if is_file_ext || candidate_path.exists() {
                    let content = std::fs::read_to_string(&candidate_path).map_err(|e| {
                        format!(
                            "Failed to read jig profile file '{}': {}",
                            candidate_path.display(),
                            e
                        )
                    })?;
                    let raw_val: serde_yaml::Value =
                        serde_yaml::from_str(&content).map_err(|e| {
                            format!(
                                "Failed to parse jig profile file '{}': {}",
                                candidate_path.display(),
                                e
                            )
                        })?;
                    let mut params: HardwareConnectParams =
                        if let Some(hw) = raw_val.get("hardware") {
                            let mut p: HardwareConnectParams =
                                serde_yaml::from_value(hw.clone()).unwrap_or_default();
                            if p.servos.is_none() {
                                if let Some(servos_val) =
                                    hw.get("servos").or_else(|| raw_val.get("servos"))
                                {
                                    p.servos = serde_yaml::from_value(servos_val.clone()).ok();
                                }
                            }
                            p
                        } else {
                            serde_yaml::from_value(raw_val.clone()).map_err(|e| {
                                format!(
                                    "Failed to parse jig profile file '{}': {}",
                                    candidate_path.display(),
                                    e
                                )
                            })?
                        };
                    if let Some(ref nested_file) = params.file {
                        let profile_dir =
                            candidate_path.parent().unwrap_or(std::path::Path::new("."));
                        let nested_path = profile_dir.join(nested_file);
                        let nested_content =
                            std::fs::read_to_string(&nested_path).map_err(|e| {
                                format!(
                                    "Failed to read nested jig file '{}': {}",
                                    nested_path.display(),
                                    e
                                )
                            })?;
                        let nested_raw: serde_yaml::Value = serde_yaml::from_str(&nested_content)
                            .map_err(|e| {
                            format!(
                                "Failed to parse nested jig file '{}': {}",
                                nested_path.display(),
                                e
                            )
                        })?;
                        let nested_params: HardwareConnectParams =
                            if let Some(hw) = nested_raw.get("hardware") {
                                let mut p: HardwareConnectParams =
                                    serde_yaml::from_value(hw.clone()).unwrap_or_default();
                                if p.servos.is_none() {
                                    if let Some(servos_val) =
                                        hw.get("servos").or_else(|| nested_raw.get("servos"))
                                    {
                                        p.servos = serde_yaml::from_value(servos_val.clone()).ok();
                                    }
                                }
                                if p.buttons.is_none() {
                                    if let Some(btns_val) =
                                        hw.get("buttons").or_else(|| nested_raw.get("buttons"))
                                    {
                                        p.buttons = serde_yaml::from_value(btns_val.clone()).ok();
                                    }
                                }
                                if p.relays.is_none() {
                                    if let Some(relays_val) =
                                        hw.get("relays").or_else(|| nested_raw.get("relays"))
                                    {
                                        p.relays = serde_yaml::from_value(relays_val.clone()).ok();
                                    }
                                }
                                p
                            } else {
                                serde_yaml::from_value(nested_raw).map_err(|e| {
                                    format!(
                                        "Failed to parse nested jig file '{}': {}",
                                        nested_path.display(),
                                        e
                                    )
                                })?
                            };
                        params = nested_params;
                    }
                    Ok(params)
                } else {
                    Ok(HardwareConnectParams {
                        port: path_or_port.clone(),
                        auto_power_off: Some(true),
                        ..Default::default()
                    })
                }
            }
            JigConfig::Struct(p) => {
                if let Some(ref file_path) = p.file {
                    let candidate_path = if let Some(base) = base_dir {
                        base.join(file_path)
                    } else {
                        std::path::PathBuf::from(file_path)
                    };
                    let content = std::fs::read_to_string(&candidate_path).map_err(|e| {
                        format!(
                            "Failed to read jig file '{}': {}",
                            candidate_path.display(),
                            e
                        )
                    })?;
                    let raw_val: serde_yaml::Value =
                        serde_yaml::from_str(&content).map_err(|e| {
                            format!(
                                "Failed to parse jig file '{}': {}",
                                candidate_path.display(),
                                e
                            )
                        })?;
                    let mut loaded: HardwareConnectParams = if let Some(hw) =
                        raw_val.get("hardware")
                    {
                        let mut p_loaded: HardwareConnectParams =
                            serde_yaml::from_value(hw.clone()).unwrap_or_default();
                        if p_loaded.servos.is_none() {
                            if let Some(servos_val) =
                                hw.get("servos").or_else(|| raw_val.get("servos"))
                            {
                                p_loaded.servos = serde_yaml::from_value(servos_val.clone()).ok();
                            }
                        }
                        if p_loaded.buttons.is_none() {
                            if let Some(btns_val) =
                                hw.get("buttons").or_else(|| raw_val.get("buttons"))
                            {
                                p_loaded.buttons = serde_yaml::from_value(btns_val.clone()).ok();
                            }
                        }
                        if p_loaded.relays.is_none() {
                            if let Some(relays_val) =
                                hw.get("relays").or_else(|| raw_val.get("relays"))
                            {
                                p_loaded.relays = serde_yaml::from_value(relays_val.clone()).ok();
                            }
                        }
                        p_loaded
                    } else {
                        serde_yaml::from_value(raw_val.clone()).map_err(|e| {
                            format!(
                                "Failed to parse jig file '{}': {}",
                                candidate_path.display(),
                                e
                            )
                        })?
                    };
                    if !p.port.is_empty() {
                        loaded.port = p.port.clone();
                    }
                    if p.baudrate.is_some() {
                        loaded.baudrate = p.baudrate;
                    }
                    if p.node_id.is_some() {
                        loaded.node_id = p.node_id;
                    }
                    if p.wire_format.is_some() {
                        loaded.wire_format = p.wire_format.clone();
                    }
                    if p.auto_power_off.is_some() {
                        loaded.auto_power_off = p.auto_power_off;
                    }
                    if p.timeout_ms.is_some() {
                        loaded.timeout_ms = p.timeout_ms;
                    }
                    if p.servos.is_some() {
                        loaded.servos = p.servos.clone();
                    }
                    Ok(loaded)
                } else {
                    Ok(p.clone())
                }
            }
        }
    }

    pub fn to_params(&self) -> HardwareConnectParams {
        self.resolve(None).unwrap_or_else(|_| match self {
            JigConfig::PortOrProfile(port) => HardwareConnectParams {
                port: port.clone(),
                auto_power_off: Some(true),
                ..Default::default()
            },
            JigConfig::Struct(p) => p.clone(),
        })
    }
}

/// Either a single camera (registered as "default") or several named cameras.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CamerasConfig {
    Single(CameraFlowConfig),
    Named(HashMap<String, CameraFlowConfig>),
}

impl CamerasConfig {
    /// Flatten into a name → config map; the single form becomes "default".
    pub fn into_map(self) -> HashMap<String, CameraFlowConfig> {
        match self {
            CamerasConfig::Single(c) => {
                let mut m = HashMap::new();
                m.insert("default".to_string(), c);
                m
            }
            CamerasConfig::Named(m) => m,
        }
    }
}

/// Header-level camera configuration. Declared once so every device-state
/// command in the flow reuses one warm RTSP stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraFlowConfig {
    /// RTSP url of the camera pointed at the physical device.
    #[serde(default)]
    pub rtsp: String,
    /// Existing camera web server URL, e.g. "http://localhost:9444".
    #[serde(default)]
    pub server: Option<String>,
    /// Path to the calibration profile JSON (relative to the test file).
    #[serde(default)]
    pub profile: Option<String>,
    /// RTSP transport: "tcp" | "udp" | "auto" (default tcp).
    #[serde(default)]
    pub transport: Option<String>,
    /// Open a live observation web view while the flow runs.
    #[serde(default)]
    pub observe: bool,
}

/// Parameters for `assertDeviceState` — read a region's LED state immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStateParams {
    /// Region label as defined in the camera profile. For switches this is a
    /// button (e.g. "Nút 1"); for a home controller it is a status LED.
    #[serde(alias = "region", alias = "led")]
    pub button: String,
    /// Expected state/color name (e.g. "ON", "OFF", "GREEN").
    pub expect: String,
    /// Which named camera to use (optional when the flow has a single camera).
    #[serde(default)]
    pub camera: Option<String>,
}

/// Parameters for `waitDeviceState` — poll until a region reaches a state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitDeviceStateParams {
    #[serde(alias = "region", alias = "led")]
    pub button: String,
    pub expect: String,
    /// Max time to wait (ms). Default 8000.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Consecutive matching reads required (anti-flicker). Default 3.
    #[serde(default)]
    pub stable_frames: Option<u32>,
    #[serde(default)]
    pub camera: Option<String>,
}

/// Parameters for `assertDeviceTransition` — verify a region changed states.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTransitionParams {
    #[serde(alias = "region", alias = "led")]
    pub button: String,
    /// Required starting state (precondition; catches false passes).
    pub from: String,
    /// Target state to wait for.
    pub to: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub stable_frames: Option<u32>,
    #[serde(default)]
    pub camera: Option<String>,
}

/// Parameters for `getDeviceState` — read all regions into a variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDeviceStateParams {
    /// Variable name to store the device state JSON in.
    #[serde(alias = "saveAs")]
    pub save_as: String,
    #[serde(default)]
    pub camera: Option<String>,
}

/// Parameters for `waitLedPattern` / `assertDevicePattern`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedPatternParams {
    #[serde(alias = "region", alias = "led")]
    pub button: String,
    /// State/color to count as the active pulse, e.g. "PINK".
    #[serde(alias = "color", alias = "state")]
    pub expect: String,
    /// Number of pulses required.
    #[serde(default = "default_blink_count")]
    pub count: u32,
    /// Max span from first pulse start to Nth pulse end.
    #[serde(default = "default_pattern_within_ms")]
    pub within_ms: u64,
    /// Max time to observe samples. Defaults to `withinMs`.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Sampling interval. Defaults to 20ms.
    #[serde(default = "default_pattern_sample_ms")]
    pub sample_ms: u64,
    /// Ignore pulses shorter than this. Defaults to 40ms.
    #[serde(default = "default_pulse_min_ms")]
    pub pulse_min_ms: u64,
    /// Ignore pulses longer than this. Defaults to 250ms.
    #[serde(default = "default_pulse_max_ms")]
    pub pulse_max_ms: u64,
    #[serde(default)]
    pub camera: Option<String>,
}

fn default_blink_count() -> u32 {
    1
}

fn default_pattern_within_ms() -> u64 {
    800
}

fn default_pattern_sample_ms() -> u64 {
    20
}

fn default_pulse_min_ms() -> u64 {
    40
}

fn default_pulse_max_ms() -> u64 {
    250
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DesktopState {
    #[serde(default)]
    pub clear: Option<DesktopClearConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DesktopClearConfig {
    #[serde(default)]
    pub mode: Option<DesktopClearMode>,

    #[serde(default)]
    pub paths: Vec<String>,

    #[serde(default, alias = "keychainServices")]
    pub keychain_services: Vec<String>,

    #[serde(default, alias = "registryKeys")]
    pub registry_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DesktopClearMode {
    AutoSafe,
    Manual,
}

/// Target platform for testing
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
    Android,
    #[serde(rename = "android_auto", alias = "androidauto")]
    AndroidAuto,
    #[serde(alias = "iOS")]
    Ios,
    Web,
    #[serde(alias = "macOS")]
    Macos,
    Windows,
    #[serde(rename = "lumi_e2e")]
    LumiE2e,
    #[serde(rename = "lumi_hc")]
    LumiHc,
    #[serde(rename = "lumi_cloud")]
    LumiCloud,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindParams {
    pub name: String,
    #[serde(flatten)]
    pub selector: TapParams,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScrollableParams {
    #[serde(default)]
    pub enable: Option<bool>,

    /// Index of the scrollable container (if multiple exist)
    #[serde(default)]
    pub index: Option<u32>,

    /// 'index' attribute of the child item within the scrollable container
    #[serde(default, alias = "itemIndex")]
    pub item_index: Option<u32>,
}

/// Device orientation modes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Orientation {
    Portrait,
    Landscape,
    UpsideDown,
    LandscapeLeft,
    LandscapeRight,
}

// Forward declarations for new param types used in TestCommand enum
// (Full definitions are below)

/// Parameters for assertTrue command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertTrueCondition {
    pub condition: String,
    #[serde(default)]
    pub soft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum AssertTrueParams {
    Expression(String),
    Condition(AssertTrueCondition),
}

/// Parameters for copyTextFrom command
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CopyTextFromParams {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    /// Accessibility description/content-desc selector
    #[serde(
        default,
        alias = "desc",
        alias = "contentDesc",
        alias = "accessibilityId"
    )]
    pub description: Option<String>,
    #[serde(default, alias = "type")]
    pub element_type: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub relative: Option<RelativeParams>,
    #[serde(default, alias = "rightOf")]
    pub right_of: Option<RelativeAnchorInput>,
    #[serde(default, alias = "leftOf")]
    pub left_of: Option<RelativeAnchorInput>,
    #[serde(default, alias = "aboveOf")]
    pub above: Option<RelativeAnchorInput>,
    #[serde(default, alias = "belowOf")]
    pub below: Option<RelativeAnchorInput>,
    /// OCR text recognition selector
    #[serde(default)]
    pub ocr: Option<OcrSelectorInput>,
}

/// Parameters for inputRandomNumber
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RandomNumberParams {
    #[serde(default)]
    pub length: Option<u32>,
}

/// Parameters for inputRandomText
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RandomTextParams {
    #[serde(default)]
    pub length: Option<u32>,
}

/// OCR selector parameters
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OcrSelectorParams {
    /// Text to find via OCR (exact or regex pattern - auto-detected)
    pub text: String,
    /// Index when multiple matches found (0-based)
    #[serde(default)]
    pub index: Option<u32>,
    /// Region to search: "top-left", "top-right", "bottom-left", "bottom-right",
    /// "top-half", "bottom-half", "left-half", "right-half", "center"
    #[serde(default)]
    pub region: Option<String>,
}

/// OCR selector input - supports both string and struct form
/// - Short: `ocr: "some text"`
/// - Full: `ocr: { text: "some text", index: 1 }`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OcrSelectorInput {
    String(String),
    Struct(OcrSelectorParams),
}

impl OcrSelectorInput {
    pub fn text(&self) -> &str {
        match self {
            Self::String(s) => s,
            Self::Struct(p) => &p.text,
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Self::String(_) => 0,
            Self::Struct(p) => p.index.unwrap_or(0) as usize,
        }
    }

    /// Auto-detect if text is a regex pattern
    pub fn is_regex(&self) -> bool {
        let text = self.text();
        text.contains('*')
            || text.contains('+')
            || text.contains('?')
            || text.contains('[')
            || text.contains('(')
            || text.contains('|')
            || text.contains('^')
            || text.contains('$')
            || text.contains("\\d")
            || text.contains("\\w")
            || text.contains("\\s")
            || text.contains("\\b")
    }

    /// Get region for cropping screenshot before OCR
    pub fn region(&self) -> Option<&str> {
        match self {
            Self::String(_) => None,
            Self::Struct(p) => p.region.as_deref(),
        }
    }
}

/// Parameters for extendedWaitUntil (forward declaration - uses AssertParams)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PressKeyParams {
    pub key: String,
    #[serde(default = "default_press_times_value")]
    pub times: serde_json::Value,
}

fn default_press_times_value() -> serde_json::Value {
    serde_json::Value::Number(1.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PressKeyParamsInput {
    String(String),
    Struct(PressKeyParams),
}

impl PressKeyParamsInput {
    pub fn key(&self) -> &str {
        match self {
            Self::String(s) => s,
            Self::Struct(p) => &p.key,
        }
    }

    pub fn times(&self) -> u32 {
        match self {
            Self::String(_) => 1,
            Self::Struct(p) => match &p.times {
                serde_json::Value::Number(n) => n.as_u64().unwrap_or(1) as u32,
                _ => 1, // Will be handled by executor if it's a string
            },
        }
    }

    pub fn times_value(&self) -> serde_json::Value {
        match self {
            Self::String(_) => serde_json::Value::Number(1.into()),
            Self::Struct(p) => p.times.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedWaitParams {
    pub timeout: u64,
    #[serde(default)]
    pub visible: Option<Box<serde_json::Value>>,
    #[serde(default)]
    pub not_visible: Option<Box<serde_json::Value>>,
}

/// All supported test commands
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestCommand {
    /// Home Controller semantic E2E command branch.
    E2e(crate::e2e::types::E2eCommand),

    // App lifecycle
    #[serde(alias = "open")]
    LaunchApp(Option<LaunchAppParamsInput>),
    StopApp,

    // Variables & Reusables
    Find(FindParams),

    // Interactions
    #[serde(alias = "tap")]
    TapOn(TapParamsInput),
    #[serde(alias = "longPress")]
    LongPressOn(TapParamsInput),
    #[serde(alias = "doubleTap")]
    DoubleTapOn(TapParamsInput),
    #[serde(alias = "write")]
    InputText(InputTextParamsInput),
    EraseText(Option<EraseTextParams>),
    HideKeyboard,
    #[serde(rename = "rightClick", alias = "contextClick")]
    RightClick(TapParams),

    // Indexed interactions (by element type and index)
    TapAt(TapAtParams),
    InputAt(InputAtParams),

    // Swipe/Scroll/Drag
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
    #[serde(alias = "swipe")]
    ManualScroll(Option<ScrollParams>),
    #[serde(alias = "drag", alias = "dragFromTo")]
    Drag(DragParams),
    #[serde(alias = "scrollTo")]
    ScrollUntilVisible(ScrollUntilVisibleInput),

    // Multi-touch gestures (map pinch-zoom / camera tilt). Android and iOS only.
    Pinch(PinchParams),
    Shove(ShoveParams),

    // Assertions
    #[serde(alias = "see")]
    AssertVisible(AssertParamsInput),
    #[serde(alias = "notSee")]
    AssertNotVisible(AssertParamsInput),
    #[serde(alias = "waitUntilVisible", alias = "waitSee")]
    WaitUntilVisible(AssertParamsInput),
    #[serde(alias = "waitNotSee")]
    WaitUntilNotVisible(AssertParamsInput),

    #[serde(rename = "sendLarkMessage", alias = "lark")]
    SendLarkMessage(SendLarkMessageParams),

    // Hardware verification via camera (read physical device LED states)
    #[serde(alias = "checkDevice")]
    AssertDeviceState(DeviceStateParams),
    WaitDeviceState(WaitDeviceStateParams),
    #[serde(alias = "assertDeviceChange")]
    AssertDeviceTransition(DeviceTransitionParams),
    #[serde(alias = "assertDevicePattern")]
    WaitLedPattern(LedPatternParams),
    GetDeviceState(GetDeviceStateParams),

    // Control flow
    WaitForAnimationToEnd,
    #[serde(alias = "await")]
    Wait(WaitParamsInput),
    Repeat(RepeatParams),
    Retry(RetryParams),
    RunFlow(RunFlowParamsInput),
    When(WhenParams),
    ForEach(ForEachParams),
    Match(MatchParams),

    // Variables
    SetVar(SetVarParams),
    AssertVar(AssertVarParams),

    // Media
    #[serde(alias = "openLink", alias = "deepLink")]
    OpenLink(String),
    /// Desktop only (macOS/Windows): switch which already-running app/window
    /// subsequent selectors resolve against, without launching or relaunching
    /// it. Value is a bundle id/app name (macOS) or process name/window title
    /// substring (Windows). Does not change the flow's `appId` used by
    /// `launchApp`/`stopApp`.
    #[serde(alias = "switchApp")]
    FocusApp(String),
    #[serde(alias = "assertScreenshot")]
    AssertScreenshot(String),
    TakeScreenshot(ScreenshotParamsInput),
    StartRecording(RecordingParamsInput),
    StopRecording,

    // Report
    ExportReport(ReportParams),

    // Navigation
    Back,
    PressHome,

    // Advanced Features
    Generate(GenerateParams),
    HttpRequest(HttpRequestParams),
    RunScript(RunScriptParamsInput),
    Conditional(ConditionalParams),

    // Web-specific (Future)
    Navigate(NavigateParams),
    Click(ClickParams),
    Type(TypeParams),

    // GPS Mock Location
    #[serde(alias = "gps")]
    MockLocation(MockLocationParamsInput),
    StopMockLocation,
    MockLocationControl(MockLocationControlParams),

    // Visual Assertions
    #[serde(alias = "checkColor")]
    AssertColor(AssertColorParams),

    // GIF Recording
    #[serde(alias = "captureFrame")]
    CaptureGifFrame(CaptureGifFrameParamsInput),
    #[serde(alias = "createGif")]
    BuildGif(BuildGifParams),
    // GIF Auto-Capture (interval-based)
    StartGifCapture(StartGifCaptureParams),
    StopGifCapture(StopGifCaptureParams),

    // Device Control
    #[serde(alias = "rotate")]
    RotateScreen(RotationParamsInput),
    #[serde(alias = "press")]
    PressKey(PressKeyParamsInput),

    // File Management
    PushFile(FileTransferParams),
    PullFile(FileTransferParams),
    ClearAppData(String), // package_id

    // Clipboard
    #[serde(alias = "setClipboard")]
    SetClipboard(String),
    #[serde(alias = "getClipboard")]
    GetClipboard(SetVarParams), // save to variable
    #[serde(alias = "assertClipboard")]
    AssertClipboard(String),

    #[serde(alias = "assert")]
    AssertTrue(AssertTrueParams),
    EvalScript(String),

    // Clipboard Operations
    CopyTextFrom(CopyTextFromParams),
    PasteText,

    // Random Input
    InputRandomEmail,
    InputRandomNumber(Option<RandomNumberParams>),
    InputRandomPersonName,
    InputRandomText(Option<RandomTextParams>),

    // Extended Wait
    ExtendedWaitUntil(ExtendedWaitParams),

    // Database
    #[serde(alias = "dbQuery")]
    DbQuery(DbQueryParams),

    // Network & Connectivity
    #[serde(alias = "setNetwork")]
    SetNetwork(NetworkParams),
    #[serde(alias = "airplaneMode")]
    ToggleAirplaneMode,

    // System Interactions
    OpenNotifications,
    OpenQuickSettings,
    SetVolume(u8),
    LockDevice,
    UnlockDevice,

    // App Management
    InstallApp(String),
    UninstallApp(String),
    BackgroundApp(BackgroundAppParams),

    // Device Orientation
    #[serde(alias = "setOrientation")]
    SetOrientation(OrientationParams),
    // Mock Location Sync
    WaitForLocation(WaitForLocationParams),
    WaitForMockCompletion(WaitForMockCompletionParams),

    // Performance & Load Testing
    #[serde(alias = "startProfiling")]
    StartProfiling(Option<StartProfilingParams>),
    #[serde(alias = "stopProfiling")]
    StopProfiling(Option<StopProfilingParams>),
    #[serde(alias = "assertPerformance")]
    AssertPerformance(AssertPerformanceParams),
    #[serde(alias = "setCpuThrottling")]
    SetCpuThrottling(f64),
    #[serde(alias = "setNetworkConditions")]
    SetNetworkConditions(String),

    #[serde(alias = "display")]
    SelectDisplay(String),

    // Locale/Language
    #[serde(alias = "locale")]
    SetLocale(String),

    // Audio Test Commands
    #[serde(alias = "playMedia")]
    PlayMedia(PlayMediaParams),
    #[serde(alias = "stopMedia")]
    StopMedia,
    #[serde(alias = "startAudioCapture")]
    StartAudioCapture(StartAudioCaptureParams),
    #[serde(alias = "stopAudioCapture")]
    StopAudioCapture,
    #[serde(alias = "verifyAudioDucking")]
    VerifyAudioDucking(VerifyAudioDuckingParams),

    // Hardware Automation Commands (Standardized hw* Commands)
    HwConnect(HardwareConnectParams),
    HwPing(HardwarePingParams),
    HwDisconnect,
    HwClick(ServoClickParams),
    HwRepeatClick(ServoRepeatParams),
    HwPress(ServoActionParams),
    HwRelease(ServoActionParams),
    HwRotate(ServoRotateParams),
    HwReadServo(ServoActionParams),
    HwReadRelay(ServoActionParams),
    HwReadColor(ServoActionParams),
    HwReadSensorLight(Option<ServoActionParams>),
    HwPowerOn(RelaySetParams),
    HwPowerOff(RelaySetParams),
    HwPowerOffAll,
    HwPowerCycle(PowerCycleParams),
    HwSeeLed(SeeColorParams),
    HwSeeLedBlink(SeeBlinkParams),
    /// Same params as HwSeeLedBlink, but detection runs on the firmware's own
    /// hardware-timed blink event log instead of client-side RGBC polling - immune
    /// to host serial/scheduling jitter. min_pulse_ms/max_pulse_ms are ignored here
    /// (the firmware uses its own flash-calibrated thresholds).
    HwSeeNativeLedBlink(SeeBlinkParams),
    HwSeeLedOff(SeeBlinkParams),

    HwConfigureServo(ServoConfigParams),
    HwReleaseAll,
    HwStartRepeatClick(ServoStartRepeatParams),
    HwStopRepeatClick(ServoActionParams),
    HwSensorLight(SensorLightParams),
    HwSetBrightnessThresholds(BrightnessThresholdsParams),
    HwWaitForBrightness(WaitForBrightnessParams),
    HwWaitForCct(WaitForCctParams),
    HwCalibrateColor(CalibrateColorParams),
    HwCalibrateBrightness(CalibrateBrightnessParams),
    HwAddCctPoint(AddCctPointParams),
    HwSaveCalibration,
    HwLoadCalibration,
    HwResetCalibration,
    HwEraseCalibration,
    HwSafeState,
    HwDiagnostics,

    RunPython(RunPythonParams),

    // Window control / Responsive testing
    #[serde(
        rename = "setWindowSize",
        alias = "resizeWindow",
        alias = "windowSize",
        alias = "setWindow"
    )]
    SetWindowSize(SetWindowSizeParamsInput),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SetWindowSizeParamsInput {
    String(String),
    Struct(SetWindowSizeParams),
}

impl SetWindowSizeParamsInput {
    pub fn to_dimensions(&self) -> Option<(u32, u32)> {
        match self {
            SetWindowSizeParamsInput::Struct(p) => Some((p.width, p.height)),
            SetWindowSizeParamsInput::String(s) => {
                let parts: Vec<&str> = if s.contains('x') || s.contains('X') {
                    s.split(|c| c == 'x' || c == 'X').collect()
                } else {
                    s.split(',').collect()
                };
                if parts.len() == 2 {
                    if let (Ok(w), Ok(h)) = (
                        parts[0].trim().parse::<u32>(),
                        parts[1].trim().parse::<u32>(),
                    ) {
                        return Some((w, h));
                    }
                }
                None
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SetWindowSizeParams {
    pub width: u32,
    pub height: u32,
}

pub fn parse_channel_str(s: &str) -> u8 {
    let trimmed = s.trim();
    if let Ok(num) = trimmed.parse::<u8>() {
        return num;
    }
    match trimmed.to_uppercase().as_str() {
        "NC1" | "BUTTON1" | "SWITCH 1.1" | "SWITCH1.1" | "1.1" => 5,
        "NC2" | "BUTTON2" | "SWITCH 1.2" | "SWITCH1.2" | "1.2" => 6,
        "NC3" | "BUTTON3" | "SWITCH 1.3" | "SWITCH1.3" | "1.3" => 7,
        "KNX" => 1,
        "24V" | "POWER24V" => 2,
        "220V_L" | "220V-L" | "220VL" | "L" => 3,
        "220V_N" | "220V-N" | "220VN" | "N" => 4,
        _ => {
            let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<u8>().unwrap_or(1)
        }
    }
}

pub fn deserialize_channel<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ChannelOrString {
        Num(u64),
        Str(String),
    }

    match ChannelOrString::deserialize(deserializer)? {
        ChannelOrString::Num(n) => Ok(n as u8),
        ChannelOrString::Str(s) => Ok(parse_channel_str(&s)),
    }
}

fn default_channel_one() -> u8 {
    1
}

pub fn parse_relay_channels(s: &str) -> Vec<u8> {
    let trimmed = s.trim();
    match trimmed.to_uppercase().as_str() {
        "MAINPOWER" | "220V" | "POWER220V" | "MAINS" | "220" => vec![3, 4],
        "220V_L" | "220V-L" | "220VL" | "L" => vec![3],
        "220V_N" | "220V-N" | "220VN" | "N" => vec![4],
        "KNX" => vec![1],
        "24V" | "POWER24V" => vec![2],
        "ALL" => vec![1, 2, 3, 4],
        _ => {
            if let Ok(num) = trimmed.parse::<u8>() {
                vec![num]
            } else {
                let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(num) = digits.parse::<u8>() {
                    vec![num]
                } else {
                    vec![1]
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HardwarePingParams {
    #[serde(default)]
    pub port: Option<String>,
    #[serde(default)]
    pub baudrate: Option<u32>,
    #[serde(default, alias = "node_id", alias = "nodeId", alias = "node")]
    pub node_id: Option<u8>,
    #[serde(default, alias = "saveAs", alias = "saveOutput", alias = "save_output")]
    pub save_as: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HardwareConnectParams {
    #[serde(default)]
    pub port: String,
    #[serde(default)]
    pub baudrate: Option<u32>,
    #[serde(default, alias = "node_id", alias = "nodeId", alias = "node")]
    pub node_id: Option<u8>,
    #[serde(
        default,
        alias = "wire_format",
        alias = "wireFormat",
        alias = "framing",
        alias = "format"
    )]
    pub wire_format: Option<String>,
    #[serde(default)]
    pub auto_power_off: Option<bool>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(
        default,
        alias = "force_config",
        alias = "forceConfig",
        alias = "force"
    )]
    pub force_config: Option<bool>,
    #[serde(default)]
    pub servos: Option<Vec<ServoConfigParams>>,
    #[serde(default)]
    pub buttons: Option<HashMap<String, crate::hardware::ButtonChannelMapping>>,
    #[serde(default)]
    pub relays: Option<HashMap<String, Vec<u8>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ChannelOrVar {
    Num(u64),
    Str(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServoClickParams {
    #[serde(default = "default_channel_one")]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    #[serde(default, alias = "hold_ms", alias = "holdMs")]
    pub hold_ms: Option<u64>,
}

impl<'de> Deserialize<'de> for ServoClickParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawStruct {
            #[serde(default)]
            channel: Option<ChannelOrVar>,
            #[serde(default, alias = "button", alias = "btn")]
            button: Option<String>,
            #[serde(default, alias = "hold_ms", alias = "holdMs")]
            hold_ms: Option<u64>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Num(u64),
            Str(String),
            Obj(RawStruct),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Num(n) => Ok(ServoClickParams {
                channel: n as u8,
                button: None,
                hold_ms: None,
            }),
            Helper::Str(s) => Ok(ServoClickParams {
                channel: parse_channel_str(&s),
                button: Some(s),
                hold_ms: None,
            }),
            Helper::Obj(raw) => {
                let (ch, btn) = match (raw.channel, raw.button) {
                    (Some(ChannelOrVar::Str(s)), None) => (parse_channel_str(&s), Some(s)),
                    (Some(ChannelOrVar::Str(s)), Some(b)) => (parse_channel_str(&s), Some(b)),
                    (Some(ChannelOrVar::Num(n)), b) => (n as u8, b),
                    (None, Some(b)) => (parse_channel_str(&b), Some(b)),
                    (None, None) => (1, None),
                };
                Ok(ServoClickParams {
                    channel: ch,
                    button: btn,
                    hold_ms: raw.hold_ms,
                })
            }
        }
    }
}

impl ServoClickParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServoActionParams {
    #[serde(default = "default_channel_one")]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    #[serde(default, alias = "saveAs", alias = "saveOutput", alias = "save_output")]
    pub save_as: Option<String>,
}

impl<'de> Deserialize<'de> for ServoActionParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawStruct {
            #[serde(default)]
            channel: Option<ChannelOrVar>,
            #[serde(default, alias = "button", alias = "btn")]
            button: Option<String>,
            #[serde(default, alias = "saveAs", alias = "saveOutput", alias = "save_output")]
            save_as: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Num(u64),
            Str(String),
            Obj(RawStruct),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Num(n) => Ok(ServoActionParams {
                channel: n as u8,
                button: None,
                save_as: None,
            }),
            Helper::Str(s) => Ok(ServoActionParams {
                channel: parse_channel_str(&s),
                button: Some(s),
                save_as: None,
            }),
            Helper::Obj(raw) => {
                let (ch, btn) = match (raw.channel, raw.button) {
                    (Some(ChannelOrVar::Str(s)), None) => (parse_channel_str(&s), Some(s)),
                    (Some(ChannelOrVar::Str(s)), Some(b)) => (parse_channel_str(&s), Some(b)),
                    (Some(ChannelOrVar::Num(n)), b) => (n as u8, b),
                    (None, Some(b)) => (parse_channel_str(&b), Some(b)),
                    (None, None) => (1, None),
                };
                Ok(ServoActionParams {
                    channel: ch,
                    button: btn,
                    save_as: raw.save_as,
                })
            }
        }
    }
}

impl ServoActionParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServoRotateParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    pub angle: i32,
    #[serde(default)]
    pub speed: Option<u32>,
}

impl ServoRotateParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySetParams {
    #[serde(default = "default_channel_one")]
    pub channel: u8,
    #[serde(
        default,
        alias = "button",
        alias = "relay",
        alias = "name",
        alias = "group"
    )]
    pub button: Option<String>,
    #[serde(default, alias = "channels")]
    pub channels: Option<Vec<u8>>,
    #[serde(default)]
    pub state: String,
}

impl<'de> Deserialize<'de> for RelaySetParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawStruct {
            #[serde(default)]
            channel: Option<ChannelOrVar>,
            #[serde(
                default,
                alias = "button",
                alias = "relay",
                alias = "name",
                alias = "group"
            )]
            button: Option<String>,
            #[serde(default, alias = "channels")]
            channels: Option<Vec<u8>>,
            #[serde(default)]
            state: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Num(u64),
            Str(String),
            Obj(RawStruct),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Num(n) => Ok(RelaySetParams {
                channel: n as u8,
                button: None,
                channels: None,
                state: "on".to_string(),
            }),
            Helper::Str(s) => Ok(RelaySetParams {
                channel: parse_channel_str(&s),
                button: Some(s),
                channels: None,
                state: "on".to_string(),
            }),
            Helper::Obj(raw) => {
                let (ch, btn) = match (raw.channel, raw.button) {
                    (Some(ChannelOrVar::Str(s)), None) => (parse_channel_str(&s), Some(s)),
                    (Some(ChannelOrVar::Str(s)), Some(b)) => (parse_channel_str(&s), Some(b)),
                    (Some(ChannelOrVar::Num(n)), b) => (n as u8, b),
                    (None, Some(b)) => (parse_channel_str(&b), Some(b)),
                    (None, None) => (1, None),
                };
                Ok(RelaySetParams {
                    channel: ch,
                    button: btn,
                    channels: raw.channels,
                    state: raw.state.unwrap_or_else(|| "on".to_string()),
                })
            }
        }
    }
}

impl RelaySetParams {
    pub fn resolved_channels(&self) -> Vec<u8> {
        if let Some(ref list) = self.channels {
            if !list.is_empty() {
                return list.clone();
            }
        }
        if let Some(ref btn) = self.button {
            return parse_relay_channels(btn);
        }
        vec![self.channel]
    }
}

fn deserialize_opt_string_or_vec<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrVec {
        Str(String),
        Vec(Vec<String>),
    }

    let opt: Option<StrOrVec> = Option::deserialize(deserializer)?;
    match opt {
        Some(StrOrVec::Str(s)) => Ok(Some(vec![s])),
        Some(StrOrVec::Vec(v)) => Ok(Some(v)),
        None => Ok(None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeeColorParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_or_vec",
        alias = "expect"
    )]
    pub expected: Option<Vec<String>>,
    #[serde(default, alias = "timeout_ms", alias = "timeoutMs")]
    pub timeout_ms: Option<u64>,
}

impl SeeColorParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }

    pub fn resolved_expected(&self) -> Vec<String> {
        if let Some(ref list) = self.expected {
            if !list.is_empty() {
                return list.clone();
            }
        }
        if let Some(ref col) = self.color {
            return vec![col.clone()];
        }
        Vec::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeeBlinkParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default, alias = "timeout_ms", alias = "timeoutMs")]
    pub timeout_ms: Option<u64>,
    #[serde(default, alias = "min_pulse_ms", alias = "minPulseMs")]
    pub min_pulse_ms: Option<u64>,
    #[serde(default, alias = "max_pulse_ms", alias = "maxPulseMs")]
    pub max_pulse_ms: Option<u64>,
    #[serde(default, alias = "max_gap_ms", alias = "maxGapMs")]
    pub max_gap_ms: Option<u64>,
}

impl SeeBlinkParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

fn default_repeat_count() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServoRepeatParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    #[serde(default = "default_repeat_count")]
    pub count: u32,
    #[serde(default, alias = "press_ms", alias = "pressMs")]
    pub press_ms: Option<u64>,
    #[serde(default, alias = "release_ms", alias = "releaseMs")]
    pub release_ms: Option<u64>,
}

impl ServoRepeatParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerCycleParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(
        default,
        alias = "button",
        alias = "relay",
        alias = "name",
        alias = "group"
    )]
    pub button: Option<String>,
    #[serde(default, alias = "channels")]
    pub channels: Option<Vec<u8>>,
    #[serde(default, alias = "off_ms", alias = "offMs")]
    pub off_ms: Option<u64>,
}

impl PowerCycleParams {
    pub fn resolved_channels(&self) -> Vec<u8> {
        if let Some(ref list) = self.channels {
            if !list.is_empty() {
                return list.clone();
            }
        }
        if let Some(ref btn) = self.button {
            return parse_relay_channels(btn);
        }
        vec![self.channel]
    }

    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServoConfigParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    #[serde(default, alias = "press_angle", alias = "pressAngle")]
    pub press_angle: Option<u8>,
    #[serde(default, alias = "release_angle", alias = "releaseAngle")]
    pub release_angle: Option<u8>,
    #[serde(default, alias = "press_duration_ms", alias = "pressDurationMs")]
    pub press_duration_ms: Option<u16>,
    #[serde(default, alias = "release_duration_ms", alias = "releaseDurationMs")]
    pub release_duration_ms: Option<u16>,
    #[serde(default, alias = "hold_duration_ms", alias = "holdDurationMs")]
    pub hold_duration_ms: Option<u16>,
    #[serde(
        default,
        alias = "force_config",
        alias = "forceConfig",
        alias = "force"
    )]
    pub force: Option<bool>,
}

impl ServoConfigParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServoStartRepeatParams {
    #[serde(default = "default_channel_one")]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn")]
    pub button: Option<String>,
    #[serde(default, alias = "period_ms", alias = "periodMs")]
    pub period_ms: Option<u64>,
}

impl<'de> Deserialize<'de> for ServoStartRepeatParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawStruct {
            #[serde(default)]
            channel: Option<ChannelOrVar>,
            #[serde(default, alias = "button", alias = "btn")]
            button: Option<String>,
            #[serde(default, alias = "period_ms", alias = "periodMs")]
            period_ms: Option<u64>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Num(u64),
            Str(String),
            Obj(RawStruct),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Num(n) => Ok(ServoStartRepeatParams {
                channel: n as u8,
                button: None,
                period_ms: None,
            }),
            Helper::Str(s) => Ok(ServoStartRepeatParams {
                channel: parse_channel_str(&s),
                button: Some(s),
                period_ms: None,
            }),
            Helper::Obj(raw) => {
                let (ch, btn) = match (raw.channel, raw.button) {
                    (Some(ChannelOrVar::Str(s)), None) => (parse_channel_str(&s), Some(s)),
                    (Some(ChannelOrVar::Str(s)), Some(b)) => (parse_channel_str(&s), Some(b)),
                    (Some(ChannelOrVar::Num(n)), b) => (n as u8, b),
                    (None, Some(b)) => (parse_channel_str(&b), Some(b)),
                    (None, None) => (1, None),
                };
                Ok(ServoStartRepeatParams {
                    channel: ch,
                    button: btn,
                    period_ms: raw.period_ms,
                })
            }
        }
    }
}

impl ServoStartRepeatParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensorLightParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn", alias = "sensor")]
    pub button: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

impl SensorLightParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrightnessThresholdsParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn", alias = "sensor")]
    pub button: Option<String>,
    pub off_below_percent: u8,
    pub on_above_percent: u8,
    #[serde(default)]
    pub min_pulse_ms: Option<u16>,
    #[serde(default)]
    pub max_pulse_ms: Option<u16>,
    #[serde(default)]
    pub sequence_end_gap_ms: Option<u16>,
}

impl BrightnessThresholdsParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitForBrightnessParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn", alias = "sensor")]
    pub button: Option<String>,
    #[serde(default)]
    pub min_percent: Option<u8>,
    #[serde(default)]
    pub max_percent: Option<u8>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl WaitForBrightnessParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitForCctParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn", alias = "sensor")]
    pub button: Option<String>,
    pub min_kelvin: u16,
    pub max_kelvin: u16,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl WaitForCctParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrateColorParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn", alias = "sensor")]
    pub button: Option<String>,
    pub color: String,
}

impl CalibrateColorParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrateBrightnessParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn", alias = "sensor")]
    pub button: Option<String>,
    pub mode: String,
    #[serde(default)]
    pub color: Option<String>,
}

impl CalibrateBrightnessParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCctPointParams {
    #[serde(
        default = "default_channel_one",
        deserialize_with = "deserialize_channel"
    )]
    pub channel: u8,
    #[serde(default, alias = "button", alias = "btn", alias = "sensor")]
    pub button: Option<String>,
    pub known_kelvin: u16,
}

impl AddCctPointParams {
    pub fn resolved_channel(&self) -> u8 {
        if let Some(ref btn) = self.button {
            parse_channel_str(btn)
        } else {
            self.channel
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SaveVarsInput {
    List(Vec<String>),
    Map(HashMap<String, String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPythonParams {
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default, alias = "timeout_ms", alias = "timeoutMs")]
    pub timeout_ms: Option<u64>,
    #[serde(default, alias = "python_path", alias = "pythonPath")]
    pub python_path: Option<String>,
    #[serde(default, alias = "save_var", alias = "saveVar")]
    pub save_var: Option<String>,
    #[serde(default, alias = "save_vars", alias = "saveVars")]
    pub save_vars: Option<SaveVarsInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitForLocationParams {
    /// Name of the mock location instance to wait for (optional, defaults to default instance)
    #[serde(default)]
    pub name: Option<String>,

    pub lat: f64,
    pub lon: f64,
    #[serde(default = "default_tolerance_meters")]
    pub tolerance: f64,
    #[serde(default = "default_wait_ms")]
    pub timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitForMockCompletionParams {
    /// Name of the mock location instance to wait for (optional, defaults to default instance)
    #[serde(default)]
    pub name: Option<String>,

    /// Timeout in ms. If not provided, waits indefinitely until mock completes.
    #[serde(default)]
    pub timeout: Option<u64>,
}

// Audio Test Params

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayMediaParams {
    /// Path to audio file (relative to test file or absolute)
    pub file: String,
    /// Loop playback
    #[serde(default)]
    pub loop_playback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartAudioCaptureParams {
    /// Capture duration in milliseconds
    #[serde(default = "default_audio_capture_duration")]
    pub duration: u64,
    /// Audio server port (default: 8890)
    #[serde(default = "default_audio_port")]
    pub port: u16,
}

fn default_audio_capture_duration() -> u64 {
    30000 // 30 seconds
}

fn default_audio_port() -> u16 {
    8890
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyAudioDuckingParams {
    /// Minimum number of ducking events required
    #[serde(default = "default_min_ducking_count")]
    pub min_ducking_count: usize,
    /// Minimum volume drop percentage (0-100)
    #[serde(default = "default_volume_drop_threshold")]
    pub volume_drop_threshold: f64,
}

fn default_min_ducking_count() -> usize {
    1
}

fn default_volume_drop_threshold() -> f64 {
    30.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartProfilingParams {
    #[serde(default)]
    pub sampling_interval_ms: Option<u64>,
    #[serde(default)]
    pub package: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopProfilingParams {
    #[serde(default)]
    pub save_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertPerformanceParams {
    pub metric: String,
    pub limit: String, // e.g. "200MB", "60fps"
}

fn default_tolerance_meters() -> f64 {
    50.0
}

// Parameter types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LaunchAppParamsInput {
    Struct(LaunchAppParams),
    String(String),
}

impl LaunchAppParamsInput {
    pub fn into_inner(self) -> LaunchAppParams {
        match self {
            Self::Struct(s) => s,
            Self::String(s) => LaunchAppParams {
                app_id: Some(s),
                clear_state: false,
                clear_keychain: false,
                stop_app: None,
                permissions: None,
                label: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchAppParams {
    #[serde(default)]
    pub clear_state: bool,

    /// Clear iOS Keychain data (simulator only)
    #[serde(default)]
    pub clear_keychain: bool,

    /// Stop app before launching (default: true)
    #[serde(default)]
    pub stop_app: Option<bool>,

    /// Permissions to set (e.g. { all: deny }, { notifications: allow })
    #[serde(default)]
    pub permissions: Option<HashMap<String, String>>,

    #[serde(default, alias = "url")]
    pub app_id: Option<String>,

    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TapParams {
    #[serde(default)]
    pub label: Option<String>,

    /// Reference to a pre-defined selector variable (from 'find' command)
    #[serde(default)]
    pub element: Option<String>,

    #[serde(default)]
    pub text: Option<String>,

    #[serde(default)]
    pub regex: Option<String>,

    #[serde(default)]
    pub relative: Option<RelativeParams>,

    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub css: Option<String>,

    #[serde(default)]
    pub xpath: Option<String>,
    #[serde(default)]
    pub role: Option<String>,

    #[serde(default)]
    pub placeholder: Option<String>,

    /// Accessibility description/content-desc selector
    #[serde(
        default,
        alias = "desc",
        alias = "contentDesc",
        alias = "accessibilityId"
    )]
    pub description: Option<String>,

    #[serde(default)]
    pub point: Option<String>, // "x,y" format

    #[serde(default)]
    pub index: Option<u32>,

    /// Element class/type (e.g., "EditText", "Button")
    #[serde(default, alias = "type")]
    pub element_type: Option<String>,

    #[serde(default)]
    pub image: Option<String>, // Path to image file for template matching

    /// Region to search for image: top-left, top-right, bottom-left, bottom-right, etc.
    #[serde(default, alias = "imageRegion")]
    pub image_region: Option<String>,

    /// OCR text recognition selector (for Flutter/game apps)
    #[serde(default)]
    pub ocr: Option<OcrSelectorInput>,

    #[serde(default)]
    pub optional: bool,

    /// Wait and retry tap if the view hierarchy doesn't change (default: true)
    #[serde(default)]
    pub retry_tap_if_no_change: Option<bool>,

    /// Require exact text match (case-sensitive), disable case-insensitive fallback
    #[serde(default)]
    pub exact: bool,

    // Relative position aliases (shorthand for relative param)
    #[serde(default, alias = "rightOf")]
    pub right_of: Option<RelativeAnchorInput>,

    #[serde(default, alias = "leftOf")]
    pub left_of: Option<RelativeAnchorInput>,

    #[serde(default, alias = "aboveOf")]
    pub above: Option<RelativeAnchorInput>,

    #[serde(default, alias = "belowOf")]
    pub below: Option<RelativeAnchorInput>,

    #[serde(default)]
    pub scrollable: Option<ScrollableParams>,

    /// Alignment preset: center (default), left, right, top, bottom
    /// Maps to offset percentages: left=10%,50% right=90%,50% top=50%,10% bottom=50%,90%
    #[serde(default)]
    pub align: Option<String>,

    /// Custom offset within element bounds: "X%,Y%" (e.g., "90%,50%")
    /// Overrides align if both are specified
    #[serde(default)]
    pub offset: Option<String>,

    /// Optional gesture duration in milliseconds (e.g. for longPress)
    #[serde(default)]
    pub duration: Option<u64>,
}

/// Tap element by type and index (e.g., tap 2nd EditText)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TapAtParams {
    /// Element class/type (e.g., "EditText", "Button", "input")
    #[serde(alias = "type")]
    pub element_type: String,

    /// 0-based index of the element
    #[serde(default)]
    pub index: u32,
}

/// Input text at element by type and index
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputAtParams {
    /// Element class/type (e.g., "EditText", "input")
    #[serde(alias = "type")]
    pub element_type: String,

    /// 0-based index of the element
    #[serde(default)]
    pub index: u32,
    /// Text to input
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EraseTextParams {
    #[serde(default)]
    pub char_count: Option<u32>,
}

/// Parameters for inputText command
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InputTextParams {
    /// Text to input
    pub text: String,
    /// Enable Unicode input mode (uses ADBKeyBoard, slower but reliable)
    #[serde(default)]
    pub unicode: bool,

    #[serde(default)]
    pub label: Option<String>,
}

/// Input for InputText command - supports both simple string and struct
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputTextParamsInput {
    String(String),
    Struct(InputTextParams),
}

impl InputTextParamsInput {
    pub fn into_inner(self) -> InputTextParams {
        match self {
            Self::String(text) => InputTextParams {
                text,
                unicode: false, // default: fast mode
                label: None,
            },
            Self::Struct(s) => s,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::String(s) => s,
            Self::Struct(p) => &p.text,
        }
    }

    pub fn unicode(&self) -> bool {
        match self {
            Self::String(_) => false,
            Self::Struct(p) => p.unicode,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollParams {
    #[serde(default)]
    pub direction: Option<String>,

    #[serde(default)]
    pub distance: Option<u32>,

    #[serde(default)]
    pub duration: Option<u32>,

    #[serde(default)]
    pub from: Option<TapParams>,
}

/// Parameters for dragging from one element/coordinate to another
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DragParams {
    pub from: TapParamsInput,
    pub to: TapParamsInput,
    #[serde(default)]
    pub duration: Option<u64>,
}

/// Parameters for a pinch (2-finger zoom) gesture, e.g. map pinch-to-zoom. Android and iOS
/// only: neither `adb shell input` nor XCTest's public gesture wrappers have a multi-touch
/// primitive, so this requires the on-device lm-android-tester/lm-ios-tester agent - there
/// is no fallback the way `tap`/`swipe` have.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinchParams {
    /// "open" (fingers spread apart - zoom in) or "close" (fingers pinch together - zoom out)
    pub direction: String,
    /// Gesture center: a point ("x,y" or "x%,y%") or a selector, same shape as `tapOn`.
    /// Defaults to screen center when omitted.
    #[serde(default)]
    pub at: Option<TapParamsInput>,
    /// How far apart the fingers end up, as a percent of half the shorter screen dimension
    /// (0-100). Default 50.
    #[serde(default)]
    pub percent: Option<f64>,
    #[serde(default)]
    pub duration: Option<u64>,
}

/// Parameters for a shove (2-finger drag in the same direction) gesture - the gesture
/// Mapbox and most map SDKs bind to camera pitch/tilt. Android and iOS only, on-device
/// agent required (see `PinchParams` doc comment).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShoveParams {
    /// "up" (tilt camera into 3D) or "down" (flatten back to 2D/top-down)
    pub direction: String,
    /// Gesture center: a point ("x,y" or "x%,y%") or a selector. Defaults to screen center.
    #[serde(default)]
    pub at: Option<TapParamsInput>,
    /// How far each finger travels, in device pixels. Default 300.
    #[serde(default)]
    pub distance: Option<u64>,
    #[serde(default)]
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollUntilVisibleParams {
    #[serde(default)]
    pub label: Option<String>,

    pub text: Option<String>,
    pub regex: Option<String>,
    pub relative: Option<RelativeParams>,
    #[serde(default, alias = "rightOf")]
    pub right_of: Option<RelativeAnchorInput>,
    #[serde(default, alias = "leftOf")]
    pub left_of: Option<RelativeAnchorInput>,
    #[serde(default, alias = "aboveOf")]
    pub above: Option<RelativeAnchorInput>,
    #[serde(default, alias = "belowOf")]
    pub below: Option<RelativeAnchorInput>,

    pub id: Option<String>,

    #[serde(default)]
    pub css: Option<String>,

    #[serde(default)]
    pub scrollable: Option<ScrollableParams>,

    #[serde(default)]
    pub xpath: Option<String>,
    #[serde(default)]
    pub role: Option<String>,

    #[serde(default)]
    pub placeholder: Option<String>,

    /// Accessibility description/content-desc selector
    #[serde(
        default,
        alias = "desc",
        alias = "contentDesc",
        alias = "accessibilityId"
    )]
    pub description: Option<String>,

    #[serde(default, alias = "type")]
    pub element_type: Option<String>,

    #[serde(default)]
    pub image: Option<String>,

    /// OCR text recognition selector (for Flutter/game apps)
    #[serde(default)]
    pub ocr: Option<OcrSelectorInput>,

    #[serde(default = "default_max_scrolls", alias = "numberScroll")]
    pub max_scrolls: u32,

    #[serde(default)]
    pub direction: Option<String>,

    #[serde(default)]
    pub from: Option<TapParams>,

    #[serde(default)]
    pub timeout: Option<u64>,
}

fn default_max_scrolls() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AssertParams {
    #[serde(default)]
    pub label: Option<String>,

    /// Reference to a pre-defined selector variable (from 'find' command)
    #[serde(default)]
    pub element: Option<String>,

    #[serde(default)]
    pub text: Option<String>,

    #[serde(default)]
    pub regex: Option<String>,

    #[serde(default)]
    pub relative: Option<RelativeParams>,

    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub css: Option<String>,

    #[serde(default)]
    pub xpath: Option<String>,
    #[serde(default)]
    pub role: Option<String>,

    #[serde(default)]
    pub placeholder: Option<String>,

    /// Accessibility description/content-desc selector
    #[serde(
        default,
        alias = "desc",
        alias = "contentDesc",
        alias = "accessibilityId"
    )]
    pub description: Option<String>,

    #[serde(default, alias = "type")]
    pub element_type: Option<String>,

    #[serde(default)]
    pub image: Option<String>,

    /// OCR text recognition selector (for Flutter/game apps)
    #[serde(default)]
    pub ocr: Option<OcrSelectorInput>,

    #[serde(default)]
    pub index: Option<u32>,

    #[serde(default)]
    pub timeout: Option<u64>,

    #[serde(default)]
    pub contains_child: Option<Box<AssertParams>>,

    #[serde(default, alias = "rightOf")]
    pub right_of: Option<RelativeAnchorInput>,
    #[serde(default, alias = "leftOf")]
    pub left_of: Option<RelativeAnchorInput>,
    #[serde(default, alias = "aboveOf")]
    pub above: Option<RelativeAnchorInput>,
    #[serde(default, alias = "belowOf")]
    pub below: Option<RelativeAnchorInput>,

    #[serde(default)]
    pub scrollable: Option<ScrollableParams>,

    #[serde(default)]
    pub soft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitParams {
    #[serde(default = "default_wait_ms")]
    pub ms: u64,

    #[serde(default)]
    pub label: Option<String>,
}

fn default_wait_ms() -> u64 {
    1000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeatParams {
    #[serde(default)]
    pub times: Option<u32>,
    #[serde(default, rename = "while")]
    pub while_condition: Option<serde_json::Value>,
    pub commands: Vec<TestCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryParams {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    pub commands: Vec<TestCommand>,
}

fn default_max_retries() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFlowParams {
    pub path: Option<String>,

    #[serde(default)]
    pub target: Option<TargetId>,

    /// Variables to pass to the nested flow
    #[serde(default, alias = "env")]
    pub vars: Option<HashMap<String, String>>,

    #[serde(default)]
    pub commands: Option<Vec<TestCommand>>,

    #[serde(default)]
    pub when: Option<serde_json::Value>,

    #[serde(default)]
    pub label: Option<String>,

    #[serde(default)]
    pub optional: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RunFlowParamsInput {
    String(String),
    Struct(RunFlowParams),
}

impl RunFlowParamsInput {
    pub fn into_inner(self) -> RunFlowParams {
        match self {
            Self::String(s) => RunFlowParams {
                path: Some(s),
                target: None,
                vars: None,
                commands: None,
                when: None,
                label: None,
                optional: None,
            },
            Self::Struct(s) => s,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhenParams {
    pub condition: serde_json::Value,
    pub command: Box<TestCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForEachParams {
    pub item: String,
    #[serde(alias = "items")]
    pub r#in: serde_yaml::Value,
    pub commands: Vec<TestCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchParams {
    pub value: String,
    pub cases: HashMap<String, Vec<TestCommand>>,
    #[serde(default)]
    pub default: Option<Vec<TestCommand>>,
}

/// Set a variable for use in subsequent commands
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetVarParams {
    /// Variable name
    pub name: String,

    /// Variable value (can use ${var} syntax for substitution)
    pub value: String,
}

/// Assert a variable has expected value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertVarParams {
    /// Variable name
    pub name: String,

    /// Expected value
    pub expected: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScreenshotParamsInput {
    Struct(ScreenshotParams),
    String(String),
}

impl ScreenshotParamsInput {
    pub fn into_inner(self) -> ScreenshotParams {
        match self {
            Self::Struct(s) => s,
            Self::String(s) => ScreenshotParams { path: s },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotParams {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecordingParamsInput {
    Struct(RecordingParams),
    String(String),
}

impl RecordingParamsInput {
    pub fn into_inner(self) -> RecordingParams {
        match self {
            Self::Struct(s) => s,
            Self::String(s) => RecordingParams { path: s },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecordingParams {
    pub path: String,
}

// GIF Recording params

/// Capture GIF frame params
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureGifFrameParams {
    /// Frame name/identifier
    pub name: String,

    /// Crop region: "left%,top%,width%,height%"
    #[serde(default)]
    pub crop: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CaptureGifFrameParamsInput {
    Struct(CaptureGifFrameParams),
    String(String),
}

impl CaptureGifFrameParamsInput {
    pub fn into_inner(self) -> CaptureGifFrameParams {
        match self {
            Self::Struct(s) => s,
            Self::String(name) => CaptureGifFrameParams { name, crop: None },
        }
    }
}

/// GIF frame input - supports both simple name and name with custom delay
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GifFrameInput {
    /// Just frame name (uses default delay)
    Name(String),
    /// Frame with custom delay
    WithDelay { name: String, delay: u32 },
}

/// Build GIF params
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildGifParams {
    /// List of frames (string or object with delay)
    pub frames: Vec<GifFrameInput>,

    /// Output file path
    pub output: String,

    /// Default delay per frame (ms), default 500
    #[serde(default = "default_gif_delay")]
    pub delay: u32,

    /// Resize width (keeps aspect ratio)
    #[serde(default)]
    pub width: Option<u32>,

    /// Resize height (keeps aspect ratio)
    #[serde(default)]
    pub height: Option<u32>,

    /// Quality: "low", "medium", "high" (default: medium)
    #[serde(default = "default_gif_quality")]
    pub quality: String,

    /// Max colors (2-256), default 128
    #[serde(default = "default_gif_colors")]
    pub colors: u16,

    /// Loop infinite (default true)
    #[serde(default = "default_gif_loop")]
    pub loop_gif: bool,

    /// Specific loop count (overrides loop_gif)
    #[serde(default)]
    pub loop_count: Option<u16>,
}

fn default_gif_delay() -> u32 {
    500
}
fn default_gif_quality() -> String {
    "medium".to_string()
}
fn default_gif_colors() -> u16 {
    128
}
fn default_gif_loop() -> bool {
    true
}

/// Start auto-capture GIF mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartGifCaptureParams {
    /// Capture interval in milliseconds
    #[serde(default = "default_capture_interval")]
    pub interval: u64,

    /// Maximum frames to capture
    #[serde(default = "default_max_frames")]
    pub max_frames: u32,

    /// Resize width for captured frames
    #[serde(default)]
    pub width: Option<u32>,
}

fn default_capture_interval() -> u64 {
    200
}
fn default_max_frames() -> u32 {
    150
}

/// Stop auto-capture and build GIF
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopGifCaptureParams {
    /// Output GIF path
    pub output: String,

    /// Frame delay in ms (default: uses capture interval)
    #[serde(default)]
    pub delay: Option<u32>,

    /// Quality: "low", "medium", "high"
    #[serde(default = "default_gif_quality")]
    pub quality: String,

    /// Loop count (None = infinite)
    #[serde(default)]
    pub loop_count: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportParams {
    pub path: String,

    #[serde(default = "default_report_format")]
    pub format: String,
}

fn default_report_format() -> String {
    "json".to_string()
}

// Web-specific params (Future)

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigateParams {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkParams {
    #[serde(default)]
    pub wifi: Option<bool>,
    #[serde(default)]
    pub data: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAppParams {
    pub app_id: Option<String>,
    #[serde(default = "default_background_duration")]
    pub duration_ms: u64,
}

fn default_background_duration() -> u64 {
    5000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrientationParams {
    pub mode: Orientation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickParams {
    #[serde(default)]
    pub selector: Option<String>,

    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeParams {
    pub text: String,

    #[serde(default)]
    pub selector: Option<String>,
}

// Assuming TestCommand enum definition is elsewhere and AssertVisible is a variant of it.
// Adding the new variant here as per the instruction's implied structure.
// This requires defining SendLarkMessageParams and AssertVisibleParams if they don't exist.
// If TestCommand enum was provided, the variant would be added directly to it.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertVisibleParams {
    // ... fields ...
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub relative: Option<RelativeParams>,
    #[serde(default)]
    pub right_of: Option<String>,
    #[serde(default)]
    pub left_of: Option<String>,
    #[serde(default)]
    pub above: Option<String>,
    #[serde(default)]
    pub below: Option<String>,
    #[serde(default)]
    pub css: Option<String>,
    #[serde(default)]
    pub xpath: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default, alias = "type")]
    pub element_type: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub scrollable: Option<bool>,

    // Assert specific
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub soft: bool,

    #[serde(default, alias = "containsChild")]
    pub contains_child: Option<Box<AssertVisibleParams>>,

    #[serde(default)]
    pub selector: Option<String>,
}

impl TestCommand {
    /// Get a display name for the command
    pub fn display_name(&self) -> String {
        match self {
            TestCommand::E2e(command) => command.display_name().to_string(),
            TestCommand::LaunchApp(p_input) => {
                let p = p_input.clone().map(|pi| pi.into_inner());
                if let Some(params) = p {
                    if let Some(label) = &params.label {
                        return label.clone();
                    }
                    let mut parts = Vec::new();
                    if let Some(app_id) = &params.app_id {
                        parts.push(format!("\"{}\"", app_id));
                    }
                    if params.clear_state {
                        parts.push("clearState".to_string());
                    }
                    if params.clear_keychain {
                        parts.push("clearKeychain".to_string());
                    }
                    if parts.is_empty() {
                        "launchApp".to_string()
                    } else {
                        format!("launchApp({})", parts.join(", "))
                    }
                } else {
                    "launchApp".to_string()
                }
            }
            TestCommand::StopApp => "stopApp".to_string(),
            TestCommand::Find(p) => format!("find(name: \"{}\")", p.name),
            TestCommand::TapOn(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    if let Some(idx) = p.index {
                        format!("tapOn(text: \"{}\", index: {})", text, idx)
                    } else {
                        format!("tapOn(text: \"{}\")", text)
                    }
                } else if let Some(point) = &p.point {
                    format!("tapOn(point: {})", point)
                } else if let Some(id) = &p.id {
                    format!("tapOn(id: \"{}\")", id)
                } else if let Some(el_type) = &p.element_type {
                    let idx = p.index.unwrap_or(0);
                    format!("tapOn(type: \"{}\", index: {})", el_type, idx)
                } else if let Some(regex) = &p.regex {
                    format!("tapOn(regex: \"{}\")", regex)
                } else if let Some(css) = &p.css {
                    format!("tapOn(css: \"{}\")", css)
                } else if let Some(xpath) = &p.xpath {
                    format!("tapOn(xpath: \"{}\")", xpath)
                } else if let Some(placeholder) = &p.placeholder {
                    format!("tapOn(placeholder: \"{}\")", placeholder)
                } else if let Some(role) = &p.role {
                    format!("tapOn(role: \"{}\")", role)
                } else if let Some(image) = &p.image {
                    format!("tapOn(image: \"{}\")", image)
                } else if let Some(rel) = &p.relative {
                    // Show direction and anchor text for better clarity
                    let (dir, anchor) = if let Some(ref input) = rel.right_of {
                        ("rightOf", input.as_string().unwrap_or("..."))
                    } else if let Some(ref input) = rel.left_of {
                        ("leftOf", input.as_string().unwrap_or("..."))
                    } else if let Some(ref input) = rel.above {
                        ("above", input.as_string().unwrap_or("..."))
                    } else if let Some(ref input) = rel.below {
                        ("below", input.as_string().unwrap_or("..."))
                    } else {
                        ("relative", "")
                    };
                    if anchor.is_empty() {
                        format!("tapOn(relative: {})", dir)
                    } else {
                        format!("tapOn({} \"{}\")", dir, anchor)
                    }
                } else {
                    "tapOn".to_string()
                }
            }
            TestCommand::LongPressOn(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    format!("longPressOn(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("longPressOn(id: \"{}\")", id)
                } else if let Some(el_type) = &p.element_type {
                    let idx = p.index.unwrap_or(0);
                    format!("longPressOn(type: \"{}\", index: {})", el_type, idx)
                } else if let Some(point) = &p.point {
                    format!("longPressOn(point: {})", point)
                } else {
                    "longPressOn".to_string()
                }
            }
            TestCommand::DoubleTapOn(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    format!("doubleTapOn(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("doubleTapOn(id: \"{}\")", id)
                } else if let Some(el_type) = &p.element_type {
                    let idx = p.index.unwrap_or(0);
                    format!("doubleTapOn(type: \"{}\", index: {})", el_type, idx)
                } else if let Some(point) = &p.point {
                    format!("doubleTapOn(point: {})", point)
                } else {
                    "doubleTapOn".to_string()
                }
            }
            TestCommand::InputText(params_input) => {
                // Check for label in the input
                if let InputTextParamsInput::Struct(p) = params_input {
                    if let Some(label) = &p.label {
                        return label.clone();
                    }
                }
                format!("inputText(\"{}\")", params_input.text())
            }
            TestCommand::EraseText(_) => "eraseText".to_string(),
            TestCommand::HideKeyboard => "hideKeyboard".to_string(),
            TestCommand::SwipeLeft => "swipeLeft".to_string(),
            TestCommand::SwipeRight => "swipeRight".to_string(),
            TestCommand::SwipeUp => "swipeUp".to_string(),
            TestCommand::SwipeDown => "swipeDown".to_string(),
            TestCommand::ManualScroll(_) => "scroll".to_string(),
            TestCommand::Drag(_) => "drag".to_string(),
            TestCommand::Pinch(p) => format!("pinch({})", p.direction),
            TestCommand::Shove(p) => format!("shove({})", p.direction),
            TestCommand::ScrollUntilVisible(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    format!("scrollUntilVisible(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("scrollUntilVisible(id: \"{}\")", id)
                } else if let Some(regex) = &p.regex {
                    format!("scrollUntilVisible(regex: \"{}\")", regex)
                } else if let Some(el_type) = &p.element_type {
                    format!("scrollUntilVisible(type: \"{}\")", el_type)
                } else if let Some(image) = &p.image {
                    format!("scrollUntilVisible(image: \"{}\")", image)
                } else {
                    "scrollUntilVisible".to_string()
                }
            }
            TestCommand::AssertVisible(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    format!("assertVisible(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("assertVisible(id: \"{}\")", id)
                } else if let Some(regex) = &p.regex {
                    format!("assertVisible(regex: \"{}\")", regex)
                } else if let Some(el_type) = &p.element_type {
                    format!("assertVisible(type: \"{}\")", el_type)
                } else if let Some(css) = &p.css {
                    format!("assertVisible(css: \"{}\")", css)
                } else if let Some(image) = &p.image {
                    format!("assertVisible(image: \"{}\")", image)
                } else {
                    "assertVisible".to_string()
                }
            }
            TestCommand::WaitUntilVisible(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    format!("waitUntilVisible(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("waitUntilVisible(id: \"{}\")", id)
                } else if let Some(regex) = &p.regex {
                    format!("waitUntilVisible(regex: \"{}\")", regex)
                } else if let Some(el_type) = &p.element_type {
                    format!("waitUntilVisible(type: \"{}\")", el_type)
                } else if let Some(css) = &p.css {
                    format!("waitUntilVisible(css: \"{}\")", css)
                } else if let Some(image) = &p.image {
                    format!("waitUntilVisible(image: \"{}\")", image)
                } else {
                    "waitUntilVisible".to_string()
                }
            }
            TestCommand::AssertNotVisible(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    format!("assertNotVisible(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("assertNotVisible(id: \"{}\")", id)
                } else if let Some(regex) = &p.regex {
                    format!("assertNotVisible(regex: \"{}\")", regex)
                } else {
                    "assertNotVisible".to_string()
                }
            }
            TestCommand::WaitUntilNotVisible(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                if let Some(text) = &p.text {
                    format!("waitNotSee(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("waitNotSee(id: \"{}\")", id)
                } else if let Some(regex) = &p.regex {
                    format!("waitNotSee(regex: \"{}\")", regex)
                } else {
                    "waitNotSee".to_string()
                }
            }
            TestCommand::WaitForAnimationToEnd => "waitForAnimationToEnd".to_string(),
            TestCommand::Wait(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(label) = &p.label {
                    return label.clone();
                }
                format!("wait({}ms)", p.ms)
            }
            TestCommand::Repeat(p) => {
                if let Some(times) = p.times {
                    format!("repeat({} times)", times)
                } else if p.while_condition.is_some() {
                    "repeat(while)".to_string()
                } else {
                    "repeat".to_string()
                }
            }
            TestCommand::Retry(p) => format!("retry(max: {})", p.max_retries),
            TestCommand::RunFlow(p_input) => {
                let p = p_input.clone().into_inner();
                if let Some(path) = &p.path {
                    format!("runFlow(\"{}\")", path)
                } else if p.commands.is_some() {
                    "runFlow(inline)".to_string()
                } else {
                    "runFlow".to_string()
                }
            }
            TestCommand::TakeScreenshot(_) => "screenshot".to_string(),
            TestCommand::StartRecording(_) => "startRecording".to_string(),
            TestCommand::StopRecording => "stopRecording".to_string(),
            TestCommand::ExportReport(_) => "exportReport".to_string(),
            TestCommand::Back => "back".to_string(),
            TestCommand::PressHome => "pressHome".to_string(),
            TestCommand::Navigate(_) => "navigate".to_string(),
            TestCommand::Click(_) => "click".to_string(),
            TestCommand::Type(p) => {
                let sel = p.selector.as_deref().unwrap_or("focused");
                format!("type(\"{}\", \"{}\")", p.text, sel)
            }
            TestCommand::TapAt(p) => {
                format!("tapAt({}[{}])", p.element_type, p.index)
            }
            TestCommand::InputAt(p) => {
                format!("inputAt({}[{}], \"{}\")", p.element_type, p.index, p.text)
            }
            TestCommand::RightClick(p) => {
                if let Some(text) = &p.text {
                    format!("rightClick(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("rightClick(id: \"{}\")", id)
                } else if let Some(el_type) = &p.element_type {
                    format!("rightClick(type: \"{}\")", el_type)
                } else if let Some(css) = &p.css {
                    format!("rightClick(css: \"{}\")", css)
                } else {
                    "rightClick".to_string()
                }
            }
            TestCommand::SetVar(p) => {
                format!("setVar({} = \"{}\")", p.name, p.value)
            }
            TestCommand::AssertVar(p) => {
                format!("assertVar({} == \"{}\")", p.name, p.expected)
            }
            TestCommand::Generate(p) => {
                format!("generate({}: {})", p.name, p.data_type)
            }
            TestCommand::HttpRequest(p) => {
                format!("httpRequest({} {})", p.method, p.url)
            }
            TestCommand::OpenLink(url) => {
                format!("openLink(\"{}\")", url)
            }
            TestCommand::FocusApp(target) => {
                format!("focusApp(\"{}\")", target)
            }
            TestCommand::AssertScreenshot(name) => {
                format!("assertScreenshot(\"{}\")", name)
            }
            TestCommand::RunScript(p_input) => {
                let p = p_input.clone().into_inner();
                format!("runScript(\"{}\")", p.command)
            }
            TestCommand::Conditional(p) => {
                if let Some(visible) = &p.condition.visible {
                    format!("conditional(visible: \"{}\")", visible)
                } else if let Some(visible_regex) = &p.condition.visible_regex {
                    format!("conditional(visibleRegex: \"{}\")", visible_regex)
                } else if let Some(not_visible) = &p.condition.not_visible {
                    format!("conditional(notVisible: \"{}\")", not_visible)
                } else if let Some(not_visible_regex) = &p.condition.not_visible_regex {
                    format!("conditional(notVisibleRegex: \"{}\")", not_visible_regex)
                } else {
                    "conditional".to_string()
                }
            }
            TestCommand::MockLocation(p_input) => {
                let p = p_input.clone().into_inner();
                format!("mockLocation(\"{}\")", p.file)
            }
            TestCommand::StopMockLocation => "stopMockLocation".to_string(),
            TestCommand::MockLocationControl(p) => {
                if let Some(speed) = p.speed {
                    format!("mockLocationControl(speed: {})", speed)
                } else if p.pause == Some(true) {
                    "mockLocationControl(pause)".to_string()
                } else if p.resume == Some(true) {
                    "mockLocationControl(resume)".to_string()
                } else {
                    "mockLocationControl".to_string()
                }
            }
            TestCommand::AssertColor(p) => {
                format!("assertColor({}, \"{}\")", p.point, p.color)
            }
            TestCommand::CaptureGifFrame(p_input) => {
                let p = p_input.clone().into_inner();
                format!("captureGifFrame(\"{}\")", p.name)
            }
            TestCommand::BuildGif(p) => {
                format!("buildGif({} frames -> \"{}\")", p.frames.len(), p.output)
            }
            TestCommand::StartGifCapture(p) => {
                format!(
                    "startGifCapture(interval: {}ms, max: {})",
                    p.interval, p.max_frames
                )
            }
            TestCommand::StopGifCapture(p) => {
                format!("stopGifCapture(\"{}\")", p.output)
            }
            TestCommand::RotateScreen(p_input) => {
                let p = p_input.clone().into_inner();
                format!("rotate(\"{}\")", p.mode)
            }
            TestCommand::PressKey(k) => {
                let times = k.times_value();
                let times_str = match times {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(s) => s,
                    _ => "1".to_string(),
                };
                if times_str != "1" {
                    format!("press(\"{}\", times: {})", k.key(), times_str)
                } else {
                    format!("press(\"{}\")", k.key())
                }
            }
            TestCommand::SendLarkMessage(_) => "sendLarkMessage".to_string(),
            TestCommand::PushFile(p) => format!("pushFile({} -> {})", p.source, p.destination),
            TestCommand::PullFile(p) => format!("pullFile({} -> {})", p.source, p.destination),
            TestCommand::ClearAppData(pkg) => format!("clearAppData({})", pkg),
            TestCommand::SetClipboard(t) => format!("setClipboard(\"{}\")", t),
            TestCommand::GetClipboard(p) => format!("getClipboard({})", p.name),
            TestCommand::AssertClipboard(e) => format!("assertClipboard(\"{}\")", e),

            TestCommand::AssertTrue(p) => match p {
                AssertTrueParams::Condition(c) => format!("assertTrue({})", c.condition),
                AssertTrueParams::Expression(expr) => format!("assertTrue({})", expr),
            },
            TestCommand::EvalScript(expr) => format!("evalScript({})", expr),
            TestCommand::CopyTextFrom(p) => {
                if let Some(text) = &p.text {
                    format!("copyTextFrom(text: \"{}\")", text)
                } else if let Some(id) = &p.id {
                    format!("copyTextFrom(id: \"{}\")", id)
                } else {
                    "copyTextFrom".to_string()
                }
            }
            TestCommand::PasteText => "pasteText".to_string(),
            TestCommand::InputRandomEmail => "inputRandomEmail".to_string(),
            TestCommand::InputRandomNumber(p) => {
                if let Some(params) = p {
                    if let Some(len) = params.length {
                        format!("inputRandomNumber(length: {})", len)
                    } else {
                        "inputRandomNumber".to_string()
                    }
                } else {
                    "inputRandomNumber".to_string()
                }
            }
            TestCommand::InputRandomPersonName => "inputRandomPersonName".to_string(),
            TestCommand::InputRandomText(p) => {
                if let Some(params) = p {
                    if let Some(len) = params.length {
                        format!("inputRandomText(length: {})", len)
                    } else {
                        "inputRandomText".to_string()
                    }
                } else {
                    "inputRandomText".to_string()
                }
            }
            TestCommand::ExtendedWaitUntil(p) => {
                format!("extendedWaitUntil(timeout: {}ms)", p.timeout)
            }
            TestCommand::DbQuery(p) => {
                format!("dbQuery(query: \"{}\")", p.query)
            }
            TestCommand::SetNetwork(p) => {
                let mut parts = Vec::new();
                if let Some(w) = p.wifi {
                    parts.push(format!("wifi: {}", w));
                }
                if let Some(d) = p.data {
                    parts.push(format!("data: {}", d));
                }
                format!("setNetwork({})", parts.join(", "))
            }
            TestCommand::ToggleAirplaneMode => "airplaneMode".to_string(),
            TestCommand::OpenNotifications => "openNotifications".to_string(),
            TestCommand::OpenQuickSettings => "openQuickSettings".to_string(),
            TestCommand::SetVolume(v) => format!("setVolume({})", v),
            TestCommand::LockDevice => "lockDevice".to_string(),
            TestCommand::UnlockDevice => "unlockDevice".to_string(),
            TestCommand::InstallApp(path) => format!("installApp(\"{}\")", path),
            TestCommand::UninstallApp(pkg) => format!("uninstallApp(\"{}\")", pkg),
            TestCommand::BackgroundApp(p) => format!(
                "backgroundApp({}, {}ms)",
                p.app_id.as_deref().unwrap_or("current"),
                p.duration_ms
            ),
            TestCommand::SetOrientation(p) => format!("setOrientation({:?})", p.mode),
            TestCommand::WaitForLocation(p) => {
                format!(
                    "waitForLocation({:.4}, {:.4}, tol: {:.1})",
                    p.lat, p.lon, p.tolerance
                )
            }
            TestCommand::WaitForMockCompletion(p) => {
                if let Some(t) = p.timeout {
                    format!("waitForMockCompletion(timeout: {}ms)", t)
                } else {
                    "waitForMockCompletion".to_string()
                }
            }

            // Performance & Load Testing
            TestCommand::StartProfiling(_) => "startProfiling".to_string(),
            TestCommand::StopProfiling(_) => "stopProfiling".to_string(),
            TestCommand::AssertPerformance(p) => {
                format!("assertPerformance({} check {})", p.metric, p.limit)
            }
            TestCommand::SetCpuThrottling(rate) => format!("setCpuThrottling({}x)", rate),
            TestCommand::SetNetworkConditions(profile) => {
                format!("setNetworkConditions(\"{}\")", profile)
            }
            TestCommand::SelectDisplay(id) => format!("selectDisplay({})", id),
            TestCommand::SetLocale(locale) => format!("setLocale(\"{}\")", locale),
            TestCommand::PlayMedia(p) => format!("playMedia(\"{}\")", p.file),
            TestCommand::StopMedia => "stopMedia".to_string(),
            TestCommand::StartAudioCapture(p) => format!("startAudioCapture({}ms)", p.duration),
            TestCommand::StopAudioCapture => "stopAudioCapture".to_string(),
            TestCommand::VerifyAudioDucking(_) => "verifyAudioDucking".to_string(),
            TestCommand::AssertDeviceState(p) => {
                format!(
                    "assertDeviceState(button: \"{}\", expect: \"{}\")",
                    p.button, p.expect
                )
            }
            TestCommand::WaitDeviceState(p) => {
                format!(
                    "waitDeviceState(button: \"{}\", expect: \"{}\")",
                    p.button, p.expect
                )
            }
            TestCommand::AssertDeviceTransition(p) => format!(
                "assertDeviceTransition(button: \"{}\", {} → {})",
                p.button, p.from, p.to
            ),
            TestCommand::WaitLedPattern(p) => format!(
                "waitLedPattern(button: \"{}\", expect: \"{}\", count: {})",
                p.button, p.expect, p.count
            ),
            TestCommand::GetDeviceState(p) => {
                format!("getDeviceState(saveAs: \"{}\")", p.save_as)
            }

            TestCommand::HwConnect(p) => format!("hwConnect(port: \"{}\")", p.port),
            TestCommand::HwPing(p) => format!("hwPing(nodeId: {:?})", p.node_id),
            TestCommand::HwDisconnect => "hwDisconnect".to_string(),
            TestCommand::HwClick(p) => {
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!("hwClick({} [ch {}])", btn.trim(), p.resolved_channel())
                    } else {
                        format!("hwClick(channel: {})", p.resolved_channel())
                    }
                } else {
                    format!("hwClick(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwRepeatClick(p) => {
                let count_str = format!(", count: {}", p.count);
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!(
                            "hwRepeatClick({} [ch {}]{})",
                            btn.trim(),
                            p.resolved_channel(),
                            count_str
                        )
                    } else {
                        format!(
                            "hwRepeatClick(channel: {}{})",
                            p.resolved_channel(),
                            count_str
                        )
                    }
                } else {
                    format!(
                        "hwRepeatClick(channel: {}{})",
                        p.resolved_channel(),
                        count_str
                    )
                }
            }
            TestCommand::HwPress(p) => {
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!("hwPress({} [ch {}])", btn.trim(), p.resolved_channel())
                    } else {
                        format!("hwPress(channel: {})", p.resolved_channel())
                    }
                } else {
                    format!("hwPress(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwRelease(p) => {
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!("hwRelease({} [ch {}])", btn.trim(), p.resolved_channel())
                    } else {
                        format!("hwRelease(channel: {})", p.resolved_channel())
                    }
                } else {
                    format!("hwRelease(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwRotate(p) => format!(
                "hwRotate(channel: {}, angle: {})",
                p.resolved_channel(),
                p.angle
            ),
            TestCommand::HwReadServo(p) => {
                format!("hwReadServo(channel: {})", p.resolved_channel())
            }
            TestCommand::HwReadRelay(p) => {
                format!("hwReadRelay(channel: {})", p.resolved_channel())
            }
            TestCommand::HwReadColor(p) => {
                format!("hwReadColor(channel: {})", p.resolved_channel())
            }
            TestCommand::HwReadSensorLight(p) => format!(
                "hwReadSensorLight(channel: {})",
                p.as_ref().map(|x| x.resolved_channel()).unwrap_or(1)
            ),
            TestCommand::HwPowerOn(p) => {
                let chs = p.resolved_channels();
                if let Some(ref name) = p.button {
                    if name.trim().parse::<u8>().is_err() {
                        format!("hwPowerOn({} [ch {:?}])", name.trim(), chs)
                    } else {
                        format!("hwPowerOn(channels: {:?})", chs)
                    }
                } else {
                    format!("hwPowerOn(channels: {:?})", chs)
                }
            }
            TestCommand::HwPowerOff(p) => {
                let chs = p.resolved_channels();
                if let Some(ref name) = p.button {
                    if name.trim().parse::<u8>().is_err() {
                        format!("hwPowerOff({} [ch {:?}])", name.trim(), chs)
                    } else {
                        format!("hwPowerOff(channels: {:?})", chs)
                    }
                } else {
                    format!("hwPowerOff(channels: {:?})", chs)
                }
            }
            TestCommand::HwPowerOffAll => "hwPowerOffAll".to_string(),
            TestCommand::HwPowerCycle(p) => {
                let chs = p.resolved_channels();
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!("hwPowerCycle({} [ch {:?}])", btn.trim(), chs)
                    } else {
                        format!("hwPowerCycle(channels: {:?})", chs)
                    }
                } else {
                    format!("hwPowerCycle(channels: {:?})", chs)
                }
            }
            TestCommand::HwSeeLed(p) => {
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!("hwSeeLed({} [ch {}])", btn.trim(), p.resolved_channel())
                    } else {
                        format!("hwSeeLed(channel: {})", p.resolved_channel())
                    }
                } else {
                    format!("hwSeeLed(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwSeeLedBlink(p) => {
                if let Some(ref btn) = p.button {
                    format!("hwSeeLedBlink({})", btn.trim())
                } else {
                    format!("hwSeeLedBlink(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwSeeNativeLedBlink(p) => {
                if let Some(ref btn) = p.button {
                    format!("hwSeeNativeLedBlink({})", btn.trim())
                } else {
                    format!("hwSeeNativeLedBlink(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwSeeLedOff(p) => {
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!("hwSeeLedOff({} [ch {}])", btn.trim(), p.resolved_channel())
                    } else {
                        format!("hwSeeLedOff(channel: {})", p.resolved_channel())
                    }
                } else {
                    format!("hwSeeLedOff(channel: {})", p.resolved_channel())
                }
            }

            TestCommand::HwConfigureServo(p) => {
                format!("hwConfigureServo(channel: {})", p.resolved_channel())
            }
            TestCommand::HwReleaseAll => "hwReleaseAll".to_string(),
            TestCommand::HwStartRepeatClick(p) => {
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!(
                            "hwStartRepeatClick({} [ch {}])",
                            btn.trim(),
                            p.resolved_channel()
                        )
                    } else {
                        format!("hwStartRepeatClick(channel: {})", p.resolved_channel())
                    }
                } else {
                    format!("hwStartRepeatClick(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwStopRepeatClick(p) => {
                if let Some(ref btn) = p.button {
                    if btn.trim().parse::<u8>().is_err() {
                        format!(
                            "hwStopRepeatClick({} [ch {}])",
                            btn.trim(),
                            p.resolved_channel()
                        )
                    } else {
                        format!("hwStopRepeatClick(channel: {})", p.resolved_channel())
                    }
                } else {
                    format!("hwStopRepeatClick(channel: {})", p.resolved_channel())
                }
            }
            TestCommand::HwSensorLight(p) => {
                format!("hwSensorLight(channel: {})", p.resolved_channel())
            }
            TestCommand::HwSetBrightnessThresholds(p) => format!(
                "hwSetBrightnessThresholds(channel: {})",
                p.resolved_channel()
            ),
            TestCommand::HwWaitForBrightness(p) => {
                format!("hwWaitForBrightness(channel: {})", p.resolved_channel())
            }
            TestCommand::HwWaitForCct(p) => {
                format!("hwWaitForCct(channel: {})", p.resolved_channel())
            }
            TestCommand::HwCalibrateColor(p) => format!(
                "hwCalibrateColor(channel: {}, color: \"{}\")",
                p.resolved_channel(),
                p.color
            ),
            TestCommand::HwCalibrateBrightness(p) => format!(
                "hwCalibrateBrightness(channel: {}, mode: \"{}\")",
                p.resolved_channel(),
                p.mode
            ),
            TestCommand::HwAddCctPoint(p) => format!(
                "hwAddCctPoint(channel: {}, knownKelvin: {})",
                p.resolved_channel(),
                p.known_kelvin
            ),
            TestCommand::HwSaveCalibration => "hwSaveCalibration".to_string(),
            TestCommand::HwLoadCalibration => "hwLoadCalibration".to_string(),
            TestCommand::HwResetCalibration => "hwResetCalibration".to_string(),
            TestCommand::HwEraseCalibration => "hwEraseCalibration".to_string(),
            TestCommand::HwSafeState => "hwSafeState".to_string(),
            TestCommand::HwDiagnostics => "hwDiagnostics".to_string(),
            TestCommand::RunPython(_) => "runPython".to_string(),
            TestCommand::SetWindowSize(p) => match p.to_dimensions() {
                Some((w, h)) => format!("setWindowSize({}x{})", w, h),
                None => "setWindowSize".to_string(),
            },
            TestCommand::When(p) => format!("{} (when: {:?})", p.command.display_name(), p.condition),
            TestCommand::ForEach(p) => format!("forEach(item: {})", p.item),
            TestCommand::Match(p) => format!("match({})", p.value),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateParams {
    pub name: String,

    #[serde(rename = "type")]
    pub data_type: String, // uuid, email, phone, name, address, number, date

    #[serde(default)]
    pub format: Option<String>, // format string for date, or min-max for number "1-100"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestParams {
    pub url: String,
    pub method: String, // GET, POST, PUT, DELETE

    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,

    #[serde(default)]
    pub body: Option<serde_yaml::Value>, // JSON/YAML value or string

    #[serde(default)]
    pub save_response: Option<HashMap<String, String>>, // map json path -> var name

    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendLarkMessageParams {
    pub webhook: String,

    #[serde(default)]
    pub secret: Option<String>,

    #[serde(default)]
    pub title: Option<String>,

    pub content: String,

    #[serde(default)]
    pub status: Option<String>,

    #[serde(default)]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RunScriptParamsInput {
    Struct(RunScriptParams),
    String(String),
}

impl RunScriptParamsInput {
    pub fn into_inner(self) -> RunScriptParams {
        match self {
            Self::Struct(s) => s,
            Self::String(s) => RunScriptParams {
                command: s,
                args: None,
                save_output: None,
                timeout_ms: None,
                fail_on_error: false,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunScriptParams {
    pub command: String,

    #[serde(default)]
    pub args: Option<Vec<String>>,

    #[serde(default)]
    pub save_output: Option<String>, // variable name to save stdout

    #[serde(default)]
    pub timeout_ms: Option<u64>,

    #[serde(default)]
    pub fail_on_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalParams {
    pub condition: Condition,
    pub then: serde_yaml::Value,
    #[serde(default, rename = "else")]
    pub else_cmd: Option<serde_yaml::Value>,
}

/// Speed simulation mode
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SpeedMode {
    /// Constant speed (linear interpolation)
    #[default]
    Linear,
    /// Speed with random noise within +/- speedNoise km/h
    Noise,
}

/// Mock location parameters for GPS simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockLocationParams {
    /// Unique name for this mock instance (optional, allows running multiple mocks)
    #[serde(default)]
    pub name: Option<String>,

    /// Path to GPX, KML, or JSON file
    pub file: String,

    /// Override speed in km/h (ignores timestamps in file)
    #[serde(default)]
    pub speed: Option<f64>,

    /// Speed simulation mode: linear (constant) or noise (variable)
    #[serde(default)]
    pub speed_mode: SpeedMode,

    /// Speed noise range in km/h (used when speedMode is noise), e.g., 5.0 means +/- 5 km/h
    #[serde(default)]
    pub speed_noise: Option<f64>,

    /// Loop the route continuously
    #[serde(default, rename = "loop")]
    pub loop_route: bool,

    /// Start from specific waypoint index (0-based)
    #[serde(default)]
    pub start_index: Option<u32>,

    /// Update interval in milliseconds (default: 1000)
    #[serde(default)]
    pub interval_ms: Option<u64>,

    /// Fixed altitude in meters (overrides altitude from file)
    #[serde(default)]
    pub altitude: Option<f64>,
}

/// Mock location control parameters for dynamic speed adjustment
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockLocationControlParams {
    /// Name of the mock instance to control (optional, defaults to default instance)
    #[serde(default)]
    pub name: Option<String>,

    /// New speed in km/h
    #[serde(default)]
    pub speed: Option<f64>,

    /// Speed simulation mode
    #[serde(default)]
    pub speed_mode: Option<SpeedMode>,

    /// Speed noise range in km/h
    #[serde(default)]
    pub speed_noise: Option<f64>,

    /// Pause the mock location playback
    #[serde(default)]
    pub pause: Option<bool>,

    /// Resume the mock location playback
    #[serde(default)]
    pub resume: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MockLocationParamsInput {
    Struct(MockLocationParams),
    String(String),
}

impl MockLocationParamsInput {
    pub fn into_inner(self) -> MockLocationParams {
        match self {
            Self::Struct(s) => s,
            Self::String(file) => MockLocationParams {
                name: None,
                file,
                speed: None,
                speed_mode: SpeedMode::Linear,
                speed_noise: None,
                loop_route: false,
                start_index: None,
                interval_ms: None,
                altitude: None,
            },
        }
    }
}

/// Assert color at a specific point on screen
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertColorParams {
    /// Point to check: "540,960" (absolute) or "50%,50%" (percentage)
    pub point: String,

    /// Expected color: "#4CAF50" (hex) or "green" (named)
    pub color: String,

    /// Color tolerance in percentage (0-100), default 10%
    #[serde(default = "default_color_tolerance")]
    pub tolerance: f64,
}

fn default_color_tolerance() -> f64 {
    10.0
}

impl AssertColorParams {
    /// Parse point string to (x, y) coordinates
    /// Supports: "540,960" (absolute) or "50%,50%" (percentage of screen)
    pub fn parse_point(&self, screen_width: u32, screen_height: u32) -> Option<(i32, i32)> {
        let parts: Vec<&str> = self.point.split(',').collect();
        if parts.len() != 2 {
            return None;
        }

        let x_str = parts[0].trim();
        let y_str = parts[1].trim();

        let x = if x_str.ends_with('%') {
            let pct: f64 = x_str.trim_end_matches('%').parse().ok()?;
            (screen_width as f64 * pct / 100.0) as i32
        } else {
            x_str.parse().ok()?
        };

        let y = if y_str.ends_with('%') {
            let pct: f64 = y_str.trim_end_matches('%').parse().ok()?;
            (screen_height as f64 * pct / 100.0) as i32
        } else {
            y_str.parse().ok()?
        };

        Some((x, y))
    }

    /// Parse color string to RGB values
    /// Supports hex "#RRGGBB" or named colors
    pub fn parse_color(&self) -> Option<(u8, u8, u8)> {
        let color = self.color.trim().to_lowercase();

        // Named colors
        match color.as_str() {
            "red" => return Some((255, 0, 0)),
            "green" => return Some((0, 255, 0)),
            "blue" => return Some((0, 0, 255)),
            "white" => return Some((255, 255, 255)),
            "black" => return Some((0, 0, 0)),
            "yellow" => return Some((255, 255, 0)),
            "orange" => return Some((255, 165, 0)),
            "gray" | "grey" => return Some((128, 128, 128)),
            "cyan" => return Some((0, 255, 255)),
            "magenta" => return Some((255, 0, 255)),
            _ => {}
        }

        // Hex color #RRGGBB or #RGB
        if color.starts_with('#') {
            let hex = color.trim_start_matches('#');
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                return Some((r, g, b));
            } else if hex.len() == 3 {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                return Some((r, g, b));
            }
        }

        None
    }

    /// Calculate color distance as percentage (0-100)
    pub fn color_distance(c1: (u8, u8, u8), c2: (u8, u8, u8)) -> f64 {
        let dr = (c1.0 as f64 - c2.0 as f64).powi(2);
        let dg = (c1.1 as f64 - c2.1 as f64).powi(2);
        let db = (c1.2 as f64 - c2.2 as f64).powi(2);
        // Max possible distance is sqrt(3 * 255^2) = 441.67
        // Normalize to 0-100%
        ((dr + dg + db).sqrt() / 441.67) * 100.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    #[serde(default)]
    pub visible: Option<String>,
    #[serde(default)]
    pub visible_regex: Option<String>,
    #[serde(default)]
    pub not_visible: Option<String>,
    #[serde(default)]
    pub not_visible_regex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RelativeAnchorInput {
    String(String),
    Struct(AnchorParams),
}

impl RelativeAnchorInput {
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            Self::Struct(p) => p.text.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RelativeParams {
    #[serde(alias = "rightOf")]
    pub right_of: Option<RelativeAnchorInput>,
    #[serde(alias = "leftOf")]
    pub left_of: Option<RelativeAnchorInput>,
    #[serde(alias = "aboveOf")]
    pub above: Option<RelativeAnchorInput>,
    #[serde(alias = "belowOf")]
    pub below: Option<RelativeAnchorInput>,
    #[serde(alias = "maxDistance")]
    pub max_dist: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum RelativeDirection {
    LeftOf,
    RightOf,
    Above,
    #[default]
    Below,
    Near,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnchorParams {
    /// Reference to a pre-defined selector variable (from 'find' command)
    #[serde(default)]
    pub element: Option<String>,

    #[serde(default)]
    pub text: Option<String>,

    #[serde(default, alias = "textRegex")]
    pub regex: Option<String>,

    #[serde(default, alias = "resourceId")]
    pub id: Option<String>,

    #[serde(default)]
    pub css: Option<String>,

    #[serde(default)]
    pub xpath: Option<String>,

    #[serde(default)]
    pub placeholder: Option<String>,

    #[serde(default)]
    pub role: Option<String>,

    /// Accessibility description/content-desc selector
    #[serde(
        default,
        alias = "desc",
        alias = "contentDesc",
        alias = "accessibilityId"
    )]
    pub description: Option<String>,

    #[serde(default, alias = "type")]
    pub element_type: Option<String>,

    #[serde(default)]
    pub image: Option<String>,

    #[serde(default)]
    pub exact: bool,

    #[serde(default)]
    pub index: Option<u32>,
}

impl Default for WaitParams {
    fn default() -> Self {
        Self {
            ms: default_wait_ms(),
            label: None,
        }
    }
}

impl Default for ScrollUntilVisibleParams {
    fn default() -> Self {
        Self {
            label: None,
            text: None,
            regex: None,
            relative: None,
            right_of: None,
            left_of: None,
            above: None,
            below: None,
            id: None,
            css: None,
            xpath: None,
            placeholder: None,
            description: None,
            role: None,
            max_scrolls: default_max_scrolls(),
            direction: None,
            element_type: None,
            image: None,
            ocr: None,
            from: None,
            scrollable: None,
            timeout: None,
        }
    }
}

pub fn is_regex_string(s: &str) -> bool {
    s.contains(".*")
        || s.contains(".+")
        || (s.starts_with('^') && s.ends_with('$'))
        || s.contains(r"\d+")
        || s.contains(r"\d{")
        || s.contains('[')
        || s.contains('(')
        || s.contains('|')
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TapParamsInput {
    String(String),
    Struct(TapParams),
}
impl TapParamsInput {
    pub fn into_inner(self) -> TapParams {
        match self {
            TapParamsInput::String(s) => {
                if is_regex_string(&s) {
                    TapParams {
                        regex: Some(s),
                        ..Default::default()
                    }
                } else {
                    TapParams {
                        text: Some(s),
                        ..Default::default()
                    }
                }
            }
            TapParamsInput::Struct(s) => s,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AssertParamsInput {
    String(String),
    Struct(AssertParams),
}
impl AssertParamsInput {
    pub fn into_inner(self) -> AssertParams {
        match self {
            AssertParamsInput::String(s) => {
                if is_regex_string(&s) {
                    AssertParams {
                        regex: Some(s),
                        ..Default::default()
                    }
                } else {
                    AssertParams {
                        text: Some(s),
                        ..Default::default()
                    }
                }
            }
            AssertParamsInput::Struct(s) => s,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WaitParamsInput {
    Number(u64),
    Struct(WaitParams),
}
impl WaitParamsInput {
    pub fn into_inner(self) -> WaitParams {
        match self {
            WaitParamsInput::Number(n) => WaitParams { ms: n, label: None },
            WaitParamsInput::Struct(s) => s,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScrollUntilVisibleInput {
    String(String),
    Struct(ScrollUntilVisibleParams),
}
impl ScrollUntilVisibleInput {
    pub fn into_inner(self) -> ScrollUntilVisibleParams {
        match self {
            ScrollUntilVisibleInput::String(s) => {
                if is_regex_string(&s) {
                    ScrollUntilVisibleParams {
                        regex: Some(s),
                        label: None,
                        ..Default::default()
                    }
                } else {
                    ScrollUntilVisibleParams {
                        text: Some(s),
                        label: None,
                        ..Default::default()
                    }
                }
            }
            ScrollUntilVisibleInput::Struct(s) => s,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnchorParamsInput {
    String(String),
    Struct(AnchorParams),
}
impl AnchorParamsInput {
    pub fn into_inner(self) -> AnchorParams {
        match self {
            AnchorParamsInput::String(s) => {
                if is_regex_string(&s) {
                    AnchorParams {
                        regex: Some(s),
                        ..Default::default()
                    }
                } else {
                    AnchorParams {
                        text: Some(s),
                        ..Default::default()
                    }
                }
            }
            AnchorParamsInput::Struct(s) => s,
        }
    }
}
impl Default for AnchorParamsInput {
    fn default() -> Self {
        AnchorParamsInput::Struct(AnchorParams::default())
    }
}
// Device Control

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RotationParamsInput {
    Struct(RotationParams),
    String(String),
}

impl RotationParamsInput {
    pub fn into_inner(self) -> RotationParams {
        match self {
            Self::Struct(s) => s,
            Self::String(s) => RotationParams { mode: s },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RotationParams {
    /// "portrait" or "landscape"
    pub mode: String,
}

// File Management

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferParams {
    pub source: String,
    pub destination: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbQueryParams {
    pub connection: String,
    pub query: String,
    #[serde(default)]
    pub params: Option<Vec<String>>,
    #[serde(default)]
    pub save: Option<HashMap<String, String>>,
}
