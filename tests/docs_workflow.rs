//! Gate the local documentation journey, including links to renamed headings.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn anchors(markdown: &str) -> BTreeSet<String> {
    let mut seen = BTreeMap::<String, usize>::new();
    let mut result = BTreeSet::new();
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || !line.starts_with('#') {
            continue;
        }
        let text = line.trim_start_matches('#').trim();
        let slug: String = text
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .map(|c| if c == ' ' { '-' } else { c })
            .collect();
        let count = seen.entry(slug.clone()).or_default();
        result.insert(if *count == 0 {
            slug
        } else {
            format!("{slug}-{count}")
        });
        *count += 1;
    }
    result
}

fn check_links(root: &Path, source: &str) -> Result<(), String> {
    let path = root.join(source);
    let markdown = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    for after_label in markdown.split("](").skip(1) {
        let Some((target, _)) = after_label.split_once(')') else {
            continue;
        };
        if target.contains("://") || target.starts_with("mailto:") {
            continue;
        }
        let (file, anchor) = target
            .split_once('#')
            .map_or((target, None), |(f, a)| (f, Some(a)));
        let destination = if file.is_empty() {
            path.clone()
        } else {
            path.parent().unwrap().join(file)
        };
        if !destination.exists() {
            return Err(format!("{source}: missing local destination {target}"));
        }
        if let Some(anchor) = anchor {
            let content = fs::read_to_string(&destination).map_err(|error| error.to_string())?;
            if !anchors(&content).contains(anchor) {
                return Err(format!("{source}: missing heading in {target}"));
            }
        }
    }
    Ok(())
}

#[test]
fn checked_work_journey_links_resolve_in_the_checkout() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for source in [
        "README.md",
        "CONTRIBUTING.md",
        "AGENTS.md",
        "REFERENCE.md",
        "docs/onboarding.md",
        "docs/first-checked-run.md",
        "docs/value-proof-methodology.md",
        "skills/soulmate/SKILL.md",
    ] {
        check_links(root, source).unwrap();
    }
}

#[test]
fn contribution_audiences_and_coffee_contract_stay_separate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |file: &str| fs::read_to_string(root.join(file)).unwrap();
    let coffee = read("skills/coffee/SKILL.md");
    let agents = read("AGENTS.md");
    let contributing = read("CONTRIBUTING.md");

    for private_stack_term in [
        "Repomix",
        "CodeGraph",
        "Ponytail",
        "Grill-Me",
        "role-perspectives",
        "qa-engineer",
        "implementation-planner",
        "current agent's Soul",
        "Venus",
        "`sonic`",
        "`default`",
    ] {
        assert!(
            !coffee.contains(private_stack_term),
            "Coffee depends on private-stack vocabulary: {private_stack_term}"
        );
    }
    for contract in [
        "short, fail-open preparation step",
        "A clear, bounded request should proceed without ceremony",
        "readiness brief containing the goal",
        "existing `soulmate brief` or",
        "Coffee grants no\n  execution authority and must not block work when unavailable",
    ] {
        assert!(
            coffee.contains(contract),
            "Coffee lost contract: {contract}"
        );
    }

    assert!(agents.contains("external coding agents modifying the Exitbind repository"));
    assert!(agents.contains("[CONTRIBUTING.md](CONTRIBUTING.md)"));
    assert!(agents.contains("For setup in another project instead of changes here"));
    assert!(agents.contains("\"Give your lead one link\" path"));
    assert!(!agents.contains("curl -fsSL"));
    assert!(contributing.contains("This guide is for human contributors"));
    assert!(contributing.contains("[AGENTS.md](AGENTS.md)"));
    assert!(contributing
        .contains("This keeps contributor and CI\nformatting and lint behavior aligned"));
    assert!(contributing.contains("cargo clippy --locked --all-targets -- -D warnings"));
}

