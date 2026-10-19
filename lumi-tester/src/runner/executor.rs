use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;
use uuid::Uuid;

use super::context::TestContext;
use super::events::{ConsoleEventListener, EventEmitter, JsonlEventListener, TestEvent};
use super::state::{CommandState, FlowState, TestSessionState};
use crate::driver::traits::PlatformDriver;
use crate::hardware::traits::*;
use crate::parser::types::TestCommand;
use crate::parser::yaml::{parse_commands_from_value, parse_test_file};
use serde_json;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};

pub struct TestExecutor {
    driver: Box<dyn PlatformDriver>,
    pub context: TestContext,
    session: TestSessionState,
    emitter: EventEmitter,
    continue_on_failure: bool,
    /// GIF frames storage: name -> PNG bytes
    gif_frames: HashMap<String, Vec<u8>>,
    /// Auto-capture GIF state
    auto_capture_frames: Vec<Vec<u8>>,
    auto_capture_active: bool,
    auto_capture_interval: u64,
    auto_capture_max: u32,
    auto_capture_width: Option<u32>,
    auto_capture_last_time: std::time::Instant,
    depth: usize,
    target_tags: Option<Vec<String>>,
    soft_errors: Vec<String>,
    video_enabled: bool,
    #[allow(dead_code)]
    snapshot_enabled: bool,
    report_enabled: bool,
    /// Header-level camera configs by name (hardware verification).
    camera_configs: HashMap<String, crate::parser::types::CameraFlowConfig>,
    /// Lazily-started warm camera streams by name (reused across commands).
    camera_sessions: HashMap<String, crate::camera::CameraSession>,
    /// Observe web views already spawned, keyed by camera config identity and
    /// valued by their assigned local port.
    camera_observe_started: HashMap<String, u16>,
    camera_observe_tasks: HashMap<String, tokio::task::JoinHandle<()>>,
    camera_observe_next_port: u16,
    hardware_controller: Option<crate::hardware::HardwareController>,
    pub auto_power_off: bool,
    /// Cache of parsed subflow YAML by resolved path, so a subflow invoked repeatedly
    /// (e.g. inside a `repeat` loop) is only read and parsed from disk once per run.
    flow_cache: HashMap<std::path::PathBuf, crate::parser::types::TestFlow>,
    pub healer: crate::runner::healer::SelfHealer,
}

/// Resolve `align` and `offset` from TapParams into (x_pct, y_pct) as 0.0..1.0.
/// Returns None if no align/offset is specified (use default center).
fn resolve_element_offset(align: &Option<String>, offset: &Option<String>) -> Option<(f64, f64)> {
    // offset takes priority over align
    if let Some(ref off) = offset {
        let parts: Vec<&str> = off.split(',').collect();
        if parts.len() == 2 {
            let x = parts[0]
                .trim()
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap_or(50.0)
                / 100.0;
            let y = parts[1]
                .trim()
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap_or(50.0)
                / 100.0;
            return Some((x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)));
        }
    }

    if let Some(ref a) = align {
        let (x, y) = match a.to_lowercase().as_str() {
            "left" => (0.10, 0.50),
            "right" => (0.90, 0.50),
            "top" => (0.50, 0.10),
            "bottom" => (0.50, 0.90),
            "center" => (0.50, 0.50),
            _ => return None,
        };
        return Some((x, y));
    }

    None
}

#[derive(Debug, Clone, Default)]
struct FailureArtifacts {
    screenshot_path: Option<String>,
    ui_hierarchy_path: Option<String>,
    log_path: Option<String>,
}

fn resolve_camera_config_vars(
    context: &TestContext,
    configs: HashMap<String, crate::parser::types::CameraFlowConfig>,
) -> HashMap<String, crate::parser::types::CameraFlowConfig> {
    configs
        .into_iter()
        .map(|(name, mut cfg)| {
            cfg.rtsp = context.substitute_vars(&cfg.rtsp);
            cfg.server = cfg.server.map(|s| context.substitute_vars(&s));
            cfg.profile = cfg.profile.map(|p| context.substitute_vars(&p));
            cfg.transport = cfg.transport.map(|t| context.substitute_vars(&t));
            (name, cfg)
        })
        .collect()
}

fn camera_view_url(server_url: &str) -> String {
    format!("{}/view", server_url.trim().trim_end_matches('/'))
}

fn open_camera_view(url: &str) {
    #[cfg(target_os = "macos")]
    let command = ("open", vec![url]);
    #[cfg(target_os = "linux")]
    let command = ("xdg-open", vec![url]);
    #[cfg(target_os = "windows")]
    let command = ("cmd", vec!["/C", "start", "", url]);

    let _ = std::process::Command::new(command.0)
        .args(command.1)
        .spawn();
}

impl TestExecutor {
    pub fn new(
        driver: Box<dyn PlatformDriver>,
        output_dir: Option<&Path>,
        continue_on_failure: bool,
        record: bool,
        snapshot: bool,
        report: bool,
        target_tags: Option<Vec<String>>,
    ) -> Self {
        Self::new_with_events(
            driver,
            output_dir,
            continue_on_failure,
            record,
            snapshot,
            report,
            target_tags,
            false,
        )
    }

