#![cfg(unix)]

mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, process::Output};

const CHECK: &str = "test -f actual-product-check";

struct Fixture {
    root: support::TempRoot,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Self { root }
    }
    fn call(&self, args: &[&str]) -> Output {
        support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .output()
            .unwrap()
    }
    fn ledger(&self) -> String {
        ".exitbind/runs/pre-return.jsonl".into()
    }
    fn start_run(&self, checker: &str) {
        self.start_run_with(checker, &[]);
    }
    fn start_run_with(&self, checker: &str, options: &[&str]) {
        let ledger = self.ledger();
        let mut args = vec![
            "run",
            "start",
            "change",
            "--goal",
            "pre-return test",
            "--ledger",
            ledger.as_str(),
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--pre-return-command",
            checker,
        ];
        args.extend_from_slice(options);
        let out = self.call(&args);
        assert!(out.status.success(), "{out:?}");
        self.submit("lead", "scoped", "scope");
    }
    fn submit(&self, actor: &str, outcome: &str, name: &str) -> Output {
        self.submit_to(actor, outcome, name, &self.ledger())
    }
    fn submit_to(&self, actor: &str, outcome: &str, name: &str, ledger: &str) -> Output {
        let relative = format!(".exitbind/artifacts/{name}.md");
        fs::write(self.root.join(&relative), format!("{name}\n")).unwrap();
        self.call(&[
            "run",
            "submit",
            actor,
            ledger,
            "--outcome",
            outcome,
            "--artifact",
            &relative,
            "--artifact-root",
            "state",
        ])
    }
    fn submit_without_path(&self, actor: &str, outcome: &str, name: &str) -> Output {
        let relative = format!(".exitbind/artifacts/{name}.md");
        fs::write(self.root.join(&relative), format!("{name}\n")).unwrap();
        let bin = self.root.parent().unwrap().join(format!(
            "{}-no-shell-bin",
            self.root.file_name().unwrap().to_string_lossy()
        ));
        let _ = fs::remove_dir_all(&bin);
        fs::create_dir_all(&bin).unwrap();
        let git = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join("git"))
            .find(|path| path.is_file())
            .expect("git is available for integration tests");
        std::os::unix::fs::symlink(git, bin.join("git")).unwrap();
        let result = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args([
                "run",
                "submit",
                actor,
                &self.ledger(),
                "--outcome",
                outcome,
                "--artifact",
                &relative,
                "--artifact-root",
                "state",
                "--config",
            ])
            .arg(self.root.join("exitbind.json"))
            .env("PATH", &bin)
            .output()
            .unwrap();
        fs::remove_dir_all(&bin).unwrap();
        result
    }
    fn work_begin(&self, checker: &str) -> (String, Value) {
        let out = self.call(&[
            "work",
            "begin",
            "change",
            "--goal",
            "pre-return facade test",
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--pre-return-command",
            checker,
        ]);
        assert!(out.status.success(), "{out:?}");
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        (
            value["work"].as_str().unwrap().into(),
            value["next"].clone(),
        )
    }
    fn work_return(&self, work: &str, assignment: &Value, outcome: &str, body: &str) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args([
                "work",
                "return",
                work,
                assignment["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
                "--config",
            ])
            .arg(self.root.join("exitbind.json"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(body.as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }
    fn events(&self, ledger: &str) -> Vec<Value> {
        fs::read_to_string(self.root.join(ledger))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    fn inspect(&self, ledger: &str) -> Value {
        let output = self.call(&["run", "inspect", ledger, "--json"]);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn admitted_suffix(&self, ledger: &str, before: &[u8], attempt_id: &str) -> Vec<u8> {
        let after = fs::read(self.root.join(ledger)).unwrap();
        assert!(after.starts_with(before));
        let suffix = std::str::from_utf8(&after[before.len()..]).unwrap();
        let lines = suffix.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "{suffix}");
        let event: Value = serde_json::from_str(lines[0]).unwrap();
        let previous: Value =
            serde_json::from_str(std::str::from_utf8(before).unwrap().lines().last().unwrap())
                .unwrap();
        assert_eq!(event["action"], "pre_return_admitted");
        assert_eq!(event["admission"]["attemptId"], attempt_id);
        assert_eq!(event["previousEventSha256"], previous["eventSha256"]);
        let mut unsigned = event.clone();
        let hash = unsigned
            .as_object_mut()
            .unwrap()
            .remove("eventSha256")
            .unwrap();
        assert_eq!(
            hash,
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&unsigned).unwrap())
            )
        );
        let attempt_path = self.root.join(".exitbind/pre-return").join(format!(
            "pre-return-{}-{attempt_id}.attempt.json",
            event["runId"].as_str().unwrap()
        ));
        let mut attempt: Value = serde_json::from_slice(&fs::read(attempt_path).unwrap()).unwrap();
        let record_hash = attempt
            .as_object_mut()
            .unwrap()
            .remove("recordSha256")
            .unwrap();
        assert_eq!(record_hash, event["admission"]["recordSha256"]);
        assert_eq!(
            record_hash,
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&attempt).unwrap())
            )
        );
        for field in ["runId", "ledger", "stage", "attempt", "agent", "role"] {
            assert_eq!(
                event["admission"]["assignment"][field],
                attempt["binding"][field]
            );
        }
        after
    }
    fn only_attempt_id(&self) -> String {
        let mut attempts = fs::read_dir(self.root.join(".exitbind/pre-return"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".attempt.json")
            });
        let entry = attempts.next().expect("one pre-return attempt");
        assert!(attempts.next().is_none(), "expected exactly one attempt");
        let name = entry.file_name().to_string_lossy().into_owned();
        name.strip_suffix(".attempt.json")
            .unwrap()
            .rsplit_once('-')
            .unwrap()
            .1
            .to_owned()
    }
    fn read_stream(&self, selector: &str, attempt_id: &str, stream: &str) -> Vec<u8> {
        let output = self.call(&[
            "run",
            "pre-return",
            "read",
            selector,
            attempt_id,
            "--stream",
            stream,
        ]);
        assert!(output.status.success(), "{output:?}");
        output.stdout
    }
    fn work_check(&self, work: &str) -> Output {
        self.call(&["work", "check", work])
    }
}

