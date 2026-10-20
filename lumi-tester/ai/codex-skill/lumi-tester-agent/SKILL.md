---
name: lumi-tester-agent
description: Design testcase coverage, write, validate, run, and debug Lumi Tester YAML automation flows for Android, iOS, Android Auto, Web, macOS, and Windows. Use when AI (Codex, Antigravity) is asked to create test cases from requirements, generate grouped test folders, create or fix Lumi YAML tests, run Lumi Tester from a repo or installed binary, inspect validate/list/doctor/schema JSON output, debug failed commands using run.json/events.jsonl/test-results.json/screenshots/UI XML/logs, or rerun a failing command by command index.
---

# Lumi Tester Agent

Use this skill to operate Lumi Tester as an AI test author and debugger. It is
for writing and running tests, not for extending the Lumi Tester framework
itself. For framework development, use the `lumi-tester` development skill.

## Platform Coverage

Support Android, Android Auto, iOS, Web, macOS, and Windows workflows. Do not
specialize the skill, helper, or flow patterns for only one platform unless the
user's target is explicitly platform-specific. Always set or infer the target
platform before selecting commands, selectors, devices, and debug artifacts:

- Android: app package `appId`, Android device serial, UIAutomator XML,
  `id`/`resourceId`, `accessibilityId`/`contentDesc`, `text`, OCR fallback.
- Android Auto: `platform: android_auto`, Android device serial, DHU runtime,
  point-only tap, dpad/key commands, screenshot/log artifacts, no UI hierarchy.
- iOS: bundle id `appId`, simulator/device UDID, accessibility tree,
  `accessibilityId`/`label`, `text`, OCR fallback.
- Web: `url`/browser, DOM selectors such as `css`, `role`, `placeholder`,
  `text`, and browser artifacts.
- macOS: local desktop app path or bundle id in `appId`, Accessibility
  hierarchy, `text`/`id`/`description`/`role`/`type` best-effort selectors,
  screenshot/pixel commands, `point` fallback, Accessibility and Screen
  Recording permissions.
- Windows: local executable path in `appId`, UI Automation hierarchy for the
  foreground window, `text`/`id`/`description`/`role`/`type` best-effort
  selectors, screenshot/pixel commands, `point` fallback, interactive desktop
  session.

For HC preflight use the helper's read-only gate:

```bash
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-hc-preflight path/to/health-and-discovery.yaml --output ./output/hc-preflight --summary-json ./output/hc-preflight.json
```

It requires `hcHealth`, allows only `hcHealth`, `hcGetEntities`, and
`hcGetState`, and rejects `hcControl`. HC uses `hcHealth` rather than
`doctor`; `agent-check --platform lumi_hc` never calls `doctor`.

## Find Lumi Tester

Prefer MCP tools when a `lumi-tester-mcp` server is configured. Use the MCP
tools in this order:

1. `doctor`
2. `run_command` for a quick one-off action (no YAML file needed)
3. `validate_yaml`
4. `list_tests`
5. `schema` when command/header shape is unclear
6. `run_test`
7. `read_report`, `read_events`, `read_artifact`
8. `inspector_get` when a Lumi Inspector server is running
9. `suggest_selectors` when a UI XML artifact is available

If MCP tools are not available, use the CLI flow below.

Prefer the repo-local CLI when the workspace contains `lumi-tester/Cargo.toml`:

```bash
cd lumi-tester
cargo run -- <command>
```

Use an installed binary only when available and the repo source is not present:

```bash
lumi-tester <command>
```

If no CLI is installed, install the AI pack:

```bash
brew install nghi-nv/tap/lumi-tester
lumi-tester ai install
```

Or use the one-line installer:

```bash
curl -fsSL https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install-ai.sh | bash
```

Windows:

```powershell
iwr https://raw.githubusercontent.com/Nghi-NV/nl-tester/main/lumi-tester/scripts/install-ai.ps1 -UseB | iex
```

You can use the bundled helper without loading it:

