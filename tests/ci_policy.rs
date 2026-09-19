use std::{fs, path::Path};

mod matrix {
    include!("compatibility_matrix.rs");
}

const WSL_SCHEDULE: &str = "${{ github.event_name == 'workflow_dispatch' || (github.event_name == 'push' && github.ref == 'refs/heads/main') }}";
const PROOF_BASE_ENV: &str =
    "VALUE_PROOF_BASE: ${{ github.event.pull_request.base.sha || github.event.before }}";
const ZERO_PROOF_BASE: &str =
    "if [ \"$VALUE_PROOF_BASE\" = \"0000000000000000000000000000000000000000\" ]; then";
const EMPTY_OR_ZERO_PROOF_BASE: &str =
    "if [ -z \"$VALUE_PROOF_BASE\" ] || [ \"$VALUE_PROOF_BASE\" = \"0000000000000000000000000000000000000000\" ]; then";
const EXACT_FETCH: &str = "git fetch --no-tags origin \"$VALUE_PROOF_BASE\"";
const VERIFY_PROOF_BASE: &str = "git cat-file -e \"${VALUE_PROOF_BASE}^{commit}\"";
const POSIX_SHA_ASSERTION: &str = "test \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"";
const WINDOWS_SHA_ASSERTION: &str = "if ((git rev-parse HEAD) -ne $env:GITHUB_SHA)";

fn source(path: &str) -> String {
    fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path)).unwrap()
}

#[test]
fn canonical_skills_keep_lf_checkout_rules() {
    let attributes = source(".gitattributes");
    for path in ["skills/exitbind/SKILL.md", "skills/soulmate/SKILL.md"] {
        assert!(
            attributes.lines().any(|line| {
                let mut fields = line.split_whitespace();
                fields.next() == Some(path) && fields.any(|field| field == "eol=lf")
            }),
            "{path} must have an explicit LF checkout rule"
        );
    }
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0;
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    let mut escaped = false;
    while let Some(character) = chars.next() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        if character == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if character == '"' {
            quoted = true;
        } else if character == '{' {
            delta += 1;
        } else if character == '}' {
            delta -= 1;
        }
    }
    delta
}

fn production_source(source: &str) -> String {
    let mut output = String::new();
    let mut test_depth = None;
    for line in source.lines() {
        if test_depth.is_none() && line.trim() == "#[cfg(test)]" {
            test_depth = Some(0);
            continue;
        }
        if let Some(depth) = test_depth.as_mut() {
            let delta = brace_delta(line);
            if *depth == 0 {
                if delta != 0 {
                    *depth = delta;
                } else if line.trim_end().ends_with(';') {
                    test_depth = None;
                }
            } else {
                *depth += delta;
                if *depth == 0 {
                    test_depth = None;
                }
            }
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

#[test]
fn lifecycle_wire_labels_have_one_production_owner() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let labels = [
        "\"READY\"",
        "\"REFUSED\"",
        "\"BLOCKED\"",
        "\"NOT_APPLICABLE\"",
    ];
    let mut violations = Vec::new();
    for entry in fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path.file_name().and_then(|name| name.to_str()) == Some("run_exit.rs")
        {
            continue;
        }
        let source = production_source(&fs::read_to_string(&path).unwrap());
        for (line, contents) in source.lines().enumerate() {
            for label in labels {
                if contents.contains(label) {
                    violations.push(format!("{}:{} contains {label}", path.display(), line + 1));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "lifecycle label ownership drift: {violations:?}"
    );
}

fn job_block<'a>(workflow: &'a str, job: &str, next_job: &str) -> &'a str {
    let after_job = workflow.split_once(&format!("  {job}:\n")).unwrap().1;
    after_job
        .split_once(&format!("  {next_job}:\n"))
        .map_or(after_job, |parts| parts.0)
}

fn selected_proof_base<'a>(pr_base: &'a str, before: &'a str, head_parent: &'a str) -> &'a str {
    let value = if pr_base.is_empty() { before } else { pr_base };
    if value.is_empty() || value == "0000000000000000000000000000000000000000" {
        head_parent
    } else {
        value
    }
}

