use serde_json::{json, Value};
mod support;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn temp(label: &str) -> PathBuf {
    fs::canonicalize(support::temp(label)).unwrap()
}

fn invoke(arguments: &[&str], bindings: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .env("SOULMATE_BINDINGS_DIR", bindings)
        .args(arguments)
        .output()
        .unwrap()
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn git(root: &Path, arguments: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap()
}

fn clean_git_product(root: &Path) {
    assert!(git(root, &["init", "-q"]).status.success());
    assert!(git(root, &["config", "user.name", "Soulmate Test"])
        .status
        .success());
    assert!(git(
        root,
        &[
            "config",
            "user.email",
            "soulmate-test@users.noreply.github.com"
        ]
    )
    .status
    .success());
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
    assert!(git(root, &["add", "src/lib.rs"]).status.success());
    assert!(git(root, &["commit", "-qm", "fixture"]).status.success());
    assert!(git(root, &["status", "--porcelain"]).stdout.is_empty());
}

#[test]
fn local_mode_separates_control_product_state_and_preserves_git_status() {
    let base = temp("local-layout");
    let product = base.join("product");
    let control = base.join("control");
    let state = base.join("state");
    let bindings = base.join("bindings");
    for path in [&product, &control, &state] {
        fs::create_dir(path).unwrap();
    }
    clean_git_product(&product);

    let implicit = invoke(&["init", "--root", product.to_str().unwrap()], &bindings);
    assert!(!implicit.status.success());
    assert!(output_text(&implicit).contains("requires explicit --mode"));
    assert!(!product.join("soulmate.json").exists());

    let initialized = invoke(
        &[
            "init",
            "--mode",
            "local",
            "--project-id",
            "layout_fixture",
            "--root",
            product.to_str().unwrap(),
            "--control-root",
            control.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(
        initialized.status.success(),
        "{}",
        output_text(&initialized)
    );
    let initialized_text = output_text(&initialized);
    let skill = control.join(".agents/skills/soulmate/SKILL.md");
    assert!(skill.is_file());
    assert!(initialized_text.contains(skill.to_str().unwrap()));
    assert!(initialized_text.contains(control.join("soulmate.json").to_str().unwrap()));
    assert!(
        initialized_text
            .find("Bounded setup facts for your existing root agent")
            .unwrap()
            < initialized_text.find("CLI reference").unwrap()
    );
    let brief = initialized_text.find("soulmate brief").unwrap();
    let work = initialized_text.find("soulmate work begin").unwrap();
    let check = initialized_text.find("soulmate check").unwrap();
    assert!(brief < work && work < check);
    assert!(git(&product, &["status", "--porcelain"]).stdout.is_empty());
    assert!(!product.join("soulmate.json").exists());
    assert!(!product.join(".soulmate").exists());
    assert!(control.join("soulmate.json").is_file());
    assert!(control.join("soulmate/agents/worker.md").is_file());
    let reviewer = fs::read_to_string(control.join("soulmate/agents/reviewer.md")).unwrap();
    assert!(reviewer.contains("file:line reference"));
    assert!(reviewer.contains("mechanical inventory"));
    assert!(reviewer.contains("agent-consumer"));
    assert!(reviewer.contains("mechanical gate"));
    assert!(reviewer.contains("role-scoped evidence"));
    for path in [
        "soulmate/agents",
        "soulmate/boundaries",
        "soulmate/policies",
        "soulmate/harness",
    ] {
        assert!(control.join(path).is_dir(), "{path}");
    }
    assert!(!control.join(".agents/profiles").exists());
    assert!(state.join(".soulmate/.gitignore").is_file());
    for path in [
        ".soulmate/runs",
        ".soulmate/memory",
        ".soulmate/artifacts",
        ".soulmate/receipts",
        ".soulmate/away",
        ".soulmate/locks",
    ] {
        assert!(state.join(path).is_dir(), "{path}");
    }

    let config = control.join("soulmate.json");
    let config_text = fs::read_to_string(&config).unwrap();
    let config_value: Value = serde_json::from_str(&config_text).unwrap();
    assert_eq!(
        config_value["agents"]["worker"]["profile"],
        "soulmate/agents/worker.md"
    );
    assert!(!config_text.contains(product.to_str().unwrap()));
    assert!(!config_text.contains(state.to_str().unwrap()));
    let binding = bindings.join("layout_fixture.json");
    assert!(binding.is_file());
    let binding_value: Value =
        serde_json::from_str(&fs::read_to_string(&binding).unwrap()).unwrap();
    assert_eq!(binding_value["controlRoot"], control.to_str().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&binding).unwrap().permissions().mode() & 0o077,
            0
        );
    }

    let checked = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(checked.status.success(), "{}", output_text(&checked));

    let mut hook = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .env("SOULMATE_BINDINGS_DIR", &bindings)
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin
        .as_mut()
        .unwrap()
        .write_all(
            serde_json::to_string(&serde_json::json!({
                "hook_event_name":"SubagentStart",
                "cwd":product,
                "agent_name":"worker"
            }))
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
    let hook = hook.wait_with_output().unwrap();
    assert!(hook.status.success(), "{}", output_text(&hook));
    let hook: Value = serde_json::from_slice(&hook.stdout).unwrap();
    let context = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("# Worker"));

    let mut legacy_binding = binding_value.clone();
    legacy_binding
        .as_object_mut()
        .unwrap()
        .remove("controlRoot");
    fs::write(
        &binding,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&legacy_binding).unwrap()
        ),
    )
    .unwrap();
    let rebound = invoke(
        &[
            "bind",
            "--config",
            config.to_str().unwrap(),
            "--root",
            product.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(rebound.status.success(), "{}", output_text(&rebound));
    let upgraded: Value = serde_json::from_str(&fs::read_to_string(&binding).unwrap()).unwrap();
    assert_eq!(upgraded["controlRoot"], control.to_str().unwrap());

    let started = invoke(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "bounded",
            "--ledger",
            ".soulmate/run.jsonl",
            "--config",
            config.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(started.status.success(), "{}", output_text(&started));
    let ledger = state.join(".soulmate/run.jsonl");
    let ledger_text = fs::read_to_string(ledger).unwrap();
    assert!(!ledger_text.contains(product.to_str().unwrap()));
    assert!(!ledger_text.contains(state.to_str().unwrap()));
    assert!(git(&product, &["status", "--porcelain"]).stdout.is_empty());

    fs::write(
        state.join(".soulmate/artifacts/lead-stage-1-attempt-1.md"),
        "scope\n",
    )
    .unwrap();
    let lead = invoke(
        &[
            "run",
            "submit",
            "lead",
            ".soulmate/run.jsonl",
            "--outcome",
            "scoped",
            "--artifact",
            ".soulmate/artifacts/lead-stage-1-attempt-1.md",
            "--artifact-root",
            "state",
            "--config",
            config.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(lead.status.success(), "{}", output_text(&lead));

    for (agent, outcome) in [
        ("worker", "completed"),
        ("reviewer", "approved"),
        ("lead", "accepted"),
    ] {
        let submitted = invoke(
            &[
                "run",
                "submit",
                agent,
                ".soulmate/run.jsonl",
                "--outcome",
                outcome,
                "--artifact",
                "src/lib.rs",
                "--config",
                config.to_str().unwrap(),
            ],
            &bindings,
        );
        assert!(submitted.status.success(), "{}", output_text(&submitted));
    }
    assert!(git(&product, &["status", "--porcelain"]).stdout.is_empty());

    let second_control = base.join("second-control");
    let second_state = base.join("second-state");
    fs::create_dir(&second_control).unwrap();
    fs::create_dir(&second_state).unwrap();
    let collision = invoke(
        &[
            "init",
            "--mode",
            "local",
            "--project-id",
            "layout_fixture",
            "--root",
            product.to_str().unwrap(),
            "--control-root",
            second_control.to_str().unwrap(),
            "--state-root",
            second_state.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(!collision.status.success());
    assert!(fs::read_dir(&second_control).unwrap().next().is_none());
    assert!(fs::read_dir(&second_state).unwrap().next().is_none());

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn portable_mode_refuses_a_tracked_private_run_ledger() {
    let base = temp("portable-preflight");
    let root = base.join("product");
    let bindings = base.join("bindings");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bindings).unwrap();
    clean_git_product(&root);
    let initialized = invoke(
        &[
            "init",
            "--mode",
            "portable",
            "--root",
            root.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(
        initialized.status.success(),
        "{}",
        output_text(&initialized)
    );
    assert!(git(
        &root,
        &[
            "add",
            "soulmate.json",
            ".agents",
            ".claude",
            ".soulmate/.gitignore"
        ]
    )
    .status
    .success());
    assert!(git(&root, &["commit", "-qm", "portable control"])
        .status
        .success());
    let config = root.join("soulmate.json");
    assert!(invoke(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "bounded",
            "--ledger",
            ".soulmate/run.jsonl",
            "--config",
            config.to_str().unwrap(),
        ],
        &bindings,
    )
    .status
    .success());
    assert!(git(&root, &["add", "-f", ".soulmate/run.jsonl"])
        .status
        .success());
    let before = fs::read(root.join(".soulmate/run.jsonl")).unwrap();
    let refused = invoke(
        &[
            "run",
            "submit",
            "lead",
            ".soulmate/run.jsonl",
            "--outcome",
            "scoped",
            "--artifact",
            "src/lib.rs",
            "--config",
            config.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(!refused.status.success());
    assert!(output_text(&refused).contains("tracked or staged"));
    assert_eq!(fs::read(root.join(".soulmate/run.jsonl")).unwrap(), before);

    let status: Value = serde_json::from_slice(
        &invoke(
            &[
                "run",
                "inspect",
                ".soulmate/run.jsonl",
                "--config",
                config.to_str().unwrap(),
            ],
            &bindings,
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(status["status"], "running");
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn git_marker_without_git_fails_with_the_missing_dependency() {
    let base = temp("missing-git");
    let root = base.join("product");
    let bindings = base.join("bindings");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bindings).unwrap();
    fs::create_dir(root.join(".git")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .env("SOULMATE_BINDINGS_DIR", &bindings)
        .env("PATH", "/nonexistent")
        .args(["init", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output_text(&output).contains("Git executable not found on PATH"));
    assert!(!root.join("soulmate.json").exists());

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn empty_starter_is_valid_but_warns_before_project_scoped_work() {
    let base = temp("empty-starter-warning");
    let root = base.join("project");
    let bindings = base.join("bindings");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bindings).unwrap();

    let initialized = invoke(&["init", "--root", root.to_str().unwrap()], &bindings);
    assert!(
        initialized.status.success(),
        "{}",
        output_text(&initialized)
    );
    let init_text = output_text(&initialized);
    assert!(init_text.contains("empty starter boundary"));
    assert!(init_text.contains("project files or commands"));

    let config = root.join("soulmate.json");
    let checked = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(checked.status.success(), "{}", output_text(&checked));
    let value: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(value["valid"], true);
    let warning = value["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|warning| warning["classification"] == "empty_starter_boundary")
        .expect("empty starter warning classification");
    assert!(warning["detail"]
        .as_str()
        .unwrap()
        .contains("project files"));

    let human = invoke(&["check", "--config", config.to_str().unwrap()], &bindings);
    assert!(human.status.success(), "{}", output_text(&human));
    let human_text = output_text(&human);
    let warning_line = "warning: empty starter boundary: review observe, write, and commands before tasks needing project files or commands; empty declarations do not grant host permission or establish task readiness";
    assert!(human_text.lines().any(|line| line == warning_line));
    assert!(!human_text.contains("warning: \"empty starter boundary"));

    let mut configured: Value =
        serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    configured["agents"]["worker"]["observe"] = json!(["src"]);
    fs::write(
        &config,
        format!("{}\n", serde_json::to_string_pretty(&configured).unwrap()),
    )
    .unwrap();
    let nonempty = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(nonempty.status.success(), "{}", output_text(&nonempty));
    let nonempty: Value = serde_json::from_slice(&nonempty.stdout).unwrap();
    assert!(!nonempty["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning["classification"] == "empty_starter_boundary"));
    let refreshed = invoke(
        &["init", "--refresh-skills", "--root", root.to_str().unwrap()],
        &bindings,
    );
    assert!(refreshed.status.success(), "{}", output_text(&refreshed));
    assert!(!output_text(&refreshed).contains("empty starter boundary"));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn migrations_preserve_local_mode_and_historical_ledger_bytes() {
    let base = temp("local-migration-invariants");
    let product = base.join("product");
    let control = base.join("control");
    let state = base.join("state");
    let bindings = base.join("bindings");
    for path in [&product, &control, &state, &bindings] {
        fs::create_dir(path).unwrap();
    }
    let initialized = invoke(
        &[
            "init",
            "--mode",
            "local",
            "--project-id",
            "migration_invariants",
            "--root",
            product.to_str().unwrap(),
            "--control-root",
            control.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(
        initialized.status.success(),
        "{}",
        output_text(&initialized)
    );
    let config = control.join("soulmate.json");
    let historical = b"historical ledger bytes must remain in place\n";
    let ledger = state.join(".soulmate/runs/resumable.jsonl");
    fs::write(&ledger, historical).unwrap();

    let canonical_profile = control.join("soulmate/agents/worker.md");
    let legacy_profile = control.join(".agents/profiles/worker.md");
    fs::create_dir_all(legacy_profile.parent().unwrap()).unwrap();
    fs::rename(canonical_profile, legacy_profile).unwrap();
    let mut config_value: Value =
        serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    config_value["agents"]["worker"]["profile"] = json!(".agents/profiles/worker.md");
    fs::write(
        &config,
        format!("{}\n", serde_json::to_string_pretty(&config_value).unwrap()),
    )
    .unwrap();
    let layout = invoke(
        &[
            "migrate",
            "layout",
            "--apply",
            "--config",
            config.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(layout.status.success(), "{}", output_text(&layout));
    let after_layout: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(after_layout["project"]["mode"], "local");
    assert_eq!(fs::read(&ledger).unwrap(), historical);

    for path in [
        "soulmate/boundaries",
        "soulmate/policies",
        "soulmate/harness",
        ".soulmate/memory",
        ".soulmate/artifacts",
        ".soulmate/receipts",
        ".soulmate/away",
        ".soulmate/locks",
    ] {
        fs::remove_dir(control.join(path))
            .or_else(|_| fs::remove_dir(state.join(path)))
            .unwrap();
    }
    let manifest = control.join("harness-manifest.json");
    fs::write(&manifest, b"legacy manifest bytes\n").unwrap();
    let paths = invoke(
        &[
            "migrate",
            "paths",
            "--apply",
            "--config",
            config.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(paths.status.success(), "{}", output_text(&paths));
    let after_paths: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(after_paths["project"]["mode"], "local");
    assert_eq!(fs::read(&ledger).unwrap(), historical);
    assert_eq!(fs::read(&manifest).unwrap(), b"legacy manifest bytes\n");
    assert!(!product.join(".soulmate").exists());
    fs::remove_dir_all(base).unwrap();
}
