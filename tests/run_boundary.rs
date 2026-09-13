use serde_json::{json, Value};
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn temp(label: &str) -> PathBuf {
    support::temp(label)
}

fn invoke(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .env("SOULMATE_BINDINGS_DIR", root.join("machine-bindings"))
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn fixture() -> (PathBuf, PathBuf) {
    let root = temp("run-boundary");
    let initialized = invoke(
        &root,
        &[
            "init",
            "--mode",
            "portable",
            "--root",
            root.to_str().unwrap(),
        ],
    );
    assert!(initialized.status.success(), "{}", text(&initialized));
    fs::create_dir(root.join("docs")).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".agents/boundaries")).unwrap();
    fs::write(root.join("docs/ADR.md"), "decision\n").unwrap();
    fs::write(root.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(root.join("src/b.rs"), "pub fn b() {}\n").unwrap();
    let config_path = root.join("soulmate.json");
    let mut config: Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["agents"]["worker"]["observe"] = json!(["docs/**", "src/**"]);
    config["agents"]["worker"]["write"] = json!(["src/**"]);
    config["agents"]["reviewer"]["write"] = json!(["builder-approved paths only"]);
    fs::write(
        &config_path,
        format!("{}\n", serde_json::to_string_pretty(&config).unwrap()),
    )
    .unwrap();
    (root, config_path)
}