#[test]
fn repository_toolchain_is_the_only_development_and_ci_selector() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |file: &str| fs::read_to_string(root.join(file)).unwrap();
    let toolchain = read("rust-toolchain.toml");
    let cargo = read("Cargo.toml");
    let contributing = read("CONTRIBUTING.md");

    assert_eq!(
        toolchain,
        "[toolchain]\nchannel = \"1.75.0\"\nprofile = \"minimal\"\ncomponents = [\"rustfmt\", \"clippy\"]\n"
    );
    assert!(cargo.contains("rust-version = \"1.75\""));
    assert!(contributing.contains("[`rust-toolchain.toml`](rust-toolchain.toml)"));
    assert!(!contributing.contains("1.75.0"));
    assert!(!contributing.contains("cargo +"));

    for (workflow, audit_install) in [
        (
            ".github/workflows/ci.yml",
            "cargo +stable install cargo-audit --locked --version 0.22.2",
        ),
        (
            ".github/workflows/release.yml",
            "cargo +stable install cargo-audit --locked --version 0.22.2 --force",
        ),
    ] {
        let workflow = read(workflow);
        assert!(!workflow.contains("1.75.0"));
        assert!(!workflow.contains("rustup toolchain"));
        assert!(!workflow.contains("rustup default"));
        let cargo_overrides: Vec<_> = workflow
            .lines()
            .map(str::trim)
            .filter(|line| line.contains("cargo +"))
            .collect();
        assert_eq!(cargo_overrides, [audit_install]);
    }
}

#[test]
fn heading_gate_distinguishes_fenced_examples_and_removed_destinations() {
    let headings = anchors("# Real\n```md\n# Gone\n```\n## `Quoted` heading\n## Real\n");
    assert!(headings.contains("real"));
    assert!(headings.contains("real-1"));
    assert!(headings.contains("quoted-heading"));
    assert!(!headings.contains("gone"));
}

#[test]
fn checked_result_docs_keep_v3_v4_and_current_stable_boundaries_consistent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |file: &str| fs::read_to_string(root.join(file)).unwrap();
    let readme = read("README.md");
    let reference = read("REFERENCE.md");
    let security = read("SECURITY.md");
    let first = read("docs/first-checked-run.md");
    let methodology = read("docs/value-proof-methodology.md");
    let glossary = read("docs/glossary.md");
    let cli = read("src/cli.rs");
    for document in [&readme, &reference, &security] {
        assert!(document.contains("v0.17.0"));
        assert!(!document.contains("unreleased v5"));
    }
    assert!(readme.contains("historical Soulmate v1–v4"));
    assert!(reference.contains("Historical checked Soulmate runs use run-event version 3"));
    assert!(reference.contains("new Exitbind\nstarts use v5"));
    assert!(glossary.contains("Observed check | v4/v5 source evidence"));
    assert!(glossary.contains("one permitted route in v4/v5"));
    assert!(security.contains("v3 supports caller-reported `run record-check` only"));
    assert!(security.contains("stable `v0.17.0`"));
    assert!(reference.contains("1,800,000 ms (30 minute)\ndefault deadline"));
    assert!(reference.contains("positive `--timeout-ms MS` override"));
    assert!(
        reference.contains("writes no check\nevent when launch, timeout, or durable binding fails")
    );
    assert!(security.contains("runs outside the ledger\nlock"));
    assert_eq!(
        security
            .matches("A process that can rewrite all local\nevidence")
            .count(),
        1
    );
    assert!(first.contains("historical v3 procedure"));
    assert!(methodology.contains("v3 `run record-check` is caller-reported-only"));
    assert!(methodology.contains("stable `v0.17.0` release writes v5 records"));
    assert!(glossary.contains("Exitbind progress"));
    assert!(cli.contains("historical v3-v4 runs remain readable"));
    assert!(cli.contains("new v5 runs may observe"));
    assert!(cli.contains("[--timeout-ms MS]"));
    assert!(cli.contains("1,800,000 ms (30 minute) timeout"));
}
