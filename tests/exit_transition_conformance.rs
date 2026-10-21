#![cfg(unix)]

mod support;

use serde_json::{json, Value};
#[cfg(any())]
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

const CHECK: &str = "test -f actual-product-check";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str]) -> Output {
        support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
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
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(body.as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
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

#[cfg(any())]
struct LegacyFixture {
    root: PathBuf,
}

#[cfg(any())]
impl LegacyFixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str]) -> Output {
        support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
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

#[cfg(any())]
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

#[cfg(any())]
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

#[cfg(any())]
fn value_hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

fn assert_receipt_matches_progress(output: &Output, progress: &Value) {
    assert_exit_1(output);
    let receipt: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid receipt projection: {error}; {}", text(output)));
    assert_eq!(receipt["outcome"], progress["state"]);
    assert_eq!(receipt["reason"]["code"], progress["reason"]["code"]);
}

#[cfg(any())]
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

#[cfg(any())]
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

#[cfg(any())]
fn without(value: &Value, key: &str) -> Value {
    let mut copy = value.clone();
    copy.as_object_mut().unwrap().remove(key);
    copy
}

#[path = "exit_transition_conformance/historical_compatibility.rs"]
mod historical_compatibility;
#[path = "exit_transition_conformance/stale_check_recovery.rs"]
mod stale_check_recovery;
#[path = "exit_transition_conformance/workflow_receipts.rs"]
mod workflow_receipts;
