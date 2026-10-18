use super::*;

#[test]
fn ended_worker_resumes_the_same_work_once_after_identity_check() {
    let fixture = Fixture::new("save-return-ended");
    let (work, _) = fixture.worker_journal_with_submission(false);
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let path = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let mut journal: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["status"] = json!("running");
    journal.as_object_mut().unwrap().remove("result");
    journal
        .as_object_mut()
        .unwrap()
        .remove("provisionalFinalResult");
    journal
        .as_object_mut()
        .unwrap()
        .remove("provisionalFinalResultBytes");
    journal["processIdentity"] = json!({
        "pid": 2147483647u32,
        "processGroup": 2147483647i32,
        "startTimeTicks": 1,
    });
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let marker = fixture.root.join("resumed-once");
    let executable = fixture.root.join("fake-resume-codex");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\ncat >/dev/null\nprintf x >> '{}'\nprintf '%s\\n' \\\n'{{\"type\":\"thread.started\",\"thread_id\":\"thread-r18\"}}' \\\n'{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"outcome\\\":\\\"completed\\\",\\\"summary\\\":\\\"resumed\\\",\\\"reason\\\":\\\"\\\"}}\"}}}}' \\\n'{{\"type\":\"turn.completed\",\"status\":\"completed\",\"usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    journal["request"] = json!({
        "executable": fs::canonicalize(&executable).unwrap(),
        "model": null,
        "reasoningEffort": null,
        "timeoutMs": 1800000,
        "persistSession": true
    });
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let output = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(fs::read(&marker).unwrap(), b"x");
    let compact = fixture.value(&["work", "next", &work], b"");
    let compact_action = compact["next"]["action"]
        .as_str()
        .expect("compact next action is a string");
    let resolved = match compact_action {
        "check" => compact["next"].clone(),
        "inspect" => {
            assert_eq!(compact["truncated"], true);
            assert!(compact["fullCommand"].is_array());
            let command = json!({
                "argv": compact["fullCommand"],
                "sameConfigRequired": compact["fullCommandSameConfigRequired"] == true,
                "sameExecutableRequired": compact["fullCommandSameExecutableRequired"] == true,
            });
            let argv = support::reconstruct_work_command(
                &command,
                std::path::Path::new(env!("CARGO_BIN_EXE_exitbind")),
                &fixture.root.join("exitbind.json"),
                &fixture.root,
            );
            assert_eq!(argv[1], "work");
            assert_eq!(argv[2], "next");
            assert_eq!(argv[3], work);
            assert_eq!(argv[4], "--full");
            let mut process = support::git_topology::command(&argv[0]);
            process
                .current_dir(&fixture.root)
                .args(&argv[1..])
                .env("EXITBIND_NATIVE_RECOVERY_TEST", "1")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = process.spawn().unwrap();
            use std::io::Write;
            child.stdin.take().unwrap().write_all(b"").unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{}", text(&output));
            let expanded: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_ne!(expanded["truncated"], true);
            assert_eq!(expanded["work"], work);
            expanded["next"].clone()
        }
        action => panic!("unexpected compact next action: {action}"),
    };
    assert_eq!(resolved["action"], "check");
}