```bash
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-check path/to/test.yaml
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-check path/to/test.yaml --summary-json ./output/agent-check.json
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-check path/to/test.yaml --platform android --device <serial> --run --output ./output
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-validate path/to/test.yaml
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-list path/to/test.yaml
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-schema
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-doctor --platform android
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-run path/to/test.yaml --platform android --device <serial> --output ./output
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-run path/to/auto.yaml --platform android_auto --device <serial> --output ./output
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-debug path/to/test.yaml --platform android --device <serial> --command-index 3 --output ./output
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-run path/to/desktop.yaml --platform macos --output ./output
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-run path/to/desktop.yaml --platform windows --output ./output
```

The helper prefers repo-local `cargo run` and falls back to an installed
`lumi-tester` binary. `agent-run` and `agent-debug` always include
`--report --snapshot --events-jsonl` so failures have artifacts. Raw Lumi
commands such as `validate`, `list`, `doctor`, and `run` are still available as
passthrough commands when custom flags are needed. Any command listed in
`references/cli.csv`, including setup/debug commands such as `system` and
`shell`, should pass through the helper unchanged. The helper prints stdout/stderr
and exits with the Lumi command exit code.

## Authoring Loop

1. Read `references/index.md` first when you are unsure which reference file
   contains the answer.
2. Search `references/cli.csv` first when choosing a Lumi CLI command.
3. Search `references/headers.csv` first when choosing YAML header fields
   such as `platform`, `appId`, `url`, `browser`, `defaultTimeout`, or
   `desktopState.clear`.
4. Search `references/commands.csv` first when choosing a YAML command.
5. Search `references/selectors.csv` first when choosing a selector.
6. Run `schema --json` when the YAML shape is unclear, but treat it as a
   guardrail, not a strict contract. Some command params/selectors are
   permissive; a successful validation means parseable, not proof that every
   field is semantically used.
7. Read `references/command-catalog.md` when examples or command intent are
   still unclear.
8. Read `references/testcase-design.md` before generating a suite from product
   requirements, user stories, screenshots, API specs, or exploratory findings.
9. Read `references/patterns.md` when the request matches a common workflow
   such as login, onboarding, search, settings, permission, GPS, or web form.
10. Read `references/android-auto.md` for Android Auto DHU tests.
11. Read `references/desktop.md` for native macOS or Windows desktop app tests.
12. For device-backed or desktop-backed requests, confirm the target device/app,
   local desktop host/app, or browser before writing or running a flow. If the
   user says "current app", inspect current focus/frontmost app instead of
   assuming an appId from an existing YAML file.
13. Discover the app identity before launch: Android package, iOS bundle id,
   Web URL/browser target, macOS `.app` path/bundle id, or Windows executable
   path.
14. After `launchApp`, wait for a stable screen element with `waitUntilVisible`
   or `waitSee`; do not use a fixed delay as launch readiness. Android Auto is
   the exception because DHU has no UI hierarchy; use bounded `wait` plus
   screenshot/log assertions there.
15. **Before writing `tap`/`see`/any selector-based command for a screen not
   already inspected in this flow, inspect it first** - use a running
   Inspector, `suggest_selectors`, or a `--snapshot` run to see the real
   hierarchy. Do not guess a selector from a variable name, a screenshot
   alone, or a similar-looking screen elsewhere in the app; wrong guesses cost
   more retries than one inspect pass. See `references/selector-discovery.md`.
16. Write YAML in canonical `header --- commands` format.
17. Run validation before any device/browser/desktop execution.
18. Use `list --json` to discover command indexes.
19. Run with reports, snapshots, and event JSONL for debug-friendly artifacts.
20. On failure, inspect artifacts and rerun the smallest failing command index.

## Preflight Before Running

Before any real device/browser/desktop execution, do this checklist:

1. Run `doctor --platform <platform> --json` and fix missing runtime
   dependencies first.
2. Run `validate <file-or-folder> --json`; stop on `valid: false`.
3. Run `list <file-or-folder> --json` to confirm collected files, setup/hooks,
   skipped subflows, and command indexes.
4. If the suite depends on login, permissions, seeded data, or `clearState`,
   run the folder/group instead of a leaf file.
5. Run with `--report --snapshot --events-jsonl --output <dir>` so debug
   artifacts are available.

State reset rules:

