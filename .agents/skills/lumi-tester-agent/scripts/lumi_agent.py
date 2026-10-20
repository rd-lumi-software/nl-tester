#!/usr/bin/env python3
"""Small helper for running Lumi Tester agent commands from any workspace."""

from __future__ import annotations

import argparse
import json
import shlex
import shutil
import subprocess
import sys
from pathlib import Path


LUMI_COMMANDS = [
    "ai",
    "camera",
    "devices",
    "doctor",
    "docs",
    "extension",
    "inspect",
    "jig",
    "list",
    "record",
    "report",
    "requirements-coverage",
    "run",
    "schema",
    "shell",
    "suggest-selectors",
    "system",
    "update",
    "validate",
    "version",
    "which",
]

AGENT_COMMANDS = [
    "agent-check",
    "agent-debug",
    "agent-doctor",
    "agent-hc-preflight",
    "agent-list",
    "agent-run",
    "agent-schema",
    "agent-validate",
]

RUN_PLATFORMS = ["android", "android_auto", "ios", "web", "macos", "windows", "lumi_hc"]
DOCTOR_PLATFORMS = ["android", "android_auto", "ios", "web", "macos", "windows"]


def find_repo_root(start: Path) -> Path | None:
    current = start.resolve()
    for candidate in [current, *current.parents]:
        if (candidate / "lumi-tester" / "Cargo.toml").exists():
            return candidate
    return None


def build_command(command: str, extra: list[str]) -> tuple[list[str], Path | None]:
    repo = find_repo_root(Path.cwd())
    if repo is not None:
        manifest = repo / "lumi-tester" / "Cargo.toml"
        return [
            "cargo",
            "run",
            "--manifest-path",
            str(manifest),
            "--",
            command,
            *extra,
        ], None

    binary = shutil.which("lumi-tester")
    if binary:
        return [binary, command, *extra], None

    raise SystemExit(
        "Could not find repo-local lumi-tester/Cargo.toml or installed lumi-tester binary"
    )


def run_lumi(command: str, extra: list[str]) -> int:
    return_code, stdout, stderr = run_lumi_capture(command, extra)

    if stdout:
        print(stdout, end="")
    if stderr:
        print(stderr, end="", file=sys.stderr)

    return return_code


def run_lumi_capture(command: str, extra: list[str]) -> tuple[int, str, str]:
    invocation, cwd = build_command(command, extra)
    proc = subprocess.run(invocation, cwd=cwd, text=True, capture_output=True)
    return proc.returncode, proc.stdout, proc.stderr


def parse_passthrough(argv: list[str]) -> tuple[str, list[str]]:
    parser = argparse.ArgumentParser(description="Run Lumi Tester commands for agents")
    parser.add_argument(
        "command",
        choices=[*LUMI_COMMANDS, *AGENT_COMMANDS],
        help="Lumi Tester command to run",
    )
    parser.add_argument("extra", nargs=argparse.REMAINDER)
    parsed = parser.parse_args(argv)
    return parsed.command, parsed.extra


def parse_agent_run(argv: list[str], *, require_command_index: bool = False) -> list[str]:
    parser = argparse.ArgumentParser(
        description="Run a Lumi test with debug-friendly artifacts enabled"
    )
    parser.add_argument("path", help="YAML file or test directory")
    parser.add_argument("--platform", required=True, choices=RUN_PLATFORMS)
    parser.add_argument("--device", help="Android serial, iOS UDID, or browser/device target")
    parser.add_argument("--output", default="./output/lumi-agent")
    parser.add_argument("--command-index", type=int, required=require_command_index)
    parsed, unknown = parser.parse_known_args(argv)

    extra = [
        parsed.path,
        "--platform",
        parsed.platform,
        "--report",
        "--snapshot",
        "--events-jsonl",
        "--output",
        parsed.output,
    ]
    if parsed.device:
        extra.extend(["--device", parsed.device])
    if parsed.command_index is not None:
        extra.extend(["--command-index", str(parsed.command_index)])
    extra.extend(unknown)
    return extra