    pub fn new_with_events(
        driver: Box<dyn PlatformDriver>,
        output_dir: Option<&Path>,
        continue_on_failure: bool,
        record: bool,
        snapshot: bool,
        report: bool,
        target_tags: Option<Vec<String>>,
        events_jsonl: bool,
    ) -> Self {
        let (emitter, receiver) = EventEmitter::new();
        let device_id = driver.device_serial();

        let context = TestContext::new(Path::new("."), output_dir, continue_on_failure, device_id);

        // Start console listener in background
        tokio::spawn(ConsoleEventListener::listen(receiver));

        if events_jsonl {
            let events_receiver = emitter.subscribe();
            let events_path = context.output_path("events.jsonl");
            tokio::spawn(async move {
                if let Err(e) = JsonlEventListener::listen(events_receiver, events_path).await {
                    eprintln!("Failed to write events JSONL: {}", e);
                }
            });
        }

        let session_id = format!("session_{}", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"));
        let mut session = TestSessionState::new(&session_id);
        session.start();
        emitter.emit(TestEvent::SessionStarted { session_id });

        Self {
            driver,
            context,
            session,
            emitter,
            continue_on_failure,
            depth: 0,
            gif_frames: HashMap::new(),
            auto_capture_frames: Vec::new(),
            auto_capture_active: false,
            auto_capture_interval: 200,
            auto_capture_max: 150,
            auto_capture_width: None,
            auto_capture_last_time: std::time::Instant::now(),
            target_tags,
            soft_errors: Vec::new(),
            video_enabled: record,
            snapshot_enabled: snapshot,
            report_enabled: report,
            camera_configs: HashMap::new(),
            camera_sessions: HashMap::new(),
            camera_observe_started: HashMap::new(),
            camera_observe_tasks: HashMap::new(),
            camera_observe_next_port: 9444,
            hardware_controller: None,
            auto_power_off: false,
            flow_cache: HashMap::new(),
            healer: crate::runner::healer::SelfHealer::new(true),
        }
    }

    /// Set session name prefix with the flow or test suite name
    pub fn with_target_name(mut self, target_name: Option<&str>) -> Self {
        if let Some(target) = target_name {
            let safe: String = target
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
                .collect();
            let clean = safe.trim_matches('_');
            if !clean.is_empty() {
                let session_id = format!(
                    "session_{}_{}",
                    clean,
                    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
                );
                self.session.session_id = session_id.clone();
                self.emitter.emit(TestEvent::SessionStarted { session_id });
            }
        }
        self
    }

    /// Subscribe to test execution events
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<TestEvent> {
        self.emitter.subscribe()
    }

    /// Run a single test file
    pub async fn run_file(
        &mut self,
        path: &Path,
        command_index: Option<usize>,
        from_command_index: Option<usize>,
        command_name: Option<&str>,
    ) -> Result<()> {
        // Update base directory for relative path resolution
        if let Some(parent) = path.parent() {
            self.context.base_dir = parent.to_path_buf();
        }

        // Parse the test file
        let flow = parse_test_file(path)?;

        // If flow is explicitly marked as skip / manual / disabled, and no single command index is targeted
        if flow.skip.unwrap_or(false) && command_index.is_none() && from_command_index.is_none() {
            self.emitter.emit(TestEvent::Log {
                message: format!(
                    "{} Skipping flow '{}' (marked as skip / manual)",
                    "⏭️".yellow(),
                    path.display()
                ),
                depth: self.depth,
            });
            return Ok(());
        }

        // Filter by tags if specified
        if let Some(ref required_tags) = self.target_tags {
            let matches_all = required_tags.iter().all(|req| flow.tags.contains(req));
            if !matches_all {
                self.emitter.emit(TestEvent::Log {
                    message: format!(
                        "{} Skipping flow due to tag mismatch. Required: {:?}, Flow tags: {:?}",
                        "ℹ".blue(),
                        required_tags,
                        flow.tags
                    ),
                    depth: self.depth,
                });
                return Ok(());
            }
        }

        // Update context from flow header
        self.context.update_from_flow(&flow);

        // Let the driver know the target app even if `launchApp` itself never runs
        // this session (e.g. `--command-index` re-running a single later command
        // while the app is already open) - see `PlatformDriver::set_current_app_id`.
        if let Some(app_id) = self.context.app_id.clone() {
            self.driver.set_current_app_id(&app_id).await;
        }

        // Pick up camera (hardware verification) configs; reset prior streams
        // when this flow declares its own cameras.
        if let Some(cameras) = flow.cameras.clone() {
            self.stop_observe_views();
            self.camera_configs = resolve_camera_config_vars(&self.context, cameras.into_map());
            self.camera_sessions.clear();
            self.maybe_start_observe_views();
        } else if self.depth == 0 {
            self.stop_observe_views();
            self.camera_configs.clear();
            self.camera_sessions.clear();
        }
        self.driver.set_active_app(flow.app_id.as_deref());
        self.driver
            .set_desktop_state(flow.desktop_state.clone(), &self.context.base_dir)?;

        if let Some(ref ws) = flow.window_size {
            if let Some((w, h)) = ws.to_dimensions() {
                let _ = self.driver.set_window_size(w, h).await;
            }
        }

        // Auto connect global hardware Jig if declared in flow header (e.g. jig: "COM5" or jig: "profiles/jig_switch.yaml")
        if let Some(jig_config) = &flow.jig {
            let base_dir = path.parent().unwrap_or(Path::new("."));
            let substituted_jig = match jig_config {
                crate::parser::types::JigConfig::PortOrProfile(s) => {
                    crate::parser::types::JigConfig::PortOrProfile(self.context.substitute_vars(s))
                }
                crate::parser::types::JigConfig::Struct(p) => {
                    let mut p_sub = p.clone();
                    p_sub.port = self.context.substitute_vars(&p_sub.port);
                    p_sub.file = p_sub.file.map(|f| self.context.substitute_vars(&f));
                    p_sub.wire_format = p_sub.wire_format.map(|w| self.context.substitute_vars(&w));
                    crate::parser::types::JigConfig::Struct(p_sub)
                }
            };
            let params = substituted_jig.resolve(Some(base_dir)).map_err(|e| anyhow::anyhow!(e))?;
            let port = self.context.substitute_vars(&params.port);
            let mut hw_config = crate::hardware::HardwareConfig::default();
            if let Some(nid) = params.node_id {
                hw_config.node_id = Some(nid);
            }
            if let Some(ref wf) = params.wire_format {
                hw_config.wire_format = Some(self.context.substitute_vars(wf));
            }
            if let Some(ref btns) = params.buttons {
                hw_config.button_mappings = btns.clone();
            }
            if let Some(ref rlys) = params.relays {
                hw_config.relay_mappings = rlys.clone();
            }
            let controller = crate::hardware::HardwareController::new(Some(hw_config));
            if let Err(e) = controller.connect(&port, params.baudrate) {
                let err_msg = format!("Hardware Jig connection failed on '{}': {}", port, e);
                self.emitter.emit(TestEvent::Log {
                    message: format!("  {} {}", "❌".red(), err_msg),
                    depth: self.depth,
                });
                anyhow::bail!(err_msg);
            }

            // Auto configure servos if provided in Jig profile (only configure once if already cached, unless force requested)
            if let Some(ref servos) = params.servos {
                let force_all = params.force_config.unwrap_or(false);
                let mut configured_count = 0;
                for cfg in servos {
                    let press_angle = cfg.press_angle.unwrap_or(5);
                    let release_angle = cfg.release_angle.unwrap_or(120);
                    let press_duration_ms = cfg.press_duration_ms.unwrap_or(100);
                    let release_duration_ms = cfg.release_duration_ms.unwrap_or(100);
                    let hold_duration_ms = cfg.hold_duration_ms.unwrap_or(100);
                    let force = force_all || cfg.force.unwrap_or(false);
                    let ch = cfg.resolved_channel();

                    if !force && crate::hardware::servo::is_servo_already_configured(
                        &port,
                        ch,
                        press_angle,
                        release_angle,
                        press_duration_ms,
                        release_duration_ms,
                        hold_duration_ms,
                    ) {
                        continue;
                    }

                    controller.servo.set_config(
                        ch,
                        press_angle,
                        release_angle,
                        press_duration_ms,
                        release_duration_ms,
                        hold_duration_ms,
                    ).map_err(|e| anyhow::anyhow!("Failed to auto-configure servo channel {}: {}", ch, e))?;
                    configured_count += 1;
                    std::thread::sleep(std::time::Duration::from_millis(30));
                }
                if configured_count > 0 {
                    println!("  {} Configured {} servo channels from Jig profile", "⚙️".cyan(), configured_count);
                }
            }

            self.auto_power_off = params.auto_power_off.unwrap_or(false);
            self.hardware_controller = Some(controller);
            println!("  {} Auto-connected hardware Jig on {}", "🔌".green(), port);
        }

        // Note: Web driver config (closeWhenFinish, browser type) is now pre-parsed and applied
        // in run_on_device before executor is created, so no re-init needed here.

        // Handle DDT (CSV Data)
        let mut iterations = Vec::new();
        if let Some(ref data_file) = flow.data {
            let base_dir = path.parent().unwrap_or(Path::new("."));
            let data_path = base_dir.join(data_file);
            println!(
                "    {} Loading data from: {}",
                "ℹ".blue(),
                data_path.display()
            );

            let file = File::open(&data_path).context("Failed to open data file")?;
            let mut rdr = csv::Reader::from_reader(file);
            for result in rdr.deserialize() {
                let record: HashMap<String, String> =
                    result.context("Failed to parse CSV record")?;
                iterations.push(record);
            }
            self.emitter.emit(TestEvent::Log {
                message: format!("{} Loaded {} data rows", "ℹ".blue(), iterations.len()),
                depth: self.depth,
            });
        } else {
            iterations.push(HashMap::new());
        }

        let base_flow_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        for (iter_idx, vars) in iterations.iter().enumerate() {
            // Apply variables from data row
            for (k, v) in vars {
                self.context.vars.insert(k.clone(), v.clone());
            }

            let flow_name = if iterations.len() > 1 {
                format!("{} [{}]", base_flow_name, iter_idx + 1)
            } else {
                base_flow_name.clone()
            };

            // Filter commands if specified
            let commands_to_run = if let Some(idx) = command_index {
                if idx >= flow.commands.len() {
                    anyhow::bail!(
                        "Command index {} is out of range. File has {} commands.",
                        idx,
                        flow.commands.len()
                    );
                }
                vec![flow.commands[idx].clone()]
            } else if let Some(from_idx) = from_command_index {
                if from_idx >= flow.commands.len() {
                    anyhow::bail!(
                        "From command index {} is out of range. File has {} commands.",
                        from_idx,
                        flow.commands.len()
                    );
                }
                flow.commands[from_idx..].to_vec()
            } else if let Some(name) = command_name {
                let found = flow
                    .commands
                    .iter()
                    .find(|cmd| {
                        let cmd_name = cmd.display_name().to_lowercase();
                        cmd_name == name.to_lowercase()
                            || cmd_name.starts_with(&name.to_lowercase())
                    })
                    .cloned();
                match found {
                    Some(cmd) => vec![cmd],
                    None => {
                        anyhow::bail!(
                            "Command '{}' not found in file. Available commands: {}",
                            name,
                            flow.commands
                                .iter()
                                .map(|c| c.display_name())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
            } else {
                flow.commands.clone()
            };

            self.run_commands_set(&commands_to_run, &flow_name, &path.display().to_string())
                .await?;
        }

        Ok(())
    }

    /// Run a set of commands as a flow
    async fn run_commands_set(
        &mut self,
        commands: &[TestCommand],
        flow_name: &str,
        flow_path: &str,
    ) -> Result<()> {
        let command_states: Vec<CommandState> = commands
            .iter()
            .enumerate()
            .map(|(i, cmd)| CommandState::new(i, &cmd.display_name(), &cmd.display_name()))
            .collect();

        let mut flow_state = FlowState::new(flow_name, flow_path, command_states);

        // Emit flow started event
        self.emitter.emit(TestEvent::FlowStarted {
            flow_name: flow_name.to_string(),
            flow_path: flow_path.to_string(),
            command_count: commands.len(),
            depth: self.depth,
        });

        flow_state.start();

        // Video Recording Setup (only on root depth)
        let video_active = self.video_enabled && self.depth == 0;
        let mut video_rel_path = None;

        if video_active {
            // Same session-scoped layout as failure evidence (images/logs) -
            // videos get their own subfolder under the session instead of the
            // flat device-level output dir.
            let videos_dir = self
                .context
                .output_path(&format!("sessions/{}/evidence/videos", self.session.session_id));
            let _ = std::fs::create_dir_all(&videos_dir);

            // Sanitize flow name safely
            let safe_name: String = flow_name
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect();

            let filename = format!(
                "video_{}_{}.mp4",
                safe_name,
                chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
            );

            let abs_path = videos_dir.join(&filename);
            let abs_path_str = abs_path.to_string_lossy().to_string();
            video_rel_path = Some(abs_path.display().to_string());

            self.emitter.emit(TestEvent::Log {
                message: format!(
                    "{} Starting video recording: {}",
                    "🎥".blue(),
                    abs_path.display()
                ),
                depth: self.depth,
            });

            if let Err(e) = self.driver.start_recording(&abs_path_str).await {
                self.emitter.emit(TestEvent::Log {
                    message: format!("{} Failed to start recording: {}", "⚠️".yellow(), e),
                    depth: self.depth,
                });
                // Disable video for this flow if start failed
                video_rel_path = None;
            }
        }

        // Execute commands
        for (i, command) in commands.iter().enumerate() {
            if let Some(cmd_state) = flow_state.commands.get_mut(i) {
                cmd_state.start();

                self.emitter.emit(TestEvent::CommandStarted {
                    flow_name: flow_name.to_string(),
                    index: i,
                    command: command.display_name(),
                    depth: self.depth,
                });

                match self.execute_command(command).await {
                    Ok(()) => {
                        cmd_state.pass();
                        let duration = cmd_state.duration_ms.unwrap_or(0);

                        // Auto-capture GIF frame if active
                        if self.auto_capture_active {
                            self.try_auto_capture().await;
                        }

                        self.emitter.emit(TestEvent::CommandPassed {
                            flow_name: flow_name.to_string(),
                            index: i,
                            duration_ms: duration,
                            depth: self.depth,
                        });
                    }
                    Err(e) => {
                        let error_msg = e.to_string();

                        // Capture debug info
                        let artifacts = self.handle_failure(flow_name, i, &error_msg).await;

                        cmd_state.fail(error_msg.clone());
                        cmd_state.screenshot_path = artifacts.screenshot_path;
                        cmd_state.ui_hierarchy_path = artifacts.ui_hierarchy_path;
                        cmd_state.log_path = artifacts.log_path;
                        let duration = cmd_state.duration_ms.unwrap_or(0);

                        self.emitter.emit(TestEvent::CommandFailed {
                            flow_name: flow_name.to_string(),
                            index: i,
                            error: error_msg,
                            duration_ms: duration,
                            depth: self.depth,
                        });

                        if !self.continue_on_failure {
                            // Skip remaining commands
                            flow_state.skip_remaining("Previous command failed");
                            break;
                        }
                    }
                }
            }

            flow_state.current_index = i + 1;
        }

        flow_state.finish();

        if let Some(rel_path) = video_rel_path {
            if let Err(e) = self.driver.stop_recording().await {
                self.emitter.emit(TestEvent::Log {
                    message: format!("{} Failed to stop recording: {}", "⚠️".yellow(), e),
                    depth: self.depth,
                });
            } else {
                // Check if file exists (optional, driver should ensure)
                flow_state.video_path = Some(rel_path);
            }
        }

        let status = flow_state.status.clone();
        let total_duration_ms = flow_state.total_duration_ms;

        self.emitter.emit(TestEvent::FlowFinished {
            flow_name: flow_name.to_string(),
            status: status.clone(),
            duration_ms: total_duration_ms,
            depth: self.depth,
        });

        // Check for soft errors
        if !self.soft_errors.is_empty() {
            let error_msg = format!(
                "Flow completed with {} soft assertion failures:\n{}",
                self.soft_errors.len(),
                self.soft_errors.join("\n")
            );

            self.emitter.emit(TestEvent::Log {
                message: format!("{} {}", "❌".red(), error_msg),
                depth: self.depth,
            });

            // Clear errors for next flow? Or fail here?
            // Fail the flow status if soft errors exist
            flow_state.status = crate::runner::state::FlowStatus::Failed;
            flow_state.error = Some(error_msg.clone());

            // If we are failing, we should bail
            self.session.add_flow(flow_state);
            anyhow::bail!(error_msg);
        }

        // Cleanup Jig connection if active (only on root depth)
        if self.depth == 0 {
            if let Some(ctrl) = &self.hardware_controller {
                if self.auto_power_off {
                    let _ = ctrl.enter_safe_state();
                    println!("  {} Auto-disconnected hardware Jig (power off & safe state)", "🔌".yellow());
                } else {
                    let _ = ctrl.color_sensor.light_off(None);
                    println!("  {} Auto-disconnected hardware Jig (hardware state preserved)", "🔌".yellow());
                }
                ctrl.disconnect();
                self.hardware_controller = None;
            }
        }

        self.session.add_flow(flow_state);

        if matches!(
            status,
            crate::runner::state::FlowStatus::Failed
                | crate::runner::state::FlowStatus::PartiallyPassed { .. }
        ) && !self.continue_on_failure
        {
            anyhow::bail!("Flow failed: {}", flow_name);
        }

        Ok(())
    }

    /// Handle assertion result with soft mode support
    fn handle_assertion(&mut self, result: Result<()>, soft: bool) -> Result<()> {
        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                if soft {
                    let msg = format!("Soft Assert Failed: {}", e);
                    self.soft_errors.push(msg.clone());
                    self.emitter.emit(TestEvent::Log {
                        message: format!("{} {}", "⚠️".yellow(), msg),
                        depth: self.depth,
                    });
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Apply align/offset to a selector, returning a new selector with resolved coordinates.
    /// If no align/offset is specified, returns the original selector unchanged.
    async fn apply_element_offset(
        &self,
        selector: crate::driver::traits::Selector,
        align: &Option<String>,
        offset: &Option<String>,
    ) -> Result<crate::driver::traits::Selector> {
        let subst_align = align.as_ref().map(|a| self.context.substitute_vars(a));
        let subst_offset = offset.as_ref().map(|o| self.context.substitute_vars(o));
        if let Some((xp, yp)) = resolve_element_offset(&subst_align, &subst_offset) {
            let (x, y) = self
                .driver
                .resolve_element_point(&selector, xp, yp)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("Element not found for offset ({:.0}%,{:.0}%): {:?}",
                        xp * 100.0, yp * 100.0, selector)
                })?;
            Ok(crate::driver::traits::Selector::Point { x, y })
        } else {
            Ok(selector)
        }
    }

    async fn try_auto_heal_tap(
        &mut self,
        selector: &crate::driver::traits::Selector,
        params: &crate::parser::types::TapParams,
    ) -> Option<crate::runner::healer::HealedMatch> {
        let selector_key = format!("{:?}", selector);
        let candidates = self.driver.get_screen_candidates().await.ok()?;
        if candidates.is_empty() {
            return None;
        }

        let query_text = params.text.as_deref().or(params.regex.as_deref());
        let query_id = params.id.as_deref();
        let query_desc = params.description.as_deref();

        self.healer.attempt_heal(&selector_key, query_text, query_id, query_desc, &candidates)
    }

    async fn record_heal_fingerprint(
        &mut self,
        selector: &crate::driver::traits::Selector,
        params: &crate::parser::types::TapParams,
    ) {
        if !self.healer.is_enabled() {
            return;
        }
        let selector_key = format!("{:?}", selector);
        if let Ok(candidates) = self.driver.get_screen_candidates().await {
            let matched = candidates.into_iter().find(|c| {
                if let (Some(pid), Some(cid)) = (&params.id, &c.id) {
                    if pid == cid {
                        return true;
                    }
                }
                if let (Some(ptext), Some(ctext)) = (&params.text, &c.text) {
                    if ptext == ctext {
                        return true;
                    }
                }
                false
            });
            if let Some(c) = matched {
                self.healer.record_success(&selector_key, &c);
            }
        }
    }

    fn resolve_tap_params(
        &self,
        input: &crate::parser::types::TapParamsInput,
    ) -> crate::parser::types::TapParams {
        let mut params = match input {
            crate::parser::types::TapParamsInput::Struct(p) => p.clone(),
            crate::parser::types::TapParamsInput::String(s) => {
                let subst = self.context.substitute_vars(s);
                if subst.trim().starts_with('{') {
                    if let Ok(p) = serde_json::from_str(&subst) {
                        return p;
                    }
                }
                if crate::parser::types::is_regex_string(&subst) {
                    crate::parser::types::TapParams {
                        regex: Some(subst),
                        ..Default::default()
                    }
                } else {
                    crate::parser::types::TapParams {
                        text: Some(subst),
                        ..Default::default()
                    }
                }
            }
        };

        // If 'element' field is specified, resolve it from variables and merge
        if let Some(element_ref) = &params.element {
            // Direct lookup if element_ref is a variable reference (e.g. "${var}")
            let resolved = if element_ref.starts_with("${") && element_ref.ends_with("}") {
                let var_name = &element_ref[2..element_ref.len() - 1];
                if let Some(val) = self.context.vars.get(var_name) {
                    val.clone()
                } else {
                    self.context.substitute_vars(element_ref)
                }
            } else {
                self.context.substitute_vars(element_ref)
            };
            if resolved.trim().starts_with('{') {
                if let Ok(element_params) =
                    serde_json::from_str::<crate::parser::types::TapParams>(&resolved)
                {
                    // Merge fields (element_params takes precedence)
                    if element_params.text.is_some() {
                        params.text = element_params.text;
                    }
                    if element_params.id.is_some() {
                        params.id = element_params.id;
                    }
                    if element_params.regex.is_some() {
                        params.regex = element_params.regex;
                    }
                    if element_params.css.is_some() {
                        params.css = element_params.css;
                    }
                    if element_params.xpath.is_some() {
                        params.xpath = element_params.xpath;
                    }
                    if element_params.description.is_some() {
                        params.description = element_params.description;
                    }
                    if element_params.placeholder.is_some() {
                        params.placeholder = element_params.placeholder;
                    }
                    if element_params.role.is_some() {
                        params.role = element_params.role;
                    }
                    if element_params.element_type.is_some() {
                        params.element_type = element_params.element_type;
                    }
                    if element_params.image.is_some() {
                        params.image = element_params.image;
                    }
                    if element_params.index.is_some() {
                        params.index = element_params.index;
                    }
                    if element_params.ocr.is_some() {
                        params.ocr = element_params.ocr;
                    }
                    if element_params.right_of.is_some() {
                        params.right_of = element_params.right_of;
                    }
                    if element_params.left_of.is_some() {
                        params.left_of = element_params.left_of;
                    }
                    if element_params.above.is_some() {
                        params.above = element_params.above;
                    }
                    if element_params.below.is_some() {
                        params.below = element_params.below;
                    }
                    if element_params.scrollable.is_some() {
                        params.scrollable = element_params.scrollable;
                    }
                }
            }
        }

        // Merge relative aliases (above, below, right_of, left_of) into relative
        if params.right_of.is_some()
            || params.left_of.is_some()
            || params.above.is_some()
            || params.below.is_some()
        {
            let mut r = params.relative.unwrap_or(crate::parser::types::RelativeParams {
                right_of: None,
                left_of: None,
                above: None,
                below: None,
                max_dist: None,
            });
            if params.right_of.is_some() {
                r.right_of = params.right_of.clone();
            }
            if params.left_of.is_some() {
                r.left_of = params.left_of.clone();
            }
            if params.above.is_some() {
                r.above = params.above.clone();
            }
            if params.below.is_some() {
                r.below = params.below.clone();
            }
            params.relative = Some(r);
        }

        params
    }

    /// Resolve a pinch/shove gesture's optional `at` field to concrete device-pixel
    /// coordinates: `None` means screen center; a point string ("x,y"/"x%,y%") or selector
    /// resolves the same way `drag`'s `from`/`to` do.
    async fn resolve_gesture_center(
        &self,
        at: &Option<crate::parser::types::TapParamsInput>,
    ) -> Result<(i32, i32)> {
        let (screen_width, screen_height) = self.driver.get_screen_size().await?;
        let Some(at) = at else {
            return Ok((screen_width as i32 / 2, screen_height as i32 / 2));
        };
        let params = self.resolve_tap_params(at);

        if let Some(point_str) = &params.point {
            let parts: Vec<&str> = point_str.split(',').collect();
            if parts.len() == 2 {
                let x_str = parts[0].trim();
                let y_str = parts[1].trim();
                let x = if x_str.ends_with('%') {
                    let pct: f64 = x_str.trim_end_matches('%').parse().unwrap_or(0.0);
                    (screen_width as f64 * pct / 100.0) as i32
                } else {
                    x_str.parse().unwrap_or(0)
                };
                let y = if y_str.ends_with('%') {
                    let pct: f64 = y_str.trim_end_matches('%').parse().unwrap_or(0.0);
                    (screen_height as f64 * pct / 100.0) as i32
                } else {
                    y_str.parse().unwrap_or(0)
                };
                return Ok((x, y));
            }
        }

        let selector = self
            .build_selector(
                &params.text,
                &params.regex,
                &params.id,
                &params.description,
                &params.relative,
                &params.css,
                &params.xpath,
                &params.placeholder,
                &params.role,
                &params.element_type,
                &params.image,
                params.index,
                &params.scrollable,
                params.exact,
                &params.ocr,
            )
            .ok_or_else(|| anyhow::anyhow!("No center point/selector specified for gesture"))?;
        let selector = self
            .apply_element_offset(selector, &params.align, &params.offset)
            .await?;

        match selector {
            crate::driver::traits::Selector::Point { x, y } => Ok((x, y)),
            _ => {
                let timeout = self.context.default_timeout_ms;
                let _ = self.driver.wait_for_element(&selector, timeout).await;
                self.driver
                    .resolve_element_point(&selector, 0.5, 0.5)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!("Gesture center element not found: {:?}", selector)
                    })
            }
        }
    }

    fn resolve_assert_params(
        &self,
        input: &crate::parser::types::AssertParamsInput,
    ) -> crate::parser::types::AssertParams {
        let mut params = match input {
            crate::parser::types::AssertParamsInput::Struct(p) => p.clone(),
            crate::parser::types::AssertParamsInput::String(s) => {
                let subst = self.context.substitute_vars(s);
                if subst.trim().starts_with('{') {
                    if let Ok(p) = serde_json::from_str(&subst) {
                        return p;
                    }
                }
                if crate::parser::types::is_regex_string(&subst) {
                    crate::parser::types::AssertParams {
                        regex: Some(subst),
                        ..Default::default()
                    }
                } else {
                    crate::parser::types::AssertParams {
                        text: Some(subst),
                        ..Default::default()
                    }
                }
            }
        };

        // If 'element' field is specified, resolve it from variables and merge
        if let Some(element_ref) = &params.element {
            // Direct lookup if element_ref is a variable reference (e.g. "${var}")
            let resolved = if element_ref.starts_with("${") && element_ref.ends_with("}") {
                let var_name = &element_ref[2..element_ref.len() - 1];
                if let Some(val) = self.context.vars.get(var_name) {
                    val.clone()
                } else {
                    self.context.substitute_vars(element_ref)
                }
            } else {
                self.context.substitute_vars(element_ref)
            };
            if resolved.trim().starts_with('{') {
                if let Ok(element_params) =
                    serde_json::from_str::<crate::parser::types::AssertParams>(&resolved)
                {
                    // Merge element_params into params
                    if params.text.is_none() {
                        params.text = element_params.text;
                    }
                    if params.id.is_none() {
                        params.id = element_params.id;
                    }
                    if params.regex.is_none() {
                        params.regex = element_params.regex;
                    }
                    if params.css.is_none() {
                        params.css = element_params.css;
                    }
                    if params.xpath.is_none() {
                        params.xpath = element_params.xpath;
                    }
                    if params.description.is_none() {
                        params.description = element_params.description;
                    }
                    if params.placeholder.is_none() {
                        params.placeholder = element_params.placeholder;
                    }
                    if params.role.is_none() {
                        params.role = element_params.role;
                    }
                    if params.element_type.is_none() {
                        params.element_type = element_params.element_type;
                    }
                    if params.image.is_none() {
                        params.image = element_params.image;
                    }
                    if params.index.is_none() {
                        params.index = element_params.index;
                    }
                    if params.ocr.is_none() {
                        params.ocr = element_params.ocr;
                    }
                    if params.right_of.is_none() {
                        params.right_of = element_params.right_of;
                    }
                    if params.left_of.is_none() {
                        params.left_of = element_params.left_of;
                    }
                    if params.above.is_none() {
                        params.above = element_params.above;
                    }
                    if params.below.is_none() {
                        params.below = element_params.below;
                    }
                    if params.scrollable.is_none() {
                        params.scrollable = element_params.scrollable;
                    }
                }
            }
        }

        // Merge relative aliases (above, below, right_of, left_of) into relative
        if params.right_of.is_some()
            || params.left_of.is_some()
            || params.above.is_some()
            || params.below.is_some()
        {
            let mut r = params.relative.unwrap_or(crate::parser::types::RelativeParams {
                right_of: None,
                left_of: None,
                above: None,
                below: None,
                max_dist: None,
            });
            if params.right_of.is_some() {
                r.right_of = params.right_of.clone();
            }
            if params.left_of.is_some() {
                r.left_of = params.left_of.clone();
            }
            if params.above.is_some() {
                r.above = params.above.clone();
            }
            if params.below.is_some() {
                r.below = params.below.clone();
            }
            params.relative = Some(r);
        }

        params
    }

    /// Execute a single command
    /// Open a read-only live camera view for each declared camera. Best-effort
    /// and spawned once per session, so testers can watch detection while the
    /// flow runs without opening the profile editor.
    fn maybe_start_observe_views(&mut self) {
        for (name, cfg) in self.camera_configs.clone() {
            let Some(profile_rel) = cfg.profile.clone() else {
                continue;
            };
            let profile_path = self.context.base_dir.join(&profile_rel);
            let server_url = cfg
                .server
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.contains("${"));
            if let Some(server_url) = server_url {
                let view_url = camera_view_url(server_url);
                let observe_key = format!("{}|{}", name, view_url);
                if self.camera_observe_started.contains_key(&observe_key) {
                    continue;
                }
                self.camera_observe_started.insert(observe_key, 0);
                self.emitter.emit(TestEvent::Log {
                    message: format!("👁  Camera '{}' live view: {}", name, view_url),
                    depth: self.depth,
                });
                open_camera_view(&view_url);
                continue;
            }

            let rtsp = cfg.rtsp.clone();
            if rtsp.trim().is_empty() || rtsp.contains("${") {
                continue;
            }
            let transport = cfg.transport.clone();
            let mut rtsp_hasher = DefaultHasher::new();
            rtsp.hash(&mut rtsp_hasher);
            let observe_key = format!(
                "{}|{}|{}|{:016x}",
                name,
                profile_path.display(),
                transport.as_deref().unwrap_or("tcp"),
                rtsp_hasher.finish()
            );
            if let Some(port) = self.camera_observe_started.get(&observe_key) {
                self.emitter.emit(TestEvent::Log {
                    message: format!(
                        "👁  Camera '{}' live view already running: http://localhost:{}/view",
                        name, port
                    ),
                    depth: self.depth,
                });
                continue;
            }
            let this_port = self.camera_observe_next_port;
            self.camera_observe_next_port = self.camera_observe_next_port.saturating_add(1);
            self.camera_observe_started
                .insert(observe_key.clone(), this_port);
            let view_server = format!("http://localhost:{}", this_port);
            if let Some(active_cfg) = self.camera_configs.get_mut(&name) {
                active_cfg.server = Some(view_server.clone());
            }
            self.emitter.emit(TestEvent::Log {
                message: format!(
                    "👁  Camera '{}' live view: http://localhost:{}/view",
                    name, this_port
                ),
                depth: self.depth,
            });
            open_camera_view(&format!("http://localhost:{}/view", this_port));
            let emitter = self.emitter.clone();
            let depth = self.depth;
            let camera_name = name.clone();
            let handle = tokio::spawn(async move {
                let server = crate::camera::server::CalibrateServer::new(
                    crate::camera::server::CalibrateConfig {
                        rtsp,
                        transport,
                        profile_path,
                        port: this_port,
                        observe: true,
                    },
                );
                if let Err(error) = server.start().await {
                    emitter.emit(TestEvent::Log {
                        message: format!(
                            "⚠️  Camera '{}' live view stopped: {}",
                            camera_name, error
                        ),
                        depth,
                    });
                }
            });
            self.camera_observe_tasks.insert(observe_key, handle);
        }
    }

    fn stop_observe_views(&mut self) {
        for (_, task) in self.camera_observe_tasks.drain() {
            task.abort();
        }
        self.camera_observe_started.clear();
    }

    /// Resolve which named camera a command refers to. An explicit name wins;
    /// otherwise "default", otherwise the sole camera if there is exactly one.
    fn resolve_camera_key(&self, requested: Option<&str>) -> Result<String> {
        if self.camera_configs.is_empty() {
            anyhow::bail!(
                "this flow uses a device command but declares no `camera:`/`cameras:` in its header"
            );
        }
        if let Some(name) = requested {
            if self.camera_configs.contains_key(name) {
                return Ok(name.to_string());
            }
            anyhow::bail!(
                "unknown camera '{}'; declared: {}",
                name,
                self.camera_configs
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if self.camera_configs.contains_key("default") {
            return Ok("default".to_string());
        }
        if self.camera_configs.len() == 1 {
            return Ok(self.camera_configs.keys().next().unwrap().clone());
        }
        anyhow::bail!(
            "multiple cameras declared ({}); specify `camera: <name>` on the command",
            self.camera_configs
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn capture_camera_evidence(
        &self,
        key: &str,
        session: &crate::camera::CameraSession,
        target_region: Option<&str>,
    ) -> Result<std::path::PathBuf> {
        let evidence = session.evidence()?;
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let uuid = Uuid::new_v4().to_string();
        let safe_key = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let dir = self.context.output_path(&format!(
            "camera_evidence/{}_{}_{}",
            safe_key,
            timestamp,
            &uuid[..8]
        ));
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create camera evidence dir: {}", dir.display()))?;

        crate::utils::image_convert::save_rgb_as_webp_lossless(&evidence.raw, &dir.join("raw.webp"))
            .context("failed to save camera raw frame")?;
        crate::utils::image_convert::save_rgb_as_webp_lossless(
            &evidence.warped,
            &dir.join("warped.webp"),
        )
        .context("failed to save camera warped frame")?;
        crate::utils::image_convert::save_rgb_as_webp_lossless(
            &evidence.annotated,
            &dir.join("annotated.webp"),
        )
        .context("failed to save camera annotated frame")?;
        let crop_artifact = target_region.and_then(|region| {
            let button = session.profile().button(region)?;
            let (iw, ih) = evidence.warped.dimensions();
            let x = button.roi[0].min(iw.saturating_sub(1));
            let y = button.roi[1].min(ih.saturating_sub(1));
            let w = button.roi[2].min(iw.saturating_sub(x)).max(1);
            let h = button.roi[3].min(ih.saturating_sub(y)).max(1);
            let crop = image::imageops::crop_imm(&evidence.warped, x, y, w, h).to_image();
            let safe_region = region
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            let file_name = format!("crop_{}.webp", safe_region);
            crate::utils::image_convert::save_rgb_as_webp_lossless(&crop, &dir.join(&file_name))
                .ok()?;
            Some(file_name)
        });
        let captured_at_ms = evidence
            .captured_at
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let frame_age_ms = evidence
            .captured_at
            .elapsed()
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let state = serde_json::to_string_pretty(&serde_json::json!({
            "camera": key,
            "capturedAtEpochMs": captured_at_ms,
            "frameAgeMs": frame_age_ms,
            "state": evidence.state,
            "artifacts": {
                "raw": "raw.webp",
                "warped": "warped.webp",
                "annotated": "annotated.webp",
                "crop": crop_artifact
            }
        }))?;
        std::fs::write(dir.join("state.json"), state).context("failed to save camera state")?;

        self.emitter.emit(TestEvent::Log {
            message: format!("📷 Camera evidence saved: {}", dir.display()),
            depth: self.depth,
        });
        Ok(dir)
    }

    fn with_camera_evidence(
        &self,
        key: &str,
        session: &crate::camera::CameraSession,
        target_region: Option<&str>,
        error: anyhow::Error,
    ) -> anyhow::Error {
        match self.capture_camera_evidence(key, session, target_region) {
            Ok(path) => anyhow::anyhow!("{}\ncamera evidence: {}", error, path.display()),
            Err(evidence_error) => anyhow::anyhow!(
                "{}\nfailed to capture camera evidence: {}",
                error,
                evidence_error
            ),
        }
    }

    async fn with_camera_timeline_evidence(
        &self,
        key: &str,
        session: &crate::camera::CameraSession,
        target_region: &str,
        error: anyhow::Error,
        pre_failure_timeline: Option<Vec<crate::camera::session::StateTimelineSample>>,
    ) -> anyhow::Error {
        match self.capture_camera_evidence(key, session, Some(target_region)) {
            Ok(path) => {
                let timeline_path = path.join("timeline.json");
                match session
                    .observe_state_timeline(target_region, 1_000, 150)
                    .await
                {
                    Ok(post_failure_timeline) => {
                        match serde_json::to_string_pretty(&serde_json::json!({
                            "camera": key,
                            "region": target_region,
                            "preFailureSamples": pre_failure_timeline.unwrap_or_default(),
                            "postFailureSamples": post_failure_timeline,
                        }))
                        .and_then(|json| {
                            std::fs::write(&timeline_path, json).map_err(serde_json::Error::io)
                        }) {
                            Ok(()) => anyhow::anyhow!(
                                "{}\ncamera evidence: {}\nstate timeline: {}",
                                error,
                                path.display(),
                                timeline_path.display()
                            ),
                            Err(timeline_error) => anyhow::anyhow!(
                                "{}\ncamera evidence: {}\nfailed to save state timeline: {}",
                                error,
                                path.display(),
                                timeline_error
                            ),
                        }
                    }
                    Err(timeline_error) => anyhow::anyhow!(
                        "{}\ncamera evidence: {}\nfailed to capture state timeline: {}",
                        error,
                        path.display(),
                        timeline_error
                    ),
                }
            }
            Err(evidence_error) => anyhow::anyhow!(
                "{}\nfailed to capture camera evidence: {}",
                error,
                evidence_error
            ),
        }
    }

    /// Lazily start (and cache) the warm camera stream for `requested`. Reused
    /// by all hardware-verification commands so the RTSP handshake — and the
    /// risk of missing an LED transition — only happens once per camera.
    async fn ensure_camera(&mut self, requested: Option<&str>) -> Result<String> {
        let key = self.resolve_camera_key(requested)?;
        if self.camera_sessions.contains_key(&key) {
            return Ok(key);
        }
        let cfg = self
            .camera_configs
            .get(&key)
            .cloned()
            .expect("config exists");
        let server_url = cfg.server.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if let Some(server_url) = server_url {
            if server_url.contains("${") {
                anyhow::bail!(
                    "camera '{}' has no resolved server URL; set camera.server to a URL such as http://localhost:9444",
                    key
                );
            }
        } else if cfg.rtsp.trim().is_empty() || cfg.rtsp.contains("${") {
            anyhow::bail!(
                "camera '{}' has no resolved frame source; set camera.server or camera.rtsp",
                key
            );
        }
        let profile_rel = cfg.profile.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "camera '{}' has no profile (path to a calibration JSON)",
                key
            )
        })?;
        if profile_rel.trim().is_empty() || profile_rel.contains("${") {
            anyhow::bail!(
                "camera '{}' has no resolved profile path (path to a calibration JSON)",
                key
            );
        }
        let profile_path = self.context.base_dir.join(&profile_rel);
        let profile = crate::camera::CameraProfile::load(&profile_path)?;

        let session = if let Some(server_url) = server_url {
            self.emitter.emit(TestEvent::Log {
                message: format!("📷 Connecting to camera '{}' ({})", key, server_url),
                depth: self.depth,
            });
            crate::camera::CameraSession::start_server(server_url, profile)?
        } else {
            self.emitter.emit(TestEvent::Log {
                message: format!(
                    "📷 Connecting to camera '{}' ({})",
                    key,
                    crate::camera::redact_url(&cfg.rtsp)
                ),
                depth: self.depth,
            });
            crate::camera::CameraSession::start(&cfg.rtsp, cfg.transport.as_deref(), profile)?
        };
        self.camera_sessions.insert(key.clone(), session);
        Ok(key)
    }

    pub async fn execute_command(&mut self, command: &TestCommand) -> Result<()> {
        match command {
            TestCommand::LaunchApp(params_input) => {
                let params_struct = params_input.as_ref().map(|p| p.clone().into_inner());
                // For web platform, prefer URL from params, context.url, or app_id
                let raw_app_id = if self.driver.platform_name() == "web" {
                    params_struct
                        .as_ref()
                        .and_then(|p| p.app_id.as_ref())
                        .or(self.context.url.as_ref())
                        .or(self.context.app_id.as_ref())
                        .ok_or_else(|| {
                            anyhow::anyhow!("No URL or app ID specified for web platform")
                        })?
                } else {
                    params_struct
                        .as_ref()
                        .and_then(|p| p.app_id.as_ref())
                        .or(self.context.app_id.as_ref())
                        .ok_or_else(|| anyhow::anyhow!("No app ID specified"))?
                };

                let app_id = &self.context.substitute_vars(raw_app_id);

                let clear_state = params_struct
                    .as_ref()
                    .map(|p| p.clear_state)
                    .unwrap_or(false);
                let clear_keychain = params_struct
                    .as_ref()
                    .map(|p| p.clear_keychain)
                    .unwrap_or(false);
                let permissions = params_struct.as_ref().and_then(|p| p.permissions.as_ref());
                let stop_app = params_struct
                    .as_ref()
                    .and_then(|p| p.stop_app)
                    .unwrap_or(true);

                // Clear keychain if requested (iOS only)
                if clear_keychain {
                    self.driver.clear_keychain().await?;
                }

                // If clearState and permissions both exist, we need to:
                // 1. Clear state first (which resets permissions)
                // 2. Set permissions after clear but before launch
                // 3. Launch app without clearing state again
                if clear_state && permissions.is_some() {
                    // Clear app data first
                    self.driver.clear_app_data(app_id).await?;

                    // Set permissions after clear state
                    if let Some(perms) = permissions {
                        self.driver.set_permissions(app_id, perms).await?;
                    }

                    // Launch app without clearing state again
                    self.driver.launch_app(app_id, false).await
                } else {
                    // Normal flow: set permissions first (if any), then launch
                    if let Some(perms) = permissions {
                        self.driver.set_permissions(app_id, perms).await?;
                    }

                    // Stop app if requested and not clearing state (state clear usually enforces stop)
                    if stop_app && !clear_state {
                        self.driver.stop_app(app_id).await.ok();
                    }

                    self.driver.launch_app(app_id, clear_state).await
                }
            }

            TestCommand::StopApp => {
                let app_id = self.context.app_id.as_deref().unwrap_or("");
                self.driver.stop_app(app_id).await
            }

            TestCommand::Find(params) => {
                // Serialize the selector part (TapParams) to JSON
                let json_val = serde_json::to_string(&params.selector)?;
                self.context.vars.insert(params.name.clone(), json_val);
                Ok(())
            }

            TestCommand::OpenLink(url) => {
                let substituted_url = self.context.substitute_vars(url);
                self.driver
                    .open_link(&substituted_url, self.context.app_id.as_deref())
                    .await
            }

            TestCommand::FocusApp(target) => {
                let substituted = self.context.substitute_vars(target);
                self.driver.focus_app(&substituted).await
            }

            TestCommand::TapOn(params_input) => {
                let params = self.resolve_tap_params(params_input);
                // If point is specified, use TapAt
                if let Some(point_str) = &params.point {
                    let subst_point = self.context.substitute_vars(point_str);
                    let parts: Vec<&str> = subst_point.split(',').collect();
                    if parts.len() == 2 {
                        // Parse point - supports both absolute "500,1000" and percentage "50%,80%"
                        let (screen_width, screen_height) = self.driver.get_screen_size().await?;

                        let x_str = parts[0].trim();
                        let y_str = parts[1].trim();

                        let x = if x_str.ends_with('%') {
                            let pct: f64 = x_str.trim_end_matches('%').parse().unwrap_or(0.0);
                            (screen_width as f64 * pct / 100.0) as i32
                        } else {
                            x_str.parse().unwrap_or(0)
                        };

                        let y = if y_str.ends_with('%') {
                            let pct: f64 = y_str.trim_end_matches('%').parse().unwrap_or(0.0);
                            (screen_height as f64 * pct / 100.0) as i32
                        } else {
                            y_str.parse().unwrap_or(0)
                        };

                        match self
                            .driver
                            .tap(&crate::driver::traits::Selector::Point { x, y })
                            .await
                        {
                            Ok(_) => Ok(()),
                            Err(e) => {
                                println!("DEBUG: TapAt Point Error: {}", e);
                                Err(e)
                            }
                        }
                    } else {
                        anyhow::bail!("Invalid point format: {}", subst_point);
                    }
                } else {
                    // Merge relative aliases
                    let mut relative = params.relative.clone();
                    if params.right_of.is_some()
                        || params.left_of.is_some()
                        || params.above.is_some()
                        || params.below.is_some()
                    {
                        let mut r = relative.unwrap_or(crate::parser::types::RelativeParams {
                            right_of: None,
                            left_of: None,
                            above: None,
                            below: None,
                            max_dist: None,
                        });
                        if params.right_of.is_some() {
                            r.right_of = params.right_of.clone();
                        }
                        if params.left_of.is_some() {
                            r.left_of = params.left_of.clone();
                        }
                        if params.above.is_some() {
                            r.above = params.above.clone();
                        }
                        if params.below.is_some() {
                            r.below = params.below.clone();
                        }
                        relative = Some(r);
                    }

                    let mut selector = self
                        .build_selector(
                            &params.text,
                            &params.regex,
                            &params.id,
                            &params.description,
                            &relative,
                            &params.css,
                            &params.xpath,
                            &params.placeholder,
                            &params.role,
                            &params.element_type,
                            &params.image,
                            params.index,
                            &params.scrollable,
                            params.exact,
                            &params.ocr,
                        )
                        .ok_or_else(|| anyhow::anyhow!("No selector specified for tapOn"))?;

                    // Inject imageRegion for Image selectors
                    if let crate::driver::traits::Selector::Image { ref mut region, .. } = selector
                    {
                        if params.image_region.is_some() {
                            *region = params.image_region.clone();
                        }
                    }

                    // Apply align/offset to resolve final tap coordinates
                    let selector = self.apply_element_offset(selector, &params.align, &params.offset).await?;

                    if params.optional {
                        if self.driver.is_visible(&selector).await? {
                            self.driver.tap(&selector).await
                        } else {
                            println!(
                                "  {} Optional element not found, skipping tap: {:?}",
                                "ℹ".blue(),
                                selector
                            );
                            Ok(())
                        }
                    } else {
                        // Single-Pass Tap fast path: attempt immediate tap first.
                        // If the element is already rendered on screen, this avoids doing a redundant
                        // wait_for_element hierarchy dump before the tap's own dump (~50% reduction in tap latency).
                        let tap_result = self.driver.tap(&selector).await;
                        let tap_err = match tap_result {
                            Ok(()) => None,
                            Err(initial_err) => {
                                let timeout = self.context.default_timeout_ms;
                                if !matches!(selector, crate::driver::traits::Selector::Point { .. }) {
                                    let _ = self.driver.wait_for_element(&selector, timeout).await;
                                }
                                match self.driver.tap(&selector).await {
                                    Ok(()) => None,
                                    Err(_) => Some(initial_err),
                                }
                            }
                        };

                        if let Some(e) = tap_err {
                            if let Some(healed) = self.try_auto_heal_tap(&selector, &params).await {
                                self.emitter.emit(crate::runner::events::TestEvent::CommandAutoHealed {
                                    flow_name: String::new(),
                                    index: 0,
                                    original_selector: format!("{:?}", selector),
                                    healed_target: healed.healed_text.clone().unwrap_or_else(|| healed.healed_id.clone().unwrap_or_default()),
                                    confidence: healed.confidence,
                                    suggestion: healed.suggestion.clone(),
                                    depth: self.depth,
                                });
                                let healed_sel = crate::driver::traits::Selector::Point {
                                    x: healed.target_point.0,
                                    y: healed.target_point.1,
                                };
                                self.driver.tap(&healed_sel).await?;
                            } else {
                                return Err(e);
                            }
                        } else {
                            self.record_heal_fingerprint(&selector, &params).await;
                        }
                        Ok(())
                    }
                }
            }

            TestCommand::LongPressOn(params_input) => {
                let params = self.resolve_tap_params(params_input);
                let selector = self
                    .build_selector(
                        &params.text,
                        &params.regex,
                        &params.id,
                        &params.description,
                        &params.relative,
                        &params.css,
                        &params.xpath,
                        &params.placeholder,
                        &params.role,
                        &params.element_type,
                        &params.image,
                        params.index,
                        &params.scrollable,
                        params.exact,
                        &params.ocr,
                    )
                    .ok_or_else(|| anyhow::anyhow!("No selector specified for longPressOn"))?;
                let selector = self.apply_element_offset(selector, &params.align, &params.offset).await?;
                let duration = params.duration.unwrap_or(800);

                // Fast path: attempt direct long_press without prior hierarchy wait
                if self.driver.long_press(&selector, duration).await.is_ok() {
                    return Ok(());
                }

                // Fallback: wait for element if not immediately present
                if !matches!(selector, crate::driver::traits::Selector::Point { .. }) {
                    let timeout = self.context.default_timeout_ms;
                    let _ = self.driver.wait_for_element(&selector, timeout).await;
                }
                self.driver.long_press(&selector, duration).await
            }

            TestCommand::DoubleTapOn(params_input) => {
                let params = self.resolve_tap_params(params_input);
                let selector = self
                    .build_selector(
                        &params.text,
                        &params.regex,
                        &params.id,
                        &params.description,
                        &params.relative,
                        &params.css,
                        &params.xpath,
                        &params.placeholder,
                        &params.role,
                        &params.element_type,
                        &params.image,
                        params.index,
                        &params.scrollable,
                        params.exact,
                        &params.ocr,
                    )
                    .ok_or_else(|| anyhow::anyhow!("No selector specified for doubleTapOn"))?;
                let selector = self.apply_element_offset(selector, &params.align, &params.offset).await?;
                let timeout = self.context.default_timeout_ms;
                if !matches!(selector, crate::driver::traits::Selector::Point { .. }) {
                    let _ = self.driver.wait_for_element(&selector, timeout).await;
                }
                self.driver.double_tap(&selector).await
            }

            TestCommand::RightClick(params_raw) => {
                let params = self.resolve_tap_params(&crate::parser::types::TapParamsInput::Struct(params_raw.clone()));
                let selector = self
                    .build_selector(
                        &params.text,
                        &params.regex,
                        &params.id,
                        &params.description,
                        &params.relative,
                        &params.css,
                        &params.xpath,
                        &params.placeholder,
                        &params.role,
                        &params.element_type,
                        &params.image,
                        params.index,
                        &params.scrollable,
                        params.exact,
                        &params.ocr,
                    )
                    .ok_or_else(|| anyhow::anyhow!("No selector specified for rightClick"))?;
                let selector = self.apply_element_offset(selector, &params.align, &params.offset).await?;
                let timeout = self.context.default_timeout_ms;
                if !matches!(selector, crate::driver::traits::Selector::Point { .. }) {
                    let _ = self.driver.wait_for_element(&selector, timeout).await;
                }
                self.driver.right_click(&selector).await
            }

            TestCommand::InputText(params_input) => {
                let text = params_input.text();
                let unicode = params_input.unicode();
                let substituted = self.context.substitute_vars(text);
                self.driver.input_text(&substituted, unicode).await
            }

            TestCommand::EraseText(params) => {
                let count = params.as_ref().and_then(|p| p.char_count);
                self.driver.erase_text(count).await
            }

            TestCommand::HideKeyboard => self.driver.hide_keyboard().await,

            TestCommand::SwipeLeft => {
                use crate::driver::traits::SwipeDirection;
                self.driver.swipe(SwipeDirection::Left, None, None).await
            }

            TestCommand::SwipeRight => {
                use crate::driver::traits::SwipeDirection;
                self.driver.swipe(SwipeDirection::Right, None, None).await
            }

            TestCommand::SwipeUp => {
                use crate::driver::traits::SwipeDirection;
                self.driver.swipe(SwipeDirection::Up, None, None).await
            }

            TestCommand::SwipeDown => {
                use crate::driver::traits::SwipeDirection;
                self.driver.swipe(SwipeDirection::Down, None, None).await
            }

            TestCommand::AssertVisible(params_input) => {
                let params = self.resolve_assert_params(params_input);
                let verification_result = async {
                    // Merge relative aliases
                    let mut relative = params.relative.clone();
                    if params.right_of.is_some()
                        || params.left_of.is_some()
                        || params.above.is_some()
                        || params.below.is_some()
                    {
                        let mut r = relative.unwrap_or(crate::parser::types::RelativeParams {
                            right_of: None,
                            left_of: None,
                            above: None,
                            below: None,
                            max_dist: None,
                        });
                        if params.right_of.is_some() {
                            r.right_of = params.right_of.clone();
                        }
                        if params.left_of.is_some() {
                            r.left_of = params.left_of.clone();
                        }
                        if params.above.is_some() {
                            r.above = params.above.clone();
                        }
                        if params.below.is_some() {
                            r.below = params.below.clone();
                        }
                        relative = Some(r);
                    }

                    let mut selector = self
                        .build_selector(
                            &params.text,
                            &params.regex,
                            &params.id,
                            &params.description,
                            &relative,
                            &params.css,
                            &params.xpath,
                            &params.placeholder,
                            &params.role,
                            &params.element_type,
                            &params.image,
                            params.index,
                            &params.scrollable,
                            false,
                            &params.ocr,
                        )
                        .ok_or_else(|| {
                            anyhow::anyhow!("No selector specified for assertVisible")
                        })?;

                    // Handle contains_child
                    if let Some(child_p) = &params.contains_child {
                        let child_params = &**child_p;
                        let child_sel = self
                            .build_selector(
                                &child_params.text,
                                &child_params.regex,
                                &child_params.id,
                                &child_params.description,
                                &child_params.relative,
                                &child_params.css,
                                &child_params.xpath,
                                &child_params.placeholder,
                                &child_params.role,
                                &child_params.element_type,
                                &child_params.image,
                                child_params.index,
                                &params.scrollable,
                                false,
                                &child_params.ocr,
                            )
                            .ok_or(anyhow::anyhow!("Invalid child selector in containsChild"))?;

                        selector = crate::driver::traits::Selector::HasChild {
                            parent: Box::new(selector),
                            child: Box::new(child_sel),
                        };
                    }

                    let timeout = params.timeout.unwrap_or(self.context.default_timeout_ms);
                    let visible = self.driver.wait_for_element(&selector, timeout).await?;

                    if visible {
                        Ok(())
                    } else {
                        anyhow::bail!("Element not visible within {}ms: {:?}", timeout, selector)
                    }
                }
                .await;
                self.handle_assertion(verification_result, params.soft)
            }

            TestCommand::WaitUntilVisible(params_input) => {
                let params = self.resolve_assert_params(params_input);
                // Identical logic to AssertVisible but semantically different
                // It's a wait command, but can be treated as an assertion that the element appears
                let verification_result = async {
                    // Merge relative aliases
                    let mut relative = params.relative.clone();
                    if params.right_of.is_some()
                        || params.left_of.is_some()
                        || params.above.is_some()
                        || params.below.is_some()
                    {
                        let mut r = relative.unwrap_or(crate::parser::types::RelativeParams {
                            right_of: None,
                            left_of: None,
                            above: None,
                            below: None,
                            max_dist: None,
                        });
                        if params.right_of.is_some() {
                            r.right_of = params.right_of.clone();
                        }
                        if params.left_of.is_some() {
                            r.left_of = params.left_of.clone();
                        }
                        if params.above.is_some() {
                            r.above = params.above.clone();
                        }
                        if params.below.is_some() {
                            r.below = params.below.clone();
                        }
                        relative = Some(r);
                    }

                    let mut selector = self
                        .build_selector(
                            &params.text,
                            &params.regex,
                            &params.id,
                            &params.description,
                            &relative,
                            &params.css,
                            &params.xpath,
                            &params.placeholder,
                            &params.role,
                            &params.element_type,
                            &params.image,
                            params.index,
                            &params.scrollable,
                            false,
                            &params.ocr,
                        )
                        .ok_or_else(|| {
                            anyhow::anyhow!("No selector specified for waitUntilVisible")
                        })?;

                    // Handle contains_child
                    if let Some(child_p) = &params.contains_child {
                        let child_params = &**child_p;
                        let child_sel = self
                            .build_selector(
                                &child_params.text,
                                &child_params.regex,
                                &child_params.id,
                                &child_params.description,
                                &child_params.relative,
                                &child_params.css,
                                &child_params.xpath,
                                &child_params.placeholder,
                                &child_params.role,
                                &child_params.element_type,
                                &child_params.image,
                                child_params.index,
                                &params.scrollable,
                                false,
                                &child_params.ocr,
                            )
                            .ok_or(anyhow::anyhow!("Invalid child selector in containsChild"))?;

                        selector = crate::driver::traits::Selector::HasChild {
                            parent: Box::new(selector),
                            child: Box::new(child_sel),
                        };
                    }

                    // Default timeout for wait is usually higher or same as assertion?
                    // Using context default timeout (default: 10s)
                    let timeout = params.timeout.unwrap_or(self.context.default_timeout_ms);
                    let visible = self.driver.wait_for_element(&selector, timeout).await?;

                    if visible {
                        Ok(())
                    } else {
                        anyhow::bail!("Element not visible within {}ms: {:?}", timeout, selector)
                    }
                }
                .await;
                // Wait command is effectively a hard assertion (it fails if not found)
                // But we support soft mode if user really wants to continue
                self.handle_assertion(verification_result, params.soft)
            }

            TestCommand::AssertNotVisible(params_input) => {
                let params = self.resolve_assert_params(params_input);
                let verification_result = async {
                    // Merge relative aliases
                    let mut relative = params.relative.clone();
                    if params.right_of.is_some()
                        || params.left_of.is_some()
                        || params.above.is_some()
                        || params.below.is_some()
                    {
                        let mut r = relative.unwrap_or(crate::parser::types::RelativeParams {
                            right_of: None,
                            left_of: None,
                            above: None,
                            below: None,
                            max_dist: None,
                        });
                        if params.right_of.is_some() {
                            r.right_of = params.right_of.clone();
                        }
                        if params.left_of.is_some() {
                            r.left_of = params.left_of.clone();
                        }
                        if params.above.is_some() {
                            r.above = params.above.clone();
                        }
                        if params.below.is_some() {
                            r.below = params.below.clone();
                        }
                        relative = Some(r);
                    }

                    let mut selector = self
                        .build_selector(
                            &params.text,
                            &params.regex,
                            &params.id,
                            &params.description,
                            &relative,
                            &params.css,
                            &params.xpath,
                            &params.placeholder,
                            &params.role,
                            &params.element_type,
                            &params.image,
                            params.index,
                            &params.scrollable,
                            false,
                            &params.ocr,
                        )
                        .ok_or_else(|| {
                            anyhow::anyhow!("No selector specified for assertNotVisible")
                        })?;

                    if let Some(child_p) = &params.contains_child {
                        let child_params = &**child_p;
                        let child_sel = self
                            .build_selector(
                                &child_params.text,
                                &child_params.regex,
                                &child_params.id,
                                &child_params.description,
                                &child_params.relative,
                                &child_params.css,
                                &child_params.xpath,
                                &child_params.placeholder,
                                &child_params.role,
                                &child_params.element_type,
                                &child_params.image,
                                child_params.index,
                                &params.scrollable,
                                false,
                                &child_params.ocr,
                            )
                            .ok_or(anyhow::anyhow!("Invalid child selector"))?;
                        selector = crate::driver::traits::Selector::HasChild {
                            parent: Box::new(selector),
                            child: Box::new(child_sel),
                        };
                    }

                    let visible = self.driver.is_visible(&selector).await?;

                    if !visible {
                        Ok(())
                    } else {
                        // Adaptive settling grace period: if the element is visible on the first check,
                        // it might be mid-way through an exit animation (modal dismiss, route pop, async state change).
                        // Give it an adaptive grace period (params.timeout or 1500ms) before declaring failure.
                        let grace_timeout = params.timeout.unwrap_or(1500);
                        let disappeared = self.driver.wait_for_absence(&selector, grace_timeout).await?;
                        if disappeared {
                            Ok(())
                        } else {
                            anyhow::bail!("Element is visible but should not be: {:?}", selector)
                        }
                    }
                }
                .await;
                self.handle_assertion(verification_result, params.soft)
            }

            TestCommand::WaitUntilNotVisible(params_input) => {
                let params = self.resolve_assert_params(params_input);

                // Merge relative aliases
                let mut relative = params.relative.clone();
                if params.right_of.is_some()
                    || params.left_of.is_some()
                    || params.above.is_some()
                    || params.below.is_some()
                {
                    let mut r = relative.unwrap_or(crate::parser::types::RelativeParams {
                        right_of: None,
                        left_of: None,
                        above: None,
                        below: None,
                        max_dist: None,
                    });
                    if params.right_of.is_some() {
                        r.right_of = params.right_of.clone();
                    }
                    if params.left_of.is_some() {
                        r.left_of = params.left_of.clone();
                    }
                    if params.above.is_some() {
                        r.above = params.above.clone();
                    }
                    if params.below.is_some() {
                        r.below = params.below.clone();
                    }
                    relative = Some(r);
                }

                let mut selector = self
                    .build_selector(
                        &params.text,
                        &params.regex,
                        &params.id,
                        &params.description,
                        &relative,
                        &params.css,
                        &params.xpath,
                        &params.placeholder,
                        &params.role,
                        &params.element_type,
                        &params.image,
                        params.index,
                        &params.scrollable,
                        false,
                        &params.ocr,
                    )
                    .ok_or_else(|| {
                        anyhow::anyhow!("No selector specified for waitUntilNotVisible")
                    })?;

                if let Some(child_p) = &params.contains_child {
                    let child_params = &**child_p;
                    let child_sel = self
                        .build_selector(
                            &child_params.text,
                            &child_params.regex,
                            &child_params.id,
                            &child_params.description,
                            &child_params.relative,
                            &child_params.css,
                            &child_params.xpath,
                            &child_params.placeholder,
                            &child_params.role,
                            &child_params.element_type,
                            &child_params.image,
                            child_params.index,
                            &params.scrollable,
                            false,
                            &child_params.ocr,
                        )
                        .ok_or(anyhow::anyhow!("Invalid child selector"))?;
                    selector = crate::driver::traits::Selector::HasChild {
                        parent: Box::new(selector),
                        child: Box::new(child_sel),
                    };
                }

                let timeout = params.timeout.unwrap_or(self.context.default_timeout_ms);
                let ok = self.driver.wait_for_absence(&selector, timeout).await?;

                if ok {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Element failed to disappear within {}ms: {:?}",
                        timeout,
                        selector
                    )
                }
            }

            TestCommand::WaitForAnimationToEnd => {
                self.driver.wait_for_animation(500).await
            }

            TestCommand::Wait(params_input) => {
                let params = params_input.clone().into_inner();
                tokio::time::sleep(tokio::time::Duration::from_millis(params.ms)).await;
                Ok(())
            }

            TestCommand::TakeScreenshot(params_input) => {
                let params = params_input.clone().into_inner();
                let path = self.context.substitute_vars(&params.path);
                let output_path = self.context.output_path(&path);
                if let Some(parent) = output_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let res = self
                    .driver
                    .take_screenshot(output_path.to_str().unwrap())
                    .await;
                match res {
                    Ok(_) => {
                        // Re-encode as lossless WebP for storage, same as failure
                        // evidence - all screenshots this tool writes to disk end
                        // up WebP, not just the ones captured on failure. Falls
                        // back to keeping the original file (whatever extension
                        // the flow asked for) if conversion fails for any reason.
                        let final_path = match crate::utils::image_convert::convert_to_webp_in_place(
                            &output_path,
                        ) {
                            Ok(webp_path) => webp_path,
                            Err(e) => {
                                self.emitter.emit(TestEvent::Log {
                                    message: format!(
                                        "{} Failed to convert screenshot to WebP, keeping original: {}",
                                        "⚠".yellow(),
                                        e
                                    ),
                                    depth: self.depth,
                                });
                                output_path.clone()
                            }
                        };
                        self.emitter.emit(TestEvent::Log {
                            message: format!(
                                "{} Saved screenshot to: {}",
                                "📸".blue(),
                                final_path.display().to_string().cyan()
                            ),
                            depth: self.depth,
                        });
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }

            TestCommand::AssertScreenshot(name) => {
                // An explicit extension in the testcase is respected literally
                // (e.g. an older baseline checked in as `.png`). Otherwise prefer
                // a `.webp` baseline - what `screenshot:` now creates - falling
                // back to `.png` so repos with pre-existing PNG baselines keep
                // working without needing to regenerate them.
                let subst_name = self.context.substitute_vars(name);
                let reference_path = if subst_name.ends_with(".png") || subst_name.ends_with(".webp") {
                    self.context.resolve_path(&format!("screenshots/{}", subst_name))
                } else {
                    let webp_ref = self
                        .context
                        .resolve_path(&format!("screenshots/{}.webp", subst_name));
                    if webp_ref.exists() {
                        webp_ref
                    } else {
                        self.context.resolve_path(&format!("screenshots/{}.png", subst_name))
                    }
                };

                if !reference_path.exists() {
                    anyhow::bail!(
                        "Reference screenshot not found: {}",
                        reference_path.display()
                    );
                }

                let diff = self.driver.compare_screenshot(&reference_path, 1.0).await?;
                if diff > 1.0 {
                    // Default 1% tolerance
                    anyhow::bail!("Visual regression detected! Difference: {:.2}%", diff);
                } else {
                    println!(
                        "  {} Visual check passed (diff: {:.2}%)",
                        "✨".green(),
                        diff
                    );
                    Ok(())
                }
            }

            TestCommand::StartRecording(params_input) => {
                let params = params_input.clone().into_inner();
                let subst_path = self.context.substitute_vars(&params.path);
                let path = self.context.output_path(&subst_path);
                self.driver
                    .start_recording(&path.display().to_string())
                    .await
            }

            TestCommand::StopRecording => self.driver.stop_recording().await,

            TestCommand::Back => self.driver.back().await,

            TestCommand::PressHome => self.driver.home().await,

            TestCommand::RunFlow(params_input) => {
                let params = params_input.clone().into_inner();

                // Check 'when' condition
                if let Some(condition) = &params.when {
                    if !self.evaluate_condition_value(condition).await {
                        if let Some(label) = &params.label {
                            self.emitter.emit(TestEvent::Log {
                                message: format!(
                                    "{} Skipped flow '{}': condition false",
                                    "⏭".blue(),
                                    label
                                ),
                                depth: self.depth,
                            });
                        }
                        return Ok(());
                    }
                }

                // Determine commands to run
                let commands_to_run = if let Some(cmds) = &params.commands {
                    Some(cmds.clone())
                } else if let Some(ref path_str) = params.path {
                    let subst_path = self.context.substitute_vars(path_str);
                    let flow_path = self.context.resolve_path(&subst_path);
                    let commands = if let Some(cached) = self.flow_cache.get(&flow_path) {
                        cached.commands.clone()
                    } else {
                        let sub_flow = parse_test_file(&flow_path)?;
                        let commands = sub_flow.commands.clone();
                        self.flow_cache.insert(flow_path, sub_flow);
                        commands
                    };
                    Some(commands)
                } else {
                    None
                };

                if let Some(cmds) = commands_to_run {
                    // Merge variables
                    if let Some(ref vars) = params.vars {
                        self.context.merge_vars(vars);
                    }

                    self.depth += 1;
                    let flow_name = params.label.clone().unwrap_or_else(|| {
                        params.path.clone().unwrap_or_else(|| "subflow".to_string())
                    });
                    let flow_path = params.path.clone().unwrap_or_default();

                    let res = Box::pin(self.run_commands_set(&cmds, &flow_name, &flow_path)).await;
                    self.depth -= 1;

                    if let Err(e) = res {
                        if params.optional.unwrap_or(false) {
                            self.emitter.emit(TestEvent::Log {
                                message: format!(
                                    "{} Optional Flow failed (ignored): {}",
                                    "ℹ".blue(),
                                    e
                                ),
                                depth: self.depth,
                            });
                            return Ok(());
                        }
                        anyhow::bail!("Flow failed: {}", e);
                    }
                }
                Ok(())
            }

            // TapAt - tap element by type and index
            TestCommand::TapAt(params) => {
                let elem_type = self.context.substitute_vars(&params.element_type);
                self.driver
                    .tap_by_type_index(&elem_type, params.index)
                    .await
            }

            // InputAt - input text at element by type and index
            TestCommand::InputAt(params) => {
                let elem_type = self.context.substitute_vars(&params.element_type);
                let text = self.context.substitute_vars(&params.text);
                self.driver
                    .input_by_type_index(&elem_type, params.index, &text)
                    .await
            }

            // SetVar - set a variable
            TestCommand::SetVar(params) => {
                self.context.set_var(&params.name, &params.value);
                Ok(())
            }

            // AssertVar - assert variable has expected value
            TestCommand::AssertVar(params) => {
                let expected = self.context.substitute_vars(&params.expected);
                let actual = self.context.get_var(&params.name).unwrap_or_default();
                if actual == expected {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Variable {} expected '{}', got '{}'",
                        params.name,
                        expected,
                        actual
                    )
                }
            }

            // Repeat - repeat commands N times or while condition matches
            TestCommand::Repeat(params) => {
                let mut iteration = 0;
                loop {
                    iteration += 1;

                    // Check 'times' condition
                    if let Some(times) = params.times {
                        if iteration > times {
                            break;
                        }
                    }

                    // Check 'while' condition
                    if let Some(ref condition) = params.while_condition {
                        if !self.evaluate_condition_value(condition).await {
                            break;
                        }
                    }

                    if params.times.is_none() && params.while_condition.is_none() {
                        // Avoid infinite loop if no condition
                        break;
                    }

                    let label = format!("Repeat #{}", iteration);
                    self.depth += 1;
                    let res =
                        Box::pin(self.run_commands_set(&params.commands, &label, "repeat")).await;
                    self.depth -= 1;
                    res?;

                    // Safety break for extremely large repeats
                    if iteration > 1000 {
                        anyhow::bail!("Repeat limit reached (1000 iterations)");
                    }
                }
                Ok(())
            }

            // Retry - retry commands on failure
            TestCommand::Retry(params) => {
                let mut last_error = None;
                for attempt in 0..params.max_retries {
                    let label = format!("Retry attempt #{}", attempt + 1);
                    self.depth += 1;
                    let res =
                        Box::pin(self.run_commands_set(&params.commands, &label, "retry")).await;
                    self.depth -= 1;

                    match res {
                        Ok(()) => return Ok(()),
                        Err(e) => {
                            last_error = Some(e);
                            if attempt < params.max_retries - 1 {
                                self.emitter.emit(TestEvent::Log {
                                    message: format!(
                                        "{} Attempt {} failed, retrying...",
                                        "⚠️".yellow(),
                                        attempt + 1
                                    ),
                                    depth: self.depth,
                                });
                            }
                        }
                    }
                }
                anyhow::bail!(
                    "Retry failed after {} attempts. Last error: {}",
                    params.max_retries,
                    last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error"))
                )
            }

            // ScrollUntilVisible
            TestCommand::ScrollUntilVisible(params_input) => {
                use crate::driver::traits::SwipeDirection;

                let mut params = params_input.clone().into_inner();
                // Merge relative aliases for target element
                if params.right_of.is_some()
                    || params.left_of.is_some()
                    || params.above.is_some()
                    || params.below.is_some()
                {
                    let mut r = params.relative.unwrap_or_default();
                    if params.right_of.is_some() {
                        r.right_of = params.right_of.clone();
                    }
                    if params.left_of.is_some() {
                        r.left_of = params.left_of.clone();
                    }
                    if params.above.is_some() {
                        r.above = params.above.clone();
                    }
                    if params.below.is_some() {
                        r.below = params.below.clone();
                    }
                    params.relative = Some(r);
                }

                // Scroll commands in parsing don't support index yet, default to None
                let selector = self
                    .build_selector(
                        &params.text,
                        &params.regex,
                        &params.id,
                        &params.description,
                        &params.relative,
                        &params.css,
                        &params.xpath,
                        &params.placeholder,
                        &params.role,
                        &params.element_type,
                        &params.image,
                        None,
                        &params.scrollable,
                        false,
                        &params.ocr,
                    )
                    .ok_or_else(|| {
                        anyhow::anyhow!("No selector specified for scrollUntilVisible")
                    })?;

                // Parse direction: "up" = swipe up (scroll content down), "down" = swipe down (scroll content up)
                let direction = params.direction.as_ref().map(|d| {
                    match d.to_lowercase().as_str() {
                        "up" => SwipeDirection::Up,
                        "down" => SwipeDirection::Down,
                        "left" => SwipeDirection::Left,
                        "right" => SwipeDirection::Right,
                        _ => SwipeDirection::Up, // Default
                    }
                });

                let from_selector = if let Some(ref from) = params.from {
                    let from_params = self.resolve_tap_params(&crate::parser::types::TapParamsInput::Struct(from.clone()));
                    self.build_selector(
                        &from_params.text,
                        &from_params.regex,
                        &from_params.id,
                        &from_params.description,
                        &from_params.relative,
                        &from_params.css,
                        &from_params.xpath,
                        &from_params.placeholder,
                        &from_params.role,
                        &from_params.element_type,
                        &from_params.image,
                        from_params.index,
                        &from_params.scrollable,
                        from_params.exact,
                        &from_params.ocr,
                    )
                } else if let Some(ref scrollable) = params.scrollable {
                    // Fallback: swipe the scrollable container itself
                    Some(crate::driver::traits::Selector::Scrollable(
                        scrollable.index.unwrap_or(0) as usize,
                    ))
                } else {
                    None
                };

                println!(
                    "      📜 Scrolling until visible (max_scrolls: {}, timeout: {:?})",
                    params.max_scrolls, params.timeout
                );

                let scroll_fut = self.driver.scroll_until_visible(
                    &selector,
                    params.max_scrolls,
                    direction,
                    from_selector,
                );

                let found = if let Some(timeout_ms) = params.timeout {
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(timeout_ms),
                        scroll_fut,
                    )
                    .await
                    {
                        Ok(res) => res?,
                        Err(_) => {
                            println!("      ⚠️  Scroll timeout reached after {}ms", timeout_ms);
                            false
                        }
                    }
                } else {
                    scroll_fut.await?
                };

                if found {
                    Ok(())
                } else {
                    // "Blind scroll" logic: If the user only provided a scrollable container
                    // without an itemIndex or any other selective criteria (text, id, etc.),
                    // we consider it a success after it has finished scrolling (or timed out).
                    let is_blind_scroll = params
                        .scrollable
                        .as_ref()
                        .map(|s| s.item_index.is_none())
                        .unwrap_or(false)
                        && params.text.is_none()
                        && params.regex.is_none()
                        && params.id.is_none()
                        && params.description.is_none()
                        && params.xpath.is_none()
                        && params.css.is_none()
                        && params.placeholder.is_none()
                        && params.role.is_none()
                        && params.element_type.is_none()
                        && params.image.is_none()
                        && params.relative.is_none();

                    if is_blind_scroll {
                        println!("      ✅ Blind scroll completed (no target specified)");
                        Ok(())
                    } else {
                        anyhow::bail!("Element not found after scrolling: {:?}", selector)
                    }
                }
            }

            // Conditional Logic
            TestCommand::Conditional(params) => {
                let condition_met = self.check_condition(&params.condition).await;

                let commands_val = if condition_met {
                    Some(&params.then)
                } else {
                    params.else_cmd.as_ref()
                };

                if let Some(val) = commands_val {
                    let cmds = parse_commands_from_value(val)?;
                    let label = format!(
                        "Conditional ({})",
                        if condition_met { "then" } else { "else" }
                    );
                    self.depth += 1;
                    let res = Box::pin(self.run_commands_set(&cmds, &label, "conditional")).await;
                    self.depth -= 1;
                    res?;
                }
                Ok(())
            }

            TestCommand::When(params) => {
                let condition_met = self.evaluate_condition_value(&params.condition).await;
                if condition_met {
                    Box::pin(self.execute_command(&params.command)).await?;
                } else {
                    println!("      ⏭️ Skipped (when condition not met)");
                }
                Ok(())
            }

            TestCommand::ForEach(params) => {
                let items_vec: Vec<serde_json::Value> = match &params.r#in {
                    serde_yaml::Value::Sequence(seq) => seq
                        .iter()
                        .map(|item| serde_json::to_value(item).unwrap_or(serde_json::Value::Null))
                        .collect(),
                    serde_yaml::Value::String(s) => {
                        let subst = self.context.substitute_vars(s);
                        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&subst) {
                            if let serde_json::Value::Array(arr) = json_val {
                                arr
                            } else {
                                vec![json_val]
                            }
                        } else {
                            subst
                                .split(',')
                                .map(|x| serde_json::Value::String(x.trim().to_string()))
                                .collect()
                        }
                    }
                    other => {
                        let json_val =
                            serde_json::to_value(other).unwrap_or(serde_json::Value::Null);
                        if let serde_json::Value::Array(arr) = json_val {
                            arr
                        } else {
                            vec![json_val]
                        }
                    }
                };

                let saved_prev_var = self.context.vars.get(&params.item).cloned();

                for (idx, item_val) in items_vec.iter().enumerate() {
                    match item_val {
                        serde_json::Value::Object(obj) => {
                            for (k, v) in obj {
                                let v_str = match v {
                                    serde_json::Value::String(s) => s.clone(),
                                    _ => v.to_string(),
                                };
                                self.context.vars.insert(format!("{}.{}", params.item, k), v_str);
                            }
                            self.context.vars.insert(params.item.clone(), item_val.to_string());
                        }
                        serde_json::Value::String(s) => {
                            self.context.vars.insert(params.item.clone(), s.clone());
                        }
                        _ => {
                            self.context.vars.insert(params.item.clone(), item_val.to_string());
                        }
                    }

                    let label = format!("ForEach {} [#{}]", params.item, idx + 1);
                    self.depth += 1;
                    let res = Box::pin(self.run_commands_set(&params.commands, &label, "forEach")).await;
                    self.depth -= 1;
                    res?;
                }

                if let Some(prev) = saved_prev_var {
                    self.context.vars.insert(params.item.clone(), prev);
                } else {
                    self.context.vars.remove(&params.item);
                }
                Ok(())
            }

            TestCommand::Match(params) => {
                let eval_val = self.context.substitute_vars(&params.value);
                let trimmed = eval_val.trim();
                if let Some(cmds) = params.cases.get(trimmed) {
                    let label = format!("Match case \"{}\"", trimmed);
                    self.depth += 1;
                    let res = Box::pin(self.run_commands_set(cmds, &label, "match")).await;
                    self.depth -= 1;
                    res?;
                } else if let Some(def_cmds) = &params.default {
                    let label = format!("Match default (for \"{}\")", trimmed);
                    self.depth += 1;
                    let res = Box::pin(self.run_commands_set(def_cmds, &label, "match")).await;
                    self.depth -= 1;
                    res?;
                } else {
                    println!("      ⏭️ Match: no case matched \"{}\" and no default", trimmed);
                }
                Ok(())
            }

            // Generate Mock Data
            TestCommand::Generate(params) => {
                use fake::faker::address::en::CityName;
                use fake::faker::internet::en::SafeEmail;
                use fake::faker::name::en::{FirstName, Name};
                use fake::faker::phone_number::en::PhoneNumber;
                use fake::Fake;
                use rand::Rng;

                let value = match params.data_type.to_lowercase().as_str() {
                    "uuid" => Uuid::new_v4().to_string(),
                    "email" | "safeemail" => SafeEmail().fake(),
                    "name" | "fullname" => Name().fake(),
                    "firstname" => FirstName().fake(),
                    "phone" | "phonenumber" => PhoneNumber().fake(),
                    "city" | "address" => CityName().fake(), // Simple city mostly
                    "number" => {
                        let mut rng = rand::thread_rng();
                        let (min, max) = if let Some(schema) = &params.format {
                            let parts: Vec<&str> = schema.split('-').collect();
                            if parts.len() == 2 {
                                (
                                    parts[0].parse().unwrap_or(0),
                                    parts[1].parse().unwrap_or(100),
                                )
                            } else {
                                (0, 100)
                            }
                        } else {
                            (0, 100)
                        };
                        rng.gen_range(min..=max).to_string()
                    }
                    _ => "unknown".to_string(),
                };
                self.context.set_var(&params.name, &value);
                Ok(())
            }

            // Run Shell Script
            TestCommand::RunScript(params_input) => {
                let params = params_input.clone().into_inner();
                let cmd_str = self.context.substitute_vars(&params.command);

                if cmd_str.trim().ends_with(".js") {
                    let script_path = self.context.resolve_path(&cmd_str);
                    if script_path.exists() {
                        let script_content = std::fs::read_to_string(&script_path)
                            .map_err(|e| anyhow::anyhow!("Failed to read JS file: {}", e))?;

                        use super::js_engine::JsEngine;
                        let mut engine = JsEngine::new();

                        // Set current context variables
                        engine.set_vars(&self.context.vars);

                        // Execute script
                        match engine.execute_script_with_output(&script_content) {
                            Ok(output_json) => {
                                // Update 'output' variable in context
                                self.context.set_var("output", &output_json);
                                self.emitter.emit(TestEvent::Log {
                                    message: format!(
                                        "{} Executed JS script: {}",
                                        "✓".green(),
                                        cmd_str
                                    ),
                                    depth: self.depth,
                                });
                            }
                            Err(e) => {
                                if params.fail_on_error {
                                    anyhow::bail!("JS Script execution failed: {}", e);
                                } else {
                                    println!(
                                        "  {} JS Script execution failed: {}",
                                        "⚠️".yellow(),
                                        e
                                    );
                                }
                            }
                        }

                        return Ok(());
                    }
                }

                let mut cmd = std::process::Command::new("sh");
                cmd.arg("-c").arg(&cmd_str);

                let output = cmd.output()?;

                if !output.status.success() && params.fail_on_error {
                    anyhow::bail!("Script failed: {}", String::from_utf8_lossy(&output.stderr));
                }

                if let Some(var_name) = &params.save_output {
                    let out_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    self.context.set_var(var_name, &out_str);
                }
                Ok(())
            }

            // HTTP Request (Simplified)
            TestCommand::HttpRequest(params) => {
                let url = self.context.substitute_vars(&params.url);
                let client = reqwest::Client::new();
                let method = params
                    .method
                    .parse::<reqwest::Method>()
                    .map_err(|_| anyhow::anyhow!("Invalid HTTP method"))?;

                let mut req = client.request(method, &url);

                if let Some(headers) = &params.headers {
                    for (k, v) in headers {
                        req = req.header(k, self.context.substitute_vars(v));
                    }
                }

                if let Some(body) = &params.body {
                    let body_str = match body {
                        serde_yaml::Value::String(s) => self.context.substitute_vars(s),
                        _ => {
                            let json_str = serde_json::to_string(body).unwrap_or_default();
                            self.context.substitute_vars(&json_str)
                        }
                    };
                    req = req.body(body_str);
                }

                let res = req.send().await?;
                let status = res.status();

                if !status.is_success() {
                    // Can allow failure but log warning
                    println!("  {} HTTP Request failed: {}", "⚠".yellow(), status);
                }

                if let Some(save_map) = &params.save_response {
                    let json: serde_json::Value = res.json().await?;
                    for (var_name, json_path) in save_map {
                        let val_to_save = if json_path == "$" || json_path == "." {
                            json.to_string()
                        } else {
                            // Convert dot path "data.token" to pointer "/data/token"
                            let pointer = if json_path.starts_with('/') {
                                json_path.clone()
                            } else {
                                format!("/{}", json_path.replace('.', "/"))
                            };

                            if let Some(val) = json.pointer(&pointer) {
                                if let Some(s) = val.as_str() {
                                    s.to_string()
                                } else {
                                    val.to_string()
                                }
                            } else if let Some(val) = json.get(json_path) {
                                // Fallback: try simple key access
                                if let Some(s) = val.as_str() {
                                    s.to_string()
                                } else {
                                    val.to_string()
                                }
                            } else {
                                println!(
                                    "  {} Warning: JSON path '{}' not found in response",
                                    "⚠".yellow(),
                                    json_path
                                );
                                continue;
                            }
                        };

                        self.context.set_var(var_name, &val_to_save);
                    }
                }
                Ok(())
            }

            // GPS Mock Location
            TestCommand::MockLocation(p_input) => {
                let p = p_input.clone().into_inner();
                let file_str = self.context.substitute_vars(&p.file);
                let file_path = self.context.resolve_path(&file_str);

                let content = std::fs::read_to_string(&file_path)
                    .context(format!("Failed to read GPS file: {}", file_path.display()))?;

                // Auto-detect format by extension
                let extension = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("gpx");

                let mut points = crate::parser::gps::parse_gps_file(&content, extension)?;

                // Apply start_index if specified
                if let Some(start_idx) = p.start_index {
                    if (start_idx as usize) < points.len() {
                        points = points.split_off(start_idx as usize);
                    }
                }

                let interval_ms = p.interval_ms.unwrap_or(1000);

                // Apply fixed altitude if specified
                if let Some(alt) = p.altitude {
                    for point in &mut points {
                        point.altitude = Some(alt);
                    }
                }

                println!(
                    "  {} Loaded {} GPS points from {}",
                    "📍".green(),
                    points.len(),
                    file_path.file_name().unwrap_or_default().to_string_lossy()
                );

                self.driver
                    .start_mock_location(
                        p.name,
                        points,
                        p.speed,
                        p.speed_mode,
                        p.speed_noise,
                        interval_ms,
                        p.loop_route,
                    )
                    .await?;

                Ok(())
            }

            TestCommand::StopMockLocation => {
                self.driver.stop_mock_location().await?;
                Ok(())
            }

            // Visual Assertions - AssertColor
            TestCommand::AssertColor(params) => {
                use crate::parser::types::AssertColorParams;

                let mut params = params.clone();
                params.point = self.context.substitute_vars(&params.point);
                params.color = self.context.substitute_vars(&params.color);

                // Get screen size for percentage calculation
                let (screen_width, screen_height) = self.driver.get_screen_size().await?;

                // Parse point (supports "540,960" or "50%,50%")
                let (x, y) = params
                    .parse_point(screen_width, screen_height)
                    .ok_or_else(|| anyhow::anyhow!("Invalid point format: {}", params.point))?;

                // Parse expected color
                let expected_color = params
                    .parse_color()
                    .ok_or_else(|| anyhow::anyhow!("Invalid color format: {}", params.color))?;

                // Get actual color from screen
                let actual_color = self.driver.get_pixel_color(x, y).await?;

                // Calculate color distance
                let distance = AssertColorParams::color_distance(expected_color, actual_color);

                if distance <= params.tolerance {
                    println!("  {} Color match at ({},{}) - expected: #{:02X}{:02X}{:02X}, actual: #{:02X}{:02X}{:02X} (diff: {:.1}%)",
                        "✓".green(),
                        x, y,
                        expected_color.0, expected_color.1, expected_color.2,
                        actual_color.0, actual_color.1, actual_color.2,
                        distance
                    );
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Color mismatch at ({},{}) - expected: #{:02X}{:02X}{:02X} ({}), actual: #{:02X}{:02X}{:02X}, diff: {:.1}% (tolerance: {:.1}%)",
                        x, y,
                        expected_color.0, expected_color.1, expected_color.2,
                        params.color,
                        actual_color.0, actual_color.1, actual_color.2,
                        distance,
                        params.tolerance
                    )
                }
            }

