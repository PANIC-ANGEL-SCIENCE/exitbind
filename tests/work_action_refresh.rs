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

fn assert_complete_route_flags(route: &Value, response: &Value) {
    for field in ["sameConfigRequired", "sameExecutableRequired"] {
        assert_eq!(
            route
                .get(field)
                .or_else(|| response["routeDefaults"].get(field))
                .and_then(Value::as_bool),
            Some(false),
            "{field}: {response}"
        );
    }
}

fn form(root: &Path, work: &str, value: &Value, label: &str) -> Vec<String> {
    let forms = value
        .get("actionForms")
        .unwrap_or(&value["effectiveAction"]);
    let choice = forms["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["label"] == label
                || (c["kind"] == "outcome_template"
                    && c["outcomes"]
                        .as_array()
                        .is_some_and(|outcomes| outcomes.iter().any(|item| item == label)))
        })
        .unwrap();
    let mut command = choice["command"].clone();
    for argument in command["argv"].as_array_mut().unwrap() {
        if argument == "<REASON>" {
            *argument = Value::String("A literal fixture reason; no shell interpolation".into());
        } else if argument == "<OUTCOME>" {
            *argument = Value::String(label.into());
        }
    }
    assert_complete_route_flags(&command, value);
    let argv = command["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(argv[0], env!("CARGO_BIN_EXE_exitbind"));
    assert_eq!(argv[argv.len() - 2], "--config");
    assert_eq!(
        argv[argv.len() - 1],
        root.join("exitbind.json").to_str().unwrap()
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
    let scoped = value(run(
        &root,
        &form(&root, work, &started, "scoped"),
        Some("Bound result-file scope"),
    ));
    assert_eq!(scoped["routeDefaults"]["sameConfigRequired"], false);
    assert_eq!(scoped["routeDefaults"]["sameExecutableRequired"], false);
    assert!(scoped["effectiveAction"]["beforeEditing"]["command"]
        .get("sameConfigRequired")
        .is_none());
    let worker = scoped["currentDelivery"].clone();
    assert_eq!(worker["kind"], "work_actor_context");
    assert_eq!(worker["requiredContentComplete"], true);
    assert_eq!(worker["instructions"]["profile"]["complete"], true);
    let old_return = form(&root, work, &scoped, "completed");
    let mut permit_command = scoped["effectiveAction"]["beforeEditing"]["command"].clone();
    for argument in permit_command["argv"].as_array_mut().unwrap() {
        if argument == "<OPERATION>" {
            *argument = Value::String("Write result file".into());
        }
    }
    assert_complete_route_flags(&permit_command, &scoped);
    let permit_argv = permit_command["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(permit_argv[0], env!("CARGO_BIN_EXE_exitbind"));
    assert_eq!(permit_argv[1], "work");
    assert_eq!(permit_argv[2], "permit");
    assert_eq!(permit_argv[3], work);
    let permit_args = permit_argv.into_iter().skip(1).collect::<Vec<_>>();
    let permit_output = run(&root, &permit_args, None);
    assert!(permit_output.stdout.len() <= 256 * 1024);
    let permit = value(permit_output);
    assert_eq!(permit["allowed"], true);
    let continuity = &permit["currentDelivery"];
    assert_eq!(continuity["kind"], "work_context_continuity");
    assert_eq!(continuity["complete"], false);
    assert_eq!(continuity["baseBinding"], worker["binding"]);
    assert_eq!(continuity["contextIdentity"], worker["contextIdentity"]);
    assert!(permit["currentGrant"].is_object());
    // This read is a regression oracle, not part of the actor's action flow.
    // The compact content identities must match the actual current detail.
    let direct = invoke(&root, &["work", "detail", work, "--json"]);
    assert_eq!(continuity["binding"], direct["binding"]);
    assert_eq!(continuity["contextIdentity"], direct["contextIdentity"]);
    assert_eq!(
        worker["instructions"]["profile"]["content"],
        direct["recipientContext"]["profile"]["content"]
    );
    let loop_state = &direct["sections"]["assignment"]["assignment"]["context"]["loop"];
    for field in ["state", "budget", "spent", "noInformationStreak"] {
        assert_eq!(continuity["governor"][field], loop_state[field]);
    }
    assert!(!permit["effectiveAction"]["choices"]
        .as_array()
        .unwrap()
        .is_empty());
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
    let next_step = invoke(&root, &["work", "next", work, "--json"]);
    assert_eq!(
        next_step["currentDelivery"]["kind"],
        "work_context_reference"
    );
    assert_eq!(
        next_step["currentDelivery"]["contextIdentity"],
        worker["contextIdentity"]
    );
    assert_ne!(
        next_step["currentDelivery"]["inputsSha256"],
        continuity["inputsSha256"]
    );
    let next_return = form(&root, work, &next_step, "completed");
    let next = &permit["nextAction"];
    assert_complete_route_flags(next, &permit);
    let next_argv = next["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(!next_argv.iter().any(|arg| arg == "--view"));
    let changed_detail = value(run(&root, &next_argv[1..], None));
    assert_eq!(changed_detail["complete"], true);
    assert_eq!(
        next_step["currentDelivery"]["contextIdentity"],
        changed_detail["contextIdentity"]
    );
    assert_eq!(
        next_step["currentDelivery"]["inputsSha256"],
        changed_detail["sections"]["assignment"]["residual"]["snapshot"]["inputsSha256"]
    );
    assert_eq!(next_return, form(&root, work, &changed_detail, "completed"));
    let returned_output = run(&root, &next_return, Some("Produced src/result.txt"));
    assert!(returned_output.stdout.len() <= 256 * 1024);
    let returned = value(returned_output);
    assert_eq!(returned["effect"], "recorded");
    assert!(returned.get("currentDelivery").is_none());
    assert_eq!(returned["effectiveAction"]["kind"], "check");
    assert!(returned["effectiveAction"]
        .get("mechanicalAction")
        .is_none());
    assert_eq!(
        returned["effectiveAction"]["executionOptions"]["timeoutMs"]["minimum"],
        1
    );
    assert!(returned["effectiveAction"]["meaning"]
        .as_str()
        .unwrap()
        .contains("current frozen"));
    let check = &returned["effectiveAction"]["command"];
    assert_complete_route_flags(check, &returned);
    let argv = check["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(argv[0], env!("CARGO_BIN_EXE_exitbind"));
    let checked = value(run(&root, &argv[1..], None));
    assert_eq!(checked["result"]["code"], 0);
    assert_eq!(checked["currentDelivery"]["requiredContentComplete"], true);
    assert_eq!(checked["effectiveAction"]["kind"], "spawn");
    assert!(!checked["eventSha256"].is_null());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn changed_worker_instructions_require_current_full_context() {
    let root = support::temp("changed recipient instructions");
    support::git_topology::repository(&root);
    fs::write(root.join("README.md"), "Current instruction fixture\n").unwrap();
    let setup = invoke(
        &root,
        &[
            "setup",
            "--mode",
            "portable",
            "--root",
            ".",
            "--worker-observe",
            "README.md",
            "--worker-write",
            "README.md",
            "--worker-commands",
            "true",
            "--goal",
            "Exercise changed recipient instructions",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
            "--json",
            "--apply",
        ],
    );
    let begin_args = setup["next"]["argv"][0]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let started = value(run(&root, &begin_args, None));
    let work = started["work"].as_str().unwrap();
    let scoped = value(run(
        &root,
        &form(&root, work, &started, "scoped"),
        Some("scope"),
    ));
    let worker = &scoped["currentDelivery"];
    assert_eq!(worker["requiredContentComplete"], true);
    let old_recipient = worker["contextIdentity"]["recipientSha256"].clone();
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
    let profile = root.join(config["agents"]["worker"]["profile"].as_str().unwrap());
    let mut content = fs::read_to_string(&profile).unwrap();
    content.push_str("\nChanged worker instruction from the fixture owner.\n");
    fs::write(&profile, &content).unwrap();
    let mut permit = scoped["effectiveAction"]["beforeEditing"]["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for word in &mut permit {
        if word == "<OPERATION>" {
            *word = "Apply the current instruction".into();
        }
    }
    let output = run(&root, &permit[1..], None);
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("changed") || error.contains("stale"),
            "{output:?}"
        );
        let current = invoke(&root, &["work", "detail", work, "--json"]);
        assert_eq!(current["recipientContext"]["profile"]["content"], content);
    } else {
        let response = value(output);
        let delivery = &response["currentDelivery"];
        if delivery["kind"] == "work_context_continuity" {
            assert_ne!(
                delivery["contextIdentity"]["recipientSha256"],
                old_recipient
            );
        } else {
            assert_eq!(delivery["kind"], "work_actor_context");
            assert_eq!(delivery["instructions"]["profile"]["content"], content);
        }
        let command = response["full"]["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(command[0], env!("CARGO_BIN_EXE_exitbind"));
        let current = value(run(&root, &command[1..], None));
        assert_eq!(current["recipientContext"]["profile"]["content"], content);
        assert_ne!(current["contextIdentity"]["recipientSha256"], old_recipient);
    }
    fs::remove_dir_all(root).unwrap();
}