#[test]
fn lower_level_failure_records_only_admission_and_same_attempt_is_not_replayed() {
    let f = Fixture::new("pre-return-lower-failure");
    f.start_run("printf failed-out; printf failed-err >&2; printf x >> .exitbind/pre-return/invocations; exit 7");
    let ledger = f.ledger();
    let before = fs::read(f.root.join(&ledger)).unwrap();
    let governor = f.inspect(&ledger)["governor"].clone();
    let failed = f.submit("worker", "completed", "failed");
    assert!(!failed.status.success(), "{failed:?}");
    let diagnostic = String::from_utf8_lossy(&failed.stdout);
    assert!(diagnostic.contains("attemptId"), "{diagnostic}");
    assert!(diagnostic.contains("nonzero_exit"), "{diagnostic}");
    let attempt = f.only_attempt_id();
    let after = f.admitted_suffix(&ledger, &before, &attempt);
    assert_eq!(f.inspect(&ledger)["governor"], governor);
    assert_eq!(f.read_stream(&ledger, &attempt, "stdout"), b"failed-out");
    assert_eq!(f.read_stream(&ledger, &attempt, "stderr"), b"failed-err");
    let attempt_dir = f.root.join(".exitbind/pre-return");
    let attempts = fs::read_dir(attempt_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".attempt.json"))
        .count();
    assert_eq!(attempts, 1);
    let again = f.submit("worker", "completed", "failed");
    assert!(!again.status.success());
    assert_eq!(
        fs::read(f.root.join(".exitbind/pre-return/invocations")).unwrap(),
        b"x"
    );
    let attempts = fs::read_dir(f.root.join(".exitbind/pre-return"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".attempt.json"))
        .count();
    assert_eq!(
        attempts, 1,
        "known failure must return its durable result without re-execution"
    );
    assert_eq!(fs::read(f.root.join(&ledger)).unwrap(), after);
}

