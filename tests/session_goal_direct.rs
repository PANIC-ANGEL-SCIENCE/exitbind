//! Direct-only session-goal closure stays separate from governed run evidence.
#![cfg(unix)]

mod support;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    process::{Command, Output, Stdio},
};

const CARD: &str =
    "+----------------------------+\n| This room remains nothing. |\n+----------------------------+";

struct Project {
    root: std::path::PathBuf,
    session_store: std::path::PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let mut init_command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        init_command
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root);
        for name in [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
        ] {
            init_command.env_remove(name);
        }
        let init = init_command.output().unwrap();
        assert!(init.status.success(), "{init:?}");
        let session_store = root.join(".exitbind/native-test-session-goals");
        Self {
            root,
            session_store,
        }
    }

    fn call(&self, args: &[&str]) -> Output {
        self.command(None, None, None, args)
    }

    fn call_in_session(
        &self,
        host: &str,
        identity: &str,
        store: &std::path::Path,
        args: &[&str],
    ) -> Output {
        self.command(Some(host), Some(identity), Some(store), args)
    }

    fn command(
        &self,
        host: Option<&str>,
        identity: Option<&str>,
        store: Option<&std::path::Path>,
        args: &[&str],
    ) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdin(Stdio::null());
        for name in [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
        ] {
            command.env_remove(name);
        }
        if let (Some(host), Some(identity), Some(store)) = (host, identity, store) {
            command.env("EXITBIND_SESSION_GOALS_DIR", store);
            match host {
                "codex" => {
                    command.env("CODEX_THREAD_ID", identity);
                }
                "claude" => {
                    command.env("CLAUDE_CODE_SESSION_ID", identity);
                }
                _ => panic!("test requested unsupported native host"),
            }
        }
        command.output().unwrap()
    }

    fn call_with_native_selectors(&self, selectors: &[(&str, &str)], args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdin(Stdio::null());
        for name in [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
        ] {
            command.env_remove(name);
        }
        command.env("EXITBIND_SESSION_GOALS_DIR", &self.session_store);
        for (name, value) in selectors {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn value(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn value_in_session(
        &self,
        host: &str,
        identity: &str,
        store: &std::path::Path,
        args: &[&str],
    ) -> Value {
        let output = self.call_in_session(host, identity, store, args);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn pty_status_in_session(&self, host: &str, identity: &str, store: &std::path::Path) -> String {
        let selector = match host {
            "codex" => format!("CODEX_THREAD_ID={}", shell_quote(identity)),
            "claude" => format!("CLAUDE_CODE_SESSION_ID={}", shell_quote(identity)),
            _ => panic!("test requested unsupported native host"),
        };
        let command = format!(
            "env -u CODEX_THREAD_ID -u CODEX_SESSION_ID -u CLAUDE_CODE_SESSION_ID -u EXITBIND_NATIVE_SESSION_ID -u EXITBIND_SESSION_GOALS_DIR {selector} EXITBIND_SESSION_GOALS_DIR={} {} goal status --themed --config {}",
            shell_quote(&store.display().to_string()),
            shell_quote(env!("CARGO_BIN_EXE_exitbind")),
            shell_quote(&self.root.join("exitbind.json").display().to_string())
        );
        let output = support::pty(&command)
            .current_dir(&self.root)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).replace('\r', "")
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn establish_direct_goal(project: &Project) {
    fs::write(project.root.join("source.txt"), b"direct-input\n").unwrap();
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--none-applicable",
        "obligations,findings,blockers,decisions,externalActions",
    ]);
}

fn establish_native_direct_goal(project: &Project, session: &str) {
    project.value_in_session(
        "codex",
        session,
        &project.session_store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "direct-event",
            "--goal",
            "Finish the whole conversation request",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
}

fn native_goal_journal(project: &Project, session: &str) -> std::path::PathBuf {
    native_session_root(&project.session_store, "codex", session)
        .join(".exitbind/session-goal.jsonl")
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
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn digest_value(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

fn digest_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn native_session_root(store: &std::path::Path, host: &str, identity: &str) -> std::path::PathBuf {
    let key = digest_value(&json!({"host":host,"identity":identity}));
    store.join(format!("session-{key}"))
}

fn start_intent_sha256(start: &Value) -> String {
    const FIELDS: &[&str] = &[
        "version",
        "kind",
        "producer",
        "action",
        "workflow",
        "goal",
        "configSha256",
        "plan",
        "harnessReceipt",
        "checkPolicy",
        "preservation",
        "basisProtocol",
        "recoveryProtocol",
        "checkObservationProtocol",
        "basis",
        "reviewPolicy",
        "governor",
    ];
    let mut projection = Map::new();
    for field in FIELDS {
        if let Some(value) = start.get(*field) {
            projection.insert((*field).to_owned(), value.clone());
        }
    }
    digest_value(&Value::Object(projection))
}

fn append_goal_admission(session_root: &std::path::Path, admission: Value) {
    let path = session_root.join(".exitbind/session-goal.jsonl");
    let raw = fs::read_to_string(&path).unwrap();
    let mut records = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let previous = records.last().unwrap().clone();
    let mut next = previous.clone();
    let revision = previous["revision"].as_u64().unwrap() + 1;
    next["revision"] = json!(revision);
    next["predecessor"] = json!({
        "goalId": previous["goalId"],
        "revision": previous["revision"],
    });
    next["successorOf"] = Value::Null;
    next["previousEventSha256"] = previous["eventSha256"].clone();
    next["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
    next["journeyAdmissions"]
        .as_array_mut()
        .expect("v2 admission list")
        .push(admission);
    next.as_object_mut().unwrap().remove("eventSha256");
    next["eventSha256"] = json!(digest_value(&next));
    records.push(next);
    let output = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{output}\n")).unwrap();
}

fn admission_for_start(member: &Value, work: &str, start: &Value, parent_goal: &Value) -> Value {
    json!({
        "work": work,
        "startIntentSha256": start_intent_sha256(start),
        "parentGoalId": parent_goal["goalId"],
        "parentObservedGoalEventSha256": parent_goal["eventSha256"],
        "workGoalSha256": digest_text(start["goal"].as_str().unwrap()),
        "host": member["host"],
        "sessionKey": member["sessionKey"],
        "ownerIdentitySha256": member["ownerIdentitySha256"],
        "ownerProjectId": member["ownerProjectId"],
        "projectIdentitySha256": member["projectIdentitySha256"],
        "localIdentitySha256": member["localIdentitySha256"],
        "configPath": member["configPath"],
        "configSha256": member["configSha256"],
        "productRoot": member["productRoot"],
        "stateRoot": member["stateRoot"],
    })
}

#[test]
fn goal_status_automatically_emits_goal_and_task_progress_without_model_formatting() {
    let project = Project::new("goal-progress-product-surface");
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "progress",
        "--goal",
        "Ship the user's external request",
        "--obligation",
        "Implement the requested behavior",
    ]);
    let status = project.value(&["goal", "status", "--json"]);
    assert_eq!(status["goalProgress"]["overall"], "in_progress");
    assert_eq!(status["goalProgress"]["decomposition"]["total"], 1);
    assert_eq!(status["goalProgress"]["decomposition"]["complete"], 0);
    assert_eq!(
        status["goalProgress"]["resultReadiness"]["state"],
        "unavailable"
    );
    let text = status["goalProgress"]["systemText"].as_str().unwrap();
    assert!(text.contains("Goal: IN PROGRESS"), "{text}");
    assert!(text.contains("Tasks: 0/1 complete"), "{text}");
    assert!(text.contains("Result readiness: unavailable"), "{text}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn standalone_work_keeps_an_unrelated_canonical_goal_private() {
    let project = Project::new("goal-progress-work-binding");
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "unrelated-goal",
        "--goal",
        "Private external goal",
        "--obligation",
        "Private task",
    ]);
    let started = project.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "Different Work goal",
        "--check-command",
        "true",
        "--standalone",
    ]);
    assert!(started["next"]["packet"].get("goalProgress").is_none());
    assert!(!started.to_string().contains("Private external goal"));
    assert!(!started.to_string().contains("Private task"));
    let progress = &started["next"]["goalProgress"];
    assert_eq!(progress["overall"], "unavailable");
    assert_eq!(progress["goal"], Value::Null);
    assert_eq!(progress["decomposition"]["available"], false);
    assert_eq!(
        progress["decomposition"]["reason"],
        "overall_goal_unavailable"
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn ordinary_work_gets_context_for_the_open_goal_without_completing_its_task() {
    let project = Project::new("goal-context-work-association");
    let session = "goal-context-work-association-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "session",
            "--goal",
            "Ship the user's broader session request",
            "--obligation",
            "Implement the shared behavior",
        ],
    );
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A different but related implementation task",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    let association = &started["next"]["goalProgress"]["workAssociation"];
    assert_eq!(association["kind"], "context_only");
    assert_eq!(association["protocol"], 1);
    assert_eq!(association["work"], work);
    assert_eq!(association["startEventSha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        started["next"]["goalProgress"]["goal"],
        "Ship the user's broader session request"
    );
    assert_eq!(
        started["next"]["goalProgress"]["decomposition"]["complete"],
        0
    );
    assert_eq!(
        started["next"]["goalProgress"]["tasks"][0]["disposition"],
        "open"
    );
    let status = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(status["goalProgress"]["decomposition"]["complete"], 0);

    let standalone = project.call_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Must not inherit context",
            "--check-command",
            "true",
            "--standalone",
        ],
    );
    assert!(standalone.status.success());
    let standalone: Value = serde_json::from_slice(&standalone.stdout).unwrap();
    assert_eq!(standalone["next"]["goalProgress"]["overall"], "unavailable");
    assert!(standalone["next"]["goalProgress"]
        .get("workAssociation")
        .is_none());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_late_side_work_reopens_a_closed_session_goal_and_suppresses_room_readiness() {
    let project = Project::new("goal-context-late-side-work");
    let session = "goal-context-late-side-work-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "journey",
            "--goal",
            "The whole conversation journey",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let closed = project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "close", "--direct", "--goal-id", "journey"],
    );
    assert_eq!(closed["closure"]["closed"], true);
    let before = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(before["closure"]["closed"], true);
    assert_eq!(
        before["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        1
    );

    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A later side request in this conversation",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["work"],
        work
    );
    let after = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(after["closure"]["closed"], false);
    assert_eq!(
        after["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(after["goalProgress"]["journeyCompletion"]["protocol"], 2);
    assert_eq!(
        after["goalProgress"]["journeyCompletion"]["pendingAdmissions"],
        0
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        0
    );

    let session_root = native_session_root(store, "codex", session);
    let history = session_root.join(".exitbind/session-goal.jsonl");
    let records = fs::read_to_string(history)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records
        .iter()
        .any(|record| { record["closure"]["closed"] == true && record["goalId"] == "journey" }));
    assert_eq!(records.last().unwrap()["closure"]["closed"], false);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_prestart_pending_admission_reopens_a_closed_goal_without_a_work_start() {
    let project = Project::new("goal-context-prestart-admission");
    let session = "goal-context-prestart-admission-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "journey",
            "--goal",
            "Every request in this conversation remains accounted for",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let closed = project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "close", "--direct", "--goal-id", "journey"],
    );
    assert_eq!(closed["closure"]["closed"], true);
    let before = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(before["closure"]["closed"], true);
    assert_eq!(
        before["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        1
    );

    let pending_work = format!("smw_{}", "9".repeat(64));
    let admission = json!({
        "work": pending_work,
        "startIntentSha256": "a".repeat(64),
        "parentGoalId": "journey",
        "parentObservedGoalEventSha256": closed["eventSha256"],
        "workGoalSha256": "b".repeat(64),
        "host": "codex",
        "sessionKey": "c".repeat(64),
        "ownerIdentitySha256": "d".repeat(64),
        "ownerProjectId": null,
        "projectIdentitySha256": "e".repeat(64),
        "localIdentitySha256": "f".repeat(64),
        "configPath": project.root.join("exitbind.json").display().to_string(),
        "configSha256": "1".repeat(64),
        "productRoot": project.root.display().to_string(),
        "stateRoot": project.root.join(".exitbind").display().to_string(),
    });
    append_goal_admission(&native_session_root(store, "codex", session), admission);

    let blocked = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(blocked["closure"]["closed"], false);
    assert_eq!(
        blocked["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(
        blocked["goalProgress"]["journeyCompletion"]["pendingAdmissions"],
        1
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        0
    );
    let close = project.call_in_session(
        "codex",
        session,
        store,
        &["goal", "close", "--direct", "--goal-id", "journey"],
    );
    assert!(!close.status.success());
    assert!(String::from_utf8_lossy(&close.stderr)
        .contains("native session journey is not ready to close"));
    assert!(!project
        .root
        .join(format!(".exitbind/runs/work-{}.jsonl", "9".repeat(64)))
        .exists());

    let history = fs::read_to_string(
        native_session_root(store, "codex", session).join(".exitbind/session-goal.jsonl"),
    )
    .unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records
        .iter()
        .any(|record| record["closure"]["closed"] == true));
    assert_eq!(
        records.last().unwrap()["journeyAdmissions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn link_session_recovers_only_the_exact_admitted_start_without_mutating_work() {
    let project = Project::new("goal-context-link-session-recovery");
    let session = "goal-context-link-session-recovery-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "journey",
            "--goal",
            "Link only a start admitted by this conversation",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let seed = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Seed current native membership",
            "--check-command",
            "true",
        ],
    );
    let session_root = native_session_root(store, "codex", session);
    let work = project.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "The interrupted side request",
        "--check-command",
        "true",
    ]);
    let work = work["work"].as_str().unwrap().to_owned();
    let token = work.strip_prefix("smw_").unwrap();
    let ledger = project
        .root
        .join(format!(".exitbind/runs/work-{token}.jsonl"));
    let ledger_before = fs::read(&ledger).unwrap();
    let start =
        serde_json::from_slice::<Value>(ledger_before.split(|byte| *byte == b'\n').next().unwrap())
            .unwrap();

    let history_path = session_root.join(".exitbind/session-goal.jsonl");
    let history = fs::read_to_string(&history_path).unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let parent = records.last().unwrap().clone();
    let seed_work = seed["work"].as_str().unwrap();
    let member = records
        .iter()
        .rev()
        .find_map(|record| {
            record["journeyMembers"]
                .as_array()?
                .iter()
                .find(|member| member["work"] == seed_work)
        })
        .unwrap()
        .clone();
    let admission = admission_for_start(&member, &work, &start, &parent);
    append_goal_admission(&session_root, admission);

    let wrong_session = project.call_in_session(
        "codex",
        "another-conversation",
        store,
        &["work", "link-session", &work, "--json"],
    );
    assert!(!wrong_session.status.success());
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    let pending = fs::read_to_string(&history_path).unwrap();
    let pending_record = pending.lines().last().unwrap();
    let pending_record: Value = serde_json::from_str(pending_record).unwrap();
    assert_eq!(
        pending_record["journeyAdmissions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let recovered = project.value_in_session(
        "codex",
        session,
        store,
        &["work", "link-session", &work, "--json"],
    );
    assert_eq!(recovered["kind"], "native_session_goal_link");
    assert_eq!(recovered["state"], "linked");
    assert_eq!(recovered["workLedger"], "unchanged");
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    let after_first_link = fs::read(&history_path).unwrap();
    let companion_path = project
        .root
        .join(format!(".exitbind/runs/work-{token}.session-goal.json"));
    let companion = fs::read(&companion_path).unwrap();
    let repeated = project.value_in_session(
        "codex",
        session,
        store,
        &["work", "link-session", &work, "--json"],
    );
    assert_eq!(repeated["state"], "linked");
    assert_eq!(fs::read(&history_path).unwrap(), after_first_link);
    assert_eq!(fs::read(&companion_path).unwrap(), companion);
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    let detail = project.value_in_session(
        "codex",
        session,
        store,
        &["work", "detail", &work, "--json"],
    );
    assert_eq!(detail["goalAssociation"]["kind"], "context_only");
    let final_history = fs::read_to_string(&history_path).unwrap();
    let final_record: Value = serde_json::from_str(final_history.lines().last().unwrap()).unwrap();
    assert!(final_record["journeyAdmissions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(final_record["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["work"] == work));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_goal_added_after_work_start_is_not_attached_retroactively() {
    let project = Project::new("goal-context-no-retroactive-link");
    let session = "goal-context-no-retroactive-link-session";
    let store = project.session_store.as_path();
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Start before the Lead has a canonical Goal",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["next"]["goalProgress"]["overall"], "not_requested");
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "later",
            "--goal",
            "A later session request",
            "--obligation",
            "A later task",
        ],
    );
    let detail =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(detail["sections"]["tasks"]["overall"], "unavailable");
    assert!(detail.get("goalAssociation").is_none());
    assert!(!detail.to_string().contains("A later session request"));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn conversation_members_survive_successors_but_work_context_stays_with_its_original_goal() {
    let project = Project::new("goal-context-goal-lifecycle");
    let session = "goal-context-goal-lifecycle-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "original",
            "--goal",
            "Original goal",
            "--obligation",
            "Original task",
        ],
    );
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Context only",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    let original_association = started["next"]["goalProgress"]["workAssociation"].clone();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "original",
            "--goal",
            "Revised original goal",
            "--obligation",
            "Revised original task",
        ],
    );
    let same_goal =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(
        same_goal["sections"]["tasks"]["goal"],
        "Revised original goal"
    );
    assert_eq!(same_goal["goalAssociation"], original_association);

    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--direct",
            "--goal-id",
            "original",
            "--goal",
            "Revised original goal",
            "--finding",
            "Lead recorded a finding",
        ],
    );
    let direct_update =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(direct_update["goalAssociation"], original_association);

    let successor = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "successor",
            "--goal",
            "Successor goal",
            "--obligation",
            "Successor task",
        ],
    );
    assert!(successor.get("workAssociationProtocol").is_none());
    assert!(successor.get("workAssociations").is_none());
    assert_eq!(successor["journeyMembershipProtocol"], 2);
    assert_eq!(successor["journeyMembers"].as_array().unwrap().len(), 1);
    assert_eq!(successor["journeyMembers"][0]["work"], work);
    assert_eq!(successor["journeyMembers"][0]["parentGoalId"], "original");
    let old_work =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(old_work["goalAssociation"], original_association);
    assert_eq!(
        old_work["sections"]["tasks"]["goal"],
        "Revised original goal"
    );
    assert_eq!(old_work["sections"]["tasks"]["overall"], "in_progress");
    assert!(!old_work.to_string().contains("Successor goal"));
    let active_successor =
        project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(active_successor["goalId"], "successor");
    assert_eq!(active_successor["closure"]["closed"], false);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn context_association_does_not_create_named_requirement_coverage() {
    let project = Project::new("goal-context-named-coverage");
    let session = "goal-context-named-coverage-session";
    let store = project.session_store.as_path();
    fs::write(
        project.root.join("request.txt"),
        "The accepted behavior is precise.\n",
    )
    .unwrap();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "named",
            "--goal",
            "Named coverage remains explicit",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "require",
            "--goal-id",
            "named",
            "--requirement",
            "R1",
            "--obligation",
            "The accepted behavior is precise.",
            "--artifact",
            "request.txt",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "cover", "--goal-id", "named"],
    );
    let refused = project.call_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Named requirement contract",
            "--check-command",
            "true",
            "--goal-id",
            "named",
            "--requirement",
            "R1",
            "--artifact",
            "request.txt",
            "--standalone",
        ],
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("cannot be combined"));
    assert_eq!(
        fs::read_dir(project.root.join(".exitbind/runs"))
            .unwrap()
            .count(),
        0
    );
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Unmapped ordinary Work",
            "--check-command",
            "true",
        ],
    );
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["kind"],
        "context_only"
    );
    assert_eq!(started["next"]["goalProgress"]["coverageConfirmed"], true);
    assert_eq!(
        started["next"]["goalProgress"]["decomposition"]["complete"],
        0
    );
    assert_eq!(started["next"]["goalProgress"]["tasks"][0]["mapped"], false);
    assert_eq!(
        started["next"]["goalProgress"]["tasks"][0]["performed"],
        false
    );
    let explicitly_named = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Explicitly cover the named requirement",
            "--check-command",
            "test -f request.txt",
            "--review-policy",
            "required",
            "--goal-id",
            "named",
            "--requirement",
            "R1",
            "--artifact",
            "request.txt",
        ],
    );
    assert_eq!(
        explicitly_named["next"]["goalProgress"]["workAssociation"]["kind"],
        "context_only"
    );
    assert!(explicitly_named["next"]
        .get("projectGoalProgress")
        .is_none());
    assert_eq!(
        explicitly_named["next"]["goalProgress"]["tasks"][0]["mapped"],
        true
    );
    assert_eq!(
        explicitly_named["next"]["goalProgress"]["tasks"][0]["performed"],
        false
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn native_sessions_are_independent_by_identity_and_host_in_one_project() {
    let project = Project::new("goal-native-session-isolation");
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        "same-native-id",
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "codex-goal",
            "--goal",
            "Codex conversation request",
            "--obligation",
            "Codex conversation task",
        ],
    );
    project.value_in_session(
        "claude",
        "same-native-id",
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "claude-goal",
            "--goal",
            "Claude conversation request",
            "--obligation",
            "Claude conversation task",
        ],
    );
    project.value_in_session(
        "codex",
        "other-native-id",
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "other-codex-goal",
            "--goal",
            "Other Codex request",
            "--obligation",
            "Other Codex task",
        ],
    );

    let codex = project.value_in_session(
        "codex",
        "same-native-id",
        store,
        &["goal", "status", "--json"],
    );
    let claude = project.value_in_session(
        "claude",
        "same-native-id",
        store,
        &["goal", "status", "--json"],
    );
    let other = project.value_in_session(
        "codex",
        "other-native-id",
        store,
        &["goal", "status", "--json"],
    );
    assert_eq!(codex["goal"], "Codex conversation request");
    assert_eq!(claude["goal"], "Claude conversation request");
    assert_eq!(other["goal"], "Other Codex request");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn another_project_receives_session_goal_context_without_mutation_or_coverage_authority() {
    let owner = Project::new("goal-native-session-owner");
    let child = Project::new("goal-native-session-child");
    let session = "shared-conversation-session";
    let store = owner.session_store.as_path();
    owner.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "parent",
            "--goal",
            "One conversation across projects",
            "--obligation",
            "Owner project task",
        ],
    );

    let started = child.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Separate child-project task",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["next"]["goalProgress"]["overall"], "context_only");
    assert_eq!(
        started["next"]["goalProgress"]["goal"],
        "One conversation across projects"
    );
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["kind"],
        "context_only"
    );

    let child_status =
        child.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(child_status["contextOnly"], true);
    assert_eq!(child_status["ownerRequired"], true);
    let mutation = child.call_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "hijack",
            "--goal",
            "Child mutation attempt",
            "--obligation",
            "Must be refused",
        ],
    );
    assert!(!mutation.status.success());
    assert!(
        String::from_utf8_lossy(&mutation.stderr).contains("owned by another configured project")
    );

    let detail =
        child.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(detail["goalAssociation"]["kind"], "context_only");
    assert_eq!(detail["sections"]["tasks"]["overall"], "context_only");

    // Optional context corruption must not hide the Work's required detail or
    // recovery surface. The malformed locator only removes its Goal preview.
    let token = work.strip_prefix("smw_").unwrap();
    let context_companion = child
        .root
        .join(".exitbind/runs")
        .join(format!("work-{token}.session-goal.json"));
    let mut companion: Value =
        serde_json::from_slice(&fs::read(&context_companion).unwrap()).unwrap();
    companion["protocol"] = serde_json::json!(99);
    fs::write(&context_companion, serde_json::to_vec(&companion).unwrap()).unwrap();
    let after_corruption =
        child.call_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert!(
        after_corruption.status.success(),
        "{}",
        String::from_utf8_lossy(&after_corruption.stderr)
    );
    let after_corruption: Value = serde_json::from_slice(&after_corruption.stdout).unwrap();
    assert_eq!(after_corruption["complete"], true);
    assert_eq!(
        after_corruption["sections"]["tasks"]["overall"],
        "unavailable"
    );
    assert!(after_corruption.get("goalAssociation").is_none());
    fs::remove_dir_all(owner.root).unwrap();
    fs::remove_dir_all(child.root).unwrap();
}