            // New Commands
            TestCommand::RotateScreen(params_input) => {
                let params = params_input.clone().into_inner();
                let mode = self.context.substitute_vars(&params.mode);
                // Deprecated: use SetOrientation
                self.driver.rotate_screen(&mode).await
            }

            TestCommand::SetOrientation(params) => {
                self.driver.set_orientation(params.mode.clone()).await
            }

            TestCommand::SetNetwork(params) => {
                self.driver
                    .set_network_connection(params.wifi, params.data)
                    .await
            }

            TestCommand::ToggleAirplaneMode => self.driver.toggle_airplane_mode().await,

            TestCommand::OpenNotifications => self.driver.open_notifications().await,

            TestCommand::OpenQuickSettings => self.driver.open_quick_settings().await,

            TestCommand::SetVolume(level) => self.driver.set_volume(*level).await,

            TestCommand::LockDevice => self.driver.lock_device().await,

            TestCommand::UnlockDevice => self.driver.unlock_device().await,

            TestCommand::InstallApp(path) => {
                let subst_path = self.context.substitute_vars(path);
                let resolved_path = self.context.resolve_path(&subst_path);
                self.driver
                    .install_app(resolved_path.to_str().unwrap())
                    .await
            }

            TestCommand::UninstallApp(pkg) => {
                let subst_pkg = self.context.substitute_vars(pkg);
                self.driver.uninstall_app(&subst_pkg).await
            }