#[test]
fn current_proof_and_refusal_gates_use_the_exitbind_binary() {
    let proof = matrix::matrix()["proof"].clone();
    for invocation in proof["invocations"].as_array().unwrap() {
        let path = invocation["path"].as_str().unwrap();
        let source = source(path);
        let command = invocation["command"].as_str().unwrap();
        let occurrences = invocation["occurrences"].as_u64().unwrap() as usize;
        assert_eq!(
            source.matches(command).count(),
            occurrences,
            "{path} proof projection count"
        );
        if path.ends_with(".yml") {
            assert!(!source.contains("SOULMATE_BIN"));
            assert!(!source.contains("target/debug/soulmate"));
        }
    }
}

#[test]
fn primary_ci_and_release_keep_the_accepted_wsl_cadence() {
    let ci = source(".github/workflows/ci.yml");
    assert!(ci.contains("on:\n  push:\n  pull_request:\n  workflow_dispatch:"));

    let exit = ci
        .split_once("  exit:\n")
        .unwrap()
        .1
        .split_once("  macos:\n")
        .unwrap()
        .0;
    let macos = ci
        .split_once("  macos:\n")
        .unwrap()
        .1
        .split_once("  windows-wsl:\n")
        .unwrap()
        .0;
    for block in [exit, macos] {
        assert_eq!(block.matches(PROOF_BASE_ENV).count(), 1);
        assert!(block.contains(EMPTY_OR_ZERO_PROOF_BASE));
        assert!(block.contains("git rev-parse --verify HEAD^"));
        assert_eq!(block.matches(EXACT_FETCH).count(), 1);
        assert_eq!(block.matches(VERIFY_PROOF_BASE).count(), 2);
        assert!(block.find(EXACT_FETCH).unwrap() < block.find("cargo test --locked").unwrap());
        assert!(!block.contains("git fetch origin main"));
        assert!(!block.contains("VALUE_PROOF_BASE=$(git rev-parse --verify HEAD^) ||"));
    }

    let wsl = ci
        .split_once("  windows-wsl:\n")
        .unwrap()
        .1
        .split_once("  audit:\n")
        .unwrap()
        .0;
    assert!(wsl.contains(&format!("    if: {WSL_SCHEDULE}\n")));
    assert!(wsl.contains("    needs: exit\n"));

    let upload = ci
        .split_once("      - name: Upload Linux stable release for WSL\n")
        .unwrap()
        .1
        .split_once("  macos:\n")
        .unwrap()
        .0;
    assert!(upload.contains(&format!("        if: {WSL_SCHEDULE}\n")));

    let release = source(".github/workflows/release.yml");
    assert!(release.contains("  windows-wsl:\n"));
    assert!(release.contains("  publish:\n    needs: [linux, macos, windows-wsl]\n"));
    assert!(!release.contains(WSL_SCHEDULE));
}

#[test]
fn workflow_dispatch_empty_proof_base_uses_head_parent_and_controls_remain_distinct() {
    let ci = source(".github/workflows/ci.yml");
    for block in [
        job_block(&ci, "exit", "macos"),
        job_block(&ci, "macos", "windows-wsl"),
    ] {
        assert_eq!(block.matches(EMPTY_OR_ZERO_PROOF_BASE).count(), 1);
        assert!(!block.contains(ZERO_PROOF_BASE));
    }

    assert_eq!(selected_proof_base("", "", "head-parent"), "head-parent");
    assert_eq!(
        selected_proof_base("pr-base", "event-before", "head-parent"),
        "pr-base"
    );
    assert_eq!(
        selected_proof_base("", "event-before", "head-parent"),
        "event-before"
    );
    assert_eq!(
        selected_proof_base(
            "",
            "0000000000000000000000000000000000000000",
            "head-parent"
        ),
        "head-parent"
    );
}

