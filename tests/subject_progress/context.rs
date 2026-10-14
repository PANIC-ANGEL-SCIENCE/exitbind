#[path = "../support/opaque_work_surface.rs"]
mod opaque_work_surface;
#[path = "../support/mod.rs"]
mod support;

use opaque_work_surface::{assert_opaque_envelope, assert_opaque_reference};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::new_with_workers(true)
    }

    fn new_single() -> Self {
        Self::new_with_workers(false)
    }

    fn new_with_workers(two_workers: bool) -> Self {
        let root = support::temp("subject-progress");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config_path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        if two_workers {
            let base = config["agents"]["worker"].clone();
            let mut second = base;
            second["profile"] = serde_json::json!("exitbind/agents/worker_two.md");
            second["purpose"] = serde_json::json!("Complete bounded work for worker_two.");
            config["agents"]["worker_two"] = second;
            fs::write(
                root.join("exitbind/agents/worker_two.md"),
                b"# worker_two\n\nComplete bounded work.\n",
            )
            .unwrap();
            config["workflows"]["change"]["workers"] = serde_json::json!(["worker", "worker_two"]);
        }
        fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        Self { root }
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&self.root).args(args);
        if args.first() == Some(&"work") && matches!(args.get(1), Some(&"next" | &"resume")) {
            command.arg("--full");
        }
        command
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            return child.wait_with_output().unwrap();
        }
        command.output().unwrap()
    }

    fn value(&self, args: &[&str], input: Option<&[u8]>) -> Value {
        let output = self.call(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn recorded_check_failure(&self, work: &str) -> Value {
        let output = self.call(&["work", "check", work], None);
        assert!(
            !output.status.success(),
            "failed check unexpectedly succeeded"
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["effect"], "recorded", "{value}");
        assert!(
            value["result"]["signal"].is_number()
                || value["result"]["code"]
                    .as_i64()
                    .is_some_and(|code| code != 0),
            "recorded check did not report failure: {value}"
        );
        assert_eq!(value["next"]["action"], "lead_decision", "{value}");
        assert_eq!(value["next"]["role"], "lead", "{value}");
        value
    }

    /// Check-execution counters live outside the product root so recording a
    /// count never changes the tested inputs being counted.
    fn counter(&self, name: &str) -> std::path::PathBuf {
        let directory = self.root.with_extension("counters");
        fs::create_dir_all(&directory).unwrap();
        directory.join(name)
    }

    fn counting_command(&self, name: &str) -> String {
        let path = self.counter(name);
        let path = path.to_str().unwrap();
        format!(
            "n=$(cat '{path}' 2>/dev/null || echo 0); n=$((n+1)); printf '%s\\n' \"$n\" > '{path}'"
        )
    }

    fn count(&self, name: &str) -> String {
        fs::read_to_string(self.counter(name)).unwrap()
    }

    fn ledger(&self, work: &str) -> String {
        format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        )
    }

    fn pin_work(&self, actual: &str, fixed: &str) -> String {
        fs::rename(
            self.root.join(self.ledger(actual)),
            self.root.join(self.ledger(fixed)),
        )
        .unwrap();
        fixed.to_owned()
    }

    fn submit_low_level(&self, agent: &str, ledger: &str, outcome: &str, artifact: &str) -> Output {
        self.call(
            &[
                "run",
                "submit",
                agent,
                ledger,
                "--outcome",
                outcome,
                "--artifact",
                artifact,
                "--artifact-root",
                "state",
                "--json",
            ],
            None,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
        let _ = fs::remove_dir_all(self.root.with_extension("counters"));
    }
}

fn complete_current_worker(fixture: &Fixture, work: &str) -> String {
    let current = fixture.value(&["work", "next", work], None);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            work,
            current["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"same-goal carry scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"same-goal carry worker result"),
    );
    let ledger = fixture.ledger(work);
    fs::read_to_string(fixture.root.join(ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn assert_progress(action: &Value) {
    if action["requiresExpansion"] == true {
        assert!(action["progress"]["state"].is_string());
        assert!(action["progress"]["reason"]["code"].is_string());
        return;
    }
    assert_eq!(action["progress"]["applicable"], true);
    assert!(
        action["progress"]["percent"].is_number(),
        "missing progress: {action}"
    );
    assert!(action["progress"]["weights"]["worker"].is_number());
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&map[key])
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => serde_json::to_string(other).unwrap(),
    }
}

fn rehash(mut event: Value) -> Value {
    event.as_object_mut().unwrap().remove("eventSha256");
    let digest = Sha256::digest(canonical(&event).as_bytes());
    event["eventSha256"] = Value::String(format!("{digest:x}"));
    event
}

/// Build governed work up to the final lead decision with a passing functional
/// check, a passing preservation check (whose checker lives in the product
/// root), and a current review.
fn prepared_for_acceptance(fixture: &Fixture) -> String {
    fs::create_dir_all(fixture.root.join("checks")).unwrap();
    fs::create_dir_all(fixture.root.join("src")).unwrap();
    fs::write(
        fixture.root.join("checks/preserve.sh"),
        b"grep -q env-first src/config.txt\n",
    )
    .unwrap();
    fs::write(fixture.root.join("src/config.txt"), b"env-first\n").unwrap();
    let functional = fixture.counting_command("functional.count");
    let preservation = format!(
        "sh checks/preserve.sh && {}",
        fixture.counting_command("preservation.count")
    );
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "simplify configuration",
            "--check-command",
            &functional,
            "--preserve-requirement",
            "precedence:environment wins over file",
            "--preservation-check-command",
            &preservation,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    drive_to_lead_decision(fixture, &work);
    work
}

/// Follow the façade until a lead decision is pending: return scripted
/// worker/reviewer results and run pending checks. Scripted review requests are
/// counted separately; they are not model reviews.
fn drive_to_lead_decision(fixture: &Fixture, work: &str) -> usize {
    let mut review_requests = 0;
    loop {
        let response = fixture.value(&["work", "next", work], None);
        assert_one_state_basis(fixture, work, &response);
        let next = response["next"].clone();
        match next["action"].as_str().unwrap() {
            "check" => {
                fixture.value(&["work", "check", work], None);
            }
            "spawn" => {
                let role = next["role"].as_str().unwrap();
                let outcome = if role == "reviewer" {
                    "approved"
                } else {
                    "completed"
                };
                if role == "reviewer" {
                    review_requests += 1;
                }
                fixture.value(
                    &[
                        "work",
                        "return",
                        work,
                        next["assignment"].as_str().unwrap(),
                        "--outcome",
                        outcome,
                    ],
                    Some(b"scripted result"),
                );
            }
            "lead_decision" if next["outcomes"][0] == "scoped" => {
                fixture.value(
                    &[
                        "work",
                        "return",
                        work,
                        next["assignment"].as_str().unwrap(),
                        "--outcome",
                        "scoped",
                    ],
                    Some(b"scope"),
                );
            }
            "lead_decision" => return review_requests,
            other => panic!("unexpected action {other}"),
        }
    }
}

struct Consumed {
    result: String,
    reason: String,
    executed: Vec<Vec<String>>,
}

/// A tiny deterministic consumer: it reads the saved packet, asks the public
/// validator, and follows only the validator's bounded Exitbind action. It
/// knows nothing about the scenario being tested.
fn consume(fixture: &Fixture, work: &str, packet: &std::path::Path) -> Consumed {
    let verdict = fixture.value(
        &[
            "work",
            "validate",
            work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    let mut executed = Vec::new();
    let mut help = verdict["humanHelp"].clone();
    while help["nextAction"]["actor"] == "exitbind" {
        let args: Vec<String> = help["nextAction"]["command"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(help["nextAction"]["command"]["program"], "exitbind");
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        fixture.value(&borrowed, None);
        executed.push(args);
        let fresh = fixture.value(&["work", "next", work], None);
        help = fresh["residual"]["humanHelp"].clone();
    }
    Consumed {
        result: verdict["result"].as_str().unwrap().to_owned(),
        reason: verdict["reason"].as_str().unwrap().to_owned(),
        executed,
    }
}

fn save_packet(fixture: &Fixture, work: &str, name: &str) -> std::path::PathBuf {
    let packet = fixture.value(&["work", "next", work], None)["residual"].clone();
    let path = fixture.counter(name);
    fs::write(&path, serde_json::to_vec(&packet).unwrap()).unwrap();
    path
}

/// Every `work next` response must describe one revision: its next action and
/// its residual packet agree, and the packet names the ledger head on disk.
fn assert_one_state_basis(fixture: &Fixture, work: &str, response: &Value) {
    assert_eq!(response["next"]["action"], response["residual"]["next"]);
    let ledger = fs::read_to_string(fixture.root.join(fixture.ledger(work))).unwrap();
    let head: Value = serde_json::from_str(ledger.lines().last().unwrap()).unwrap();
    assert_eq!(
        response["residual"]["snapshot"]["headEventSha256"],
        head["eventSha256"]
    );
    assert_eq!(
        response["residual"]["snapshot"]["eventCount"],
        ledger.lines().count()
    );
}