def parse_agent_path_json(argv: list[str], command: str) -> list[str]:
    parser = argparse.ArgumentParser(description=f"Run Lumi {command} with JSON output")
    parser.add_argument("path", help="YAML file or test directory")
    parsed = parser.parse_args(argv)
    return [parsed.path, "--json"]


def parse_agent_doctor(argv: list[str]) -> list[str]:
    parser = argparse.ArgumentParser(description="Run Lumi doctor with JSON output")
    parser.add_argument("--platform", required=True)
    parsed = parser.parse_args(argv)
    if parsed.platform == "lumi_hc":
        parser.error("doctor is not applicable to lumi_hc; use hcHealth through agent-hc-preflight")
    if parsed.platform not in DOCTOR_PLATFORMS:
        parser.error(f"unsupported doctor platform: {parsed.platform}")
    return ["--platform", parsed.platform, "--json"]


def parse_agent_check(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Lumi authoring gates and optional runtime execution"
    )
    parser.add_argument("path", help="YAML file or test directory")
    parser.add_argument("--platform", choices=RUN_PLATFORMS)
    parser.add_argument("--device", help="Android serial, iOS UDID, or browser/device target")
    parser.add_argument("--output", default="./output/lumi-agent")
    parser.add_argument(
        "--summary-json",
        help="Write a compact JSON summary for AI final-answer evidence",
    )
    parser.add_argument(
        "--run",
        action="store_true",
        help="Run the test after validate/list and platform preflight pass",
    )
    parsed, unknown = parser.parse_known_args(argv)
    parsed.extra = unknown
    parsed.command_line = shlex.join(["agent-check", *argv])
    if parsed.run and not parsed.platform:
        parser.error("--run requires --platform")
    return parsed


def parse_agent_hc_preflight(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a read-only HC health/discovery flow with debug artifacts"
    )
    parser.add_argument("path", help="Read-only HC preflight YAML flow")
    parser.add_argument("--device", help="Reserved for consistent agent invocation syntax")
    parser.add_argument("--output", default="./output/lumi-agent-hc-preflight")
    parser.add_argument("--summary-json", help="Write a compact preflight summary")
    parsed = parser.parse_args(argv)
    parsed.platform = "lumi_hc"
    parsed.run = True
    parsed.extra = []
    parsed.command_line = shlex.join(["agent-hc-preflight", *argv])
    parsed.run_extra = parse_agent_run(
        [
            parsed.path,
            "--platform",
            parsed.platform,
            "--output",
            parsed.output,
            *( ["--device", parsed.device] if parsed.device else [] ),
        ]
    )
    return parsed


def hc_command_names(list_stdout: str) -> list[str] | None:
    try:
        payload = json.loads(list_stdout)
    except json.JSONDecodeError:
        return None
    names: list[str] = []
    for flow in payload.get("files", []):
        for command in flow.get("commands", []):
            name = command.get("name")
            if isinstance(name, str):
                names.append(name)
    return names


def validate_hc_preflight_commands(list_stdout: str) -> tuple[bool, str]:
    names = hc_command_names(list_stdout)
    if names is None:
        return False, "list --json did not return parseable command metadata"
    if "hcHealth" not in names:
        return False, "HC preflight flow must contain hcHealth"
    allowed = {"hcHealth", "hcGetEntities", "hcGetState"}
    unsupported = sorted(set(names) - allowed)
    if unsupported:
        return False, "HC preflight flow contains mutating or unsupported commands: " + ", ".join(unsupported)
    return True, ""


def write_agent_check_summary(
    parsed: argparse.Namespace,
    steps: list[dict[str, object]],
    status: str,
    runtime_status: str,
    exit_code: int,
) -> None:
    if not parsed.summary_json:
        return
    summary_path = Path(parsed.summary_json)
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary = {
        "status": status,
        "exitCode": exit_code,
        "path": parsed.path,
        "platform": parsed.platform,
        "device": parsed.device,
        "runRequested": parsed.run,
        "runtimeStatus": runtime_status,
        "output": parsed.output if parsed.run else None,
        "commandLine": parsed.command_line,
        "steps": steps,
    }
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")


