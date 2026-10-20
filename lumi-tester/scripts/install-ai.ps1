$ErrorActionPreference = "Stop"

$Repo = if ($env:LUMI_TESTER_REPO) { $env:LUMI_TESTER_REPO } else { "Nghi-NV/nl-tester" }
$Version = if ($env:LUMI_TESTER_VERSION) { $env:LUMI_TESTER_VERSION } else { "latest" }
$Ref = if ($env:LUMI_TESTER_REF) { $env:LUMI_TESTER_REF } else { "main" }
$AiHome = if ($env:LUMI_AI_HOME) { $env:LUMI_AI_HOME } else { Join-Path $env:USERPROFILE ".lumi-tester\ai" }
$CodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE ".codex" }
$ConfigureCodex = $env:LUMI_AI_CONFIGURE_CODEX -ne "0"
$SkipCli = $env:LUMI_AI_SKIP_CLI -eq "1"

function Fail($Message) {
    Write-Error $Message
    exit 1
}

function Require-Command($Command) {
    if (!(Get-Command $Command -ErrorAction SilentlyContinue)) {
        Fail "Missing required command: $Command"
    }
}

function Get-Target {
    switch ($env:PROCESSOR_ARCHITECTURE) {
        "ARM64" { return "aarch64-pc-windows-msvc" }
        "AMD64" { return "x86_64-pc-windows-msvc" }
        default { Fail "Unsupported Windows architecture: $env:PROCESSOR_ARCHITECTURE" }
    }
}

function Get-ReleaseBaseUrl {
    if ($Version -eq "latest") {
        return "https://github.com/$Repo/releases/latest/download"
    }
    return "https://github.com/$Repo/releases/download/$Version"
}

function Get-RawBaseUrl {
    if ($Version -ne "latest") {
        return "https://raw.githubusercontent.com/$Repo/$Version"
    }
    return "https://raw.githubusercontent.com/$Repo/$Ref"
}

function Get-RawRefBaseUrl {
    return "https://raw.githubusercontent.com/$Repo/$Ref"
}

function Download-File($Url, $Output) {
    Invoke-WebRequest -Uri $Url -OutFile $Output -UseBasicParsing
}

function Test-UrlExists($Url) {
    try {
        Invoke-WebRequest -Uri $Url -Method Head -UseBasicParsing | Out-Null
        return $true
    } catch {
        return $false
    }
}

function Install-Cli {
    if ($SkipCli) {
        Write-Host "Skipping CLI install because LUMI_AI_SKIP_CLI=1"
        return
    }

    $tempDir = Join-Path $env:TEMP ("lumi-ai-install-" + [Guid]::NewGuid().ToString("N"))
    $installer = Join-Path $tempDir "install.ps1"
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    try {
        Write-Host "Installing Lumi Tester CLI..."
        Download-File "$(Get-RawBaseUrl)/lumi-tester/scripts/install.ps1" $installer
        $env:LUMI_TESTER_REPO = $Repo
        $env:LUMI_TESTER_VERSION = $Version
        & $installer
    } finally {
        if (Test-Path $tempDir) {
            Remove-Item -Recurse -Force $tempDir
        }
    }
}

function Install-Mcp {
    Require-Command node
    Require-Command npm

    $target = Get-Target
    $asset = "lumi-tester-mcp-$target.tgz"
    $baseUrl = Get-ReleaseBaseUrl
    $tempDir = Join-Path $env:TEMP ("lumi-ai-mcp-" + [Guid]::NewGuid().ToString("N"))
    $tgz = Join-Path $tempDir $asset
    $packageDir = Join-Path $AiHome "mcp"
    $serverPath = Join-Path $packageDir "node_modules\lumi-tester-mcp\src\server.js"

    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    try {
        Write-Host "Installing Lumi Tester MCP package..."
        Write-Host "  Asset: $asset"
        Download-File "$baseUrl/$asset" $tgz
        New-Item -ItemType Directory -Force -Path $packageDir | Out-Null
        npm install --prefix $packageDir $tgz --omit=dev --no-audit --no-fund
        if (!(Test-Path $serverPath)) {
            Fail "MCP server was not installed at $serverPath"
        }
        Write-Host "Installed MCP server: $serverPath"
    } finally {
        if (Test-Path $tempDir) {
            Remove-Item -Recurse -Force $tempDir
        }
    }
}

