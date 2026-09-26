//! A receiving host starts from `work continuation WORK` and completes bind,
//! native-child recording, and readback using only the routes that view gives.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod support;

fn run(root: &Path, argv: &[String], input: &[u8]) -> Output {
    let mut child = Command::new(&argv[0])
        .current_dir(root)
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn exitbind(root: &Path, args: &[&str], input: &[u8]) -> Value {
    let mut argv = vec![env!("CARGO_BIN_EXE_exitbind").to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv.extend(["--config".to_owned(), "exitbind.json".to_owned()]);
    let output = run(root, &argv, input);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fill(template: &Value, values: &[(&str, &str)]) -> Vec<String> {
    template
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| {
            let arg = arg.as_str().unwrap();
            values
                .iter()
                .find(|(name, _)| *name == arg)
                .map_or(arg.to_owned(), |(_, value)| (*value).to_owned())
        })
        .collect()
}

fn project(label: &str, requirements: Value, source: &str) -> (PathBuf, String) {
    let root = support::temp(label);
    let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let begun = exitbind(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Receive work",
            "--check-command",
            "true",
        ],
        b"",
    );
    let work = begun["work"].as_str().unwrap().to_owned();
    let init = json!({"action":"init","sourceRef":"owner:request","sourceText":source,
        "requirements":requirements});
    exitbind(
        &root,
        &["work", "record", &work],
        init.to_string().as_bytes(),
    );
    (root, work)
}

#[test]
fn receiver_binds_and_records_a_child_from_the_continuation_view_alone() {
    let (root, work) = project(
        "receiving-entry",
        json!([{"id":"docs","text":"Document the flag."}]),
        "Document the flag.",
    );
    let view = exitbind(&root, &["work", "continuation", &work], b"");
    let token = view["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(view["receive"]["bind"][5], token.as_str());

    let bind = fill(
        &view["receive"]["bind"],
        &[
            ("HOST", "claude"),
            ("NATIVE_SESSION", "session-a"),
            ("HOST_VERSION", "2.1.283"),
        ],
    );
    let bound = run(&root, &bind, b"");
    assert!(bound.status.success(), "{bound:?}");
    let bound: Value = serde_json::from_slice(&bound.stdout).unwrap();
    let fresh = run(
        &root,
        &bound["nextAction"]["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect::<Vec<_>>(),
        b"",
    );
    assert!(fresh.status.success(), "{fresh:?}");
    let fresh: Value = serde_json::from_slice(&fresh.stdout).unwrap();
    let fresh_token = fresh["mutationContext"]["token"].as_str().unwrap();
    assert_ne!(fresh_token, token);

    let child = fill(
        &fresh["receive"]["child"],
        &[
            ("SHORT_ASSIGNMENT", "check docs"),
            ("FRESH_TOKEN", fresh_token),
            ("CHILD_ID", "agent-1"),
        ],
    );
    let recorded = run(&root, &child, "README lacks the flag.\n".as_bytes());
    assert!(recorded.status.success(), "{recorded:?}");

    let readback = exitbind(&root, &["work", "continuation", &work], b"");
    assert_eq!(readback["binding"]["session"], "session-a");
    assert_eq!(readback["children"][0]["nativeChild"], "agent-1");
    assert_eq!(
        readback["children"][0]["resultText"],
        "README lacks the flag.\n"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn expanded_view_keeps_the_token_and_receive_routes() {
    let (root, work) = project(
        "receiving-expanded",
        json!([{"id":"docs","text":"Document the flag."}]),
        "Document the flag.",
    );
    let view = exitbind(&root, &["work", "continuation", &work], b"");
    let token = view["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    exitbind(
        &root,
        &[
            "work",
            "bind",
            &work,
            "--context",
            &token,
            "--host",
            "claude",
            "--session",
            "s",
            "--host-version",
            "1",
        ],
        b"",
    );
    let result = "r".repeat(7 * 1024);
    for index in 0..10 {
        let view = exitbind(&root, &["work", "continuation", &work], b"");
        let token = view["mutationContext"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let child = format!("agent-{index}");
        exitbind(
            &root,
            &[
                "work",
                "child",
                &work,
                "inspect",
                "--context",
                &token,
                "--native-child",
                &child,
            ],
            result.as_bytes(),
        );
    }
    let view = exitbind(&root, &["work", "continuation", &work], b"");
    assert_eq!(view["requiresExpansion"], true);
    let token = view["mutationContext"]["token"].as_str().unwrap();
    assert_eq!(view["receive"]["bind"][5], token);
    assert!(view["receive"]["child"].is_array());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn governed_session_start_routes_explicit_locators_to_continuation() {
    let (root, _work) = project(
        "receiving-hook",
        json!([{"id":"docs","text":"Document the flag."}]),
        "Document the flag.",
    );
    let cwd = root.canonicalize().unwrap().display().to_string();
    let payload = format!(r#"{{"hook_event_name":"SessionStart","cwd":{cwd:?}}}"#);
    let output = run(
        &root,
        &[
            env!("CARGO_BIN_EXE_exitbind").to_owned(),
            "hook-run".to_owned(),
        ],
        payload.as_bytes(),
    );
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("first run `exitbind work continuation WORK`"));
    std::fs::remove_dir_all(&root).unwrap();
}