fn write_manifest(root: &Path, value: Value) -> PathBuf {
    let path = root.join(".agents/boundaries/task.json");
    fs::write(
        &path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    path
}

#[test]
fn run_boundary_narrows_assignment_and_drift_fails_closed() {
    let (root, config) = fixture();
    let manifest = write_manifest(
        &root,
        json!({
            "version": 1,
            "agents": {
                "worker": {
                    "observe": ["docs/ADR.md", "src/a.rs"],
                    "write": ["src/a.rs"]
                }
            }
        }),
    );
    let started = invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "bounded",
            "--ledger",
            ".soulmate/run.jsonl",
            "--boundary",
            ".agents/boundaries/task.json",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(started.status.success(), "{}", text(&started));

    let first: Value = serde_json::from_str(
        fs::read_to_string(root.join(".soulmate/run.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["producer"]["name"], "soulmate");
    assert_eq!(first["producer"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        first["plan"]["boundaryManifest"]["path"],
        ".agents/boundaries/task.json"
    );
    let worker = first["plan"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|stage| stage["agents"].as_array().unwrap())
        .find(|agent| agent["name"] == "worker")
        .unwrap();
    assert_eq!(
        worker["declaredBoundary"]["observe"],
        json!(["docs/ADR.md", "src/a.rs"])
    );
    assert_eq!(worker["declaredBoundary"]["write"], json!(["src/a.rs"]));

    fs::write(manifest, "{\"version\":1,\"agents\":{}}\n").unwrap();
    let drift = invoke(
        &root,
        &[
            "run",
            "next",
            ".soulmate/run.jsonl",
            "--json",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(!drift.status.success());
    let diagnostic: Value = serde_json::from_slice(&drift.stdout).unwrap();
    assert_eq!(diagnostic["classification"], "boundary_drift");
    assert!(diagnostic.get("goal").is_none());

    fs::remove_file(root.join(".agents/boundaries/task.json")).unwrap();
    let missing = invoke(
        &root,
        &[
            "run",
            "next",
            ".soulmate/run.jsonl",
            "--json",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(!missing.status.success());
    let missing_diagnostic: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert!(missing_diagnostic["error"]
        .as_str()
        .unwrap()
        .contains("missing or unreadable"));
    assert!(missing_diagnostic.get("currentBoundarySha256").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_or_widening_boundaries_never_create_a_ledger() {
    let (root, config) = fixture();
    let checked = invoke(
        &root,
        &["check", "--json", "--config", config.to_str().unwrap()],
    );
    let check: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(
        check["warnings"][0]["classification"],
        "boundary_placeholder"
    );
    assert_eq!(check["warnings"][0]["agent"], "reviewer");

    for (name, manifest) in [
        (
            "widening",
            json!({"version":1,"agents":{"worker":{"observe":["README.md"],"write":[]}}}),
        ),
        (
            "escape",
            json!({"version":1,"agents":{"worker":{"observe":["../outside"],"write":[]}}}),
        ),
    ] {
        write_manifest(&root, manifest);
        let ledger = format!(".soulmate/{name}.jsonl");
        let result = invoke(
            &root,
            &[
                "run",
                "start",
                "change",
                "--goal",
                "bounded",
                "--ledger",
                &ledger,
                "--boundary",
                ".agents/boundaries/task.json",
                "--config",
                config.to_str().unwrap(),
            ],
        );
        assert!(!result.status.success());
        assert!(!root.join(&ledger).exists());
    }

    write_manifest(
        &root,
        json!({"version":1,"agents":{"worker":{"observe":["src/a.rs"],"write":[]}}}),
    );
    let accepted = invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "read-only worker",
            "--ledger",
            ".soulmate/empty-write.jsonl",
            "--boundary",
            ".agents/boundaries/task.json",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(accepted.status.success(), "{}", text(&accepted));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_later_stage_observe_path_refuses_before_ledger_creation() {
    let (root, config) = fixture();
    write_manifest(
        &root,
        json!({
            "version": 1,
            "agents": {
                "worker": {
                    "observe": ["src/later-stage.rs"],
                    "write": []
                }
            }
        }),
    );
    let ledger = ".soulmate/missing-later-stage.jsonl";
    let refused = invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "future observe path",
            "--ledger",
            ledger,
            "--boundary",
            ".agents/boundaries/task.json",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(!refused.status.success());
    assert!(text(&refused).contains("observe path does not exist: src/later-stage.rs"));
    assert!(!root.join(ledger).exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn parallel_workers_keep_their_independent_exact_boundaries() {
    let (root, config_path) = fixture();
    let mut config: Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["agents"]["worker_two"] = config["agents"]["worker"].clone();
    config["agents"]["worker_two"]["profile"] = json!("soulmate/agents/worker-two.md");
    fs::write(
        root.join("soulmate/agents/worker-two.md"),
        "Second bounded worker.\n",
    )
    .unwrap();
    config["workflows"]["change"]["workers"] = json!(["worker", "worker_two"]);
    fs::write(
        &config_path,
        format!("{}\n", serde_json::to_string_pretty(&config).unwrap()),
    )
    .unwrap();
    write_manifest(
        &root,
        json!({
            "version":1,
            "agents":{
                "worker":{"observe":["src/a.rs"],"write":["src/a.rs"]},
                "worker_two":{"observe":["src/b.rs"],"write":["src/b.rs"]}
            }
        }),
    );
    assert!(invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "parallel",
            "--ledger",
            ".soulmate/parallel.jsonl",
            "--boundary",
            ".agents/boundaries/task.json",
            "--config",
            config_path.to_str().unwrap()
        ],
    )
    .status
    .success());
    assert!(invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ".soulmate/parallel.jsonl",
            "--outcome",
            "scoped",
            "--artifact",
            "docs/ADR.md",
            "--config",
            config_path.to_str().unwrap()
        ],
    )
    .status
    .success());
    let next = invoke(
        &root,
        &[
            "run",
            "next",
            ".soulmate/parallel.jsonl",
            "--json",
            "--config",
            config_path.to_str().unwrap(),
        ],
    );
    assert!(next.status.success(), "{}", text(&next));
    let value: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(value["assignments"].as_array().unwrap().len(), 2);
    assert_eq!(
        value["assignments"][0]["declaredBoundary"]["write"],
        json!(["src/a.rs"])
    );
    assert_eq!(
        value["assignments"][1]["declaredBoundary"]["write"],
        json!(["src/b.rs"])
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rework_uses_fresh_state_artifacts_and_preserves_prior_bytes() {
    let (root, config) = fixture();
    let artifacts = root.join(".soulmate/artifacts");
    let started = invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "rework",
            "--ledger",
            ".soulmate/rework.jsonl",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(started.status.success(), "{}", text(&started));

    let submit = |agent: &str, outcome: &str, name: &str, contents: &str| {
        fs::write(artifacts.join(name), contents).unwrap();
        let path = format!(".soulmate/artifacts/{name}");
        let output = invoke(
            &root,
            &[
                "run",
                "submit",
                agent,
                ".soulmate/rework.jsonl",
                "--outcome",
                outcome,
                "--artifact",
                &path,
                "--artifact-root",
                "state",
                "--config",
                config.to_str().unwrap(),
            ],
        );
        assert!(output.status.success(), "{}", text(&output));
    };
    submit("lead", "scoped", "lead-stage-1-attempt-1.md", "scope\n");
    submit(
        "worker",
        "completed",
        "worker-stage-2-attempt-1.md",
        "first\n",
    );
    submit(
        "reviewer",
        "rework",
        "reviewer-stage-3-attempt-1.md",
        "rework\n",
    );

    let next = invoke(
        &root,
        &[
            "run",
            "next",
            ".soulmate/rework.jsonl",
            "--json",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(next.status.success(), "{}", text(&next));
    let assignment: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(assignment["assignments"][0]["artifactRootHint"], "state");
    assert_eq!(
        assignment["assignments"][0]["upstreamArtifactsImmutable"],
        true
    );
    assert!(assignment["assignments"][0]["artifactPathHint"]
        .as_str()
        .unwrap()
        .ends_with("worker-stage-2-attempt-2.md"));

    submit(
        "worker",
        "completed",
        "worker-stage-2-attempt-2.md",
        "second\n",
    );
    assert_eq!(
        fs::read_to_string(artifacts.join("worker-stage-2-attempt-1.md")).unwrap(),
        "first\n"
    );
    submit(
        "reviewer",
        "approved",
        "reviewer-stage-3-attempt-2.md",
        "approved\n",
    );
    submit(
        "lead",
        "accepted",
        "lead-stage-4-attempt-2.md",
        "accepted\n",
    );

    fs::write(artifacts.join("worker-stage-2-attempt-1.md"), "changed\n").unwrap();
    let drift = invoke(
        &root,
        &[
            "run",
            "next",
            ".soulmate/rework.jsonl",
            "--json",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(!drift.status.success());
    assert!(text(&drift).contains("artifact drift detected"));
    fs::remove_dir_all(root).unwrap();
}
