//! R55 F3 (FR54-10): `setup --lessons-from` in a local-mode checkout writes
//! the carried lesson sources under the product root through the one private
//! writer, so `git status` lists none of them and the privacy gate's untracked
//! enumeration (`git ls-files --others --exclude-standard`) never reaches a
//! carried fact. R54 failed the privacy gate on exactly such a file.
use super::lesson_carry::{accept, previous_project};
use super::support;
use serde_json::Value;
use std::{fs, path::Path, process::Stdio};

fn git_lines(root: &Path, args: &[&str]) -> Vec<String> {
    let output = support::git_topology::git(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn carried_lessons_in_a_local_mode_checkout_are_never_listed_by_git() {
    let previous = previous_project();
    // A fact naming a private home path: listed, it would fail the gate.
    // (Built at run time so this test source itself stays clean.)
    let fact = format!("Logs live under /{}/fixture-operator/runs.", "home");
    accept(&previous.0, "carry-private", &fact, "2100-01-01T00:00:00Z");
    let base = support::temp("lesson-privacy");
    let product = base.join("product");
    let control = base.join("control");
    // The local-mode state root sits inside an unrelated repository, which
    // the stamped configuration copy must never touch.
    let outer = base.join("outer");
    let state = outer.join("state");
    for dir in [&product, &control, &state] {
        fs::create_dir_all(dir).unwrap();
    }
    support::git_topology::repository(&product);
    support::git_topology::repository(&outer);
    let outer_exclude = outer.join(".git/info/exclude");
    let outer_before = fs::read(&outer_exclude).ok();
    let bin = support::git_topology::git_only_path(&product);
    let from = previous.0.join("exitbind.json");
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .args([
            "setup",
            "--json",
            "--apply",
            "--mode",
            "local",
            "--root",
            product.to_str().unwrap(),
            "--project-id",
            "carry_private",
            "--control-root",
            control.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
            "--scope",
            "worker",
            "--observe",
            "notes",
            "--write",
            "notes",
            "--commands",
            "true",
            "--check-command",
            "true",
            "--goal",
            "Carry the reviewed lessons privately",
            "--review-policy",
            "required",
            "--skip-skills",
            "--lessons-from",
            from.to_str().unwrap(),
        ])
        .current_dir(&product)
        .env("PATH", &bin)
        .env("EXITBIND_BINDINGS_DIR", base.join("bindings"))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::null());
    let output = support::run(&mut command);
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let carried = &report["lessons"]["carried"][0];
    assert_eq!(carried["state"], "accepted", "{report}");
    let source = carried["source"].as_str().unwrap();
    assert!(source.starts_with(".exitbind/lessons/"), "{report}");
    assert!(product.join(source).is_file());
    let stamped = report["lessons"]["stampedConfig"].as_str().unwrap();
    assert!(
        fs::canonicalize(stamped)
            .unwrap()
            .starts_with(fs::canonicalize(&state).unwrap()),
        "{report}"
    );

    let listed = git_lines(
        &product,
        &["status", "--porcelain", "--untracked-files=all"],
    );
    assert!(
        listed.iter().all(|line| !line.contains(".exitbind")),
        "git status lists a carried lesson: {listed:?}"
    );
    let candidates = git_lines(
        &product,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    );
    assert!(
        candidates
            .iter()
            .all(|path| !path.starts_with(".exitbind/")),
        "the privacy gate would scan a carried lesson: {candidates:?}"
    );
    let exclude = fs::read_to_string(product.join(".git/info/exclude")).unwrap();
    assert!(
        exclude.lines().any(|line| line == "/.exitbind/lessons/"),
        "{exclude}"
    );
    assert_eq!(
        fs::read(&outer_exclude).ok(),
        outer_before,
        "the unrelated repository holding the state root is untouched"
    );
    assert!(
        fs::read_to_string(product.join(".gitignore")).is_err(),
        "no tracked ignore file is written"
    );
}