- Use `launchApp: { clearState: true }` only for first-run/reset cases.
- Android/iOS clear state uses the app identity directly.
- macOS/Windows clear state requires header-level `desktopState.clear`; do not
  use Android-only `clearAppData` for desktop apps.
- For desktop, prefer `mode: autoSafe`; use `mode: manual` only when explicit
  app-scoped paths, Keychain services, or HKCU registry keys are known.

Canonical commands:

```bash
cargo run -- validate ./test.yaml --json
cargo run -- list ./test.yaml --json
cargo run -- doctor --platform android --json
cargo run -- doctor --platform android_auto --json
cargo run -- devices --platform android
cargo run -- schema --json
cargo run -- run ./test.yaml --platform android --report --snapshot --events-jsonl --output ./output
cargo run -- run ./test.yaml --platform android --command-index 3 --report --snapshot --events-jsonl --output ./output
```

For Web:

```bash
cargo run -- doctor --platform web --json
cargo run -- run ./test.yaml --platform web --report --snapshot --events-jsonl --output ./output
```

For iOS:

```bash
cargo run -- doctor --platform ios --json
cargo run -- run ./test.yaml --platform ios --report --snapshot --events-jsonl --output ./output
```

For Android Auto:

```bash
cargo run -- doctor --platform android_auto --json
cargo run -- run ./auto.yaml --platform android_auto --device <serial> --report --snapshot --events-jsonl --output ./output
```

For desktop:

```bash
cargo run -- doctor --platform macos --json
cargo run -- doctor --platform windows --json
cargo run -- run ./desktop.yaml --platform macos --report --snapshot --events-jsonl --output ./output
cargo run -- run ./desktop.yaml --platform windows --report --snapshot --events-jsonl --output ./output
```

## Canonical YAML

Prefer stable selectors over coordinates:

```yaml
platform: android
appId: com.example.app
tags:
  - smoke
defaultTimeout: 10000
---
- launchApp
- waitUntilVisible:
    id: "login_button"
- tap:
    id: "login_button"
- tap:
    id: "email"
- inputText: "test@example.com"
- see:
    text: "Welcome"
    exact: true
```

Selector priority:

1. `regex` / Shorthand (e.g. `tap: "name|tên"`, `see: "Accept|Đồng ý"`) for dynamic/multi-language UI
2. `id` when stable resource ID exists in hierarchy
3. `text` with `exact: true` when stable single-locale text
4. Platform fields (`desc`, `accessibilityId`, `contentDesc`, `placeholder`)
5. Relative positioning (`below`, `above`, `rightOf`, `leftOf`) near a stable anchor
6. `type` with `index` (omit `index` if 0, require `index: N` if > 0)
7. `ocr` when native hierarchy cannot expose text
8. `point` only as a last resort, preferably percentages like `"50%,80%"`

### Sub-Element Positioning (`align` / `offset`)
For composite elements (e.g. Switch toggles, slider tracks, list rows):
```yaml
# Tap right side of switch:
- tap:
    type: "Switch"
    index: 1
    align: right # Presets: left, right, top, bottom, center

# Specific offset inside element bounds:
- tap:
    id: "settings_row"
    offset: "85%,50%"
```

### Relative Positioning (`below`, `above`, `rightOf`, `leftOf`)
Target elements without IDs or text relative to nearby stable anchors:
```yaml
- tap:
    type: "View"
    below: "30%"
    offset: "50%,50%"
```

### Continuous Drag & Slider Control (`drag`)
Use `drag` for sliders, seekbars, reordering, and canvas drawing across Android, iOS, Web, macOS, Windows:
```yaml
- drag:
    from:
      type: "View"
      below: "Brightness"
      offset: "0%,50%"
    to:
      type: "View"
      below: "Brightness"
      offset: "80%,50%"
    duration: 500
```

For macOS and Windows desktop tests, `ocr` and `image` are not selector
fallbacks in the current runtime. Use Accessibility/UI Automation selectors
first, then screenshot/pixel assertions and `point` only when needed.

Do not switch to coordinates just because a selector failed once. First inspect
the UI XML/screenshot, choose the best semantic selector, and only use `point`
for canvas/graphics UI or when no native/visual semantic selector exists.