#[test]
fn ended_provisional_worker_is_reconciled_as_a_new_operation() {
    let fixture = Fixture::new("save-return-provisional");
    let (work, _) = fixture.worker_journal_with_submission(false);
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let path = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let mut journal: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let provisional = json!({
        "outcome": "completed",
        "summary": "provider emitted a result before failing",
        "reason": ""
    });
    let provisional_bytes = serde_json::to_vec(&provisional).unwrap();
    journal["operationId"] = json!("old-native-operation");
    let old_operation = journal["operationId"].clone();
    journal["status"] = json!("started");
    journal["provisionalFinalResult"] = provisional;
    journal["provisionalFinalResultBytes"] = json!(hex_encode(&provisional_bytes));
    journal["observation"] = json!({
        "process": {"code": 1, "signal": null, "success": false, "timedOut": false},
        "turn": "failed",
        "commands": [],
        "usage": {"source": "provider", "inputTokens": 1, "cachedInputTokens": 0, "outputTokens": 1},
        "threadId": "thread-r18",
        "coverageGaps": [],
        "unobservedItemCount": 0,
        "interrupted": false
    });
    journal["processIdentity"] = json!({
        "pid": 2147483647u32,
        "processGroup": 2147483647i32,
        "startTimeTicks": 1,
    });
    let marker = fixture.root.join("provisional-retry");
    let executable = fixture.root.join("fake-provisional-retry-codex");
    fs::write(
        &executable,
        format!(
            r#"#!/bin/sh
cat >/dev/null
printf x >> '{}'
printf '%s\n' '{{"type":"thread.started","thread_id":"thread-r18"}}'
printf '%s\n' '{{"type":"item.completed","item":{{"type":"agent_message","text":"{{\"outcome\":\"completed\",\"summary\":\"retried\",\"reason\":\"\"}}"}}}}'
printf '%s\n' '{{"type":"turn.completed","status":"completed","usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}}}'
"#,
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    journal["request"] = json!({
        "executable": fs::canonicalize(&executable).unwrap(),
        "model": null,
        "reasoningEffort": null,
        "timeoutMs": 1800000,
        "persistSession": true,
        "resumed": true,
    });
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["effect"], "no-change");
    assert_eq!(inspected["result"]["state"], "provisional");
    assert_eq!(inspected["reconciliation"], "resume_ready");
    assert_eq!(inspected["nextAction"]["type"], "resume_current");
    assert_eq!(fs::read(&path).unwrap(), before);

    let conflict = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--codex-bin",
            executable.to_str().unwrap(),
            "--model",
            "different-model",
        ],
        b"",
    );
    assert!(!conflict.status.success());
    assert!(text(&conflict).contains("changed execution parameters"));
    assert!(!fs::read_dir(&directory).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .to_string_lossy()
        .contains(".provisional.")));

    let output = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(fs::read(&marker).unwrap(), b"x");

    let current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(current["status"], "completed");
    assert_ne!(current["operationId"], old_operation);
    assert_eq!(current["resumedFromOperation"], old_operation);
    assert_eq!(current["priorFailure"]["status"], "started");
    assert_eq!(
        current["priorFailure"]["provisionalFinalResultBytes"],
        json!(hex_encode(&provisional_bytes))
    );
    assert!(fs::read_dir(&directory).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .to_string_lossy()
        .contains(".provisional.")));
}

#[test]
fn provisional_with_possible_external_effects_or_prior_retry_refuses_automatic_resume() {
    let fixture = Fixture::new("save-return-effects");
    let (work, _) = fixture.worker_journal_with_submission(false);
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let path = fs::read_dir(directory)
        .unwrap()
        .map(|item| item.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let mut journal: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let provisional = json!({"outcome":"completed","summary":"held","reason":""});
    journal["status"] = json!("running");
    journal["operationId"] = json!("effectful-operation");
    journal["provisionalFinalResultBytes"] =
        json!(hex_encode(&serde_json::to_vec(&provisional).unwrap()));
    journal["provisionalFinalResult"] = provisional;
    journal["processIdentity"] =
        json!({"pid":2147483647u32,"processGroup":2147483647i32,"startTimeTicks":1});
    journal["observation"] = json!({
        "process":{"code":1,"signal":null,"success":false,"timedOut":false},
        "turn":"failed","commands":[],"unobservedItemCount":1,
        "usage":{"inputTokens":1,"cachedInputTokens":0,"outputTokens":1},
        "threadId":"thread-r18","coverageGaps":[],"interrupted":false
    });
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["nextAction"]["safe"], false);
    assert!(
        text(&fixture.call(&["work", "act", &work, "--resume"], b"")).contains("external effects")
    );

    journal["observation"]["unobservedItemCount"] = json!(0);
    journal["observation"]["commands"] =
        json!([{"status":"completed","exitCode":0,"invocationSha256":"command-id"}]);
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    assert_eq!(
        fixture.value(&["work", "act", &work, "--inspect"], b"")["nextAction"]["safe"],
        false
    );

    journal["observation"]["commands"] = json!([]);
    journal["priorFailure"] = json!({"operation":"earlier"});
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    assert_eq!(
        fixture.value(&["work", "act", &work, "--inspect"], b"")["nextAction"]["safe"],
        false
    );

    journal
        .as_object_mut()
        .unwrap()
        .remove("provisionalFinalResult");
    journal
        .as_object_mut()
        .unwrap()
        .remove("provisionalFinalResultBytes");
    journal["status"] = json!("started");
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["nextAction"]["safe"], false);
    let refused = fixture.call(&["work", "act", &work, "--resume"], b"");
    assert!(!refused.status.success());
    assert!(text(&refused).contains("bounded provider retry"));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
