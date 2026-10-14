use super::*;

#[test]
fn completed_but_unsubmitted_result_returns_through_the_same_work() {
    let fixture = Fixture::new("save-return-held");
    let (work, body) = fixture.worker_journal_with_submission(false);
    let marker = fixture.marker();
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let path = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let assignment = path.file_stem().unwrap().to_str().unwrap();
    let retained = fs::read(&path).unwrap();
    let conflict = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--operation",
            assignment,
            "--model",
            "different-model",
        ],
        b"",
    );
    assert!(!conflict.status.success());
    assert!(text(&conflict).contains("changed execution parameters"));
    assert_eq!(fs::read(&path).unwrap(), retained);
    let response = fixture.value(&["work", "act", &work, "--resume"], b"");
    assert_eq!(response["recoveryEvent"]["kind"], "native_saved_return");
    assert_eq!(response["recoveryEvent"]["cue"], "same_door");
    assert_eq!(response["recoveryEvent"]["providerExecuted"], false);
    assert!(!marker.exists());
    let events = fs::read_to_string(
        fixture
            .root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..])),
    )
    .unwrap();
    let submitted = events
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["action"] == "submit" && event["role"] == "worker")
        .collect::<Vec<_>>();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0]["artifact"]["sha256"], sha256(&body));
}

#[test]
fn inspect_does_not_create_journal_or_report_unsubmitted_result_as_recorded() {
    let fixture = Fixture::new("save-return-inspect-readonly");
    let begun = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "inspect read only",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = begun["work"].as_str().unwrap();
    let lead = begun["next"]["assignment"].as_str().unwrap();
    let scoped = fixture.call(
        &["work", "return", work, lead, "--outcome", "scoped"],
        b"scope",
    );
    assert!(scoped.status.success(), "{}", text(&scoped));
    let directory = fixture.root.join(".exitbind/native-actions").join(work);
    assert!(!directory.exists());
    let empty = fixture.value(&["work", "act", work, "--inspect"], b"");
    assert_eq!(empty["reconciliation"], "no_record");
    assert!(!directory.exists());

    let (other_work, _) = fixture.worker_journal_with_submission(false);
    let directory = fixture
        .root
        .join(".exitbind/native-actions")
        .join(&other_work);
    let before = fs::read_dir(&directory).unwrap().count();
    let inspected = fixture.value(&["work", "act", &other_work, "--inspect"], b"");
    assert_eq!(inspected["result"]["state"], "retained_pending_submission");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), before);
}

