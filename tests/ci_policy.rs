#[path = "inventory/rust_source.rs"]
mod rust_source;
#[allow(dead_code)]
mod support;
use std::{fs, path::Path};

mod matrix {
    include!("compatibility_matrix.rs");
}

const PROOF_BASE_ENV: &str =
    "VALUE_PROOF_BASE: ${{ inputs.proof_base || github.event.pull_request.base.sha || github.event.before }}";
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

/// The lifecycle wire labels a production line may not spell outside their
/// owner, in `source` without its test-only items.
fn label_lines(source: &str, labels: &[&str]) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (line, contents) in rust_source::strip_test_modules(source).lines().enumerate() {
        for label in labels {
            if contents.contains(label) {
                found.push((line + 1, (*label).to_owned()));
            }
        }
    }
    found
}

/// The label scan reads production code by extent: a label after a test
/// module holding brace literals is found, one inside it is not.
#[test]
fn lifecycle_label_scan_reads_production_after_a_test_module() {
    let seeded = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    const OPEN: char = '{';\n",
        "    const S: &str = \"}}\";\n",
        "    fn t() -> &'static str { \"READY\" }\n",
        "}\n",
        "fn production() -> &'static str { \"REFUSED\" }\n",
    );
    assert_eq!(
        label_lines(seeded, &["\"READY\"", "\"REFUSED\""]),
        [(7, "\"REFUSED\"".to_owned())]
    );
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
        let source = fs::read_to_string(&path).unwrap();
        for (line, label) in label_lines(&source, &labels) {
            violations.push(format!("{}:{line} contains {label}", path.display()));
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

fn ci_concurrency(event: &str, reference: &str, pr: Option<u64>, run_id: u64) -> (String, bool) {
    let group = if event == "pull_request" {
        format!("Exitbind / Exit-pull-request-{}", pr.unwrap())
    } else if event == "push" && reference.starts_with("refs/heads/feature/") {
        format!("Exitbind / Exit-feature-push-{reference}")
    } else {
        format!("Exitbind / Exit-protected-run-{run_id}")
    };
    let cancel = event == "pull_request"
        || (event == "push" && reference.starts_with("refs/heads/feature/"));
    (group, cancel)
}

#[test]
fn workflow_concurrency_reuses_only_selected_feature_runs() {
    let workflow = source(".github/workflows/ci.yml");
    let group = "group: ${{ (github.event_name == 'pull_request' && format('{0}-pull-request-{1}', github.workflow, github.event.pull_request.number)) || (github.event_name == 'push' && startsWith(github.ref, 'refs/heads/feature/') && format('{0}-feature-push-{1}', github.workflow, github.ref)) || format('{0}-protected-run-{1}', github.workflow, github.run_id) }}";
    let cancellation = "cancel-in-progress: ${{ github.event_name == 'pull_request' || (github.event_name == 'push' && startsWith(github.ref, 'refs/heads/feature/')) }}";
    assert!(
        workflow.contains(group),
        "workflow concurrency group drifted"
    );
    assert!(
        workflow.contains(cancellation),
        "workflow cancellation policy drifted"
    );

    let rows = [
        ("pull_request", "refs/pull/7/merge", Some(7), 10, "-7", true),
        ("pull_request", "refs/pull/7/merge", Some(7), 11, "-7", true),
        ("pull_request", "refs/pull/8/merge", Some(8), 12, "-8", true),
        (
            "push",
            "refs/heads/feature/repair",
            None,
            20,
            "repair",
            true,
        ),
        (
            "push",
            "refs/heads/feature/repair",
            None,
            21,
            "repair",
            true,
        ),
        ("push", "refs/heads/feature/other", None, 22, "other", true),
        (
            "push",
            "refs/heads/main",
            None,
            30,
            "protected-run-30",
            false,
        ),
        (
            "push",
            "refs/heads/main",
            None,
            31,
            "protected-run-31",
            false,
        ),
        (
            "push",
            "refs/tags/v1.0.0",
            None,
            40,
            "protected-run-40",
            false,
        ),
        (
            "push",
            "refs/tags/v1.0.0",
            None,
            41,
            "protected-run-41",
            false,
        ),
        (
            "workflow_dispatch",
            "refs/heads/main",
            None,
            50,
            "protected-run-50",
            false,
        ),
        (
            "workflow_dispatch",
            "refs/heads/main",
            None,
            51,
            "protected-run-51",
            false,
        ),
        (
            "push",
            "refs/heads/release",
            None,
            60,
            "protected-run-60",
            false,
        ),
        (
            "push",
            "refs/heads/release",
            None,
            61,
            "protected-run-61",
            false,
        ),
    ];
    let evaluated = rows
        .iter()
        .map(|(event, reference, pr, run_id, _, _)| ci_concurrency(event, reference, *pr, *run_id))
        .collect::<Vec<_>>();
    for ((_, _, _, _, expected_suffix, expected_cancel), (group, cancel)) in
        rows.iter().zip(evaluated.iter())
    {
        assert!(group.ends_with(expected_suffix), "{group}");
        assert_eq!(cancel, expected_cancel);
    }
    assert_eq!(
        evaluated[0].0, evaluated[1].0,
        "same PR replaces its pending run"
    );
    assert_ne!(
        evaluated[0].0, evaluated[2].0,
        "different PRs stay isolated"
    );
    assert_eq!(
        evaluated[3].0, evaluated[4].0,
        "same feature ref cancels stale work"
    );
    for left in 6usize..evaluated.len() {
        for right in (left + 1)..evaluated.len() {
            assert!(rows[left].4.starts_with("protected-"));
            assert!(rows[right].4.starts_with("protected-"));
            assert_ne!(evaluated[left].0, evaluated[right].0);
            assert!(!evaluated[left].1);
            assert!(!evaluated[right].1);
        }
    }
}

#[test]
fn ci_tests_the_default_product_and_runs_the_real_native_handoff() {
    let manifest = source("Cargo.toml");
    assert!(!manifest.contains("name = \"soulmate\""));
    assert!(!manifest.contains("legacy-cli-test"));

    let shared = source("tooling/repository/ci/ci-local.sh");
    assert!(shared.contains(
        "run native-handoff \"$CARGO\" test --locked --test away_native real_tmux_child_presents_bound_evidence_without_persisting_the_prompt -- --ignored --exact"
    ));
    assert!(shared.contains(
        "./scripts/assert-test-ran.sh real_tmux_child_presents_bound_evidence_without_persisting_the_prompt"
    ));
    assert!(!shared.contains("--features legacy-cli-test"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!root.join("scripts/ci-legacy-tests.sh").exists());
    assert!(!root.join("scripts/test-ci-legacy-selection.sh").exists());
}

#[test]
fn current_proof_and_drift_warning_gates_use_the_exitbind_binary() {
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
fn ordinary_ci_is_linux_only_and_stable_readiness_retains_native_platforms() {
    let ci = source(".github/workflows/ci.yml");
    assert!(ci.contains("  workflow_call:\n"));
    assert!(!ci.contains("  macos:\n"));
    assert!(!ci.contains("  windows-wsl:\n"));
    assert!(!ci.contains("runs-on: macos"));
    assert!(!ci.contains("runs-on: windows"));
    let exit = job_block(&ci, "exit", "audit");
    assert_eq!(exit.matches(PROOF_BASE_ENV).count(), 1);
    assert_eq!(
        exit.matches("run: ./tooling/repository/ci/ci-local.sh\n")
            .count(),
        1
    );
    assert!(ci.contains("ref: ${{ inputs.candidate_sha || github.sha }}"));
    assert!(ci.contains("HEAD_SHA: ${{ inputs.candidate_sha || github.sha }}"));
    let shared = source("tooling/repository/ci/ci-local.sh");
    assert!(shared.contains(EMPTY_OR_ZERO_PROOF_BASE));
    assert_eq!(shared.matches(EXACT_FETCH).count(), 1);
    assert_eq!(shared.matches(VERIFY_PROOF_BASE).count(), 2);
    assert!(shared.find(EXACT_FETCH).unwrap() < shared.find("test --locked").unwrap());
    assert!(!shared.contains("git fetch origin main"));
    let release = source(".github/workflows/release.yml");
    assert!(release.contains("options: [release, stable-readiness]"));
    assert!(release.contains("needs: [channel, linux, macos, windows-wsl]"));
    assert!(release.contains("needs.channel.outputs.full_platforms == 'true'"));
    assert!(release.contains("needs.channel.outputs.prerelease == 'true' || (needs.macos.result == 'success' && needs.windows-wsl.result == 'success')"));
    assert!(release.contains(
        "needs.channel.outputs.readiness_only == 'false' && needs.linux.result == 'success'"
    ));
    assert!(release.contains("matrix: ${{ fromJSON(needs.channel.outputs.installed) }}"));
    let release_evidence = source("tooling/repository/release/verify-release-evidence.sh");
    assert!(release_evidence.contains("for target in $RELEASE_TARGETS; do"));
    let readiness = source("tooling/repository/release/write-stable-readiness.py");
    assert!(readiness.contains("\"published\": False"));
    assert!(readiness.contains("\"mainChanged\": False"));
    for block in [
        job_block(&release, "linux", "macos"),
        job_block(&release, "macos", "windows-wsl"),
    ] {
        assert_eq!(
            block
                .matches("run: ./tooling/repository/ci/ci-local.sh --checks-only")
                .count(),
            1
        );
    }
}

#[test]
fn every_checkout_job_asserts_the_github_sha_once_before_use() {
    let ci = source(".github/workflows/ci.yml");
    for (job, next_job, assertion) in [
        (
            "exit",
            "audit",
            "test \"$(git rev-parse HEAD)\" = \"$EXITBIND_EXPECTED_SHA\"",
        ),
        (
            "audit",
            "__missing__",
            "test \"$(git rev-parse HEAD)\" = \"$EXITBIND_EXPECTED_SHA\"",
        ),
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

    for (job, next) in [
        ("scope", "readme"),
        ("readme", "exit"),
        ("exit", "audit"),
        ("audit", "__missing__"),
    ] {
        let block = job_block(&ci, job, next);
        assert!(
            block.contains("ref: ${{ inputs.candidate_sha || github.sha }}"),
            "CI {job} must checkout the explicit candidate, including audit"
        );
    }

    let release = source(".github/workflows/release.yml");
    for (job, next_job, assertion) in [
        ("linux", "macos", POSIX_SHA_ASSERTION),
        ("macos", "windows-wsl", POSIX_SHA_ASSERTION),
        ("windows-wsl", "publish", WINDOWS_SHA_ASSERTION),
        ("publish", "refresh-readme-badges", POSIX_SHA_ASSERTION),
        ("installed", "stable-readiness-result", POSIX_SHA_ASSERTION),
        (
            "stable-readiness-result",
            "__missing__",
            POSIX_SHA_ASSERTION,
        ),
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

    let readiness_job = release
        .split_once("  stable-readiness-result:\n")
        .unwrap()
        .1;
    let readiness_checkout = readiness_job.find("actions/checkout@").unwrap();
    let readiness_helper = readiness_job
        .find("run: python3 tooling/repository/release/write-stable-readiness.py")
        .unwrap();
    assert_eq!(readiness_job.matches("actions/checkout@").count(), 1);
    assert!(readiness_checkout < readiness_job.find(POSIX_SHA_ASSERTION).unwrap());
    assert!(readiness_job.find(POSIX_SHA_ASSERTION).unwrap() < readiness_helper);
    assert!(
        readiness_job.contains("uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683")
    );
    assert!(readiness_job.contains("ref: ${{ github.sha }}"));
    assert!(readiness_job.contains("fetch-depth: 1"));
    assert!(readiness_job.contains("persist-credentials: false"));
}

#[test]
fn release_dispatches_readme_badge_refresh_after_publish_with_only_actions_write() {
    // GITHUB_TOKEN-published releases start no `release`/`workflow_run` runs, so the
    // release workflow must request the badge refresh itself, with least privilege.
    let release = source(".github/workflows/release.yml");
    let refresh = job_block(&release, "refresh-readme-badges", "installed");
    assert_eq!(
        refresh,
        concat!(
            "    needs: publish\n",
            "    if: ${{ always() && needs.publish.result == 'success' }}\n",
            "    runs-on: ubuntu-latest\n",
            "    permissions:\n",
            "      actions: write\n",
            "    steps:\n",
            "      - name: Dispatch README badge refresh on main\n",
            "        env:\n",
            "          GH_TOKEN: ${{ github.token }}\n",
            "        run: gh workflow run readme-badges.yml --repo \"$GITHUB_REPOSITORY\" --ref main\n",
            "\n",
        )
    );
    assert_eq!(release.matches("gh workflow run").count(), 1);
    assert_eq!(release.matches("actions: write").count(), 1);
    let publish = job_block(&release, "publish", "refresh-readme-badges");
    assert!(publish.contains("    permissions:\n      contents: write\n    steps:\n"));
    assert!(!publish.contains("actions: write"));
    let badges = source(".github/workflows/readme-badges.yml");
    assert!(badges.contains("\n  workflow_dispatch:\n"));
    assert!(badges.contains("          ref: main\n"));
}

#[test]
fn paired_privileged_plan_requires_main_before_checkout() {
    let paired = source(".github/workflows/paired-prerelease.yml");
    let plan = job_block(&paired, "plan", "check");
    let admission = plan
        .lines()
        .find(|line| line.trim_start().starts_with("if:"))
        .unwrap();
    assert_eq!(
        admission.trim(),
        concat!(
            "if: github.ref == 'refs/heads/main' && (",
            "github.event_name == 'workflow_dispatch' || (",
            "github.event.workflow_run.conclusion == 'success' && ",
            "github.event.workflow_run.head_repository.id == github.repository_id))"
        )
    );
    assert!(plan.find(admission).unwrap() < plan.find("actions/checkout@").unwrap());
    assert!(plan.contains("persist-credentials: false"));
}

/// The context-surface inventory is a measurement, not a gate. It must report
/// the same numbers from any working directory — otherwise a later context
/// audit would compare against an environment, not against the repository —
/// and it must not print where the repository happens to be checked out.
#[cfg(unix)]
#[test]
fn context_surface_inventory_is_reproducible_and_path_free() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tooling/repository/ci/context-surface-inventory.sh");
    let run = |cwd: &Path| {
        let output = support::git_topology::command("sh")
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

    let agents = fs::read(root.join("AGENTS.md")).unwrap();
    let row = here
        .lines()
        .find(|line| line.split_whitespace().next() == Some("AGENTS.md"))
        .expect("inventory omits the source-reading rules");
    let fields: Vec<_> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 4, "unexpected AGENTS row: {row}");
    assert_eq!(fields[1].parse::<usize>().unwrap(), agents.len());
    assert_eq!(
        fields[2].parse::<usize>().unwrap(),
        agents.iter().filter(|byte| **byte == b'\n').count()
    );
    assert_eq!(fields[3], "source-reading");

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

#[cfg(unix)]
fn run_inventory_validator(
    name: &str,
    args: &[std::ffi::OsString],
    marker: &Path,
) -> std::process::Output {
    let script = source("tooling/repository/ci/context-surface-inventory.sh");
    let validators = script
        .split_once("# BEGIN inventory receipt validators\n")
        .expect("inventory validators missing")
        .1
        .split_once("# END inventory receipt validators")
        .expect("inventory validators not closed")
        .0;
    let call = if name == "validate_inventory_submit" {
        "receipt=$(validate_inventory_submit \"$@\")\ntest -n \"$receipt\"".to_owned()
    } else {
        format!("{name} \"$@\"")
    };
    let shell = format!("set -e\n{validators}\n{call}\nprintf reached > \"$INVENTORY_MARKER\"\n");
    support::git_topology::command("sh")
        .arg("-c")
        .arg(shell)
        .arg("inventory-validator")
        .args(args)
        .env("INVENTORY_MARKER", marker)
        .output()
        .unwrap()
}

#[cfg(unix)]
fn assert_inventory_validator(
    name: &str,
    args: &[std::ffi::OsString],
    marker: &Path,
    expected: bool,
) -> std::process::Output {
    if marker.exists() {
        fs::remove_file(marker).unwrap();
    }
    let output = run_inventory_validator(name, args, marker);
    assert_eq!(output.status.success(), expected, "{output:?}");
    assert_eq!(marker.exists(), expected, "{output:?}");
    output
}

/// A held, incomplete or foreign result must not advance to the next check.
#[cfg(unix)]
#[test]
fn inventory_receipts_fail_closed_before_following_actions() {
    use serde_json::json;

    let root = support::temp("inventory-receipt");
    let path = root.join("receipt.json");
    let marker = root.join("next-action");
    let work = "smw_fixture";
    let assignment = "sma_fixture";
    let event = "a".repeat(64);
    let valid = json!({"work":work,"assignment":assignment,"status":"recorded",
        "effect":"recorded","outcome":"completed","eventSha256":event,"event":{"action":"submit",
        "outcome":"completed","eventSha256":event}});
    let mut held = valid.clone();
    held["status"] = json!("held");
    held["effect"] = json!("held");
    held["event"] = serde_json::Value::Null;
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("event");
    let mut foreign_work = valid.clone();
    foreign_work["work"] = json!("smw_foreign");
    let mut foreign_assignment = valid.clone();
    foreign_assignment["assignment"] = json!("sma_foreign");
    let mut mismatched_event = valid.clone();
    mismatched_event["eventSha256"] = json!("f".repeat(64));
    let args = vec![
        path.as_os_str().to_owned(),
        work.into(),
        assignment.into(),
        "completed".into(),
    ];
    for (value, accepted) in [
        (valid, true),
        (held, false),
        (missing, false),
        (foreign_work, false),
        (foreign_assignment, false),
        (mismatched_event, false),
    ] {
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_inventory_validator("validate_inventory_submit", &args, &marker, accepted);
    }

    let valid = json!({"work":work,"status":"recorded","effect":"recorded",
        "eventSha256":"b".repeat(64),"targetEventSha256":event,"result":{"kind":"exit","code":0},
        "event":{"action":"check","eventSha256":"b".repeat(64),
        "targetEventSha256":event,"result":{"kind":"exit","code":0}}});
    let mut wrong_target = valid.clone();
    wrong_target["event"]["targetEventSha256"] = json!("c".repeat(64));
    let mut failed_check = valid.clone();
    failed_check["event"]["result"]["code"] = json!(1);
    let mut ambiguous_check = valid.clone();
    ambiguous_check["event"]["result"]["code"] = json!(false);
    ambiguous_check["result"]["code"] = json!(false);
    let args = vec![path.as_os_str().to_owned(), work.into(), event.into()];
    for (value, accepted) in [
        (valid, true),
        (wrong_target, false),
        (failed_check, false),
        (ambiguous_check, false),
    ] {
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_inventory_validator("validate_inventory_check", &args, &marker, accepted);
    }
}

/// Recovery uses observed growing history and checked events, never a copied
/// or foreign envelope merely to reopen the governor.
#[cfg(unix)]
#[test]
fn inventory_evidence_rejects_duplicate_and_foreign_observations() {
    use serde_json::{json, Value};

    let root = support::temp("inventory-evidence");
    let marker = root.join("next-action");
    let detail_path = root.join("detail.json");
    let history = root.join("history.jsonl");
    let next1 = root.join("next-1.json");
    let next2 = root.join("next-2.json");
    let ledger1 = root.join("ledger-1.jsonl");
    let ledger2 = root.join("ledger-2.jsonl");
    let submit1 = root.join("submit-1.json");
    let submit2 = root.join("submit-2.json");
    let check1 = root.join("check-1.json");
    let check2 = root.join("check-2.json");
    let work = "smw_fixture";
    let binary_file = root.join("inventory-bin");
    let binary_alias = root.join("inventory-bin-alias");
    let config_file = root.join("exitbind.json");
    let config_alias = root.join("inventory-config-alias");
    fs::write(&binary_file, b"fixture").unwrap();
    fs::write(&config_file, b"{}\n").unwrap();
    std::os::unix::fs::symlink(&binary_file, &binary_alias).unwrap();
    std::os::unix::fs::symlink(&config_file, &config_alias).unwrap();
    let binary = binary_file.to_str().unwrap();
    let config = config_file.to_str().unwrap().to_owned();
    let detail = json!({"work":work,"complete":true,"valid":true,"binding":"b3",
        "recipient":{"role":"worker","assignment":"sma_3"},
        "actionForms":{"work":work,"assignment":"sma_3","binding":"b3",
          "choiceActor":{"role":"lead"},"recovery":{"state":"replan_required"},
          "choices":[{"label":"evidence","placeholders":["ARTIFACT_ROOT","ARTIFACT_PATH"],
            "inputConstraints":{"ARTIFACT_ROOT":{"allowedValues":["product","state"]}},
            "command":{"sameConfigRequired":false,"sameExecutableRequired":false,
              "argv":[binary,"work","evidence",work,"sma_3","--artifact-root","<ARTIFACT_ROOT>",
                "--artifact","<ARTIFACT_PATH>","--config",config]}}]},
        "sections":{"assignment":{"assignment":{"context":{"loop":{"state":"replan_required",
          "spent":2,"noInformationStreak":2,"evidence":[],"seenEvidenceSha256":[]}}}}}});
    let packet = |assignment: &str| {
        json!({"work":work,"next":{"role":"worker","assignment":assignment,
        "packet":{"context":{"expansions":[{}],"evidence":[]},"upstreamArtifacts":[]}}})
    };
    let submit = |assignment: &str, hash: &str| {
        json!({"work":work,"assignment":assignment,
        "status":"recorded","effect":"recorded","outcome":"completed","eventSha256":hash,
        "event":{"action":"submit","outcome":"completed","eventSha256":hash}})
    };
    let check = |target: &str, hash: &str| {
        json!({"work":work,"status":"recorded","effect":"recorded",
        "eventSha256":hash,"targetEventSha256":target,"result":{"kind":"exit","code":0},
        "event":{"action":"check","eventSha256":hash,"targetEventSha256":target,
        "result":{"kind":"exit","code":0}}})
    };
    let write_json = |path: &Path, value: &Value| {
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    };
    let worker1 = "a".repeat(64);
    let worker2 = "b".repeat(64);
    let checked1 = "c".repeat(64);
    let checked2 = "d".repeat(64);
    let first = json!({"historyEvents":1,"routineExpansionCount":1,"routineFieldsBounded":true});
    let second = json!({"historyEvents":2,"routineExpansionCount":1,"routineFieldsBounded":true});
    write_json(&detail_path, &detail);
    write_json(&next1, &packet("sma_1"));
    write_json(&next2, &packet("sma_2"));
    fs::write(&ledger1, b"{}\n").unwrap();
    fs::write(&ledger2, b"{}\n{}\n").unwrap();
    fs::write(&history, format!("{first}\n{second}\n")).unwrap();
    write_json(&submit1, &submit("sma_1", &worker1));
    write_json(&submit2, &submit("sma_2", &worker2));
    write_json(&check1, &check(&worker1, &checked1));
    write_json(&check2, &check(&worker2, &checked2));
    let route_args = vec![
        detail_path.as_os_str().to_owned(),
        binary.into(),
        root.as_os_str().to_owned(),
        work.into(),
    ];
    assert_inventory_validator(
        "validate_inventory_evidence_route",
        &route_args,
        &marker,
        true,
    );
    let mut aliases = detail.clone();
    aliases["actionForms"]["choices"][0]["command"]["argv"][0] =
        json!(binary_alias.to_str().unwrap());
    aliases["actionForms"]["choices"][0]["command"]["argv"][10] =
        json!(config_alias.to_str().unwrap());
    write_json(&detail_path, &aliases);
    assert_inventory_validator(
        "validate_inventory_evidence_route",
        &route_args,
        &marker,
        true,
    );
    let mut foreign_route = detail.clone();
    foreign_route["actionForms"]["choices"][0]["command"]["argv"][0] = json!("/foreign");
    write_json(&detail_path, &foreign_route);
    assert_inventory_validator(
        "validate_inventory_evidence_route",
        &route_args,
        &marker,
        false,
    );
    let mut absent_route = detail.clone();
    absent_route["actionForms"]["choices"] = json!([]);
    write_json(&detail_path, &absent_route);
    assert_inventory_validator(
        "validate_inventory_evidence_route",
        &route_args,
        &marker,
        false,
    );
    let mut foreign_config = detail.clone();
    foreign_config["actionForms"]["choices"][0]["command"]["argv"][10] =
        json!("/foreign/exitbind.json");
    write_json(&detail_path, &foreign_config);
    assert_inventory_validator(
        "validate_inventory_evidence_route",
        &route_args,
        &marker,
        false,
    );
    let mut stale_binding = detail.clone();
    stale_binding["actionForms"]["binding"] = json!("stale");
    write_json(&detail_path, &stale_binding);
    assert_inventory_validator(
        "validate_inventory_evidence_route",
        &route_args,
        &marker,
        false,
    );
    let mut duplicate_placeholder = detail.clone();
    duplicate_placeholder["actionForms"]["choices"][0]["placeholders"] =
        json!(["ARTIFACT_ROOT", "ARTIFACT_PATH", "ARTIFACT_PATH"]);
    write_json(&detail_path, &duplicate_placeholder);
    assert_inventory_validator(
        "validate_inventory_evidence_route",
        &route_args,
        &marker,
        false,
    );
    write_json(&detail_path, &detail);

    let args = vec![
        work.into(),
        detail_path.as_os_str().to_owned(),
        history.as_os_str().to_owned(),
        next1.as_os_str().to_owned(),
        next2.as_os_str().to_owned(),
        ledger1.as_os_str().to_owned(),
        ledger2.as_os_str().to_owned(),
        submit1.as_os_str().to_owned(),
        submit2.as_os_str().to_owned(),
        check1.as_os_str().to_owned(),
        check2.as_os_str().to_owned(),
    ];
    let output = assert_inventory_validator("inventory_history_evidence", &args, &marker, true);
    let observation: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(observation["work"], work);
    assert_eq!(observation["samples"], json!([first, second]));
    assert_eq!(observation["checks"][1]["targetEventSha256"], worker2);

    fs::write(&history, format!("{first}\n{first}\n")).unwrap();
    fs::write(&ledger2, b"{}\n").unwrap();
    assert_inventory_validator("inventory_history_evidence", &args, &marker, false);
    fs::write(&history, format!("{first}\n{second}\n")).unwrap();
    fs::write(&ledger2, b"{}\n{}\n").unwrap();
    let mut seen = detail.clone();
    seen["sections"]["assignment"]["assignment"]["context"]["loop"]["seenEvidenceSha256"] =
        json!(["e".repeat(64)]);
    write_json(&detail_path, &seen);
    assert_inventory_validator("inventory_history_evidence", &args, &marker, false);
    let mut ambiguous_spent = detail.clone();
    ambiguous_spent["sections"]["assignment"]["assignment"]["context"]["loop"]["spent"] =
        json!(2.0);
    write_json(&detail_path, &ambiguous_spent);
    assert_inventory_validator("inventory_history_evidence", &args, &marker, false);
    write_json(&detail_path, &detail);
    let mut foreign_check = check(&worker2, &checked2);
    foreign_check["event"]["targetEventSha256"] = json!("f".repeat(64));
    write_json(&check2, &foreign_check);
    assert_inventory_validator("inventory_history_evidence", &args, &marker, false);
    write_json(&check2, &check(&worker2, &checked2));
    let mut held_worker = submit("sma_2", &worker2);
    held_worker["effect"] = json!("held");
    write_json(&submit2, &held_worker);
    assert_inventory_validator("inventory_history_evidence", &args, &marker, false);
}

/// This explicit maintainer check builds the measured Source and exercises the
/// complete three-attempt fixture; the ordinary inventory check stays cheap.
#[cfg(unix)]
#[test]
#[ignore = "builds the measured product and runs the complete inventory fixture"]
fn context_surface_json_measures_real_packets_and_history() {
    use sha2::{Digest, Sha256};

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let git = |args: &[&str]| {
        let output = support::git_topology::command("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "git failed: {output:?}");
        output.stdout
    };
    let revision = String::from_utf8(git(&["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let before = git(&["diff", "--binary", "--", "."]);
    let output = support::git_topology::command("sh")
        .arg(root.join("tooling/repository/ci/context-surface-inventory.sh"))
        .arg("--json")
        .current_dir(root)
        .output()
        .unwrap();
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "inventory failed: {output:?}");
    assert_eq!(git(&["diff", "--binary", "--", "."]), before);

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let agents = report["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == "AGENTS.md")
        .expect("missing source-reading rules");
    let bytes = fs::read(root.join("AGENTS.md")).unwrap();
    assert_eq!(agents["stage"], "source-reading");
    assert_eq!(agents["projection"], "explicit");
    assert!(agents["duplicateOf"].is_null());
    assert_eq!(agents["after"]["bytes"], bytes.len());
    assert_eq!(
        agents["after"]["lines"],
        bytes.iter().filter(|byte| **byte == b'\n').count()
    );
    assert_eq!(
        agents["after"]["sha256"],
        format!("{:x}", Sha256::digest(bytes))
    );

    let dynamic = &report["dynamic"];
    assert_eq!(dynamic["sourceMode"], "worktree");
    assert_eq!(dynamic["sourceRevision"], revision);
    assert_eq!(dynamic["sourceIdentity"]["mode"], "worktree");
    assert_eq!(dynamic["sourceIdentity"]["revision"], revision);
    assert_eq!(
        dynamic["trackedDiffSha256"],
        format!("{:x}", Sha256::digest(before))
    );
    let binary_sha = dynamic["binarySha256"].as_str().unwrap();
    assert_eq!(binary_sha.len(), 64);
    assert!(binary_sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
    for role in ["lead", "worker", "reviewer"] {
        let packet = &dynamic["packets"][role];
        assert!(packet["bytes"].as_u64().unwrap() > 0, "{role}: {packet}");
        assert!(packet["expansionCount"].is_u64(), "{role}: {packet}");
    }
    let history = dynamic["historyGrowth"].as_array().unwrap();
    assert_eq!(history.len(), 3);
    let mut previous = 0;
    for item in history {
        let events = item["historyEvents"].as_u64().unwrap();
        assert!(events > previous, "history did not grow: {history:?}");
        assert!(item["routineExpansionCount"].is_u64());
        assert_eq!(item["routineFieldsBounded"], true);
        previous = events;
    }
    assert!(dynamic["tokenTelemetry"].is_null());
    assert!(dynamic["humanTelemetry"].is_null());
}
