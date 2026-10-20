use std::fs;
use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn frontmatter_description(skill: &str) -> &str {
    skill
        .lines()
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap()
}

#[test]
fn hc_reference_is_present_in_every_skill_copy_and_installer_bundle() {
    let root = repository_root();
    let bundles = [
        root.join(".agents/skills/lumi-tester-agent"),
        root.join("lumi-tester/ai/codex-skill/lumi-tester-agent"),
        root.join("lumi-tester/ai/antigravity-skill/lumi-tester-agent"),
    ];

    for bundle in bundles {
        let skill = fs::read_to_string(bundle.join("SKILL.md")).unwrap();
        assert!(skill.contains("references/hc-execution.md"));
        assert!(bundle.join("references/hc-execution.md").is_file());
    }

    let ai_source = fs::read_to_string(root.join("lumi-tester/src/ai.rs")).unwrap();
    assert!(ai_source.contains("\"references/hc-execution.md\""));
    assert!(ai_source.contains("lumi-tester/ai/antigravity-skill/lumi-tester-agent"));

    for installer in [
        root.join("lumi-tester/scripts/install-ai.sh"),
        root.join("lumi-tester/scripts/install-ai.ps1"),
    ] {
        let source = fs::read_to_string(installer).unwrap();
        assert!(source.contains("references/hc-execution.md"));
    }
}

#[test]
fn antigravity_skill_docs_use_the_actual_global_skill_path() {
    let root = repository_root();
    let skill = fs::read_to_string(
        root.join("lumi-tester/ai/antigravity-skill/lumi-tester-agent/SKILL.md"),
    )
    .unwrap();
    assert!(!skill.contains("~/.codex/skills/lumi-tester-agent"));
    assert!(skill.contains("~/.gemini/config/skills/lumi-tester-agent"));

    let docs = fs::read_to_string(root.join("lumi-tester/docs/ai-skills-integration.md")).unwrap();
    assert!(docs.contains("~/.gemini/config/skills/lumi-tester-agent"));
    assert!(docs.contains("./.agents/skills/lumi-tester-agent"));
}

#[test]
fn maintained_skill_bundles_have_hc_metadata_parity() {
    let root = repository_root();
    let bundles = [
        ("agents", root.join(".agents/skills/lumi-tester-agent")),
        (
            "codex",
            root.join("lumi-tester/ai/codex-skill/lumi-tester-agent"),
        ),
        (
            "antigravity",
            root.join("lumi-tester/ai/antigravity-skill/lumi-tester-agent"),
        ),
    ];
    let skills = bundles
        .iter()
        .map(|(_, bundle)| fs::read_to_string(bundle.join("SKILL.md")).unwrap())
        .collect::<Vec<_>>();

    assert!(frontmatter_description(&skills[0]).contains("lumi_hc"));
    for skill in &skills[1..] {
        assert_eq!(
            frontmatter_description(&skills[0]),
            frontmatter_description(skill)
        );
    }

    let cli = bundles
        .iter()
        .map(|(_, bundle)| fs::read_to_string(bundle.join("references/cli.csv")).unwrap())
        .collect::<Vec<_>>();
    for command in ["validate", "list", "schema", "run", "report"] {
        let rows = cli
            .iter()
            .map(|contents| {
                contents
                    .lines()
                    .find(|line| line.starts_with(&format!("{command},")))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let canonical = rows[1];
        let canonical_metadata = canonical.splitn(9, ',').take(8).collect::<Vec<_>>();
        for ((name, _), row) in bundles.iter().zip(rows.iter()) {
            let metadata = row.splitn(9, ',').take(8).collect::<Vec<_>>();
            assert_eq!(
                metadata, canonical_metadata,
                "CLI metadata drift for {name}/{command}"
            );
            assert!(
                row.splitn(9, ',')
                    .nth(3)
                    .unwrap()
                    .split('|')
                    .any(|platform| platform == "lumi_hc"),
                "{name}/{command} must declare lumi_hc"
            );
        }
        assert_eq!(
            rows[2], canonical,
            "Codex/Antigravity CLI drift for {command}"
        );
    }
}

#[test]
fn hc_run_helpers_keep_the_generic_validate_list_run_contract() {
    let root = repository_root();
    let helper_paths = [
        root.join(".agents/skills/lumi-tester-agent/scripts/lumi_agent.py"),
        root.join("lumi-tester/ai/codex-skill/lumi-tester-agent/scripts/lumi_agent.py"),
        root.join("lumi-tester/ai/antigravity-skill/lumi-tester-agent/scripts/lumi_agent.py"),
    ];
    let sources = helper_paths
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>();
    for source in &sources {
        assert!(!source.contains("validate_hc_action_contract"));
        assert!(!source.contains("actionContract"));
        assert!(source.contains("agent-validate"));
        assert!(source.contains("agent-run"));
    }
    assert_eq!(
        sources[0], sources[1],
        "Codex helper drifted from canonical copy"
    );
    assert_eq!(
        sources[0], sources[2],
        "Antigravity helper drifted from canonical copy"
    );
}
