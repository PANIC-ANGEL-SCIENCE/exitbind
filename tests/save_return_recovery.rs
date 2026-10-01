//! Focused R18 checks for same-Work native return recovery.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .env("EXITBIND_NATIVE_RECOVERY_TEST", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn value(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn worker_journal(&self) -> (String, Vec<u8>) {
        self.worker_journal_with_submission(true)
    }

    fn pending_worker(&self, goal: &str) -> (String, Value) {
        let started = self.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                goal,
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--review-policy",
                "required",
            ],
            b"",
        );
        let work = started["work"].as_str().unwrap().to_owned();
        let lead = started["next"]["assignment"].as_str().unwrap().to_owned();
        let scoped = self.call(
            &["work", "return", &work, &lead, "--outcome", "scoped"],
            b"scope",
        );
        assert!(scoped.status.success(), "{}", text(&scoped));
        let worker = self.value(&["work", "next", &work, "--full"], b"")["next"].clone();
        (work, worker)
    }

    fn worker_journal_with_submission(&self, submit: bool) -> (String, Vec<u8>) {
        let started = self.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "same Work native return",
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--review-policy",
                "required",
            ],
            b"",
        );
        let work = started["work"].as_str().unwrap().to_owned();
        let lead = started["next"].clone();
        let lead_return = self.call(
            &[
                "work",
                "return",
                &work,
                lead["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            b"scope",
        );
        assert!(lead_return.status.success(), "{}", text(&lead_return));
        let worker = self.value(&["work", "next", &work, "--full"], b"")["next"].clone();
        let body = br#"{"outcome":"completed","summary":"recorded","reason":""}"#.to_vec();
        if submit {
            let worker_return = self.call(
                &[
                    "work",
                    "return",
                    &work,
                    worker["assignment"].as_str().unwrap(),
                    "--outcome",
                    "completed",
                ],
                &body,
            );
            assert!(worker_return.status.success(), "{}", text(&worker_return));
        }
        let packet = worker["packet"].clone();
        let assignment = worker["assignment"].as_str().unwrap().to_owned();
        let directory = self.root.join(".exitbind/native-actions").join(&work);
        fs::create_dir_all(&directory).unwrap();
        let journal = json!({
            "version": 1,
            "status": "completed",
            "work": work,
            "assignment": assignment,
            "assignmentSha256": canonical_sha(&packet),
            "role": "worker",
            "agent": "worker",
            "stage": packet["stage"],
            "attempt": packet["attempt"],
            "threadId": "thread-r18",
            "result": hex_encode(&body),
        });
        fs::write(
            directory.join(format!("{assignment}.json")),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        (work, body)
    }

    fn marker(&self) -> PathBuf {
        let marker = self.root.join("provider-spawned");
        let executable = self.root.join("fake-codex");
        fs::write(
            &executable,
            format!("#!/bin/sh\nprintf spawned > '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        marker
    }
}

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

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn canonical_sha(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&object[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn completed_same_work_journal_is_replayed_without_provider_spawn() {
    let fixture = Fixture::new("save-return-recovery");
    let (work, body) = fixture.worker_journal();
    let marker = fixture.marker();
    let before = fs::read_to_string(
        fixture
            .root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..])),
    )
    .unwrap()
    .lines()
    .count();
    let output = fixture.call(&["work", "act", &work, "--resume", "--themed"], b"");
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("completed"));
    let themed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(themed["world"]["motifs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["name"] == "passing_black_cat" && m["active"] == true));
    let later = fixture.value(&["work", "world", &work, "--json"], b"");
    assert!(later["motifs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["name"] == "passing_black_cat" && m["active"] == false));
    assert!(!marker.exists());
    let after = fs::read_to_string(
        fixture
            .root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..])),
    )
    .unwrap()
    .lines()
    .count();
    assert_eq!(after, before);
    assert!(text(&output).contains(&sha256(&body)));
}

#[test]
fn uncertain_native_journal_refuses_without_provider_spawn() {
    let fixture = Fixture::new("save-return-uncertain");
    let (work, _) = fixture.worker_journal_with_submission(false);
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let journal_path = fs::read_dir(directory)
        .unwrap()
        .find_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension().and_then(|value| value.to_str()) == Some("json")).then_some(path)
        })
        .unwrap();
    let mut journal: Value = serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    journal["status"] = json!("running");
    let provisional = json!({"outcome":"completed","summary":"held","reason":""});
    journal["provisionalFinalResultBytes"] =
        json!(hex_encode(&serde_json::to_vec(&provisional).unwrap()));
    journal["provisionalFinalResult"] = provisional;
    fs::write(&journal_path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let before = fs::read(&journal_path).unwrap();
    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["effect"], "no-change");
    assert_eq!(inspected["reconciliation"], "inspection_required");
    assert_eq!(inspected["nextAction"]["safe"], false);
    assert_eq!(fs::read(&journal_path).unwrap(), before);
    let marker = fixture.marker();
    let output = fixture.call(&["work", "act", &work, "--resume"], b"");
    assert!(!output.status.success());
    assert!(
        text(&output).contains("execution is uncertain"),
        "{}",
        text(&output)
    );
    assert!(!marker.exists());
}

#[test]
fn multiple_saved_operations_require_an_exact_selector() {
    let fixture = Fixture::new("save-return-select");
    let (work, _) = fixture.worker_journal();
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let path = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let original: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let assignment = original["assignment"].as_str().unwrap();
    let mut other = original.clone();
    other["assignment"] = json!("sma_other_saved_operation");
    fs::write(
        directory.join("sma_other_saved_operation.json"),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let ambiguous = fixture.call(&["work", "act", &work, "--resume"], b"");
    assert!(!ambiguous.status.success());
    assert!(text(&ambiguous).contains("--operation"));
    let selected = fixture.call(
        &["work", "act", &work, "--resume", "--operation", assignment],
        b"",
    );
    assert!(selected.status.success(), "{}", text(&selected));
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
}

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
    assert_eq!(
        fixture.value(&["work", "next", &work], b"")["next"]["action"],
        "check"
    );
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