            TestCommand::BackgroundApp(params) => {
                let raw_app_id = params.app_id.as_deref().or(self.context.app_id.as_deref());
                let subst_app_id = raw_app_id.map(|id| self.context.substitute_vars(id));
                self.driver.background_app(subst_app_id.as_deref(), params.duration_ms).await
            }

            TestCommand::PressKey(params) => {
                let raw_key = params.key();
                let key = self.context.substitute_vars(raw_key);
                let times_val = params.times_value();
                let times = match times_val {
                    serde_json::Value::Number(n) => n.as_u64().unwrap_or(1) as u32,
                    serde_json::Value::String(s) => {
                        let substituted = self.context.substitute_vars(&s);
                        substituted.parse::<u32>().unwrap_or(1)
                    }
                    _ => 1,
                };
                for _ in 0..times {
                    self.driver.press_key(&key).await?;
                }
                Ok(())
            }

            TestCommand::PushFile(params) => {
                let subst_source = self.context.substitute_vars(&params.source);
                let subst_dest = self.context.substitute_vars(&params.destination);
                let source = self.context.resolve_path(&subst_source);
                if !source.exists() {
                    anyhow::bail!("Source file not found: {}", source.display());
                }
                self.driver
                    .push_file(source.to_str().unwrap(), &subst_dest)
                    .await
            }

            TestCommand::PullFile(params) => {
                let subst_source = self.context.substitute_vars(&params.source);
                let subst_dest = self.context.substitute_vars(&params.destination);
                let dest = self.context.output_path(&subst_dest);
                self.driver
                    .pull_file(&subst_source, dest.to_str().unwrap())
                    .await
            }

            TestCommand::ClearAppData(app_id) => {
                let subst_app_id = self.context.substitute_vars(app_id);
                self.driver.clear_app_data(&subst_app_id).await
            }

            TestCommand::SetClipboard(text) => {
                let content = self.context.substitute_vars(text);
                self.driver.set_clipboard(&content).await
            }

            TestCommand::GetClipboard(params) => match self.driver.get_clipboard().await {
                Ok(content) => {
                    self.context.set_var(&params.name, &content);
                    Ok(())
                }
                Err(e) => {
                    println!(
                        "  {} GetClipboard failed (platform limitation?): {}",
                        "⚠️".yellow(),
                        e
                    );
                    Ok(())
                }
            },