#[test]
fn lower_level_passing_check_allows_the_result_to_enter_the_run_ledger() {
    let f = Fixture::new("pre-return-lower-pass");
    f.start_run("printf passed-out; printf passed-err >&2; exit 0");
    let submitted = f.submit("worker", "completed", "passed");
    assert!(submitted.status.success(), "{submitted:?}");
    let events = f.events(&f.ledger());
    assert!(events
        .iter()
        .any(|event| event["action"] == "submit" && event["agent"] == "worker"));
    let attempt = f.only_attempt_id();
    let run_id = f.events(&f.ledger())[0]["runId"]
        .as_str()
        .unwrap()
        .to_owned();
    let attempt_value: Value = serde_json::from_slice(
        &fs::read(f.root.join(format!(
            ".exitbind/pre-return/pre-return-{run_id}-{attempt}.attempt.json"
        )))
        .unwrap(),
    )
    .unwrap();
    for field in [
        "verificationPreparation",
        "verificationPreparationSha256",
        "verificationOperationId",
    ] {
        assert!(
            attempt_value["binding"].get(field).is_none(),
            "unselected legacy binding acquired {field}"
        );
    }
    assert_eq!(
        f.read_stream(&f.ledger(), &attempt, "stdout"),
        b"passed-out"
    );
    assert_eq!(
        f.read_stream(&f.ledger(), &attempt, "stderr"),
        b"passed-err"
    );
}

#[test]
fn public_lower_level_submission_keeps_blocked_return_available_without_running_precheck() {
    let f = Fixture::new("pre-return-lower-blocked");
    let (work, lead) = f.work_begin("exit 9");
    let scoped = f.work_return(&work, &lead, "scoped", "scope");
    assert!(scoped.status.success(), "{scoped:?}");
    let run_id = work.strip_prefix("smw_").unwrap();
    let ledger = format!(".exitbind/runs/work-{run_id}.jsonl");
    let blocked = f.submit_to("worker", "blocked", "blocked-without-precheck", &ledger);
    assert!(blocked.status.success(), "{blocked:?}");
    assert!(!f.root.join(".exitbind/pre-return").exists());
    let events = f.events(&ledger);
    assert!(events.iter().any(|event| {
        event["action"] == "submit" && event["role"] == "worker" && event["outcome"] == "blocked"
    }));
}

#[test]
fn missing_owned_direct_admission_offers_a_blocked_report_route() {
    let f = Fixture::new("pre-return-direct-missing-owned");
    f.start_run("printf x >> .exitbind/direct-count; exit 9");
    let first = f.submit("worker", "completed", "first-result");
    assert!(!first.status.success(), "{first:?}");
    let attempt = f.only_attempt_id();
    let ledger = f.ledger();
    let before = fs::read(f.root.join(&ledger)).unwrap();
    let start: Value =
        serde_json::from_slice(before.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    let run_id = start["runId"].as_str().unwrap();
    fs::remove_file(f.root.join(format!(
        ".exitbind/pre-return/pre-return-{run_id}-{attempt}.attempt.json"
    )))
    .unwrap();

    let refused = f.submit("worker", "completed", "different-result");
    assert!(!refused.status.success(), "{refused:?}");
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(error.contains("execution is unknown"), "{error}");
    let route = error
        .split("and run ")
        .nth(1)
        .expect("a blocked report route")
        .trim();
    assert!(route.contains("<ARTIFACT_PATH>"), "{route}");
    let report = ".exitbind/artifacts/direct-blocked.md";
    fs::write(
        f.root.join(report),
        b"Admission unavailable; execution unknown.",
    )
    .unwrap();
    let command = route.replace("<ARTIFACT_PATH>", report);
    let blocked = support::git_topology::command("sh")
        .current_dir(&f.root)
        .args(["-c", command.as_str()])
        .output()
        .unwrap();
    assert!(blocked.status.success(), "{blocked:?}");
    assert_eq!(
        fs::read(f.root.join(".exitbind/direct-count")).unwrap(),
        b"x"
    );
    let after = fs::read(f.root.join(&ledger)).unwrap();
    assert!(after.starts_with(&before));
    let event: Value = serde_json::from_slice(&after[before.len()..]).unwrap();
    assert_eq!(event["action"], "submit");
    assert_eq!(event["outcome"], "blocked");
}

#[test]
fn facade_runs_the_frozen_check_before_worker_return_and_passes_through_on_success() {
    let f = Fixture::new("pre-return-facade-pass");
    let (work, mut next) = f.work_begin("exit 0");
    let scoped = f.work_return(&work, &next, "scoped", "scope");
    assert!(scoped.status.success(), "{scoped:?}");
    next = serde_json::from_slice::<Value>(&scoped.stdout).unwrap()["next"].clone();
    let returned = f.work_return(&work, &next, "completed", "worker result");
    assert!(returned.status.success(), "{returned:?}");
    let value: Value = serde_json::from_slice(&returned.stdout).unwrap();
    assert_eq!(value["next"]["action"], "check");
    let run_id = work.strip_prefix("smw_").unwrap();
    let ledger = format!(".exitbind/runs/work-{run_id}.jsonl");
    assert!(f
        .events(&ledger)
        .iter()
        .any(|e| e["action"] == "submit" && e["agent"] == "worker"));
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let check = f.work_check(&work);
    assert!(check.status.success(), "{check:?}");
    let reviewer_assignment = f.call(&["work", "next", &work, "--json"]);
    assert!(
        reviewer_assignment.status.success(),
        "{reviewer_assignment:?}"
    );
    let reviewer_assignment: Value = serde_json::from_slice(&reviewer_assignment.stdout).unwrap();
    let reviewer = f.work_return(
        &work,
        &reviewer_assignment["next"],
        "approved",
        "review approved",
    );
    assert!(reviewer.status.success(), "{reviewer:?}");
    let next = serde_json::from_slice::<Value>(&reviewer.stdout).unwrap()["next"].clone();
    let accepted = f.work_return(&work, &next, "accepted", "accepted");
    assert!(accepted.status.success(), "{accepted:?}");
    let status = f.call(&["run", "status", &ledger, "--json"]);
    assert!(status.status.success(), "{status:?}");
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["status"], "accepted");
}