#[test]
fn failed_native_inspection_keeps_git_precondition_typed_and_private() {
    let fixture = Fixture::new("native-diagnostic-git-precondition");
    let (work, worker) = fixture.pending_worker("native diagnostic failure");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("fake-diagnostic-failure-codex");
    fs::write(
        &executable,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' 'Not inside a trusted directory and --skip-git-repo-check was not specified.' 'private stderr body' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

    let failed = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!failed.status.success(), "{}", text(&failed));
    assert!(!text(&failed).contains("Not inside a trusted directory"));
    assert!(!text(&failed).contains("private stderr body"));

    let journal_path = fixture
        .root
        .join(".exitbind/native-actions")
        .join(&work)
        .join(format!("{assignment}.json"));
    let journal_bytes = fs::read(journal_path).unwrap();
    let journal: Value = serde_json::from_slice(&journal_bytes).unwrap();
    assert_eq!(journal["status"], "started");
    assert_eq!(journal["observation"]["process"]["code"], 1);
    assert_eq!(journal["observation"]["diagnostic"]["stdoutBytes"], 0);
    assert!(journal["observation"]["diagnostic"]["codes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|code| code == "git_precondition"));
    let journal_text = String::from_utf8_lossy(&journal_bytes);
    assert!(!journal_text.contains("Not inside a trusted directory"));
    assert!(!journal_text.contains("private stderr body"));

    let inspected = fixture.call(&["work", "act", &work, "--inspect"], b"");
    assert!(inspected.status.success(), "{}", text(&inspected));
    let inspected_value: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected_value["assignment"], assignment);
    assert_eq!(inspected_value["process"]["exitCode"], 1);
    assert_eq!(inspected_value["nextAction"]["safe"], false);
    assert!(inspected_value["observation"]["diagnostic"]["codes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|code| code == "git_precondition"));
    assert!(!text(&inspected).contains("Not inside a trusted directory"));
    assert!(!text(&inspected).contains("private stderr body"));
    assert_eq!(
        fixture.value(&["work", "next", &work], b"")["next"]["assignment"],
        assignment
    );
}

#[test]
fn structured_native_failure_is_inspectable_without_event_payloads() {
    let fixture = Fixture::new("native-diagnostic-structured-failure");
    let (work, worker) = fixture.pending_worker("native structured failure");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("fake-structured-failure-codex");
    fs::write(
        &executable,
        r##"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"type":"turn.failed","message":"private turn message"}' '{"type":"error","body":"private error body","path":"/private/path"}'
exit 1
"##,
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

    let failed = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!failed.status.success(), "{}", text(&failed));
    let inspected = fixture.call(&["work", "act", &work, "--inspect"], b"");
    assert!(inspected.status.success(), "{}", text(&inspected));
    let value: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(value["assignment"], assignment);
    assert_eq!(value["process"]["exitCode"], 1);
    assert_eq!(
        value["observation"]["diagnostic"]["codes"],
        json!(["turn_failed", "provider_error"])
    );
    assert!(!text(&inspected).contains("private turn message"));
    assert!(!text(&inspected).contains("private error body"));
    assert!(!text(&inspected).contains("/private/path"));
}

#[test]
fn successful_native_typed_result_retains_observed_success_diagnostic() {
    let fixture = Fixture::new("native-diagnostic-success");
    let (work, worker) = fixture.pending_worker("native diagnostic success");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("fake-diagnostic-success-codex");
    fs::write(
        &executable,
        r##"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"thread-diagnostic-success"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"{\"outcome\":\"completed\",\"summary\":\"typed success\",\"reason\":\"\"}"}}'
printf '%s\n' '{"type":"turn.completed","status":"completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'
"##,
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

    let succeeded = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(succeeded.status.success(), "{}", text(&succeeded));
    let journal_path = fixture
        .root
        .join(".exitbind/native-actions")
        .join(&work)
        .join(format!("{assignment}.json"));
    let journal: Value = serde_json::from_slice(&fs::read(journal_path).unwrap()).unwrap();
    assert_eq!(journal["status"], "completed");
    assert_eq!(journal["observation"]["diagnostic"]["status"], "observed");
    assert_eq!(journal["observation"]["diagnostic"]["codes"], json!([]));
    assert!(journal["observation"]["diagnostic"]["stdoutBytes"]
        .as_u64()
        .is_some_and(|bytes| bytes > 0));
}

#[test]
fn invalid_native_summary_is_inspectable_without_a_second_provider_attempt() {
    use std::fmt::Write as _;

    let fixture = Fixture::new("native-invalid-summary");
    let (work, worker) = fixture.pending_worker("recover bounded native summary");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("fake-invalid-summary-codex");
    let capture = fixture.root.join("request-prompt");
    let marker = fixture.root.join("provider-attempts");
    let events = [
        json!({"type":"thread.started","thread_id":"invalid-summary-thread"}),
        json!({"type":"item.completed","item":{"type":"agent_message","text":
            json!({"outcome":"completed","summary":"private first line\nprivate second line","reason":""}).to_string()}}),
        json!({"type":"turn.completed","status":"completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}),
    ];
    let mut event_script = String::new();
    for event in events {
        writeln!(&mut event_script, "printf '%s\\n' '{event}'").unwrap();
    }
    let script = format!(
        "#!/bin/sh\ncat > '{}'\nprintf x >> '{}'\n{}",
        capture.display(),
        marker.display(),
        event_script
    );
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let failed = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!failed.status.success());
    assert!(text(&failed).contains("summary contains control text"));
    let path = fixture
        .root
        .join(".exitbind/native-actions")
        .join(&work)
        .join(format!("{assignment}.json"));
    let retained = fs::read(&path).unwrap();
    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["observation"]["turn"], "completed");
    assert_eq!(inspected["process"]["exitCode"], 0);
    assert_eq!(inspected["result"]["state"], "provisional");
    assert_eq!(inspected["nextAction"]["safe"], false);
    assert_eq!(inspected["result"]["validation"]["state"], "invalid");
    assert_eq!(
        inspected["result"]["validation"]["reason"],
        "native worker result summary contains control text"
    );
    assert!(!inspected.to_string().contains("private first line"));
    let refused = fixture.call(&["work", "act", &work, "--resume"], b"");
    assert!(!refused.status.success());
    assert!(text(&refused).contains("control text"));
    assert_eq!(fs::read(&marker).unwrap(), b"x");
    assert_eq!(fs::read(&path).unwrap(), retained);
    let prompt = fs::read_to_string(&capture).unwrap();
    assert!(prompt.contains("single-line text"));
    assert!(prompt.contains("8192 UTF-8 bytes"));
    assert!(prompt.contains("Do not rewrite retained evidence"));
}
