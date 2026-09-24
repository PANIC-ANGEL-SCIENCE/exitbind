#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const CHECK: &str = "test -f actual-product-check";

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

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("invalid JSON for {args:?}: {error}; {}", text(&output)))
    }

    fn artifact(&self, name: &str, body: &str) -> String {
        let relative = format!(".exitbind/artifacts/{name}.md");
        fs::write(self.root.join(&relative), body).unwrap();
        relative
    }

    fn start(&self, ledger: &str) -> Value {
        let output = self.call(&[
            "run",
            "start",
            "change",
            "--goal",
            "transition conformance",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--config",
            "exitbind.json",
        ]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn start_unchecked(&self, ledger: &str) {
        let output = self.call(&[
            "run",
            "start",
            "change",
            "--goal",
            "unchecked transition",
            "--ledger",
            ledger,
            "--config",
            "exitbind.json",
        ]);
        assert!(output.status.success(), "{}", text(&output));
    }

    fn submit(&self, agent: &str, ledger: &str, outcome: &str, name: &str) -> Output {
        let artifact = self.artifact(name, &format!("{name}\n"));
        self.call(&[
            "run",
            "submit",
            agent,
            ledger,
            "--outcome",
            outcome,
            "--artifact",
            &artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ])
    }

    fn submit_ok(&self, agent: &str, ledger: &str, outcome: &str, name: &str) -> Value {
        let output = self.submit(agent, ledger, outcome, name);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn worker_target(submission: &Value) -> String {
        submission["event"]["eventSha256"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn check(&self, ledger: &str, target: &str, exit_code: u32) -> Output {
        self.call(&[
            "run",
            "record-check",
            ledger,
            "--target",
            target,
            "--check-command",
            CHECK,
            "--exit-code",
            &exit_code.to_string(),
            "--json",
            "--config",
            "exitbind.json",
        ])
    }

    fn check_ok(&self, ledger: &str, target: &str, exit_code: u32) -> Value {
        let output = self.check(ledger, target, exit_code);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn status(&self, ledger: &str) -> Value {
        self.json(&[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "exitbind.json",
        ])
    }

    fn events(&self, ledger: &str) -> Vec<Value> {
        fs::read_to_string(self.root.join(ledger))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn work_begin(&self, goal: &str) -> (String, Value) {
        let value = self.json(&[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--config",
            "exitbind.json",
        ]);
        (
            value["work"].as_str().unwrap().to_owned(),
            value["next"].clone(),
        )
    }

    fn work_return(&self, work: &str, action: &Value, outcome: &str, body: &str) -> Output {
        let assignment = action["assignment"].as_str().unwrap();
        let args = vec![
            "work".to_owned(),
            "return".to_owned(),
            work.to_owned(),
            assignment.to_owned(),
            "--outcome".to_owned(),
            outcome.to_owned(),
            "--config".to_owned(),
            "exitbind.json".to_owned(),
        ];
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn work_return_ok(&self, work: &str, action: &Value, outcome: &str, body: &str) -> Value {
        let output = self.work_return(work, action, outcome, body);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn work_check(&self, work: &str) -> Output {
        self.call(&["work", "check", work, "--config", "exitbind.json"])
    }

    fn work_next(&self, token: &str) -> Value {
        self.json(&[
            "work",
            "next",
            &format!("smw_{token}"),
            "--full",
            "--config",
            "exitbind.json",
        ])
    }

    fn configure_roles(&self, workers: &[&str], reviewers: &[&str]) {
        let path = self.root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let worker = config["agents"]["worker"].clone();
        for name in workers.iter().copied().filter(|name| *name != "worker") {
            let mut agent = worker.clone();
            agent["profile"] = json!(format!("exitbind/agents/{name}.md"));
            agent["purpose"] = json!(format!("Complete bounded work for {name}."));
            config["agents"][name] = agent;
            fs::write(
                self.root.join(format!("exitbind/agents/{name}.md")),
                format!("# {name}\n\nComplete bounded work.\n"),
            )
            .unwrap();
        }
        let reviewer = config["agents"]["reviewer"].clone();
        for name in reviewers.iter().copied().filter(|name| *name != "reviewer") {
            let mut agent = reviewer.clone();
            agent["profile"] = json!(format!("exitbind/agents/{name}.md"));
            agent["purpose"] = json!(format!("Review bounded work for {name}."));
            config["agents"][name] = agent;
            fs::write(
                self.root.join(format!("exitbind/agents/{name}.md")),
                format!("# {name}\n\nReview bounded work.\n"),
            )
            .unwrap();
        }
        config["workflows"]["change"]["workers"] = json!(workers);
        config["workflows"]["change"]["reviewers"] = json!(reviewers);
        fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    }

    fn accepted_single(&self, ledger: &str) -> String {
        self.start(ledger);
        self.submit_ok("lead", ledger, "scoped", "scope");
        let worker = self.submit_ok("worker", ledger, "completed", "worker");
        let target = Self::worker_target(&worker);
        self.check_ok(ledger, &target, 0);
        self.submit_ok("reviewer", ledger, "approved", "review");
        self.submit_ok("lead", ledger, "accepted", "accept");
        let path = ".exitbind/receipts/accepted.json";
        let output = self.call(&[
            "receipt",
            ledger,
            "--output",
            path,
            "--config",
            "exitbind.json",
        ]);
        assert!(output.status.success(), "{}", text(&output));
        path.to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct LegacyFixture {
    root: PathBuf,
}

impl LegacyFixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .current_dir(&self.root)
            .args(args)
            .output()
            .unwrap()
    }

    fn start(&self, ledger: &str, check: bool, harness: Option<&str>) {
        let mut args = vec![
            "run",
            "start",
            "change",
            "--goal",
            "historical transition",
            "--ledger",
            ledger,
        ];
        if check {
            args.extend(["--check-command", CHECK]);
        }
        if let Some(harness) = harness {
            args.extend(["--harness-receipt", harness]);
        }
        args.extend(["--config", "soulmate.json"]);
        let output = self.call(&args);
        assert!(output.status.success(), "{}", text(&output));
    }

    fn artifact(&self, name: &str) -> String {
        let relative = format!(".soulmate/artifacts/{name}.md");
        fs::write(self.root.join(&relative), format!("{name}\n")).unwrap();
        relative
    }

    fn submit(&self, agent: &str, ledger: &str, outcome: &str, name: &str) -> Value {
        let artifact = self.artifact(name);
        let output = self.call(&[
            "run",
            "submit",
            agent,
            ledger,
            "--outcome",
            outcome,
            "--artifact",
            &artifact,
            "--artifact-root",
            "state",
            "--config",
            "soulmate.json",
        ]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn check(&self, ledger: &str, target: &str, exit_code: u32) {
        let output = self.call(&[
            "run",
            "record-check",
            ledger,
            "--target",
            target,
            "--check-command",
            CHECK,
            "--exit-code",
            &exit_code.to_string(),
            "--json",
            "--config",
            "soulmate.json",
        ]);
        assert!(output.status.success(), "{}", text(&output));
    }

    fn status(&self, ledger: &str) -> Value {
        let output = self.call(&[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn next(&self, ledger: &str) -> Value {
        let output = self.call(&["run", "next", ledger, "--json", "--config", "soulmate.json"]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn work_next(&self, token: &str) -> Value {
        let output = self.call(&[
            "work",
            "next",
            &format!("smw_{token}"),
            "--config",
            "soulmate.json",
        ]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for LegacyFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_exit_1(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{}", text(output));
}

fn assert_receipt_refused(output: &Output, detail: &str) {
    assert_exit_1(output);
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid refusal JSON: {error}; {}", text(output)));
    assert_eq!(value["outcome"], "REFUSED");
    assert_eq!(value["reason"]["code"], "receipt_mismatch");
    assert!(
        value["reason"]["detail"].to_string().contains(detail),
        "{value}"
    );
}

fn assert_receipt_valid(f: &Fixture, path: &str) {
    let output = f.call(&["verify", path, "--config", "exitbind.json"]);
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["outcome"], "READY");
}

fn assert_receipt_variant_refused(f: &Fixture, original: &str, variant: &str, detail: &str) {
    assert_receipt_valid(f, original);
    let output = f.call(&["verify", variant, "--config", "exitbind.json"]);
    assert_receipt_refused(&output, detail);
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap();
}

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

fn value_hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

fn assert_not_ready_progress(value: &Value, code: &str) {
    assert_eq!(value["progress"]["applicable"], false);
    assert!(value["progress"]["percent"].is_null());
    assert_eq!(value["progress"]["state"], "NOT_APPLICABLE");
    assert_eq!(value["progress"]["reason"]["code"], code);
}

fn assert_receipt_matches_progress(output: &Output, progress: &Value) {
    assert_exit_1(output);
    let receipt: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid receipt projection: {error}; {}", text(output)));
    assert_eq!(receipt["outcome"], progress["state"]);
    assert_eq!(receipt["reason"]["code"], progress["reason"]["code"]);
}

#[test]
fn stale_subject_check_is_missing_until_reobserved_at_current_subject() {
    let f = Fixture::new("transition-stale-subject");
    f.configure_roles(&["worker", "worker_two"], &["reviewer"]);
    let ledger = ".exitbind/runs/stale.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let a = f.submit_ok("worker", ledger, "completed", "worker-a");
    let a_target = Fixture::worker_target(&a);
    f.check_ok(ledger, &a_target, 0);
    let b = f.submit_ok("worker_two", ledger, "completed", "worker-b");
    let b_target = Fixture::worker_target(&b);
    f.check_ok(ledger, &b_target, 0);
    f.submit_ok("reviewer", ledger, "approved", "review");

    let before = f.events(ledger);
    let refused = f.submit("lead", ledger, "accepted", "stale-acceptance");
    assert_exit_1(&refused);
    assert!(
        text(&refused).contains("check_missing"),
        "{}",
        text(&refused)
    );
    let after = f.events(ledger);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after.last().unwrap()["action"], "protect");
    assert_eq!(after.last().unwrap()["reason"], "check_missing");
    let status = f.status(ledger);
    let targets = status["checks"]["targets"].as_array().unwrap();
    assert_eq!(targets[0]["status"], "missing");
    assert_eq!(targets[1]["status"], "passed");
    let next = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    let receipt = f.call(&["receipt", ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &next["progress"]);

    // The same A target is valid again only when the new observation binds S2.
    f.check_ok(ledger, &a_target, 0);
    let accepted = f.submit("lead", ledger, "accepted", "accept-current-subject");
    assert!(accepted.status.success(), "{}", text(&accepted));
    assert_eq!(
        serde_json::from_slice::<Value>(&accepted.stdout).unwrap()["status"],
        "accepted"
    );
}

#[test]
fn stale_subject_recovery_is_visible_through_the_work_facade() {
    let f = Fixture::new("transition-stale-subject-work");
    f.configure_roles(&["worker", "worker_two"], &["reviewer"]);
    let (work, mut action) = f.work_begin("work stale subject");
    assert!(work.starts_with("smw_"));
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    assert_eq!(action["role"], "worker");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    action = f.work_return_ok(&work, &action, "completed", "worker-a")["next"].clone();
    assert_eq!(action["action"], "check");
    let first_check = f.work_check(&work);
    assert!(first_check.status.success(), "{}", text(&first_check));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["agent"], "worker_two", "{action}");
    action = f.work_return_ok(&work, &action, "completed", "worker-b")["next"].clone();
    assert_eq!(action["action"], "check");
    assert_eq!(action["progress"]["state"], "BLOCKED");
    assert_eq!(action["progress"]["reason"]["code"], "check_missing");
    let work_ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let receipt = f.call(&["receipt", &work_ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &action["progress"]);

    // The façade selects A's stale target first, then B's missing target.
    let repair_a = f.work_check(&work);
    assert!(repair_a.status.success(), "{}", text(&repair_a));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["action"], "check");
    assert_eq!(action["progress"]["state"], "BLOCKED");
    let repair_b = f.work_check(&work);
    assert!(repair_b.status.success(), "{}", text(&repair_b));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["role"], "reviewer");
    assert_eq!(action["progress"]["state"], "IN_PROGRESS");
    action = f.work_return_ok(&work, &action, "approved", "review")["next"].clone();
    let accepted = f.work_return_ok(&work, &action, "accepted", "acceptance");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");

    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let events = f.events(&ledger);
    assert!(ledger
        .strip_prefix(".exitbind/runs/")
        .is_some_and(|name| name.starts_with("work-")));
    let checks = events
        .iter()
        .filter(|event| event["action"] == "check")
        .collect::<Vec<_>>();
    assert_eq!(checks.len(), 3);
    let s1 = checks[0]["subjectSha256"].as_str().unwrap();
    let s2 = checks[1]["subjectSha256"].as_str().unwrap();
    assert_ne!(s1, s2);
    assert_eq!(checks[2]["subjectSha256"], s2);
    assert_eq!(checks[0]["acquisition"], "observed");
    assert_eq!(checks[1]["acquisition"], "observed");
    assert_eq!(checks[2]["acquisition"], "observed");
    assert_eq!(
        checks[0]["targetEventSha256"],
        checks[1]["targetEventSha256"]
    );
    assert_ne!(
        checks[1]["targetEventSha256"],
        checks[2]["targetEventSha256"]
    );
}

#[test]
fn caller_reported_failed_check_reaches_rework_and_fresh_low_level_attempt() {
    let f = Fixture::new("transition-low-level-rework");
    let ledger = ".exitbind/runs/low-level-rework.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let first = f.submit_ok("worker", ledger, "completed", "attempt-one");
    let first_target = Fixture::worker_target(&first);
    let failed = f.check(ledger, &first_target, 1);
    assert!(failed.status.success(), "{}", text(&failed));
    let status = f.status(ledger);
    assert_eq!(status["checks"]["status"], "blocked");
    assert_eq!(status["checks"]["targets"][0]["status"], "failed");
    let next = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    let receipt = f.call(&["receipt", ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &next["progress"]);
    assert_eq!(next["progress"]["state"], "REFUSED");
    assert_eq!(next["progress"]["reason"]["code"], "check_failed");
    assert_eq!(next["assignments"][0]["role"], "reviewer");

    f.submit_ok("reviewer", ledger, "rework", "rework");
    let attempt_two = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    assert_eq!(attempt_two["attempt"], 2);
    assert_eq!(attempt_two["progress"]["state"], "IN_PROGRESS");
    let second = f.submit_ok("worker", ledger, "completed", "attempt-two");
    let second_target = Fixture::worker_target(&second);
    let fresh = f.check(ledger, &second_target, 0);
    assert!(fresh.status.success(), "{}", text(&fresh));
    f.submit_ok("reviewer", ledger, "approved", "review-two");
    let accepted = f.submit("lead", ledger, "accepted", "accept-two");
    assert!(accepted.status.success(), "{}", text(&accepted));

    let events = f.events(ledger);
    assert!(events.iter().any(|event| {
        event["action"] == "check"
            && event["acquisition"] == "reported"
            && event["result"]["kind"] == "exit"
            && event["result"]["code"] == 1
    }));
    let subjects = events
        .iter()
        .filter(|event| event["action"] == "submit" && event["role"] == "worker")
        .map(|event| event["subjectSha256"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(subjects.len(), 2);
    assert_ne!(subjects[0], subjects[1]);
}

#[test]
fn real_attempt_one_reviewer_assignment_is_rejected_on_attempt_two_by_work_facade() {
    let f = Fixture::new("transition-work-stale-review");
    let (work, mut action) = f.work_begin("work stale review");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let _ = f.work_return_ok(&work, &action, "completed", "attempt-one");
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    let reviewer_one = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(reviewer_one["role"], "reviewer");
    let reworked = f.work_return_ok(&work, &reviewer_one, "rework", "rework");
    assert_eq!(reworked["next"]["requiresExpansion"], true);
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["packet"]["attempt"], 2);
    let _ = f.work_return_ok(&work, &action, "completed", "attempt-two");
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    let reviewer_two = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(reviewer_two["role"], "reviewer");
    assert_ne!(reviewer_one["assignment"], reviewer_two["assignment"]);
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let before = f.events(&ledger);
    let stale = f.work_return(&work, &reviewer_one, "approved", "stale-review");
    assert_exit_1(&stale);
    assert!(text(&stale).contains("assignment is not the current pending work action"));
    assert_eq!(f.events(&ledger), before);
    let accepted_review = f.work_return_ok(&work, &reviewer_two, "approved", "review-two");
    let accepted = f.work_return_ok(&work, &accepted_review["next"], "accepted", "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
}

#[test]
fn no_reviewer_configuration_reaches_canonical_missing_review_guard() {
    let f = Fixture::new("transition-missing-review-canonical");
    f.configure_roles(&["worker"], &[]);
    let ledger = ".exitbind/runs/no-reviewer.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let worker = f.submit_ok("worker", ledger, "completed", "worker");
    f.check_ok(ledger, &Fixture::worker_target(&worker), 0);
    let next = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    assert_eq!(next["assignments"][0]["role"], "lead");
    let before = f.events(ledger);
    let missing = f.submit("lead", ledger, "accepted", "missing-review");
    assert_exit_1(&missing);
    assert!(text(&missing).contains("canonical acceptance requires reviewer approval"));
    assert_eq!(f.events(ledger), before);

    // The same checked, no-reviewer configuration is visible through the
    // facade: work next exposes the pending lead decision, but work return
    // reaches the canonical missing-review guard without appending an event.
    let facade = Fixture::new("transition-missing-review-work");
    facade.configure_roles(&["worker"], &[]);
    let (work, mut action) = facade.work_begin("work missing review");
    action = facade.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    let _ = facade.work_return_ok(&work, &action, "completed", "worker");
    fs::write(facade.root.join("actual-product-check"), b"pass").unwrap();
    let checked = facade.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    let checked_value: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked_value["next"]["requiresExpansion"], true);
    let pending = facade.work_next(&work[4..])["next"].clone();
    assert_eq!(pending["action"], "lead_decision");
    assert_eq!(pending["progress"]["state"], "IN_PROGRESS");
    assert_eq!(
        pending["progress"]["reason"]["code"],
        "prerequisites_incomplete"
    );
    assert_eq!(checked_value["next"]["assignment"], pending["assignment"]);
    assert_eq!(checked_value["next"]["action"], pending["action"]);
    let facade_ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let facade_before = facade.events(&facade_ledger);
    let facade_missing = facade.work_return(&work, &pending, "accepted", "missing-review");
    assert_exit_1(&facade_missing);
    assert!(text(&facade_missing).contains("canonical acceptance requires reviewer approval"));
    assert_eq!(facade.events(&facade_ledger), facade_before);

    // The unchanged normal configuration is the paired valid control.
    let control = Fixture::new("transition-missing-review-control");
    let receipt = control.accepted_single(".exitbind/runs/control.jsonl");
    let verified = control.call(&["verify", &receipt, "--config", "exitbind.json"]);
    assert!(verified.status.success(), "{}", text(&verified));
    assert_eq!(
        serde_json::from_slice::<Value>(&verified.stdout).unwrap()["outcome"],
        "READY"
    );

    let (control_work, mut control_action) = control.work_begin("work reviewed control");
    control_action =
        control.work_return_ok(&control_work, &control_action, "scoped", "scope")["next"].clone();
    let _ = control.work_return_ok(&control_work, &control_action, "completed", "worker");
    fs::write(control.root.join("actual-product-check"), b"pass").unwrap();
    let control_checked = control.work_check(&control_work);
    assert!(
        control_checked.status.success(),
        "{}",
        text(&control_checked)
    );
    control_action =
        serde_json::from_slice::<Value>(&control_checked.stdout).unwrap()["next"].clone();
    control_action = control.work_return_ok(&control_work, &control_action, "approved", "review")
        ["next"]
        .clone();
    let control_accepted =
        control.work_return_ok(&control_work, &control_action, "accepted", "acceptance");
    assert_eq!(control_accepted["next"]["progress"]["state"], "READY");
}

#[test]
fn work_facade_drives_multi_worker_multi_reviewer_receipt() {
    let f = Fixture::new("transition-work-receipt");
    f.configure_roles(&["worker", "worker_two"], &["reviewer", "reviewer_two"]);
    let (work, mut action) = f.work_begin("work receipt completeness");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    assert_eq!(action["role"], "worker");
    action = f.work_return_ok(&work, &action, "completed", "worker")["next"].clone();
    assert_eq!(action["action"], "check");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let first_check = f.work_check(&work);
    assert!(first_check.status.success(), "{}", text(&first_check));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["agent"], "worker_two", "{action}");
    action = f.work_return_ok(&work, &action, "completed", "worker_two")["next"].clone();
    assert_eq!(action["action"], "check");
    let repair_first = f.work_check(&work);
    assert!(repair_first.status.success(), "{}", text(&repair_first));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["action"], "check");
    let second_check = f.work_check(&work);
    assert!(second_check.status.success(), "{}", text(&second_check));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["role"], "reviewer");
    let approved = f.work_return_ok(&work, &action, "approved", "reviewer");
    assert_eq!(approved["next"]["requiresExpansion"], true);
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["agent"], "reviewer_two");
    action = f.work_return_ok(&work, &action, "approved", "reviewer_two")["next"].clone();
    let accepted = f.work_return_ok(&work, &action, "accepted", "acceptance");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");

    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let receipt_path = ".exitbind/receipts/work-multi.json";
    let receipt = f.call(&[
        "receipt",
        &ledger,
        "--output",
        receipt_path,
        "--config",
        "exitbind.json",
    ]);
    assert!(receipt.status.success(), "{}", text(&receipt));
    let receipt_value: Value =
        serde_json::from_slice(&fs::read(f.root.join(receipt_path)).unwrap()).unwrap();
    assert_eq!(receipt_value["artifacts"].as_array().unwrap().len(), 5);
    assert_eq!(
        receipt_value["check"]["targets"].as_array().unwrap().len(),
        2
    );
    let events = f.events(&ledger);
    let last_reviewer = events
        .iter()
        .rev()
        .find(|event| event["role"] == "reviewer" && event["outcome"] == "approved")
        .unwrap();
    assert_eq!(
        receipt_value["review"]["eventSha256"],
        last_reviewer["eventSha256"]
    );
    assert_eq!(
        receipt_value["review"]["artifactSha256"],
        last_reviewer["artifact"]["sha256"]
    );
    let verified = f.call(&["verify", receipt_path, "--config", "exitbind.json"]);
    assert!(verified.status.success(), "{}", text(&verified));
    assert_eq!(
        serde_json::from_slice::<Value>(&verified.stdout).unwrap()["outcome"],
        "READY"
    );
}

#[test]
fn failed_work_check_reaches_rework_then_only_fresh_attempt_reaches_ready() {
    let f = Fixture::new("transition-work-rework");
    let (work, mut action) = f.work_begin("failed check recovery");
    assert_eq!(action["action"], "lead_decision");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    assert_eq!(action["action"], "spawn");
    assert_eq!(action["role"], "worker");
    action = f.work_return_ok(&work, &action, "completed", "attempt one")["next"].clone();
    assert_eq!(action["action"], "check");
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    let checked_value: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked_value["next"]["progress"]["state"], "REFUSED");
    assert_eq!(
        checked_value["next"]["progress"]["reason"]["code"],
        "check_failed"
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let receipt = f.call(&["receipt", &ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &checked_value["next"]["progress"]);
    action = checked_value["next"].clone();
    assert_eq!(action["action"], "spawn");
    assert_eq!(action["role"], "reviewer");
    let rework = f.work_return_ok(&work, &action, "rework", "repair request");
    assert_eq!(rework["next"]["requiresExpansion"], true);
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["action"], "spawn");
    assert_eq!(action["role"], "worker");
    assert_eq!(action["packet"]["attempt"], 2);
    assert_eq!(
        action["packet"]["upstreamArtifacts"][0]["attemptStatus"],
        "prior"
    );
    action = f.work_return_ok(&work, &action, "completed", "attempt two")["next"].clone();
    assert_eq!(action["action"], "check");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    action = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(action["role"], "reviewer");
    action = f.work_return_ok(&work, &action, "approved", "fresh review")["next"].clone();
    assert_eq!(action["action"], "lead_decision");
    let accepted = f.work_return_ok(&work, &action, "accepted", "lead acceptance");
    assert_eq!(accepted["next"]["action"], "done");
    let accepted_next = f.work_next(&work[4..]);
    assert_eq!(accepted_next["next"]["status"], "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    assert_eq!(accepted_next["next"]["progress"]["percent"], 100);
}

#[test]
fn missing_and_fabricated_review_calls_preserve_ledger_until_current_reviewer_controls() {
    let f = Fixture::new("transition-review-authority");
    let (work, mut action) = f.work_begin("review authority");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    let _ = f.work_return_ok(&work, &action, "completed", "worker");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    action = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(action["role"], "reviewer");

    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let before = f.events(&ledger);
    let fabricated = f.work_return(
        &work,
        &json!({"assignment": format!("sma_{}", "0".repeat(64))}),
        "approved",
        "fabricated review",
    );
    assert_exit_1(&fabricated);
    assert!(text(&fabricated).contains("assignment is not the current pending work action"));
    assert_eq!(f.events(&ledger), before);

    let missing = f.submit("lead", &ledger, "accepted", "missing-review");
    assert_exit_1(&missing);
    assert!(
        text(&missing).contains("agent 'lead' is not currently pending"),
        "{}",
        text(&missing)
    );
    assert_eq!(f.events(&ledger), before);

    action = f.work_return_ok(&work, &action, "approved", "real current review")["next"].clone();
    assert_eq!(action["action"], "lead_decision");
    let accepted = f.work_return_ok(&work, &action, "accepted", "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
}

#[test]
fn attempt_one_review_does_not_authorize_attempt_two_until_fresh_review() {
    let f = Fixture::new("transition-stale-review");
    let ledger = ".exitbind/runs/attempts.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let first = f.submit_ok("worker", ledger, "completed", "attempt-one");
    let first_target = Fixture::worker_target(&first);
    f.check_ok(ledger, &first_target, 0);
    f.submit_ok("reviewer", ledger, "approved", "review-one");
    f.submit_ok("lead", ledger, "rework", "request-rework");
    let second = f.submit_ok("worker", ledger, "completed", "attempt-two");
    let second_target = Fixture::worker_target(&second);
    f.check_ok(ledger, &second_target, 0);

    let status = f.status(ledger);
    assert_eq!(status["attempt"], 2);
    assert_eq!(status["review"]["status"], "absent");
    assert_eq!(status["checks"]["status"], "passed");
    let before = f.events(ledger);
    let stale = f.submit("lead", ledger, "accepted", "stale-review-acceptance");
    assert_exit_1(&stale);
    assert!(
        text(&stale).contains("not currently pending"),
        "{}",
        text(&stale)
    );
    assert_eq!(f.events(ledger), before);

    f.submit_ok("reviewer", ledger, "approved", "review-two");
    let accepted = f.submit("lead", ledger, "accepted", "accepted-two");
    assert!(accepted.status.success(), "{}", text(&accepted));
    assert_eq!(
        serde_json::from_slice::<Value>(&accepted.stdout).unwrap()["status"],
        "accepted"
    );
}

#[test]
fn multi_receipt_binds_all_current_artifacts_and_keeps_last_review_singular() {
    let f = Fixture::new("transition-receipt-completeness");
    f.configure_roles(&["worker", "worker_two"], &["reviewer", "reviewer_two"]);
    let ledger = ".exitbind/runs/multi.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let mut targets = Vec::new();
    for worker in ["worker", "worker_two"] {
        let submission = f.submit_ok(worker, ledger, "completed", worker);
        targets.push(Fixture::worker_target(&submission));
    }
    for target in &targets {
        f.check_ok(ledger, target, 0);
    }
    for reviewer in ["reviewer", "reviewer_two"] {
        f.submit_ok(reviewer, ledger, "approved", reviewer);
    }
    f.submit_ok("lead", ledger, "accepted", "acceptance");
    let receipt_path = ".exitbind/receipts/multi.json";
    let receipt = f.call(&[
        "receipt",
        ledger,
        "--output",
        receipt_path,
        "--config",
        "exitbind.json",
    ]);
    assert!(receipt.status.success(), "{}", text(&receipt));
    let original: Value =
        serde_json::from_slice(&fs::read(f.root.join(receipt_path)).unwrap()).unwrap();
    assert_eq!(original["outcome"], "READY");
    assert_eq!(original["check"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(original["artifacts"].as_array().unwrap().len(), 5);
    let artifact_paths = original["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        artifact_paths,
        vec![
            ".exitbind/artifacts/worker.md",
            ".exitbind/artifacts/worker_two.md",
            ".exitbind/artifacts/reviewer.md",
            ".exitbind/artifacts/reviewer_two.md",
            ".exitbind/artifacts/acceptance.md",
        ]
    );
    assert_eq!(original["review"].as_object().unwrap().len(), 2);
    assert!(original["review"].get("eventSha256").is_some());
    assert!(original["review"].get("artifactSha256").is_some());
    assert_receipt_valid(&f, receipt_path);

    // Every current worker/reviewer/acceptance artifact gets omission and hash-tamper cells.
    let artifact_count = original["artifacts"].as_array().unwrap().len();
    for index in 0..artifact_count {
        let mut omitted = original.clone();
        omitted["artifacts"].as_array_mut().unwrap().remove(index);
        let omitted_path = format!(".exitbind/receipts/multi-omitted-{index}.json");
        write_json(&f.root.join(&omitted_path), &omitted);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &omitted_path,
            "receipt artifacts binding changed",
        );

        let mut tampered = original.clone();
        tampered["artifacts"][index]["sha256"] = json!("0".repeat(64));
        let tampered_path = format!(".exitbind/receipts/multi-tampered-{index}.json");
        write_json(&f.root.join(&tampered_path), &tampered);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &tampered_path,
            "receipt artifacts binding changed",
        );
    }

    // Every worker check target gets omission and target-hash tamper cells.
    let target_count = original["check"]["targets"].as_array().unwrap().len();
    for index in 0..target_count {
        let mut omitted = original.clone();
        omitted["check"]["targets"]
            .as_array_mut()
            .unwrap()
            .remove(index);
        let omitted_path = format!(".exitbind/receipts/multi-check-omitted-{index}.json");
        write_json(&f.root.join(&omitted_path), &omitted);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &omitted_path,
            "receipt check binding changed",
        );

        let mut tampered = original.clone();
        tampered["check"]["targets"][index]["targetEventSha256"] = json!("0".repeat(64));
        let tampered_path = format!(".exitbind/receipts/multi-check-tampered-{index}.json");
        write_json(&f.root.join(&tampered_path), &tampered);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &tampered_path,
            "receipt check binding changed",
        );
    }

    // The singular review binding gets omission, fabrication, event, and artifact cells.
    let review_variants = ["missing", "fabricated", "event", "artifact"];
    for variant in review_variants {
        let mut mutated = original.clone();
        match variant {
            "missing" => {
                mutated.as_object_mut().unwrap().remove("review");
            }
            "fabricated" => {
                mutated["review"] = json!({
                    "eventSha256": "0".repeat(64),
                    "artifactSha256": "1".repeat(64)
                })
            }
            "event" => mutated["review"]["eventSha256"] = json!("0".repeat(64)),
            "artifact" => mutated["review"]["artifactSha256"] = json!("0".repeat(64)),
            _ => unreachable!(),
        }
        let path = format!(".exitbind/receipts/multi-review-{variant}.json");
        write_json(&f.root.join(&path), &mutated);
        assert_receipt_variant_refused(&f, receipt_path, &path, "receipt review binding changed");
    }
}

#[test]
fn historical_v1_to_v4_and_unchecked_v5_remain_non_applicable_with_checked_v5_control() {
    let legacy = |label: &str| {
        let root = support::temp(label);
        let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        root
    };

    let v1_root = legacy("transition-history-v1");
    let v1 = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v1",
            "--ledger",
            ".soulmate/runs/v1.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v1.status.success(), "{}", text(&v1));
    let v1_status = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v1.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v1_status.status.success(), "{}", text(&v1_status));
    let v1_next = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v1.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v1_next.status.success(), "{}", text(&v1_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v1_next.stdout).unwrap(),
        "historical_run",
    );
    let old_receipt = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "receipt",
            ".soulmate/runs/v1.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_receipt_matches_progress(
        &old_receipt,
        &serde_json::from_slice::<Value>(&v1_next.stdout).unwrap()["progress"],
    );

    let v2_root = legacy("transition-history-v2");
    let manifest = json!({
        "$schema": "https://raw.githubusercontent.com/envy404/soulmate/v0.4.0/schema/harness-manifest.schema.json",
        "version": 1,
        "project": {"id": "history", "session": "history"},
        "harness": {"name": "history", "version": "2026.09.13"},
        "activations": [{"kind": "skill", "name": "soulmate", "evidence": "configured"}]
    });
    write_json(&v2_root.join("harness-manifest.json"), &manifest);
    let planned = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "plan",
            "change",
            "--goal",
            "v2",
            "--receipt",
            ".soulmate/harness-receipt.json",
            "--harness-manifest",
            "harness-manifest.json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(planned.status.success(), "{}", text(&planned));
    let v2 = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v2",
            "--ledger",
            ".soulmate/runs/v2.jsonl",
            "--harness-receipt",
            ".soulmate/harness-receipt.json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v2.status.success(), "{}", text(&v2));
    let v2_status = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v2.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v2_status.status.success(), "{}", text(&v2_status));
    let v2_next = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v2.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v2_next.status.success(), "{}", text(&v2_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v2_next.stdout).unwrap(),
        "historical_run",
    );
    let v2_receipt = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "receipt",
            ".soulmate/runs/v2.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_exit_1(&v2_receipt);

    let v3_root = legacy("transition-history-v3");
    let v3_start = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v3",
            "--ledger",
            ".soulmate/runs/v3.jsonl",
            "--check-command",
            CHECK,
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v3_start.status.success(), "{}", text(&v3_start));
    let v3_path = v3_root.join(".soulmate/runs/v3.jsonl");
    let mut v3_event: Value = serde_json::from_str(&fs::read_to_string(&v3_path).unwrap()).unwrap();
    v3_event["version"] = json!(3);
    v3_event["eventSha256"] = json!(value_hash(&without(&v3_event, "eventSha256")));
    fs::write(
        &v3_path,
        format!("{}\n", serde_json::to_string(&v3_event).unwrap()),
    )
    .unwrap();
    let v3_status = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v3.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v3_status.status.success(), "{}", text(&v3_status));
    let v3_next = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v3.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v3_next.status.success(), "{}", text(&v3_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v3_next.stdout).unwrap(),
        "historical_run",
    );
    let v3_receipt = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "receipt",
            ".soulmate/runs/v3.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_exit_1(&v3_receipt);

    let v4_root = legacy("transition-history-v4");
    let v4 = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v4",
            "--ledger",
            ".soulmate/runs/v4.jsonl",
            "--check-command",
            CHECK,
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v4.status.success(), "{}", text(&v4));
    let v4_status = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v4.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v4_status.status.success(), "{}", text(&v4_status));
    let v4_next = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v4.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v4_next.status.success(), "{}", text(&v4_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v4_next.stdout).unwrap(),
        "historical_run",
    );
    let v4_receipt = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "receipt",
            ".soulmate/runs/v4.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_exit_1(&v4_receipt);

    let unchecked = Fixture::new("transition-unchecked-v5");
    let unchecked_start = unchecked.call(&[
        "run",
        "start",
        "change",
        "--goal",
        "unchecked",
        "--ledger",
        ".exitbind/runs/unchecked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert!(
        unchecked_start.status.success(),
        "{}",
        text(&unchecked_start)
    );
    let _unchecked_status = unchecked.status(".exitbind/runs/unchecked.jsonl");
    let unchecked_next = unchecked.json(&[
        "run",
        "next",
        ".exitbind/runs/unchecked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_not_ready_progress(&unchecked_next, "unchecked_run");
    let unchecked_receipt = unchecked.call(&[
        "receipt",
        ".exitbind/runs/unchecked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_receipt_matches_progress(&unchecked_receipt, &unchecked_next["progress"]);

    let checked = Fixture::new("transition-checked-v5-control");
    let receipt = checked.accepted_single(".exitbind/runs/checked.jsonl");
    let checked_status = checked.json(&[
        "run",
        "next",
        ".exitbind/runs/checked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(checked_status["progress"]["applicable"], true);
    assert_eq!(checked_status["progress"]["state"], "READY");
    let verified = checked.call(&["verify", &receipt, "--config", "exitbind.json"]);
    assert!(verified.status.success(), "{}", text(&verified));
    assert_eq!(
        serde_json::from_slice::<Value>(&verified.stdout).unwrap()["outcome"],
        "READY"
    );

    let _ = fs::remove_dir_all(v1_root);
    let _ = fs::remove_dir_all(v2_root);
    let _ = fs::remove_dir_all(v3_root);
    let _ = fs::remove_dir_all(v4_root);
}

fn assert_accepted_historical(legacy: &LegacyFixture, token: &str, ledger: &str) {
    let status = legacy.status(ledger);
    assert_eq!(status["status"], "accepted");
    let next = legacy.next(ledger);
    assert_eq!(next["status"], "accepted");
    assert_eq!(next["progress"]["applicable"], false);
    assert!(next["progress"]["percent"].is_null());
    assert_eq!(next["progress"]["state"], "NOT_APPLICABLE");
    assert_eq!(next["progress"]["reason"]["code"], "historical_run");
    let facade = legacy.work_next(token);
    assert_eq!(facade["next"]["action"], "done");
    assert_eq!(facade["next"]["status"], "accepted");
    assert_eq!(facade["next"]["progress"]["state"], "NOT_APPLICABLE");
    assert!(facade["next"]["progress"]["percent"].is_null());
    assert_eq!(
        facade["next"]["progress"]["reason"]["code"],
        "historical_run"
    );
    let receipt = legacy.call(&["receipt", ledger, "--config", "soulmate.json"]);
    assert_receipt_matches_progress(&receipt, &next["progress"]);
}

fn downgrade_start_to_v3(path: &Path) {
    let mut event: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    event["version"] = json!(3);
    event["eventSha256"] = json!(value_hash(&without(&event, "eventSha256")));
    fs::write(
        path,
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();
}

#[test]
fn accepted_historical_and_unchecked_states_project_through_work_next_without_ready_upgrade() {
    let token_v1 = "1".repeat(64);
    let v1 = LegacyFixture::new("transition-accepted-v1");
    let ledger_v1 = format!(".soulmate/runs/work-{token_v1}.jsonl");
    v1.start(&ledger_v1, false, None);
    v1.submit("lead", &ledger_v1, "scoped", "v1-scope");
    v1.submit("worker", &ledger_v1, "completed", "v1-worker");
    v1.submit("reviewer", &ledger_v1, "approved", "v1-review");
    v1.submit("lead", &ledger_v1, "accepted", "v1-accept");
    assert_accepted_historical(&v1, &token_v1, &ledger_v1);

    let token_v2 = "2".repeat(64);
    let v2 = LegacyFixture::new("transition-accepted-v2");
    let manifest = json!({
        "$schema": "https://raw.githubusercontent.com/envy404/soulmate/v0.4.0/schema/harness-manifest.schema.json",
        "version": 1,
        "project": {"id": "accepted-v2", "session": "accepted-v2"},
        "harness": {"name": "history", "version": "2026.09.13"},
        "activations": [{"kind": "skill", "name": "soulmate", "evidence": "configured"}]
    });
    write_json(&v2.root.join("harness-manifest.json"), &manifest);
    let planned = v2.call(&[
        "plan",
        "change",
        "--goal",
        "v2",
        "--receipt",
        ".soulmate/harness-receipt.json",
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        "soulmate.json",
    ]);
    assert!(planned.status.success(), "{}", text(&planned));
    let ledger_v2 = format!(".soulmate/runs/work-{token_v2}.jsonl");
    v2.start(&ledger_v2, false, Some(".soulmate/harness-receipt.json"));
    v2.submit("lead", &ledger_v2, "scoped", "v2-scope");
    v2.submit("worker", &ledger_v2, "completed", "v2-worker");
    v2.submit("reviewer", &ledger_v2, "approved", "v2-review");
    v2.submit("lead", &ledger_v2, "accepted", "v2-accept");
    assert_accepted_historical(&v2, &token_v2, &ledger_v2);

    let token_v3 = "3".repeat(64);
    let v3 = LegacyFixture::new("transition-accepted-v3");
    let ledger_v3 = format!(".soulmate/runs/work-{token_v3}.jsonl");
    v3.start(&ledger_v3, true, None);
    downgrade_start_to_v3(&v3.root.join(&ledger_v3));
    v3.submit("lead", &ledger_v3, "scoped", "v3-scope");
    let worker_v3 = v3.submit("worker", &ledger_v3, "completed", "v3-worker");
    let events_v3: Vec<Value> = fs::read_to_string(v3.root.join(&ledger_v3))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let target_v3 = events_v3
        .iter()
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(worker_v3["event"]["version"], 3);
    v3.check(&ledger_v3, &target_v3, 0);
    v3.submit("reviewer", &ledger_v3, "approved", "v3-review");
    v3.submit("lead", &ledger_v3, "accepted", "v3-accept");
    assert_accepted_historical(&v3, &token_v3, &ledger_v3);

    let frozen = LegacyFixture::new("transition-frozen-v3-bytes");
    let frozen_token = "f".repeat(64);
    let frozen_ledger = format!(".soulmate/runs/work-{frozen_token}.jsonl");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-v3.jsonl");
    fs::copy(fixture, frozen.root.join(&frozen_ledger)).unwrap();
    let inspected = frozen.call(&[
        "run",
        "inspect",
        &frozen_ledger,
        "--config",
        "soulmate.json",
    ]);
    assert!(inspected.status.success(), "{}", text(&inspected));
    let inspected_value: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected_value["events"][0]["version"], 3);
    assert_eq!(inspected_value["events"][0]["producer"]["name"], "soulmate");

    let token_v4 = "4".repeat(64);
    let v4 = LegacyFixture::new("transition-accepted-v4");
    let ledger_v4 = format!(".soulmate/runs/work-{token_v4}.jsonl");
    v4.start(&ledger_v4, true, None);
    v4.submit("lead", &ledger_v4, "scoped", "v4-scope");
    let worker_v4 = v4.submit("worker", &ledger_v4, "completed", "v4-worker");
    let target_v4 = worker_v4["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    v4.check(&ledger_v4, &target_v4, 0);
    v4.submit("reviewer", &ledger_v4, "approved", "v4-review");
    v4.submit("lead", &ledger_v4, "accepted", "v4-accept");
    assert_accepted_historical(&v4, &token_v4, &ledger_v4);

    let unchecked = Fixture::new("transition-accepted-unchecked-v5");
    let token_v5 = "5".repeat(64);
    let ledger_v5 = format!(".exitbind/runs/work-{token_v5}.jsonl");
    unchecked.start_unchecked(&ledger_v5);
    unchecked.submit_ok("lead", &ledger_v5, "scoped", "v5-scope");
    unchecked.submit_ok("worker", &ledger_v5, "completed", "v5-worker");
    unchecked.submit_ok("reviewer", &ledger_v5, "approved", "v5-review");
    unchecked.submit_ok("lead", &ledger_v5, "accepted", "v5-accept");
    let unchecked_next = unchecked.json(&["run", "next", &ledger_v5, "--config", "exitbind.json"]);
    assert_eq!(unchecked_next["status"], "accepted");
    assert_eq!(unchecked_next["progress"]["applicable"], false);
    assert!(unchecked_next["progress"]["percent"].is_null());
    assert_eq!(unchecked_next["progress"]["state"], "NOT_APPLICABLE");
    assert_eq!(
        unchecked_next["progress"]["reason"]["code"],
        "unchecked_run"
    );
    let unchecked_facade = unchecked.work_next(&token_v5);
    assert_eq!(unchecked_facade["next"]["action"], "done");
    assert_eq!(unchecked_facade["next"]["status"], "accepted");
    assert_eq!(
        unchecked_facade["next"]["progress"]["state"],
        "NOT_APPLICABLE"
    );
    assert_eq!(unchecked_facade["next"]["progress"]["applicable"], false);
    assert!(unchecked_facade["next"]["progress"]["percent"].is_null());
    assert_eq!(
        unchecked_facade["next"]["progress"]["reason"]["code"],
        "unchecked_run"
    );
    let unchecked_receipt = unchecked.call(&["receipt", &ledger_v5, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&unchecked_receipt, &unchecked_next["progress"]);

    let checked = Fixture::new("transition-accepted-checked-v5-control");
    let receipt = checked.accepted_single(".exitbind/runs/checked.jsonl");
    let checked_next = checked.json(&[
        "run",
        "next",
        ".exitbind/runs/checked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(checked_next["progress"]["applicable"], true);
    assert_eq!(checked_next["progress"]["state"], "READY");
    assert_eq!(checked_next["progress"]["percent"], 100);
    let checked_receipt = checked.call(&[
        "receipt",
        ".exitbind/runs/checked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert!(
        checked_receipt.status.success(),
        "{}",
        text(&checked_receipt)
    );
    let checked_receipt_value: Value = serde_json::from_slice(&checked_receipt.stdout).unwrap();
    assert_eq!(
        checked_receipt_value["outcome"],
        checked_next["progress"]["state"]
    );
    assert_eq!(
        checked_receipt_value["reason"]["code"],
        checked_next["progress"]["reason"]["code"]
    );
    assert_receipt_valid(&checked, &receipt);

    let (checked_work, mut checked_action) = checked.work_begin("work checked control");
    checked_action =
        checked.work_return_ok(&checked_work, &checked_action, "scoped", "scope")["next"].clone();
    let _ = checked.work_return_ok(&checked_work, &checked_action, "completed", "worker");
    fs::write(checked.root.join("actual-product-check"), b"pass").unwrap();
    let checked_work_check = checked.work_check(&checked_work);
    assert!(
        checked_work_check.status.success(),
        "{}",
        text(&checked_work_check)
    );
    checked_action =
        serde_json::from_slice::<Value>(&checked_work_check.stdout).unwrap()["next"].clone();
    checked_action = checked.work_return_ok(&checked_work, &checked_action, "approved", "review")
        ["next"]
        .clone();
    let checked_work_accepted =
        checked.work_return_ok(&checked_work, &checked_action, "accepted", "acceptance");
    assert_eq!(checked_work_accepted["next"]["progress"]["state"], "READY");
    let accepted_next = checked.work_next(&checked_work[4..]);
    assert_eq!(accepted_next["next"]["progress"]["applicable"], true);
    assert_eq!(accepted_next["next"]["progress"]["percent"], 100);
}

fn without(value: &Value, key: &str) -> Value {
    let mut copy = value.clone();
    copy.as_object_mut().unwrap().remove(key);
    copy
}
