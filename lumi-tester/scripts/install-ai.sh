#!/usr/bin/env bash
set -euo pipefail

REPO="${LUMI_TESTER_REPO:-Nghi-NV/nl-tester}"
VERSION="${LUMI_TESTER_VERSION:-latest}"
REF="${LUMI_TESTER_REF:-main}"
AI_HOME="${LUMI_AI_HOME:-$HOME/.lumi-tester/ai}"
CODEX_HOME="${CODEX_HOME:-$HOME/.codex}"
CONFIGURE_CODEX="${LUMI_AI_CONFIGURE_CODEX:-1}"
SKIP_CLI="${LUMI_AI_SKIP_CLI:-0}"

say() {
  printf '%s\n' "$*"
}

fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "Missing required command: $1"
}

download() {
  local url="$1"
  local output="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$output"
  else
    fail "Missing curl or wget"
  fi
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os:$arch" in
    Darwin:arm64) echo "aarch64-apple-darwin" ;;
    Darwin:x86_64) echo "x86_64-apple-darwin" ;;
    Linux:x86_64|Linux:amd64) echo "x86_64-unknown-linux-gnu" ;;
    Linux:aarch64|Linux:arm64) echo "aarch64-unknown-linux-gnu" ;;
    *) fail "Unsupported platform: $os $arch" ;;
  esac
}

release_base_url() {
  if [ "$VERSION" = "latest" ]; then
    echo "https://github.com/$REPO/releases/latest/download"
  else
    echo "https://github.com/$REPO/releases/download/$VERSION"
  fi
}

raw_base_url() {
  if [ "$VERSION" != "latest" ]; then
    echo "https://raw.githubusercontent.com/$REPO/$VERSION"
  else
    echo "https://raw.githubusercontent.com/$REPO/$REF"
  fi
}

raw_ref_base_url() {
  echo "https://raw.githubusercontent.com/$REPO/$REF"
}

url_exists() {
  local url="$1"
  if command -v curl >/dev/null 2>&1; then
    curl -fsIL "$url" >/dev/null 2>&1
  elif command -v wget >/dev/null 2>&1; then
    wget --spider -q "$url" >/dev/null 2>&1
  else
    fail "Missing curl or wget"
  fi
}

install_cli() {
  if [ "$SKIP_CLI" = "1" ]; then
    say "Skipping CLI install because LUMI_AI_SKIP_CLI=1"
    return
  fi

  local tmp_dir installer
  tmp_dir="$(mktemp -d)"
  installer="$tmp_dir/install.sh"

  say "Installing Lumi Tester CLI..."
  download "$(raw_base_url)/lumi-tester/scripts/install.sh" "$installer"
  chmod +x "$installer"
  LUMI_TESTER_REPO="$REPO" LUMI_TESTER_VERSION="$VERSION" "$installer"
  rm -rf "$tmp_dir"
}

install_mcp() {
  need_cmd node
  need_cmd npm

  local target asset base_url tmp_dir tgz package_dir server_path
  target="$(detect_target)"
  asset="lumi-tester-mcp-$target.tgz"
  base_url="$(release_base_url)"
  tmp_dir="$(mktemp -d)"
  tgz="$tmp_dir/$asset"
  package_dir="$AI_HOME/mcp"
  server_path="$package_dir/node_modules/lumi-tester-mcp/src/server.js"

  say "Installing Lumi Tester MCP package..."
  say "  Asset: $asset"
  download "$base_url/$asset" "$tgz"
  mkdir -p "$package_dir"
  npm install --prefix "$package_dir" "$tgz" --omit=dev --no-audit --no-fund

  [ -f "$server_path" ] || fail "MCP server was not installed at $server_path"
  say "Installed MCP server: $server_path"
  rm -rf "$tmp_dir"
}

