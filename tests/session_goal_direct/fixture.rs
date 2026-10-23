use crate::support;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    process::{Output, Stdio},
};

pub(crate) const CARD: &str =
    "+----------------------------+\n| This room remains nothing. |\n+----------------------------+";

pub(crate) struct Project {
    pub(crate) root: super::support::TempRoot,
    pub(crate) session_store: std::path::PathBuf,
}

impl Project {
    pub(crate) fn new(label: &str) -> Self {
        let root = support::temp(label);
        let mut init_command =
            super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
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

    pub(crate) fn call(&self, args: &[&str]) -> Output {
        self.command(None, None, None, args)
    }

    pub(crate) fn call_in_session(
        &self,
        host: &str,
        identity: &str,
        store: &std::path::Path,
        args: &[&str],
    ) -> Output {
        self.command(Some(host), Some(identity), Some(store), args)
    }

    pub(crate) fn command(
        &self,
        host: Option<&str>,
        identity: Option<&str>,
        store: Option<&std::path::Path>,
        args: &[&str],
    ) -> Output {
        let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
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

    pub(crate) fn call_with_native_selectors(
        &self,
        selectors: &[(&str, &str)],
        args: &[&str],
    ) -> Output {
        let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
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

    pub(crate) fn value(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub(crate) fn value_in_session(
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

    pub(crate) fn pty_status_in_session(
        &self,
        host: &str,
        identity: &str,
        store: &std::path::Path,
    ) -> String {
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

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn establish_direct_goal(project: &Project) {
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

pub(crate) fn establish_native_direct_goal(project: &Project, session: &str) {
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

pub(crate) fn native_goal_journal(project: &Project, session: &str) -> std::path::PathBuf {
    native_session_root(&project.session_store, "codex", session)
        .join(".exitbind/session-goal.jsonl")
}

pub(crate) fn canonical(value: &Value) -> String {
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

pub(crate) fn digest_value(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

pub(crate) fn digest_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(crate) fn native_session_root(
    store: &std::path::Path,
    host: &str,
    identity: &str,
) -> std::path::PathBuf {
    let key = digest_value(&json!({"host":host,"identity":identity}));
    store.join(format!("session-{key}"))
}

pub(crate) fn start_intent_sha256(start: &Value) -> String {
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
        "preReturnPolicy",
        "preservation",
        "basisProtocol",
        "recoveryProtocol",
        "observationNoveltyProtocol",
        "checkObservationProtocol",
        "basis",
        "reviewPolicy",
        "governor",
        "projectContinuation",
        "quarantineVerification",
        "verificationPreparationProtocol",
        "verificationCheckerSources",
        "guidanceContinuityProtocol",
        "guidanceConfigSha256",
        "verificationInputContract",
    ];
    let mut projection = Map::new();
    for field in FIELDS {
        if let Some(value) = start.get(*field) {
            projection.insert((*field).to_owned(), value.clone());
        }
    }
    digest_value(&Value::Object(projection))
}

pub(crate) fn append_goal_admission(session_root: &std::path::Path, admission: Value) {
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

pub(crate) fn admission_for_start(
    member: &Value,
    work: &str,
    start: &Value,
    parent_goal: &Value,
) -> Value {
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
