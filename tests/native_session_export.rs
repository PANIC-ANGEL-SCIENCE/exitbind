use std::io::Write;
use std::process::{Command, Stdio};

mod support;

fn hook(payload: &str, env_file: &std::path::Path) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command.arg("hook-run");
    run_hook(command, payload, env_file)
}

fn run_hook(
    mut command: Command,
    payload: &str,
    env_file: &std::path::Path,
) -> std::process::Output {
    let mut child = command
        .env("CLAUDE_ENV_FILE", env_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
#[test]
fn private_session_export_respects_creation_and_host_owned_existing_files() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = support::temp("native-session-permissions");
    let payload = format!(
        r#"{{"hook_event_name":"SessionStart","session_id":"permission-fixture","cwd":{:?}}}"#,
        root.to_str().unwrap()
    );
    let fresh = root.join("fresh");
    // Set umask in the child only: changing the test process would race its
    // other tests. The new file must be private even with no masking.
    let mut command = Command::new("sh");
    command
        .args(["-c", "umask 000; exec \"$@\"", "session-export-test"])
        .arg(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run");
    assert!(run_hook(command, &payload, &fresh).status.success());
    assert_eq!(
        std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read_to_string(&fresh).unwrap(),
        "export EXITBIND_NATIVE_SESSION_ID=permission-fixture\n"
    );

    let public = root.join("host-public");
    std::fs::write(&public, "export KEPT=1\n").unwrap();
    std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(hook(&payload, &public).status.success());
    assert_eq!(std::fs::read_to_string(&public).unwrap(), "export KEPT=1\n");
    assert_eq!(
        std::fs::metadata(&public).unwrap().permissions().mode() & 0o777,
        0o644
    );

    let private = root.join("host-private");
    std::fs::write(&private, "export KEPT=1\n").unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(hook(&payload, &private).status.success());
    let expected = "export KEPT=1\nexport EXITBIND_NATIVE_SESSION_ID=permission-fixture\n";
    assert_eq!(std::fs::read_to_string(&private).unwrap(), expected);
    let link = root.join("host-link");
    symlink(&private, &link).unwrap();
    assert!(hook(&payload, &link).status.success());
    assert_eq!(std::fs::read_to_string(&private).unwrap(), expected);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn session_start_hook_exposes_host_session_id_to_later_shell_commands() {
    let root = support::temp("native-session-export");
    // Claude Code names a file that does not exist yet.
    let env_file = root.join("claude-env");
    let cwd = root.display().to_string();

    let start = format!(
        r#"{{"hook_event_name":"SessionStart","session_id":"0f1e2d3c-demo","cwd":{cwd:?}}}"#
    );
    assert!(hook(&start, &env_file).status.success());
    let subagent =
        format!(r#"{{"hook_event_name":"SubagentStart","session_id":"other","cwd":{cwd:?}}}"#);
    assert!(hook(&subagent, &env_file).status.success());
    let unsafe_id =
        format!(r#"{{"hook_event_name":"SessionStart","session_id":"a b","cwd":{cwd:?}}}"#);
    assert!(hook(&unsafe_id, &env_file).status.success());

    assert_eq!(
        std::fs::read_to_string(&env_file).unwrap(),
        "export EXITBIND_NATIVE_SESSION_ID=0f1e2d3c-demo\n"
    );
    std::fs::remove_dir_all(&root).unwrap();
}
