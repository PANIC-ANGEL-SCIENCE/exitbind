//! Exercise the real CLI effect boundary, including refusals before writes and
//! replay after a lost reply. Native tools that bypass it are outside its claim.
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
mod support;

fn call(root: &Path, args: &[&str], body: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(body).unwrap();
    child.wait_with_output().unwrap()
}
fn ok(root: &Path, args: &[&str], body: &[u8]) -> Value {
    let output = call(root, args, body);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}
fn project(broad: bool) -> (PathBuf, String, String) {
    let root = support::temp("file-effect");
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--skip-skills", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["worker"]["write"] = if broad {
        json!(["**"])
    } else {
        json!(["src/**"])
    };
    fs::write(config_path, config.to_string()).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    let begin = ok(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "replace a source file",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
        ],
        b"",
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let assignment = begin["next"]["assignment"].as_str().unwrap();
    ok(
        &root,
        &["work", "return", &work, assignment, "--outcome", "scoped"],
        b"Scoped text replacement",
    );
    let next = ok(&root, &["work", "next", &work, "--full"], b"");
    let worker = next["next"]["assignment"].as_str().unwrap().to_owned();
    (root, work, worker)
}
fn permit(root: &Path, work: &str, assignment: &str) {
    assert_eq!(
        ok(
            root,
            &[
                "work",
                "permit",
                work,
                assignment,
                "--operation",
                "source-update"
            ],
            b""
        )["allowed"],
        true
    );
}
fn write(
    root: &Path,
    work: &str,
    assignment: &str,
    file: &str,
    operation: &str,
    expected: &str,
    body: &[u8],
) -> Output {
    call(
        root,
        &[
            "work",
            "write",
            work,
            assignment,
            file,
            "--operation",
            operation,
            "--expected-sha256",
            expected,
            "--json",
        ],
        body,
    )
}