def lumi_command_string(command: str, extra: list[str]) -> str:
    return shlex.join(["lumi-tester", command, *extra])


def run_agent_check(argv: list[str]) -> int:
    parsed = parse_agent_check(argv)
    steps: list[dict[str, object]] = []
    runtime_ran = False
    hc_preflight_completed = False

    print("== lumi agent-check: validate --json ==", file=sys.stderr)
    validate_extra = [parsed.path, "--json"]
    code = run_lumi("validate", validate_extra)
    steps.append(
        {
            "name": "validate",
            "command": lumi_command_string("validate", validate_extra),
            "passed": code == 0,
            "exitCode": code,
        }
    )
    if code != 0:
        print("== lumi agent-check: validate FAILED ==", file=sys.stderr)
        write_agent_check_summary(parsed, steps, "failed", "not_checked", code)
        return code
    print("== lumi agent-check: validate PASSED ==", file=sys.stderr)

    print("== lumi agent-check: list --json ==", file=sys.stderr)
    list_extra = [parsed.path, "--json"]
    code, list_stdout, list_stderr = run_lumi_capture("list", list_extra)
    if list_stdout:
        print(list_stdout, end="")
    if list_stderr:
        print(list_stderr, end="", file=sys.stderr)
    steps.append(
        {
            "name": "list",
            "command": lumi_command_string("list", list_extra),
            "passed": code == 0,
            "exitCode": code,
        }
    )
    if code != 0:
        print("== lumi agent-check: list FAILED ==", file=sys.stderr)
        write_agent_check_summary(parsed, steps, "failed", "not_checked", code)
        return code
    print("== lumi agent-check: list PASSED ==", file=sys.stderr)

    if parsed.platform and parsed.platform != "lumi_hc":
        print("== lumi agent-check: doctor --json ==", file=sys.stderr)
        doctor_extra = ["--platform", parsed.platform, "--json"]
        code = run_lumi("doctor", doctor_extra)
        steps.append(
            {
                "name": "doctor",
                "command": lumi_command_string("doctor", doctor_extra),
                "passed": code == 0,
                "exitCode": code,
            }
        )
        if code != 0:
            print("== lumi agent-check: doctor BLOCKED ==", file=sys.stderr)
            write_agent_check_summary(parsed, steps, "blocked", "blocked", code)
            return code
        print("== lumi agent-check: doctor PASSED ==", file=sys.stderr)

    if parsed.platform == "lumi_hc" and parsed.run:
        allowed, reason = validate_hc_preflight_commands(list_stdout)
        if not allowed:
            print(f"== lumi agent-check: HC preflight BLOCKED: {reason} ==", file=sys.stderr)
            steps.append(
                {
                    "name": "hcPreflight",
                    "command": "lumi-tester run <read-only hcHealth flow>",
                    "passed": False,
                    "exitCode": 2,
                    "reason": reason,
                }
            )
            write_agent_check_summary(parsed, steps, "blocked", "blocked", 2)
            return 2
        print("== lumi agent-check: hcHealth preflight ==", file=sys.stderr)
        run_extra = parse_agent_run(
            [
                parsed.path,
                "--platform",
                parsed.platform,
                "--output",
                parsed.output,
                *( ["--device", parsed.device] if parsed.device else [] ),
                *parsed.extra,
            ]
        )
        code = run_lumi("run", run_extra)
        steps.append(
            {
                "name": "hcPreflight",
                "command": lumi_command_string("run", run_extra),
                "passed": code == 0,
                "exitCode": code,
                "requires": "hcHealth",
            }
        )
        if code != 0:
            print("== lumi agent-check: hcHealth preflight FAILED ==", file=sys.stderr)
            write_agent_check_summary(parsed, steps, "failed", "failed", code)
            return code
        print("== lumi agent-check: hcHealth preflight PASSED ==", file=sys.stderr)
        runtime_ran = True
        hc_preflight_completed = True

    if parsed.run and not hc_preflight_completed:
        print("== lumi agent-check: run with artifacts ==", file=sys.stderr)
        run_extra = parse_agent_run(
            [
                parsed.path,
                "--platform",
                parsed.platform,
                "--output",
                parsed.output,
                *(["--device", parsed.device] if parsed.device else []),
                *parsed.extra,
            ]
        )
        code = run_lumi(
            "run",
            run_extra,
        )
        steps.append(
            {
                "name": "run",
                "command": lumi_command_string("run", run_extra),
                "passed": code == 0,
                "exitCode": code,
            }
        )
        if code != 0:
            print("== lumi agent-check: run FAILED ==", file=sys.stderr)
            write_agent_check_summary(parsed, steps, "failed", "failed", code)
            return code
        print("== lumi agent-check: run PASSED ==", file=sys.stderr)
        runtime_ran = True

    print("== lumi agent-check: PASS ==", file=sys.stderr)
    runtime_status = "passed" if runtime_ran else ("not_run" if parsed.platform else "not_checked")
    write_agent_check_summary(parsed, steps, "passed", runtime_status, 0)
    return 0