#[test]
fn every_checkout_job_asserts_the_github_sha_once_before_use() {
    let ci = source(".github/workflows/ci.yml");
    for (job, next_job, assertion) in [
        ("exit", "macos", POSIX_SHA_ASSERTION),
        ("macos", "windows-wsl", POSIX_SHA_ASSERTION),
        ("windows-wsl", "audit", WINDOWS_SHA_ASSERTION),
        ("audit", "__missing__", POSIX_SHA_ASSERTION),
    ] {
        let block = if next_job == "__missing__" {
            ci.split_once(&format!("  {job}:\n")).unwrap().1
        } else {
            job_block(&ci, job, next_job)
        };
        assert_eq!(
            block.matches(assertion).count(),
            1,
            "CI {job} checkout identity"
        );
        assert!(block.find("actions/checkout@").unwrap() < block.find(assertion).unwrap());
    }

    let release = source(".github/workflows/release.yml");
    for (job, next_job, assertion) in [
        ("linux", "macos", POSIX_SHA_ASSERTION),
        ("macos", "windows-wsl", POSIX_SHA_ASSERTION),
        ("windows-wsl", "publish", WINDOWS_SHA_ASSERTION),
        ("publish", "installed", POSIX_SHA_ASSERTION),
        ("installed", "__missing__", POSIX_SHA_ASSERTION),
    ] {
        let block = if next_job == "__missing__" {
            release.split_once(&format!("  {job}:\n")).unwrap().1
        } else {
            job_block(&release, job, next_job)
        };
        assert_eq!(
            block.matches(assertion).count(),
            1,
            "release {job} checkout identity"
        );
        assert!(block.find("actions/checkout@").unwrap() < block.find(assertion).unwrap());
    }
}

/// The context-surface inventory is a measurement, not a gate. It must report
/// the same numbers from any working directory — otherwise a later context
/// audit would compare against an environment, not against the repository —
/// and it must not print where the repository happens to be checked out.
#[cfg(unix)]
#[test]
fn context_surface_inventory_is_reproducible_and_path_free() {
    use std::process::Command;

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/context-surface-inventory.sh");
    let run = |cwd: &Path| {
        let output = Command::new("sh")
            .arg(&script)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let here = run(root);
    let elsewhere = run(Path::new("/"));
    assert_eq!(
        here, elsewhere,
        "the inventory must not depend on the working directory"
    );

    // Every measured surface, with the stage label that says when a session
    // reads it, appears in the report.
    for surface in [
        "AGENTS.md",
        "skills/exitbind-bootstrap/SKILL.md",
        "skills/exitbind/SKILL.md",
        "skills/exitbind/references/preservation.md",
        "plugins/exitbind/skills/exitbind/SKILL.md",
    ] {
        assert!(here.contains(surface), "inventory omits {surface}");
    }
    for stage in [
        "bootstrap",
        "activated-skill",
        "delayed-reference",
        "packaged-duplicate",
    ] {
        assert!(here.contains(stage), "inventory omits stage {stage}");
    }

    // Leave the absolute checkout path out: no root prefix, no home directory.
    let absolute = root.to_str().unwrap();
    assert!(
        !here.contains(absolute),
        "inventory leaks the checkout path"
    );
    if let Some(home) = std::env::var_os("HOME").and_then(|home| home.into_string().ok()) {
        assert!(!here.contains(&home), "inventory leaks the home directory");
    }

    // The report is sizes, not estimates: every measured line carries integer
    // bytes and lines, and no provider token count is invented.
    let rows: Vec<&str> = here
        .lines()
        .filter(|line| line.contains("skills/") || line.starts_with("AGENTS.md"))
        .collect();
    assert!(!rows.is_empty(), "no measured rows: {here}");
    for row in rows {
        let numbers: Vec<&str> = row
            .split_whitespace()
            .filter(|field| field.chars().all(|c| c.is_ascii_digit()))
            .collect();
        assert_eq!(
            numbers.len(),
            2,
            "row must carry byte and line counts only: {row}"
        );
    }
    assert!(
        !here.to_lowercase().contains("token"),
        "the inventory must not estimate provider tokens: {here}"
    );
}