#[test]
fn invalid_or_unavailable_native_identity_never_falls_back_and_does_not_block_ordinary_work() {
    let project = Project::new("goal-native-session-invalid-selector");
    project.value(&[
        "goal",
        "incorporate",
        "--project",
        "--goal-id",
        "local",
        "--goal",
        "Local Goal must not be a native fallback",
        "--obligation",
        "Local-only task",
    ]);
    for selectors in [
        vec![
            ("CODEX_THREAD_ID", "codex-one"),
            ("CLAUDE_CODE_SESSION_ID", "claude-one"),
        ],
        vec![
            ("CODEX_THREAD_ID", "codex-one"),
            ("CODEX_SESSION_ID", "codex-two"),
        ],
    ] {
        let started = project.call_with_native_selectors(
            &selectors,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Still usable ordinary Work",
                "--check-command",
                "true",
            ],
        );
        assert!(
            started.status.success(),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        let started: Value = serde_json::from_slice(&started.stdout).unwrap();
        let work = started["work"].as_str().unwrap();
        assert_eq!(started["next"]["goalProgress"]["overall"], "unavailable");
        assert!(started["next"]["goalProgress"]
            .get("workAssociation")
            .is_none());
        let detail =
            project.call_with_native_selectors(&selectors, &["work", "detail", work, "--json"]);
        assert!(
            detail.status.success(),
            "{}",
            String::from_utf8_lossy(&detail.stderr)
        );
    }
    let session_without_goal = "valid-but-not-yet-incorporated";
    let started = project.value_in_session(
        "codex",
        session_without_goal,
        &project.session_store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "No implicit project fallback",
            "--check-command",
            "true",
        ],
    );
    assert_eq!(started["next"]["goalProgress"]["overall"], "not_requested");
    assert!(started["next"]["goalProgress"]
        .get("workAssociation")
        .is_none());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn project_named_contract_can_coexist_with_native_parent_context() {
    let project = Project::new("goal-project-contract-with-parent-context");
    let session = "project-contract-parent-session";
    let store = project.session_store.as_path();
    fs::write(
        project.root.join("request.txt"),
        "Project obligation text.\n",
    )
    .unwrap();
    fs::write(
        project.root.join("check.sh"),
        "#!/bin/sh\ntest -f request.txt\n",
    )
    .unwrap();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "same",
            "--goal",
            "Session parent request",
            "--obligation",
            "Session parent task",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            "same",
            "--goal",
            "Project-local requirement contract",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "require",
            "--project",
            "--goal-id",
            "same",
            "--requirement",
            "P1",
            "--obligation",
            "Project obligation text.",
            "--artifact",
            "request.txt",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "cover", "--project", "--goal-id", "same"],
    );
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Project-local named Work",
            "--check-command",
            "sh check.sh",
            "--review-policy",
            "required",
            "--goal-id",
            "same",
            "--requirement",
            "P1",
            "--artifact",
            "check.sh",
            "--project-goal",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(
        started["next"]["goalProgress"]["goal"],
        "Session parent request"
    );
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["kind"],
        "context_only"
    );
    assert_eq!(started["next"]["projectGoalProgress"]["scope"], "project");
    assert_eq!(started["next"]["projectGoalProgress"]["goalId"], "same");
    assert_eq!(
        started["next"]["projectGoalProgress"]["requirements"][0]["mapped"],
        true
    );
    assert_eq!(
        started["next"]["projectGoalProgress"]["requirements"][0]["state"],
        "pending"
    );
    assert_eq!(
        started["next"]["projectGoalProgress"]["requirements"][0]["support"]["state"],
        "pending"
    );
    let detail =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(detail["goalAssociation"]["kind"], "context_only");
    assert_eq!(detail["projectGoalProgress"]["scope"], "project");

    // A malformed named-scope selector is not silently reinterpreted as a
    // contract in another Goal journal, but remains optional for Work detail.
    let token = work.strip_prefix("smw_").unwrap();
    let scope_companion = project
        .root
        .join(".exitbind/runs")
        .join(format!("work-{token}.named-goal-scope.json"));
    let mut companion: Value =
        serde_json::from_slice(&fs::read(&scope_companion).unwrap()).unwrap();
    companion["protocol"] = serde_json::json!(99);
    fs::write(&scope_companion, serde_json::to_vec(&companion).unwrap()).unwrap();
    let after_scope_corruption =
        project.call_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert!(
        after_scope_corruption.status.success(),
        "{}",
        String::from_utf8_lossy(&after_scope_corruption.stderr)
    );
    let after_scope_corruption: Value =
        serde_json::from_slice(&after_scope_corruption.stdout).unwrap();
    assert_eq!(
        after_scope_corruption["goalAssociation"]["kind"],
        "context_only"
    );
    assert_eq!(
        after_scope_corruption["projectGoalProgress"]["state"],
        "unavailable"
    );
    let assignment = project.call_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "assign",
            "--project",
            "--goal-id",
            "same",
            "--requirement",
            "P1",
            "--result-ref",
            work,
        ],
    );
    assert!(!assignment.status.success());
    assert!(String::from_utf8_lossy(&assignment.stderr).contains("named Goal scope companion"));

    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            "local-direct",
            "--goal",
            "Local direct completion",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "close",
            "--project",
            "--direct",
            "--goal-id",
            "local-direct",
        ],
    );
    let parent = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(parent["goalId"], "same");
    assert_eq!(parent["closure"]["closed"], false);
    fs::remove_dir_all(project.root).unwrap();
}

