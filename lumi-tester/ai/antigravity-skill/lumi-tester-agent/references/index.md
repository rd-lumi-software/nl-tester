# Lumi Tester Reference Index

Use this file first when you are unsure which Lumi Tester reference to open.
Prefer searching the CSV files for exact command/header/selector names, then
open the focused Markdown reference only when examples or workflow rules are
needed.

## Fast Lookup

- `cli.csv`: CLI commands, platform support, JSON output, and agent use.
- `headers.csv`: YAML header fields such as `platform`, `appId`, `url`,
  `browser`, `defaultTimeout`, and `desktopState.clear`.
- `commands.csv`: YAML command names, aliases, parameter shape, selector
  support, platforms, examples, and notes.
- `selectors.csv`: selector fields, aliases, platform support, stability rank,
  examples, and anti-patterns.

## Workflow References

- `command-catalog.md`: examples and intent for common YAML commands.
- `testcase-design.md`: coverage design, generated suite layout, grouping, and
  stop conditions.
- `patterns.md`: common flows such as current Android app smoke, login,
  onboarding, search, settings, permission dialogs, GPS, and web forms.
- `selector-discovery.md`: how to discover selectors from Inspector, snapshots,
  UI XML, screenshots, and MCP `suggest_selectors`.
- `debug-artifacts.md`: how to classify and debug failures from `run.json`,
  `test-results.json`, `events.jsonl`, screenshots, UI hierarchy, and logs.

## Platform References

- `android-auto.md`: Android Auto DHU setup, point-only input, supported
  commands, and debug limits.
- `desktop.md`: macOS/Windows desktop app identity, permissions, selectors,
  screenshots/pixel checks, and `desktopState.clear`.
- `hardware.md`: hardware Jig automation, `hw*` commands, shared profiles, and TCS34725 LED blink verification.
- `hc-execution.md`: HC gateway profile, health/discovery/control/readback,
  restoration, and evidence rules.
