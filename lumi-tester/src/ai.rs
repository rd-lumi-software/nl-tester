use anyhow::{Context, Result};
use clap::ValueEnum;
use colored::Colorize;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use uuid::Uuid;

const SKILL_FILES: &[&str] = &[
    "SKILL.md",
    "references/android-auto.md",
    "references/cli.csv",
    "references/command-catalog.md",
    "references/commands.csv",
    "references/debug-artifacts.md",
    "references/desktop.md",
    "references/hardware.md",
    "references/hc-execution.md",
    "references/headers.csv",
    "references/index.md",
    "references/patterns.md",
    "references/requirements-example/cases.csv",
    "references/requirements-example/requirements/auth.yaml",
    "references/requirements-example/requirements/common.yaml",
    "references/requirements-example/requirements/index.yaml",
    "references/requirements-example/smoke/001_login_otp.yaml",
    "references/requirements-example/smoke/002_login_otp_negative.yaml",
    "references/selector-discovery.md",
    "references/selectors.csv",
    "references/testcase-design.md",
    "scripts/lumi_agent.py",
    "agents/openai.yaml",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum AiClient {
    #[value(name = "codex")]
    Codex,
    #[value(name = "claude")]
    Claude,
    #[value(name = "antigravity")]
    Antigravity,
}

impl AiClient {
    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Antigravity => "Antigravity",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SkillBundle {
    Codex,
    Antigravity,
}

impl SkillBundle {
    fn source_path(self) -> &'static str {
        match self {
            Self::Codex => "lumi-tester/ai/codex-skill/lumi-tester-agent",
            Self::Antigravity => "lumi-tester/ai/antigravity-skill/lumi-tester-agent",
        }
    }
}

pub struct AiInstallOptions {
    pub repo: String,
    pub version: Option<String>,
    pub git_ref: String,
    pub ai_home: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub clients: Vec<AiClient>,
    pub configure_codex: bool,
}

pub async fn install(options: AiInstallOptions) -> Result<()> {
    let home = dirs::home_dir().context("Could not resolve home directory")?;
    let version = normalize_version(
        options
            .version
            .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION"))),
    );
    let ai_home = options
        .ai_home
        .unwrap_or_else(|| home.join(".lumi-tester").join("ai"));
    let codex_home = options.codex_home.unwrap_or_else(|| home.join(".codex"));
    let claude_home = home.join(".claude");
    // Antigravity discovers skills at two levels (see docs/ai-skills-integration.md):
    // global (`~/.gemini/config/skills`, all workspaces) and workspace-local
    // (`<cwd>/.agents/skills`, this project only).
    let antigravity_global_home = home.join(".gemini").join("config");
    let antigravity_workspace_dir = std::env::current_dir()
        .unwrap_or_default()
        .join(".agents");
    let target = detect_target()?;
    let clients = choose_clients(options.clients, Some(&codex_home))?;
    if clients.is_empty() {
        anyhow::bail!("Choose at least one AI client to install");
    }
    let needs_mcp = clients
        .iter()
        .any(|client| matches!(client, AiClient::Codex | AiClient::Claude));

    println!(
        "{}",
        "Installing Lumi Tester AI integration...".green().bold()
    );
    println!("  Repo: {}", options.repo.cyan());
    println!("  Version: {}", version.cyan());
    println!("  Target: {}", target.cyan());
    println!(
        "  AI clients: {}",
        clients
            .iter()
            .map(|client| client.label())
            .collect::<Vec<_>>()
            .join(", ")
            .cyan()
    );
    println!("  AI home: {}", ai_home.display().to_string().cyan());

    if needs_mcp {
        install_mcp(&options.repo, &version, &target, &ai_home).await?;
    }

    if clients.contains(&AiClient::Codex) {
        install_skill_bundle(
            &options.repo,
            &version,
            &options.git_ref,
            &codex_home.join("skills").join("lumi-tester-agent"),
            SkillBundle::Codex,
            "Codex",
        )
        .await?;
    }
    if clients.contains(&AiClient::Claude) {
        install_skill_bundle(
            &options.repo,
            &version,
            &options.git_ref,
            &claude_home.join("skills").join("lumi-tester-agent"),
            SkillBundle::Codex,
            "Claude Code",
        )
        .await?;
    }
    if clients.contains(&AiClient::Antigravity) {
        install_skill_bundle(
            &options.repo,
            &version,
            &options.git_ref,
            &antigravity_global_home
                .join("skills")
                .join("lumi-tester-agent"),
            SkillBundle::Antigravity,
            "Antigravity (global)",
        )
        .await?;
        install_skill_bundle(
            &options.repo,
            &version,
            &options.git_ref,
            &antigravity_workspace_dir
                .join("skills")
                .join("lumi-tester-agent"),
            SkillBundle::Antigravity,
            "Antigravity (workspace)",
        )
        .await?;
    }

    let snippets = if needs_mcp {
        Some(write_config_snippets(&ai_home, &clients).await?)
    } else {
        None
    };
    if clients.contains(&AiClient::Codex) && options.configure_codex {
        if let Some(codex_snippet) = snippets
            .as_ref()
            .and_then(|snippets| snippets.codex.as_ref())
        {
            configure_codex(&codex_home, codex_snippet).await?;
        }
    } else if clients.contains(&AiClient::Codex) {
        println!("{} Skipped Codex config update", "•".blue());
    }

    println!();
    println!("{}", "Lumi Tester AI integration installed.".green().bold());
    println!(
        "Restart {} so it reloads the installed skill{}.",
        clients
            .iter()
            .map(|client| client.label())
            .collect::<Vec<_>>()
            .join(", "),
        if needs_mcp { " and MCP server" } else { "" }
    );
    println!("Quick checks:");
    println!("  lumi-tester doctor --platform android --json");
    println!("  lumi-tester doctor --platform android_auto --json");
    println!("  lumi-tester doctor --platform ios --json  # macOS + idb");
    println!("  lumi-tester doctor --platform web --json");
    println!("  lumi-tester doctor --platform macos --json");
    println!("  lumi-tester doctor --platform windows --json");
    if clients.contains(&AiClient::Codex) {
        println!("  python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-schema");
        println!("  python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-check path/to/test.yaml --summary-json ./output/agent-check.json");
    }
    if let Some(server) = snippets.as_ref().map(|snippets| &snippets.server) {
        println!("  node \"{}\"", server.display());
    }

    Ok(())
}

fn deduplicate_clients(clients: Vec<AiClient>) -> Vec<AiClient> {
    let mut selected = Vec::new();
    for client in clients {
        if !selected.contains(&client) {
            selected.push(client);
        }
    }
    selected
}

pub fn choose_clients(
    requested: Vec<AiClient>,
    codex_home: Option<&Path>,
) -> Result<Vec<AiClient>> {
    if !requested.is_empty() {
        return Ok(deduplicate_clients(requested));
    }
    let home = dirs::home_dir().context("Could not resolve home directory")?;
    let default_codex_home = home.join(".codex");
    select_clients(&home, codex_home.unwrap_or(&default_codex_home))
}

fn select_clients(home: &Path, codex_home: &Path) -> Result<Vec<AiClient>> {
    let candidates = [AiClient::Codex, AiClient::Claude, AiClient::Antigravity];
    let antigravity_app_detected = [
        home.join("Applications/Antigravity.app"),
        PathBuf::from("/Applications/Antigravity.app"),
    ]
    .iter()
    .any(|path| path.exists());
    let detected = candidates
        .iter()
        .copied()
        .filter(|client| match client {
            AiClient::Codex => codex_home.is_dir() || which::which("codex").is_ok(),
            AiClient::Claude => home.join(".claude").is_dir() || which::which("claude").is_ok(),
            AiClient::Antigravity => {
                home.join(".gemini/config/skills").is_dir()
                    || antigravity_app_detected
                    || which::which("antigravity").is_ok()
            }
        })
        .collect::<Vec<_>>();

    if !io::stdin().is_terminal() {
        return match detected.as_slice() {
            [client] => Ok(vec![*client]),
            [] => anyhow::bail!(
                "No AI client was detected. Run this command in a terminal to choose one, or pass --client codex, --client claude, or --client antigravity."
            ),
            _ => anyhow::bail!(
                "Several AI clients were detected. Choose one or more with --client codex, --client claude, or --client antigravity."
            ),
        };
    }

    let detected_labels = if detected.is_empty() {
        "none".to_string()
    } else {
        detected
            .iter()
            .map(|client| client.label())
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("\nDetected AI clients: {}", detected_labels.cyan());
    println!("Choose which AI client(s) should receive Lumi Tester:");
    for (index, client) in candidates.iter().enumerate() {
        let status = if detected.contains(client) {
            "detected"
        } else {
            "not detected; setup path will be created"
        };
        println!("  {}) {} ({})", index + 1, client.label(), status);
    }
    if detected.len() == 1 {
        println!(
            "Press Enter to choose the detected client, or enter numbers separated by commas."
        );
    } else {
        println!(
            "Enter numbers separated by commas (for example: 1 or 1,3). Enter 'all' for all three."
        );
    }
    print!("Selection: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        if detected.len() == 1 {
            return Ok(detected);
        }
        anyhow::bail!("No AI clients selected; rerun and choose one or more listed clients");
    }
    if input.eq_ignore_ascii_case("all") {
        return Ok(candidates.to_vec());
    }

    let mut selected = Vec::new();
    for raw_index in input.split(',') {
        let index = raw_index.trim().parse::<usize>().with_context(|| {
            format!(
                "Invalid AI client selection '{}'; enter numbers from 1 to 3",
                raw_index.trim()
            )
        })?;
        let client = candidates
            .get(index.checked_sub(1).unwrap_or(usize::MAX))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "AI client selection {} is out of range; enter numbers from 1 to 3",
                    index
                )
            })?;
        if !selected.contains(client) {
            selected.push(*client);
        }
    }
    Ok(selected)
}