fn pty_status(project: &Project) -> String {
    let command = format!(
        "env -u CODEX_THREAD_ID -u CODEX_SESSION_ID -u CLAUDE_CODE_SESSION_ID -u EXITBIND_NATIVE_SESSION_ID -u EXITBIND_SESSION_GOALS_DIR {} goal status --themed --config {}",
        env!("CARGO_BIN_EXE_exitbind"),
        project.root.join("exitbind.json").display()
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).replace('\r', "")
}

fn assert_no_run_files(project: &Project) {
    let runs = project.root.join(".exitbind/runs");
    let count = fs::read_dir(runs).unwrap().count();
    assert_eq!(count, 0, "direct goal created governed run files");
}

#[test]
fn direct_goal_closes_and_emits_exact_card_without_a_run() {
    let project = Project::new("direct-goal-card");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_no_run_files(&project);

    let plain_command = format!(
        "{} goal status --config {}",
        env!("CARGO_BIN_EXE_exitbind"),
        project.root.join("exitbind.json").display()
    );
    let plain = support::pty(&plain_command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&plain.stdout).contains(CARD));
    assert_eq!(
        project.value(&["goal", "status", "--json"])["currentReadiness"]["state"],
        "current"
    );

    let first = pty_status(&project);
    assert_eq!(first.matches(CARD).count(), 1, "{first}");
    assert_eq!(first.lines().last(), Some("+----------------------------+"));
    let second = pty_status(&project);
    assert_eq!(second.matches(CARD).count(), 0, "{second}");
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn direct_completion_event_is_immediate_idempotent_and_recoverable_once() {
    let project = Project::new("direct-goal-completion-event");
    let session = "direct-goal-completion-event-session";
    establish_native_direct_goal(&project, session);
    let journal = native_goal_journal(&project, session);

    let first_output = project.call_in_session(
        "codex",
        session,
        &project.session_store,
        &[
            "goal",
            "close",
            "--direct",
            "--goal-id",
            "direct-event",
            "--view",
            "event",
            "--json",
        ],
    );
    assert!(first_output.status.success(), "{first_output:?}");
    assert!(
        first_output.stdout.len() <= 4096,
        "{} bytes",
        first_output.stdout.len()
    );
    let first: Value = serde_json::from_slice(&first_output.stdout).unwrap();
    assert_eq!(first["kind"], "goal_completion_response");
    assert_eq!(first["effect"], "committed");
    assert_eq!(first["completionEvent"]["kind"], "session_goal_completed");
    assert_eq!(first["completionEvent"]["mode"], "direct");
    assert_eq!(
        first["presentation"]["terminal"],
        "This room remains nothing."
    );
    assert!(!first["presentation"]["terminal"]
        .as_str()
        .unwrap()
        .contains("EXIT READY"));
    let event_id = first["completionEvent"]["id"].as_str().unwrap();
    let committed_journal = fs::read(&journal).unwrap();

    let duplicate = project.value_in_session(
        "codex",
        session,
        &project.session_store,
        &[
            "goal",
            "close",
            "--direct",
            "--goal-id",
            "direct-event",
            "--view",
            "event",
            "--json",
        ],
    );
    assert_eq!(duplicate["effect"], "no-change");
    assert_eq!(duplicate["completionEvent"]["id"], event_id);
    assert_eq!(duplicate["presentation"]["terminal"], Value::Null);
    assert_eq!(fs::read(&journal).unwrap(), committed_journal);

    let recovered = project.value_in_session(
        "codex",
        session,
        &project.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(recovered["completionEvent"]["id"], event_id);
    assert_eq!(recovered["recovery"]["readOnly"], true);
    assert_eq!(
        recovered["presentation"]["terminal"],
        "This room remains nothing."
    );
    assert_eq!(fs::read(&journal).unwrap(), committed_journal);
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn completion_event_is_suppressed_for_unfinished_stale_and_foreign_goals() {
    let unfinished = Project::new("direct-goal-event-unfinished");
    let unfinished_session = "direct-goal-event-unfinished-session";
    unfinished.value_in_session(
        "codex",
        unfinished_session,
        &unfinished.session_store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "unfinished",
            "--goal",
            "Leave one agreed obligation open",
            "--obligation",
            "Finish the remaining task",
        ],
    );
    let unfinished_journal = native_goal_journal(&unfinished, unfinished_session);
    let before_unfinished = fs::read(&unfinished_journal).unwrap();
    let unfinished_status = unfinished.value_in_session(
        "codex",
        unfinished_session,
        &unfinished.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(unfinished_status["completionEvent"], Value::Null);
    assert_eq!(unfinished_status["presentation"]["terminal"], Value::Null);
    assert_eq!(fs::read(&unfinished_journal).unwrap(), before_unfinished);
    fs::remove_dir_all(unfinished.root).unwrap();

    let stale = Project::new("direct-goal-event-stale");
    let stale_session = "direct-goal-event-stale-session";
    fs::write(stale.root.join("source.txt"), b"before-close\n").unwrap();
    establish_native_direct_goal(&stale, stale_session);
    let stale_journal = native_goal_journal(&stale, stale_session);
    stale.value_in_session(
        "codex",
        stale_session,
        &stale.session_store,
        &["goal", "close", "--direct", "--goal-id", "direct-event"],
    );
    fs::write(stale.root.join("source.txt"), b"changed-after-close\n").unwrap();
    let before_stale_read = fs::read(&stale_journal).unwrap();
    let stale_status = stale.value_in_session(
        "codex",
        stale_session,
        &stale.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(stale_status["completionEvent"], Value::Null);
    assert_eq!(stale_status["presentation"]["terminal"], Value::Null);
    assert_eq!(fs::read(&stale_journal).unwrap(), before_stale_read);

    let foreign = Project::new("direct-goal-event-foreign");
    let before_foreign_read = fs::read(&stale_journal).unwrap();
    let foreign_status = foreign.value_in_session(
        "codex",
        stale_session,
        &stale.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(foreign_status["contextOnly"], true);
    assert!(foreign_status.get("completionEvent").is_none());
    assert!(foreign_status.get("presentation").is_none());
    assert_eq!(fs::read(&stale_journal).unwrap(), before_foreign_read);
    assert_no_run_files(&stale);
    fs::remove_dir_all(stale.root).unwrap();
    fs::remove_dir_all(foreign.root).unwrap();
}

#[test]
fn direct_item_completion_is_typed_and_unknown_or_open_facts_refuse_close() {
    let project = Project::new("direct-goal-item");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--direct",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "direct finding",
    ]);
    let closed = project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_eq!(closed["closure"]["kind"], "direct");
    assert_eq!(closed["closure"]["resultRefs"].as_array().unwrap().len(), 0);
    fs::remove_dir_all(project.root).unwrap();

    let open = Project::new("direct-goal-open");
    open.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "open",
        "--goal",
        "open goal",
        "--finding",
        "open finding",
    ]);
    let refused = open.call(&["goal", "close", "--direct", "--goal-id", "open"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("unresolved"));
    fs::remove_dir_all(open.root).unwrap();

    let unknown = Project::new("direct-goal-unknown");
    unknown.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "unknown",
        "--goal",
        "unknown goal",
    ]);
    let refused = unknown.call(&["goal", "close", "--direct", "--goal-id", "unknown"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("unresolved"));
    fs::remove_dir_all(unknown.root).unwrap();
}

