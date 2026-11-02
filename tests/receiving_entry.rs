//! A receiving host starts from `work continuation WORK` and completes bind,
//! native-child recording, and readback using only the routes that view gives.

use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

mod support;

fn run(root: &Path, argv: &[String], input: &[u8]) -> Output {
    let mut child = support::git_topology::command(&argv[0])
        .current_dir(root)
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
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

fn project(label: &str, requirements: Value, source: &str) -> (support::TempRoot, String) {
    project_with_write(label, requirements, source, None)
}

fn project_with_write(
    label: &str,
    requirements: Value,
    source: &str,
    write: Option<&str>,
) -> (support::TempRoot, String) {
    let root = support::temp(label);
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    if let Some(path) = write {
        let config_path = root.join("exitbind.json");
        let mut config: Value =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        config["agents"]["worker"]["write"] = json!([path]);
        std::fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    }
    let requirement = requirements.as_array().unwrap().first().unwrap();
    let preserve_requirement = format!(
        "{}:{}",
        requirement["id"].as_str().unwrap(),
        requirement["text"].as_str().unwrap()
    );
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
            "--preservation-check-command",
            "true",
            "--preserve-requirement",
            &preserve_requirement,
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
            ("<HOST>", "claude"),
            ("<NATIVE_SESSION>", "session-a"),
            ("<HOST_VERSION>", "2.1.283"),
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
            ("<SHORT_ASSIGNMENT>", "check docs"),
            ("<FRESH_TOKEN>", fresh_token),
            ("<CHILD_ID>", "agent-1"),
        ],
    );
    let recorded = run(&root, &child, "README lacks the flag.\n".as_bytes());
    assert!(recorded.status.success(), "{recorded:?}");

    // The Lead's views keep their continuation copy without receiving routes.
    let lead = exitbind(&root, &["work", "next", &work, "--full"], b"");
    assert!(lead["continuation"].is_object());
    assert!(lead["continuation"].get("receive").is_none());

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
fn receiver_gets_a_current_named_prepare_route_without_changing_legacy_capture() {
    let (root, work) = project_with_write(
        "receiving-named",
        json!([{"id":"docs","text":"Document the flag."}]),
        "Document the flag.",
        Some("fixture.txt"),
    );
    let lead = exitbind(&root, &["work", "next", &work, "--full"], b"");
    let lead_assignment = lead["next"]["assignment"].as_str().unwrap();
    exitbind(
        &root,
        &[
            "work",
            "return",
            &work,
            lead_assignment,
            "--outcome",
            "scoped",
        ],
        b"Scoped work",
    );
    let view = exitbind(&root, &["work", "continuation", &work], b"");
    let bind = fill(
        &view["receive"]["bind"],
        &[
            ("<HOST>", "claude"),
            ("<NATIVE_SESSION>", "session-a"),
            ("<HOST_VERSION>", "fixture"),
        ],
    );
    let bound = run(&root, &bind, b"");
    assert!(bound.status.success(), "{bound:?}");
    let view = exitbind(&root, &["work", "continuation", &work], b"");
    let named = &view["receive"]["currentAssignmentPrepare"];
    assert_eq!(named["preferred"], true, "{view}");
    assert!(view["receive"]["prepareChild"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg == "<SHORT_ASSIGNMENT>"));
    let route = fill(&named["command"], &[]);
    assert!(
        route.iter().any(|arg| arg == "--current-binding"),
        "{route:?}"
    );
    assert!(route.iter().any(|arg| arg == "--agent-type"), "{route:?}");
    let prepared = run(&root, &route, b"");
    assert!(prepared.status.success(), "{prepared:?}");
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["selected"]["assignment"], named["assignment"]);
    assert_eq!(
        prepared["selected"]["nativeTaskName"],
        named["nativeTaskName"]
    );
    let hook = |payload: Value| {
        run(
            &root,
            &[
                env!("CARGO_BIN_EXE_exitbind").to_owned(),
                "hook-run".to_owned(),
            ],
            payload.to_string().as_bytes(),
        )
    };
    let native = named["nativeTaskName"].as_str().unwrap();
    let start = hook(json!({"hook_event_name":"SubagentStart", "cwd":root.path(),
        "session_id":"session-a", "agent_id":"named-child", "agent_type":native}));
    assert!(start.status.success(), "{start:?}");
    let output: Value = serde_json::from_slice(&start.stdout).unwrap();
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        context.contains("Automatic Work return. ENTIRE final message"),
        "{context}"
    );
    assert!(
        context.contains("JSON object only; no prose, fence or extra fields"),
        "{context}"
    );
    assert!(
        !context.contains("Complete assignment exceeds"),
        "{context}"
    );
    let assignment = context
        .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("missing complete assignment: {context}"));
    let assignment: Value = serde_json::from_str(assignment).unwrap();
    assert_eq!(
        assignment["evidence"]["items"][0]["artifact"],
        "Scoped work"
    );
    assert_eq!(
        assignment["preservationAssignment"]["requirements"][0]["text"],
        "Document the flag."
    );
    assert!(assignment["additionalPreservationAssignment"].is_null());
    assert_eq!(
        assignment["taskCategories"]["obligations"]["items"][0]["disposition"],
        "open"
    );
    let expanded = run(&root, &fill(&assignment["ownerActionDetail"], &[]), b"");
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(
        expanded["sections"]["assignment"]["assignment"]["preservationAssignment"],
        expanded["sections"]["assignment"]["residual"]["humanHelp"]["preservationAssignment"]
    );
    assert_eq!(
        assignment["preservationAssignment"],
        expanded["sections"]["assignment"]["assignment"]["preservationAssignment"]
    );
    assert!(expanded["sections"]["assignment"]["assignment"]["artifactPathHint"].is_null());
    assert!(
        expanded["sections"]["tasks"]["categoryDispositions"]["obligations"]["items"][0]
            ["nextActions"]
            .is_array(),
        "{expanded}"
    );
    let delivered: Value =
        serde_json::from_slice(&std::fs::read(root.join(".exitbind/child-intents.json")).unwrap())
            .unwrap();
    assert_eq!(
        delivered["intents"][0]["assignmentDelivery"]["state"], "presented",
        "{context}"
    );
    let form = &assignment["currentAction"]["beforeEditing"];
    assert_eq!(form["required"], true, "{context}");
    assert_eq!(*form, expanded["actionForms"]["beforeEditing"]);
    let permit = run(
        &root,
        &fill(
            &form["command"]["argv"],
            &[("<OPERATION>", "Update fixture text")],
        ),
        b"",
    );
    assert!(permit.status.success(), "{permit:?}");
    let permit: Value = serde_json::from_slice(&permit.stdout).unwrap();
    assert_eq!(permit["allowed"], true, "{permit}");
    assert_eq!(
        permit["childDeliveryContinuity"]["state"], "current",
        "{permit}"
    );
    let stale = run(&root, &fill(&assignment["ownerActionDetail"], &[]), b"");
    assert!(
        !stale.status.success(),
        "old detail must not survive the grant"
    );
    std::fs::write(root.join("fixture.txt"), "updated fixture\n").unwrap();
    let current = exitbind(&root, &["work", "detail", &work, "--json"], b"");
    assert_eq!(
        current["effectiveAction"]["currentGrant"]["state"],
        "current_for_this_worker_assignment"
    );
    let stop = hook(json!({"hook_event_name":"SubagentStop", "cwd":root.path(),
        "session_id":"session-a", "agent_id":"named-child", "agent_type":native,
        "last_assistant_message":r#"{"outcome":"completed","summary":"Updated fixture text","reason":""}"#}));
    assert!(stop.status.success(), "{stop:?}");
    let captured: Value =
        serde_json::from_slice(&std::fs::read(root.join(".exitbind/child-intents.json")).unwrap())
            .unwrap();
    assert!(
        captured["intents"][0]["returnEventSha256"].is_string(),
        "{captured}"
    );
    assert!(
        captured["intents"][0].get("pendingReturn").is_none(),
        "{captured}"
    );
    let replay = hook(json!({"hook_event_name":"SubagentStop", "cwd":root.path(),
        "session_id":"session-a", "agent_id":"named-child", "agent_type":native,
        "last_assistant_message":r#"{"outcome":"completed","summary":"Updated fixture text","reason":""}"#}));
    assert!(replay.status.success(), "{replay:?}");
    let repeated: Value =
        serde_json::from_slice(&std::fs::read(root.join(".exitbind/child-intents.json")).unwrap())
            .unwrap();
    assert_eq!(
        repeated, captured,
        "the same result must not be returned twice"
    );
    let after = exitbind(&root, &["work", "continuation", &work], b"");
    assert_eq!(
        after["preparedChildren"][0]["workReturn"]["state"], "recorded",
        "{after}"
    );
}

fn prepare_codex_direct(label: &str) -> (support::TempRoot, String, String, String, String) {
    let (root, work) = project(
        label,
        json!([{"id":"docs","text":"Document the flag."}]),
        "Document the flag.",
    );
    let lead = exitbind(&root, &["work", "next", &work, "--full"], b"");
    let lead_assignment = lead["next"]["assignment"].as_str().unwrap();
    exitbind(
        &root,
        &[
            "work",
            "return",
            &work,
            lead_assignment,
            "--outcome",
            "scoped",
        ],
        b"Scoped work",
    );
    let view = exitbind(&root, &["work", "continuation", &work], b"");
    let bind = fill(
        &view["receive"]["bind"],
        &[
            ("<HOST>", "codex"),
            ("<NATIVE_SESSION>", "codex-session-a"),
            ("<HOST_VERSION>", "fixture"),
        ],
    );
    let bound = run(&root, &bind, b"");
    assert!(bound.status.success(), "{bound:?}");
    let view = exitbind(&root, &["work", "continuation", &work], b"");
    let named = &view["receive"]["currentAssignmentPrepare"];
    let assignment = named["assignment"].as_str().unwrap().to_owned();
    let native = named["nativeTaskName"].as_str().unwrap().to_owned();
    let prepared = run(&root, &fill(&named["command"], &[]), b"");
    assert!(prepared.status.success(), "{prepared:?}");
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let intent = prepared["intent"].as_str().unwrap().to_owned();
    (root, work, assignment, native, intent)
}

fn codex_direct_hook(root: &Path, native: &str, event: &str, message: Option<&str>) -> Output {
    let payload = json!({"hook_event_name":event,"cwd":root,
        "session_id":"codex-session-a","agent_id":"codex-child",
        "agent_type":native,"last_assistant_message":message});
    run(
        root,
        &[
            env!("CARGO_BIN_EXE_exitbind").to_owned(),
            "hook-run".to_owned(),
        ],
        payload.to_string().as_bytes(),
    )
}

#[test]
fn codex_direct_hook_retains_the_new_result_without_automatic_credit() {
    let (root, work, assignment, native, _) =
        prepare_codex_direct("receiving-codex-unverified-limit");
    let start = codex_direct_hook(&root, &native, "SubagentStart", None);
    assert!(start.status.success(), "{start:?}");
    let output: Value = serde_json::from_slice(&start.stdout).unwrap();
    let text = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        text.contains("Codex direct-hook context limit is unverified"),
        "{text}"
    );
    assert!(
        text.contains("automatic Work return cannot receive credit"),
        "{text}"
    );
    assert!(text.contains("Full assignment packet route:"), "{text}");
    assert!(!text.contains("Complete current assignment, evidence and task conditions"));
    let delivery: Value =
        serde_json::from_slice(&std::fs::read(root.join(".exitbind/child-intents.json")).unwrap())
            .unwrap();
    assert_eq!(
        delivery["intents"][0]["assignmentDelivery"]["state"],
        "unavailable"
    );
    assert_eq!(
        delivery["intents"][0]["assignmentDelivery"]["reason"],
        "codex_direct_hook_limit_unverified"
    );
    assert!(delivery["intents"][0]["assignmentDelivery"]
        .get("actorFactsSha256")
        .is_none());
    assert_eq!(delivery["intents"][0]["selectedDeliveryProtocol"], 1);
    let duplicate = codex_direct_hook(&root, &native, "SubagentStart", None);
    assert!(duplicate.status.success(), "{duplicate:?}");
    let repeated: Value =
        serde_json::from_slice(&std::fs::read(root.join(".exitbind/child-intents.json")).unwrap())
            .unwrap();
    assert_eq!(
        repeated["intents"][0]["assignmentDelivery"],
        delivery["intents"][0]["assignmentDelivery"]
    );
    let message = r#"{"outcome":"completed","summary":"uncredited report","reason":""}"#;
    let stop = codex_direct_hook(&root, &native, "SubagentStop", Some(message));
    assert!(stop.status.success(), "{stop:?}");
    let captured: Value =
        serde_json::from_slice(&std::fs::read(root.join(".exitbind/child-intents.json")).unwrap())
            .unwrap();
    assert!(captured["intents"][0]["returnEventSha256"].is_null());
    assert!(captured["intents"][0].get("pendingReturn").is_none());
    assert!(captured["intents"][0]["rawObservation"]["reason"]
        .as_str()
        .unwrap()
        .contains("Codex direct-hook context limit is unverified"));
    let next = exitbind(&root, &["work", "next", &work, "--full"], b"");
    assert_eq!(next["next"]["assignment"], assignment);
}

#[test]
fn historical_codex_delivery_survives_duplicate_start_but_not_changed_rules() {
    use sha2::{Digest, Sha256};

    // Recreate the older producer's delivered proof from this fixture's
    // current public context and recipient detail, without an actor marker.
    fn digest(value: &Value) -> String {
        fn canonical(value: &Value) -> String {
            match value {
                Value::Object(object) => {
                    let mut keys = object.keys().collect::<Vec<_>>();
                    keys.sort();
                    let fields = keys
                        .into_iter()
                        .map(|key| {
                            format!(
                                "{}:{}",
                                serde_json::to_string(key).unwrap(),
                                canonical(&object[key])
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("{{{fields}}}")
                }
                Value::Array(values) => format!(
                    "[{}]",
                    values.iter().map(canonical).collect::<Vec<_>>().join(",")
                ),
                _ => serde_json::to_string(value).unwrap(),
            }
        }
        format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
    }

    for changed in [false, true] {
        let label = if changed {
            "codex-legacy-drift"
        } else {
            "codex-legacy-exact"
        };
        let (root, work, _, native, intent) = prepare_codex_direct(label);
        let first = codex_direct_hook(&root, &native, "SubagentStart", None);
        assert!(first.status.success(), "{first:?}");
        let context = exitbind(&root, &["work", "child", "context", &work, &intent], b"");
        let detail = exitbind(&root, &["work", "detail", &work, "--json"], b"");
        assert_eq!(detail["recipientContext"]["available"], true, "{detail}");
        let instructions = json!({"profile":context["profile"],
            "perspectives":context["perspectives"],"guidance":context["guidance"],
            "architectureContract":context["architectureContract"],
            "capabilities":context["capabilities"],
            "specialistExperience":context["specialistExperience"],
            "rules":detail["recipientContext"]["rules"]});
        let path = root.join(".exitbind/child-intents.json");
        let mut saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        saved["intents"][0]
            .as_object_mut()
            .unwrap()
            .remove("selectedDeliveryProtocol");
        saved["intents"][0]["assignmentDelivery"] = json!({"state":"presented",
            "binding":detail["binding"],"recipientSha256":digest(&detail["recipientContext"]),
            "instructionsSha256":digest(&instructions)});
        let legacy = saved["intents"][0]["assignmentDelivery"].clone();
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        let duplicate = codex_direct_hook(&root, &native, "SubagentStart", None);
        assert!(duplicate.status.success(), "{duplicate:?}");
        let preserved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(preserved["intents"][0]["assignmentDelivery"], legacy);
        assert!(preserved["intents"][0]
            .get("selectedDeliveryProtocol")
            .is_none());
        if changed {
            std::fs::write(root.join("AGENTS.md"), "A changed required instruction.\n").unwrap();
        }
        let message = r#"{"outcome":"completed","summary":"Legacy fixture result","reason":""}"#;
        let stop = codex_direct_hook(&root, &native, "SubagentStop", Some(message));
        assert!(stop.status.success(), "{stop:?}");
        let captured: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if changed {
            assert!(
                captured["intents"][0]["returnEventSha256"].is_null(),
                "{captured}"
            );
            assert!(
                captured["intents"][0]["rawObservation"].is_object(),
                "{captured}"
            );
        } else {
            assert!(
                captured["intents"][0]["returnEventSha256"].is_string(),
                "{captured}"
            );
            assert!(
                captured["intents"][0]["rawObservation"].is_null(),
                "{captured}"
            );
        }
    }
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
fn governed_session_start_routes_known_work_to_detail_and_unknown_work_to_resume() {
    let (root, work) = project(
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
    let exe = env!("CARGO_BIN_EXE_exitbind");
    assert!(
        context.contains(&format!("run `{exe} work detail <WORK> --json --config ")),
        "{context}"
    );
    assert!(context.contains(&format!(
        "Unknown Work uses `{exe} work resume --json --config "
    )));
    assert!(!context.contains("first run `exitbind work next WORK --json`"));
    assert!(context.contains("only for an initialized same-Work cross-host handoff"));
    assert!(context.contains("stale or incomplete detail needs its supported refresh/expansion"));
    assert!(context.contains("preserve it and use current Work detail"));
    let detail = exitbind(&root, &["work", "detail", &work, "--json"], b"");
    assert_eq!(detail["complete"], true);
    assert_eq!(detail["recipientContext"]["complete"], true);
    assert_eq!(detail["actionForms"]["work"], work);
    std::fs::remove_dir_all(&root).unwrap();
}