fn normalize_version(version: String) -> String {
    if version == "latest" || version.starts_with('v') {
        version
    } else if version
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("v{}", version)
    } else {
        version
    }
}

fn detect_target() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let target = match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        _ => anyhow::bail!("Unsupported platform: {} {}", os, arch),
    };
    Ok(target.to_string())
}

async fn install_mcp(repo: &str, version: &str, target: &str, ai_home: &Path) -> Result<()> {
    let node = which::which("node").context("Missing required command: node")?;
    let npm = which::which("npm").context("Missing required command: npm")?;
    let package_dir = ai_home.join("mcp");
    let server_path = package_dir
        .join("node_modules")
        .join("lumi-tester-mcp")
        .join("src")
        .join("server.js");
    let asset = format!("lumi-tester-mcp-{}.tgz", target);
    let url = format!("{}/{}", release_base_url(repo, version), asset);
    let tmp_dir = std::env::temp_dir().join(format!("lumi-tester-ai-{}", Uuid::new_v4()));
    let tgz = tmp_dir.join(&asset);

    println!("{} Installing MCP package", "•".blue());
    println!("  Asset: {}", asset.cyan());
    tokio::fs::create_dir_all(&tmp_dir).await?;
    tokio::fs::create_dir_all(&package_dir).await?;
    download_to_file(&url, &tgz).await?;

    let status = Command::new(&npm)
        .arg("install")
        .arg("--prefix")
        .arg(&package_dir)
        .arg(&tgz)
        .arg("--omit=dev")
        .arg("--no-audit")
        .arg("--no-fund")
        .status()
        .await
        .context("Failed to run npm install for Lumi Tester MCP package")?;
    if !status.success() {
        anyhow::bail!("npm install failed with status {}", status);
    }

    if !server_path.is_file() {
        anyhow::bail!("MCP server was not installed at {}", server_path.display());
    }

    let node_status = Command::new(&node)
        .arg("--check")
        .arg(&server_path)
        .status()
        .await
        .context("Failed to validate MCP server with node --check")?;
    if !node_status.success() {
        anyhow::bail!("MCP server validation failed with status {}", node_status);
    }

    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    println!("  Installed MCP server: {}", server_path.display());
    Ok(())
}

