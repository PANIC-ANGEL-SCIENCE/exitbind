//! Native saved-result submission must pass the opted-in pre-return diagnostic.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Output, Stdio},
};

struct Fixture {
    root: support::TempRoot,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        support::git_topology::repository(&root);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            use std::io::Write;
            // A command may refuse and exit before reading stdin; its output is still asserted.
            if let Err(error) = child.stdin.take().unwrap().write_all(input) {
                assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
            }
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    }

    fn value(&self, args: &[&str]) -> Value {
        let output = self.call(args, None);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn pending_worker(&self, checker: &str) -> (String, Value) {
        let begin = self.value(&[
            "work",
            "begin",
            "change",
            "--goal",
            "native saved result requires the selected pre-return diagnostic",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--pre-return-command",
            checker,
            "--pre-return-timeout-ms",
            "10000",
            "--pre-return-capture-bytes",
            "4096",
        ]);
        let work = begin["work"].as_str().unwrap().to_owned();
        assert!(begin["next"]["assignment"].is_string());
        let scoped = self.call(
            &[
                "work",
                "return",
                &work,
                begin["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            Some(b"lead scope"),
        );
        assert!(scoped.status.success(), "{}", text(&scoped));
        let worker = self.value(&["work", "next", &work, "--full"])["next"].clone();
        assert_eq!(worker["role"], "worker");
        (work, worker)
    }

    fn native_worker(&self, work: &str, executable: &str) -> Output {
        self.call(
            &["work", "act", work, "--json", "--codex-bin", executable],
            None,
        )
    }

    fn pre_return_attempt(&self, work: &str) -> Value {
        let detail = self.value(&["work", "detail", work, "--json"]);
        let diagnostic = &detail["sections"]["evidence"]["preReturn"];
        assert_eq!(diagnostic["enabled"], true);
        diagnostic["attempt"].clone()
    }

    fn ledger_events(&self, work: &str) -> Vec<Value> {
        let token = work.strip_prefix("smw_").unwrap();
        fs::read_to_string(self.root.join(format!(".exitbind/runs/work-{token}.jsonl")))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn pre_return_counter(&self) -> Vec<u8> {
        fs::read(self.root.join(".exitbind/pre-return-counter")).unwrap_or_default()
    }

    fn provider_counter(&self) -> Vec<u8> {
        fs::read(self.root.join(".exitbind/provider-counter")).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let checker = checker_path(&self.root);
        if checker.exists() {
            fs::remove_file(checker).unwrap();
        }
    }
}

fn text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_native_resume_rejected(
    fixture: &Fixture,
    output: &Output,
    ledger: &std::path::Path,
    expected_ledger: &[u8],
    expected_error: &str,
) -> Value {
    assert!(!output.status.success(), "{}", text(output));
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("expected a machine refusal: {}", text(output)));
    assert_eq!(value["error"], expected_error, "{}", text(output));
    assert!(value["preReturn"].is_null(), "{}", text(output));
    assert!(value["replayed"].is_null(), "{}", text(output));
    assert_eq!(fs::read(ledger).unwrap(), expected_ledger);
    assert_eq!(fixture.provider_counter(), b"x");
    assert_eq!(fixture.pre_return_counter(), b"x");
    value
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn checker_path(root: &std::path::Path) -> PathBuf {
    root.with_file_name(format!(
        "{}-pre-return-check.sh",
        root.file_name().unwrap().to_string_lossy()
    ))
}

fn make_executable(path: &std::path::Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn fake_codex(root: &std::path::Path) -> PathBuf {
    let executable = root.join("fake-codex");
    let provider_counter = root.join(".exitbind/provider-counter");
    let result = json!({"outcome":"completed","summary":"fixture worker completed","reason":""});
    let events = [
        json!({"type":"thread.started","thread_id":"native-pre-return-fixture"}),
        json!({"type":"item.completed","item":{"type":"agent_message","text":result.to_string()}}),
        json!({"type":"turn.completed","status":"completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}),
    ];
    let mut script = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf x >> {}\n",
        shell_quote(&provider_counter.display().to_string())
    );
    for event in events {
        script.push_str(&format!(
            "printf '%s\\n' {}\n",
            shell_quote(&event.to_string())
        ));
    }
    make_executable(&executable, &script);
    executable
}

fn write_checker(root: &std::path::Path, failure: bool) -> String {
    let path = checker_path(root);
    let status = if failure { "19" } else { "0" };
    let output = if failure {
        "printf 'diagnostic failure evidence\\n'"
    } else {
        "printf 'diagnostic passed\\n'"
    };
    make_executable(
        &path,
        &format!("#!/bin/sh\nprintf x >> .exitbind/pre-return-counter\n{output}\nexit {status}\n"),
    );
    format!("sh {}", shell_quote(&path.display().to_string()))
}

#[test]
fn native_worker_submission_runs_diagnostic_but_does_not_credit_the_final_check() {
    let fixture = Fixture::new("native-pre-return-pass");
    let checker = write_checker(&fixture.root, false);
    let executable = fake_codex(&fixture.root);
    let work = fixture.pending_worker(&checker).0;
    let output = fixture.native_worker(&work, executable.to_str().unwrap());
    assert!(output.status.success(), "{}", text(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["effect"], "recorded");
    assert_eq!(response["next"]["action"], "check");
    assert_eq!(fixture.provider_counter(), b"x");
    assert_eq!(fixture.pre_return_counter(), b"x");

    let events = fixture.ledger_events(&work);
    let submissions = events
        .iter()
        .filter(|event| event["action"] == "submit" && event["role"] == "worker")
        .collect::<Vec<_>>();
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0]["outcome"], "completed");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "check")
            .count(),
        0
    );
    let diagnostic = fixture.pre_return_attempt(&work);
    assert_eq!(diagnostic["result"]["state"], "passed");
    assert_eq!(
        diagnostic["attempt"]["binding"]["resultSha256"],
        submissions[0]["artifact"]["sha256"]
    );
}

#[test]
fn saved_native_result_resume_reuses_failed_diagnostic_attempt_after_lost_reply() {
    let fixture = Fixture::new("native-pre-return-lost-reply");
    let input_path = fixture.root.join("native-input.txt");
    fs::write(&input_path, b"original input").unwrap();
    let checker = write_checker(&fixture.root, true);
    let executable = fake_codex(&fixture.root);
    let (work, worker) = fixture.pending_worker(&checker);

    // The first native action saves its completed provider result, then the
    // selected diagnostic refuses submission. Drop the reply to model a lost
    // caller response before recovering through the same native assignment.
    let first = fixture.native_worker(&work, executable.to_str().unwrap());
    assert!(!first.status.success(), "{}", text(&first));
    drop(first);
    assert_eq!(fixture.provider_counter(), b"x");
    assert_eq!(fixture.pre_return_counter(), b"x");
    let journal = fixture
        .root
        .join(".exitbind/native-actions")
        .join(&work)
        .join(format!("{}.json", worker["assignment"].as_str().unwrap()));
    let journal_bytes = fs::read(&journal).unwrap();
    let native: Value = serde_json::from_slice(&journal_bytes).unwrap();
    assert_eq!(native["status"], "completed");
    assert!(native["result"].is_string());

    let diagnostic = fixture.pre_return_attempt(&work);
    let attempt_id = diagnostic["attempt"]["attemptId"].as_str().unwrap();
    assert_eq!(diagnostic["result"]["state"], "failed");
    assert_eq!(diagnostic["result"]["failureKind"], "nonzero_exit");
    let read = fixture.value(&["run", "pre-return", "read", &work, attempt_id, "--json"]);
    assert_eq!(read["attempt"]["attemptId"], attempt_id);
    assert_eq!(read["result"]["attemptId"], attempt_id);
    assert_eq!(read["result"]["state"], "failed");
    let exact_stdout = fixture.call(
        &[
            "run",
            "pre-return",
            "read",
            &work,
            attempt_id,
            "--stream",
            "stdout",
        ],
        None,
    );
    assert!(exact_stdout.status.success(), "{}", text(&exact_stdout));
    assert_eq!(exact_stdout.stdout, b"diagnostic failure evidence\n");

    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let expected_ledger = fs::read(&ledger).unwrap();
    let pending = fixture.value(&["work", "next", &work, "--full"]);
    assert_eq!(pending["next"]["action"], "spawn");
    assert_eq!(pending["next"]["assignment"], worker["assignment"]);
    let resumed = fixture.call(&["work", "act", &work, "--resume", "--json"], None);
    assert!(!resumed.status.success());
    let refused: Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(refused["preReturn"], "refused", "{refused}: {resumed:?}");
    assert_eq!(refused["attemptId"], attempt_id);
    assert_eq!(refused["replayed"], true);
    assert_eq!(refused["finalCheckCredited"], false);
    assert_eq!(fixture.provider_counter(), b"x");
    assert_eq!(fixture.pre_return_counter(), b"x");
    assert_eq!(fs::read(&ledger).unwrap(), expected_ledger);

    let mut changed_native = native.clone();
    let mut changed_result = changed_native["result"].as_str().unwrap().to_owned();
    changed_result.replace_range(..2, "00");
    changed_native["result"] = json!(changed_result);
    fs::write(&journal, serde_json::to_vec(&changed_native).unwrap()).unwrap();
    let forged = fixture.call(&["work", "act", &work, "--resume", "--json"], None);
    assert_native_resume_rejected(
        &fixture,
        &forged,
        &ledger,
        &expected_ledger,
        "native assignment journal does not match the current assignment",
    );
    fs::write(&journal, journal_bytes).unwrap();

    fs::write(&input_path, b"changed input").unwrap();
    let changed_input = fixture.call(&["work", "act", &work, "--resume", "--json"], None);
    assert_native_resume_rejected(
        &fixture,
        &changed_input,
        &ledger,
        &expected_ledger,
        "native assignment journal does not match the current assignment",
    );
    fs::write(&input_path, b"original input").unwrap();

    let checker_path = checker_path(&fixture.root);
    let checker_bytes = fs::read(&checker_path).unwrap();
    fs::write(&checker_path, b"#!/bin/sh\nexit 0\n").unwrap();
    let changed_checker = fixture.call(&["work", "act", &work, "--resume", "--json"], None);
    let changed_checker = assert_native_resume_rejected(
        &fixture,
        &changed_checker,
        &ledger,
        &expected_ledger,
        "pre-return checker source changed; use authorized supersession to bind the changed checker",
    );
    assert_eq!(
        changed_checker["classification"],
        "pre_return_checker_changed"
    );
    fs::write(&checker_path, checker_bytes).unwrap();

    let scope = fixture
        .ledger_events(&work)
        .into_iter()
        .find(|event| event["action"] == "submit" && event["role"] == "lead")
        .unwrap();
    let scope_path = fixture
        .root
        .join(scope["artifact"]["path"].as_str().unwrap());
    let scope_bytes = fs::read(&scope_path).unwrap();
    fs::write(&scope_path, b"changed scope artifact").unwrap();
    let changed_artifact = fixture.call(&["work", "act", &work, "--resume", "--json"], None);
    assert_native_resume_rejected(
        &fixture,
        &changed_artifact,
        &ledger,
        &expected_ledger,
        &format!(
            "artifact drift detected: {}",
            scope["artifact"]["path"].as_str().unwrap()
        ),
    );
    fs::write(scope_path, scope_bytes).unwrap();

    let restored = fixture.call(&["work", "act", &work, "--resume", "--json"], None);
    assert!(!restored.status.success());
    let restored: Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert_eq!(restored["preReturn"], "refused");
    assert_eq!(restored["attemptId"], attempt_id);
    assert_eq!(restored["replayed"], true);
    assert_eq!(fixture.provider_counter(), b"x");
    assert_eq!(fixture.pre_return_counter(), b"x");
    assert_eq!(fs::read(&ledger).unwrap(), expected_ledger);

    let foreign_artifact = fixture.root.join(".exitbind/artifacts/foreign-evidence");
    fs::write(&foreign_artifact, b"a different recorded event").unwrap();
    let foreign = fixture.call(
        &[
            "work",
            "evidence",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--artifact-root",
            "state",
            "--artifact",
            ".exitbind/artifacts/foreign-evidence",
        ],
        None,
    );
    assert!(foreign.status.success(), "{}", text(&foreign));
    let foreign_ledger = fs::read(&ledger).unwrap();
    let stale = fixture.call(&["work", "act", &work, "--resume", "--json"], None);
    assert_native_resume_rejected(
        &fixture,
        &stale,
        &ledger,
        &foreign_ledger,
        "native assignment journal does not match the current assignment",
    );

    let events = fixture.ledger_events(&work);
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event["action"] == "submit"
                    && event["role"] == "worker"
                    && event["outcome"] == "completed"
            })
            .count(),
        0
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "check")
            .count(),
        0
    );
}