When a screen has duplicate accessibility labels, keep the semantic selector and
disambiguate with `index`, `type`, or a relative anchor before considering
coordinates.

For text entry, focus first, then type:

```yaml
- tap:
    id: "email"
- inputText: "user@example.com"
```

Do not put selector fields inside `inputText` unless the local parser explicitly
supports that form.

## Terse YAML Style

Prefer the shortest form that still expresses the selector correctly: a bare
string on `tap`/`longPress`/`doubleTap`, `see`/`assertVisible`/
`assertNotVisible`/`waitUntilVisible`/`waitUntilNotVisible`,
`scrollUntilVisible`, `drag.from`/`drag.to`, and the `rightOf`/`leftOf`/
`above`/`below` anchor sub-fields auto-resolves to `regex` (if it has a regex
metacharacter) or plain `text` otherwise - so try the shorthand first and let
the parser classify it, instead of writing `regex:`/`text:` by hand.
Escalate to the full object form only for fields the shorthand can't express
(`index > 0`, `exact`, `timeout`, `soft`, a platform-specific field). Omit
`index: 0` (it's the default). `wait`/`ocr` have their own separate
plain-value shorthand (not regex-related); `copyTextFrom` has none. See
`references/selector-discovery.md`'s "Terse YAML Style" section for the full
metacharacter list and before/after examples - run `validate --json` after
simplifying an existing flow to confirm behavior didn't change.

## Secrets And Reusable Flows

Never write real credentials, tokens, phone numbers, or other sensitive
values directly into a YAML flow. Reference them with `${VAR_NAME}` and
supply the value via a real OS environment variable (export before running -
preferred for CI) or the header's `env: { file: ".env" }` syntax (reads a
local dotenv file). Commit only a placeholder `.env.example`; add the real
`.env` to `.gitignore`. See `references/testcase-design.md`'s "Grouping
Strategy" and "Generated Suite Example" sections for exact per-shell export
commands, the full `.env` pattern, and a worked example.

Extract any block reused by more than one test (login, permission grant,
seeded data, GPS setup, cleanup) into its own file under a `subflows/`
directory and call it with `runFlow`, instead of duplicating commands into
every leaf test. `setup.yaml`/`teardown.yaml` only run for the exact folder
passed to `run` - they do **not** cascade into nested feature folders even
though `run` still executes every file found recursively underneath. See
`references/testcase-design.md`'s "Grouping Strategy" for the exact hook-scope
rule, the suite-wide-vs-per-feature pattern, and the full folder convention.

## App Identity Discovery

Find the app identity before writing `launchApp`:

- Android uses package name in `appId`, for example `com.example.app`.
- iOS uses bundle id in `appId`, for example `com.example.app`.
- Web uses `url` plus optional `browser`.
- macOS uses a `.app` path or bundle id in `appId`, for example
  `/Applications/MyApp.app` or `com.example.MyApp`.
- Windows uses an executable path in `appId`, for example
  `C:\Program Files\Example\Example.exe`.

When a user or existing YAML already provides an `appId`, verify it is installed
and launchable before debugging selectors:

```bash
adb -s <serial> shell pm path <appId>
adb -s <serial> shell cmd package resolve-activity --brief <appId>
xcrun simctl listapps <udid-or-booted> | rg '<bundleId>'
mdls -name kMDItemCFBundleIdentifier /Applications/MyApp.app
powershell -NoProfile -Command "Get-Item 'C:\Program Files\Example\Example.exe'"
```

After launch, compare the foreground/frontmost app with the expected `appId`.
If it does not match, debug install, launch, crash, or device selection before
tuning selectors.

Android foreground package/activity:

When the user references a connected device by exclusion, such as "not the LM
device", list devices and choose by serial/model explicitly:

```bash
adb devices -l
```

When the user asks to test the current Android app, identify the foreground app
from the selected device:

```bash
adb -s <serial> shell dumpsys window | rg -i 'mCurrentFocus|mFocusedApp|topResumed'
adb -s <serial> exec-out uiautomator dump /dev/tty
```

