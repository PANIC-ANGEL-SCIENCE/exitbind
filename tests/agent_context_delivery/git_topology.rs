//! Native and direct launches use the no-Git option only after classification.
use super::*;
use std::io::Write;

fn invoke(root: &Path, arguments: &[&str], input: &[u8], environment: &[(&str, &Path)]) -> Output {
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(root)
        .args(arguments)
        .args(["--config", "exitbind.json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

fn init(root: &Path) {
    let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--skip-skills", "--root"])
        .arg(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
}

fn fake_codex(root: &Path) -> PathBuf {
    let path = root.join(".exitbind/fake-topology-codex");
    let message = json!({"outcome":"completed","summary":"fixture completed","reason":""});
    let events = [
        json!({"type":"thread.started","thread_id":"topology-thread"}),
        json!({"type":"item.completed","item":{"type":"agent_message","text":message.to_string()}}),
        json!({"type":"turn.completed","status":"completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}),
    ];
    let mut source =
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > .exitbind/topology-argv\ncat >/dev/null\n".to_owned();
    for event in events {
        source.push_str(&format!(
            "printf '%s\\n' '{}'\n",
            event.to_string().replace('\'', "'\\''")
        ));
    }
    fs::write(&path, source).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn arguments(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join(".exitbind/topology-argv"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn activity(root: &Path, executable: &Path, environment: &[(&str, &Path)]) -> Output {
    invoke(
        root,
        &[
            "activity",
            "codex",
            "--json",
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"bounded direct task",
        environment,
    )
}

fn prepare_worker(root: &Path) -> String {
    let begin = invoke(
        root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "bounded topology fixture",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
        &[],
    );
    assert!(begin.status.success(), "{}", text(&begin));
    let begin: Value = serde_json::from_slice(&begin.stdout).unwrap();
    let work = begin["work"].as_str().unwrap().to_owned();
    let assignment = begin["next"]["assignment"].as_str().unwrap();
    let scoped = invoke(
        root,
        &["work", "return", &work, assignment, "--outcome", "scoped"],
        b"scope",
        &[],
    );
    assert!(scoped.status.success(), "{}", text(&scoped));
    work
}

fn act(root: &Path, work: &str, executable: &Path) -> Output {
    invoke(
        root,
        &[
            "work",
            "act",
            work,
            "--json",
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
        &[],
    )
}

fn assert_launch(root: &Path, expect_skip: bool) {
    init(root);
    let executable = fake_codex(root);
    let direct = activity(root, &executable, &[]);
    assert!(direct.status.success(), "{}", text(&direct));
    let direct_record: Value = serde_json::from_slice(&direct.stdout).unwrap();
    assert_eq!(direct_record["status"], "observed");
    let direct_args = arguments(root);
    assert_eq!(direct_args.first().map(String::as_str), Some("exec"));
    assert_eq!(
        direct_args.iter().any(|arg| arg == "--skip-git-repo-check"),
        expect_skip,
        "{direct_args:?}"
    );
    let work = prepare_worker(root);
    let completed = act(root, &work, &executable);
    assert!(completed.status.success(), "{}", text(&completed));
    let work_args = arguments(root);
    assert_eq!(work_args.first().map(String::as_str), Some("exec"));
    assert_eq!(
        work_args.iter().any(|arg| arg == "--skip-git-repo-check"),
        expect_skip,
        "{work_args:?}"
    );
}

#[test]
fn marker_free_work_and_direct_activity_pass_the_flag() {
    let Some(root) = support::marker_free_temp("native-no-git") else {
        return;
    };
    assert_launch(&root, true);
}

#[test]
fn enclosing_repository_and_gitfile_never_pass_the_flag() {
    let normal = support::temp("native-git-repository");
    support::git_topology::repository(&normal);
    assert_launch(&normal, false);

    let enclosing = support::temp("native-enclosing-git");
    support::git_topology::repository(&enclosing);
    let nested = enclosing.join("nested");
    fs::create_dir(&nested).unwrap();
    support::git_topology::assert_worktree(&nested, &enclosing);
    assert_launch(&nested, false);

    let fixture = support::temp("native-gitfile");
    let source = fixture.join("source");
    let gitdir = fixture.join("git-dir");
    fs::create_dir(&source).unwrap();
    let initialized = support::git_topology::command("git")
        .args(["init", "-q", "--separate-git-dir"])
        .arg(&gitdir)
        .arg(&source)
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{}", text(&initialized));
    assert!(fs::metadata(source.join(".git")).unwrap().is_file());
    support::git_topology::assert_worktree(&source, &source);
    assert_launch(&source, false);
}

#[test]
fn malformed_marker_and_unknown_git_failure_never_launch_codex() {
    let Some(root) = support::marker_free_temp("native-bad-git") else {
        return;
    };
    init(&root);
    let executable = fake_codex(&root);
    let work = prepare_worker(&root);
    fs::write(root.join(".git"), "gitdir: missing-directory\n").unwrap();
    let rejected = act(&root, &work, &executable);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert!(!root.join(".exitbind/topology-argv").exists());
    fs::remove_file(root.join(".git")).unwrap();
    let missing = root.join("missing-git-directory");
    let rejected = activity(&root, &executable, &[("GIT_DIR", &missing)]);
    assert!(rejected.status.success(), "{}", text(&rejected));
    let record: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(record["status"], "capture_gap");
    assert!(record["observation"]["error"]
        .as_str()
        .unwrap()
        .contains("Git launch preflight could not confirm"));
    assert!(!root.join(".exitbind/topology-argv").exists());
    // Git prints the ordinary marker-free diagnostic for GIT_WORK_TREE alone;
    // an incomplete explicit location must still fail closed.
    let rejected = activity(&root, &executable, &[("GIT_WORK_TREE", &root)]);
    assert!(rejected.status.success(), "{}", text(&rejected));
    let record: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(record["status"], "capture_gap");
    assert!(record["observation"]["error"]
        .as_str()
        .unwrap()
        .contains("Git launch preflight could not confirm"));
    assert!(!root.join(".exitbind/topology-argv").exists());
    let fake_bin = root.join(".exitbind/fake-git-bin");
    fs::create_dir(&fake_bin).unwrap();
    let fake_git = fake_bin.join("git");
    fs::write(
        &fake_git,
        "#!/bin/sh\nprintf '%s\\n' 'fatal: simulated untrusted Git state' >&2\nexit 128\n",
    )
    .unwrap();
    fs::set_permissions(&fake_git, fs::Permissions::from_mode(0o700)).unwrap();
    let rejected = activity(&root, &executable, &[("PATH", &fake_bin)]);
    assert!(rejected.status.success(), "{}", text(&rejected));
    let record: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(record["status"], "capture_gap");
    assert!(record["observation"]["error"]
        .as_str()
        .unwrap()
        .contains("Git launch preflight could not confirm"));
    assert!(!root.join(".exitbind/topology-argv").exists());
}

#[test]
fn externally_selected_worktree_never_receives_the_bypass() {
    let Some(root) = support::marker_free_temp("native-env-worktree") else {
        return;
    };
    let repository = support::temp("native-env-gitdir");
    support::git_topology::repository(&repository);
    init(&root);
    let executable = fake_codex(&root);
    let gitdir = repository.join(".git");
    let direct = activity(
        &root,
        &executable,
        &[("GIT_DIR", &gitdir), ("GIT_WORK_TREE", &root)],
    );
    assert!(direct.status.success(), "{}", text(&direct));
    let record: Value = serde_json::from_slice(&direct.stdout).unwrap();
    assert_eq!(record["status"], "observed");
    assert!(!arguments(&root)
        .iter()
        .any(|arg| arg == "--skip-git-repo-check"));
}
