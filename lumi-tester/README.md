# 🚀 lumi-tester

`lumi-tester` is a high-performance command-line tool (CLI) written in Rust, designed for automating mobile, web, and desktop application testing. It empowers testers to define test scenarios using simple, intuitive YAML files.

## ✨ Highlights

- 📝 **YAML DSL**: Script-free test authoring with straightforward syntax.
- 📍 **Mock Location**: Simulate GPS coordinates via GPX, KML, or JSON files.
- 🎨 **Visual Assertion**: Verify UI consistency with color (`assertColor`) and pixel-level precision.
- 📹 **Recording & Screenshots**: Automatically capture images and videos during test execution.

### Support Platforms
- **Android**: ADB, UiAutomator (full control)
- **Android Auto**: Android device plus Desktop Head Unit (DHU)
- **iOS**: IDB (simulators & devices), XCUITest
- **Web**: Playwright (Chrome/Firefox/WebKit)
- **macOS**: native desktop automation through Accessibility, Apple Events, screenshots, and clipboard tooling
- **Windows**: native desktop automation through PowerShell and UI Automation

### Key Features
- **Cross-platform DSL**: Write once, run everywhere (Android, Android Auto, iOS, Web, macOS, Windows).
- **Smart Selectors**: Support for `text`, `id`, `css`, `xpath`, `regex` (including advanced patterns like `\d+`, `[...]`, `(...)`), and `point`.
- **Control Flow**: Advanced logic with `repeat`, `conditional`, `runFlow`, and `variables`.
- **Python Integration**: Run external Python scripts (`runPython`) with argument passing, custom env, and automatic single/multi variable extraction (`saveVar`, `saveVars`).
- **Media Support**: Integrated screenshots and video recording (video currently Android only).
- **Professional Reports**: Automated HTML/JSON reports with failure context and embedded media.
- 🛠️ **Parallel Execution**: Scale testing by running concurrently on multiple devices.

### HC orchestration

Inventory-driven HC plans can be validated and aggregated without moving HC
transport into the orchestrator:

```bash
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate plan.yaml --for-run --json
cargo run --manifest-path lumi-tester/Cargo.toml -- plan validate-delegation plan.yaml response.json --json
```

See [docs/orchestrator.md](docs/orchestrator.md) for lifecycle, inventory
drift, delegation, restoration, and evidence contracts.

## 📦 Installation

### AI Pack (recommended for AI agents)

The AI pack installs:

- `lumi-tester` CLI.
- A platform-matched `lumi-tester-mcp` package with a bundled binary.
- The Codex skill `lumi-tester-agent`.
- MCP config snippets for Codex and Claude-style clients.

If you installed the CLI with Homebrew, use the built-in AI installer:

```bash
brew install nghi-nv/tap/lumi-tester
lumi-tester ai install
```

The built-in installer lets you choose Codex, Claude Code, and/or Antigravity.
Codex and Claude Code also receive the MCP package; Antigravity receives its
global and workspace skills. To skip the picker, pass `--client codex`,
`--client claude`, or `--client antigravity`.

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install-ai.sh | bash
```

Windows PowerShell:

```powershell
iwr https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install-ai.ps1 -UseB | iex
```

Requirements:

- `node` and `npm` for the MCP server.
- Restart the AI client after install.
- Device/browser dependencies for the platform under test. Run `lumi-tester system install --all` when you want Lumi Tester to install common local dependencies.

Pin a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install-ai.sh | LUMI_TESTER_VERSION=v0.1.7 bash
lumi-tester ai install --version v0.1.7
```

Quick checks:

```bash
lumi-tester --version
lumi-tester doctor --platform android --json
lumi-tester doctor --platform android_auto --json
lumi-tester doctor --platform ios --json  # macOS + idb
lumi-tester doctor --platform web --json
lumi-tester doctor --platform macos --json
lumi-tester doctor --platform windows --json
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-schema
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-check path/to/test.yaml --summary-json ./output/agent-check.json
```

### CLI One-line Install

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install.sh | bash
```

Windows PowerShell:

```powershell
iwr https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install.ps1 -UseB | iex
```

The scripts detect OS/CPU, download the matching GitHub Release binary, add it to PATH, verify `SHA256SUMS` when available, and run `lumi-tester system install --all`.

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install.sh | LUMI_TESTER_VERSION=v0.1.7 bash
```

Skip driver/browser initialization:

```bash
curl -fsSL https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install.sh | LUMI_SKIP_SYSTEM_INSTALL=1 bash
```

### Package Managers

Homebrew tap:

```bash
brew install nghi-nv/tap/lumi-tester
```

Scoop bucket:

```powershell
scoop bucket add Nghi-NV https://github.com/Nghi-NV/scoop-bucket.git
scoop install lumi-tester
```

Winget manifests are attached to each release. After the package is accepted into `microsoft/winget-pkgs`, Windows users can install it with:

```powershell
winget install NghiNV.LumiTester
```

### Manual Download

Download native binaries from [Releases](https://github.com/Nghi-NV/nl-tester/releases):

- **Windows**: `lumi-tester-x86_64-pc-windows-msvc.exe` or `lumi-tester-aarch64-pc-windows-msvc.exe`.
- **macOS**: `lumi-tester-aarch64-apple-darwin` for Apple Silicon, or `lumi-tester-x86_64-apple-darwin` for Intel.
- **Linux**: `lumi-tester-x86_64-unknown-linux-gnu` or `lumi-tester-aarch64-unknown-linux-gnu`.

## 🚀 Getting Started

Once installed, use the `lumi-tester` command directly in your terminal.

### 1. Run Tests
Execute a single test file or an entire directory:

```bash
# Run a specific file
lumi-tester run ./e2e/workspaces/login_flow.yaml

# Run all tests in a directory
lumi-tester run ./e2e/workspaces/
```

### 2. Environment Management
If you encounter ADB or driver issues, use the following command to repair/reinstall:

```bash
lumi-tester system install --all
```

### 3. List Connected Devices
```bash
lumi-tester devices
```

## 📚 Documentation

Deep-dive into our guides located in the `docs/` directory:

1. [**Writing Tests Guide**](docs/writing_tests.md): Master YAML syntax and selectors.
2. [**Python Integration Guide**](docs/python-integration.md): Run Python scripts and extract variables.
3. [**Commands Reference**](docs/api/commands.md): Comprehensive details on all supported commands (`tap`, `see`, `mockLocation`, `runPython`, etc.).
4. [**Test Flow Structure**](docs/flows/test_execution_flow.md): Understand the lifecycle and flow of a test session.

---

## 💡 Quick Example

```yaml
appId: com.example.app
---
- open: "com.example.app"
- see: "Welcome"
- tap: "Login"
- inputText:
    id: "email_field"
    text: "test@example.com"
- assertColor:
    point: "50%,50%"
    color: "#4CAF50"
- takeScreenshot: "completed.png"
```