            TestCommand::AssertClipboard(expected) => {
                let expected_text = self.context.substitute_vars(expected);
                let actual = self.driver.get_clipboard().await?;
                if actual == expected_text {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Clipboard content mismatch. Expected: '{}', Got: '{}'",
                        expected_text,
                        actual
                    )
                }
            }

            TestCommand::AssertTrue(params) => {
                use super::js_engine::JsEngine;
                use crate::parser::types::AssertTrueParams;

                let (condition_str, soft) = match params {
                    AssertTrueParams::Condition(c) => (c.condition.clone(), c.soft),
                    AssertTrueParams::Expression(expr) => (expr.clone(), false),
                };

                let result = {
                    // Substitute variables first
                    let substituted = self.context.substitute_vars(&condition_str);

                    // Create JS engine with current context variables
                    let mut engine = JsEngine::new();
                    engine.set_vars(&self.context.vars);
                    engine.set_vars(&self.context.env);

                    // Evaluate the boolean expression
                    match engine.eval_bool(&substituted) {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(anyhow::anyhow!(
                            "Assertion failed: {} evaluated to false",
                            condition_str
                        )),
                        Err(e) => Err(anyhow::anyhow!(
                            "Assertion error: {} - {}",
                            condition_str,
                            e
                        )),
                    }
                };

                self.handle_assertion(result, soft)
            }

            TestCommand::EvalScript(expr) => {
                use super::js_engine::JsEngine;

                // Create a new JS engine and load current variables
                let mut engine = JsEngine::new();
                engine.set_vars(&self.context.vars);
                engine.set_vars(&self.context.env);

                // Substitute variables first for ${var} syntax
                let substituted = self.context.substitute_vars(expr);

                // Evaluate the JavaScript expression
                match engine.eval_assignment(&substituted) {
                    Ok(Some((var_name, value))) => {
                        // Assignment expression - save the result
                        self.context.set_var(&var_name, &value);
                        println!("  {} evalScript: {} = {}", "📝".blue(), var_name, value);
                    }
                    Ok(None) => {
                        // Non-assignment expression, just evaluate
                        if let Ok(result) = engine.eval(&substituted) {
                            println!(
                                "  {} evalScript: {} => {}",
                                "📝".blue(),
                                substituted,
                                result
                            );
                        }
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("evalScript error: {}", e));
                    }
                }

                Ok(())
            }

            TestCommand::CopyTextFrom(params) => {
                let mut params = params.clone();
                // Merge relative aliases
                if params.right_of.is_some()
                    || params.left_of.is_some()
                    || params.above.is_some()
                    || params.below.is_some()
                {
                    let mut r = params.relative.unwrap_or_default();
                    if params.right_of.is_some() {
                        r.right_of = params.right_of.clone();
                    }
                    if params.left_of.is_some() {
                        r.left_of = params.left_of.clone();
                    }
                    if params.above.is_some() {
                        r.above = params.above.clone();
                    }
                    if params.below.is_some() {
                        r.below = params.below.clone();
                    }
                    params.relative = Some(r);
                }

                let selector = self.build_selector(
                    &params.text,
                    &params.regex,
                    &params.id,
                    &params.description,
                    &params.relative,
                    &None, // css
                    &None, // xpath
                    &None, // placeholder
                    &None, // role
                    &params.element_type,
                    &None, // image
                    params.index.map(|i| i as u32),
                    &None,
                    false,
                    &params.ocr,
                );

                if let Some(sel) = selector {
                    match self.driver.get_element_text(&sel).await {
                        Ok(text) => {
                            self.context.set_var("nl.copiedText", &text);
                            println!("  {} Copied text: '{}'", "📝".blue(), text);
                        }
                        Err(e) => {
                            println!("  {} Failed to extract text: {}", "⚠️".yellow(), e);
                            // Fallback mock if needed for specific tests
                            if let Some(fallback) = &params.text {
                                self.context.set_var("nl.copiedText", fallback);
                            }
                        }
                    }
                }
                Ok(())
            }

            TestCommand::PasteText => {
                // Get copied text and input it
                if let Some(copied) = self.context.get_var("nl.copiedText") {
                    self.driver.input_text(&copied, false).await?;
                }
                Ok(())
            }

            TestCommand::InputRandomEmail => {
                let email = {
                    use rand::Rng;
                    let mut rng = rand::thread_rng();
                    let random_part: String = (0..8)
                        .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
                        .collect();
                    format!("{}@test.com", random_part.to_lowercase())
                };
                self.driver.input_text(&email, false).await?;
                Ok(())
            }

            TestCommand::InputRandomNumber(params) => {
                let number = {
                    use rand::Rng;
                    let length = params.as_ref().and_then(|p| p.length).unwrap_or(10) as usize;
                    let mut rng = rand::thread_rng();
                    (0..length)
                        .map(|_| rng.gen_range(0..10).to_string())
                        .collect::<String>()
                };
                self.driver.input_text(&number, false).await?;
                Ok(())
            }

            TestCommand::InputRandomPersonName => {
                let name = {
                    use rand::seq::SliceRandom;
                    let first_names = [
                        "John", "Jane", "Alice", "Bob", "Charlie", "Diana", "Eve", "Frank",
                    ];
                    let last_names = [
                        "Smith", "Johnson", "Williams", "Brown", "Jones", "Davis", "Miller",
                        "Wilson",
                    ];
                    let mut rng = rand::thread_rng();
                    let first = first_names.choose(&mut rng).unwrap_or(&"John");
                    let last = last_names.choose(&mut rng).unwrap_or(&"Doe");
                    format!("{} {}", first, last)
                };
                self.driver.input_text(&name, false).await?;
                Ok(())
            }

            TestCommand::InputRandomText(params) => {
                let text = {
                    use rand::Rng;
                    let length = params.as_ref().and_then(|p| p.length).unwrap_or(10) as usize;
                    let mut rng = rand::thread_rng();
                    (0..length)
                        .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
                        .collect::<String>()
                };
                self.driver.input_text(&text, false).await?;
                Ok(())
            }

            TestCommand::ExtendedWaitUntil(params) => {
                let timeout_ms = params.timeout;

                if let Some(visible_val) = &params.visible {
                    let selector = self.selector_from_extended_wait_value(visible_val)?;
                    self.driver.wait_for_element(&selector, timeout_ms).await?;
                }

                if let Some(not_visible_val) = &params.not_visible {
                    let selector = self.selector_from_extended_wait_value(not_visible_val)?;
                    self.driver.wait_for_absence(&selector, timeout_ms).await?;
                }

                Ok(())
            }

            // Database Query
            TestCommand::DbQuery(params) => {
                let connection_str = self.context.substitute_vars(&params.connection);
                let query_str = self.context.substitute_vars(&params.query);

                // Create a pool (using sqlx::any for multi-db support)
                use sqlx::any::AnyPoolOptions;
                use sqlx::Row;

                // Create pool with 1 connection for simplicity in strict flow
                let pool = AnyPoolOptions::new()
                    .max_connections(1)
                    .connect(&connection_str)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?;

                // Prepare query
                let mut query_builder = sqlx::query(&query_str);

                if let Some(query_params) = &params.params {
                    for p in query_params {
                        let val = self.context.substitute_vars(p);
                        query_builder = query_builder.bind(val);
                    }
                }

                // Execute and fetch all
                let rows = query_builder
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to execute query: {}", e))?;

                self.emitter.emit(TestEvent::Log {
                    message: format!("{} Fetched {} rows", "ℹ".blue(), rows.len()),
                    depth: self.depth,
                });

                // Save results
                if let Some(save_map) = &params.save {
                    if let Some(first_row) = rows.first() {
                        for (col_name, var_name) in save_map {
                            // Try to get as string first, then fallbacks
                            let val_str = match first_row.try_get::<String, _>(col_name.as_str()) {
                                Ok(s) => s,
                                Err(_) => {
                                    // Try other common types
                                    if let Ok(v) = first_row.try_get::<i64, _>(col_name.as_str()) {
                                        v.to_string()
                                    } else if let Ok(v) =
                                        first_row.try_get::<f64, _>(col_name.as_str())
                                    {
                                        v.to_string()
                                    } else if let Ok(v) =
                                        first_row.try_get::<bool, _>(col_name.as_str())
                                    {
                                        v.to_string()
                                    } else {
                                        "null".to_string()
                                    }
                                }
                            };
                            self.context.set_var(var_name, &val_str);

                            self.emitter.emit(TestEvent::Log {
                                message: format!(
                                    "{} Saved db value {} = '{}'",
                                    "ℹ".blue(),
                                    var_name,
                                    val_str
                                ),
                                depth: self.depth,
                            });
                        }
                    } else {
                        self.emitter.emit(TestEvent::Log {
                            message: format!(
                                "{} No rows returned, cannot save variables",
                                "⚠".yellow()
                            ),
                            depth: self.depth,
                        });
                    }
                }

                Ok(())
            }

            // Hardware Automation Commands (Standardized hw* Commands)
            TestCommand::HwPing(params) => {
                let default_port = "/dev/cu.usbserial-A5069RR4".to_string();
                let port_raw = params.port.as_ref().unwrap_or(&default_port);
                let port = self.context.substitute_vars(port_raw);
                if let Some(ctrl) = &self.hardware_controller {
                    let resp = ctrl.ping(params.node_id)?;
                    println!("  {} Ping Node {:?} OK: {}", "🏓".green(), params.node_id.or(Some(1)), resp);
                    if let Some(ref var_name) = params.save_as {
                        let var = self.context.substitute_vars(var_name);
                        self.context.set_var(&var, &resp);
                    }
                } else {
                    let res = crate::hardware::ping_details(&port, params.baudrate, params.node_id, None)?;
                    println!("  {} Pinged Jig on {}: Node={:?}, Firmware={:?}, Status={:?} ({}ms)",
                        "🏓".green(), res.port, res.node_id, res.firmware_version, res.system_status, res.latency_ms);
                    if let Some(ref var_name) = params.save_as {
                        let var = self.context.substitute_vars(var_name);
                        self.context.set_var(&var, &res.latency_ms.to_string());
                    }
                }
                Ok(())
            }

            TestCommand::HwConnect(params) => {
                let resolved_params = if let Some(ref file_path) = params.file {
                    let substituted_file = self.context.substitute_vars(file_path);
                    let candidate_path = self.context.base_dir.join(&substituted_file);
                    let content = std::fs::read_to_string(&candidate_path)
                        .map_err(|e| anyhow::anyhow!("Failed to read jig file '{}': {}", candidate_path.display(), e))?;
                    let raw_val: serde_yaml::Value = serde_yaml::from_str(&content)
                        .map_err(|e| anyhow::anyhow!("Failed to parse jig file '{}': {}", candidate_path.display(), e))?;
                    
                    let mut loaded: crate::parser::types::HardwareConnectParams = if let Some(hw) = raw_val.get("hardware") {
                        let mut p: crate::parser::types::HardwareConnectParams = serde_yaml::from_value(hw.clone())
                            .unwrap_or_default();
                        if let Some(servos_val) = raw_val.get("servos") {
                            p.servos = serde_yaml::from_value(servos_val.clone()).ok();
                        }
                        if let Some(btns_val) = raw_val.get("buttons") {
                            p.buttons = serde_yaml::from_value(btns_val.clone()).ok();
                        }
                        if let Some(relays_val) = raw_val.get("relays") {
                            p.relays = serde_yaml::from_value(relays_val.clone()).ok();
                        }
                        p
                    } else {
                        serde_yaml::from_value(raw_val.clone())
                            .map_err(|e| anyhow::anyhow!("Failed to parse jig file '{}': {}", candidate_path.display(), e))?
                    };

                    if !params.port.is_empty() {
                        loaded.port = params.port.clone();
                    }
                    if params.baudrate.is_some() {
                        loaded.baudrate = params.baudrate;
                    }
                    if params.node_id.is_some() {
                        loaded.node_id = params.node_id;
                    }
                    if params.wire_format.is_some() {
                        loaded.wire_format = params.wire_format.clone();
                    }
                    if params.servos.is_some() {
                        loaded.servos = params.servos.clone();
                    }
                    if params.buttons.is_some() {
                        loaded.buttons = params.buttons.clone();
                    }
                    if params.relays.is_some() {
                        loaded.relays = params.relays.clone();
                    }
                    loaded
                } else {
                    params.clone()
                };

                let port = self.context.substitute_vars(&resolved_params.port);
                let mut hw_config = crate::hardware::HardwareConfig::default();
                if let Some(nid) = resolved_params.node_id {
                    hw_config.node_id = Some(nid);
                }
                if let Some(ref wf) = resolved_params.wire_format {
                    hw_config.wire_format = Some(self.context.substitute_vars(wf));
                }
                if let Some(ref btns) = resolved_params.buttons {
                    hw_config.button_mappings = btns.clone();
                }
                if let Some(ref rlys) = resolved_params.relays {
                    hw_config.relay_mappings = rlys.clone();
                }
                let controller = crate::hardware::HardwareController::new(Some(hw_config));
                controller.connect(&port, resolved_params.baudrate)?;

                if let Some(ref servos) = resolved_params.servos {
                    let force_all = resolved_params.force_config.unwrap_or(false);
                    let mut configured_count = 0;
                    for cfg in servos {
                        let press_angle = cfg.press_angle.unwrap_or(5);
                        let release_angle = cfg.release_angle.unwrap_or(120);
                        let press_duration_ms = cfg.press_duration_ms.unwrap_or(100);
                        let release_duration_ms = cfg.release_duration_ms.unwrap_or(100);
                        let hold_duration_ms = cfg.hold_duration_ms.unwrap_or(100);
                        let force = force_all || cfg.force.unwrap_or(false);
                        let ch = cfg.resolved_channel();

                        if !force && crate::hardware::servo::is_servo_already_configured(
                            &port,
                            ch,
                            press_angle,
                            release_angle,
                            press_duration_ms,
                            release_duration_ms,
                            hold_duration_ms,
                        ) {
                            continue;
                        }

                        controller.servo.set_config(
                            ch,
                            press_angle,
                            release_angle,
                            press_duration_ms,
                            release_duration_ms,
                            hold_duration_ms,
                        )?;
                        configured_count += 1;
                        std::thread::sleep(std::time::Duration::from_millis(30));
                    }
                    if configured_count > 0 {
                        println!("  {} Configured {} servo channels from Jig profile", "⚙️".cyan(), configured_count);
                    }
                }

                self.auto_power_off = resolved_params.auto_power_off.unwrap_or(false);
                self.hardware_controller = Some(controller);
                println!("  {} Connected hardware Jig on {}", "🔌".green(), port);
                Ok(())
            }

            TestCommand::HwDisconnect => {
                if let Some(ctrl) = &self.hardware_controller {
                    ctrl.disconnect();
                }
                self.hardware_controller = None;
                println!("  {} Disconnected hardware Jig", "🔌".yellow());
                Ok(())
            }

            TestCommand::HwClick(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let res = ctrl.servo.click(ch, params.hold_ms)?;
                println!("  {} Click button {}: completed={}", "⚙️".green(), label, res.completed);
                Ok(())
            }

            TestCommand::HwPress(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let res = ctrl.servo.press(ch)?;
                println!("  {} Pressed button {}: completed={}", "⚙️".green(), label, res.completed);
                Ok(())
            }

            TestCommand::HwRelease(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let res = ctrl.servo.release(ch)?;
                println!("  {} Released button {}: completed={}", "⚙️".green(), label, res.completed);
                Ok(())
            }

            TestCommand::HwRotate(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let speed = params.speed.unwrap_or(50);
                let res = ctrl.servo.rotate(ch, params.angle, speed)?;
                println!("  {} Rotated servo {} to {}° (speed={}): completed={}", "⚙️".green(), label, params.angle, speed, res.completed);
                Ok(())
            }

            TestCommand::HwReadServo(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let state_str = ctrl.servo.get_state(ch)?;
                println!("  {} Servo {} state: {}", "⚙️".blue(), label, state_str);
                if let Some(ref var_name) = params.save_as {
                    let var = self.context.substitute_vars(var_name);
                    self.context.set_var(&var, &state_str);
                }
                Ok(())
            }

            TestCommand::HwReadRelay(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref()
                    .map(|b| ctrl.resolve_relay_channels(b).first().copied().unwrap_or(1))
                    .unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let state = ctrl.relay.get_state(ch)?;
                println!("  {} Relay {} state: {}", "⚡".green(), label, state.as_str());
                if let Some(ref var_name) = params.save_as {
                    let var = self.context.substitute_vars(var_name);
                    self.context.set_var(&var, state.as_str());
                }
                Ok(())
            }

            TestCommand::HwReadColor(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let reading = ctrl.color_sensor.read_color(ch)?;
                println!(
                    "  {} Color sensor {}: Color={} (Conf={:?}, RGBC=[R:{} G:{} B:{} C:{}])",
                    "🎨".green(),
                    label,
                    reading.color.as_str(),
                    reading.confidence,
                    reading.sample.red,
                    reading.sample.green,
                    reading.sample.blue,
                    reading.sample.clear
                );
                if let Some(ref var_name) = params.save_as {
                    let var = self.context.substitute_vars(var_name);
                    self.context.set_var(&var, reading.color.as_str());
                }
                Ok(())
            }

            TestCommand::HwReadSensorLight(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.as_ref().and_then(|p| p.button.as_ref()).map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.as_ref().map(|p| p.resolved_channel()).unwrap_or(1));
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let enabled = ctrl.color_sensor.get_light_state(Some(ch))?;
                let state_str = if enabled { "on" } else { "off" };
                println!("  {} Color sensor {} LED (PB15): {}", "💡".blue(), label, if enabled { "ON" } else { "OFF" });
                if let Some(var_name) = params.as_ref().and_then(|p| p.save_as.as_ref()) {
                    let var = self.context.substitute_vars(var_name);
                    self.context.set_var(&var, state_str);
                }
                Ok(())
            }

            TestCommand::HwPowerOn(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let channels = resolved_btn.as_deref().map(|b| ctrl.resolve_relay_channels(b)).unwrap_or_else(|| params.resolved_channels());
                let label = format_hw_channels_label(resolved_btn.as_deref(), &channels);
                for ch in &channels {
                    let _ = ctrl.relay.set_state(*ch, crate::hardware::RelayState::On)?;
                }
                println!("  {} Turn ON relay {}: completed=true", "⚡".green(), label);
                Ok(())
            }

            TestCommand::HwPowerOff(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let channels = resolved_btn.as_deref().map(|b| ctrl.resolve_relay_channels(b)).unwrap_or_else(|| params.resolved_channels());
                let label = format_hw_channels_label(resolved_btn.as_deref(), &channels);
                for ch in &channels {
                    let _ = ctrl.relay.set_state(*ch, crate::hardware::RelayState::Off)?;
                }
                println!("  {} Turn OFF relay {}: completed=true", "⚡".yellow(), label);
                Ok(())
            }

            TestCommand::HwPowerOffAll => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let res = ctrl.relay.all_off()?;
                println!("  {} Turn OFF all: completed={}", "⚡".yellow(), res.completed);
                Ok(())
            }

            TestCommand::HwPowerCycle(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let channels = resolved_btn.as_deref().map(|b| ctrl.resolve_relay_channels(b)).unwrap_or_else(|| params.resolved_channels());
                let label = format_hw_channels_label(resolved_btn.as_deref(), &channels);
                let off_ms = params.off_ms.unwrap_or(2000);
                for ch in &channels {
                    ctrl.relay.set_state(*ch, crate::hardware::RelayState::Off)?;
                }
                println!("  {} Power Cycle: Turn OFF relay {}", "⚡".yellow(), label);
                std::thread::sleep(std::time::Duration::from_millis(off_ms));
                for ch in &channels {
                    ctrl.relay.set_state(*ch, crate::hardware::RelayState::On)?;
                }
                println!("  {} Power Cycle: Turn ON relay {}", "⚡".green(), label);
                Ok(())
            }

            TestCommand::HwSeeLed(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let timeout_s = params.timeout_ms.unwrap_or(5000) as f64 / 1000.0;
                let exp_list = params.resolved_expected();
                let exp_colors: Option<Vec<crate::hardware::Color>> = if !exp_list.is_empty() {
                    Some(exp_list.iter().map(|s| crate::hardware::Color::from_str(&self.context.substitute_vars(s))).collect())
                } else {
                    None
                };
                let reading = ctrl.color_sensor.wait_for_color(ch, exp_colors.as_deref(), timeout_s)?;
                println!(
                    "  {} Matched LED color {}: {} (Conf={:?}, RGBC=[R:{} G:{} B:{} C:{}])",
                    "🎨".green(),
                    label,
                    reading.color.as_str().cyan().bold(),
                    reading.confidence,
                    reading.sample.red,
                    reading.sample.green,
                    reading.sample.blue,
                    reading.sample.clear
                );
                Ok(())
            }

            TestCommand::HwSeeLedBlink(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let timeout_s = params.timeout_ms.unwrap_or(5000) as f64 / 1000.0;
                let resolved_col = params.color.as_ref().map(|c| self.context.substitute_vars(c));
                let blink_res = ctrl.color_sensor.wait_for_blinks(
                    ch,
                    resolved_col.as_deref(),
                    params.count,
                    None,
                    params.min_pulse_ms,
                    params.max_pulse_ms,
                    timeout_s,
                )?;
                let color_name = blink_res.color.as_ref().map(|c| c.as_str()).unwrap_or("UNKNOWN");
                let durations_str = blink_res
                    .durations_ms
                    .iter()
                    .map(|d| format!("{}ms", d))
                    .collect::<Vec<_>>()
                    .join(", ");
                let total_duration: u64 = blink_res.durations_ms.iter().sum();
                let target_cnt = params.count.unwrap_or(1);
                println!(
                    "  {} Verified LED Blink {}: count={} (target={}), color={}, durations=[{}] (total: {}ms)",
                    "💡".green().bold(),
                    label.cyan().bold(),
                    blink_res.blink_count.to_string().green().bold(),
                    target_cnt.to_string().yellow(),
                    color_name.magenta().bold(),
                    durations_str.yellow(),
                    total_duration
                );
                if !blink_res.pulses.is_empty() {
                    for (i, p) in blink_res.pulses.iter().enumerate() {
                        let is_last = i + 1 == blink_res.pulses.len();
                        let prefix = if is_last { "└─" } else { "├─" };
                        println!(
                            "     {} Pulse #{}: {}ms | Color: {} | RGBC=[R:{} G:{} B:{} C:{}] | ΔRGBC=[+{}, +{}, +{}, +{}]",
                            prefix.dimmed(),
                            p.index,
                            p.duration_ms.to_string().yellow().bold(),
                            p.color.as_str().magenta().bold(),
                            p.sample.red,
                            p.sample.green,
                            p.sample.blue,
                            p.sample.clear,
                            p.delta.0,
                            p.delta.1,
                            p.delta.2,
                            p.delta.3
                        );
                    }
                } else {
                    for (i, dur) in blink_res.durations_ms.iter().enumerate() {
                        let is_last = i + 1 == blink_res.durations_ms.len();
                        let prefix = if is_last { "└─" } else { "├─" };
                        println!(
                            "     {} Pulse #{}: {}ms (color: {})",
                            prefix.dimmed(),
                            i + 1,
                            dur.to_string().yellow().bold(),
                            color_name.magenta()
                        );
                    }
                }
                Ok(())
            }

            TestCommand::HwSeeNativeLedBlink(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let timeout_s = params.timeout_ms.unwrap_or(5000) as f64 / 1000.0;
                let resolved_col = params.color.as_ref().map(|c| self.context.substitute_vars(c));
                let blink_res = ctrl.color_sensor.wait_for_blinks_native(
                    ch,
                    resolved_col.as_deref(),
                    params.count,
                    None,
                    timeout_s,
                )?;
                let color_name = blink_res.color.as_ref().map(|c| c.as_str()).unwrap_or("UNKNOWN");
                let durations_str = blink_res
                    .durations_ms
                    .iter()
                    .map(|d| format!("{}ms", d))
                    .collect::<Vec<_>>()
                    .join(", ");
                let total_duration: u64 = blink_res.durations_ms.iter().sum();
                let target_cnt = params.count.unwrap_or(1);
                println!(
                    "  {} Verified LED Blink (native/firmware) {}: count={} (target={}), color={}, durations=[{}] (total: {}ms, event_id={})",
                    "💡".green().bold(),
                    label.cyan().bold(),
                    blink_res.blink_count.to_string().green().bold(),
                    target_cnt.to_string().yellow(),
                    color_name.magenta().bold(),
                    durations_str.yellow(),
                    total_duration,
                    blink_res.event_id
                );
                for (i, dur) in blink_res.durations_ms.iter().enumerate() {
                    let is_last = i + 1 == blink_res.durations_ms.len();
                    let prefix = if is_last { "└─" } else { "├─" };
                    println!(
                        "     {} Pulse #{}: {}ms (color: {})",
                        prefix.dimmed(),
                        i + 1,
                        dur.to_string().yellow().bold(),
                        color_name.magenta()
                    );
                }
                Ok(())
            }

            TestCommand::HwRepeatClick(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let press_ms = params.press_ms.unwrap_or(200);
                let release_ms = params.release_ms.unwrap_or(200);
                let res = ctrl.servo.repeat(ch, params.count, press_ms, release_ms)?;
                println!("  {} Repeat click {} (x{}): completed={}", "⚙️".green(), label, params.count, res.completed);
                Ok(())
            }

            TestCommand::HwSeeLedOff(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let timeout_s = params.timeout_ms.unwrap_or(5000) as f64 / 1000.0;
                let exp_colors = vec![crate::hardware::Color::Off, crate::hardware::Color::Unknown];
                let reading = ctrl.color_sensor.wait_for_color(ch, Some(&exp_colors), timeout_s)?;
                println!(
                    "  {} LED is OFF {}: {} (Conf={:?}, RGBC=[R:{} G:{} B:{} C:{}])",
                    "🌑".green(),
                    label,
                    reading.color.as_str().yellow(),
                    reading.confidence,
                    reading.sample.red,
                    reading.sample.green,
                    reading.sample.blue,
                    reading.sample.clear
                );
                Ok(())
            }

            TestCommand::HwConfigureServo(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let port = ctrl.port();
                let press_angle = params.press_angle.unwrap_or(15);
                let release_angle = params.release_angle.unwrap_or(72);
                let press_ms = params.press_duration_ms.unwrap_or(400);
                let release_ms = params.release_duration_ms.unwrap_or(150);
                let hold_ms = params.hold_duration_ms.unwrap_or(300);
                let force = params.force.unwrap_or(false);
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);

                if !force && crate::hardware::servo::is_servo_already_configured(
                    &port,
                    ch,
                    press_angle,
                    release_angle,
                    press_ms,
                    release_ms,
                    hold_ms,
                ) {
                    println!("  {} Servo {} already configured (cached, skipped)", "⚙️".cyan(), label);
                    return Ok(());
                }

                ctrl.servo.set_config(
                    ch,
                    press_angle,
                    release_angle,
                    press_ms,
                    release_ms,
                    hold_ms,
                )?;
                println!("  {} Configured servo {}", "⚙️".green(), label);
                Ok(())
            }

            TestCommand::HwReleaseAll => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                ctrl.servo.release_all()?;
                println!("  {} Released all servos", "⚙️".green());
                Ok(())
            }

            TestCommand::HwStartRepeatClick(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                let period_ms = params.period_ms.unwrap_or(1500);
                ctrl.servo.start_repeat(ch, period_ms)?;
                println!("  {} Started continuous click repeat {}", "⚙️".green(), label);
                Ok(())
            }

            TestCommand::HwStopRepeatClick(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_servo_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let label = format_hw_label(resolved_btn.as_deref(), ch);
                ctrl.servo.stop_repeat(ch)?;
                println!("  {} Stopped continuous click repeat {}", "⚙️".yellow(), label);
                Ok(())
            }

            TestCommand::HwSensorLight(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let resolved_state = params.state.as_ref().map(|s| self.context.substitute_vars(s));
                let on = params.enabled.unwrap_or_else(|| {
                    resolved_state.as_deref().unwrap_or("on").to_lowercase() == "on"
                });
                if on {
                    ctrl.color_sensor.light_on(Some(ch))?;
                } else {
                    ctrl.color_sensor.light_off(Some(ch))?;
                }
                println!("  {} Sensor light ch {} set to {}", "💡".green(), ch, if on { "ON" } else { "OFF" });
                Ok(())
            }

            TestCommand::HwSetBrightnessThresholds(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let min_pulse = params.min_pulse_ms.unwrap_or(50);
                let max_pulse = params.max_pulse_ms.unwrap_or(1000);
                let end_gap = params.sequence_end_gap_ms.unwrap_or(500);
                ctrl.color_sensor.set_thresholds(
                    ch,
                    params.off_below_percent,
                    params.on_above_percent,
                    min_pulse,
                    max_pulse,
                    end_gap,
                )?;
                println!("  {} Brightness thresholds set ch {}", "⚙️".green(), ch);
                Ok(())
            }

            TestCommand::HwWaitForBrightness(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let reading = ctrl.color_sensor.read_color(ch)?;
                println!("  {} Brightness check ch {}: sample={:?}", "💡".green(), ch, reading.sample);
                Ok(())
            }

            TestCommand::HwWaitForCct(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let reading = ctrl.color_sensor.read_color(ch)?;
                println!("  {} CCT check ch {}: sample={:?}", "💡".green(), ch, reading.sample);
                Ok(())
            }

            TestCommand::HwCalibrateColor(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let resolved_color = self.context.substitute_vars(&params.color);
                ctrl.calibration.calibrate_color(ch, &resolved_color)?;
                println!("  {} Calibrated color {} ch {}", "🎯".green(), resolved_color, ch);
                Ok(())
            }

            TestCommand::HwCalibrateBrightness(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                let resolved_mode = self.context.substitute_vars(&params.mode);
                let resolved_color = params.color.as_ref().map(|c| self.context.substitute_vars(c));
                ctrl.calibration.calibrate_brightness(ch, &resolved_mode, resolved_color.as_deref())?;
                println!("  {} Calibrated brightness mode {} ch {}", "🎯".green(), resolved_mode, ch);
                Ok(())
            }

            TestCommand::HwAddCctPoint(params) => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let resolved_btn = params.button.as_ref().map(|b| self.context.substitute_vars(b));
                let ch = resolved_btn.as_deref().map(|b| ctrl.resolve_sensor_channel(b)).unwrap_or_else(|| params.resolved_channel());
                ctrl.calibration.add_cct_point(ch, params.known_kelvin)?;
                println!("  {} Added CCT point {}K ch {}", "🎯".green(), params.known_kelvin, ch);
                Ok(())
            }

            TestCommand::HwSaveCalibration => {
                println!("  {} Calibration is auto-saved by firmware on each step", "💾".green());
                Ok(())
            }

            TestCommand::HwLoadCalibration => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                ctrl.calibration.action("load")?;
                println!("  {} Loaded calibration from Flash", "💾".green());
                Ok(())
            }

            TestCommand::HwResetCalibration => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                ctrl.calibration.action("defaults")?;
                println!("  {} Reset calibration to defaults", "💾".yellow());
                Ok(())
            }

            TestCommand::HwEraseCalibration => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                ctrl.calibration.action("erase")?;
                println!("  {} Erased Flash calibration", "💾".red());
                Ok(())
            }

            TestCommand::HwSafeState => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                ctrl.enter_safe_state()?;
                println!("  {} Entered safe state (relays OFF, servos released, sensor light OFF)", "🛡️".yellow());
                Ok(())
            }

            TestCommand::HwDiagnostics => {
                let ctrl = self.hardware_controller.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Hardware controller not connected! Call hwConnect first.")
                })?;
                let diag = ctrl.system_diagnostics()?;
                println!("  {} System diagnostics: {}", "🔍".green(), diag);
                self.context.set_var("hwDiagnostics", &diag);
                Ok(())
            }

            TestCommand::RunPython(params) => {
                let timeout_ms = params.timeout_ms.unwrap_or(30000);

                let py_exe = if let Some(ref p) = params.python_path {
                    self.context.substitute_vars(p)
                } else if cfg!(target_os = "windows") {
                    "python".to_string()
                } else {
                    "python3".to_string()
                };

                let (_temp_file, script_path) = if let Some(ref code) = params.code {
                    let substituted_code = self.context.substitute_vars(code);
                    let temp_dir = std::env::temp_dir();
                    let file_path = temp_dir.join(format!("lumi_py_{}.py", Uuid::new_v4()));
                    std::fs::write(&file_path, substituted_code)
                        .with_context(|| format!("Failed to write inline Python script to {}", file_path.display()))?;
                    (Some(file_path.clone()), file_path.to_string_lossy().to_string())
                } else if let Some(ref script) = params.script {
                    let substituted = self.context.substitute_vars(script);
                    let resolved = self.context.resolve_path(&substituted);
                    (None, resolved.to_string_lossy().to_string())
                } else {
                    anyhow::bail!("runPython requires either 'script' or 'code' parameter");
                };

                let substituted_args: Vec<String> = params
                    .args
                    .iter()
                    .map(|arg| self.context.substitute_vars(arg))
                    .collect();

                let mut cmd = tokio::process::Command::new(&py_exe);
                cmd.arg(&script_path);
                for arg in &substituted_args {
                    cmd.arg(arg);
                }

                for (k, v) in &params.env {
                    cmd.env(k, self.context.substitute_vars(v));
                }

                println!(
                    "  {} Running Python command: {} {}...",
                    "🐍".green(),
                    py_exe,
                    script_path
                );

                let start = std::time::Instant::now();
                let output_res = tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_ms),
                    cmd.output(),
                )
                .await;

                if let Some(ref temp_p) = _temp_file {
                    std::fs::remove_file(temp_p).ok();
                }

                let output = match output_res {
                    Ok(Ok(out)) => out,
                    Ok(Err(e)) => anyhow::bail!("Failed to execute Python process '{}': {}", py_exe, e),
                    Err(_) => anyhow::bail!("Python script execution timed out after {}ms", timeout_ms),
                };

                let stdout_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr_str = String::from_utf8_lossy(&output.stderr).trim().to_string();

                if !output.status.success() {
                    if !stderr_str.is_empty() {
                        println!("    {} {}", "❌ Python stderr:".red(), stderr_str);
                    }
                    anyhow::bail!("Python script failed with exit code {:?}: {}", output.status.code(), stderr_str);
                }

                if !stdout_str.is_empty() {
                    println!("    {} {}", "📄 Python stdout:".cyan(), stdout_str);
                }

                if let Some(ref var_name) = params.save_var {
                    self.context.set_var(var_name, &stdout_str);
                    println!(
                        "    {} Saved Python output to variable '${}' ({} bytes)",
                        "💾".green(),
                        var_name,
                        stdout_str.len()
                    );
                }

                if let Some(ref save_vars) = params.save_vars {
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
                        match save_vars {
                            crate::parser::types::SaveVarsInput::List(list) => {
                                for key in list {
                                    if let Some(val) = json_val.get(key) {
                                        let val_str = match val {
                                            serde_json::Value::String(s) => s.clone(),
                                            other => other.to_string(),
                                        };
                                        self.context.set_var(key, &val_str);
                                        println!(
                                            "    {} Saved Python JSON field '{}' to variable '${}'",
                                            "💾".green(),
                                            key,
                                            key
                                        );
                                    }
                                }
                            }
                            crate::parser::types::SaveVarsInput::Map(map) => {
                                for (var_name, json_path) in map {
                                    let pointer_path = if json_path.starts_with('/') {
                                        json_path.clone()
                                    } else {
                                        format!("/{}", json_path.replace('.', "/"))
                                    };
                                    let found_val = json_val.pointer(&pointer_path).or_else(|| json_val.get(json_path));
                                    if let Some(val) = found_val {
                                        let val_str = match val {
                                            serde_json::Value::String(s) => s.clone(),
                                            other => other.to_string(),
                                        };
                                        self.context.set_var(var_name, &val_str);
                                        println!(
                                            "    {} Saved Python JSON path '{}' to variable '${}'",
                                            "💾".green(),
                                            json_path,
                                            var_name
                                        );
                                    }
                                }
                            }
                        }
                    } else {
                        println!(
                            "    {} Warning: saveVars requires Python stdout to be valid JSON",
                            "⚠️".yellow()
                        );
                    }
                }

                println!(
                    "  {} Python execution completed in {}ms",
                    "✓".green(),
                    start.elapsed().as_millis()
                );

                Ok(())
            }

            TestCommand::SetWindowSize(params_input) => {
                let dimensions = match params_input {
                    crate::parser::types::SetWindowSizeParamsInput::Struct(p) => Some((p.width, p.height)),
                    crate::parser::types::SetWindowSizeParamsInput::String(s) => {
                        let subst = self.context.substitute_vars(s);
                        crate::parser::types::SetWindowSizeParamsInput::String(subst).to_dimensions()
                    }
                };
                if let Some((w, h)) = dimensions {
                    self.driver.set_window_size(w, h).await?;
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    self.emitter.emit(TestEvent::Log {
                        message: format!("  {} Window size set to {}x{}", "✓".green(), w, h),
                        depth: self.depth,
                    });
                }
                Ok(())
            }

            // GIF Recording
            TestCommand::CaptureGifFrame(params_input) => {
                let params = params_input.clone().into_inner();
                let frame_name = self.context.substitute_vars(&params.name);
                let temp_path = format!("/tmp/gif_frame_{}.png", Uuid::new_v4());
                self.driver.take_screenshot(&temp_path).await?;

                let mut img_bytes = std::fs::read(&temp_path)?;
                std::fs::remove_file(&temp_path).ok();

                // Crop if specified
                if let Some(ref crop_str) = params.crop {
                    let subst_crop = self.context.substitute_vars(crop_str);
                    img_bytes = self.crop_image(&img_bytes, &subst_crop)?;
                }

                self.gif_frames.insert(frame_name.clone(), img_bytes);
                println!("  {} Captured GIF frame: {}", "📷".green(), frame_name);
                Ok(())
            }

            TestCommand::BuildGif(params) => {
                use crate::parser::types::GifFrameInput;
                use image::codecs::gif::{GifEncoder, Repeat};
                use image::{Delay, Frame};

                let subst_output = self.context.substitute_vars(&params.output);
                let output_path = self.context.output_path(&subst_output);

                // Determine loop count
                let repeat = match params.loop_count {
                    Some(n) => Repeat::Finite(n),
                    None if params.loop_gif => Repeat::Infinite,
                    None => Repeat::Finite(1),
                };

                // Speed based on quality
                let speed = match params.quality.as_str() {
                    "high" => 1,
                    "low" => 30,
                    _ => 10, // medium
                };

                // Collect and process frames
                let mut frames = Vec::new();
                for frame_input in &params.frames {
                    let (name, delay) = match frame_input {
                        GifFrameInput::Name(n) => (self.context.substitute_vars(n), params.delay),
                        GifFrameInput::WithDelay { name, delay } => (self.context.substitute_vars(name), *delay),
                    };

                    let bytes = self
                        .gif_frames
                        .get(&name)
                        .ok_or_else(|| anyhow::anyhow!("GIF frame not found: {}", name))?;

                    let mut img = image::load_from_memory(bytes)?;

                    // Resize if width or height specified
                    if let Some(w) = params.width {
                        let ratio = w as f32 / img.width() as f32;
                        let h = (img.height() as f32 * ratio) as u32;
                        img = img.resize(w, h, image::imageops::FilterType::Lanczos3);
                    } else if let Some(h) = params.height {
                        let ratio = h as f32 / img.height() as f32;
                        let w = (img.width() as f32 * ratio) as u32;
                        img = img.resize(w, h, image::imageops::FilterType::Lanczos3);
                    }

                    frames.push((img.to_rgba8(), delay));
                }

                // Encode GIF
                let file = std::fs::File::create(&output_path)?;
                let mut encoder = GifEncoder::new_with_speed(file, speed);
                encoder.set_repeat(repeat)?;

                for (frame_img, delay_ms) in &frames {
                    let frame = Frame::from_parts(
                        frame_img.clone(),
                        0,
                        0,
                        Delay::from_numer_denom_ms(*delay_ms, 1),
                    );
                    encoder.encode_frame(frame)?;
                }

                println!(
                    "  {} Built GIF: {} ({} frames, quality: {})",
                    "🎬".green(),
                    output_path.display(),
                    frames.len(),
                    params.quality
                );
                Ok(())
            }

            // Start auto-capture mode
            TestCommand::StartGifCapture(params) => {
                self.auto_capture_frames.clear();
                self.auto_capture_active = true;
                self.auto_capture_interval = params.interval;
                self.auto_capture_max = params.max_frames;
                self.auto_capture_width = params.width;
                self.auto_capture_last_time = std::time::Instant::now();

                println!(
                    "  {} Started auto-capture (interval: {}ms, max: {} frames)",
                    "📹".green(),
                    params.interval,
                    params.max_frames
                );
                Ok(())
            }

            // Stop auto-capture and build GIF
            TestCommand::StopGifCapture(params) => {
                use image::codecs::gif::{GifEncoder, Repeat};
                use image::{Delay, Frame};

                self.auto_capture_active = false;

                if self.auto_capture_frames.is_empty() {
                    anyhow::bail!("No frames captured! Make sure startGifCapture was called.");
                }

                let output_path = self.context.output_path(&params.output);
                let delay_ms = params.delay.unwrap_or(self.auto_capture_interval as u32);

                let repeat = match params.loop_count {
                    Some(n) => Repeat::Finite(n),
                    None => Repeat::Infinite,
                };

                let speed = match params.quality.as_str() {
                    "high" => 1,
                    "low" => 30,
                    _ => 10,
                };

                // Process frames
                let mut processed_frames = Vec::new();
                for bytes in &self.auto_capture_frames {
                    let mut img = image::load_from_memory(bytes)?;

                    // Resize if width was specified
                    if let Some(w) = self.auto_capture_width {
                        let ratio = w as f32 / img.width() as f32;
                        let h = (img.height() as f32 * ratio) as u32;
                        img = img.resize(w, h, image::imageops::FilterType::Lanczos3);
                    }

                    processed_frames.push(img.to_rgba8());
                }

                // Encode GIF
                let file = std::fs::File::create(&output_path)?;
                let mut encoder = GifEncoder::new_with_speed(file, speed);
                encoder.set_repeat(repeat)?;

                for frame_img in &processed_frames {
                    let frame = Frame::from_parts(
                        frame_img.clone(),
                        0,
                        0,
                        Delay::from_numer_denom_ms(delay_ms, 1),
                    );
                    encoder.encode_frame(frame)?;
                }

                let frame_count = self.auto_capture_frames.len();
                self.auto_capture_frames.clear();

                println!(
                    "  {} Built smooth GIF: {} ({} frames, {}ms delay)",
                    "🎬".green(),
                    output_path.display(),
                    frame_count,
                    delay_ms
                );
                Ok(())
            }

            // ManualScroll (swipe command)
            TestCommand::ManualScroll(params) => {
                use crate::driver::traits::SwipeDirection;

                let direction = params
                    .as_ref()
                    .and_then(|p| p.direction.as_ref())
                    .map(|d| {
                        let subst_d = self.context.substitute_vars(d);
                        match subst_d.to_lowercase().as_str() {
                            "left" => SwipeDirection::Left,
                            "right" => SwipeDirection::Right,
                            "up" => SwipeDirection::Up,
                            "down" => SwipeDirection::Down,
                            _ => SwipeDirection::Up,
                        }
                    })
                    .unwrap_or(SwipeDirection::Up);

                let duration = params
                    .as_ref()
                    .and_then(|p| p.duration.or(p.distance))
                    .map(|d| d as u64);

                let from_selector =
                    if let Some(ref from) = params.as_ref().and_then(|p| p.from.as_ref()) {
                        let from_params = self.resolve_tap_params(&crate::parser::types::TapParamsInput::Struct((*from).clone()));
                        self.build_selector(
                            &from_params.text,
                            &from_params.regex,
                            &from_params.id,
                            &from_params.description,
                            &from_params.relative,
                            &from_params.css,
                            &from_params.xpath,
                            &from_params.placeholder,
                            &from_params.role,
                            &from_params.element_type,
                            &from_params.image,
                            from_params.index,
                            &from_params.scrollable,
                            from_params.exact,
                            &from_params.ocr,
                        )
                    } else {
                        None
                    };

                self.driver.swipe(direction, duration, from_selector).await
            }

            // Drag / DragFromTo command
            TestCommand::Drag(params) => {
                let from_params = self.resolve_tap_params(&params.from);
                let to_params = self.resolve_tap_params(&params.to);

                let from_selector = if let Some(point_str) = &from_params.point {
                    let subst_point = self.context.substitute_vars(point_str);
                    let parts: Vec<&str> = subst_point.split(',').collect();
                    if parts.len() == 2 {
                        let (screen_width, screen_height) = self.driver.get_screen_size().await?;
                        let x_str = parts[0].trim();
                        let y_str = parts[1].trim();
                        let x = if x_str.ends_with('%') {
                            let pct: f64 = x_str.trim_end_matches('%').parse().unwrap_or(0.0);
                            (screen_width as f64 * pct / 100.0) as i32
                        } else {
                            x_str.parse().unwrap_or(0)
                        };
                        let y = if y_str.ends_with('%') {
                            let pct: f64 = y_str.trim_end_matches('%').parse().unwrap_or(0.0);
                            (screen_height as f64 * pct / 100.0) as i32
                        } else {
                            y_str.parse().unwrap_or(0)
                        };
                        Some(crate::driver::traits::Selector::Point { x, y })
                    } else {
                        None
                    }
                } else {
                    self.build_selector(
                        &from_params.text,
                        &from_params.regex,
                        &from_params.id,
                        &from_params.description,
                        &from_params.relative,
                        &from_params.css,
                        &from_params.xpath,
                        &from_params.placeholder,
                        &from_params.role,
                        &from_params.element_type,
                        &from_params.image,
                        from_params.index,
                        &from_params.scrollable,
                        from_params.exact,
                        &from_params.ocr,
                    )
                }
                .ok_or_else(|| anyhow::anyhow!("No 'from' selector specified for drag"))?;

                let to_selector = if let Some(point_str) = &to_params.point {
                    let subst_point = self.context.substitute_vars(point_str);
                    let parts: Vec<&str> = subst_point.split(',').collect();
                    if parts.len() == 2 {
                        let (screen_width, screen_height) = self.driver.get_screen_size().await?;
                        let x_str = parts[0].trim();
                        let y_str = parts[1].trim();
                        let x = if x_str.ends_with('%') {
                            let pct: f64 = x_str.trim_end_matches('%').parse().unwrap_or(0.0);
                            (screen_width as f64 * pct / 100.0) as i32
                        } else {
                            x_str.parse().unwrap_or(0)
                        };
                        let y = if y_str.ends_with('%') {
                            let pct: f64 = y_str.trim_end_matches('%').parse().unwrap_or(0.0);
                            (screen_height as f64 * pct / 100.0) as i32
                        } else {
                            y_str.parse().unwrap_or(0)
                        };
                        Some(crate::driver::traits::Selector::Point { x, y })
                    } else {
                        None
                    }
                } else {
                    self.build_selector(
                        &to_params.text,
                        &to_params.regex,
                        &to_params.id,
                        &to_params.description,
                        &to_params.relative,
                        &to_params.css,
                        &to_params.xpath,
                        &to_params.placeholder,
                        &to_params.role,
                        &to_params.element_type,
                        &to_params.image,
                        to_params.index,
                        &to_params.scrollable,
                        to_params.exact,
                        &to_params.ocr,
                    )
                }
                .ok_or_else(|| anyhow::anyhow!("No 'to' selector specified for drag"))?;

                let from_sel = self
                    .apply_element_offset(from_selector, &from_params.align, &from_params.offset)
                    .await?;
                let to_sel = self
                    .apply_element_offset(to_selector, &to_params.align, &to_params.offset)
                    .await?;

                let (from_x, from_y) = match from_sel {
                    crate::driver::traits::Selector::Point { x, y } => (x, y),
                    _ => {
                        let timeout = self.context.default_timeout_ms;
                        let _ = self.driver.wait_for_element(&from_sel, timeout).await;
                        self.driver
                            .resolve_element_point(&from_sel, 0.5, 0.5)
                            .await?
                            .ok_or_else(|| anyhow::anyhow!("'from' element not found for drag: {:?}", from_sel))?
                    }
                };

                let (to_x, to_y) = match to_sel {
                    crate::driver::traits::Selector::Point { x, y } => (x, y),
                    _ => {
                        let timeout = self.context.default_timeout_ms;
                        let _ = self.driver.wait_for_element(&to_sel, timeout).await;
                        self.driver
                            .resolve_element_point(&to_sel, 0.5, 0.5)
                            .await?
                            .ok_or_else(|| anyhow::anyhow!("'to' element not found for drag: {:?}", to_sel))?
                    }
                };

                let duration = params.duration.unwrap_or(500);
                self.driver.drag((from_x, from_y), (to_x, to_y), duration).await
            }

            TestCommand::Pinch(params) => {
                let (cx, cy) = self.resolve_gesture_center(&params.at).await?;
                let (screen_width, screen_height) = self.driver.get_screen_size().await?;
                let max_radius = (screen_width.min(screen_height) as f64 / 2.0) * 0.9;
                let percent = params.percent.unwrap_or(50.0).clamp(5.0, 100.0);
                let target_radius = (max_radius * percent / 100.0) as i32;
                // "open" (zoom in): fingers start close together and spread apart to the
                // target radius. "close" (zoom out): the reverse.
                let (start_radius, end_radius) = match params.direction.as_str() {
                    "open" => (target_radius / 5, target_radius),
                    "close" => (target_radius, target_radius / 5),
                    other => {
                        return Err(anyhow::anyhow!(
                            "pinch direction must be \"open\" or \"close\", got \"{}\"",
                            other
                        ))
                    }
                };
                let duration = params.duration.unwrap_or(400);
                self.driver
                    .pinch(cx, cy, start_radius, end_radius, 0.0, duration)
                    .await
            }

            TestCommand::Shove(params) => {
                let (cx, cy) = self.resolve_gesture_center(&params.at).await?;
                let distance = params.distance.unwrap_or(300) as i32;
                let dy = match params.direction.as_str() {
                    // Mapbox/most map SDKs: fingers sliding UP the screen tilts the camera
                    // INTO 3D (increases pitch); sliding down flattens it back to 2D.
                    "up" => -distance,
                    "down" => distance,
                    other => {
                        return Err(anyhow::anyhow!(
                            "shove direction must be \"up\" or \"down\", got \"{}\"",
                            other
                        ))
                    }
                };
                // Fixed finger spacing approximating a natural 2-finger hand-span; shove
                // only cares about the shared vertical motion, not the spacing itself.
                const SHOVE_FINGER_SPACING_PX: i32 = 200;
                let duration = params.duration.unwrap_or(400);
                self.driver
                    .shove(cx, cy, SHOVE_FINGER_SPACING_PX, 0, dy, duration)
                    .await
            }

            // Mock Location Synchronization
            TestCommand::WaitForLocation(params) => {
                self.driver
                    .wait_for_location(
                        params.name.clone(),
                        params.lat,
                        params.lon,
                        params.tolerance,
                        params.timeout,
                    )
                    .await
            }

            TestCommand::WaitForMockCompletion(params) => {
                self.driver
                    .wait_for_mock_completion(params.name.clone(), params.timeout)
                    .await
            }

            TestCommand::MockLocationControl(params) => {
                self.driver
                    .control_mock_location(
                        params.name.clone(),
                        params.speed,
                        params.speed_mode.clone(),
                        params.speed_noise,
                        params.pause,
                        params.resume,
                    )
                    .await
            }

            // Performance & Load Testing
            TestCommand::StartProfiling(params) => {
                self.driver.start_profiling(params.clone()).await?;
                println!("  {} Started performance profiling", "⚡".green());
                Ok(())
            }

            TestCommand::StopProfiling(params) => {
                self.driver.stop_profiling().await?;
                println!("  {} Stopped performance profiling", "⚡".green());
                // Optional: Save report if path provided
                if let Some(p) = params.as_ref().and_then(|x| x.save_path.as_ref()) {
                    let subst_p = self.context.substitute_vars(p);
                    let metrics = self.driver.get_performance_metrics().await?;
                    let json = serde_json::to_string_pretty(&metrics)?;
                    let path = self.context.output_path(&subst_p);
                    std::fs::write(&path, json)?;
                    println!(
                        "  {} Saved performance report: {}",
                        "📄".green(),
                        path.display()
                    );
                }
                Ok(())
            }

            TestCommand::AssertPerformance(params) => {
                let metrics = self.driver.get_performance_metrics().await?;
                let subst_metric = self.context.substitute_vars(&params.metric);
                let metric_name = &subst_metric;
                let subst_limit = self.context.substitute_vars(&params.limit);
                let limit_str = &subst_limit;

                // Find metric (case-insensitive key search)
                let value = metrics
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(metric_name))
                    .map(|(_, v)| *v)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Metric '{}' not found in performance data. Available: {:?}",
                            metric_name,
                            metrics.keys()
                        )
                    })?;

                // Parse limit
                let (limit_val, _unit) = if limit_str.to_lowercase().ends_with("mb") {
                    (
                        limit_str
                            .to_lowercase()
                            .trim_end_matches("mb")
                            .trim()
                            .parse::<f64>()?,
                        "MB",
                    )
                } else if limit_str.to_lowercase().ends_with("kb") {
                    (
                        limit_str
                            .to_lowercase()
                            .trim_end_matches("kb")
                            .trim()
                            .parse::<f64>()?,
                        "kB",
                    )
                } else if limit_str.to_lowercase().ends_with("fps") {
                    (
                        limit_str
                            .to_lowercase()
                            .trim_end_matches("fps")
                            .trim()
                            .parse::<f64>()?,
                        "FPS",
                    )
                } else if limit_str.to_lowercase().ends_with("%") {
                    (
                        limit_str
                            .to_lowercase()
                            .trim_end_matches("%")
                            .trim()
                            .parse::<f64>()?,
                        "%",
                    )
                } else {
                    (limit_str.parse::<f64>()?, "")
                };

                // Check condition (Assuming limit is MAX allowed, except for FPS where it might be MIN?)
                // Usually "limit" implies upper bound for resource usage (RAM, CPU).
                // But for FPS, we usually want "min 60fps".
                // Heuristic: if fps, check >=. If memory/cpu, check <=.
                let passed = if metric_name.to_lowercase().contains("fps") {
                    value >= limit_val
                } else {
                    value <= limit_val
                };

                if passed {
                    println!(
                        "  {} Performance Check Passed: {} = {:.2} (Limit: {})",
                        "✓".green(),
                        metric_name,
                        value,
                        limit_str
                    );
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Performance Check Failed: {} = {:.2} (Limit: {})",
                        metric_name,
                        value,
                        limit_str
                    )
                }
            }

            TestCommand::SetCpuThrottling(rate) => {
                self.driver.set_cpu_throttling(*rate).await?;
                println!("  {} Set CPU throttling rate: {}x", "⚡".green(), rate);
                Ok(())
            }

            TestCommand::SetNetworkConditions(profile) => {
                let subst_profile = self.context.substitute_vars(profile);
                self.driver.set_network_conditions(&subst_profile).await?;
                println!("  {} Set network profile: {}", "⚡".green(), subst_profile);
                Ok(())
            }

            TestCommand::SelectDisplay(id_str) => {
                let id_val = self.context.substitute_vars(id_str);

                // Support "auto" keyword for auto-detection
                if id_val.eq_ignore_ascii_case("auto") {
                    // For Android, try to detect or create secondary display
                    if self.driver.platform_name() == "android" {
                        println!("  {} Auto-detecting secondary display...", "🔍".cyan());

                        // 1. Try to detect existing Android Auto display first
                        if let Ok(Some(id)) = self.driver.detect_android_auto_display().await {
                            println!("  {} Selected found display ID: {}", "📺".green(), id);
                            self.driver.select_display(id).await?;
                        } else {
                            // 2. If not found, create overlay display for simulation
                            println!(
                                "  {} No suitable display found, creating overlay...",
                                "⚠️".yellow()
                            );
                            let adb_path = crate::utils::binary_resolver::find_adb()
                                .unwrap_or_else(|_| std::path::PathBuf::from("adb"));
                            let _ = std::process::Command::new(adb_path)
                                .args(&[
                                    "shell",
                                    "settings",
                                    "put",
                                    "global",
                                    "overlay_display_devices",
                                    "1024x768/120",
                                ])
                                .output();

                            // Wait for display to be created
                            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

                            // Re-detect to find the new overlay
                            if let Ok(Some(id)) = self.driver.detect_android_auto_display().await {
                                println!(
                                    "  {} Created and selected overlay display ID: {}",
                                    "📺".green(),
                                    id
                                );
                                self.driver.select_display(id).await?;
                            } else {
                                // Fallback to likely ID 2
                                self.driver.select_display(2).await?;
                                println!("  {} Created overlay display and selected Display 2 (fallback)", "📺".green());
                            }
                        }
                    } else {
                        println!(
                            "  {} Auto-detect display only supported on Android",
                            "⚠".yellow()
                        );
                    }
                } else {
                    let id = id_val
                        .parse::<u64>()
                        .map_err(|e| anyhow::anyhow!("Invalid display ID '{}': {}", id_val, e))?;

                    // If switching back to display 0, cleanup overlay display
                    if id == 0 && self.driver.platform_name() == "android" {
                        let adb_path = crate::utils::binary_resolver::find_adb()
                            .unwrap_or_else(|_| std::path::PathBuf::from("adb"));
                        let _ = std::process::Command::new(adb_path)
                            .args(&[
                                "shell",
                                "settings",
                                "delete",
                                "global",
                                "overlay_display_devices",
                            ])
                            .output();
                        println!(
                            "  {} Removed overlay display and switched to main display",
                            "🧹".cyan()
                        );
                    }

                    self.driver.select_display(id).await?;
                }
                Ok(())
            }

            // Set device locale for i18n testing
            TestCommand::SetLocale(locale) => {
                let locale_val = self.context.substitute_vars(locale);
                self.driver.set_locale(&locale_val).await
            }

            // Audio Test Commands
            TestCommand::PlayMedia(params) => {
                let subst_file = self.context.substitute_vars(&params.file);
                let file_path = self.context.resolve_path(&subst_file);
                self.driver
                    .play_media(&file_path, params.loop_playback)
                    .await
            }

            TestCommand::StopMedia => self.driver.stop_media().await,

            TestCommand::StartAudioCapture(params) => {
                self.driver
                    .start_audio_capture(params.duration, params.port)
                    .await
            }

            TestCommand::StopAudioCapture => self.driver.stop_audio_capture().await,

            TestCommand::VerifyAudioDucking(params) => {
                self.driver
                    .verify_audio_ducking(params.min_ducking_count, params.volume_drop_threshold)
                    .await
            }

            TestCommand::SendLarkMessage(params) => {
                let webhook_url = self.context.substitute_vars(&params.webhook);

                let title = params
                    .title
                    .as_ref()
                    .map(|t| self.context.substitute_vars(t))
                    .unwrap_or_else(|| "Test Report".to_string());

                let mut content = self.context.substitute_vars(&params.content);

                // Append file contents if any
                if let Some(files) = &params.files {
                    for file_path_str in files {
                        // Resolve variable in file path
                        let resolved_path_str = self.context.substitute_vars(file_path_str);
                        let path = self.context.resolve_path(&resolved_path_str);

                        if path.exists() {
                            match std::fs::read_to_string(&path) {
                                Ok(file_content) => {
                                    let filename =
                                        path.file_name().unwrap_or_default().to_string_lossy();
                                    // Truncate if too long (Lark has limits)
                                    let display_content = if file_content.len() > 4000 {
                                        format!("{}... (truncated)", &file_content[..4000])
                                    } else {
                                        file_content
                                    };
                                    content.push_str(&format!(
                                        "\n\n📄 **File: {}**\n```\n{}\n```",
                                        filename, display_content
                                    ));
                                }
                                Err(e) => {
                                    content.push_str(&format!(
                                        "\n\n⚠️ Failed to read file {}: {}",
                                        resolved_path_str, e
                                    ));
                                }
                            }
                        } else {
                            content
                                .push_str(&format!("\n\n⚠️ File not found: {}", resolved_path_str));
                        }
                    }
                }

                // Determine status color/style
                let status = params.status.as_deref().unwrap_or("info");
                let theme_color = match status.to_lowercase().as_str() {
                    "success" => "green",
                    "failure" | "failed" | "error" => "red",
                    "warning" => "yellow",
                    _ => "blue",
                };

                // Prepare payload
                let mut payload_map = serde_json::Map::new();
                payload_map.insert(
                    "msg_type".to_string(),
                    serde_json::Value::String("interactive".to_string()),
                );

                // Add signature if secret is provided
                if let Some(secret_tmpl) = &params.secret {
                    let secret = self.context.substitute_vars(secret_tmpl);
                    if !secret.is_empty() {
                        let timestamp = chrono::Utc::now().timestamp();
                        let string_to_sign = format!("{}\n{}", timestamp, secret);

                        use base64::Engine;
                        use hmac::{Hmac, Mac};
                        use sha2::Sha256;

                        type HmacSha256 = Hmac<Sha256>;
                        // Note: Lark uses the timestamp+secret string as the HMAC key, and signs an empty message.
                        let mut mac = HmacSha256::new_from_slice(string_to_sign.as_bytes())
                            .expect("HMAC can take any size");
                        mac.update(&[]);
                        let signature_bytes = mac.finalize().into_bytes();
                        let sign =
                            base64::engine::general_purpose::STANDARD.encode(signature_bytes);

                        payload_map.insert(
                            "timestamp".to_string(),
                            serde_json::Value::Number(timestamp.into()),
                        );
                        payload_map.insert("sign".to_string(), serde_json::Value::String(sign));
                    }
                }

                // Construct Lark Card JSON
                let card_value = serde_json::json!({
                    "header": {
                        "title": {
                            "tag": "plain_text",
                            "content": title
                        },
                        "template": theme_color
                    },
                    "elements": [
                        {
                            "tag": "div",
                            "text": {
                                "tag": "lark_md",
                                "content": content
                            }
                        }
                    ]
                });
                payload_map.insert("card".to_string(), card_value);

                let payload = serde_json::Value::Object(payload_map);

                self.emitter.emit(TestEvent::Log {
                    message: format!("{} Sending Lark message to {}", "📨".cyan(), webhook_url),
                    depth: self.depth,
                });

                let client = reqwest::Client::new();
                let res = client
                    .post(&webhook_url)
                    .json(&payload)
                    .send()
                    .await
                    .context("Failed to send request to Lark")?;

                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    anyhow::bail!("Lark API failed: {} - {}", status, text);
                }

                Ok(())
            }

            TestCommand::AssertDeviceState(p) => {
                let camera = p.camera.as_ref().map(|c| self.context.substitute_vars(c));
                let button = self.context.substitute_vars(&p.button);
                let expect = self.context.substitute_vars(&p.expect);
                let key = self.ensure_camera(camera.as_deref()).await?;
                let session = self.camera_sessions.get(&key).expect("camera session");
                match session.assert_state_with_timeline(&button, &expect) {
                    Ok(()) => Ok(()),
                    Err((e, timeline)) => Err(self
                        .with_camera_timeline_evidence(&key, session, &button, e, Some(timeline))
                        .await),
                }
            }

            TestCommand::WaitDeviceState(p) => {
                let camera = p.camera.as_ref().map(|c| self.context.substitute_vars(c));
                let button = self.context.substitute_vars(&p.button);
                let expect = self.context.substitute_vars(&p.expect);
                let key = self.ensure_camera(camera.as_deref()).await?;
                let session = self.camera_sessions.get(&key).expect("camera session");
                match session
                    .wait_state_with_timeline(&button, &expect, p.timeout_ms, p.stable_frames)
                    .await
                {
                    Ok(()) => Ok(()),
                    Err((e, timeline)) => Err(self
                        .with_camera_timeline_evidence(&key, session, &button, e, Some(timeline))
                        .await),
                }
            }

            TestCommand::AssertDeviceTransition(p) => {
                let camera = p.camera.as_ref().map(|c| self.context.substitute_vars(c));
                let button = self.context.substitute_vars(&p.button);
                let from = self.context.substitute_vars(&p.from);
                let to = self.context.substitute_vars(&p.to);
                let key = self.ensure_camera(camera.as_deref()).await?;
                let session = self.camera_sessions.get(&key).expect("camera session");
                match session
                    .assert_transition_with_timeline(
                        &button,
                        &from,
                        &to,
                        p.timeout_ms,
                        p.stable_frames,
                    )
                    .await
                {
                    Ok(()) => Ok(()),
                    Err((e, timeline)) => Err(self
                        .with_camera_timeline_evidence(&key, session, &button, e, Some(timeline))
                        .await),
                }
            }

            TestCommand::WaitLedPattern(p) => {
                let camera = p.camera.as_ref().map(|c| self.context.substitute_vars(c));
                let button = self.context.substitute_vars(&p.button);
                let expect = self.context.substitute_vars(&p.expect);
                let key = self.ensure_camera(camera.as_deref()).await?;
                let session = self.camera_sessions.get(&key).expect("camera session");
                let pattern = crate::camera::pattern::BlinkPattern {
                    state: expect.clone(),
                    count: p.count.max(1),
                    within_ms: p.within_ms,
                    pulse_min_ms: p.pulse_min_ms,
                    pulse_max_ms: p.pulse_max_ms,
                };
                let result = session
                    .observe_blink_pattern(&button, pattern, p.timeout_ms, p.sample_ms)
                    .await
                    .map_err(|e| self.with_camera_evidence(&key, session, Some(&button), e))?;
                if result.matched {
                    Ok(())
                } else {
                    let evidence_dir = self
                        .capture_camera_evidence(&key, session, Some(&button))
                        .map_err(|e| anyhow::anyhow!("failed to capture camera evidence: {}", e))?;
                    let pattern_path = evidence_dir.join("pattern.json");
                    let pattern_json = serde_json::to_string_pretty(&result)?;
                    std::fs::write(&pattern_path, pattern_json)
                        .context("failed to save camera pattern timeline")?;
                    anyhow::bail!(
                        "timed out waiting for '{}' to blink '{}' {} time(s) within {}ms; observed {} pulse(s)\ncamera evidence: {}\npattern timeline: {}",
                        button,
                        expect,
                        p.count,
                        p.within_ms,
                        result.observed_count,
                        evidence_dir.display(),
                        pattern_path.display()
                    );
                }
            }

            TestCommand::GetDeviceState(p) => {
                let camera = p.camera.as_ref().map(|c| self.context.substitute_vars(c));
                let save_as = self.context.substitute_vars(&p.save_as);
                let key = self.ensure_camera(camera.as_deref()).await?;
                let session = self.camera_sessions.get(&key).expect("camera session");
                let state = session.read()?;
                let json = serde_json::to_string(&state)?;
                self.context.vars.insert(save_as.clone(), json);
                let safe_name = save_as
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>();
                let path = self
                    .context
                    .output_path(&format!("camera-state-{}.json", safe_name));
                let payload = serde_json::to_string_pretty(&serde_json::json!({
                    "camera": key,
                    "saveAs": save_as,
                    "state": state
                }))?;
                std::fs::write(&path, payload).with_context(|| {
                    format!("failed to save camera state artifact: {}", path.display())
                })?;
                self.emitter.emit(TestEvent::Log {
                    message: format!("📷 Camera state saved: {}", path.display()),
                    depth: self.depth,
                });
                Ok(())
            }

            TestCommand::V3(_) => {
                anyhow::bail!("HC commands must run through MultiTargetRunner")
            }

            // Unimplemented commands
            TestCommand::ExportReport(_)
            | TestCommand::Navigate(_)
            | TestCommand::Click(_)
            | TestCommand::Type(_) => {
                println!(
                    "  {} Command not yet implemented: {}",
                    "⚠".yellow(),
                    command.display_name()
                );
                Ok(())
            }
        }
    }

    /// Execute one delegated App command while preserving the same failure
    /// evidence path used by the normal flow runner.
    pub async fn execute_command_with_failure_artifacts(
        &mut self,
        command: &TestCommand,
        flow_name: &str,
        command_index: usize,
    ) -> Result<()> {
        match self.execute_command(command).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = self
                    .handle_failure(flow_name, command_index, &error.to_string())
                    .await;
                Err(error)
            }
        }
    }

    fn build_selector(
        &self,
        text: &Option<String>,
        regex: &Option<String>,
        id: &Option<String>,
        description: &Option<String>,
        relative: &Option<crate::parser::types::RelativeParams>,
        css: &Option<String>,
        xpath: &Option<String>,
        placeholder: &Option<String>,
        role: &Option<String>,
        element_type: &Option<String>,
        image: &Option<String>,
        index: Option<u32>,
        scrollable: &Option<crate::parser::types::ScrollableParams>,
        exact: bool,
        ocr: &Option<crate::parser::types::OcrSelectorInput>,
    ) -> Option<crate::driver::traits::Selector> {
        use crate::driver::traits::Selector;

        let idx = index.unwrap_or(0) as usize;

        let primary = if let Some(r) = regex {
            Selector::TextRegex(self.context.substitute_vars(r), idx)
        } else if let Some(t) = text {
            let subst_text = self.context.substitute_vars(t);
            if !exact && crate::parser::types::is_regex_string(&subst_text) {
                Selector::TextRegex(subst_text, idx)
            } else {
                Selector::Text(subst_text, idx, exact)
            }
        } else if let Some(i) = id {
            let subst_id = self.context.substitute_vars(i);
            if crate::parser::types::is_regex_string(&subst_id) {
                Selector::IdRegex(subst_id, idx)
            } else {
                Selector::Id(subst_id, idx)
            }
        } else if let Some(d) = description {
            let subst = self.context.substitute_vars(d);
            if crate::parser::types::is_regex_string(&subst) {
                Selector::DescriptionRegex(subst, idx)
            } else {
                Selector::Description(subst, idx)
            }
        } else if let Some(p) = placeholder {
            Selector::Placeholder(self.context.substitute_vars(p), idx)
        } else if let Some(r) = role {
            Selector::Role(self.context.substitute_vars(r), idx)
        } else if let Some(e) = element_type {
            Selector::Type(self.context.substitute_vars(e), idx)
        } else if let Some(c) = css {
            Selector::Css(self.context.substitute_vars(c))
        } else if let Some(img) = image {
            let resolved = self.context.resolve_path(img);
            Selector::Image {
                path: resolved.to_string_lossy().to_string(),
                region: None,
            }
        } else if let Some(ocr_input) = ocr {
            // OCR selector - similar pattern to image selector
            Selector::OCR(
                self.context.substitute_vars(ocr_input.text()),
                ocr_input.index(),
                ocr_input.is_regex(),
                ocr_input.region().map(|s| s.to_string()),
            )
        } else if let Some(x) = xpath {
            Selector::XPath(self.context.substitute_vars(x))
        } else if let Some(scroll_params) = scrollable {
            Selector::ScrollableItem {
                scrollable_index: scroll_params.index.unwrap_or(0) as usize,
                item_index: scroll_params.item_index.map(|i| i as usize),
            }
        } else if relative.is_some() {
            // When only relative is specified, default to Type("") (match all) as target
            // XPath is not supported by driver.rs relative search, but Type("") matches all classes
            Selector::Type("".to_string(), idx)
        } else {
            return None;
        };

        if let Some(rel) = relative {
            let (dir, anchor_input) = if let Some(input) = &rel.right_of {
                (crate::driver::traits::RelativeDirection::RightOf, input)
            } else if let Some(input) = &rel.left_of {
                (crate::driver::traits::RelativeDirection::LeftOf, input)
            } else if let Some(input) = &rel.above {
                (crate::driver::traits::RelativeDirection::Above, input)
            } else if let Some(input) = &rel.below {
                (crate::driver::traits::RelativeDirection::Below, input)
            } else {
                return Some(primary);
            };

            let anchor_selector = match anchor_input {
                crate::parser::types::RelativeAnchorInput::String(s) => {
                    let subst = self.context.substitute_vars(s);
                    // Try to parse as JSON first (to support recursive object selectors)
                    if subst.trim().starts_with('{') {
                        if let Ok(p) =
                            serde_json::from_str::<crate::parser::types::AnchorParams>(&subst)
                        {
                            // Recursive build for anchor from strict Struct(p) logic below
                            let idx = p.index.unwrap_or(0) as usize;

                            if let Some(r) = &p.regex {
                                Selector::TextRegex(self.context.substitute_vars(r), idx)
                            } else if let Some(t) = &p.text {
                                Selector::Text(self.context.substitute_vars(t), idx, p.exact)
                            } else if let Some(id) = &p.id {
                                let s = self.context.substitute_vars(id);
                                if crate::parser::types::is_regex_string(&s) {
                                    Selector::IdRegex(s, idx)
                                } else {
                                    Selector::Id(s, idx)
                                }
                            } else if let Some(d) = &p.description {
                                let s = self.context.substitute_vars(d);
                                if crate::parser::types::is_regex_string(&s) {
                                    Selector::DescriptionRegex(s, idx)
                                } else {
                                    Selector::Description(s, idx)
                                }
                            } else if let Some(img) = &p.image {
                                let resolved = self.context.resolve_path(img);
                                Selector::Image {
                                    path: resolved.to_string_lossy().to_string(),
                                    region: None,
                                }
                            } else if let Some(e) = &p.element_type {
                                Selector::Type(self.context.substitute_vars(e), idx)
                            } else if let Some(c) = &p.css {
                                Selector::Css(self.context.substitute_vars(c))
                            } else if let Some(x) = &p.xpath {
                                Selector::XPath(self.context.substitute_vars(x))
                            } else if let Some(role) = &p.role {
                                Selector::Role(self.context.substitute_vars(role), idx)
                            } else if let Some(ph) = &p.placeholder {
                                Selector::Placeholder(self.context.substitute_vars(ph), idx)
                            } else {
                                // If struct is empty or invalid, fallback to text
                                Selector::Text(subst, 0, false)
                            }
                        } else {
                            // JSON parsing failed, treat as string
                            if crate::parser::types::is_regex_string(&subst) {
                                Selector::TextRegex(subst, 0)
                            } else {
                                Selector::Text(subst, 0, false)
                            }
                        }
                    } else {
                        // Not a JSON string
                        if crate::parser::types::is_regex_string(&subst) {
                            Selector::TextRegex(subst, 0)
                        } else {
                            Selector::Text(subst, 0, false)
                        }
                    }
                }
                crate::parser::types::RelativeAnchorInput::Struct(p) => {
                    let resolved_params = if let Some(element_ref) = &p.element {
                        // Direct lookup if element_ref is a variable reference (e.g. "${var}")
                        let resolved =
                            if element_ref.starts_with("${") && element_ref.ends_with("}") {
                                let var_name = &element_ref[2..element_ref.len() - 1];
                                if let Some(val) = self.context.vars.get(var_name) {
                                    val.clone()
                                } else {
                                    self.context.substitute_vars(element_ref)
                                }
                            } else {
                                self.context.substitute_vars(element_ref)
                            };
                        if resolved.trim().starts_with('{') {
                            // Try to parse as TapParams first (since Find stores TapParams)
                            if let Ok(tap_params) =
                                serde_json::from_str::<crate::parser::types::TapParams>(&resolved)
                            {
                                // Convert TapParams to AnchorParams
                                Some(crate::parser::types::AnchorParams {
                                    element: None,
                                    text: tap_params.text,
                                    regex: tap_params.regex,
                                    id: tap_params.id,
                                    css: tap_params.css,
                                    xpath: tap_params.xpath,
                                    placeholder: tap_params.placeholder,
                                    role: tap_params.role,
                                    description: tap_params.description,
                                    element_type: tap_params.element_type,
                                    image: tap_params.image,
                                    exact: tap_params.exact,
                                    index: tap_params.index,
                                })
                            } else {
                                serde_json::from_str::<crate::parser::types::AnchorParams>(
                                    &resolved,
                                )
                                .ok()
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let params = resolved_params.as_ref().unwrap_or(p);

                    // Recursive build for anchor (simplified manual recursion)
                    // We map AnchorParams fields to build_selector-like logic
                    let idx = params.index.unwrap_or(0) as usize;

                    if let Some(r) = &params.regex {
                        Selector::TextRegex(self.context.substitute_vars(r), idx)
                    } else if let Some(t) = &params.text {
                        Selector::Text(self.context.substitute_vars(t), idx, params.exact)
                    } else if let Some(id) = &params.id {
                        let s = self.context.substitute_vars(id);
                        if crate::parser::types::is_regex_string(&s) {
                            Selector::IdRegex(s, idx)
                        } else {
                            Selector::Id(s, idx)
                        }
                    } else if let Some(d) = &params.description {
                        let s = self.context.substitute_vars(d);
                        if crate::parser::types::is_regex_string(&s) {
                            Selector::DescriptionRegex(s, idx)
                        } else {
                            Selector::Description(s, idx)
                        }
                    } else if let Some(img) = &params.image {
                        let resolved = self.context.resolve_path(img);
                        Selector::Image {
                            path: resolved.to_string_lossy().to_string(),
                            region: None,
                        }
                    } else if let Some(e) = &params.element_type {
                        Selector::Type(self.context.substitute_vars(e), idx)
                    } else if let Some(c) = &params.css {
                        Selector::Css(self.context.substitute_vars(c))
                    } else if let Some(x) = &params.xpath {
                        Selector::XPath(self.context.substitute_vars(x))
                    } else if let Some(role) = &params.role {
                        Selector::Role(self.context.substitute_vars(role), idx)
                    } else if let Some(ph) = &params.placeholder {
                        Selector::Placeholder(self.context.substitute_vars(ph), idx)
                    } else {
                        // Fallback?
                        return None;
                    }
                }
            };

            Some(Selector::Relative {
                target: Box::new(primary),
                anchor: Box::new(anchor_selector),
                direction: dir,
                max_dist: rel.max_dist,
            })
        } else {
            Some(primary)
        }
    }

    fn selector_from_extended_wait_value(
        &self,
        value: &serde_json::Value,
    ) -> Result<crate::driver::traits::Selector> {
        if let Ok(params_input) =
            serde_json::from_value::<crate::parser::types::AssertParamsInput>(value.clone())
        {
            let params = self.resolve_assert_params(&params_input);
            self.build_selector(
                &params.text,
                &params.regex,
                &params.id,
                &params.description,
                &params.relative,
                &params.css,
                &params.xpath,
                &params.placeholder,
                &params.role,
                &params.element_type,
                &params.image,
                params.index,
                &params.scrollable,
                false,
                &params.ocr,
            )
            .ok_or_else(|| anyhow::anyhow!("No selector specified for extendedWaitUntil"))
        } else {
            anyhow::bail!(
                "extendedWaitUntil expects visible/notVisible to be a string or selector object"
            )
        }
    }

    /// Handle command failure by dumping UI, screenshot, and recent logs.
    async fn handle_failure(
        &self,
        flow_name: &str,
        index: usize,
        _error: &str,
    ) -> FailureArtifacts {
        let safe_flow_name = flow_name.replace("/", "_").replace("\\", "_");
        let mut artifacts = FailureArtifacts::default();

        // Check for app crash (do this first to include in logs)
        // Only detects real crashes (FATAL EXCEPTION in logcat), not intentional stops
        if let Some(ref app_id) = self.context.app_id {
            match self.driver.detect_app_crash(app_id).await {
                Ok(crashed) if crashed => {
                    self.emitter.emit(TestEvent::AppCrashed {
                        app_id: app_id.clone(),
                        flow_name: flow_name.to_string(),
                        command_index: index,
                        depth: self.depth,
                    });
                }
                _ => {}
            }
        }

        if !self.report_enabled && !self.snapshot_enabled {
            return artifacts;
        }

        self.emitter.emit(TestEvent::Log {
            message: format!("  {} Capturing failure context...", "ℹ".blue()),
            depth: self.depth,
        });

        let uuid = Uuid::new_v4().to_string();
        let timestamp = chrono::Local::now().format("%H%M%S");

        // Evidence lives under this run's own session folder, split by kind
        // (images/logs) so a device with many sessions doesn't end up with
        // hundreds of loose fail_*.png/.xml/.log files mixed together with no
        // indication of which session - or which kind of artifact - they are.
        // The report (sessions/<id>/report/summary.html) links back into these.
        let evidence_dir = self
            .context
            .output_path(&format!("sessions/{}/evidence", self.session.session_id));
        let images_dir = evidence_dir.join("images");
        let logs_dir = evidence_dir.join("logs");
        let _ = std::fs::create_dir_all(&images_dir);
        let _ = std::fs::create_dir_all(&logs_dir);

        // 1. Screenshot
        let filename = format!(
            "fail_{}_{}_cmd{}_{}.png",
            safe_flow_name,
            timestamp,
            index,
            &uuid[..8]
        );
        let path = images_dir.join(&filename);
        let path_str = path.to_string_lossy().to_string();

        match self.driver.take_screenshot(&path_str).await {
            Ok(_) => {
                // Re-encode as lossless WebP for storage - evidence accumulates
                // fast across a long-running --snapshot run, and WebP lands
                // meaningfully smaller than PNG for this flat-fill, sharp-edge
                // UI content while keeping every pixel exact. Fall back to the
                // original PNG on any conversion error rather than losing the
                // evidence.
                let final_path = match crate::utils::image_convert::convert_to_webp_in_place(&path)
                {
                    Ok(webp_path) => webp_path,
                    Err(e) => {
                        println!(
                            "  {} Failed to convert screenshot to WebP, keeping PNG: {}",
                            "⚠".yellow(),
                            e
                        );
                        path.clone()
                    }
                };
                println!("  {} Saved Screenshot: {}", "📸".green(), final_path.display());
                artifacts.screenshot_path = Some(final_path.display().to_string());
            }
            Err(e) => println!("  {} Failed to take screenshot: {}", "⚠".yellow(), e),
        }

        // 2. Logcat (Recent 1000 lines)
        match self.driver.dump_logs(1000).await {
            Ok(logs) => {
                // Must use safe_flow_name (slashes replaced with "_"), not the raw
                // flow_name - a nested subflow's name (e.g. "subflows/select_home.yaml")
                // contains "/", which `logs_dir.join(&filename)` treats as a real
                // subdirectory separator that doesn't exist, silently failing the
                // write. This is exactly why failure logs were missing from reports
                // for subflow failures while the screenshot capture (which already
                // used safe_flow_name) succeeded.
                let filename = format!(
                    "fail_{}_{}_cmd{}_{}.log",
                    safe_flow_name,
                    timestamp,
                    index,
                    &uuid[..8]
                );
                let path = logs_dir.join(&filename);
                if let Ok(_) = std::fs::write(&path, logs) {
                    println!("  {} Saved Recent Logs: {}", "📋".green(), path.display());
                    artifacts.log_path = Some(path.display().to_string());
                }
            }
            Err(e) => println!("  {} Failed to dump logs: {}", "⚠".yellow(), e),
        }

        artifacts
    }

    /// Crop image by percentage region
    fn crop_image(&self, bytes: &[u8], crop_str: &str) -> Result<Vec<u8>> {
        let parts: Vec<f32> = crop_str
            .split(',')
            .filter_map(|s| s.trim().trim_end_matches('%').parse().ok())
            .collect();

        if parts.len() != 4 {
            anyhow::bail!("Invalid crop format, expected: left%,top%,width%,height%");
        }

        let img = image::load_from_memory(bytes)?;
        let (w, h) = (img.width() as f32, img.height() as f32);

        let x = (parts[0] / 100.0 * w) as u32;
        let y = (parts[1] / 100.0 * h) as u32;
        let cw = (parts[2] / 100.0 * w) as u32;
        let ch = (parts[3] / 100.0 * h) as u32;

        let cropped = img.crop_imm(x, y, cw, ch);

        let mut buf = std::io::Cursor::new(Vec::new());
        cropped.write_to(&mut buf, image::ImageFormat::Png)?;
        Ok(buf.into_inner())
    }

    /// Try to auto-capture a GIF frame if interval has passed
    async fn try_auto_capture(&mut self) {
        if !self.auto_capture_active {
            return;
        }

        // Check if we've reached max frames
        if self.auto_capture_frames.len() >= self.auto_capture_max as usize {
            return;
        }

        // Check if interval has passed
        let elapsed = self.auto_capture_last_time.elapsed().as_millis() as u64;
        if elapsed < self.auto_capture_interval {
            return;
        }

        // Capture frame
        let temp_path = format!("/tmp/auto_gif_frame_{}.png", uuid::Uuid::new_v4());
        if let Ok(()) = self.driver.take_screenshot(&temp_path).await {
            if let Ok(bytes) = std::fs::read(&temp_path) {
                self.auto_capture_frames.push(bytes);
                std::fs::remove_file(&temp_path).ok();
            }
        }

        self.auto_capture_last_time = std::time::Instant::now();
    }

    /// Finish a delegated App session without taking ownership of aggregate
    /// multi-target artifacts. The parent target runner writes those once.
    pub async fn finish_without_artifacts(&mut self) -> Result<()> {
        self.stop_observe_views();
        self.session.finish();
        let summary = self.session.summary();
        self.emitter.emit(TestEvent::SessionFinished { summary });
        Ok(())
    }

    /// Finish the test session and generate reports
    pub async fn finish(&mut self) -> Result<()> {
        self.stop_observe_views();
        self.session.finish();

        let summary = self.session.summary();
        self.emitter.emit(TestEvent::SessionFinished {
            summary: summary.clone(),
        });

        // Persist a lightweight run manifest for agents even when full reports are disabled.
        let report_data = self.session.to_report();
        let manifest_path = self.context.output_path("run.json");
        let manifest_json = serde_json::to_string_pretty(&report_data)?;
        std::fs::write(&manifest_path, manifest_json)?;

        if !self.report_enabled {
            // Wait for ConsoleEventListener to process remaining events before exiting
            // This is needed because the listener runs in tokio::spawn and needs time to print output
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            return Ok(());
        }

        // Small delay to ensure SessionFinished event is processed before printing reports
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Convert TestSessionReport to TestResults (adds `generated_at`, which the
        // `lumi-tester report <file>` subcommand requires when it later reads
        // test-results.json back in - report_data/TestSessionReport doesn't carry
        // that field, so writing test-results.json straight from report_data would
        // silently produce a file that subcommand can't deserialize).
        let test_results = crate::report::types::TestResults {
            session_id: report_data.session_id.clone(),
            flows: report_data.flows.clone(),
            summary: report_data.summary.clone(),
            generated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };

        // Save JSON report (machine-readable input for `lumi-tester report` and CI tooling)
        let report_path = self.context.output_path("test-results.json");
        let json = serde_json::to_string_pretty(&test_results)?;
        std::fs::write(&report_path, json)?;

        // Generate Session-structured output folder matching "Phần mềm - Tool test thiết bị.md"
        let session_dir = self.context.output_path(&format!("sessions/{}", report_data.session_id));
        let report_dir = session_dir.join("report");
        let _ = std::fs::create_dir_all(&report_dir);

        let session_info_path = session_dir.join("session.json");
        let session_info_json = serde_json::to_string_pretty(&serde_json::json!({
            "session_id": report_data.session_id,
            "summary": report_data.summary,
            "created_at": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
        }))?;
        let _ = std::fs::write(&session_info_path, session_info_json);

        let platform_str = self.driver.platform_name();
        let app_id_str = self.context.app_id.as_deref();

        // Default title to a name that identifies the WHOLE session rather than a
        // generic label - makes the report identifiable in a browser tab/history
        // when several are open at once, since "session_id" (a timestamped slug)
        // isn't memorable on its own. For a single-flow run the flow's own name is
        // the most specific and useful choice; for a directory/multi-file run
        // (this session covers several unrelated top-level files) the FIRST flow's
        // name is misleading (e.g. "subflows/launch_clean.yaml" for a 58-flow batch
        // that ran the whole `lumi_life` workspace) - fall back to the target name
        // baked into the session id instead (same extraction the sessions
        // dashboard already uses in `html::generate_sessions_dashboard`).
        let summary_title = if report_data.flows.len() == 1 {
            report_data.flows.first().map(|f| f.flow_name.clone())
        } else {
            Some(crate::report::html::target_name_from_session_id(
                &report_data.session_id,
                report_data.flows.len(),
            ))
        };
        let summary_html_path = report_dir.join("summary.html");
        let _ = crate::report::summary_html::generate(
            &test_results,
            app_id_str,
            Some(&platform_str),
            summary_title.as_deref(),
            Some(&self.context.output_dir),
            Some(&summary_html_path),
        )
        .await;

        // Generate and update Sessions History Dashboard (index.html)
        let _ = crate::report::html::generate_sessions_dashboard(&self.context.output_dir);
        let dashboard_path = self.context.output_path("index.html");

        let to_file_url = |p: &std::path::Path| -> String {
            if let Ok(abs) = std::fs::canonicalize(p) {
                format!("file://{}", abs.display().to_string().replace('\\', "/"))
            } else if let Ok(cwd) = std::env::current_dir() {
                let abs = cwd.join(p);
                format!("file://{}", abs.display().to_string().replace('\\', "/"))
            } else {
                p.display().to_string()
            }
        };

        println!(
            "\n{} JSON report: {}",
            "📄".blue(),
            to_file_url(&report_path).cyan()
        );
        println!(
            "{} Session report: {}",
            "📋".blue(),
            to_file_url(&summary_html_path).cyan()
        );
        println!(
            "{} Sessions Dashboard: {}",
            "🗂️ ".blue(),
            to_file_url(&dashboard_path).cyan()
        );

        // Generate and save JUnit report
        crate::report::junit::write_report(&test_results, &self.context.output_dir)?;

        Ok(())
    }

    async fn evaluate_condition_value(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::String(s) => {
                let subst = self.context.substitute_vars(s);
                use super::js_engine::JsEngine;
                let mut engine = JsEngine::new();
                engine.set_vars(&self.context.vars);
                engine.set_vars(&self.context.env);
                if let Ok(res) = engine.eval_bool(&subst) {
                    res
                } else {
                    let re = regex::Regex::new(r"\$\{([a-zA-Z0-9_]+)\}").unwrap();
                    let ident_expr = re.replace_all(s, "$1").to_string();
                    engine.eval_bool(&ident_expr).unwrap_or(false)
                }
            }
            serde_json::Value::Number(n) => n.as_f64().map_or(false, |v| v != 0.0),
            serde_json::Value::Object(map) => {
                if let Some(v) = map.get("true") {
                    return Box::pin(self.evaluate_condition_value(v)).await;
                }
                if let Some(v) = map.get("false") {
                    return !Box::pin(self.evaluate_condition_value(v)).await;
                }

                if let Ok(cond) =
                    serde_json::from_value::<crate::parser::types::Condition>(value.clone())
                {
                    return self.check_condition(&cond).await;
                }
                true
            }
            _ => true,
        }
    }

    async fn check_condition(&self, cond: &crate::parser::types::Condition) -> bool {
        use crate::driver::traits::Selector;

        if let Some(ref text) = cond.visible {
            let text = self.context.substitute_vars(text);
            let selector = if crate::parser::types::is_regex_string(&text) {
                Selector::TextRegex(text, 0)
            } else {
                Selector::Text(text, 0, false)
            };
            return self.driver.is_visible(&selector).await.unwrap_or(false);
        }
        if let Some(ref re) = cond.visible_regex {
            let re = self.context.substitute_vars(re);
            let selector = Selector::TextRegex(re, 0);
            return self.driver.is_visible(&selector).await.unwrap_or(false);
        }
        if let Some(ref text) = cond.not_visible {
            let text = self.context.substitute_vars(text);
            let selector = if crate::parser::types::is_regex_string(&text) {
                Selector::TextRegex(text, 0)
            } else {
                Selector::Text(text, 0, false)
            };
            return !self.driver.is_visible(&selector).await.unwrap_or(false);
        }
        if let Some(ref re) = cond.not_visible_regex {
            let re = self.context.substitute_vars(re);
            let selector = Selector::TextRegex(re, 0);
            return !self.driver.is_visible(&selector).await.unwrap_or(false);
        }
        true
    }
}

fn format_hw_label(name: Option<&str>, ch: u8) -> String {
    if let Some(n) = name {
        let trimmed = n.trim();
        if trimmed.parse::<u8>().is_ok() {
            format!("ch {}", ch)
        } else {
            format!("{} (ch {})", trimmed, ch)
        }
    } else {
        format!("ch {}", ch)
    }
}

fn format_hw_channels_label(name: Option<&str>, channels: &[u8]) -> String {
    if let Some(n) = name {
        let trimmed = n.trim();
        if trimmed.parse::<u8>().is_ok() {
            if channels.len() == 1 {
                format!("ch {}", channels[0])
            } else {
                format!("ch {:?}", channels)
            }
        } else if channels.len() == 1 {
            format!("{} (ch {})", trimmed, channels[0])
        } else {
            format!("{} (ch {:?})", trimmed, channels)
        }
    } else if channels.len() == 1 {
        format!("ch {}", channels[0])
    } else {
        format!("ch {:?}", channels)
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_camera_config_vars;
    use crate::parser::types::CameraFlowConfig;
    use crate::runner::context::TestContext;
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn resolves_camera_rtsp_from_context_env() {
        let mut context = TestContext::new(Path::new("."), None, false, None);
        let rtsp = format!("{}{}:{}{}10.0.0.5/live", "rtsp://", "user", "pass", "@");
        context.env.insert("CAMERA_RTSP".to_string(), rtsp.clone());

        let mut configs = HashMap::new();
        configs.insert(
            "default".to_string(),
            CameraFlowConfig {
                rtsp: "${CAMERA_RTSP}".to_string(),
                server: None,
                profile: Some("profiles/${PROFILE_NAME}.json".to_string()),
                transport: Some("tcp".to_string()),
                observe: false,
            },
        );
        context
            .env
            .insert("PROFILE_NAME".to_string(), "switch4".to_string());

        let resolved = resolve_camera_config_vars(&context, configs);
        let cfg = resolved.get("default").unwrap();
        assert_eq!(cfg.rtsp, rtsp);
        assert_eq!(cfg.profile.as_deref(), Some("profiles/switch4.json"));
        assert_eq!(cfg.transport.as_deref(), Some("tcp"));
    }

    #[test]
    fn test_variable_substitution_in_commands() {
        let mut context = TestContext::new(Path::new("."), None, false, None);
        context.vars.insert("OFFSET_VAL".to_string(), "90%,50%".to_string());
        context.vars.insert("ALIGN_VAL".to_string(), "right".to_string());

        let subst_offset = Some(context.substitute_vars("${OFFSET_VAL}"));
        let subst_align = Some(context.substitute_vars("${ALIGN_VAL}"));
        let offset = super::resolve_element_offset(&subst_align, &subst_offset);
        assert_eq!(offset, Some((0.9, 0.5)));

        // Window size string substitution
        context.vars.insert("WIN_DIMS".to_string(), "1920x1080".to_string());
        let subst_win = context.substitute_vars("${WIN_DIMS}");
        let win_input = crate::parser::types::SetWindowSizeParamsInput::String(subst_win);
        assert_eq!(win_input.to_dimensions(), Some((1920, 1080)));

        // Point substitution
        context.vars.insert("TAP_COORDS".to_string(), "50%,80%".to_string());
        let subst_pt = context.substitute_vars("${TAP_COORDS}");
        let parts: Vec<&str> = subst_pt.split(',').collect();
        assert_eq!(parts, vec!["50%", "80%"]);
    }

    #[tokio::test]
    async fn test_dsl_conditions_and_matching() {
        let mut context = TestContext::new(Path::new("."), None, false, None);
        context.vars.insert("ENV".to_string(), "staging".to_string());
        context.vars.insert("COUNT".to_string(), "5".to_string());

        let cond_val = "${ENV} == 'staging'";
        let mut engine = crate::runner::js_engine::JsEngine::new();
        engine.set_vars(&context.vars);
        let re = regex::Regex::new(r"\$\{([a-zA-Z0-9_]+)\}").unwrap();
        let ident_expr = re.replace_all(cond_val, "$1").to_string();
        assert!(engine.eval_bool(&ident_expr).unwrap());

        let cond_false = "${COUNT} > 10";
        let ident_false = re.replace_all(cond_false, "$1").to_string();
        assert!(!engine.eval_bool(&ident_false).unwrap());

        // Also test quoted string substitution
        let quoted = "'${ENV}' == 'staging'";
        let subst_quoted = context.substitute_vars(quoted);
        assert!(engine.eval_bool(&subst_quoted).unwrap());
    }

    #[test]
    fn test_tap_regex_shorthand_resolution() {
        let input = crate::parser::types::TapParamsInput::String("Login with.*".to_string());
        let params = input.into_inner();
        assert_eq!(params.regex.as_deref(), Some("Login with.*"));
        assert_eq!(params.text, None);
    }
}
