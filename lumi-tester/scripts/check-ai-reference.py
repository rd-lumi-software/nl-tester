#!/usr/bin/env python3
"""Validate AI reference CSV files against parser command names."""

from __future__ import annotations

import contextlib
import csv
import importlib.util
import io
import json
import re
import sys
import tempfile
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[2]
ROOT_README_MD = ROOT / "README.md"
YAML_RS = ROOT / "lumi-tester" / "src" / "parser" / "yaml.rs"
COMMANDS_CSV = (
    ROOT
    / "lumi-tester"
    / "ai"
    / "codex-skill"
    / "lumi-tester-agent"
    / "references"
    / "commands.csv"
)
SKILL_DIR = ROOT / "lumi-tester" / "ai" / "codex-skill" / "lumi-tester-agent"
SKILL_MD = SKILL_DIR / "SKILL.md"
SELECTORS_CSV = (
    ROOT
    / "lumi-tester"
    / "ai"
    / "codex-skill"
    / "lumi-tester-agent"
    / "references"
    / "selectors.csv"
)
CLI_CSV = (
    ROOT
    / "lumi-tester"
    / "ai"
    / "codex-skill"
    / "lumi-tester-agent"
    / "references"
    / "cli.csv"
)
HEADERS_CSV = SKILL_DIR / "references" / "headers.csv"
OPENAI_YAML = SKILL_DIR / "agents" / "openai.yaml"
TESTCASE_DESIGN_MD = SKILL_DIR / "references" / "testcase-design.md"
DEBUG_ARTIFACTS_MD = SKILL_DIR / "references" / "debug-artifacts.md"
REFERENCE_INDEX_MD = SKILL_DIR / "references" / "index.md"
PATTERNS_MD = SKILL_DIR / "references" / "patterns.md"
DESKTOP_MD = SKILL_DIR / "references" / "desktop.md"
ANDROID_AUTO_MD = SKILL_DIR / "references" / "android-auto.md"
AI_AUTHORING_MD = ROOT / "lumi-tester" / "docs" / "ai-authoring.md"
SCHEMA_JSON = ROOT / "lumi-tester" / "schema" / "lumi-test.schema.json"
HELPER_SCRIPT = SKILL_DIR / "scripts" / "lumi_agent.py"
AI_RS = ROOT / "lumi-tester" / "src" / "ai.rs"
INSTALL_AI_SH = ROOT / "lumi-tester" / "scripts" / "install-ai.sh"
INSTALL_AI_PS1 = ROOT / "lumi-tester" / "scripts" / "install-ai.ps1"
MCP_SERVER_JS = ROOT / "lumi-tester-mcp" / "src" / "server.js"
MCP_README = ROOT / "lumi-tester-mcp" / "README.md"
README_MD = ROOT / "lumi-tester" / "README.md"
CLI_REFERENCE_MD = ROOT / "lumi-tester" / "docs" / "cli-reference.md"
WRITING_TESTS_MD = ROOT / "lumi-tester" / "docs" / "writing_tests.md"
COMMANDS_MD = ROOT / "lumi-tester" / "docs" / "api" / "commands.md"
AUTOMOTIVE_TESTING_MD = ROOT / "lumi-tester" / "docs" / "automotive-testing.md"
HARDWARE_CONTROL_MD = ROOT / "lumi-tester" / "docs" / "hardware-control.md"
CAMERA_HARDWARE_MD = ROOT / "lumi-tester" / "docs" / "camera-hardware-testing.md"
PYTHON_INTEGRATION_MD = ROOT / "lumi-tester" / "docs" / "python-integration.md"
FLOWS_MD = ROOT / "lumi-tester" / "docs" / "flows" / "test_execution_flow.md"
DOCS_INDEX_HTML = ROOT / "lumi-tester" / "docs" / "index.html"
DESKTOP_TESTING_MD = ROOT / "lumi-tester" / "docs" / "desktop-testing.md"
DISTRIBUTION_MD = ROOT / "lumi-tester" / "docs" / "distribution.md"
PACKAGE_MANIFEST_SCRIPT = ROOT / "lumi-tester" / "scripts" / "generate-package-manifests.sh"
MAIN_RS = ROOT / "lumi-tester" / "src" / "main.rs"
REQUIRED_AGENT_PLATFORMS = {"android", "android_auto", "ios", "web", "macos", "windows"}
REQUIRED_AGENT_RUN_PLATFORMS = REQUIRED_AGENT_PLATFORMS | {"lumi_hc"}


def parser_commands() -> set[str]:
    commands: set[str] = set()
    arm_re = re.compile(r"^\s*((?:\"[^\"]+\"\s*(?:\|\s*)?)+)\s*=>")
    for line in YAML_RS.read_text(encoding="utf-8").splitlines():
        match = arm_re.match(line)
        if not match:
            continue
        commands.update(re.findall(r'"([^"]+)"', match.group(1)))
    return commands


def cli_commands() -> set[str]:
    text = (ROOT / "lumi-tester" / "src" / "main.rs").read_text(encoding="utf-8")
    match = re.search(r"enum Commands\s*\{(.*?)\n\}", text, flags=re.DOTALL)
    if not match:
        return set()
    variants: set[str] = set()
    for line in match.group(1).splitlines():
        match_line = re.match(r"^\s*([A-Z][A-Za-z0-9]*)\s*(?:\{|,)", line)
        if match_line:
            name = match_line.group(1)
            variants.add(re.sub(r"(?<!^)([A-Z])", r"-\1", name).lower())
    return variants


def csv_command_names() -> set[str]:
    names: set[str] = set()
    with COMMANDS_CSV.open(newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh):
            names.add(row["command"].strip())
            aliases = row.get("aliases", "")
            for alias in re.split(r"[|,]", aliases):
                alias = alias.strip()
                if alias:
                    names.add(alias)
    return names


def command_catalog_rows() -> dict[str, dict[str, str]]:
    rows: dict[str, dict[str, str]] = {}
    with COMMANDS_CSV.open(newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh):
            names = [row["command"].strip()]
            names.extend(
                alias.strip()
                for alias in re.split(r"[|,]", row.get("aliases", ""))
                if alias.strip()
            )
            for name in names:
                rows[name] = row
    return rows


def split_field_names(value: str) -> set[str]:
    return {item.strip() for item in re.split(r"[|,]", value) if item.strip()}


def split_aliases(value: str) -> list[str]:
    return [item.strip() for item in re.split(r"[|,]", value) if item.strip()]