/// Downloads the shared `SKILL_FILES` bundle into `skill_dir`. Used for both
/// `~/.codex/skills/lumi-tester-agent` and `~/.claude/skills/lumi-tester-agent`
/// - Codex and Claude Code both discover skills this way, and the bundle
/// content (SKILL.md + references/scripts) is identical between them.
async fn install_skill_bundle(
    repo: &str,
    version: &str,
    git_ref: &str,
    skill_dir: &Path,
    bundle: SkillBundle,
    label: &str,
) -> Result<()> {
    println!("{} Installing {} skill", "•".blue(), label);
    let base = resolve_skill_base_url(repo, version, git_ref, bundle).await?;
    tokio::fs::create_dir_all(skill_dir.join("references")).await?;
    tokio::fs::create_dir_all(skill_dir.join("scripts")).await?;
    tokio::fs::create_dir_all(skill_dir.join("agents")).await?;

    for file in SKILL_FILES {
        let output = skill_dir.join(file);
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        download_to_file(&format!("{}/{}", base, file), &output).await?;
    }

    make_executable(&skill_dir.join("scripts").join("lumi_agent.py"))?;
    println!("  Installed {} skill: {}", label, skill_dir.display());
    Ok(())
}

async fn resolve_skill_base_url(
    repo: &str,
    version: &str,
    git_ref: &str,
    bundle: SkillBundle,
) -> Result<String> {
    let primary = format!(
        "{}/{}",
        raw_base_url(repo, version, git_ref),
        bundle.source_path()
    );
    if version == "latest" {
        return Ok(primary);
    }

    for file in SKILL_FILES {
        let url = format!("{}/{}", primary, file);
        if !url_exists(&url).await? {
            let fallback = format!(
                "{}/{}",
                raw_ref_base_url(repo, git_ref),
                bundle.source_path()
            );
            eprintln!(
                "{} Skill file {} is not available at {}; falling back to {}",
                "warning:".yellow(),
                file,
                version,
                git_ref
            );
            return Ok(fallback);
        }
    }

    Ok(primary)
}

