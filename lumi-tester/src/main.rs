use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

mod ai;

use lumi_tester::{driver, recorder, report, requirements, runner, utils};

#[derive(Parser)]
#[command(name = "lumi-tester")]
#[command(author = "NL Team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Multi-platform automation testing CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run test file(s) or directory
    Run {
        /// Path to test file or directory
        path: PathBuf,

        /// Target platform (android, android_auto, ios, web, macos, windows, lumi_e2e).
        /// Parsed from file if not provided.
        #[arg(short, long)]
        platform: Option<String>,

        /// Device serial(s) (Android) or UDID(s) (iOS). Can be specified multiple times.
        #[arg(short, long)]
        device: Vec<String>,

        /// Run tests in parallel across multiple devices
        #[arg(long, default_value = "false")]
        parallel: bool,

        /// Output directory for reports and artifacts
        #[arg(short, long, default_value = "./output")]
        output: PathBuf,

        /// Continue on failure
        #[arg(long, default_value = "false")]
        continue_on_failure: bool,

        /// Enable video recording during test execution
        #[arg(long, short = 'r', default_value = "false")]
        record: bool,

        /// Enable screenshot capture on failures
        #[arg(long, short = 's', default_value = "false")]
        snapshot: bool,

        /// Generate reports (JSON, HTML, JUnit)
        #[arg(long, default_value = "false")]
        report: bool,

        /// Write machine-readable execution events to output/events.jsonl
        #[arg(long, default_value = "false")]
        events_jsonl: bool,

        /// Filter tests by tags (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        tags: Option<Vec<String>>,

        /// Run only a specific command by index (0-based)
        #[arg(long)]
        command_index: Option<usize>,

        /// Run commands starting from this 0-based index to the end of the file
        #[arg(long, alias = "from-index", alias = "start-from")]
        from_command_index: Option<usize>,

        /// Run only a specific command by name (first match)
        #[arg(long)]
        command_name: Option<String>,

        /// Repeat the entire test file N times
        #[arg(long, default_value = "1")]
        repeat: u32,

        /// Path to test data file (CSV or JSON) for data-driven testing
        #[arg(long, alias = "data-file")]
        data: Option<PathBuf>,

        /// Execution speed profile override (turbo, fast, normal, safe)
        #[arg(long)]
        speed: Option<String>,
    },

    /// List connected devices
    Devices {
        /// Target platform
        #[arg(short, long, default_value = "android")]
        platform: String,
    },

    /// Generate report from test results
    Report {
        /// Path to test results JSON
        results: PathBuf,

        /// Output format (json, html)
        #[arg(short, long, default_value = "html")]
        format: String,

        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Validate YAML test file(s) without launching a device or browser
    Validate {
        /// Path to test file or directory
        path: PathBuf,

        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// List discovered test files and command indexes without running tests
    List {
        /// Path to test file or directory
        path: PathBuf,

        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// Compute SRS requirements coverage from a `requirements/index.yaml`
    /// against `cases.csv` - never trust a hand-written status field, always
    /// compute from files that actually exist on disk. See "SRS Requirements
    /// Format" in the AI skill's testcase-design.md for the file layout.
    RequirementsCoverage {
        /// Path to `requirements/index.yaml`
        path: PathBuf,

        /// Path to `cases.csv` - the testcase matrix that references
        /// requirement ids. Omit to only check `skip` vs unskipped counts
        /// (every non-skipped requirement will report as missing).
        #[arg(long)]
        cases: Option<PathBuf>,

        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// Check local automation dependencies
    Doctor {
        /// Target platform to check (android, android_auto, ios, web, macos, windows, all)
        #[arg(short, long, default_value = "android")]
        platform: String,

        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// Print the bundled Lumi YAML JSON Schema
    Schema {
        /// Print machine-readable JSON
        #[arg(long, default_value = "true")]
        json: bool,
    },

    Shell {
        /// Target platform
        #[arg(short, long, default_value = "android")]
        platform: String,

        /// Device serial (Android) or UDID (iOS)
        #[arg(short, long)]
        device: Option<String>,

        /// Run this single command and exit, instead of starting the interactive prompt -
        /// no YAML file needed. Repeatable: each occurrence runs in order against the same
        /// device session (one connection for all of them, not one per command). Accepts
        /// the exact same YAML-sugar syntax as a command in a test file's command list,
        /// e.g. `-c 'tapOn: "Login"' -c 'pinch: {direction: open, percent: 60}'`.
        #[arg(short = 'c', long = "command")]
        command: Vec<String>,

        /// With -c/--command, print one JSON object per line
        /// (`{"command":"...","success":bool,"error":string|null}`) instead of colored
        /// text - for a script or AI agent to parse the per-command result, not just rely
        /// on the overall exit code.
        #[arg(long)]
        json: bool,
    },

    /// Manage system components
    System {
        #[command(subcommand)]
        command: SystemCommands,
    },

    /// Install AI agent integrations for Lumi Tester
    Ai {
        #[command(subcommand)]
        command: AiCommands,
    },

    /// Record user interactions and generate YAML test file
    Record {
        /// Output file path for the generated YAML
        #[arg(short, long)]
        output: PathBuf,

        /// Device serial (Android)
        #[arg(short, long)]
        device: Option<String>,

        /// App ID to record (auto-detected if not provided)
        #[arg(short, long)]
        app: Option<String>,

        /// Test name for the generated file
        #[arg(short, long)]
        name: Option<String>,

        /// Include wait commands between actions
        #[arg(long, default_value = "true")]
        include_waits: bool,

        /// Include selector alternatives as comments
        #[arg(long, default_value = "true")]
        include_comments: bool,
    },

    /// Camera-based hardware verification (read device LED states over RTSP)
    Camera {
        #[command(subcommand)]
        command: CameraCommands,
    },

    /// Start web-based inspector for visual test creation
    Inspect {
        /// Target platform (android, ios, web)
        #[arg(short, long, default_value = "android")]
        platform: String,

        /// Device serial (Android) or UDID (iOS)
        #[arg(short, long)]
        device: Option<String>,

        /// Server port
        #[arg(long, default_value = "9333")]
        port: u16,

        /// Output YAML file (optional, can be selected in UI)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Suggest ranked, cross-platform-correct selectors for elements matching a
    /// query and/or near a point - for use when no Inspector session is running
    /// (e.g. from an AI agent working from a saved hierarchy dump)
    SuggestSelectors {
        /// Target platform (android, ios, macos, windows, web)
        #[arg(short, long, default_value = "android")]
        platform: String,

        /// Device serial (Android) or UDID (iOS). Omit when using --file
        #[arg(short, long)]
        device: Option<String>,

        /// App bundle id / package to scope the hierarchy dump to (iOS/macOS)
        #[arg(long)]
        app_id: Option<String>,

        /// Case-insensitive substring match against text/resource-id/content-desc/class
        #[arg(short, long)]
        query: Option<String>,

        /// "x,y" - rank by proximity to this point (e.g. from a screenshot click)
        #[arg(long)]
        point: Option<String>,

        /// Read a previously-saved hierarchy dump instead of querying a live device
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Screen width in px - only used with --file, ignored for a live device
        #[arg(long, default_value = "1080")]
        width: u32,

        /// Screen height in px - only used with --file, ignored for a live device
        #[arg(long, default_value = "1920")]
        height: u32,

        /// Max number of suggestions to return
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Include elements that aren't clickable and have no text/content-desc
        #[arg(long, default_value = "false")]
        include_non_clickable: bool,

        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// Hardware Jig automation tools (list serial ports, ping/test Jig connection)
    Jig {
        #[command(subcommand)]
        command: JigCommands,
    },

    /// Update lumi-tester CLI and/or VS Code extension to the latest release
    #[command(alias = "self-update", alias = "upgrade")]
    Update {
        /// GitHub repository hosting releases
        #[arg(long, default_value = "Nghi-NV/nl-tester", env = "LUMI_TESTER_REPO")]
        repo: String,

        /// Target version tag (e.g. v0.1.17; default: latest release)
        #[arg(short, long)]
        version: Option<String>,

        /// Force reinstall even if already on the latest version
        #[arg(short, long, default_value = "false")]
        force: bool,

        /// Only check for updates without installing
        #[arg(long, default_value = "false")]
        check: bool,

        /// Update the VS Code extension
        #[arg(long, default_value = "false")]
        extension: bool,

        /// Update both CLI and VS Code extension
        #[arg(long, default_value = "false")]
        all: bool,

        /// Print check status as machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// Install the latest Lumi Tester IDE extension
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },

    /// Check installed and latest version of Lumi Tester CLI and VS Code Extension
    Version {
        /// GitHub repository hosting releases
        #[arg(long, default_value = "Nghi-NV/nl-tester", env = "LUMI_TESTER_REPO")]
        repo: String,

        /// Print check status as machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// Open the Lumi Tester documentation website in your default browser
    Docs {
        /// Print the URL instead of opening a browser
        #[arg(long, default_value = "false")]
        print: bool,
    },

    /// Show which lumi-tester binary is running and where related config/skill
    /// directories live - useful when an IDE/MCP client seems to be using a
    /// different (stale) copy than the one you just built or updated
    #[command(alias = "where")]
    Which {
        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ExtensionCommands {
    /// Install the latest extension into selected IDEs, with an optional AI client integration
    Install {
        /// IDE target: vscode, antigravity, cursor, windsurf, vscodium, vscode-insiders, or all. If omitted, choose from detected IDEs.
        #[arg(long, value_enum)]
        ide: Option<ExtensionIde>,

        /// Also install the AI integration. If no --ai-client is given, the CLI shows a client picker.
        #[arg(long, default_value = "false")]
        ai: bool,

        /// AI client target (also enables AI installation); repeat or separate with commas: codex, claude, antigravity
        #[arg(long = "ai-client", value_enum, value_delimiter = ',')]
        ai_clients: Vec<ai::AiClient>,

        /// GitHub repository hosting extension releases
        #[arg(long, default_value = "Nghi-NV/nl-tester", env = "LUMI_TESTER_REPO")]
        repo: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExtensionIde {
    #[value(name = "all")]
    All,
    #[value(name = "vscode", alias = "vs-code")]
    Vscode,
    #[value(name = "antigravity")]
    Antigravity,
    #[value(name = "cursor")]
    Cursor,
    #[value(name = "windsurf")]
    Windsurf,
    #[value(name = "vscodium")]
    Vscodium,
    #[value(name = "vscode-insiders", alias = "insiders")]
    VscodeInsiders,
}

impl ExtensionIde {
    fn selector(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Vscode => "vscode",
            Self::Antigravity => "antigravity",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Vscodium => "vscodium",
            Self::VscodeInsiders => "vscode-insiders",
        }
    }
}

#[derive(Subcommand)]
enum JigCommands {
    /// List all available Serial / COM ports with hardware details
    #[command(alias = "list")]
    Ports {
        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },

    /// Ping and test connection to a hardware Jig controller (port, profile, or auto-detect)
    Ping {
        /// Serial port name (e.g. COM5, /dev/ttyUSB0) or Jig profile file path (e.g. profiles/jig_switch_sample.yaml)
        port: Option<String>,

        /// RS485 Node ID to address (e.g. 1, 2, 7; optional, default: broadcast or from profile)
        #[arg(short, long)]
        node: Option<u8>,

        /// Baudrate (default: 115200)
        #[arg(short, long, default_value = "115200")]
        baudrate: u32,

        /// Print machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CameraCommands {
    /// Open the camera profile editor using discovered defaults
    Profile {
        /// RTSP url of the camera pointed at the device
        #[arg(short, long)]
        rtsp: Option<String>,

        /// Profile file to create or edit (saved as JSON)
        #[arg(short, long)]
        profile: Option<PathBuf>,

        /// Web UI port
        #[arg(long, default_value = "9444")]
        port: u16,

        /// RTSP transport: tcp, udp, or auto
        #[arg(long)]
        transport: Option<String>,
    },

    /// Open read-only live camera observe UI using discovered defaults
    Observe {
        /// RTSP url of the camera pointed at the device
        #[arg(short, long)]
        rtsp: Option<String>,

        /// Profile file to observe
        #[arg(short, long)]
        profile: Option<PathBuf>,

        /// Web UI port
        #[arg(long, default_value = "9444")]
        port: u16,

        /// RTSP transport: tcp, udp, or auto
        #[arg(long)]
        transport: Option<String>,
    },

    /// Read current camera/device state using discovered defaults
    Check {
        /// RTSP url of the camera pointed at the device
        #[arg(short, long)]
        rtsp: Option<String>,

        /// Profile file to read
        #[arg(short, long)]
        profile: Option<PathBuf>,

        /// RTSP transport: tcp, udp, or auto
        #[arg(long)]
        transport: Option<String>,

        /// Continuously print state transitions until interrupted
        #[arg(long, default_value = "false")]
        watch: bool,
    },

    /// Validate and run the discovered camera YAML with reports enabled
    Test {
        /// Camera YAML test path
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Output directory for reports and artifacts
        #[arg(short, long, default_value = "./output/camera-test")]
        output: PathBuf,
    },

    /// Show discovered camera defaults and short commands
    Doctor,

    /// Open the web UI to pick device corners, choose a layout, and learn LED colors
    Calibrate {
        /// RTSP url of the camera pointed at the device
        #[arg(short, long)]
        rtsp: String,

        /// Profile file to create or edit (saved as JSON)
        #[arg(short, long, default_value = "camera-profile.json")]
        profile: PathBuf,

        /// Web UI port
        #[arg(long, default_value = "9444")]
        port: u16,

        /// RTSP transport: tcp, udp, or auto (default tcp)
        #[arg(long)]
        transport: Option<String>,

        /// Read-only live view of an existing profile (no editing)
        #[arg(long, default_value = "false")]
        observe: bool,
    },

    /// Capture a single frame to a file (to aim the camera)
    Snapshot {
        /// RTSP url
        #[arg(short, long)]
        rtsp: String,

        /// Output image path (.jpg/.png)
        #[arg(short, long, default_value = "snapshot.jpg")]
        output: PathBuf,

        /// RTSP transport: tcp, udp, or auto
        #[arg(long)]
        transport: Option<String>,
    },

    /// Read device button states from a saved profile
    Detect {
        /// Profile file (JSON) produced by `camera calibrate`
        #[arg(short, long)]
        profile: PathBuf,

        /// RTSP url (defaults to the one stored in the profile)
        #[arg(short, long)]
        rtsp: Option<String>,

        /// RTSP transport: tcp, udp, or auto
        #[arg(long)]
        transport: Option<String>,

        /// Continuously print state transitions until interrupted
        #[arg(long, default_value = "false")]
        watch: bool,
    },
}

#[derive(Subcommand)]
enum SystemCommands {
    /// Install required drivers and tools
    Install {
        /// Install all components
        #[arg(long)]
        all: bool,
    },

    /// Update lumi-tester CLI and/or VS Code extension to latest release
    #[command(alias = "self-update", alias = "upgrade")]
    Update {
        /// GitHub repository hosting releases
        #[arg(long, default_value = "Nghi-NV/nl-tester", env = "LUMI_TESTER_REPO")]
        repo: String,

        /// Target version tag (e.g. v0.1.17; default: latest release)
        #[arg(short, long)]
        version: Option<String>,

        /// Force reinstall even if already on the latest version
        #[arg(short, long, default_value = "false")]
        force: bool,

        /// Only check for updates without installing
        #[arg(long, default_value = "false")]
        check: bool,

        /// Update the VS Code extension
        #[arg(long, default_value = "false")]
        extension: bool,

        /// Update both CLI and VS Code extension
        #[arg(long, default_value = "false")]
        all: bool,

        /// Print check status as machine-readable JSON
        #[arg(long, default_value = "false")]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AiCommands {
    /// Install Lumi Tester skills and MCP integration for selected AI clients
    Install {
        /// GitHub repo that hosts release assets and skill files
        #[arg(long, default_value = "Nghi-NV/nl-tester", env = "LUMI_TESTER_REPO")]
        repo: String,

        /// Release tag to install assets from. Defaults to this CLI version.
        #[arg(long, env = "LUMI_TESTER_VERSION")]
        version: Option<String>,

        /// Git ref used for raw skill files when --version latest is used
        #[arg(long = "ref", default_value = "main", env = "LUMI_TESTER_REF")]
        git_ref: String,

        /// Directory for MCP package and generated config snippets
        #[arg(long, env = "LUMI_AI_HOME")]
        ai_home: Option<PathBuf>,

        /// Codex home directory
        #[arg(long, env = "CODEX_HOME")]
        codex_home: Option<PathBuf>,

        /// Write snippets only; do not modify CODEX_HOME/config.toml
        #[arg(long, default_value = "false")]
        no_configure_codex: bool,

        /// AI client target; repeat or separate with commas to select multiple: codex, claude, antigravity
        #[arg(long = "client", value_enum, value_delimiter = ',')]
        clients: Vec<ai::AiClient>,
    },
}

fn main() -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("lumi-main".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(16 * 1024 * 1024)
                .build()?
                .block_on(async_main())
        })?
        .join()
        .map_err(|panic| {
            if let Some(message) = panic.downcast_ref::<&str>() {
                anyhow::anyhow!("lumi-tester panicked: {}", message)
            } else if let Some(message) = panic.downcast_ref::<String>() {
                anyhow::anyhow!("lumi-tester panicked: {}", message)
            } else {
                anyhow::anyhow!("lumi-tester panicked")
            }
        })?
}

async fn async_main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            path,
            platform,
            device,
            parallel,
            output,
            continue_on_failure,
            record,
            snapshot,
            report,
            events_jsonl,
            tags,
            command_index,
            from_command_index,
            command_name,
            repeat,
            data,
            speed,
        } => {
            if let Some(ref s) = speed {
                std::env::set_var("LUMI_SPEED", s);
            }
            if path.is_file() {
                let parsed = lumi_tester::parser::yaml::parse_test_file(&path)?;
                let is_legacy_hc_flow = matches!(
                    parsed.platform,
                    Some(lumi_tester::parser::types::Platform::LumiE2e)
                );
                let normalized = lumi_tester::parser::normalize::normalize_flow(parsed, &path)?;
                let has_non_app_target = normalized
                    .components
                    .keys()
                    .any(|target| *target != lumi_tester::parser::types::TargetId::App);
                if has_non_app_target && !is_legacy_hc_flow {
                    lumi_tester::parser::validation::validate_flow_graph(&normalized)?;
                    let mock_mode = multi_target_mock_mode(&normalized);
                    let attached_driver = if !mock_mode {
                        if let Some(app) = normalized
                            .components
                            .get(&lumi_tester::parser::types::TargetId::App)
                        {
                            runner::create_platform_driver_for_target(
                                &app.platform,
                                &device,
                                speed.as_deref(),
                            )
                            .await
                            .ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let data_records = data
                        .as_deref()
                        .map(runner::load_data_records)
                        .transpose()?
                        .unwrap_or_default();
                    let multi_options = lumi_tester::adapter::MultiTargetRunOptions {
                        continue_on_failure,
                        snapshot,
                        report,
                        events_jsonl,
                        command_index,
                        from_command_index,
                        command_name: command_name.clone(),
                        repeat,
                        data_records,
                    };
                    let mut target_runner = if mock_mode {
                        lumi_tester::adapter::MultiTargetRunner::mock_harness(normalized)?
                            .with_options(multi_options.clone())
                    } else if let Some(driver) = attached_driver {
                            lumi_tester::adapter::MultiTargetRunner::standard_with_app_driver_and_options(
                                normalized,
                                driver,
                                &output,
                                multi_options.clone(),
                            )?
                    } else {
                        lumi_tester::adapter::MultiTargetRunner::standard(normalized)?
                            .with_options(multi_options.clone())
                    };
                    let target_report = target_runner.run()?;
                    lumi_tester::adapter::MultiTargetRunner::write_artifacts_with_options(
                        &target_report,
                        &output,
                        report,
                        events_jsonl,
                    )?;
                    println!(
                        "Multi-target status: {:?} ({} operations, evidence: {})",
                        target_report.status,
                        target_report.operations.len(),
                        if mock_mode { "mock_contract" } else { "real_preflight" }
                    );
                    match target_report.status {
                        lumi_tester::adapter::AdapterStatus::Passed if mock_mode => return Ok(()),
                        lumi_tester::adapter::AdapterStatus::Blocked => {
                            anyhow::bail!("multi-target runtime BLOCKED; see run.json")
                        }
                        lumi_tester::adapter::AdapterStatus::Passed => return Ok(()),
                        _ => anyhow::bail!("multi-target run did not pass; see run.json"),
                    }
                }
            }
            let platform_val = if let Some(p) = platform {
                normalize_platform(&p)
            } else {
                detect_platform(&path)?.unwrap_or_else(|| "android".to_string())
            };

            println!(
                "{} Running tests from: {}",
                "▶".green().bold(),
                path.display()
            );
            println!("  Platform: {}", platform_val.cyan());
            if !device.is_empty() {
                println!("  Devices: {}", device.join(", ").cyan());
            }
            if parallel {
                println!("  Parallel: {}", "Enabled".yellow());
            }
            if let Some(ref tags_list) = tags {
                println!("  Tags: {}", tags_list.join(", ").yellow());
            }
            println!("  Output: {}", output.display().to_string().cyan());
            if record {
                println!("  Recording: {}", "Enabled".green());
            }
            if snapshot {
                println!("  Snapshots: {}", "Enabled".green());
            }
            if report {
                println!("  Reports: {}", "Enabled".green());
            }
            if events_jsonl {
                println!("  Events JSONL: {}", "Enabled".green());
            }
            if let Some(idx) = command_index {
                println!("  Command Index: {}", idx.to_string().yellow());
            }
            if let Some(from_idx) = from_command_index {
                println!("  From Command Index: {} to end", from_idx.to_string().yellow());
            }
            if let Some(ref name) = command_name {
                println!("  Command Name: {}", name.cyan());
            }
            if repeat > 1 {
                println!("  Repeat: {}", format!("{}x", repeat).yellow());
            }
            if let Some(ref d) = data {
                println!("  Data File: {}", d.display().to_string().cyan());
            }

            // lumi_e2e uses the existing TestExecutor command loop. The runner's
            // platform driver is only a host lifecycle adapter for this semantic
            // domain; E2E I/O is owned by the profile-bound runtime in executor.
            let runner_platform = if platform_val == "lumi_e2e" {
                "macos"
            } else {
                platform_val.as_str()
            };
            runner::run_tests(
                &path,
                runner_platform,
                if device.is_empty() {
                    None
                } else {
                    Some(device)
                },
                &output,
                continue_on_failure,
                parallel,
                record,
                snapshot,
                report,
                events_jsonl,
                tags,
                command_index,
                from_command_index,
                command_name,
                repeat,
                data.as_deref(),
            )
            .await?;
        }

        Commands::Devices { platform } => {
            println!(
                "{} Listing {} devices...",
                "🔍".to_string().blue(),
                platform.cyan()
            );
            driver::list_devices(&normalize_platform(&platform)).await?;
        }

        Commands::Report {
            results,
            format,
            output,
        } => {
            println!(
                "{} Generating {} report from: {}",
                "📊".to_string().blue(),
                format.cyan(),
                results.display()
            );
            report::generate_report(&results, &format, output.as_deref()).await?;
        }

        Commands::Validate { path, json } => {
            let result = validate_test_files(&path);
            print_validation_result(&result, json)?;
            if !result.valid {
                anyhow::bail!("validation failed");
            }
        }

        Commands::List { path, json } => {
            let result = list_test_files(&path)?;
            print_list_result(&result, json)?;
            if !result.errors.is_empty() {
                anyhow::bail!("list failed with {} error(s)", result.errors.len());
            }
        }

        Commands::RequirementsCoverage { path, cases, json } => {
            let report = requirements::check_coverage(&path, cases.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "{} {} requirement(s): {} covered, {} skipped, {} gap(s)",
                    if report.ok { "✓".green() } else { "✗".red() },
                    report.total,
                    report.covered,
                    report.skipped,
                    report.gaps.len()
                );
                for gap in &report.gaps {
                    println!(
                        "  {} [{}] {} ({}) - {}",
                        "✗".red(),
                        gap.status,
                        gap.id,
                        gap.title,
                        gap.detail
                    );
                }
            }
            if !report.ok {
                anyhow::bail!("requirements coverage has unresolved gaps");
            }
        }

        Commands::Doctor { platform, json } => {
            let result = doctor_report(&normalize_platform(&platform));
            print_doctor_result(&result, json)?;
            if !result.ok {
                anyhow::bail!("doctor found missing dependencies");
            }
        }

        Commands::Schema { json: _ } => {
            println!("{}", include_str!("../schema/lumi-test.schema.json"));
        }

        Commands::Shell {
            platform,
            device,
            command,
            json,
        } => {
            let platform = normalize_platform(&platform);

            if command.is_empty() {
                println!(
                    "{} Starting interactive shell for {}...",
                    "🐚".to_string().blue(),
                    platform.cyan()
                );
            }

            let driver: Box<dyn driver::traits::PlatformDriver> = match platform.as_str() {
                "android" => {
                    Box::new(driver::android::AndroidDriver::new(device.as_deref()).await?)
                }
                "ios" => Box::new(driver::ios::IosDriver::new(device.as_deref()).await?),
                "macos" => Box::new(driver::macos::MacosDriver::new()),
                "windows" => Box::new(driver::windows::WindowsDriver::new()),
                _ => anyhow::bail!("Unknown platform: {}", platform),
            };

            if command.is_empty() {
                runner::shell::run_shell(driver).await?;
            } else {
                runner::shell::run_one_shot(driver, command, json).await?;
            }
        }

        Commands::System { command } => match command {
            SystemCommands::Install { all } => {
                utils::system::handle_system_command(utils::system::SystemCommand::Install { all })
                    .await?;
            }
            SystemCommands::Update {
                repo,
                version,
                force,
                check,
                extension,
                all,
                json,
            } => {
                lumi_tester::updater::run_update(lumi_tester::updater::UpdateOptions {
                    repo,
                    version,
                    force,
                    check_only: check,
                    update_extension: extension,
                    update_all: all,
                    json,
                })
                .await?;
            }
        },

        Commands::Update {
            repo,
            version,
            force,
            check,
            extension,
            all,
            json,
        } => {
            lumi_tester::updater::run_update(lumi_tester::updater::UpdateOptions {
                repo,
                version,
                force,
                check_only: check,
                update_extension: extension,
                update_all: all,
                json,
            })
            .await?;
        }

        Commands::Extension { command } => match command {
            ExtensionCommands::Install {
                ide,
                ai: install_ai,
                ai_clients,
                repo,
            } => {
                let ai_clients = if install_ai || !ai_clients.is_empty() {
                    ai::choose_clients(ai_clients, None)?
                } else {
                    Vec::new()
                };
                let ide_selector = ide.map(ExtensionIde::selector);
                lumi_tester::updater::install_latest_extension(
                    &repo,
                    ide_selector.as_deref(),
                )
                .await?;
                if !ai_clients.is_empty() {
                    ai::install(ai::AiInstallOptions {
                        repo,
                        version: Some("latest".to_string()),
                        git_ref: "main".to_string(),
                        ai_home: None,
                        codex_home: None,
                        clients: ai_clients,
                        configure_codex: true,
                    })
                    .await?;
                }
            }
        },

        Commands::Version { repo, json } => {
            lumi_tester::updater::run_update(lumi_tester::updater::UpdateOptions {
                repo,
                version: None,
                force: false,
                check_only: true,
                update_extension: false,
                update_all: false,
                json,
            })
            .await?;
        }

        Commands::Docs { print } => {
            let url = "https://nghi-nv.github.io/nl-tester/";
            if print {
                println!("{}", url);
            } else {
                println!("{} Opening {}", "📖".cyan(), url.cyan());
                if let Err(e) = lumi_tester::utils::system::open_url(url) {
                    eprintln!(
                        "{} Could not open a browser automatically: {}",
                        "⚠️".yellow(),
                        e
                    );
                    println!("Open this URL manually: {}", url.cyan());
                }
            }
        }

        Commands::Which { json } => {
            let exe = std::env::current_exe().ok();
            let path_binary = which::which("lumi-tester").ok();
            let path_mismatch = matches!((&exe, &path_binary), (Some(a), Some(b)) if a != b);
            let home = dirs::home_dir();
            let ai_home = home.as_ref().map(|h| h.join(".lumi-tester").join("ai"));
            let codex_skill = home
                .as_ref()
                .map(|h| h.join(".codex").join("skills").join("lumi-tester-agent"));
            let claude_skill = home
                .as_ref()
                .map(|h| h.join(".claude").join("skills").join("lumi-tester-agent"));
            let antigravity_global_skill = home.as_ref().map(|h| {
                h.join(".gemini")
                    .join("config")
                    .join("skills")
                    .join("lumi-tester-agent")
            });
            let antigravity_workspace_skill = std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join(".agents").join("skills").join("lumi-tester-agent"));

            if json {
                let info = serde_json::json!({
                    "runningBinary": exe.as_ref().map(|p| p.display().to_string()),
                    "version": env!("CARGO_PKG_VERSION"),
                    "pathResolvedBinary": path_binary.as_ref().map(|p| p.display().to_string()),
                    "pathMismatch": path_mismatch,
                    "aiHome": ai_home.as_ref().map(|p| p.display().to_string()),
                    "codexSkill": codex_skill.as_ref().map(|p| (p.display().to_string(), p.exists())),
                    "claudeSkill": claude_skill.as_ref().map(|p| (p.display().to_string(), p.exists())),
                    "antigravityGlobalSkill": antigravity_global_skill.as_ref().map(|p| (p.display().to_string(), p.exists())),
                    "antigravityWorkspaceSkill": antigravity_workspace_skill.as_ref().map(|p| (p.display().to_string(), p.exists())),
                });
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!("{} Lumi Tester {}", "•".blue(), env!("CARGO_PKG_VERSION").cyan());
                println!(
                    "  Running binary:   {}",
                    exe.as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(unknown)".to_string())
                        .cyan()
                );
                println!(
                    "  Resolved on PATH: {}",
                    path_binary
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(not found on PATH)".to_string())
                        .cyan()
                );
                if path_mismatch {
                    println!(
                        "  {} PATH resolves to a DIFFERENT binary than the one running here - \
                         an IDE/MCP client using plain `lumi-tester` may be using a stale copy.",
                        "⚠️".yellow()
                    );
                }
                if let Some(p) = &ai_home {
                    println!("  AI home:           {}", p.display().to_string().cyan());
                }
                for (label, dir) in [
                    ("Codex skill", &codex_skill),
                    ("Claude skill", &claude_skill),
                    ("Antigravity skill (global)", &antigravity_global_skill),
                    ("Antigravity skill (workspace)", &antigravity_workspace_skill),
                ] {
                    if let Some(p) = dir {
                        let marker = if p.exists() { "✓".green() } else { "✗".dimmed() };
                        println!("  {} {:<32} {}", marker, label, p.display().to_string().dimmed());
                    }
                }
            }
        }

        Commands::Ai { command } => match command {
            AiCommands::Install {
                repo,
                version,
                git_ref,
                ai_home,
                codex_home,
                no_configure_codex,
                clients,
            } => {
                let options = ai::AiInstallOptions {
                    repo,
                    version,
                    git_ref,
                    ai_home,
                    codex_home,
                    clients,
                    configure_codex: !no_configure_codex,
                };
                ai::install(options).await?;
            }
        },

        Commands::Record {
            output,
            device,
            app,
            name,
            include_waits,
            include_comments,
        } => {
            println!("{} Starting record mode...", "🔴".to_string().red().bold());

            // Create event recorder
            let event_recorder = recorder::EventRecorder::new(device.as_deref()).await?;

            // Start recording
            event_recorder.start_recording().await?;

            // Set up Ctrl+C handler with atomic flag
            let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop_flag_handler = stop_flag.clone();

            ctrlc::set_handler(move || {
                println!("\n\n{} Stopping recording...", "⏹️ ".yellow());
                stop_flag_handler.store(true, std::sync::atomic::Ordering::SeqCst);
            })?;

            // Start real-time touch capture using getevent
            println!("\n📲 Monitoring device interactions...");
            println!("   Tap, type, and swipe on your device.");
            println!("   Press Ctrl+C when done.\n");

            // Find touch device
            let getevent_info =
                lumi_tester::driver::android::adb::shell(device.as_deref(), "getevent -pl")
                    .await
                    .unwrap_or_default();

            let touch_device = find_touch_device(&getevent_info);

            if let Some(ref dev) = touch_device {
                println!("📲 Found touch device: {}", dev);

                // Get max touch coordinates for scaling
                let (max_x, max_y) = parse_touch_range(&getevent_info);
                println!("   Touch range: {}x{}", max_x, max_y);

                // Start getevent stream
                let adb_path = lumi_tester::utils::binary_resolver::find_adb()?;
                let mut args = Vec::new();
                if let Some(ref d) = device {
                    args.push("-s".to_string());
                    args.push(d.clone());
                }
                args.push("shell".to_string());
                args.push(format!("getevent -lt {}", dev));

                let mut child = tokio::process::Command::new(&adb_path)
                    .args(&args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;

                let stdout = child.stdout.take().unwrap();
                let reader = tokio::io::BufReader::new(stdout);

                use tokio::io::AsyncBufReadExt;
                let mut lines = reader.lines();

                let mut current_x: Option<i32> = None;
                let mut current_y: Option<i32> = None;
                let mut touch_down_time: Option<std::time::Instant> = None;

                loop {
                    tokio::select! {
                        line = lines.next_line() => {
                            if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                                break;
                            }
                            match line {
                                Ok(Some(line)) => {
                                    // Parse getevent output
                                    if line.contains("ABS_MT_POSITION_X") {
                                        if let Some(val) = parse_hex_value(&line) {
                                            current_x = Some((val as f64 / max_x as f64 * event_recorder.screen_width as f64) as i32);
                                        }
                                    } else if line.contains("ABS_MT_POSITION_Y") {
                                        if let Some(val) = parse_hex_value(&line) {
                                            current_y = Some((val as f64 / max_y as f64 * event_recorder.screen_height as f64) as i32);
                                        }
                                    } else if line.contains("BTN_TOUCH") && line.contains("DOWN") {
                                        touch_down_time = Some(std::time::Instant::now());
                                    } else if line.contains("BTN_TOUCH") && line.contains("UP") {
                                        if let (Some(x), Some(y)) = (current_x, current_y) {
                                            let duration = touch_down_time
                                                .map(|t| t.elapsed().as_millis())
                                                .unwrap_or(0);

                                            if duration > 500 {
                                                // Long press - for now just log
                                                println!("  👆 longPress at ({}, {})", x, y);
                                            } else {
                                                // Record tap
                                                if let Err(e) = event_recorder.record_tap(x, y).await {
                                                    eprintln!("  ⚠️ Failed to record tap: {}", e);
                                                }
                                            }
                                        }
                                        current_x = None;
                                        current_y = None;
                                        touch_down_time = None;
                                    }
                                }
                                Ok(None) => break,
                                Err(_) => break,
                            }
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                            if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                                break;
                            }
                        }
                    }
                }

                let _ = child.kill().await;
            } else {
                println!("⚠️ No touch device found. Running in passive mode.");
                // Fallback: just wait for Ctrl+C
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                }
            }

            // Generate YAML - this now runs after Ctrl+C
            let actions = event_recorder.stop_recording().await?;
            let detected_app = event_recorder.get_current_app().await;
            let app_id = app.as_deref().or(detected_app.as_deref());

            let config = recorder::yaml_generator::YamlGeneratorConfig {
                include_comments,
                include_waits,
                min_wait_ms: 1000,
                mask_sensitive: true,
                suggest_assertions: true,
            };

            let generator = recorder::YamlGenerator::with_config(config);
            generator.save_to_file(&actions, app_id, name.as_deref(), &output)?;

            println!("\n{} Recording complete!", "✅".green().bold());
            println!("   Output: {}", output.display().to_string().cyan());
        }

        Commands::Camera { command } => match command {
            CameraCommands::Profile {
                rtsp,
                profile,
                port,
                transport,
            } => {
                let discovered = lumi_tester::camera::launcher::CameraLauncherConfig::discover(
                    &std::env::current_dir()?,
                )?;
                let rtsp = rtsp.or(discovered.rtsp).with_context(|| {
                    "Missing CAMERA_RTSP. Add it to .env or pass --rtsp rtsp://..."
                })?;
                let profile = profile
                    .or(discovered.profile)
                    .unwrap_or_else(|| PathBuf::from("camera-profile.json"));
                lumi_tester::camera::run_calibrate(rtsp, profile, port, transport, false).await?;
            }
            CameraCommands::Observe {
                rtsp,
                profile,
                port,
                transport,
            } => {
                let discovered = lumi_tester::camera::launcher::CameraLauncherConfig::discover(
                    &std::env::current_dir()?,
                )?;
                let rtsp = match rtsp {
                    Some(rtsp) => rtsp,
                    None => discovered.require_rtsp("observe")?,
                };
                let profile = match profile {
                    Some(profile) => profile,
                    None => discovered.require_profile("observe")?,
                };
                lumi_tester::camera::run_calibrate(rtsp, profile, port, transport, true).await?;
            }
            CameraCommands::Check {
                rtsp,
                profile,
                transport,
                watch,
            } => {
                let discovered = lumi_tester::camera::launcher::CameraLauncherConfig::discover(
                    &std::env::current_dir()?,
                )?;
                let profile = match profile {
                    Some(profile) => profile,
                    None => discovered.require_profile("check")?,
                };
                let rtsp = rtsp.or(discovered.rtsp);
                lumi_tester::camera::run_detect(rtsp, &profile, transport, watch).await?;
            }
            CameraCommands::Test { path, output } => {
                let discovered = lumi_tester::camera::launcher::CameraLauncherConfig::discover(
                    &std::env::current_dir()?,
                )?;
                let path = match path {
                    Some(path) => path,
                    None => discovered.require_test_yaml()?,
                };
                println!(
                    "{} Camera test shortcut: validate + run with report/snapshot/events",
                    "▶".green().bold()
                );
                let validation = validate_test_files(&path);
                print_validation_result(&validation, false)?;
                if !validation.valid {
                    anyhow::bail!("validation failed");
                }
                runner::run_tests(
                    &path, "android", None, &output, false, false, false, true, true, true, None,
                    None, None, None, 1, None,
                )
                .await?;
            }
            CameraCommands::Doctor => {
                let discovered = lumi_tester::camera::launcher::CameraLauncherConfig::discover(
                    &std::env::current_dir()?,
                )?;
                println!("{}", discovered.render_doctor_summary());
            }
            CameraCommands::Calibrate {
                rtsp,
                profile,
                port,
                transport,
                observe,
            } => {
                lumi_tester::camera::run_calibrate(rtsp, profile, port, transport, observe).await?;
            }
            CameraCommands::Snapshot {
                rtsp,
                output,
                transport,
            } => {
                lumi_tester::camera::run_snapshot(&rtsp, &output, transport.as_deref())?;
            }
            CameraCommands::Detect {
                profile,
                rtsp,
                transport,
                watch,
            } => {
                lumi_tester::camera::run_detect(rtsp, &profile, transport, watch).await?;
            }
        },

        Commands::Inspect {
            platform,
            device,
            port,
            output,
        } => {
            use lumi_tester::inspector::{server::InspectorConfig, InspectorServer};

            let config = InspectorConfig {
                port,
                platform,
                device_serial: device,
                output_file: output,
                workspace_root: None,
            };

            let server = InspectorServer::new(config);
            server.start().await?;
        }

        Commands::SuggestSelectors {
            platform,
            device,
            app_id,
            query,
            point,
            file,
            width,
            height,
            limit,
            include_non_clickable,
            json,
        } => {
            use lumi_tester::inspector::{api, screen_capture};

            let (elements, width, height) = if let Some(path) = file {
                let raw = std::fs::read_to_string(&path)
                    .with_context(|| format!("Failed to read hierarchy dump: {}", path.display()))?;
                let elements = screen_capture::parse_hierarchy_for_platform(
                    &platform,
                    &raw,
                    app_id.as_deref().unwrap_or(""),
                    width,
                    height,
                );
                (elements, width, height)
            } else {
                let capture = screen_capture::ScreenCapture::new(&platform, device.as_deref())
                    .await
                    .context("Failed to connect to device - pass --file to use a saved hierarchy dump instead")?;
                let (dim_w, dim_h) = capture.dimensions();
                let raw = screen_capture::get_hierarchy_for_platform(&platform, device.as_deref(), app_id.as_deref())
                    .await
                    .context("Failed to dump UI hierarchy from device")?;
                let elements = screen_capture::parse_hierarchy_for_platform(
                    &platform,
                    &raw,
                    app_id.as_deref().unwrap_or(""),
                    dim_w,
                    dim_h,
                );
                (elements, dim_w, dim_h)
            };

            let point_arg = point.as_deref().and_then(api::parse_point_arg);
            let response = api::suggest_selectors_from_elements(
                &elements,
                width,
                height,
                query.as_deref(),
                point_arg,
                limit,
                include_non_clickable,
            );

            if json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else if response.suggestions.is_empty() {
                println!("\n{} No matching elements found.", "⚠️".yellow());
            } else {
                println!(
                    "\n{} {} element(s) matched ({} shown):\n",
                    "🔎".cyan(),
                    response.count,
                    response.suggestions.len()
                );
                for s in &response.suggestions {
                    let label = s
                        .text
                        .clone()
                        .or_else(|| s.content_desc.clone())
                        .unwrap_or_else(|| s.class.clone());
                    println!("  • {} {}", label.bold(), format!("[{}]", s.class).dimmed());
                    if let Some(dist) = s.distance_to_point {
                        println!("      distance to point: {}px", dist);
                    }
                    if let Some(best) = &s.best_selector {
                        println!("      best ({} pts): {}", best.score, best.value.green());
                        println!("      {}", best.yaml.replace('\n', "\n      "));
                    }
                    println!();
                }
            }
        }

        Commands::Jig { command } => match command {
            JigCommands::Ports { json } => {
                let ports = lumi_tester::hardware::list_serial_ports();
                if json {
                    println!("{}", serde_json::to_string_pretty(&ports)?);
                } else {
                    println!("\n{} Available Serial / COM Ports ({} found):", "🔌".green(), ports.len());
                    if ports.is_empty() {
                        println!("  {} No serial ports detected. Check USB connection.", "⚠️".yellow());
                    } else {
                        for p in &ports {
                            let mut desc = Vec::new();
                            if let Some(ref prod) = p.product {
                                desc.push(prod.as_str());
                            }
                            if let Some(ref mfg) = p.manufacturer {
                                desc.push(mfg.as_str());
                            }
                            let vid_pid = match (p.vid, p.pid) {
                                (Some(v), Some(d)) => format!(" (VID:{:04X} PID:{:04X})", v, d),
                                _ => String::new(),
                            };
                            let desc_str = if desc.is_empty() {
                                String::new()
                            } else {
                                format!(" - {}", desc.join(" / "))
                            };
                            println!("  • {:<12} [{}] {}{}", p.port_name.cyan().bold(), p.port_type, desc_str, vid_pid.dimmed());
                        }
                    }
                    println!();
                }
            }
            JigCommands::Ping { port, node, baudrate, json } => {
                let (target_port, target_node, target_format) = if let Some(p) = port {
                    if p.ends_with(".yaml") || p.ends_with(".yml") || p.ends_with(".json") || std::path::Path::new(&p).exists() {
                        if let Ok(content) = std::fs::read_to_string(&p) {
                            if let Ok(params) = serde_yaml::from_str::<lumi_tester::parser::types::HardwareConnectParams>(&content) {
                                (resolve_env_string(&params.port), node.or(params.node_id), params.wire_format)
                            } else {
                                (resolve_env_string(&p), node, None)
                            }
                        } else {
                            (resolve_env_string(&p), node, None)
                        }
                    } else {
                        (resolve_env_string(&p), node, None)
                    }
                } else {
                    let ports = lumi_tester::hardware::list_serial_ports();
                    if let Some(first) = ports.first() {
                        (first.port_name.clone(), node, None)
                    } else {
                        anyhow::bail!("No serial ports found. Connect a Jig or specify a port manually.");
                    }
                };

                if !json {
                    let node_info = if let Some(n) = target_node { format!(" (Node @{})", n) } else { String::new() };
                    println!("\n{} Testing connection to Jig on {}{} ({} baud)...", "🔌".cyan(), target_port, node_info, baudrate);
                }

                match lumi_tester::hardware::ping_details(&target_port, Some(baudrate), target_node, target_format) {
                    Ok(res) => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&res)?);
                        } else {
                            println!("  {} Connected successfully! (Latency: {}ms)", "✅".green(), res.latency_ms);
                            if let Some(nid) = res.node_id {
                                println!("  {} Node ID: {}", "📍".cyan(), nid);
                            }
                            if let Some(ref fw) = res.firmware_version {
                                println!("  {} Firmware: {}", "🏷️".cyan(), fw);
                            }
                            if let Some(ref st) = res.system_status {
                                println!("  {} Status: {}", "⚙️".cyan(), st);
                            }
                            println!("  {} Response: {}", "💬".dimmed(), res.raw_response.trim());
                            println!();
                        }
                    }
                    Err(e) => {
                        if json {
                            let err_obj = serde_json::json!({
                                "port": target_port,
                                "connected": false,
                                "error": e.to_string(),
                            });
                            println!("{}", serde_json::to_string_pretty(&err_obj)?);
                        } else {
                            println!("  {} Failed to connect to Jig on {}: {}", "❌".red(), target_port, e);
                            println!();
                        }
                        std::process::exit(1);
                    }
                }
            }
        },
    }

    Ok(())
}

fn normalize_platform(platform: &str) -> String {
    platform
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase()
}

fn resolve_env_string(val: &str) -> String {
    let trimmed = val.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') {
        let inner = &trimmed[2..trimmed.len() - 1];
        if let Some((var_name, default_val)) = inner.split_once(":-") {
            std::env::var(var_name).unwrap_or_else(|_| default_val.to_string())
        } else {
            std::env::var(inner).unwrap_or_else(|_| trimmed.to_string())
        }
    } else {
        trimmed.to_string()
    }
}