#[test]
#[cfg(target_os = "linux")]
fn selected_pre_return_uses_its_own_prepared_scratch_and_disables_product_bytecode() {
    let f = Fixture::new("pre-return-verification-preparation");
    let config_path = f.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = serde_json::json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    fs::write(f.root.join("pre_return_probe.py"), "VALUE = 'prepared'\n").unwrap();
    let checker = "python3 -c 'import pre_return_probe, os, pathlib; assert os.environ[\"PYTHONDONTWRITEBYTECODE\"] == \"1\"; p=pathlib.Path(os.environ[\"EXITBIND_VERIFICATION_SCRATCH\"]); (p/\"pre-return.txt\").write_text(pre_return_probe.VALUE); print(pre_return_probe.VALUE)'";
    f.start_run_with(checker, &["--review-policy", "required"]);
    let returned = f.submit("worker", "completed", "prepared");
    assert!(returned.status.success(), "{returned:?}");
    assert!(
        !f.root.join("__pycache__").exists(),
        "pre-return wrote Python bytecode into product inputs"
    );
    let verification = f.root.join(".exitbind/verification");
    let mut pending = vec![verification];
    let mut scratch = None;
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                if entry.path().join("pre-return.txt").is_file() {
                    scratch = Some(entry.path());
                } else {
                    pending.push(entry.path());
                }
            }
        }
    }
    let scratch = scratch.expect("pre-return did not receive its owned scratch directory");
    assert_eq!(
        fs::read_to_string(scratch.join("pre-return.txt")).unwrap(),
        "prepared"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn selected_pre_return_alias_refusal_is_durable_and_never_retries() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("pre-return-verification-alias");
    let config_path = f.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = serde_json::json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    symlink(".exitbind/verification", f.root.join("verification-alias")).unwrap();
    let output = f.call(&[
        "run",
        "start",
        "change",
        "--goal",
        "pre-return alias",
        "--ledger",
        ".exitbind/runs/pre-return.jsonl",
        "--check-command",
        "exit 0",
        "--review-policy",
        "required",
        "--pre-return-command",
        "touch verification-alias/should-not-run",
    ]);
    assert!(output.status.success(), "{output:?}");
    f.submit("lead", "scoped", "scope");
    let failed = f.submit("worker", "completed", "alias-refused");
    assert!(!failed.status.success());
    assert!(!f.root.join("verification-alias/should-not-run").exists());
    let attempt_id = f.only_attempt_id();
    let attempt_path = f.root.join(format!(
        ".exitbind/pre-return/pre-return-{attempt_id}.result.json"
    ));
    let result: Value = serde_json::from_slice(&fs::read(attempt_path).unwrap()).unwrap();
    assert_eq!(result["failureKind"], "preparation_refused");
    assert_eq!(result["execution"]["certainty"], "known_not_started");
    assert_eq!(result["execution"]["started"], false);
    let retry = f.submit("worker", "completed", "alias-refused-again");
    assert!(!retry.status.success());
    assert!(!f.root.join("verification-alias/should-not-run").exists());
}