def validate_csv(path: Path, required_columns: set[str]) -> list[str]:
    errors: list[str] = []
    with path.open(newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        missing = required_columns.difference(reader.fieldnames or [])
        if missing:
            errors.append(f"{path}: missing columns: {', '.join(sorted(missing))}")
        rows = list(reader)
    if not rows:
        errors.append(f"{path}: no data rows")
    return errors


def validate_csv_alias_quality(path: Path, primary_column: str) -> list[str]:
    errors: list[str] = []
    seen: dict[str, str] = {}
    with path.open(newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh):
            primary = row[primary_column].strip()
            names = [primary, *split_aliases(row.get("aliases", ""))]
            seen_in_row: set[str] = set()
            for name in names:
                if name in seen_in_row:
                    errors.append(f"{path}: {primary} repeats alias/name: {name}")
                seen_in_row.add(name)

                previous = seen.get(name)
                if previous and previous != primary:
                    errors.append(
                        f"{path}: alias/name {name} is used by both {previous} and {primary}"
                    )
                seen[name] = primary
            for alias in split_aliases(row.get("aliases", "")):
                if alias == primary:
                    errors.append(f"{path}: {primary} aliases itself")
    return errors


def validate_headers_catalog() -> list[str]:
    errors: list[str] = []
    with HEADERS_CSV.open(newline="", encoding="utf-8") as fh:
        rows = {row["field"].strip(): row for row in csv.DictReader(fh)}

    required_fields = {
        "platform",
        "appId",
        "url",
        "desktopState",
        "desktopState.clear",
        "desktopState.clear.mode",
        "desktopState.clear.paths",
        "desktopState.clear.keychainServices",
        "desktopState.clear.registryKeys",
    }
    missing = sorted(required_fields.difference(rows))
    if missing:
        errors.append(f"{HEADERS_CSV}: missing header fields: {', '.join(missing)}")

    schema = json.loads(SCHEMA_JSON.read_text(encoding="utf-8"))
    for field in ("platform", "appId", "url", "desktopState"):
        if field not in schema["properties"]:
            errors.append(f"{SCHEMA_JSON}: missing documented header field: {field}")

    desktop_clear = schema["$defs"].get("desktopClear", {})
    clear_properties = desktop_clear.get("properties", {})
    for field in ("mode", "paths", "keychainServices", "registryKeys"):
        if field not in clear_properties:
            errors.append(f"{SCHEMA_JSON}: missing documented desktop clear field: {field}")

    mode_row = rows.get("desktopState.clear.mode", {})
    if "autosafe" not in mode_row.get("notes", "").lower():
        errors.append(f"{HEADERS_CSV}: desktopState.clear.mode should explain autoSafe")
    if "manual" not in mode_row.get("notes", "").lower():
        errors.append(f"{HEADERS_CSV}: desktopState.clear.mode should explain manual")

    catalog_text = (SKILL_DIR / "references" / "command-catalog.md").read_text(
        encoding="utf-8"
    )
    skill_text = SKILL_MD.read_text(encoding="utf-8")
    for term in ("references/headers.csv", "desktopState.clear.mode", "registryKeys"):
        if term not in catalog_text and term not in skill_text:
            errors.append(f"{HEADERS_CSV}: {term} is not surfaced in skill guidance")
    if "Read or search `references/headers.csv`" not in skill_text:
        errors.append(f"{SKILL_MD}: Extra Reference should mention headers.csv")
    return errors


def schema_command_names() -> set[str]:
    schema = json.loads(SCHEMA_JSON.read_text(encoding="utf-8"))
    return set(schema["$defs"]["commandName"]["enum"])


def schema_selector_names() -> set[str]:
    schema = json.loads(SCHEMA_JSON.read_text(encoding="utf-8"))
    return set(schema["$defs"]["selector"]["properties"])


def schema_selector_example_names() -> set[str]:
    schema = json.loads(SCHEMA_JSON.read_text(encoding="utf-8"))
    names = set(schema["$defs"]["selector"]["properties"])
    scrollable = schema["$defs"]["selector"]["properties"]["scrollable"]["oneOf"][1]
    names.update(scrollable["properties"])
    ocr_selector = schema["$defs"]["ocrSelector"]["oneOf"][1]
    names.update(ocr_selector["properties"])
    return names


def validate_skill_references() -> list[str]:
    errors: list[str] = []
    text = SKILL_MD.read_text(encoding="utf-8")
    refs = sorted(set(re.findall(r"references/[A-Za-z0-9_.-]+", text)))
    for ref in refs:
        path = SKILL_DIR / ref
        if not path.exists():
            errors.append(f"{SKILL_MD}: referenced file does not exist: {ref}")
    linked = set(refs)
    for path in sorted((SKILL_DIR / "references").glob("*")):
        if path.is_file() and f"references/{path.name}" not in linked:
            errors.append(f"{path}: reference file is not linked from {SKILL_MD}")
    return errors


def skill_file_paths() -> list[str]:
    paths: list[str] = []
    for path in sorted(SKILL_DIR.rglob("*")):
        if not path.is_file():
            continue
        if "__pycache__" in path.parts:
            continue
        if path.suffix == ".pyc":
            continue
        paths.append(path.relative_to(SKILL_DIR).as_posix())
    return paths


def validate_skill_install_file_lists() -> list[str]:
    errors: list[str] = []
    expected = skill_file_paths()
    install_sources = {
        AI_RS: AI_RS.read_text(encoding="utf-8"),
        INSTALL_AI_SH: INSTALL_AI_SH.read_text(encoding="utf-8"),
        INSTALL_AI_PS1: INSTALL_AI_PS1.read_text(encoding="utf-8"),
    }
    for file_path in expected:
        for source_path, text in install_sources.items():
            if file_path not in text:
                errors.append(
                    f"{source_path}: skill installer is missing {file_path}"
                )
    return errors


def normalize_heading(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", " ", value.lower()).strip()


def validate_reference_navigation() -> list[str]:
    errors: list[str] = []
    for path in sorted((SKILL_DIR / "references").glob("*.md")):
        lines = path.read_text(encoding="utf-8").splitlines()
        if len(lines) <= 100:
            continue
        text = "\n".join(lines[:40])
        if "## Contents" not in text:
            errors.append(f"{path}: long reference should have ## Contents near the top")
        contents_index = next(
            (idx for idx, line in enumerate(lines) if line.strip() == "## Contents"),
            None,
        )
        if contents_index is None:
            continue
        next_heading_index = next(
            (
                idx
                for idx, line in enumerate(lines[contents_index + 1 :], start=contents_index + 1)
                if line.startswith("## ") and idx > contents_index
            ),
            min(len(lines), contents_index + 15),
        )
        bullets = [
            line[2:].strip()
            for line in lines[contents_index + 1 : next_heading_index]
            if line.startswith("- ")
        ]
        if len(bullets) < 3:
            errors.append(f"{path}: Contents should include at least 3 bullets")
            continue
        headings = {
            normalize_heading(line.removeprefix("##").strip())
            for line in lines
            if line.startswith("## ") and line.strip() != "## Contents"
        }
        for bullet in bullets:
            label = re.sub(r"^\[([^\]]+)\]\([^)]+\)$", r"\1", bullet)
            normalized = normalize_heading(label)
            if normalized not in headings:
                errors.append(
                    f"{path}: Contents bullet does not match a ## heading: {bullet}"
                )
    return errors


def validate_reference_index() -> list[str]:
    errors: list[str] = []
    text = REFERENCE_INDEX_MD.read_text(encoding="utf-8")
    skill_text = SKILL_MD.read_text(encoding="utf-8")
    if "references/index.md" not in skill_text:
        errors.append(f"{SKILL_MD}: missing reference index guidance")

    for path in sorted((SKILL_DIR / "references").glob("*")):
        if not path.is_file() or path.name == "index.md":
            continue
        if f"`{path.name}`" not in text:
            errors.append(f"{REFERENCE_INDEX_MD}: missing reference entry: {path.name}")

    required_terms = {
        "Fast Lookup": "fast lookup section",
        "Workflow References": "workflow section",
        "Platform References": "platform section",
        "MCP `suggest_selectors`": "MCP selector guidance",
        "`desktopState.clear`": "desktop state reset guidance",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{REFERENCE_INDEX_MD}: missing {label}")
    return errors


def validate_agents_metadata() -> list[str]:
    errors: list[str] = []
    if not OPENAI_YAML.exists():
        return [f"{OPENAI_YAML}: file does not exist"]

    text = OPENAI_YAML.read_text(encoding="utf-8")
    required = {
        "display_name": r"^\s*display_name\s*:",
        "short_description": r"^\s*short_description\s*:",
        "default_prompt": r"^\s*default_prompt\s*:",
    }
    for field, pattern in required.items():
        if not re.search(pattern, text, flags=re.MULTILINE):
            errors.append(f"{OPENAI_YAML}: missing interface field: {field}")

    if "$lumi-tester-agent" not in text:
        errors.append(f"{OPENAI_YAML}: default_prompt should mention $lumi-tester-agent")
    for term in ("design", "run", "debug", "android auto", "macos", "windows"):
        if term not in text.lower():
            errors.append(f"{OPENAI_YAML}: metadata should mention {term}")
    return errors


def validate_helper_script_reference() -> list[str]:
    errors: list[str] = []
    if not HELPER_SCRIPT.exists():
        return [f"{HELPER_SCRIPT}: file does not exist"]

    helper_text = HELPER_SCRIPT.read_text(encoding="utf-8")
    skill_text = SKILL_MD.read_text(encoding="utf-8")
    for platform in REQUIRED_AGENT_PLATFORMS:
        if f'"{platform}"' not in helper_text:
            errors.append(f"{HELPER_SCRIPT}: helper should accept platform: {platform}")
    if '"lumi_hc"' not in helper_text:
        errors.append(f"{HELPER_SCRIPT}: helper should accept platform: lumi_hc for agent-run")
    required_agent_commands = {
        "agent-check": {
            "validate",
            "list",
            "doctor",
            "run",
            "--json",
            "--report",
            "--snapshot",
            "--events-jsonl",
            "--output",
            "--summary-json",
            "PASSED",
            "FAILED",
            "PASS",
            "command",
            "commandLine",
            "runtimeStatus",
            "blocked",
            "BLOCKED",
        },
        "agent-validate": {"validate", "--json"},
        "agent-list": {"list", "--json"},
        "agent-schema": {"schema", "--json"},
        "agent-doctor": {"doctor", "--platform", "--json"},
        "agent-hc-preflight": {
            "agent-hc-preflight",
            "hcHealth",
            "hcGetEntities",
            "--report",
            "--snapshot",
            "--events-jsonl",
            "--summary-json",
        },
        "agent-run": {"run", "--platform", "--report", "--snapshot", "--events-jsonl", "--output"},
        "agent-debug": {
            "run",
            "--platform",
            "--command-index",
            "--report",
            "--snapshot",
            "--events-jsonl",
            "--output",
        },
    }
    for command, terms in required_agent_commands.items():
        if command not in helper_text:
            errors.append(f"{HELPER_SCRIPT}: missing helper command: {command}")
        if command not in skill_text:
            errors.append(f"{SKILL_MD}: missing helper usage for: {command}")
        for term in terms:
            if term not in helper_text:
                errors.append(f"{HELPER_SCRIPT}: {command} should include {term}")

    with CLI_CSV.open(newline="", encoding="utf-8") as fh:
        cli_reference_commands = {
            row["command"].strip() for row in csv.DictReader(fh) if row["command"].strip()
        }
    for command in sorted(cli_reference_commands):
        if f'"{command}"' not in helper_text:
            errors.append(f"{HELPER_SCRIPT}: missing raw Lumi passthrough command: {command}")

    if "repo-local `cargo run`" not in skill_text:
        errors.append(f"{SKILL_MD}: helper docs should explain repo-local cargo fallback")
    if "falls back to an installed" not in skill_text:
        errors.append(f"{SKILL_MD}: helper docs should explain installed binary fallback")
    return errors


def validate_helper_script_behavior() -> list[str]:
    errors: list[str] = []
    spec = importlib.util.spec_from_file_location("lumi_agent_reference", HELPER_SCRIPT)
    if spec is None or spec.loader is None:
        return [f"{HELPER_SCRIPT}: could not load helper module"]
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)

    agent_run = helper.parse_agent_run(
        [
            "tests/generated/login",
            "--platform",
            "android",
            "--device",
            "emulator-5554",
            "--output",
            "./output/login",
            "--command-index",
            "4",
            "--debug",
        ]
    )
    expected_agent_run = [
        "tests/generated/login",
        "--platform",
        "android",
        "--report",
        "--snapshot",
        "--events-jsonl",
        "--output",
        "./output/login",
        "--device",
        "emulator-5554",
        "--command-index",
        "4",
        "--debug",
    ]
    if agent_run != expected_agent_run:
        errors.append(
            f"{HELPER_SCRIPT}: agent-run parse output changed: {agent_run!r}"
        )

    agent_debug = helper.parse_agent_run(
        ["tests/smoke.yaml", "--platform", "ios", "--command-index", "2"],
        require_command_index=True,
    )
    if "--command-index" not in agent_debug or "2" not in agent_debug:
        errors.append(f"{HELPER_SCRIPT}: agent-debug should require and forward command index")
    for required in ("--report", "--snapshot", "--events-jsonl"):
        if required not in agent_debug:
            errors.append(f"{HELPER_SCRIPT}: agent-debug should include {required}")

    with contextlib.redirect_stderr(io.StringIO()):
        try:
            helper.parse_agent_run(
                ["tests/smoke.yaml", "--platform", "ios"],
                require_command_index=True,
            )
        except SystemExit:
            pass
        else:
            errors.append(f"{HELPER_SCRIPT}: agent-debug should fail without --command-index")

    if helper.parse_agent_path_json(["tests/smoke.yaml"], "validate") != [
        "tests/smoke.yaml",
        "--json",
    ]:
        errors.append(f"{HELPER_SCRIPT}: agent-validate should append --json")
    if helper.parse_agent_doctor(["--platform", "web"]) != ["--platform", "web", "--json"]:
        errors.append(f"{HELPER_SCRIPT}: agent-doctor should append --json")
    agent_check = helper.parse_agent_check(["tests/generated/login", "--platform", "android"])
    if (
        agent_check.path != "tests/generated/login"
        or agent_check.platform != "android"
        or agent_check.run
    ):
        errors.append(f"{HELPER_SCRIPT}: agent-check should parse authoring gates")
    summary_target = Path(tempfile.mkdtemp()) / "agent-check.json"
    agent_check_run = helper.parse_agent_check(
        [
            "tests/generated/login",
            "--platform",
            "android",
            "--device",
            "emulator-5554",
            "--run",
            "--output",
            "./output/login",
            "--summary-json",
            str(summary_target),
            "--debug",
        ]
    )
    if (
        not agent_check_run.run
        or agent_check_run.device != "emulator-5554"
        or agent_check_run.summary_json != str(summary_target)
        or agent_check_run.command_line
        != f"agent-check tests/generated/login --platform android --device emulator-5554 --run --output ./output/login --summary-json {summary_target} --debug"
        or agent_check_run.extra != ["--debug"]
    ):
        errors.append(f"{HELPER_SCRIPT}: agent-check should preserve runtime args")
    helper.write_agent_check_summary(
        agent_check_run,
        [
            {
                "name": "validate",
                "command": "lumi-tester validate tests/generated/login --json",
                "passed": True,
                "exitCode": 0,
            }
        ],
        "passed",
        "passed",
        0,
    )
    if not summary_target.exists():
        errors.append(f"{HELPER_SCRIPT}: agent-check should write summary JSON")
    else:
        summary = json.loads(summary_target.read_text(encoding="utf-8"))
        first_step = summary.get("steps", [{}])[0]
        if (
            summary.get("status") != "passed"
            or summary.get("runtimeStatus") != "passed"
            or summary.get("commandLine") != agent_check_run.command_line
            or first_step.get("name") != "validate"
            or "lumi-tester validate" not in first_step.get("command", "")
        ):
            errors.append(f"{HELPER_SCRIPT}: agent-check summary JSON shape changed")
    with contextlib.redirect_stderr(io.StringIO()):
        try:
            helper.parse_agent_check(["tests/generated/login", "--run"])
        except SystemExit:
            pass
        else:
            errors.append(f"{HELPER_SCRIPT}: agent-check --run should require --platform")
    command, extra = helper.parse_passthrough(["agent-schema"])
    if command != "agent-schema" or extra != []:
        errors.append(f"{HELPER_SCRIPT}: agent-schema should parse without extra args")
    if helper.parse_agent_doctor(["--platform", "windows"]) != [
        "--platform",
        "windows",
        "--json",
    ]:
        errors.append(f"{HELPER_SCRIPT}: agent-doctor should accept windows")
    if helper.parse_agent_doctor(["--platform", "android_auto"]) != [
        "--platform",
        "android_auto",
        "--json",
    ]:
        errors.append(f"{HELPER_SCRIPT}: agent-doctor should accept android_auto")
    auto_run = helper.parse_agent_run(
        ["auto.yaml", "--platform", "android_auto", "--device", "emulator-5554"]
    )
    if "--platform" not in auto_run or "android_auto" not in auto_run:
        errors.append(f"{HELPER_SCRIPT}: agent-run should accept android_auto")
    desktop_run = helper.parse_agent_run(["desktop.yaml", "--platform", "macos"])
    if "--platform" not in desktop_run or "macos" not in desktop_run:
        errors.append(f"{HELPER_SCRIPT}: agent-run should accept macos")
    hc_run = helper.parse_agent_run(["hc.yaml", "--platform", "lumi_hc"])
    if "--platform" not in hc_run or "lumi_hc" not in hc_run:
        errors.append(f"{HELPER_SCRIPT}: agent-run should accept lumi_hc")
    hc_preflight = helper.parse_agent_hc_preflight(
        ["preflight.yaml", "--output", "./output/hc", "--summary-json", "./output/hc.json"]
    )
    expected_hc_preflight = [
        "preflight.yaml",
        "--platform",
        "lumi_hc",
        "--report",
        "--snapshot",
        "--events-jsonl",
        "--output",
        "./output/hc",
    ]
    if hc_preflight.run_extra != expected_hc_preflight:
        errors.append(f"{HELPER_SCRIPT}: HC preflight should use hcHealth flow artifacts")
    if hc_preflight.summary_json != "./output/hc.json":
        errors.append(f"{HELPER_SCRIPT}: HC preflight should preserve summary path")
    with contextlib.redirect_stderr(io.StringIO()):
        try:
            helper.parse_agent_doctor(["--platform", "lumi_hc"])
        except SystemExit:
            pass
        else:
            errors.append(f"{HELPER_SCRIPT}: agent-doctor must reject lumi_hc")
    safe_hc_list = json.dumps(
        {"files": [{"commands": [{"index": 0, "name": "hcHealth"}, {"index": 1, "name": "hcGetEntities"}]}]}
    )
    if helper.validate_hc_preflight_commands(safe_hc_list) != (True, ""):
        errors.append(f"{HELPER_SCRIPT}: read-only HC preflight should be accepted")
    missing_health = json.dumps({"files": [{"commands": [{"index": 0, "name": "hcGetEntities"}]}]})
    if helper.validate_hc_preflight_commands(missing_health)[0]:
        errors.append(f"{HELPER_SCRIPT}: HC preflight should require hcHealth")
    mutating_hc = json.dumps({"files": [{"commands": [{"index": 0, "name": "hcHealth"}, {"index": 1, "name": "hcControl"}]}]})
    if helper.validate_hc_preflight_commands(mutating_hc)[0]:
        errors.append(f"{HELPER_SCRIPT}: HC preflight should reject hcControl")
    if helper.parse_passthrough(["run", "tests/smoke.yaml", "--platform", "web"]) != (
        "run",
        ["tests/smoke.yaml", "--platform", "web"],
    ):
        errors.append(f"{HELPER_SCRIPT}: raw run passthrough should preserve extra args")
    if helper.parse_passthrough(["system", "install", "--all"]) != (
        "system",
        ["install", "--all"],
    ):
        errors.append(f"{HELPER_SCRIPT}: raw system passthrough should preserve extra args")
    if helper.parse_passthrough(["shell", "--platform", "macos"]) != (
        "shell",
        ["--platform", "macos"],
    ):
        errors.append(f"{HELPER_SCRIPT}: raw shell passthrough should preserve extra args")
    return errors


def validate_skill_preflight() -> list[str]:
    errors: list[str] = []
    raw_text = SKILL_MD.read_text(encoding="utf-8")
    section = markdown_section(raw_text, "Preflight Before Running").lower()
    if not section:
        return [f"{SKILL_MD}: missing section: Preflight Before Running"]
    required_terms = {
        "doctor --platform": "platform doctor preflight",
        "validate <file-or-folder> --json": "YAML validation preflight",
        "valid: false": "validation failure stop rule",
        "list <file-or-folder> --json": "list/index preflight",
        "setup/hooks": "group setup/hook check",
        "skipped subflows": "subflow collection check",
        "folder/group": "group run guidance",
        "leaf file": "leaf-file warning",
        "--report --snapshot --events-jsonl --output": "debug artifact flags",
    }
    for term, label in required_terms.items():
        if term not in section:
            errors.append(f"{SKILL_MD}: preflight missing {label}")
    return errors


def validate_agent_self_test_contract() -> list[str]:
    errors: list[str] = []
    raw_text = SKILL_MD.read_text(encoding="utf-8")
    section = markdown_section(raw_text, "Agent Self-Test Contract").lower()
    if not section:
        return [f"{SKILL_MD}: missing section: Agent Self-Test Contract"]
    required_terms = {
        "agent-check <file-or-folder>": "agent-check authoring shortcut",
        "agent-check <file-or-folder> --platform <platform> --run --output <dir>": "agent-check runtime shortcut",
        "== lumi agent-check: pass ==": "agent-check summary marker",
        "--summary-json <file>": "agent-check machine-readable summary",
        "exact lumi command string": "agent-check command evidence",
        "runtimestatus": "agent-check runtime status evidence",
        "status: blocked": "agent-check blocked status evidence",
        "validate --json": "validation evidence",
        "list --json": "collection/index evidence",
        "setup/teardown": "group setup evidence",
        "--report --snapshot --events-jsonl --output": "runtime artifact flags",
        "doctor --platform <platform>": "blocked runtime doctor evidence",
        "--json": "machine-readable blocked runtime evidence",
        "do not claim": "no false runtime pass rule",
        "runtime pass": "runtime pass wording",
        "--command-index": "targeted rerun evidence",
        "exact validation/list/run commands": "final evidence reporting",
    }
    for term, label in required_terms.items():
        if term not in section:
            errors.append(f"{SKILL_MD}: self-test contract missing {label}")
    return errors


def validate_skill_app_identity_guidance() -> list[str]:
    errors: list[str] = []
    text = SKILL_MD.read_text(encoding="utf-8")
    required_terms = {
        "Android uses package name": "Android appId identity",
        "iOS uses bundle id": "iOS appId identity",
        "Web uses `url`": "Web URL identity",
        "macOS uses a `.app` path or bundle id": "macOS app identity",
        "Windows uses an executable path": "Windows app identity",
        "mdls -name kMDItemCFBundleIdentifier": "macOS bundle id discovery command",
        "Get-Item 'C:\\Program Files\\Example\\Example.exe'": "Windows executable discovery command",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{SKILL_MD}: missing app identity guidance: {label}")
    return errors


def mcp_tool_names() -> set[str]:
    text = MCP_SERVER_JS.read_text(encoding="utf-8")
    return set(re.findall(r"server\.registerTool\(\s*\n\s*\"([^\"]+)\"", text))


def mcp_tool_block(text: str, tool: str) -> str:
    pattern = rf"server\.registerTool\(\s*\n\s*\"{re.escape(tool)}\"(.*?)(?=\nserver\.registerTool\(|\nconst transport =)"
    match = re.search(pattern, text, flags=re.DOTALL)
    return match.group(1) if match else ""


def validate_mcp_tool_references() -> list[str]:
    errors: list[str] = []
    tools = mcp_tool_names()
    skill_text = SKILL_MD.read_text(encoding="utf-8")
    readme_text = MCP_README.read_text(encoding="utf-8")
    for tool in sorted(tools):
        if f"`{tool}`" not in skill_text:
            errors.append(f"{SKILL_MD}: missing MCP tool reference: {tool}")
        if f"`{tool}`" not in readme_text:
            errors.append(f"{MCP_README}: missing MCP tool reference: {tool}")

    server_text = MCP_SERVER_JS.read_text(encoding="utf-8")
    for tool in ("doctor", "run_test"):
        block = mcp_tool_block(server_text, tool)
        if not block:
            errors.append(f"{MCP_SERVER_JS}: missing MCP tool block: {tool}")
            continue
        for platform in ("macos", "windows"):
            if f'"{platform}"' not in block:
                errors.append(
                    f"{MCP_SERVER_JS}: MCP tool {tool} should accept platform: {platform}"
                )

    required_readme_terms = {
        "agent workflow": "MCP agent workflow",
        "android_auto": "Android Auto platform guidance",
        "macos": "macOS platform guidance",
        "windows": "Windows platform guidance",
        "run_test` supports": "run_test platform support",
        "report, snapshot, and `events.jsonl` by default": "debug artifact defaults",
        "`read_report`": "read_report debug step",
        "`read_events`": "read_events debug step",
        "`read_artifact`": "read_artifact debug step",
        "`suggest_selectors`": "selector suggestion debug step",
        "native desktop tests must run on the local desktop host": "desktop host limitation",
    }
    lower_readme = readme_text.lower()
    for term, label in required_readme_terms.items():
        if term.lower() not in lower_readme:
            errors.append(f"{MCP_README}: missing {label}")
    return errors


def validate_user_install_docs() -> list[str]:
    errors: list[str] = []
    docs = {
        ROOT_README_MD: ROOT_README_MD.read_text(encoding="utf-8").lower(),
        README_MD: README_MD.read_text(encoding="utf-8").lower(),
        DISTRIBUTION_MD: DISTRIBUTION_MD.read_text(encoding="utf-8").lower(),
    }
    required_terms = {
        "lumi-tester ai install": "AI installer command",
        "install-ai.sh": "Unix AI one-line installer",
        "install-ai.ps1": "Windows AI one-line installer",
        "doctor --platform android --json": "Android quick check",
        "doctor --platform android_auto --json": "Android Auto quick check",
        "doctor --platform ios --json": "iOS quick check",
        "doctor --platform web --json": "Web quick check",
        "doctor --platform macos --json": "macOS quick check",
        "doctor --platform windows --json": "Windows quick check",
        "codex skill": "Codex skill install explanation",
        "mcp": "MCP install explanation",
    }
    for path, text in docs.items():
        for term, label in required_terms.items():
            if term not in text:
                errors.append(f"{path}: missing {label}")

    readme = docs[README_MD]
    for platform in ("android", "android auto", "ios", "web", "macos", "windows"):
        if platform not in readme:
            errors.append(f"{README_MD}: missing platform mention: {platform}")
    return errors


def validate_ai_authoring_contract() -> list[str]:
    errors: list[str] = []
    text = AI_AUTHORING_MD.read_text(encoding="utf-8").lower()
    required_terms = {
        "doctor --platform <platform> --json": "explicit platform doctor loop",
        "run ./test.yaml --platform <platform>": "explicit platform run loop",
        "doctor --platform all --json": "environment audit guidance",
        "output/run.json": "run artifact guidance",
        "output/test-results.json": "report artifact guidance",
        "list --json": "index confirmation guidance",
        "--command-index <index>": "targeted rerun guidance",
        "do not count yaml commands": "manual index counting warning",
        "rerun the whole flow": "full flow confirmation",
        "references/debug-artifacts.md": "debug artifact reference",
        "wrong target": "failure classification",
        "setup/state": "failure classification",
        "app/runtime": "failure classification",
        "selector": "failure classification",
        "platform: android_auto": "Android Auto platform identity",
        "platform: macos": "macOS platform identity",
        "platform: windows": "Windows platform identity",
        "dhu": "Android Auto DHU guidance",
        "`.app` path or bundle id": "macOS app identity guidance",
        "executable path": "Windows app identity guidance",
        "desktopstate.clear": "desktop clearState header guidance",
        "clearstate: true": "state reset launch guidance",
        "clearappdata": "desktop clearAppData warning",
        "mode: autosafe": "desktop autoSafe guidance",
        "mode: manual": "desktop manual guidance",
        "keychain services": "macOS Keychain reset guidance",
        "hkcu:\\software": "Windows registry reset guidance",
        "grouped suite": "stateful suite grouping guidance",
        "after `launchapp`, wait for a stable screen element": "launch readiness wait rule",
        "do not use a fixed `wait` as launch readiness": "fixed wait launch warning",
        "permissions: { all: allow }": "blanket permission warning",
        "separate allow/deny cases": "permission behavior split guidance",
        "validate/list/run the folder or group": "group run guidance",
        "setup.yaml": "setup file group guidance",
        "tests/generated/<feature>/": "generated folder convention",
        "references/selectors.csv": "selector CSV guidance",
        "references/selector-discovery.md": "selector discovery playbook guidance",
        "references/headers.csv": "header CSV guidance",
        "~/.codex/skills/lumi-tester-agent/references/": "installed skill reference path",
        "~/.codex/skills/lumi-tester-agent/references/headers.csv": (
            "installed header CSV path"
        ),
        "inspector_get /api/hierarchy": "Inspector hierarchy guidance",
        "inspector_get /api/element-at": "Inspector element-at guidance",
        "uihierarchypath": "UI hierarchy artifact guidance",
        "suggest_selectors": "MCP selector suggestion guidance",
        "debug launch/crash/wrong target": "wrong target before selector tuning guidance",
        "~/.codex/skills/lumi-tester-agent/references/debug-artifacts.md": (
            "installed debug-artifacts path"
        ),
    }
    for platform in REQUIRED_AGENT_PLATFORMS:
        required_terms[f"doctor --platform {platform} --json"] = (
            f"{platform} doctor command"
        )
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{AI_AUTHORING_MD}: missing {label}")
    stale_terms = [
        "`doctor --json` defaults to android",
        "for other targets",
        "--platform android --report",
    ]
    for term in stale_terms:
        if term in text:
            errors.append(f"{AI_AUTHORING_MD}: stale platform guidance still present: {term}")
    return errors


def validate_package_manager_ai_guidance() -> list[str]:
    errors: list[str] = []
    text = PACKAGE_MANIFEST_SCRIPT.read_text(encoding="utf-8")
    required_terms = {
        "lumi-tester ai install": "AI installer command",
        "Codex skill": "Codex skill guidance",
        "MCP server": "MCP server guidance",
        "def caveats": "Homebrew caveats",
        '"notes"': "Scoop notes",
        "Description:": "Winget description",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{PACKAGE_MANIFEST_SCRIPT}: missing {label}")
    return errors


def validate_ai_installer_skill_fallback() -> list[str]:
    errors: list[str] = []
    sources = {
        AI_RS: AI_RS.read_text(encoding="utf-8"),
        INSTALL_AI_SH: INSTALL_AI_SH.read_text(encoding="utf-8"),
        INSTALL_AI_PS1: INSTALL_AI_PS1.read_text(encoding="utf-8"),
    }
    for path, text in sources.items():
        for term in ("falling back", "SKILL.md", "references/cli.csv"):
            if term not in text:
                errors.append(f"{path}: missing AI skill install fallback term: {term}")

    if "resolve_skill_base_url" not in sources[AI_RS] or "url_exists" not in sources[AI_RS]:
        errors.append(f"{AI_RS}: Rust AI installer should preflight skill file URLs")
    if "url_exists" not in sources[INSTALL_AI_SH]:
        errors.append(f"{INSTALL_AI_SH}: shell AI installer should preflight skill file URLs")
    if "Test-UrlExists" not in sources[INSTALL_AI_PS1]:
        errors.append(f"{INSTALL_AI_PS1}: PowerShell AI installer should preflight skill file URLs")
    for path, text in sources.items():
        for platform in REQUIRED_AGENT_PLATFORMS:
            if f"doctor --platform {platform} --json" not in text:
                errors.append(f"{path}: AI installer quick checks missing platform: {platform}")
        if "lumi_agent.py agent-schema" not in text:
            errors.append(f"{path}: AI installer quick checks missing agent-schema")
        if "lumi_agent.py agent-check" not in text:
            errors.append(f"{path}: AI installer quick checks missing agent-check")
        if "--summary-json ./output/agent-check.json" not in text:
            errors.append(f"{path}: AI installer quick checks missing agent-check summary JSON")
    return errors


def markdown_section(text: str, heading: str) -> str:
    pattern = rf"^## {re.escape(heading)}\s*$"
    match = re.search(pattern, text, flags=re.MULTILINE)
    if not match:
        return ""
    next_heading = re.search(r"^##\s+", text[match.end() :], flags=re.MULTILINE)
    if next_heading:
        return text[match.end() : match.end() + next_heading.start()]
    return text[match.end() :]


def csv_header_line(text: str) -> str:
    match = re.search(r"```csv\n(.*?)```", text, flags=re.DOTALL)
    if not match:
        return ""
    return match.group(1).strip().splitlines()[0].strip().lower()


def normalize_posix_path(path: str) -> str:
    parts: list[str] = []
    for part in PurePosixPath(path).parts:
        if part in {"", "."}:
            continue
        if part == "..":
            if parts:
                parts.pop()
            continue
        parts.append(part)
    return "/".join(parts)


def labeled_yaml_examples(section: str) -> list[tuple[str, str]]:
    examples: list[tuple[str, str]] = []
    pattern = re.compile(r"`([^`]+\.ya?ml)`:\s*\n\s*```ya?ml\n(.*?)```", re.DOTALL)
    for match in pattern.finditer(section):
        examples.append((match.group(1), match.group(2)))
    return examples


def run_flow_paths(body: str) -> list[str]:
    paths: list[str] = []
    for line in body.splitlines():
        match = re.match(r"^\s*-\s+runFlow:\s*[\"']?([^\"'\s]+)[\"']?\s*$", line)
        if match:
            paths.append(match.group(1))
    return paths


def validate_testcase_design_reference() -> list[str]:
    errors: list[str] = []
    raw_text = TESTCASE_DESIGN_MD.read_text(encoding="utf-8")
    text = raw_text.lower()
    required_terms = {
        "equivalence partitioning": "equivalence partitioning",
        "boundary value": "boundary value analysis",
        "decision tables": "decision tables",
        "state transition": "state transition testing",
        "pairwise": "pairwise/combinatorial testing",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{TESTCASE_DESIGN_MD}: missing {label}")

    section_requirements = {
        "Research Inputs": {
            "product requirements": "product artifact research",
            "runtime exploration": "runtime exploration",
            "ui xml": "UI hierarchy research",
            "dom": "DOM research",
            "platform contracts": "platform contract research",
        },
        "Coverage Model": {
            "actors/roles": "actors/roles model",
            "entry points": "entry points model",
            "states": "states model",
            "objects/data": "objects/data model",
            "operations": "operations model",
            "oracles": "oracles model",
        },
        "App And Web Coverage Checklist": {
            "web-specific": "web-specific coverage",
            "mobile-specific": "mobile-specific coverage",
            "permissions and privacy": "permissions coverage",
            "state and data": "state/data coverage",
            "security-focused web/api smoke": "security coverage",
        },
        "Stop Conditions": {
            "every requirement": "requirement traceability stop condition",
            "high-risk": "high-risk stop condition",
            "boundary": "boundary stop condition",
            "permission": "permission stop condition",
            "reports/artifacts": "debug artifact stop condition",
        },
        "Generated Suite Example": {
            "setup.yaml": "root setup example",
            "subflows/login.yaml": "login subflow example",
            "001_toggle_notifications.yaml": "leaf regression example",
            'env: { file: ".env" }': "credential env file loading",
            "user_password=replace-with-secret": "credential env file example",
            "lumi-tester validate tests/generated": "folder validation command",
            "lumi-tester run tests/generated": "folder run command",
            "not the leaf file": "folder run warning",
        },
    }
    for heading, terms in section_requirements.items():
        section = markdown_section(raw_text, heading).lower()
        if not section:
            errors.append(f"{TESTCASE_DESIGN_MD}: missing section: {heading}")
            continue
        for term, label in terms.items():
            if term not in section:
                errors.append(f"{TESTCASE_DESIGN_MD}: missing {label}")

    header = csv_header_line(raw_text)
    for column in ("source", "entry_point"):
        if column not in {part.strip() for part in header.split(",")}:
            errors.append(f"{TESTCASE_DESIGN_MD}: cases.csv is missing {column} column")

    suite_section = markdown_section(raw_text, "Generated Suite Example")
    suite_examples = labeled_yaml_examples(suite_section)
    declared_paths = {normalize_posix_path(path) for path, _ in suite_examples}
    if len(suite_examples) < 3:
        errors.append(f"{TESTCASE_DESIGN_MD}: generated suite should include at least 3 YAML files")
    for file_path, body in suite_examples:
        base_dir = PurePosixPath(file_path).parent
        for run_flow in run_flow_paths(body):
            resolved = normalize_posix_path(str(base_dir / run_flow))
            if resolved not in declared_paths:
                errors.append(
                    f"{TESTCASE_DESIGN_MD}: {file_path} runFlow target is not "
                    f"declared in generated suite example: {run_flow}"
                )
    return errors


def validate_debug_artifacts_reference() -> list[str]:
    errors: list[str] = []
    raw_text = DEBUG_ARTIFACTS_MD.read_text(encoding="utf-8")
    text = raw_text.lower()
    skill_text = SKILL_MD.read_text(encoding="utf-8").lower()
    debug_loop = markdown_section(SKILL_MD.read_text(encoding="utf-8"), "Debugging Loop").lower()
    if "references/debug-artifacts.md" not in skill_text:
        errors.append(f"{SKILL_MD}: missing debug artifacts reference")
    if "references/debug-artifacts.md" not in debug_loop:
        errors.append(f"{SKILL_MD}: debug loop should point to debug-artifacts.md")
    for term in ("wrong app", "crash", "permission", "platform-specific"):
        if term not in debug_loop:
            errors.append(f"{SKILL_MD}: debug loop should mention {term} failures")

    required_terms = {
        "run.json": "run summary artifact",
        "test-results.json": "report artifact",
        "events.jsonl": "event stream artifact",
        "commandfailed.index": "failed command rerun guidance",
        "extract failure data": "failure extraction section",
        "screenshotpath": "screenshot artifact path extraction",
        "uihierarchypath": "UI hierarchy artifact path extraction",
        "logpath": "log artifact path extraction",
        "jq '.commandresults[]?": "run.json jq extraction",
        "do not infer indexes from the yaml by hand": "manual command index warning",
        "wrong app": "wrong target diagnosis",
        "element not found": "selector failure diagnosis",
        "assertion timeout": "assertion timeout diagnosis",
        "runtime dependency failure": "runtime dependency diagnosis",
        "app launch/crash/abort": "launch/crash diagnosis",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{DEBUG_ARTIFACTS_MD}: missing {label}")

    section = markdown_section(raw_text, "Common Failure Diagnosis").lower()
    if not section:
        errors.append(f"{DEBUG_ARTIFACTS_MD}: missing section: Common Failure Diagnosis")
        return errors

    platform_patterns = {
        "android": {
            "focus": r"mcurrentfocus|mfocusedapp|topresumed",
            "process": r"\bpidof\b|process",
            "logs": r"\blogcat\b",
        },
        "ios": {
            "bundle": r"simctl listapps|bundle id",
            "hierarchy": r"accessibility (?:hierarchy|tree)",
            "permission": r"permission (?:alert|dialog|state)",
        },
        "web": {
            "url": r"(?:actual|resolved|browser|page) (?:page )?url|\burl\b",
            "console": r"console (?:errors?|logs?)|pageerror",
            "network": r"(?:failed )?network requests?|http failures?|4\[0-9\]\{2\}|5\[0-9\]\{2\}",
        },
        "macos": {
            "frontmost": r"frontmost app|frontmost is true|applescript",
            "permissions": r"accessibility|screen recording",
            "logs": r"unified logs?|\blog show\b",
        },
        "windows": {
            "foreground": r"foreground window|mainwindowtitle|interactive desktop",
            "hierarchy": r"ui automation hierarchy",
            "powershell": r"powershell errors?|powershell",
        },
    }
    for platform, patterns in platform_patterns.items():
        for label, pattern in patterns.items():
            if not re.search(pattern, section):
                errors.append(f"{DEBUG_ARTIFACTS_MD}: missing {platform} debug signal: {label}")

    classifications = (
        "wrong target",
        "setup/state issue",
        "app/runtime issue",
        "selector issue",
    )
    for label in classifications:
        if label not in section:
            errors.append(f"{DEBUG_ARTIFACTS_MD}: missing failure classification: {label}")
    return errors


def validate_patterns_reference() -> list[str]:
    errors: list[str] = []
    raw_text = PATTERNS_MD.read_text(encoding="utf-8")
    text = raw_text.lower()
    skill_text = SKILL_MD.read_text(encoding="utf-8").lower()
    if "references/patterns.md" not in skill_text:
        errors.append(f"{SKILL_MD}: missing flow patterns reference")

    section_requirements = {
        "Current Android App Smoke": {
            "adb devices -l": "device enumeration",
            "mcurrentfocus": "current focus discovery",
            "uiautomator dump": "UI XML discovery",
            "appId": "discovered app id",
            "do not use\n  coordinates": "coordinate avoidance",
            "accessibilityId": "semantic Android selector",
        },
        "Login": {
            "env: { file: \".env\" }": "sensitive credential guidance",
            "USER_PASSWORD": "credential variable example",
            "out of committed YAML": "credential secrecy warning",
            "hideKeyboard": "keyboard handling",
            "waitUntilVisible": "login readiness wait",
        },
        "Permission Dialog": {
            "permission": "permission workflow",
            "os version": "OS-specific prompt warning",
            "runFlow": "reusable permission flow",
        },
        "GPS Route": {
            "mockLocation": "GPS route start",
            "waitForLocation": "GPS assertion wait",
            "stopMockLocation": "GPS cleanup",
        },
        "Web Form": {
            "platform: web": "web platform header",
            "browser: chromium": "browser target",
            "css": "web CSS selector",
            "role": "web role selector",
        },
        "Failure Recovery Pattern": {
            "list ./flow.yaml --json": "command index discovery",
            "--command-index": "smallest failing command rerun",
            "--snapshot": "snapshot artifact",
            "--events-jsonl": "event artifact",
            "suggest_selectors": "selector suggestion workflow",
            "rerun the whole flow": "full flow confirmation",
        },
    }
    for heading, terms in section_requirements.items():
        section = markdown_section(raw_text, heading)
        if not section:
            errors.append(f"{PATTERNS_MD}: missing section: {heading}")
            continue
        normalized_section = section.lower()
        for term, label in terms.items():
            if term.lower() not in normalized_section:
                errors.append(f"{PATTERNS_MD}: missing {label} in {heading}")

    required_global_terms = {
        "replace selectors with discovered": "selector discovery prerequisite",
        "validate before running": "validation prerequisite",
        "do not reuse a nearby\n  yaml file": "current app package guard",
        "coordinates": "coordinate warning",
        "clearState: true": "first-run state reset example",
        "runFlow": "reusable setup flow guidance",
    }
    for term, label in required_global_terms.items():
        if term.lower() not in text:
            errors.append(f"{PATTERNS_MD}: missing {label}")
    return errors


def validate_desktop_reference() -> list[str]:
    errors: list[str] = []
    raw_text = DESKTOP_MD.read_text(encoding="utf-8")
    text = raw_text.lower()
    desktop_testing_text = DESKTOP_TESTING_MD.read_text(encoding="utf-8").lower()
    skill_text = SKILL_MD.read_text(encoding="utf-8").lower()
    if "references/desktop.md" not in skill_text:
        errors.append(f"{SKILL_MD}: missing desktop reference")
    for term in ("macos", "windows"):
        if term not in skill_text:
            errors.append(f"{SKILL_MD}: platform coverage should mention {term}")

    required_terms = {
        "platform: macos": "macOS YAML example",
        "platform: windows": "Windows YAML example",
        "accessibility permission": "macOS Accessibility permission guidance",
        "screen recording": "macOS Screen Recording permission guidance",
        "interactive foreground desktop session": "Windows interactive desktop guidance",
        "ui automation": "Windows UI Automation guidance",
        "desktopstate": "desktop state reset guidance",
        "clearstate: true": "desktop clearState example",
        "mode: autosafe": "desktop autoSafe clear mode",
        "mode: manual": "desktop manual clear mode",
        "keychainservices": "macOS Keychain clearing guidance",
        "registrykeys": "Windows registry clearing guidance",
        "hklm": "Windows unsafe registry warning",
        "doctor --platform macos": "macOS doctor command",
        "doctor --platform windows": "Windows doctor command",
        "events-jsonl": "debug artifact flag",
        "point": "desktop point fallback",
        "not implemented for macos/windows desktop drivers": "desktop OCR/image runtime limit",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{DESKTOP_MD}: missing {label}")

    for path, source in {
        DESKTOP_MD: raw_text,
        DESKTOP_TESTING_MD: DESKTOP_TESTING_MD.read_text(encoding="utf-8"),
    }.items():
        macos_section = markdown_section(source, "macOS Flow")
        if not macos_section:
            macos_section = markdown_section(source, "macOS")
        if "point:" in macos_section:
            errors.append(
                f"{path}: macOS example should not teach point-first desktop flows"
            )
        for term in ("setClipboard", "assertClipboard"):
            if term.lower() not in macos_section.lower():
                errors.append(f"{path}: macOS example should prefer {term} over point tap")

    desktop_testing_required = {
        "prefer native desktop selectors": "desktop selector priority guidance",
        "documented fallback": "desktop point fallback framing",
        "setclipboard": "semantic/key/clipboard-first macOS example",
        "assertclipboard": "clipboard assertion macOS example",
    }
    for term, label in desktop_testing_required.items():
        if term not in desktop_testing_text:
            errors.append(f"{DESKTOP_TESTING_MD}: missing {label}")
    return errors


def validate_android_auto_reference() -> list[str]:
    errors: list[str] = []
    text = ANDROID_AUTO_MD.read_text(encoding="utf-8").lower()
    skill_text = SKILL_MD.read_text(encoding="utf-8").lower()
    if "references/android-auto.md" not in skill_text:
        errors.append(f"{SKILL_MD}: missing Android Auto reference")
    required_terms = {
        "platform: android_auto": "Android Auto platform header",
        "desktop head unit": "DHU explanation",
        "point-only": "point-only interaction guidance",
        "no ui hierarchy": "UI hierarchy limitation",
        "waituntilvisible": "selector wait warning",
        "doctor --platform android_auto --json": "Android Auto doctor command",
        "devices --platform android": "Android device discovery",
        "--device <serial>": "device serial guidance",
        "press: navigation": "DHU key example",
        "notsee": "unsupported assertion warning",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{ANDROID_AUTO_MD}: missing {label}")

    for term in ("android auto", "android_auto"):
        if term not in skill_text:
            errors.append(f"{SKILL_MD}: platform coverage should mention {term}")

    with CLI_CSV.open(newline="", encoding="utf-8") as fh:
        cli_rows = {row["command"].strip(): row for row in csv.DictReader(fh)}
    for command in (
        "validate",
        "list",
        "schema",
        "doctor",
        "devices",
        "run",
        "report",
        "ai",
        "system",
    ):
        platforms = split_field_names(cli_rows[command]["platforms"])
        if "android_auto" not in platforms:
            errors.append(f"{CLI_CSV}: command {command} is missing android_auto")

    command_rows = command_catalog_rows()
    supported = {
        "launchApp",
        "stopApp",
        "back",
        "pressHome",
        "hideKeyboard",
        "openLink",
        "tapOn",
        "doubleTapOn",
        "swipeLeft",
        "swipeRight",
        "swipeUp",
        "swipeDown",
        "swipe",
        "press",
        "selectDisplay",
    }
    for command in supported:
        platforms = split_field_names(command_rows[command]["platforms"])
        if "android_auto" not in platforms:
            errors.append(f"{COMMANDS_CSV}: command {command} is missing android_auto")

    unsupported = {
        "waitUntilVisible",
        "waitUntilNotVisible",
        "assertVisible",
        "assertNotVisible",
        "inputText",
        "longPressOn",
        "rightClick",
        "scrollUntilVisible",
    }
    for command in unsupported:
        platforms = split_field_names(command_rows[command]["platforms"])
        if "android_auto" in platforms:
            errors.append(
                f"{COMMANDS_CSV}: command {command} should not list android_auto"
            )
    return errors


def validate_cli_platform_catalog() -> list[str]:
    errors: list[str] = []
    with CLI_CSV.open(newline="", encoding="utf-8") as fh:
        rows = {row["command"].strip(): row for row in csv.DictReader(fh)}

    platform_agnostic_commands = {"validate", "list", "schema", "report"}
    for command in platform_agnostic_commands:
        platforms = split_field_names(rows[command]["platforms"])
        missing = sorted(REQUIRED_AGENT_RUN_PLATFORMS.difference(platforms))
        if missing:
            errors.append(
                f"{CLI_CSV}: command {command} should list every agent platform: "
                + ", ".join(missing)
            )

    run_platforms = split_field_names(rows["run"]["platforms"])
    missing_run = sorted(REQUIRED_AGENT_RUN_PLATFORMS.difference(run_platforms))
    if missing_run:
        errors.append(
            f"{CLI_CSV}: command run should list every execution platform: "
            + ", ".join(missing_run)
        )

    shell_platforms = split_field_names(rows["shell"]["platforms"])
    for unsupported in ("android_auto", "web"):
        if unsupported in shell_platforms:
            errors.append(
                f"{CLI_CSV}: shell lists unsupported platform: {unsupported}"
            )
    if "current CLI shell does not support web or android_auto" not in rows["shell"]["notes"]:
        errors.append(f"{CLI_CSV}: shell notes should explain unsupported web/android_auto")
    return errors


def validate_cli_help_platform_guidance() -> list[str]:
    errors: list[str] = []
    text = MAIN_RS.read_text(encoding="utf-8")
    stale_patterns = {
        "run platform help": r"Target platform \(android, ios, web\).*parsed from file",
        "doctor platform help": r"Target platform to check \(android, ios, web, macos, windows, all\)",
    }
    for label, pattern in stale_patterns.items():
        if re.search(pattern, text, flags=re.DOTALL | re.IGNORECASE):
            errors.append(f"{MAIN_RS}: stale {label} is missing android_auto/desktop coverage")

    required_terms = {
        "Target platform (android, android_auto, ios, web, macos, windows).": "run platform help",
        "Target platform to check (android, android_auto, ios, web, macos, windows, all)": "doctor platform help",
    }
    for term, label in required_terms.items():
        if term not in text:
            errors.append(f"{MAIN_RS}: missing {label}")
    return errors


def validate_desktop_platform_catalog() -> list[str]:
    errors: list[str] = []
    schema = json.loads(SCHEMA_JSON.read_text(encoding="utf-8"))
    schema_platforms = set(schema["properties"]["platform"]["enum"])
    for platform in ("macos", "windows"):
        if platform not in schema_platforms:
            errors.append(f"{SCHEMA_JSON}: missing desktop platform: {platform}")
    if "desktopState" not in schema["properties"]:
        errors.append(f"{SCHEMA_JSON}: missing desktopState header property")
    desktop_state = schema["$defs"].get("desktopState", {})
    desktop_clear = schema["$defs"].get("desktopClear", {})
    if "clear" not in desktop_state.get("properties", {}):
        errors.append(f"{SCHEMA_JSON}: desktopState should define clear")
    clear_properties = desktop_clear.get("properties", {})
    for field in ("mode", "paths", "keychainServices", "registryKeys"):
        if field not in clear_properties:
            errors.append(f"{SCHEMA_JSON}: desktopClear should define {field}")
    modes = set(clear_properties.get("mode", {}).get("enum", []))
    if {"autoSafe", "manual"}.difference(modes):
        errors.append(f"{SCHEMA_JSON}: desktopClear.mode should allow autoSafe and manual")

    required_cli = {
        "validate",
        "list",
        "schema",
        "doctor",
        "devices",
        "run",
        "report",
        "ai",
        "shell",
        "system",
    }
    with CLI_CSV.open(newline="", encoding="utf-8") as fh:
        rows = {row["command"].strip(): row for row in csv.DictReader(fh)}
    for command in required_cli:
        platforms = split_field_names(rows[command]["platforms"])
        if "all" in platforms:
            continue
        missing = sorted({"macos", "windows"}.difference(platforms))
        if missing:
            errors.append(
                f"{CLI_CSV}: command {command} is missing desktop platform(s): "
                + ", ".join(missing)
            )

    required_yaml_commands = {
        "launchApp",
        "stopApp",
        "installApp",
        "backgroundApp",
        "back",
        "pressHome",
        "hideKeyboard",
        "openLink",
        "tapOn",
        "longPressOn",
        "doubleTapOn",
        "rightClick",
        "inputText",
        "eraseText",
        "swipe",
        "scrollUntilVisible",
        "assertVisible",
        "assertNotVisible",
        "waitUntilVisible",
        "waitUntilNotVisible",
        "extendedWaitUntil",
        "screenshot",
        "assertScreenshot",
        "assertColor",
        "press",
        "setClipboard",
        "getClipboard",
        "assertClipboard",
        "pasteText",
    }
    command_rows = command_catalog_rows()
    for command in required_yaml_commands:
        platforms = split_field_names(command_rows[command]["platforms"])
        if "all" in platforms:
            continue
        missing = sorted({"macos", "windows"}.difference(platforms))
        if missing:
            errors.append(
                f"{COMMANDS_CSV}: command {command} is missing desktop platform(s): "
                + ", ".join(missing)
            )
    return errors


def validate_desktop_clear_state_docs() -> list[str]:
    errors: list[str] = []
    agent_command_catalog = SKILL_DIR / "references" / "command-catalog.md"
    required = {
        SKILL_MD: [
            "macOS/Windows clear state requires header-level `desktopState.clear`",
            "do not\n  use Android-only `clearAppData` for desktop apps",
            "Read `references/desktop.md` for macOS/Windows app identity",
        ],
        WRITING_TESTS_MD: [
            "android_auto",
            "macos",
            "windows",
            "desktopState",
            "desktopState.clear",
            "clearState: true",
        ],
        COMMANDS_MD: [
            ".app",
            ".exe",
            "desktopState.clear",
            "Android. Không dùng lệnh này cho macOS/Windows",
        ],
        agent_command_catalog: [
            "macOS `.app` paths",
            "Windows executable paths",
            "desktopState.clear",
            "clearState: true",
        ],
    }
    stale_terms = {
        WRITING_TESTS_MD: [
            "Package name (Android) hoặc Bundle ID (iOS).",
            "`android`, `ios`, `web`.",
        ],
        COMMANDS_MD: [
            "Package name (Android) hoặc Bundle ID (iOS).",
            "seconds: 5 # Đưa vào nền 5 giây",
            "| `seconds`| - | Number | Số giây để ứng dụng ở trong nền. |",
        ],
        agent_command_catalog: [
            "Use `appId` for Android/iOS app tests",
        ],
    }
    for path, terms in required.items():
        text = path.read_text(encoding="utf-8")
        for term in terms:
            if term not in text:
                errors.append(f"{path}: missing desktop clearState/appId guidance: {term}")
        for term in stale_terms.get(path, []):
            if term in text:
                errors.append(f"{path}: stale desktop guidance still present: {term}")
    return errors


def validate_docs_index_content() -> list[str]:
    errors: list[str] = []
    html = DOCS_INDEX_HTML.read_text(encoding="utf-8")
    match = re.search(
        r"const docs = (\{.*?\});\n\s*const pageNames",
        html,
        flags=re.DOTALL,
    )
    if not match:
        return [f"{DOCS_INDEX_HTML}: could not find embedded docs block"]
    embedded = json.loads(match.group(1))
    expected = {
        "cli_reference": CLI_REFERENCE_MD.read_text(encoding="utf-8"),
        "commands": COMMANDS_MD.read_text(encoding="utf-8"),
        "automotive_testing": AUTOMOTIVE_TESTING_MD.read_text(encoding="utf-8"),
        "hardware_control": HARDWARE_CONTROL_MD.read_text(encoding="utf-8"),
        "camera_hardware": CAMERA_HARDWARE_MD.read_text(encoding="utf-8"),
        "desktop_testing": DESKTOP_TESTING_MD.read_text(encoding="utf-8"),
        "python_integration": PYTHON_INTEGRATION_MD.read_text(encoding="utf-8"),
        "flows": FLOWS_MD.read_text(encoding="utf-8"),
        "writing_tests": WRITING_TESTS_MD.read_text(encoding="utf-8"),
        "ai_authoring": AI_AUTHORING_MD.read_text(encoding="utf-8"),
    }
    expected_page_names = {
        "cli_reference": "CLI Commands & Options Reference",
        "commands": "YAML Commands Reference",
        "automotive_testing": "Android Auto & Apple CarPlay",
        "hardware_control": "Hardware Jig Control",
        "camera_hardware": "Camera & LED Testing",
        "desktop_testing": "Desktop Testing (macOS & Windows)",
        "python_integration": "Python Integration",
        "flows": "Test Flows",
        "writing_tests": "Writing Tests",
        "ai_authoring": "AI Authoring",
    }
    if set(embedded) != set(expected):
        missing = sorted(set(expected).difference(embedded))
        extra = sorted(set(embedded).difference(expected))
        if missing:
            errors.append(f"{DOCS_INDEX_HTML}: missing embedded docs: {', '.join(missing)}")
        if extra:
            errors.append(f"{DOCS_INDEX_HTML}: unknown embedded docs: {', '.join(extra)}")
        return errors
    for key, expected_text in expected.items():
        if embedded[key] != expected_text:
            errors.append(
                f"{DOCS_INDEX_HTML}: embedded {key} docs are stale; "
                "run lumi-tester/scripts/generate-docs-index.py"
            )
        if f"loadPage('{key}')" not in html:
            errors.append(f"{DOCS_INDEX_HTML}: missing nav item for {key}")

    page_names_match = re.search(
        r"const pageNames = (\{.*?\});\s*(?=function|let|var|const)",
        html,
        flags=re.DOTALL,
    )
    if not page_names_match:
        errors.append(f"{DOCS_INDEX_HTML}: could not find pageNames block")
        return errors
    page_names = json.loads(page_names_match.group(1))
    if page_names != expected_page_names:
        errors.append(
            f"{DOCS_INDEX_HTML}: pageNames are stale; "
            "run lumi-tester/scripts/generate-docs-index.py"
        )
    return errors


def validate_reference_examples() -> list[str]:
    stale_patterns = {
        "killApp command": r"\bkillApp\b",
        "runScript.file field": r"runScript:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+file:",
        "runScript.env field": r"runScript:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+env:",
        "runScript inline vars JS": r"runScript:\s*[\"']?vars\.",
        "screenshot.name field": r"screenshot:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+name:",
        "mockLocation latitude field": r"mockLocation:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+latitude:",
        "mockLocation longitude field": r"mockLocation:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+longitude:",
        "conditional.when field": r"conditional:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+when:",
        "conditional.commands field": r"conditional:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+commands:",
        "relative.anchor schema": r"relative:\s*\n[ \t]+anchor:",
        "relative.direction schema": r"relative:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+direction:",
        "relative.target schema": r"relative:\s*\n(?:[ \t]+[A-Za-z0-9_]+:.*\n)*[ \t]+target:",
        "setup hook per-file wording": r"setup\.ya?ml[^.\n]*(?:before each file|per file)",
        "teardown hook per-file wording": r"teardown\.ya?ml[^.\n]*(?:after each file|per file)",
    }
    errors: list[str] = []
    paths = [SKILL_MD, *sorted((SKILL_DIR / "references").glob("*"))]
    for path in paths:
        if path.suffix not in {".md", ".csv"}:
            continue
        text = path.read_text(encoding="utf-8")
        for label, pattern in stale_patterns.items():
            if re.search(pattern, text):
                errors.append(f"{path}: stale AI reference example: {label}")
    return errors


def yaml_fence_bodies(text: str) -> list[str]:
    return re.findall(r"```ya?ml\n(.*?)```", text, flags=re.DOTALL)


def command_names_in_yaml_example(body: str) -> set[str]:
    lines = body.splitlines()
    if "---" in [line.strip() for line in lines]:
        separator_index = next(i for i, line in enumerate(lines) if line.strip() == "---")
        lines = lines[separator_index + 1 :]
    elif not any(line.lstrip().startswith("- ") for line in lines):
        return set()

    names: set[str] = set()
    for line in lines:
        match = re.match(r"^\s*-\s+([A-Za-z][A-Za-z0-9_]*)\b", line)
        if match:
            names.add(match.group(1))
    return names


def validate_yaml_command_examples(parser_names: set[str]) -> list[str]:
    errors: list[str] = []
    paths = [SKILL_MD, *sorted((SKILL_DIR / "references").glob("*.md"))]
    for path in paths:
        text = path.read_text(encoding="utf-8")
        for idx, body in enumerate(yaml_fence_bodies(text), start=1):
            unknown = sorted(command_names_in_yaml_example(body).difference(parser_names))
            if unknown:
                errors.append(
                    f"{path}: YAML example block {idx} uses unknown command(s): "
                    + ", ".join(unknown)
                )

    for path in (COMMANDS_CSV, CLI_CSV):
        with path.open(newline="", encoding="utf-8") as fh:
            for row in csv.DictReader(fh):
                example = row.get("example", "")
                unknown = sorted(command_names_in_yaml_example(example).difference(parser_names))
                if unknown:
                    errors.append(
                        f"{path}: example for {row.get('command', '<unknown>')} "
                        f"uses unknown command(s): {', '.join(unknown)}"
                    )
    return errors


def selector_example_keys(example: str) -> set[str]:
    keys: set[str] = set()
    for match in re.finditer(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*:", example):
        prefix = example[: match.start()].rstrip()
        if not prefix or prefix[-1] in "{,;":
            keys.add(match.group(1))
    return keys


def top_level_map_keys(map_text: str) -> set[str]:
    keys: set[str] = set()
    depth = 0
    in_quote: str | None = None
    escape = False
    token_start = 0
    segments: list[str] = []
    for idx, char in enumerate(map_text):
        if in_quote:
            if escape:
                escape = False
            elif char == "\\":
                escape = True
            elif char == in_quote:
                in_quote = None
            continue
        if char in {"'", '"'}:
            in_quote = char
        elif char in "{[":
            depth += 1
        elif char in "}]":
            depth -= 1
        elif char == "," and depth == 0:
            segments.append(map_text[token_start:idx])
            token_start = idx + 1
    segments.append(map_text[token_start:])

    for segment in segments:
        match = re.match(r"\s*([A-Za-z_][A-Za-z0-9_]*)\s*:", segment)
        if match:
            keys.add(match.group(1))
    return keys


def command_example_param_keys(example: str) -> set[str]:
    match = re.search(r"-\s+[A-Za-z][A-Za-z0-9_]*\s*:\s*\{(.*)\}\s*$", example)
    if match:
        return top_level_map_keys(match.group(1))
    return set()


def command_entries_in_yaml_example(body: str) -> list[tuple[str, set[str]]]:
    lines = body.splitlines()
    if "---" in [line.strip() for line in lines]:
        separator_index = next(i for i, line in enumerate(lines) if line.strip() == "---")
        lines = lines[separator_index + 1 :]
    elif not any(line.lstrip().startswith("- ") for line in lines):
        return []

    entries: list[tuple[str, set[str]]] = []
    idx = 0
    while idx < len(lines):
        line = lines[idx]
        inline = re.match(r"^(\s*)-\s+([A-Za-z][A-Za-z0-9_]*)\s*:\s*\{(.*)\}\s*$", line)
        if inline:
            entries.append((inline.group(2), top_level_map_keys(inline.group(3))))
            idx += 1
            continue

        scalar = re.match(r"^(\s*)-\s+([A-Za-z][A-Za-z0-9_]*)\s*:\s+\S.*$", line)
        if scalar:
            entries.append((scalar.group(2), set()))
            idx += 1
            continue

        block = re.match(r"^(\s*)-\s+([A-Za-z][A-Za-z0-9_]*)\s*:\s*$", line)
        if not block:
            idx += 1
            continue

        base_indent = len(block.group(1))
        child_lines: list[str] = []
        idx += 1
        while idx < len(lines):
            next_line = lines[idx]
            next_indent = len(next_line) - len(next_line.lstrip(" "))
            if next_line.lstrip().startswith("- ") and next_indent <= base_indent:
                break
            child_lines.append(next_line)
            idx += 1

        key_matches: list[tuple[int, str]] = []
        for child in child_lines:
            match = re.match(r"^(\s+)([A-Za-z_][A-Za-z0-9_]*)\s*:", child)
            if match:
                key_matches.append((len(match.group(1)), match.group(2)))
        if not key_matches:
            entries.append((block.group(2), set()))
            continue
        top_indent = min(indent for indent, _ in key_matches)
        entries.append(
            (block.group(2), {key for indent, key in key_matches if indent == top_indent})
        )
    return entries


def allowed_command_fields(row: dict[str, str], selector_names: set[str]) -> set[str]:
    allowed = split_field_names(row["required_fields"]).union(
        split_field_names(row["common_fields"])
    )
    if row["selector_supported"].strip().lower() in {"yes", "partial"}:
        allowed.update(selector_names)
    return allowed


def validate_command_example_fields() -> list[str]:
    errors: list[str] = []
    selector_names = schema_selector_names()
    rows = command_catalog_rows()
    with COMMANDS_CSV.open(newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh):
            example_keys = command_example_param_keys(row["example"])
            if not example_keys:
                continue
            allowed = allowed_command_fields(row, selector_names)
            unknown = sorted(example_keys.difference(allowed))
            if unknown:
                errors.append(
                    f"{COMMANDS_CSV}: example for {row['command']} has fields "
                    "not declared in required/common fields: " + ", ".join(unknown)
                )

    for path in [SKILL_MD, *sorted((SKILL_DIR / "references").glob("*.md"))]:
        text = path.read_text(encoding="utf-8")
        for idx, body in enumerate(yaml_fence_bodies(text), start=1):
            for command, example_keys in command_entries_in_yaml_example(body):
                if command not in rows or not example_keys:
                    continue
                allowed = allowed_command_fields(rows[command], selector_names)
                unknown = sorted(example_keys.difference(allowed))
                if unknown:
                    errors.append(
                        f"{path}: YAML example block {idx} command {command} has "
                        "fields not declared in commands.csv/schema: "
                        + ", ".join(unknown)
                    )
    return errors


def validate_selector_catalog() -> list[str]:
    errors: list[str] = []
    schema_names = schema_selector_names()
    example_names = schema_selector_example_names()
    with SELECTORS_CSV.open(newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh):
            names = [row["selector"].strip()]
            aliases = row.get("aliases", "")
            names.extend(alias.strip() for alias in re.split(r"[|,]", aliases) if alias.strip())
            unknown = sorted(set(names).difference(schema_names))
            if unknown:
                errors.append(
                    f"{SELECTORS_CSV}: selector row {row['selector']} has names "
                    "not present in schema selector properties: " + ", ".join(unknown)
                )
            example_keys = selector_example_keys(row["example"])
            unknown_example_keys = sorted(example_keys.difference(example_names))
            if unknown_example_keys:
                errors.append(
                    f"{SELECTORS_CSV}: selector row {row['selector']} example has "
                    "fields not present in schema selector properties: "
                    + ", ".join(unknown_example_keys)
                )
    return errors


def validate_selector_quality() -> list[str]:
    errors: list[str] = []
    rows: dict[str, dict[str, str]] = {}
    with SELECTORS_CSV.open(newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh):
            selector = row["selector"].strip()
            rows[selector] = row
            for column in ("when_to_use", "example", "anti_patterns"):
                if not row[column].strip():
                    errors.append(f"{SELECTORS_CSV}: selector {selector} has empty {column}")
            try:
                rank = int(row["stability_rank"])
            except ValueError:
                errors.append(f"{SELECTORS_CSV}: selector {selector} has non-numeric rank")
                continue
            if rank < 0 or rank > 9:
                errors.append(f"{SELECTORS_CSV}: selector {selector} rank must be 0..9")

    required = {"id", "accessibilityId", "text", "regex", "relative", "ocr", "image", "point"}
    missing = sorted(required.difference(rows))
    if missing:
        errors.append(f"{SELECTORS_CSV}: missing selector priority rows: " + ", ".join(missing))
        return errors

    ranks = {name: int(row["stability_rank"]) for name, row in rows.items()}
    if ranks["point"] <= ranks["ocr"]:
        errors.append(f"{SELECTORS_CSV}: point must rank below OCR fallback")
    if ranks["point"] <= ranks["image"]:
        errors.append(f"{SELECTORS_CSV}: point must rank below image fallback")
    if ranks["relative"] >= ranks["point"]:
        errors.append(f"{SELECTORS_CSV}: relative selectors must rank above point")
    for semantic in ("id", "accessibilityId"):
        if ranks[semantic] > 2:
            errors.append(f"{SELECTORS_CSV}: {semantic} should be a high-stability selector")
    if "coordinate" not in rows["point"]["anti_patterns"].lower():
        errors.append(f"{SELECTORS_CSV}: point anti-pattern should warn about coordinates")

    aliases = rows["accessibilityId"].get("aliases", "")
    for alias in ("desc", "contentDesc"):
        if alias not in aliases:
            errors.append(f"{SELECTORS_CSV}: accessibilityId aliases should include {alias}")

    discovery = (SKILL_DIR / "references" / "selector-discovery.md").read_text(
        encoding="utf-8"
    ).lower()
    normalized_discovery = re.sub(r"\s+", " ", discovery)
    for phrase in (
        "coordinates are allowed only",
        "do not immediately replace it with `point`",
        "do not start with `point`",
        "macos `.app` path",
        "windows executable path",
        "desktop selector examples",
        "accessibility/ui automation selectors",
    ):
        if phrase not in normalized_discovery:
            errors.append(f"selector-discovery.md: missing coordinate guard: {phrase}")
    return errors


def validate_desktop_selector_guidance() -> list[str]:
    errors: list[str] = []
    with SELECTORS_CSV.open(newline="", encoding="utf-8") as fh:
        rows = {row["selector"].strip(): row for row in csv.DictReader(fh)}

    for selector in ("text", "id", "accessibilityId", "role", "type", "description"):
        if selector not in rows:
            errors.append(f"{SELECTORS_CSV}: missing desktop semantic selector: {selector}")
            continue
        platforms = split_field_names(rows[selector]["platforms"])
        missing = sorted({"macos", "windows"}.difference(platforms))
        if missing:
            errors.append(
                f"{SELECTORS_CSV}: selector {selector} is missing desktop platform(s): "
                + ", ".join(missing)
            )

    for selector in ("ocr", "image"):
        if selector not in rows:
            continue
        platforms = split_field_names(rows[selector]["platforms"])
        unsupported = sorted({"macos", "windows"}.intersection(platforms))
        if unsupported:
            errors.append(
                f"{SELECTORS_CSV}: selector {selector} lists unsupported desktop platform(s): "
                + ", ".join(unsupported)
            )

    for path in (DESKTOP_MD, SKILL_DIR / "references" / "selector-discovery.md"):
        text = path.read_text(encoding="utf-8").lower()
        for phrase in (
            "not implemented for macos/windows desktop",
            "screenshot/pixel",
            "`point`",
        ):
            if phrase not in text:
                errors.append(f"{path}: missing desktop selector runtime limit: {phrase}")
    return errors


def validate_cli_catalog() -> list[str]:
    errors: list[str] = []
    source_names = cli_commands()
    with CLI_CSV.open(newline="", encoding="utf-8") as fh:
        csv_names = {row["command"].strip() for row in csv.DictReader(fh)}
    missing = sorted(source_names.difference(csv_names))
    if missing:
        errors.append("cli.csv is missing top-level CLI commands: " + ", ".join(missing))
    extra = sorted(csv_names.difference(source_names))
    if extra:
        errors.append("cli.csv contains commands not present in CLI: " + ", ".join(extra))
    return errors


def csv_cli_names() -> set[str]:
    with CLI_CSV.open(newline="", encoding="utf-8") as fh:
        return {row["command"].strip() for row in csv.DictReader(fh)}


def shell_fence_bodies(text: str) -> list[str]:
    return re.findall(r"```(?:bash|sh)\n(.*?)```", text, flags=re.DOTALL)


def cli_commands_in_shell_example(body: str) -> set[str]:
    names: set[str] = set()
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        match = re.match(r"lumi-tester\s+([A-Za-z][A-Za-z0-9-]*|<command>)\b", stripped)
        if not match:
            match = re.match(
                r"cargo\s+run\s+--\s+([A-Za-z][A-Za-z0-9-]*|<command>)\b",
                stripped,
            )
        if not match:
            continue
        command = match.group(1)
        if command != "<command>":
            names.add(command)
    return names


def validate_shell_cli_examples(cli_names: set[str]) -> list[str]:
    errors: list[str] = []
    paths = [SKILL_MD, *sorted((SKILL_DIR / "references").glob("*.md"))]
    for path in paths:
        text = path.read_text(encoding="utf-8")
        for idx, body in enumerate(shell_fence_bodies(text), start=1):
            unknown = sorted(cli_commands_in_shell_example(body).difference(cli_names))
            if unknown:
                errors.append(
                    f"{path}: shell example block {idx} uses unknown CLI command(s): "
                    + ", ".join(unknown)
                )
    return errors


def main() -> int:
    errors: list[str] = []
    errors.extend(
        validate_csv(
            COMMANDS_CSV,
            {
                "command",
                "aliases",
                "category",
                "purpose",
                "param_shape",
                "required_fields",
                "common_fields",
                "selector_supported",
                "platforms",
                "example",
                "notes",
            },
        )
    )
    errors.extend(
        validate_csv(
            SELECTORS_CSV,
            {
                "selector",
                "aliases",
                "platforms",
                "stability_rank",
                "when_to_use",
                "example",
                "anti_patterns",
            },
        )
    )
    errors.extend(
        validate_csv(
            CLI_CSV,
            {
                "command",
                "category",
                "purpose",
                "platforms",
                "common_options",
                "machine_readable",
                "agent_use",
                "example",
                "notes",
            },
        )
    )
    errors.extend(
        validate_csv(
            HEADERS_CSV,
            {
                "field",
                "aliases",
                "type",
                "platforms",
                "required",
                "example",
                "notes",
            },
        )
    )
    errors.extend(validate_skill_references())
    errors.extend(validate_skill_install_file_lists())
    errors.extend(validate_csv_alias_quality(COMMANDS_CSV, "command"))
    errors.extend(validate_csv_alias_quality(SELECTORS_CSV, "selector"))
    errors.extend(validate_csv_alias_quality(HEADERS_CSV, "field"))
    errors.extend(validate_headers_catalog())
    errors.extend(validate_reference_navigation())
    errors.extend(validate_reference_index())
    errors.extend(validate_agents_metadata())
    errors.extend(validate_helper_script_reference())
    errors.extend(validate_helper_script_behavior())
    errors.extend(validate_skill_preflight())
    errors.extend(validate_agent_self_test_contract())
    errors.extend(validate_skill_app_identity_guidance())
    errors.extend(validate_mcp_tool_references())
    errors.extend(validate_user_install_docs())
    errors.extend(validate_ai_authoring_contract())
    errors.extend(validate_package_manager_ai_guidance())
    errors.extend(validate_ai_installer_skill_fallback())
    errors.extend(validate_testcase_design_reference())
    errors.extend(validate_debug_artifacts_reference())
    errors.extend(validate_patterns_reference())
    errors.extend(validate_android_auto_reference())
    errors.extend(validate_cli_platform_catalog())
    errors.extend(validate_cli_help_platform_guidance())
    errors.extend(validate_desktop_reference())
    errors.extend(validate_desktop_platform_catalog())
    errors.extend(validate_desktop_clear_state_docs())
    errors.extend(validate_docs_index_content())
    errors.extend(validate_reference_examples())
    errors.extend(validate_command_example_fields())
    errors.extend(validate_selector_catalog())
    errors.extend(validate_selector_quality())
    errors.extend(validate_desktop_selector_guidance())
    errors.extend(validate_cli_catalog())

    parser_names = parser_commands()
    csv_names = csv_command_names()
    cli_names = csv_cli_names()
    schema_names = schema_command_names()
    errors.extend(validate_yaml_command_examples(parser_names))
    errors.extend(validate_shell_cli_examples(cli_names))

    missing = sorted(parser_names.difference(csv_names))
    if missing:
        errors.append(
            "commands.csv is missing parser commands/aliases: " + ", ".join(missing)
        )

    extra_csv = sorted(csv_names.difference(parser_names))
    if extra_csv:
        errors.append(
            "commands.csv contains commands/aliases not accepted by parser: "
            + ", ".join(extra_csv)
        )

    schema_missing = sorted(parser_names.difference(schema_names))
    if schema_missing:
        errors.append(
            "lumi-test.schema.json is missing parser commands/aliases: "
            + ", ".join(schema_missing)
        )

    extra_schema = sorted(schema_names.difference(parser_names))
    if extra_schema:
        errors.append(
            "lumi-test.schema.json contains commands/aliases not accepted by parser: "
            + ", ".join(extra_schema)
        )

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print("AI references are in sync.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
