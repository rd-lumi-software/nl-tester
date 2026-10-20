# Lumi Test Orchestrator Implementation Plan

> **For agentic workers:** Use the repository skill instructions and verify every artifact before reporting completion. This plan intentionally omits commit and push steps because the user asked not to commit or push.

**Goal:** Add a project-local `lumi-test-orchestrator` skill that turns a natural-language test request into a safe, structured test plan and delegates YAML authoring and execution to `lumi-tester-agent`, with a mandatory intake gate before mutating tests.

**Architecture:** The orchestrator owns scope interpretation, inventory selection, capability classification, scenario policy, safety gates, and result aggregation. It does not own Lumi YAML syntax, command semantics, transport, or execution. `lumi-tester-agent` remains the required execution/authoring sub-skill.

**Tech Stack:** Markdown skill instructions, YAML frontmatter, `agents/openai.yaml`, Markdown references, repository-local skill validation.

---

### Task 1: Initialize the skill directory

**Files:**
- Create: `.agents/skills/lumi-test-orchestrator/SKILL.md`
- Create: `.agents/skills/lumi-test-orchestrator/agents/openai.yaml`

- [x] Attempt the repository skill initializer with the new skill name, local
  skill path, and references resource. The command was unavailable because
  Python is not installed on this machine, so the same required structure was
  scaffolded manually with patches.

```powershell
python C:\Users\wuno9\.codex\skills\.system\skill-creator\scripts\init_skill.py lumi-test-orchestrator --path .agents\skills --resources references --interface display_name="Lumi Test Orchestrator" --interface short_description="Plan and coordinate Lumi test runs" --interface default_prompt="Use $lumi-test-orchestrator to plan a safe Lumi HC or app-to-cloud test run and delegate execution to $lumi-tester-agent."
```

- [x] Confirm the skill contains only the required skill files, metadata, and
  focused references.

### Task 2: Write the orchestrator contract

**Files:**
- Modify: `.agents/skills/lumi-test-orchestrator/SKILL.md`

- [x] Replace the initializer template with a concise skill that defines
  triggers, boundaries, the orchestration loop, the structured test-plan
  contract, safety rules, delegation rules, and one HC example.
- [x] State that the orchestrator must invoke/read `lumi-tester-agent` for YAML
  authoring, validation, execution, reports, evidence, and targeted reruns.
- [x] State that the orchestrator must not issue raw device-control API calls
  or invent unsupported HC mappings.
- [x] Keep the frontmatter description trigger-focused, third-person, and
  beginning with `Use when...`.

### Task 3: Add reusable references

**Files:**
- Create: `.agents/skills/lumi-test-orchestrator/references/workflow.md`
- Create: `.agents/skills/lumi-test-orchestrator/references/test-plan-contract.md`
- Create: `.agents/skills/lumi-test-orchestrator/references/hc-orchestration.md`
- Create: `.agents/skills/lumi-test-orchestrator/references/safety-and-evidence.md`
- Create: `.agents/skills/lumi-test-orchestrator/references/delegation.md`

- [x] Document the common phases: clarify, discover, classify, plan, delegate,
  execute, aggregate, and restore.
- [x] Define required test-plan fields for target, inventory, capabilities,
  actions, repeats, concurrency, timing, acceptance criteria, restoration,
  evidence, and outcome.
- [x] Document HC-specific rules: use live discovery, preserve IDs as strings,
  distinguish `entity`, `cell_idx`, and state/input `idx`, block unknown
  mappings, and require an explicit safety gate before control.
- [x] Document safety/evidence tiers and the future App → Cloud → HC extension
  without making it part of the current HC-only execution contract.
- [x] Add a mandatory intake gate that asks for missing endpoint, target,
  capability, repeat, concurrency, delay, timeout, restoration, and evidence
  policy before discovery/control.
- [x] Normalize partial HC connection input so a supplied IP/host remains
  visible as `provided`, while missing scheme/port/profile are asked separately.

### Task 4: Validate the skill artifact

**Files:**
- Verify: `.agents/skills/lumi-test-orchestrator/**`

- [ ] Run the skill quick validator against the new directory. Blocked because
  Python is not installed; the command was attempted and returned exit 9009.

```powershell
python C:\Users\wuno9\.codex\skills\.system\skill-creator\scripts\quick_validate.py .agents\skills\lumi-test-orchestrator
```

- [x] Verify frontmatter, name, description, `agents/openai.yaml`, and absence of
  initializer TODO markers with manual PowerShell checks.
- [x] Verify the role boundary against `.agents/skills/lumi-tester-agent/SKILL.md`
  and its HC reference.
- [x] Run `git diff --check`, scan untracked files for trailing whitespace, and
  inspect the final file list; do not commit or push.

### Task 5: Report the handoff

- [x] Report created/modified files, validation output, and the exact invocation
  pattern for the two skills.
- [x] Explicitly state that no commit or push was performed.