#[test]
fn mixed_direct_closure_cannot_bypass_missing_governed_evidence() {
    let project = Project::new("direct-goal-mixed");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "governed finding",
        "--disposition",
        "accepted",
        "--result-ref",
        "smw_missing",
    ]);
    let refused = project.call(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not a governed result"));
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn direct_close_keeps_history_but_suppresses_current_room_after_input_drift() {
    let project = Project::new("direct-goal-drift");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--direct",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "direct finding",
    ]);
    fs::write(project.root.join("source.txt"), b"drifted-input\n").unwrap();
    let closed = project.call(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert!(
        closed.status.success(),
        "{}{}",
        String::from_utf8_lossy(&closed.stdout),
        String::from_utf8_lossy(&closed.stderr)
    );
    assert!(String::from_utf8_lossy(&closed.stderr).contains("tested_inputs_drift"));
    let closed: Value = serde_json::from_slice(&closed.stdout).unwrap();
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(pty_status(&project).matches(CARD).count(), 0);
    let status = project.value(&["goal", "status", "--json"]);
    assert_eq!(status["closure"]["closed"], true);
    assert_eq!(status["currentReadiness"]["state"], "stale");
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn current_direct_room_survives_corrupt_or_missing_display_memo() {
    let project = Project::new("direct-goal-memo");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);
    let cache = project
        .root
        .join(".exitbind/presentation/session-goal.json");
    fs::write(&cache, b"corrupt cache").unwrap();
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);
    fs::remove_file(&cache).unwrap();
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);
    assert_eq!(
        project.value(&["goal", "status", "--json"])["currentReadiness"]["state"],
        "current"
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn new_goal_request_invalidates_direct_closure_and_machine_surfaces_stay_silent() {
    let project = Project::new("direct-goal-invalidate");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    let machine = project.value(&["goal", "status", "--json"]);
    assert_eq!(machine["closure"]["kind"], "direct");
    assert_eq!(machine["completionMode"], Value::Null);
    assert!(!serde_json::to_string(&machine).unwrap().contains(CARD));
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);

    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "new finding",
    ]);
    let after = pty_status(&project);
    assert_eq!(after.matches(CARD).count(), 0, "{after}");

    let headless = project.call(&["goal", "status"]);
    assert!(!String::from_utf8_lossy(&headless.stdout).contains(CARD));
    let closed_stdin = project.call(&["goal", "status"]);
    assert!(!String::from_utf8_lossy(&closed_stdin.stdout).contains(CARD));
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}