#[test]
fn facade_failure_preserves_action_and_blocked_worker_can_report_without_running_checker() {
    let f = Fixture::new("pre-return-facade-failure");
    let (work, mut next) = f.work_begin("exit 4");
    let scoped = f.work_return(&work, &next, "scoped", "scope");
    next = serde_json::from_slice::<Value>(&scoped.stdout).unwrap()["next"].clone();
    let failed = f.work_return(&work, &next, "completed", "worker result");
    assert!(!failed.status.success(), "{failed:?}");
    let failure: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(failure["preReturn"], "refused");
    assert_eq!(failure["finalCheckCredited"], false);
    let blocked = f.work_return(&work, &next, "blocked", "blocked: checker failed");
    assert!(blocked.status.success(), "{blocked:?}");
    let run_id = work.strip_prefix("smw_").unwrap();
    let ledger = format!(".exitbind/runs/work-{run_id}.jsonl");
    assert!(f
        .events(&ledger)
        .iter()
        .any(|e| e["action"] == "submit" && e["outcome"] == "blocked"));
}

#[test]
fn valid_grant_survives_failure_and_lead_can_replace_only_the_defective_pre_return_check() {
    let f = Fixture::new("pre-return-lead-replacement");
    let (work, lead) = f.work_begin("exit 8");
    let scoped = f.work_return(&work, &lead, "scoped", "scope");
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = serde_json::from_slice::<Value>(&scoped.stdout).unwrap()["next"].clone();
    let assignment = worker["assignment"].as_str().unwrap();
    let permit = f.call(&[
        "work",
        "permit",
        &work,
        assignment,
        "--operation",
        "pre-return-repair",
    ]);
    assert!(permit.status.success(), "{permit:?}");
    let run_id = work.strip_prefix("smw_").unwrap();
    let ledger = format!(".exitbind/runs/work-{run_id}.jsonl");
    let after_grant = fs::read(f.root.join(&ledger)).unwrap();
    let governor = f.inspect(&ledger)["governor"].clone();
    let failed = f.work_return(&work, &worker, "completed", "worker result before repair");
    assert!(!failed.status.success(), "{failed:?}");
    let diagnostic: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(diagnostic["preReturn"], "refused");
    let attempt_id = diagnostic["attemptId"].as_str().unwrap();
    f.admitted_suffix(&ledger, &after_grant, attempt_id);
    assert_eq!(f.inspect(&ledger)["governor"], governor);

    let blocked_body = format!(
        "Blocked: pre-return attemptId {attempt_id} exited nonzero; Lead diagnosis requested."
    );
    let blocked = f.work_return(&work, &worker, "blocked", &blocked_body);
    assert!(blocked.status.success(), "{blocked:?}");
    let events = f.events(&ledger);
    let blocked_event = events
        .iter()
        .rev()
        .find(|event| event["action"] == "submit" && event["outcome"] == "blocked")
        .unwrap();
    assert_eq!(blocked_event["role"], "worker");
    let current = f.call(&["work", "next", &work, "--json"]);
    assert!(current.status.success(), "{current:?}");
    let current: Value = serde_json::from_slice(&current.stdout).unwrap();
    let binding = current["current"]["binding"].as_str().unwrap();
    let replacement_args = [
        "work",
        "replace-check",
        work.as_str(),
        "--pre-return-command",
        "exit 0",
        "--reason",
        "The frozen pre-return contract was defective; replace it after linked nonzero evidence.",
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
    ];
    let preview = f.call(&replacement_args);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["decision"]["target"], "pre_return");
    assert_eq!(preview["decision"]["source"]["attemptId"], attempt_id);
    assert_eq!(preview["successor"]["preservesFinalCheckPolicy"], true);
    let applied = f.call(&[
        "work",
        "replace-check",
        &work,
        "--pre-return-command",
        "exit 0",
        "--reason",
        "The frozen pre-return contract was defective; replace it after linked nonzero evidence.",
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
        "--apply",
    ]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let applied: Value = serde_json::from_slice(&applied.stdout).unwrap();
    let successor = applied["successor"]["work"].as_str().unwrap();
    let successor_run_id = successor.strip_prefix("smw_").unwrap();
    let successor_ledger = format!(".exitbind/runs/work-{successor_run_id}.jsonl");
    let successor_start = f.call(&["run", "inspect", &successor_ledger]);
    assert!(successor_start.status.success(), "{successor_start:?}");
    let successor_state: Value = serde_json::from_slice(&successor_start.stdout).unwrap();
    let successor_event = &successor_state["events"][0];
    assert_eq!(successor_event["checkReplacement"]["target"], "pre_return");
    assert_eq!(successor_event["preReturnPolicy"]["command"], "exit 0");
    assert_eq!(successor_event["checkPolicy"], events[0]["checkPolicy"]);
    let predecessor_status = f.call(&["run", "inspect", &ledger, "--json"]);
    assert!(
        predecessor_status.status.success(),
        "{predecessor_status:?}"
    );
    let predecessor_status: Value = serde_json::from_slice(&predecessor_status.stdout).unwrap();
    assert_eq!(
        successor_event["governorCarry"]["accounting"]["spent"],
        predecessor_status["governor"]["spent"]
    );
    assert_eq!(
        successor_event["governorCarry"]["accounting"]["budget"],
        predecessor_status["governor"]["budget"]
    );

    let lead = f.call(&["work", "next", successor, "--json"]);
    let lead: Value = serde_json::from_slice(&lead.stdout).unwrap();
    let scoped = f.work_return(successor, &lead["next"], "scoped", "successor scope");
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = serde_json::from_slice::<Value>(&scoped.stdout).unwrap()["next"].clone();
    let permit = f.call(&[
        "work",
        "permit",
        successor,
        worker["assignment"].as_str().unwrap(),
        "--operation",
        "successor-completion",
    ]);
    assert!(permit.status.success(), "{permit:?}");
    let returned = f.work_return(
        successor,
        &worker,
        "completed",
        "fresh successor worker result",
    );
    assert!(returned.status.success(), "{returned:?}");
    let returned_value: Value = serde_json::from_slice(&returned.stdout).unwrap();
    assert_eq!(returned_value["effect"], "held");
    let prefix = format!("{successor}-{}-", worker["assignment"].as_str().unwrap());
    let retained = fs::read_dir(f.root.join(".exitbind/artifacts"))
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .unwrap()
        .path();
    let retained_bytes = fs::read(&retained).unwrap();
    let before = fs::read(f.root.join(&successor_ledger)).unwrap();
    fs::write(&retained, b"tampered retained result").unwrap();
    let refused = f.work_return(
        successor,
        &worker,
        "completed",
        "fresh successor worker result",
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert!(String::from_utf8_lossy(&refused.stderr)
        .contains("retained worker result artifact bytes changed"));
    assert_eq!(fs::read(&retained).unwrap(), b"tampered retained result");
    assert_eq!(fs::read(f.root.join(&successor_ledger)).unwrap(), before);
    // Restore only this owned failure-injection fixture's original bytes.
    fs::write(&retained, retained_bytes).unwrap();
    // The carried bound needs new evidence, without resetting spent accounting.
    // Observe the replacement command's actual runnability as a bounded probe.
    // This probe earns no pre-return or final-check credit.
    let command = successor_event["preReturnPolicy"]["command"]
        .as_str()
        .unwrap();
    let probe = support::git_topology::command("sh")
        .args(["-c", command])
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(probe.status.success(), "{probe:?}");
    let diagnostic = serde_json::json!({
        "commandSha256": successor_event["preReturnPolicy"]["commandSha256"],
        "successorStartEventSha256": successor_event["eventSha256"],
        "exitCode": probe.status.code(), "stdout": probe.stdout, "stderr": probe.stderr,
        "observation": "replacement command is runnable; final and pre-return evidence still required"
    });
    fs::write(
        f.root.join(".exitbind/artifacts/fresh-diagnostic.json"),
        serde_json::to_vec(&diagnostic).unwrap(),
    )
    .unwrap();
    let evidence = f.call(&[
        "work",
        "evidence",
        successor,
        worker["assignment"].as_str().unwrap(),
        "--artifact-root",
        "state",
        "--artifact",
        ".exitbind/artifacts/fresh-diagnostic.json",
    ]);
    assert!(evidence.status.success(), "{evidence:?}");
    let returned = f.work_return(
        successor,
        &worker,
        "completed",
        "fresh successor worker result",
    );
    assert!(returned.status.success(), "{returned:?}");
    let returned_value: Value = serde_json::from_slice(&returned.stdout).unwrap();
    assert_eq!(
        returned_value["next"]["action"], "check",
        "{returned_value}"
    );
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let check = f.work_check(successor);
    assert!(check.status.success(), "{check:?}");
    let next = f.call(&["work", "next", successor, "--json"]);
    assert!(next.status.success(), "{next:?}");
    let next = serde_json::from_slice::<Value>(&next.stdout).unwrap()["next"].clone();
    let reviewer = f.work_return(successor, &next, "approved", "review approved");
    assert!(reviewer.status.success(), "{reviewer:?}");
    let lead = serde_json::from_slice::<Value>(&reviewer.stdout).unwrap()["next"].clone();
    let accepted = f.work_return(successor, &lead, "accepted", "accepted");
    assert!(accepted.status.success(), "{accepted:?}");
    let final_events = f.events(&successor_ledger);
    assert!(final_events
        .iter()
        .any(|event| event["action"] == "check" && event["result"]["code"] == 0));
    assert!(final_events
        .iter()
        .any(|event| event["action"] == "submit" && event["outcome"] == "approved"));
    assert!(final_events
        .iter()
        .any(|event| event["action"] == "submit" && event["outcome"] == "accepted"));
    let final_status = f.call(&["run", "status", &successor_ledger, "--json"]);
    assert!(final_status.status.success(), "{final_status:?}");
    let final_status: Value = serde_json::from_slice(&final_status.stdout).unwrap();
    assert_eq!(final_status["status"], "accepted");
}

#[test]
fn explicit_checker_script_is_pinned_and_drift_is_rejected_before_attempt_admission() {
    let f = Fixture::new("pre-return-script-drift");
    fs::write(f.root.join("precheck.sh"), "exit 0\n").unwrap();
    f.start_run("sh ./precheck.sh");
    fs::write(f.root.join("precheck.sh"), "exit 1\n").unwrap();
    let before = fs::read(f.root.join(f.ledger())).unwrap();
    let refused = f.submit("worker", "completed", "drift");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("checker source changed"));
    assert_eq!(fs::read(f.root.join(f.ledger())).unwrap(), before);
    assert!(!f.root.join(".exitbind/pre-return").exists());
}

