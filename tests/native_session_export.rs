use std::io::Write;
use std::process::{Command, Stdio};

mod support;

fn hook(payload: &str, env_file: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run")
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