struct ConfigSnippets {
    codex: Option<PathBuf>,
    server: PathBuf,
}

async fn write_config_snippets(ai_home: &Path, clients: &[AiClient]) -> Result<ConfigSnippets> {
    let lumi_bin = which::which("lumi-tester")
        .or_else(|_| std::env::current_exe())
        .context("Could not resolve lumi-tester binary path")?;
    tokio::fs::create_dir_all(ai_home).await?;
    let server_path = ai_home
        .join("mcp")
        .join("node_modules")
        .join("lumi-tester-mcp")
        .join("src")
        .join("server.js");
    let codex_path = if clients.contains(&AiClient::Codex) {
        let codex_snippet = ai_home.join("lumi-tester-mcp.codex.toml");
        let codex = format!(
            "[mcp_servers.lumi-tester]\ncommand = \"node\"\nargs = [\"{}\"]\nenv = {{ LUMI_TESTER_BIN = \"{}\" }}\nstartup_timeout_sec = 10\ntool_timeout_sec = 300\n",
            toml_escape(&server_path.display().to_string()),
            toml_escape(&lumi_bin.display().to_string())
        );
        tokio::fs::write(&codex_snippet, codex).await?;
        println!("  Codex: {}", codex_snippet.display());
        Some(codex_snippet)
    } else {
        None
    };

    if clients.contains(&AiClient::Claude) {
        let claude_snippet = ai_home.join("lumi-tester-mcp.claude.json");
        let claude = serde_json::json!({
            "mcpServers": {
                "lumi-tester": {
                    "command": "node",
                    "args": [server_path.display().to_string()],
                    "env": {
                        "LUMI_TESTER_BIN": lumi_bin.display().to_string()
                    }
                }
            }
        });
        tokio::fs::write(
            &claude_snippet,
            serde_json::to_string_pretty(&claude)? + "\n",
        )
        .await?;
        println!("  Claude: {}", claude_snippet.display());
    }

    println!("{} Wrote MCP config snippets", "•".blue());

    Ok(ConfigSnippets {
        codex: codex_path,
        server: server_path,
    })
}

async fn configure_codex(codex_home: &Path, snippet_path: &Path) -> Result<()> {
    let config = codex_home.join("config.toml");
    tokio::fs::create_dir_all(codex_home).await?;

    if config.is_file() {
        let current = tokio::fs::read_to_string(&config).await?;
        if current
            .lines()
            .any(|line| line.trim() == "[mcp_servers.lumi-tester]")
        {
            println!(
                "{} Codex MCP server already exists in {}",
                "•".blue(),
                config.display()
            );
            return Ok(());
        }

        let backup = config.with_extension(format!(
            "toml.bak-lumi-tester-{}",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        ));
        tokio::fs::copy(&config, &backup).await?;
        println!("  Backed up Codex config: {}", backup.display());
    }

    let snippet = tokio::fs::read_to_string(snippet_path).await?;
    let mut updated = if config.is_file() {
        tokio::fs::read_to_string(&config).await?
    } else {
        String::new()
    };
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push('\n');
    updated.push_str(&snippet);
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    tokio::fs::write(&config, updated).await?;

    println!(
        "{} Configured Codex MCP server in {}",
        "•".blue(),
        config.display()
    );
    Ok(())
}

async fn download_to_file(url: &str, output: &Path) -> Result<()> {
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to download {}", url))?
        .error_for_status()
        .with_context(|| format!("Download returned an error status: {}", url))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read response body: {}", url))?;
    tokio::fs::write(output, bytes)
        .await
        .with_context(|| format!("Failed to write {}", output.display()))?;
    Ok(())
}

async fn url_exists(url: &str) -> Result<bool> {
    let response = reqwest::Client::new()
        .head(url)
        .send()
        .await
        .with_context(|| format!("Failed to check {}", url))?;
    Ok(response.status().is_success())
}

fn release_base_url(repo: &str, version: &str) -> String {
    if version == "latest" {
        format!("https://github.com/{}/releases/latest/download", repo)
    } else {
        format!("https://github.com/{}/releases/download/{}", repo, version)
    }
}

fn raw_base_url(repo: &str, version: &str, git_ref: &str) -> String {
    if version == "latest" {
        raw_ref_base_url(repo, git_ref)
    } else {
        format!("https://raw.githubusercontent.com/{}/{}", repo, version)
    }
}

fn raw_ref_base_url(repo: &str, git_ref: &str) -> String {
    format!("https://raw.githubusercontent.com/{}/{}", repo, git_ref)
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}
