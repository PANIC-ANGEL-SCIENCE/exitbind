//! A committed permit returns current forms; old forms cannot mutate state.
mod support;

use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Output, Stdio},
};

fn run(root: &Path, args: &[String], input: Option<&str>) -> Output {
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    support::git_topology::isolate(&mut command);
    command
        .current_dir(root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input.as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
    }
    child.wait_with_output().unwrap()
}

fn value(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn invoke(root: &Path, args: &[&str]) -> Value {
    value(run(
        root,
        &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
        None,
    ))
}

fn form(root: &Path, work: &str, value: &Value, label: &str) -> Vec<String> {
    let forms = value
        .get("actionForms")
        .unwrap_or(&value["effectiveAction"]);
    let choice = forms["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == label)
        .unwrap();
    let mut command = choice["command"].clone();
    for argument in command["argv"].as_array_mut().unwrap() {
        if argument == "<REASON>" {
            *argument = Value::String("A literal fixture reason; no shell interpolation".into());
        }
    }
    let argv = support::reconstruct_work_command(
        &command,
        Path::new(env!("CARGO_BIN_EXE_exitbind")),
        &root.join("exitbind.json"),
        root,
    );
    assert_eq!(argv[1], "work");
    assert_eq!(argv[2], "return");
    assert_eq!(argv[3], work);
    argv.into_iter().skip(1).collect()
}

#[test]
fn post_permit_forms_are_current_and_old_return_binding_refuses() {
    let root = support::temp("post-permit actions with spaces");
    support::git_topology::repository(&root);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("README.md"), "Current action fixture\n").unwrap();
    let setup = invoke(
        &root,
        &[
            "setup",
            "--mode",
            "portable",
            "--root",
            ".",
            "--worker-observe",
            "README.md,src",
            "--worker-write",
            "src",
            "--worker-commands",
            "test -s src/result.txt",
            "--reviewer-observe",
            "README.md,src",
            "--reviewer-write",
            "none",
            "--reviewer-commands",
            "none",
            "--goal",
            "Produce the result file",
            "--check-command",
            "test -s src/result.txt",
            "--review-policy",
            "required",
            "--json",
            "--apply",
        ],
    );
    let args = setup["next"]["argv"][0]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|a| a.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let started = value(run(&root, &args, None));
    let work = started["work"].as_str().unwrap();
    value(run(
        &root,
        &form(&root, work, &started, "scoped"),
        Some("Bound result-file scope"),
    ));
    let worker = invoke(&root, &["work", "detail", work, "--json"]);
    assert_eq!(worker["recipientContext"]["complete"], true);
    let old_return = form(&root, work, &worker, "completed");
    let mut permit_command = worker["actionForms"]["beforeEditing"]["command"].clone();
    for argument in permit_command["argv"].as_array_mut().unwrap() {
        if argument == "<OPERATION>" {
            *argument = Value::String("Write result file".into());
        }
    }
    let permit_argv = support::reconstruct_work_command(
        &permit_command,
        Path::new(env!("CARGO_BIN_EXE_exitbind")),
        &root.join("exitbind.json"),
        &root,
    );
    assert_eq!(permit_argv[1], "work");
    assert_eq!(permit_argv[2], "permit");
    assert_eq!(permit_argv[3], work);
    let permit_args = permit_argv.into_iter().skip(1).collect::<Vec<_>>();
    let permit_output = run(&root, &permit_args, None);
    assert!(permit_output.stdout.len() <= 8192);
    let permit = value(permit_output);
    assert_eq!(permit["allowed"], true);
    assert_eq!(permit["effectiveAction"]["exists"], true);
    assert_ne!(permit["effectiveAction"]["binding"], worker["binding"]);
    assert_eq!(permit["effect"], "recorded");
    fs::write(root.join("src/result.txt"), "The requested result\n").unwrap();
    let stale = run(
        &root,
        &old_return,
        Some("Stale form must not return this result"),
    );
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("binding is stale"));
    let returned_output = run(
        &root,
        &form(
            &root,
            work,
            &invoke(
                &root,
                &["work", "detail", work, "--view", "action", "--json"],
            ),
            "completed",
        ),
        Some("Produced src/result.txt"),
    );
    assert!(returned_output.stdout.len() <= 8192);
    let returned = value(returned_output);
    assert_eq!(returned["effect"], "recorded");
    assert_eq!(returned["effectiveAction"]["kind"], "check");
    let checked = invoke(&root, &["work", "check", work]);
    assert_eq!(checked["result"]["code"], 0);
    assert_eq!(checked["effectiveAction"]["kind"], "spawn");
    assert!(!checked["eventSha256"].is_null());
    fs::remove_dir_all(root).unwrap();
}