#[cfg(target_os = "linux")]
fn alive_worker_keeps_its_existing_execution_without_another_spawn() {
    let fixture = Fixture::new("save-return-alive");
    let (work, _) = fixture.worker_journal_with_submission(false);
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let path = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let mut journal: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["status"] = json!("running");
    journal.as_object_mut().unwrap().remove("result");
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let start_time: u64 = stat
        .rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .nth(19)
        .unwrap()
        .parse()
        .unwrap();
    journal["processIdentity"] = json!({
        "pid": std::process::id(),
        "processGroup": std::process::id(),
        "startTimeTicks": start_time,
    });
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let marker = fixture.marker();
    journal["request"] =
        json!({"executable": fs::canonicalize(fixture.root.join("fake-codex")).unwrap()});
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let output = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--codex-bin",
            fixture.root.join("fake-codex").to_str().unwrap(),
        ],
        b"",
    );
    assert!(!output.status.success());
    assert!(text(&output).contains("provider is alive"));
    assert!(!marker.exists());
}

#[test]
fn repair_assignment_resumes_current_thread_and_explicit_old_replay_stays_idempotent() {
    let fixture = Fixture::new("save-return-repair-resume");
    let (work, _) = fixture.worker_journal();
    let old_path = fs::read_dir(fixture.root.join(".exitbind/native-actions").join(&work))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let old_operation = old_path.file_stem().unwrap().to_str().unwrap();
    fixture.value(&["work", "check", &work], b"");
    let reviewer = fixture.value(&["work", "next", &work, "--full"], b"")["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.value(
        &[
            "work",
            "return",
            &work,
            &reviewer,
            "--outcome",
            "rework",
            "--reason",
            "review_finding",
        ],
        b"one exact repair",
    );
    let lead = fixture.value(&["work", "next", &work, "--full"], b"")["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.value(
        &[
            "work",
            "disposition",
            &work,
            &lead,
            "--decision",
            "repair",
            "--reason",
            "one exact repair",
            "--repair-boundary",
            "the note only",
            "--regression",
            "run the frozen check",
        ],
        b"",
    );
    let pending = fixture.value(&["work", "next", &work, "--full"], b"");
    assert_eq!(pending["next"]["role"], "worker");
    assert_eq!(pending["next"]["packet"]["attempt"], 2);
    let marker = fixture.root.join("repair-resumed");
    let executable = fixture.root.join("fake-repair-codex");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\ncat >/dev/null\nprintf x >> '{}'\nprintf '%s\\n' \\\n'{{\"type\":\"thread.started\",\"thread_id\":\"thread-r18\"}}' \\\n'{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"outcome\\\":\\\"completed\\\",\\\"summary\\\":\\\"repaired\\\",\\\"reason\\\":\\\"\\\"}}\"}}}}' \\\n'{{\"type\":\"turn.completed\",\"status\":\"completed\",\"usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let resumed = fixture.value(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(resumed["next"]["action"], "check");
    assert_eq!(fs::read(&marker).unwrap(), b"x");
    let replay = fixture.value(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--operation",
            old_operation,
        ],
        b"",
    );
    assert_eq!(replay["idempotent"], true);
    assert_eq!(fs::read(&marker).unwrap(), b"x");
}