def run_hc_preflight(parsed: argparse.Namespace) -> tuple[int, list[dict[str, object]]]:
    steps: list[dict[str, object]] = []

    validate_extra = [parsed.path, "--json"]
    code = run_lumi("validate", validate_extra)
    steps.append({
        "name": "validate",
        "command": lumi_command_string("validate", validate_extra),
        "passed": code == 0,
        "exitCode": code,
    })
    if code != 0:
        return code, steps

    list_extra = [parsed.path, "--json"]
    code, list_stdout, list_stderr = run_lumi_capture("list", list_extra)
    if list_stdout:
        print(list_stdout, end="")
    if list_stderr:
        print(list_stderr, end="", file=sys.stderr)
    steps.append({
        "name": "list",
        "command": lumi_command_string("list", list_extra),
        "passed": code == 0,
        "exitCode": code,
    })
    if code != 0:
        return code, steps

    allowed, reason = validate_hc_preflight_commands(list_stdout)
    if not allowed:
        steps.append({
            "name": "hcPreflightGate",
            "command": "read-only hcHealth gate",
            "passed": False,
            "exitCode": 2,
            "reason": reason,
        })
        return 2, steps

    code = run_lumi("run", parsed.run_extra)
    steps.append({
        "name": "hcPreflight",
        "command": lumi_command_string("run", parsed.run_extra),
        "passed": code == 0,
        "exitCode": code,
        "requires": "hcHealth",
    })
    return code, steps


def main(argv: list[str] | None = None) -> int:
    command, extra = parse_passthrough(sys.argv[1:] if argv is None else argv)

    if command == "agent-check":
        return run_agent_check(extra)
    if command == "agent-hc-preflight":
        parsed = parse_agent_hc_preflight(extra)
        code, steps = run_hc_preflight(parsed)
        if parsed.summary_json:
            status = "passed" if code == 0 else ("blocked" if code == 2 else "failed")
            runtime_status = "passed" if code == 0 else ("blocked" if code == 2 else "failed")
            write_agent_check_summary(parsed, steps, status, runtime_status, code)
        return code
    if command == "agent-run":
        return run_lumi("run", parse_agent_run(extra))
    if command == "agent-debug":
        return run_lumi("run", parse_agent_run(extra, require_command_index=True))
    if command == "agent-validate":
        return run_lumi("validate", parse_agent_path_json(extra, "validate"))
    if command == "agent-list":
        return run_lumi("list", parse_agent_path_json(extra, "list"))
    if command == "agent-doctor":
        return run_lumi("doctor", parse_agent_doctor(extra))
    if command == "agent-schema":
        return run_lumi("schema", ["--json", *extra])

    return run_lumi(command, extra)


if __name__ == "__main__":
    raise SystemExit(main())