function Install-CodexSkill {
    $skillDir = Join-Path $CodexHome "skills\lumi-tester-agent"
    $base = "$(Get-RawBaseUrl)/lumi-tester/ai/codex-skill/lumi-tester-agent"
    $files = @(
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
        "agents/openai.yaml"
    )

    if ($Version -ne "latest") {
        foreach ($file in $files) {
            if (!(Test-UrlExists "$base/$file")) {
                Write-Host "Warning: skill file $file is not available at $Version; falling back to $Ref"
                $base = "$(Get-RawRefBaseUrl)/lumi-tester/ai/codex-skill/lumi-tester-agent"
                break
            }
        }
    }

    Write-Host "Installing Codex skill..."
    New-Item -ItemType Directory -Force -Path (Join-Path $skillDir "references") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $skillDir "scripts") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $skillDir "agents") | Out-Null

    foreach ($file in $files) {
        Download-File "$base/$file" (Join-Path $skillDir $file)
    }
    Write-Host "Installed Codex skill: $skillDir"
}

function Write-ConfigSnippets {
    $lumiCommand = Get-Command lumi-tester -ErrorAction SilentlyContinue
    if ($lumiCommand) {
        $lumiBin = $lumiCommand.Source
    } else {
        $lumiBin = Join-Path $env:USERPROFILE ".lumi-tester\bin\lumi-tester.exe"
    }

    $serverPath = Join-Path $AiHome "mcp\node_modules\lumi-tester-mcp\src\server.js"
    $codexSnippet = Join-Path $AiHome "lumi-tester-mcp.codex.toml"
    $claudeSnippet = Join-Path $AiHome "lumi-tester-mcp.claude.json"
    New-Item -ItemType Directory -Force -Path $AiHome | Out-Null

    @"
[mcp_servers.lumi-tester]
command = "node"
args = ["$($serverPath.Replace('\', '\\'))"]
env = { LUMI_TESTER_BIN = "$($lumiBin.Replace('\', '\\'))" }
startup_timeout_sec = 10
tool_timeout_sec = 300
"@ | Set-Content -Path $codexSnippet -Encoding UTF8

    @"
{
  "mcpServers": {
    "lumi-tester": {
      "command": "node",
      "args": ["$($serverPath.Replace('\', '\\'))"],
      "env": {
        "LUMI_TESTER_BIN": "$($lumiBin.Replace('\', '\\'))"
      }
    }
  }
}
"@ | Set-Content -Path $claudeSnippet -Encoding UTF8

    Write-Host "Wrote MCP config snippets:"
    Write-Host "  Codex: $codexSnippet"
    Write-Host "  Claude: $claudeSnippet"
}

function Configure-Codex {
    if (!$ConfigureCodex) {
        return
    }

    $config = Join-Path $CodexHome "config.toml"
    $snippet = Join-Path $AiHome "lumi-tester-mcp.codex.toml"
    New-Item -ItemType Directory -Force -Path $CodexHome | Out-Null

    if ((Test-Path $config) -and ((Get-Content $config -Raw) -match '\[mcp_servers\.lumi-tester\]')) {
        Write-Host "Codex MCP server already exists in $config"
        return
    }

    if (Test-Path $config) {
        Copy-Item $config "$config.bak-lumi-tester-$(Get-Date -Format yyyyMMddHHmmss)"
    }

    Add-Content -Path $config -Value ""
    Add-Content -Path $config -Value (Get-Content $snippet -Raw)
    Write-Host "Configured Codex MCP server in $config"
}

Install-Cli
Install-Mcp
Install-CodexSkill
Write-ConfigSnippets
Configure-Codex

Write-Host ""
Write-Host "Lumi Tester AI pack installed."
Write-Host "Restart your AI client, then ask it to use the lumi-tester agent/MCP tools."
Write-Host "Quick checks:"
Write-Host "  lumi-tester doctor --platform android --json"
Write-Host "  lumi-tester doctor --platform android_auto --json"
Write-Host "  lumi-tester doctor --platform web --json"
Write-Host "  # iOS checks require macOS + idb: lumi-tester doctor --platform ios --json"
Write-Host "  lumi-tester doctor --platform macos --json"
Write-Host "  lumi-tester doctor --platform windows --json"
Write-Host "  python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-schema"
Write-Host "  python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-check path/to/test.yaml --summary-json ./output/agent-check.json"
Write-Host "  node `"$AiHome\mcp\node_modules\lumi-tester-mcp\src\server.js`""