Use the package/activity from current focus as the flow `appId`. Do not reuse an
unrelated YAML file just because it exists in the workspace.

Flutter/Compose screens often expose visible labels as Android `content-desc`
instead of `text`; prefer `accessibilityId`/`desc` when the XML shows
`content-desc="..."`.

iOS bundle id discovery:

```bash
xcrun simctl list devices
xcrun simctl listapps booted | rg -i 'CFBundleIdentifier|CFBundleDisplayName|CFBundleName'
idb list-apps --udid <udid>
```

Web target discovery:

- Use the requested URL as `url`.
- If a local dev server is needed, start it first and use its localhost URL.
- Use `browser: chromium`, `firefox`, or `webkit` only when the browser matters.

## Launch Readiness

Immediately after opening an app or page, wait for a stable element that proves
the expected screen is ready:

```yaml
- launchApp:
    appId: com.example.app
- waitUntilVisible:
    accessibilityId: "Home"
    timeout: 15000
```

For Web:

```yaml
- launchApp
- waitUntilVisible:
    css: "[data-testid='home']"
    timeout: 15000
```

Use fixed `wait` after launch only as a last resort after a selector-based wait
cannot represent readiness. If launch restores a nested screen, inspect the UI
first and navigate with a real command such as `back` or a semantic button tap;
do not add coordinates.

`conditional.condition.visible` and `visibleRegex` currently check text only.
Do not use `conditional` to detect an Android `content-desc`/`accessibilityId`
unless the local runner has been verified to support that selector form.

## Debugging Loop

Use machine-readable files first:

- `output/run.json`: always written after executor finalization.
- `output/events.jsonl`: written when `--events-jsonl` is passed.
- `output/test-results.json`: written when `--report` is passed.
- Failed commands may contain `screenshotPath`, `uiHierarchyPath`, and `logPath`
  when `--snapshot` or `--report` is enabled.

Debug process:

1. Read the first failed command from `run.json` or `test-results.json`.
2. Check its `index`, `commandName`, `status.error`, and artifact paths.
3. Inspect the screenshot/UI XML/log excerpt if present.
4. Read `references/debug-artifacts.md` when the cause is not obvious,
   especially for wrong app, crash, permission dialog, or platform-specific
   failures.
5. Patch only the smallest YAML selector/timing/setup issue.
6. Rerun the failed command with `--command-index`.
7. Rerun the whole flow after the targeted command passes.

Do not count command indexes by hand; use `list --json`.

## Agent Self-Test Contract

Before reporting a Lumi test as ready, produce evidence from the strongest gate
that can run in the current environment:

1. Prefer `agent-check <file-or-folder>` for authoring-only evidence, or
   `agent-check <file-or-folder> --platform <platform> --run --output <dir>`
   when runtime execution is available. Treat `== lumi agent-check: PASS ==`
   as the concise self-test summary. Use
   `--summary-json <file>` when a machine-readable evidence file is useful;
   each JSON step includes the exact Lumi command string, and `runtimeStatus`
   states whether runtime was `passed`, `failed`, `blocked`, `not_run`, or
   `not_checked`.
2. Always run `validate --json` on the file or folder you changed.
3. Always run `list --json` on the same file or folder to prove grouping,
   setup/teardown, skipped subflows, and command indexes.
4. When the target device, browser, desktop host, or app is available, run the
   test with `--report --snapshot --events-jsonl --output <dir>`.
5. When runtime execution is blocked, still run `doctor --platform <platform>
   --json`, record the exact missing dependency/device/app, and do not claim
   runtime pass. In `agent-check` summary JSON, treat `status: blocked` or
   `runtimeStatus: blocked` as an environment/app readiness blocker.
6. After fixing a runtime failure, rerun the smallest failing command with
   `--command-index`, then rerun the full file or folder.
7. In the final answer, mention the exact validation/list/run commands and
   whether each passed, failed, or was blocked.

## Hardware Jig Automation

When testing physical IoT/Smart devices requiring hardware buttons, relays, servos, or color sensors:

1. **Jig Profile in Header**:
   Prefer reusable Jig profiles in the YAML header:
   ```yaml
   platform: android
   appId: com.lumi.lifenext
   jig: "profiles/jig_switch_sample.yaml"
   ---
   - hwPowerOn: 1
   - hwClick: 1
   - hwSeeLedBlink:
       channel: 1
       color: "BLUE"
       count: 2
   ```
2. **Standardized Hardware Commands (`hw*`)**:
   - Relays: `hwPowerOn`, `hwPowerOff`, `hwPowerCycle`, `hwPowerOffAll`
   - Servos: `hwClick`, `hwPress`, `hwRelease`, `hwRotate`, `hwRepeatClick`, `hwStartRepeatClick`, `hwStopRepeatClick`, `hwReleaseAll`, `hwConfigureServo`
   - Sensors & LED: `hwSeeLed`, `hwSeeLedBlink`, `hwSeeNativeLedBlink`, `hwSeeLedOff`, `hwSensorLight`, `hwCalibrateColor`, `hwCalibrateBrightness`, `hwAddCctPoint`, `hwSaveCalibration`, `hwLoadCalibration`, `hwSetBrightnessThresholds`
   - Diagnostics: `hwReadServo`, `hwReadRelay`, `hwReadColor`, `hwReadSensorLight`, `hwDiagnostics`, `hwSafeState`
3. **Serial Port & Jig CLI Utilities**:
   - `lumi-tester jig ports`: Enumerate all connected COM/Serial ports.
   - `lumi-tester jig ping <port_or_profile>`: Verify connectivity, MCU node ID, and firmware latency.

## Validation Rules

Treat these as authoring bugs:

- `validate --json` returns `valid: false`.
- Unknown command errors.
- Missing referenced files.
- A command index is absent from `list --json`.

Treat these as runtime/debug bugs:

- Element not found despite valid YAML.
- Assertion timeout.
- App crash event.
- Screenshot/UI hierarchy mismatch.
- Current focus or failure screenshot shows a different app than the expected
  appId.
- Hardware Jig connection / MCU response timeout.

## Extra Reference

- Read `references/index.md` first when choosing which reference file to open.
- Read or search `references/cli.csv` for fast Lumi CLI lookup by purpose,
  platform, options, output, and when an AI agent should use it.
- Read or search `references/headers.csv` for YAML header fields, aliases,
  platform support, examples, and desktop `desktopState.clear` shape.
- Read `references/command-catalog.md` before writing unfamiliar commands.
- Read `references/testcase-design.md` before turning requirements or
  exploratory findings into a folder of generated tests.
- Read or search `references/commands.csv` for fast command lookup by alias,
  category, parameter shape, selector support, platform, and example.
- Read or search `references/selectors.csv` for selector priority, platform
  support, examples, and anti-patterns.
- Read `references/patterns.md` for common end-to-end flow templates and
  adaptation rules.
- Read `references/desktop.md` for macOS/Windows app identity, permissions,
  selectors, and `desktopState.clear` examples.
- Read `references/selector-discovery.md` when the app/page is unfamiliar,
  selectors are unknown, or a selector fails.
- Read `references/android-auto.md` for DHU setup, point-only interaction, and
  Android Auto command limits.
- Read `references/hardware.md` for hardware Jig automation, `hw*` commands,
  shared profiles, and TCS34725 LED blink verification.
- Read `references/debug-artifacts.md` only when interpreting runtime files or
  building an agentic debug report.

## HC Preflight Safety

For `platform: lumi_hc`, `doctor` is not a readiness check. Use a canonical,
read-only flow containing `hcHealth` (and, when needed, `hcGetEntities` and
`hcGetState`) through:

```bash
python3 ~/.codex/skills/lumi-tester-agent/scripts/lumi_agent.py agent-hc-preflight path/to/health-and-discovery.yaml --output ./output/hc-preflight --summary-json ./output/hc-preflight.json
```

`agent-hc-preflight` runs `validate --json`, `list --json`, rejects
`hcControl` and other mutating commands, then runs with
`--report --snapshot --events-jsonl`. Use `agent-run --platform lumi_hc` only
after the orchestrator plan and delegation response authorize mutation. A
successful control flow is not a restore proof unless initial/final state and
restore readback evidence are present.