fn detect_platform(path: &std::path::Path) -> anyhow::Result<Option<String>> {
    if path.is_file() {
        return Ok(detect_platform_in_file(path));
    }

    if !path.is_dir() {
        return Ok(None);
    }

    let mut platforms = std::collections::BTreeSet::new();
    for file in collect_test_files(path)? {
        if let Some(platform) = detect_platform_in_file(&file) {
            platforms.insert(platform);
        }
    }

    match platforms.len() {
        0 => Ok(None),
        1 => Ok(platforms.into_iter().next()),
        _ => anyhow::bail!(
            "multiple platforms found under {}; pass --platform explicitly or run each platform directory separately: {}",
            path.display(),
            platforms.into_iter().collect::<Vec<_>>().join(", ")
        ),
    }
}

fn detect_platform_in_file(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines().take(20) {
        if let Some(rest) = line.trim().strip_prefix("platform:") {
            let p = normalize_platform(rest);
            if !p.is_empty() {
                return Some(p);
            }
        }
    }
    None
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationReport {
    valid: bool,
    files: Vec<ListedFlow>,
    errors: Vec<ValidationError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationError {
    path: String,
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListReport {
    files: Vec<ListedFlow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<ValidationError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListedFlow {
    path: String,
    app_id: Option<String>,
    url: Option<String>,
    platform: Option<lumi_tester::parser::types::Platform>,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    journey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required_capabilities: Option<Vec<lumi_tester::e2e::types::E2eCapability>>,
    components: std::collections::BTreeMap<
        lumi_tester::parser::types::TargetId,
        lumi_tester::parser::types::ComponentSpec,
    >,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<lumi_tester::parser::types::Diagnostic>,
    command_count: usize,
    commands: Vec<ListedCommand>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListedCommand {
    index: usize,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Vec<lumi_tester::e2e::types::E2eCapability>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<lumi_tester::parser::types::TargetId>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorReport {
    ok: bool,
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorCheck {
    name: String,
    ok: bool,
    path: Option<String>,
    error: Option<String>,
}

fn validate_test_files(path: &std::path::Path) -> ValidationReport {
    let mut files = Vec::new();
    let mut errors = Vec::new();

    match collect_test_files(path) {
        Ok(paths) => {
            for file in paths {
                match parse_listed_flow(&file) {
                    Ok(flow) => files.push(flow),
                    Err(error) => errors.push(make_validation_error(&file, error)),
                }
            }
        }
        Err(error) => errors.push(make_validation_error(path, error)),
    }

    ValidationReport {
        valid: errors.is_empty(),
        files,
        errors,
    }
}

fn list_test_files(path: &std::path::Path) -> anyhow::Result<ListReport> {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for file in collect_test_files(path)? {
        match parse_listed_flow(&file) {
            Ok(flow) => files.push(flow),
            Err(error) => errors.push(make_validation_error(&file, error)),
        }
    }

    Ok(ListReport { files, errors })
}

fn make_validation_error(path: &std::path::Path, error: anyhow::Error) -> ValidationError {
    let e2e = error.downcast_ref::<lumi_tester::e2e::E2eValidationError>();
    let flow = error.downcast_ref::<lumi_tester::parser::validation::FlowValidationError>();
    let normalization = error.downcast_ref::<lumi_tester::parser::normalize::NormalizationError>();
    ValidationError {
        path: path.display().to_string(),
        error: error.to_string(),
        code: e2e
            .map(|error| error.code().to_string())
            .or_else(|| flow.map(|error| error.code().to_string()))
            .or_else(|| normalization.map(|error| error.code().to_string())),
        field: e2e.and_then(|error| error.field().map(str::to_string)),
    }
}

fn collect_test_files(path: &std::path::Path) -> anyhow::Result<Vec<PathBuf>> {
    if !path.exists() {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let is_yaml = path
                .extension()
                .map_or(false, |ext| ext == "yaml" || ext == "yml");
            let name = e.file_name().to_string_lossy();
            is_yaml
                && name != "setup.yaml"
                && name != "setup.yml"
                && name != "teardown.yaml"
                && name != "teardown.yml"
        })
    {
        files.push(entry.path().to_path_buf());
    }

    files.sort();
    Ok(files)
}

fn parse_listed_flow(path: &std::path::Path) -> anyhow::Result<ListedFlow> {
    let flow = lumi_tester::parser::yaml::parse_test_file(path)?;
    let normalized = lumi_tester::parser::normalize::normalize_flow(flow.clone(), path)?;
    lumi_tester::parser::validation::validate_flow_graph(&normalized)?;
    // A multi-target parent is validated by the recursive target graph. The
    // HC profile validator applies only to a file that actually contains HC
    // commands; otherwise runFlow itself is a valid orchestration command.
    let e2e_metadata = if flow
        .commands
        .iter()
        .any(|command| matches!(command, lumi_tester::parser::types::TestCommand::E2e(_)))
    {
        lumi_tester::e2e::validate_flow(path, &flow)?
    } else {
        lumi_tester::e2e::E2eFlowMetadata {
            profile_ref: String::new(),
            profile_path: PathBuf::new(),
            required_capabilities: Vec::new(),
            command_requirements: Vec::new(),
        }
    };
    let commands = flow
        .commands
        .iter()
        .enumerate()
        .map(|(index, command)| ListedCommand {
            index,
            name: command.display_name(),
            capabilities: match command {
                lumi_tester::parser::types::TestCommand::E2e(e2e_command) => {
                    Some(e2e_command.required_capabilities())
                }
                _ => None,
            },
            target: match command {
                lumi_tester::parser::types::TestCommand::RunFlow(params) => {
                    params.clone().into_inner().target
                }
                _ => normalized.components.keys().next().copied()
                    .filter(|_| normalized.components.len() == 1),
            },
        })
        .collect::<Vec<_>>();

    Ok(ListedFlow {
        path: path.display().to_string(),
        app_id: flow.app_id,
        url: flow.url,
        platform: flow.platform,
        tags: flow.tags,
        journey: flow.journey,
        profile: flow.profile,
        required_capabilities: if e2e_metadata.required_capabilities.is_empty() {
            None
        } else {
            Some(e2e_metadata.required_capabilities)
        },
        components: normalized.components,
        diagnostics: normalized.legacy_metadata.diagnostics,
        command_count: commands.len(),
        commands,
    })
}

fn print_validation_result(report: &ValidationReport, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    if report.valid {
        println!(
            "{} Validated {} file(s)",
            "✓".green(),
            report.files.len().to_string().cyan()
        );
    } else {
        println!(
            "{} Validation failed with {} error(s)",
            "✗".red(),
            report.errors.len().to_string().red()
        );
        for error in &report.errors {
            println!("  {}: {}", error.path.cyan(), error.error);
        }
    }

    Ok(())
}

fn print_list_result(report: &ListReport, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    for file in &report.files {
        println!(
            "{} {} ({} command(s))",
            "•".blue(),
            file.path.cyan(),
            file.command_count
        );
        for command in &file.commands {
            println!("  [{}] {}", command.index, command.name);
        }
    }

    if !report.errors.is_empty() {
        println!(
            "{} error(s):",
            report.errors.len().to_string().red()
        );
        for error in &report.errors {
            println!("  {}: {}", error.path.cyan(), error.error);
        }
    }

    Ok(())
}

fn multi_target_mock_mode(flow: &lumi_tester::parser::types::NormalizedFlow) -> bool {
    flow.components
        .iter()
        .filter(|(target, _)| **target != lumi_tester::parser::types::TargetId::App)
        .all(|(_, component)| {
            let Some(profile) = component.profile.as_deref() else {
                return false;
            };
            let Ok(content) = std::fs::read_to_string(profile) else {
                return false;
            };
            let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&content) else {
                return false;
            };
            value.get("mock").and_then(serde_yaml::Value::as_bool) == Some(true)
                || value.get("mode").and_then(serde_yaml::Value::as_str) == Some("mock")
        })
}

fn doctor_report(platform: &str) -> DoctorReport {
    let mut checks = Vec::new();

    match platform {
        "android" | "android_auto" => {
            checks.push(binary_check(
                "adb",
                lumi_tester::utils::binary_resolver::find_adb(),
            ));
            checks.push(binary_check(
                "ffmpeg",
                lumi_tester::utils::binary_resolver::find_ffmpeg(),
            ));
        }
        "ios" => {
            checks.push(binary_check(
                "idb",
                lumi_tester::utils::binary_resolver::find_idb(),
            ));
            checks.push(binary_check(
                "ffmpeg",
                lumi_tester::utils::binary_resolver::find_ffmpeg(),
            ));
        }
        "web" => {
            checks.push(binary_check(
                "ffmpeg",
                lumi_tester::utils::binary_resolver::find_ffmpeg(),
            ));
        }
        "macos" => {
            checks.push(command_check("open"));
            checks.push(command_check("osascript"));
            checks.push(command_check("screencapture"));
        }
        "windows" => {
            if cfg!(target_os = "windows") {
                checks.push(command_check("powershell"));
                checks.push(windows_powershell_check(
                    "uia-automation",
                    "Add-Type -AssemblyName UIAutomationClient; Add-Type -AssemblyName UIAutomationTypes",
                ));
            } else {
                checks.push(unsupported_host_check(
                    "windows",
                    "Windows desktop automation is only supported on Windows hosts",
                ));
            }
        }
        "all" => {
            checks.push(binary_check(
                "adb",
                lumi_tester::utils::binary_resolver::find_adb(),
            ));
            checks.push(binary_check(
                "idb",
                lumi_tester::utils::binary_resolver::find_idb(),
            ));
            checks.push(binary_check(
                "ffmpeg",
                lumi_tester::utils::binary_resolver::find_ffmpeg(),
            ));
            if cfg!(target_os = "macos") {
                checks.push(command_check("open"));
                checks.push(command_check("osascript"));
                checks.push(command_check("screencapture"));
            }
            if cfg!(target_os = "windows") {
                checks.push(command_check("powershell"));
                checks.push(windows_powershell_check(
                    "uia-automation",
                    "Add-Type -AssemblyName UIAutomationClient; Add-Type -AssemblyName UIAutomationTypes",
                ));
            }
        }
        other => checks.push(DoctorCheck {
            name: "platform".to_string(),
            ok: false,
            path: None,
            error: Some(format!("Unknown platform: {}", other)),
        }),
    }

    DoctorReport {
        ok: checks.iter().all(|check| check.ok),
        checks,
    }
}

fn binary_check(name: &str, result: anyhow::Result<std::path::PathBuf>) -> DoctorCheck {
    match result {
        Ok(path) => DoctorCheck {
            name: name.to_string(),
            ok: true,
            path: Some(path.display().to_string()),
            error: None,
        },
        Err(error) => DoctorCheck {
            name: name.to_string(),
            ok: false,
            path: None,
            error: Some(error.to_string()),
        },
    }
}

fn command_check(name: &str) -> DoctorCheck {
    binary_check(name, which::which(name).map_err(anyhow::Error::from))
}

fn windows_powershell_check(name: &str, script: &str) -> DoctorCheck {
    if !cfg!(target_os = "windows") {
        return unsupported_host_check(
            name,
            "Windows PowerShell checks are only supported on Windows hosts",
        );
    }

    match std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
    {
        Ok(output) if output.status.success() => DoctorCheck {
            name: name.to_string(),
            ok: true,
            path: Some("powershell".to_string()),
            error: None,
        },
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            DoctorCheck {
                name: name.to_string(),
                ok: false,
                path: None,
                error: Some(if stderr.is_empty() {
                    format!("PowerShell check failed with status {}", output.status)
                } else {
                    stderr
                }),
            }
        }
        Err(error) => DoctorCheck {
            name: name.to_string(),
            ok: false,
            path: None,
            error: Some(error.to_string()),
        },
    }
}

fn unsupported_host_check(name: &str, message: &str) -> DoctorCheck {
    DoctorCheck {
        name: name.to_string(),
        ok: false,
        path: None,
        error: Some(message.to_string()),
    }
}

fn print_doctor_result(report: &DoctorReport, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    for check in &report.checks {
        if check.ok {
            println!(
                "{} {}: {}",
                "✓".green(),
                check.name.cyan(),
                check.path.as_deref().unwrap_or("").green()
            );
        } else {
            println!(
                "{} {}: {}",
                "✗".red(),
                check.name.cyan(),
                check.error.as_deref().unwrap_or("missing").red()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn camera_shortcuts_are_available_in_help() {
        use clap::CommandFactory;

        let mut help = Vec::new();
        Cli::command()
            .find_subcommand_mut("camera")
            .unwrap()
            .write_long_help(&mut help)
            .unwrap();
        let help = String::from_utf8(help).unwrap();

        assert!(help.contains("profile"));
        assert!(help.contains("observe"));
        assert!(help.contains("check"));
        assert!(help.contains("test"));
        assert!(help.contains("doctor"));
    }

    #[test]
    fn camera_doctor_shortcut_parses_without_required_flags() {
        let cli = Cli::try_parse_from(["lumi-tester", "camera", "doctor"]).unwrap();

        match cli.command {
            Commands::Camera {
                command: CameraCommands::Doctor,
            } => {}
            _ => panic!("expected camera doctor command"),
        }
    }

    #[test]
    fn camera_profile_shortcut_accepts_optional_overrides() {
        let cli = Cli::try_parse_from([
            "lumi-tester",
            "camera",
            "profile",
            "--rtsp",
            "rtsp://10.0.0.5/live",
            "--profile",
            "profiles/lab_switch4_camera.json",
        ])
        .unwrap();

        match cli.command {
            Commands::Camera {
                command:
                    CameraCommands::Profile {
                        rtsp,
                        profile,
                        port,
                        ..
                    },
            } => {
                assert_eq!(rtsp.as_deref(), Some("rtsp://10.0.0.5/live"));
                assert_eq!(
                    profile.unwrap(),
                    PathBuf::from("profiles/lab_switch4_camera.json")
                );
                assert_eq!(port, 9444);
            }
            _ => panic!("expected camera profile command"),
        }
    }

    #[test]
    fn detect_platform_reads_directory_when_all_flows_match() {
        let dir = std::env::temp_dir().join(format!("lumi-platform-detect-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.yaml"),
            "platform: macos\nappId: /Applications/Calculator.app\n---\n- launchApp\n",
        )
        .unwrap();
        fs::write(
            dir.join("b.yaml"),
            "platform: macOS\nappId: /Applications/TextEdit.app\n---\n- launchApp\n",
        )
        .unwrap();

        let platform = detect_platform(&dir).unwrap();

        assert_eq!(platform.as_deref(), Some("macos"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_platform_rejects_mixed_platform_directory() {
        let dir = std::env::temp_dir().join(format!("lumi-platform-mixed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("mac.yaml"),
            "platform: macos\nappId: /Applications/Calculator.app\n---\n- launchApp\n",
        )
        .unwrap();
        fs::write(
            dir.join("web.yaml"),
            "platform: web\nurl: https://example.com\n---\n- launchApp\n",
        )
        .unwrap();

        let error = detect_platform(&dir).unwrap_err().to_string();

        assert!(error.contains("multiple platforms found"));
        assert!(error.contains("macos"));
        assert!(error.contains("web"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_test_files_includes_nested_subflows() {
        let dir =
            std::env::temp_dir().join(format!("lumi-collect-subflows-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("subflows")).unwrap();
        fs::write(
            dir.join("main.yaml"),
            "appId: com.example\n---\n- launchApp\n",
        )
        .unwrap();
        fs::write(
            dir.join("subflows").join("login.yaml"),
            "appId: com.example\n---\n- tap: Login\n",
        )
        .unwrap();

        let files = collect_test_files(&dir).unwrap();
        let relative = files
            .iter()
            .map(|p| {
                p.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect::<Vec<_>>();

        assert_eq!(relative, vec!["main.yaml", "subflows/login.yaml"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn doctor_windows_fails_fast_on_non_windows_hosts() {
        let report = doctor_report("windows");

        assert!(!report.ok);
        assert!(report.checks[0]
            .error
            .as_deref()
            .unwrap()
            .contains("only supported on Windows hosts"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn doctor_all_does_not_fail_on_windows_unsupported_host_check() {
        let report = doctor_report("all");

        assert!(report.checks.iter().all(|check| check.name != "windows"));
    }
}

/// Find the primary touch input device from getevent -pl output
fn find_touch_device(getevent_output: &str) -> Option<String> {
    let mut current_device: Option<String> = None;

    for line in getevent_output.lines() {
        if line.starts_with("add device") || line.contains("/dev/input/event") {
            // Extract device path
            if let Some(path_start) = line.find("/dev/input/") {
                let rest = &line[path_start..];
                let device_path = rest.split_whitespace().next().unwrap_or(rest);
                // Clean up device path - remove trailing colon or other chars
                let clean_path = device_path.trim_end_matches(':').trim();
                current_device = Some(clean_path.to_string());
            }
        } else if line.contains("ABS_MT_POSITION_X") || line.contains("ABS_MT_TOUCH") {
            // This device supports multi-touch, it's likely the main touchscreen
            if let Some(ref device) = current_device {
                return Some(device.clone());
            }
        }
    }

    None
}

/// Parse touch coordinate range from getevent -pl output
fn parse_touch_range(getevent_output: &str) -> (f64, f64) {
    let mut max_x: f64 = 32767.0; // Default
    let mut max_y: f64 = 32767.0;

    for line in getevent_output.lines() {
        // Look for lines like: ABS_MT_POSITION_X : value 0, min 0, max 1080
        if line.contains("ABS_MT_POSITION_X") && line.contains("max") {
            if let Some(max_val) = extract_max_value(line) {
                max_x = max_val as f64;
            }
        } else if line.contains("ABS_MT_POSITION_Y") && line.contains("max") {
            if let Some(max_val) = extract_max_value(line) {
                max_y = max_val as f64;
            }
        }
    }

    (max_x, max_y)
}

/// Extract max value from getevent line
fn extract_max_value(line: &str) -> Option<i32> {
    // Format: "ABS_MT_POSITION_X : value 0, min 0, max 1080, fuzz 0, flat 0, resolution 0"
    if let Some(max_pos) = line.find("max ") {
        let rest = &line[max_pos + 4..];
        let val_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        return val_str.parse().ok();
    }
    None
}

/// Parse hex or decimal value from getevent line
fn parse_hex_value(line: &str) -> Option<i32> {
    // Format: [timestamp] /dev/input/eventX: EV_ABS ABS_MT_POSITION_X 0000abcd
    let parts: Vec<&str> = line.split_whitespace().collect();
    if let Some(last) = parts.last() {
        // Value is usually in hex without 0x prefix
        if last.chars().all(|c| c.is_ascii_hexdigit()) {
            return i32::from_str_radix(last, 16).ok();
        }
        // Try decimal
        return last.parse().ok();
    }
    None
}