install_codex_skill() {
  local skill_dir base files file
  skill_dir="$CODEX_HOME/skills/lumi-tester-agent"
  base="$(raw_base_url)/lumi-tester/ai/codex-skill/lumi-tester-agent"
  files=(
    "SKILL.md"
    "references/android-auto.md"
    "references/cli.csv"
    "references/command-catalog.md"
    "references/commands.csv"
    "references/debug-artifacts.md"
    "references/desktop.md"
    "references/hardware.md"
    "references/hc-execution.md"
    "references/headers.csv"
    "references/index.md"
    "references/patterns.md"
    "references/requirements-example/cases.csv"
    "references/requirements-example/requirements/auth.yaml"
    "references/requirements-example/requirements/common.yaml"
    "references/requirements-example/requirements/index.yaml"
    "references/requirements-example/smoke/001_login_otp.yaml"
    "references/requirements-example/smoke/002_login_otp_negative.yaml"
    "references/selector-discovery.md"
    "references/selectors.csv"
    "references/testcase-design.md"
    "scripts/lumi_agent.py"
    "agents/openai.yaml"
  )

  if [ "$VERSION" != "latest" ]; then
    for file in "${files[@]}"; do
      if ! url_exists "$base/$file"; then
        say "Warning: skill file $file is not available at $VERSION; falling back to $REF"
        base="$(raw_ref_base_url)/lumi-tester/ai/codex-skill/lumi-tester-agent"
        break
      fi
    done
  fi

  say "Installing Codex skill..."
  mkdir -p "$skill_dir/references" "$skill_dir/scripts" "$skill_dir/agents"
  for file in "${files[@]}"; do
    download "$base/$file" "$skill_dir/$file"
  done
  chmod +x "$skill_dir/scripts/lumi_agent.py"
  say "Installed Codex skill: $skill_dir"
}

write_config_snippets() {
  local lumi_bin server_path codex_snippet claude_snippet
  lumi_bin="$(command -v lumi-tester || true)"
  [ -n "$lumi_bin" ] || lumi_bin="${LUMI_INSTALL_DIR:-$HOME/.local/bin}/lumi-tester"
  server_path="$AI_HOME/mcp/node_modules/lumi-tester-mcp/src/server.js"
  codex_snippet="$AI_HOME/lumi-tester-mcp.codex.toml"
  claude_snippet="$AI_HOME/lumi-tester-mcp.claude.json"

  mkdir -p "$AI_HOME"
  cat >"$codex_snippet" <<EOF
[mcp_servers.lumi-tester]
command = "node"
args = ["$server_path"]
env = { LUMI_TESTER_BIN = "$lumi_bin" }
startup_timeout_sec = 10
tool_timeout_sec = 300
EOF

  cat >"$claude_snippet" <<EOF
{
  "mcpServers": {
    "lumi-tester": {
      "command": "node",
      "args": ["$server_path"],
      "env": {
        "LUMI_TESTER_BIN": "$lumi_bin"
      }
    }
  }
}
EOF

  say "Wrote MCP config snippets:"
  say "  Codex: $codex_snippet"
  say "  Claude: $claude_snippet"
}

configure_codex() {
  [ "$CONFIGURE_CODEX" = "1" ] || return

  local config snippet
  config="$CODEX_HOME/config.toml"
  snippet="$AI_HOME/lumi-tester-mcp.codex.toml"
  mkdir -p "$CODEX_HOME"

  if [ -f "$config" ] && grep -q '^\[mcp_servers\.lumi-tester\]' "$config"; then
    say "Codex MCP server already exists in $config"
    return
  fi

  if [ -f "$config" ]; then
    cp "$config" "$config.bak-lumi-tester-$(date +%Y%m%d%H%M%S)"
  fi

  {
    printf '\n'
    cat "$snippet"
    printf '\n'
  } >>"$config"

  say "Configured Codex MCP server in $config"
}

main() {
  need_cmd uname
  need_cmd mktemp

  install_cli
  install_mcp
  install_codex_skill
  write_config_snippets
  configure_codex

  say ""
  say "Lumi Tester AI pack installed."
  say "Restart your AI client, then ask it to use the lumi-tester agent/MCP tools."
  say "Quick checks:"
  say "  lumi-tester doctor --platform android --json"
  say "  lumi-tester doctor --platform android_auto --json"
  say "  lumi-tester doctor --platform ios --json  # macOS + idb"
  say "  lumi-tester doctor --platform web --json"
  say "  lumi-tester doctor --platform macos --json"
  say "  lumi-tester doctor --platform windows --json"
  say "  python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-schema"
  say "  python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-check path/to/test.yaml --summary-json ./output/agent-check.json"
  say "  node \"$AI_HOME/mcp/node_modules/lumi-tester-mcp/src/server.js\""
}

main "$@"