#[test]
fn checker_argument_symlink_redirection_is_rejected_before_attempt_admission() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("pre-return-wrapper-redirection");
    fs::write(f.root.join("check-a.sh"), "exit 0\n").unwrap();
    fs::write(f.root.join("check-b.sh"), "exit 1\n").unwrap();
    symlink("check-a.sh", f.root.join("check-link.sh")).unwrap();
    f.start_run("sh ./check-link.sh");
    fs::remove_file(f.root.join("check-link.sh")).unwrap();
    symlink("check-b.sh", f.root.join("check-link.sh")).unwrap();
    let before = fs::read(f.root.join(f.ledger())).unwrap();
    let refused = f.submit("worker", "completed", "redirected-checker");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("checker source changed"));
    assert_eq!(fs::read(f.root.join(f.ledger())).unwrap(), before);
    assert!(!f.root.join(".exitbind/pre-return").exists());
}

#[test]
fn timeout_capture_launch_and_storage_failures_keep_distinct_durable_categories() {
    let timeout = Fixture::new("pre-return-timeout");
    timeout.start_run_with("sleep 2", &["--pre-return-timeout-ms", "20"]);
    let result = timeout.submit("worker", "completed", "timeout");
    let output: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(output["failureKind"], "timeout");

    let capture = Fixture::new("pre-return-capture-limit");
    capture.start_run_with("printf abc", &["--pre-return-capture-bytes", "1"]);
    let result = capture.submit("worker", "completed", "capture");
    let output: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(output["failureKind"], "capture_failure");

    let launch = Fixture::new("pre-return-launch-failure");
    launch.start_run("exit 0");
    let result = launch.submit_without_path("worker", "completed", "launch");
    let output: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(output["failureKind"], "launch_failure");

    let storage = Fixture::new("pre-return-storage-failure");
    storage.start_run("rm -rf .exitbind/artifacts && touch .exitbind/artifacts");
    let result = storage.submit("worker", "completed", "storage");
    let output: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(output["failureKind"], "storage_failure");
}