#[test]
fn replacement_requires_a_current_grant_and_replay_never_replaces_changed_bytes() {
    let (root, work, assignment) = project(false);
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/a.txt",
        "write-a",
        "absent",
        b"hello"
    )
    .status
    .success());
    assert!(!root.join("src/a.txt").exists());
    permit(&root, &work, &assignment);
    let output = write(
        &root,
        &work,
        &assignment,
        "src/a.txt",
        "write-a",
        "absent",
        b"hello",
    );
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["effect"], "file-replaced");
    let before = fs::metadata(root.join("src/a.txt"))
        .unwrap()
        .modified()
        .unwrap();
    let replay = write(
        &root,
        &work,
        &assignment,
        "src/a.txt",
        "write-a",
        "absent",
        b"hello",
    );
    assert!(replay.status.success(), "{replay:?}");
    let result: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(result["replay"], true);
    assert_eq!(result["effect"], "no-change");
    assert_eq!(
        fs::metadata(root.join("src/a.txt"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/b.txt",
        "write-a",
        "absent",
        b"hello"
    )
    .status
    .success());
    fs::write(root.join("src/a.txt"), "changed").unwrap();
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/a.txt",
        "write-a",
        "absent",
        b"hello"
    )
    .status
    .success());
    assert_eq!(fs::read(root.join("src/a.txt")).unwrap(), b"changed");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scope_protected_paths_wrong_file_state_and_old_assignment_are_fenced() {
    let (root, work, assignment) = project(false);
    permit(&root, &work, &assignment);
    for file in [
        "other.txt",
        "../escape.txt",
        "src/../escape.txt",
        "/absolute",
        "src//alias.txt",
    ] {
        assert!(
            !write(
                &root,
                &work,
                &assignment,
                file,
                "bad",
                "absent",
                b"forbidden"
            )
            .status
            .success(),
            "{file}"
        );
    }
    fs::write(root.join("src/a.txt"), "existing").unwrap();
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/a.txt",
        "state-conflict",
        "absent",
        b"wrong"
    )
    .status
    .success());
    assert_eq!(fs::read(root.join("src/a.txt")).unwrap(), b"existing");
    ok(
        &root,
        &[
            "work",
            "return",
            &work,
            &assignment,
            "--outcome",
            "completed",
        ],
        b"Completed permitted change",
    );
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/late.txt",
        "late",
        "absent",
        b"late"
    )
    .status
    .success());
    assert!(!root.join("src/late.txt").exists());
    fs::remove_dir_all(root).unwrap();
    let (root, work, assignment) = project(true);
    permit(&root, &work, &assignment);
    let config = fs::read(root.join("exitbind.json")).unwrap();
    for file in [
        "exitbind.json",
        "AGENTS.md",
        ".git/config",
        ".exitbind/evidence.txt",
        "exitbind/agents/worker.md",
        ".claude/settings.json",
    ] {
        assert!(
            !write(
                &root,
                &work,
                &assignment,
                file,
                "control",
                "absent",
                b"forbidden"
            )
            .status
            .success(),
            "{file}"
        );
    }
    assert_eq!(fs::read(root.join("exitbind.json")).unwrap(), config);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn revoked_configuration_new_work_and_uncertain_operation_never_renew_authority() {
    let (root, work, assignment) = project(false);
    permit(&root, &work, &assignment);
    let config_path = root.join("exitbind.json");
    let original = fs::read(&config_path).unwrap();
    let mut config: Value = serde_json::from_slice(&original).unwrap();
    config["agents"]["worker"]["write"] = json!([]);
    fs::write(&config_path, config.to_string()).unwrap();
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/revoked.txt",
        "revoked",
        "absent",
        b"wrong"
    )
    .status
    .success());
    assert!(!root.join("src/revoked.txt").exists());
    fs::write(&config_path, original).unwrap();
    let replacement = write(
        &root,
        &work,
        &assignment,
        "src/a.txt",
        "write-a",
        "absent",
        b"result",
    );
    assert!(replacement.status.success(), "{replacement:?}");
    let effects = root.join(".exitbind/effects");
    let journal = fs::read_dir(effects)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut record: Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
    record["status"] = json!("admitted");
    fs::write(journal, record.to_string()).unwrap();
    let retry = write(
        &root,
        &work,
        &assignment,
        "src/a.txt",
        "write-a",
        "absent",
        b"result",
    );
    assert!(!retry.status.success());
    assert!(String::from_utf8(retry.stdout)
        .unwrap()
        .contains("unresolved admitted effect"));
    assert_eq!(fs::read(root.join("src/a.txt")).unwrap(), b"result");
    let begin = ok(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "a genuinely new task",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
        ],
        b"",
    );
    let new_work = begin["work"].as_str().unwrap();
    assert_ne!(new_work, work);
    assert!(!write(
        &root,
        new_work,
        &assignment,
        "src/new.txt",
        "new",
        "absent",
        b"wrong"
    )
    .status
    .success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_utf8_file_is_replaced_only_from_its_exact_hash() {
    use sha2::{Digest, Sha256};
    let (root, work, assignment) = project(false);
    permit(&root, &work, &assignment);
    let target = root.join("src/existing.txt");
    fs::write(&target, "previous").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o750)).unwrap();
    }
    let expected = format!("{:x}", Sha256::digest(b"previous"));
    let output = write(
        &root,
        &work,
        &assignment,
        "src/existing.txt",
        "replace-existing",
        &expected,
        b"current",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(&target).unwrap(), b"current");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/oversized.txt",
        "oversized",
        "absent",
        &vec![b'x'; 256 * 1024 + 1]
    )
    .status
    .success());
    assert!(!root.join("src/oversized.txt").exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_target_or_parent_and_non_utf8_replacement_are_rejected() {
    use std::os::unix::fs::symlink;
    let (root, work, assignment) = project(false);
    permit(&root, &work, &assignment);
    fs::write(root.join("outside.txt"), "safe").unwrap();
    symlink("../outside.txt", root.join("src/link.txt")).unwrap();
    symlink("..", root.join("src/parent")).unwrap();
    for file in ["src/link.txt", "src/parent/outside.txt"] {
        assert!(!write(
            &root,
            &work,
            &assignment,
            file,
            "symlink",
            "absent",
            b"wrong"
        )
        .status
        .success());
    }
    assert_eq!(fs::read(root.join("outside.txt")).unwrap(), b"safe");
    assert!(!write(
        &root,
        &work,
        &assignment,
        "src/invalid.txt",
        "invalid",
        "absent",
        b"\xff"
    )
    .status
    .success());
    assert!(!root.join("src/invalid.txt").exists());
    fs::remove_dir_all(root).unwrap();
}