#[test]
fn current_configuration_and_prior_artifact_drift_are_rejected_without_submission() {
    let config = Fixture::new("pre-return-config-drift");
    config.start_run("exit 0");
    let config_file = config.root.join("exitbind.json");
    let mut bytes = fs::read(&config_file).unwrap();
    bytes.push(b'\n');
    fs::write(&config_file, bytes).unwrap();
    let before = fs::read(config.root.join(config.ledger())).unwrap();
    let refused = config.submit("worker", "completed", "config-drift");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("frozen current configuration"));
    assert_eq!(fs::read(config.root.join(config.ledger())).unwrap(), before);

    let artifact = Fixture::new("pre-return-artifact-drift");
    artifact.start_run("exit 0");
    fs::write(
        artifact.root.join(".exitbind/artifacts/scope.md"),
        "changed after lead scoping\n",
    )
    .unwrap();
    let before = fs::read(artifact.root.join(artifact.ledger())).unwrap();
    let refused = artifact.submit("worker", "completed", "artifact-drift");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("artifact drift detected"));
    assert_eq!(
        fs::read(artifact.root.join(artifact.ledger())).unwrap(),
        before
    );

    let input = Fixture::new("pre-return-input-drift");
    fs::write(input.root.join("product-input.txt"), "before\n").unwrap();
    input.start_run("exit 0");
    fs::write(input.root.join("product-input.txt"), "after\n").unwrap();
    let fresh = input.submit("worker", "completed", "fresh-input-candidate");
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stderr)
    );
    assert!(input.events(&input.ledger()).iter().any(|event| {
        event["action"] == "submit" && event["role"] == "worker" && event["outcome"] == "completed"
    }));
}

#[test]
fn drift_created_during_checker_execution_blocks_the_append() {
    let source = Fixture::new("pre-return-source-drift-during-check");
    fs::write(
        source.root.join("precheck.sh"),
        "printf 'exit 0\\n' > precheck.sh\nexit 0\n",
    )
    .unwrap();
    source.start_run("sh ./precheck.sh");
    let before = fs::read(source.root.join(source.ledger())).unwrap();
    let refused = source.submit("worker", "completed", "source-changed");
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("changed components: checker,inputs"),
        "{stderr}"
    );
    assert!(
        stderr.contains("inspect the saved attempt with"),
        "{stderr}"
    );
    source.admitted_suffix(&source.ledger(), &before, &source.only_attempt_id());

    let config = Fixture::new("pre-return-config-drift-during-check");
    config.start_run("printf '\\n' >> exitbind.json; exit 0");
    let before = fs::read(config.root.join(config.ledger())).unwrap();
    let refused = config.submit("worker", "completed", "config-changed");
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("changed components: config,inputs"),
        "{stderr}"
    );
    assert!(
        stderr.contains("inspect the saved attempt with"),
        "{stderr}"
    );
    config.admitted_suffix(&config.ledger(), &before, &config.only_attempt_id());

    let artifact = Fixture::new("pre-return-artifact-drift-during-check");
    artifact.start_run("printf changed > .exitbind/artifacts/scope.md; exit 0");
    let before = fs::read(artifact.root.join(artifact.ledger())).unwrap();
    let refused = artifact.submit("worker", "completed", "artifact-changed");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("artifact drift detected"));
    artifact.admitted_suffix(&artifact.ledger(), &before, &artifact.only_attempt_id());

    let input = Fixture::new("pre-return-input-drift-during-check");
    fs::write(input.root.join("product-input.txt"), "before\n").unwrap();
    input.start_run(
        "python3 -c \"from pathlib import Path; Path('product-input.txt').write_text('after\\n')\"; exit 0",
    );
    let before = fs::read(input.root.join(input.ledger())).unwrap();
    let refused = input.submit("worker", "completed", "input-changed-by-checker");
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("pre-return attempt no longer matches the current result binding")
            && stderr.contains("changed components: inputs")
            && stderr.contains("run\",\"pre-return\",\"read\""),
        "{stderr}"
    );
    input.admitted_suffix(&input.ledger(), &before, &input.only_attempt_id());
}
