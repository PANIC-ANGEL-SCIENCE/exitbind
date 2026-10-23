//! Action forms expose only the current Lead outcomes and executable, bound
//! commands. These tests execute the descriptors, not the CLI spelling.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    ffi::OsStr,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    cleanup_root: PathBuf,
    executable: PathBuf,
    native_session: Option<NativeSession>,
}

struct NativeSession {
    identity: String,
    store: PathBuf,
}

const NATIVE_SESSION_ENV: [&str; 5] = [
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "CLAUDE_CODE_SESSION_ID",
    "EXITBIND_NATIVE_SESSION_ID",
    "EXITBIND_SESSION_GOALS_DIR",
];

fn isolated_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = support::git_topology::command(program);
    for name in NATIVE_SESSION_ENV {
        command.env_remove(name);
    }
    command
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        Self::at(root.clone(), root)
    }

    fn at(root: PathBuf, cleanup_root: PathBuf) -> Self {
        Self::at_with_executable(
            root,
            cleanup_root,
            PathBuf::from(env!("CARGO_BIN_EXE_exitbind")),
        )
    }

    fn at_with_executable(root: PathBuf, cleanup_root: PathBuf, executable: PathBuf) -> Self {
        fs::create_dir_all(&root).unwrap();
        support::git_topology::repository(&root);
        let output = isolated_command(&executable)
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self {
            root,
            cleanup_root,
            executable,
            native_session: None,
        }
    }

    fn with_codex_session(mut self) -> Self {
        let name = self
            .root
            .file_name()
            .expect("fixture root has a final component")
            .to_string_lossy()
            .into_owned();
        let store = self
            .root
            .with_file_name(format!("{name}-native-session-goals"));
        assert!(
            !store.exists(),
            "fixture native session store already exists: {}",
            store.display()
        );
        self.native_session = Some(NativeSession {
            identity: format!("action-forms-{name}"),
            store,
        });
        self
    }

    fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = isolated_command(program);
        if let Some(session) = &self.native_session {
            command
                .env("CODEX_THREAD_ID", &session.identity)
                .env("EXITBIND_SESSION_GOALS_DIR", &session.store);
        }
        command
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = self
            .command(&self.executable)
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }

    fn json(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn begin(&self, check: &str, review: bool) -> (String, Value) {
        self.begin_with_review_policy(check, review.then_some("required"))
    }

    fn begin_with_policy(&self, check: &str, review_policy: &str) -> (String, Value) {
        self.begin_with_review_policy(check, Some(review_policy))
    }

    fn begin_with_review_policy(
        &self,
        check: &str,
        review_policy: Option<&str>,
    ) -> (String, Value) {
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            "exercise executable action forms",
            "--check-command",
            check,
            "--proof-origin",
            "synthetic",
        ];
        if let Some(review_policy) = review_policy {
            args.extend(["--review-policy", review_policy]);
        }
        let value = self.json(&args, b"");
        let work = value["work"].as_str().unwrap().to_owned();
        let initial = self.next(&work);
        (work, initial)
    }

    fn next(&self, work: &str) -> Value {
        let full = self.json(&["work", "next", work, "--full"], b"");
        let compact = self.json(&["work", "next", work, "--json"], b"");
        assert_eq!(
            full["effectiveAction"]["exists"],
            compact["effectiveAction"]["exists"]
        );
        assert_eq!(
            full["effectiveAction"]["binding"],
            compact["effectiveAction"]["binding"]
        );
        assert_eq!(
            full["effectiveAction"]["kind"],
            compact["effectiveAction"]["kind"]
        );
        let mut next = full["next"].clone();
        let detail = self.json(&["work", "detail", work], b"");
        assert_eq!(detail["binding"], next["current"]["binding"]);
        assert_eq!(
            detail["effectiveAction"]["binding"],
            full["effectiveAction"]["binding"]
        );
        assert_eq!(
            detail["effectiveAction"]["kind"],
            full["effectiveAction"]["kind"]
        );
        assert_eq!(
            detail["effectiveAction"]["readiness"],
            full["effectiveAction"]["readiness"]
        );
        if detail["actionForms"]["choices"].is_array() {
            assert_eq!(
                detail["effectiveAction"]["choices"],
                detail["actionForms"]["choices"]
            );
        }
        next["current"]["actionForm"] = detail["actionForms"].clone();
        next
    }

    fn execute_choice(&self, choice: &Value, replacements: &[(&str, &str)], input: &[u8]) -> Value {
        let mut argv = self.expand_command(&choice["command"]);
        for argument in &mut argv {
            for (from, to) in replacements {
                if argument == from {
                    *argument = (*to).to_owned();
                }
            }
        }
        let output = self.run_argv(&argv, input);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn expand_command(&self, command: &Value) -> Vec<String> {
        let mut argv = command["argv"]
            .as_array()
            .expect("action command argv")
            .iter()
            .map(|item| item.as_str().expect("action argv is text").to_owned())
            .collect::<Vec<_>>();
        let executable = fs::canonicalize(&self.executable).unwrap();
        let config = fs::canonicalize(self.root.join("exitbind.json")).unwrap();
        if command["sameExecutableRequired"] == true {
            argv.insert(
                0,
                executable
                    .to_str()
                    .expect("fixture executable path is UTF-8")
                    .to_owned(),
            );
        } else {
            assert_eq!(
                fs::canonicalize(argv.first().expect("full action executable")).unwrap(),
                executable,
                "action command must use the fixture product executable"
            );
        }
        if command["sameConfigRequired"] == true {
            assert_eq!(argv.last().map(String::as_str), Some("--config"));
            argv.push(
                config
                    .to_str()
                    .expect("fixture config path is UTF-8")
                    .to_owned(),
            );
        } else {
            assert!(argv.len() >= 2);
            assert_eq!(argv[argv.len() - 2], "--config");
            let path = std::path::Path::new(argv.last().unwrap());
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.root.join(path)
            };
            assert_eq!(
                fs::canonicalize(path).unwrap(),
                config,
                "action command must retain the fixture config"
            );
        }
        argv
    }

    fn run_argv(&self, argv: &[String], input: &[u8]) -> Output {
        let mut child = self
            .command(argv[0].as_str())
            .current_dir(&self.root)
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }

    fn run_route(&self, route: &Value) -> Output {
        let command = json!({
            "argv": route["command"],
            "sameExecutableRequired": route["sameExecutableRequired"],
            "sameConfigRequired": route["sameConfigRequired"],
        });
        let argv = self.expand_command(&command);
        self.run_argv(&argv, b"")
    }

    fn choice(&self, next: &Value, label: &str) -> Value {
        next["current"]["actionForm"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|choice| choice["label"] == label)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "missing action choice {label}: {}",
                    next["current"]["actionForm"]
                )
            })
    }

    fn counting_check(&self, name: &str, exit_code: i32) -> String {
        let directory = self.root.with_extension("action-form-counters");
        fs::create_dir_all(&directory).unwrap();
        let counter = directory.join(name);
        let script = directory.join(format!("{name}.sh"));
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf x >> '{}'\nexit {exit_code}\n",
                counter.display()
            ),
        )
        .unwrap();
        format!("sh {}", script.display())
    }

    fn check_count(&self, name: &str) -> Vec<u8> {
        fs::read(self.root.with_extension("action-form-counters").join(name)).unwrap_or_default()
    }

    fn ledger_path(&self, work: &str) -> PathBuf {
        self.root.join(format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        ))
    }

    fn events(&self, work: &str) -> Vec<Value> {
        fs::read_to_string(self.ledger_path(work))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn assert_observed_check_link(&self, work: &str) {
        let events = self.events(work);
        let start = events
            .iter()
            .find(|event| event["action"] == "start")
            .unwrap();
        assert_eq!(start["version"], 8);
        assert_eq!(start["checkObservationProtocol"], 1);
        let admission = events
            .iter()
            .find(|event| event["action"] == "check_observation")
            .unwrap();
        let check = events
            .iter()
            .find(|event| event["action"] == "check")
            .unwrap();
        let binding = &admission["observation"]["binding"];
        assert!(
            binding.is_object(),
            "admission binding is malformed: {binding}"
        );
        for field in [
            "targetEventSha256",
            "subjectSha256",
            "configSha256",
            "checkCommandSha256",
            "inputsSha256",
        ] {
            assert!(
                binding[field].is_string(),
                "missing binding {field}: {binding}"
            );
            assert!(check[field].is_string(), "missing check {field}: {check}");
            assert_eq!(binding[field], check[field], "binding field {field}");
        }
        // Ordinary checks have no requirementId in the binding or the check
        // event; keep that absence/null equivalence from JSON access.
        assert!(binding["requirementId"].is_null());
        assert!(check["requirementId"].is_null());
        assert_eq!(binding["requirementId"], check["requirementId"]);
        assert_eq!(check["observationEventSha256"], admission["eventSha256"]);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(session) = &self.native_session {
            let _ = std::fs::remove_dir_all(&session.store);
        }
        let _ = std::fs::remove_dir_all(self.root.with_extension("action-form-counters"));
        let _ = std::fs::remove_dir_all(&self.cleanup_root);
    }
}

fn text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn initial_scope_form_exposes_only_scoped_and_blocked_and_executes() {
    let fixture = Fixture::new("action-form-scope");
    let (_work, initial) = fixture.begin("true", false);
    let form = &initial["current"]["actionForm"];
    assert_eq!(form["state"], "initial_scope");
    assert_eq!(form["choices"].as_array().unwrap().len(), 2);
    assert_eq!(form["choices"][0]["stdin"]["required"], true);
    assert!(form["choices"][0]["command"].get("program").is_none());
    assert!(form["choices"][0]["command"].get("args").is_none());
    let result = fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope recorded by the Lead")],
        b"the complete scope artifact\n",
    );
    assert_eq!(result["next"]["role"], "worker");
}

#[test]
fn ordinary_action_view_omits_inapplicable_contradiction_and_refuses_stale_return() {
    let fixture = Fixture::new("action-view-current");
    let (work, initial) = fixture.begin("true", false);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope agreed")],
        b"scope\n",
    );
    let before = fixture.json(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    assert!(!before["effectiveAction"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["label"] == "contradiction"));
    assert_eq!(before["instructions"]["complete"], false);
    assert_eq!(before["decision"]["readiness"], "IN_PROGRESS");
    assert_eq!(before["decision"]["result"]["exists"], false);
    assert!(before["decision"]["goal"]["workGoalSha256"].is_string());
    assert!(before["decision"]["obligations"].is_array());
    assert!(before.get("sections").is_none());
    assert!(before.get("recipientContext").is_none());
    let old_return = before["effectiveAction"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == "completed")
        .unwrap()
        .clone();
    let mut permit = fixture.expand_command(&before["effectiveAction"]["beforeEditing"]["command"]);
    let i = permit.iter().position(|s| s == "<OPERATION>").unwrap();
    permit[i] = "edit source".into();
    let out = fixture.run_argv(&permit, b"");
    assert!(out.status.success(), "{}", text(&out));
    let permit: Value = serde_json::from_slice(&out.stdout).unwrap();
    let output = fixture.run_route(&permit["nextAction"]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(output.stdout.len() <= 8192, "{} bytes", output.stdout.len());
    let current: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(current["binding"], before["binding"]);
    assert!(current["effectiveAction"]["currentGrant"].is_object());
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let recorded = fs::read(fixture.root.join(&ledger)).unwrap();
    let argv = fixture.expand_command(&old_return["command"]);
    let stale = fixture.run_argv(&argv, b"result\n");
    assert!(
        !stale.status.success(),
        "stale return unexpectedly succeeded"
    );
    assert!(
        text(&stale).contains("current action form binding is stale"),
        "stale return failed for the wrong reason: {}",
        text(&stale)
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), recorded);
    assert!(current["details"]["assignment"].is_object());
}

#[test]
fn oversized_reviewer_action_uses_complete_grouped_route_and_keeps_its_fence() {
    let cleanup_root = support::temp("action-view-long-root");
    // Calibrate the canonical root length across ordinary and deep TMPDIRs.
    // The four detail commands each retain the exact config path near their
    // 768-byte argv budget, preserving enough aggregate route bytes to exercise
    // the grouped fallback after action-command reprojection.
    let mut components = (0..3)
        .map(|index| format!("agent-host-managed-workspace-project-checkout-{index:02}"))
        .collect::<Vec<_>>();
    let fixed_len = cleanup_root.to_str().unwrap().len()
        + components.iter().map(String::len).sum::<usize>()
        + components.len();
    let target_len = 550;
    assert!(
        fixed_len <= target_len,
        "TMPDIR leaves no room for the grouped-route path fixture: {fixed_len} > {target_len}"
    );
    let padding = target_len - fixed_len;
    let per_component = padding / components.len();
    let remainder = padding % components.len();
    for (index, component) in components.iter_mut().enumerate() {
        component.push_str(&"x".repeat(per_component + usize::from(index < remainder)));
    }
    assert!(
        components.iter().all(|component| component.len() <= 255),
        "grouped-route fixture exceeds the filesystem component limit"
    );
    let mut root = cleanup_root.clone();
    for component in components {
        root.push(component);
    }
    fs::create_dir_all(&root).unwrap();
    assert_eq!(
        root.to_str().unwrap().len(),
        target_len,
        "{}",
        root.display()
    );
    // Hold executable length constant as well as config-root length. CI's
    // Cargo test binary lives at a substantially shorter path than the local
    // warm-target binary, and executable bytes participate in grouped-route
    // projection just like config bytes do.
    let mut binary_components = ["b0".to_owned(), "b1".to_owned()];
    let binary_fixed_len = cleanup_root.to_str().unwrap().len()
        + binary_components.iter().map(String::len).sum::<usize>()
        + binary_components.len()
        + 1
        + "exitbind".len();
    let binary_target_len = 400;
    let binary_component_count = binary_components.len();
    assert!(
        binary_fixed_len <= binary_target_len,
        "TMPDIR leaves no room for the executable-path fixture: {binary_fixed_len} > {binary_target_len}"
    );
    let binary_padding = binary_target_len - binary_fixed_len;
    for (index, component) in binary_components.iter_mut().enumerate() {
        component.push_str(&"x".repeat(
            binary_padding / binary_component_count
                + usize::from(index < binary_padding % binary_component_count),
        ));
    }
    assert!(
        binary_components
            .iter()
            .all(|component| component.len() <= 255),
        "executable-path fixture exceeds the filesystem component limit"
    );
    let mut executable = cleanup_root.join(&binary_components[0]);
    executable.push(&binary_components[1]);
    fs::create_dir_all(&executable).unwrap();
    executable.push("exitbind");
    support::place_executable(
        std::path::Path::new(env!("CARGO_BIN_EXE_exitbind")),
        &executable,
    );
    let executable = fs::canonicalize(executable).unwrap();
    assert_eq!(executable.to_str().unwrap().len(), binary_target_len);
    let fixture = Fixture::at_with_executable(root, cleanup_root, executable);
    let project_rule = "Reviewer: keep the complete current evidence and instructions.\n";
    fs::write(fixture.root.join("AGENTS.md"), project_rule).unwrap();

    let (work, initial) = fixture.begin("true", true);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope agreed")],
        b"scope\n",
    );
    let worker = fixture.next(&work);
    let worker_artifact = "complete worker evidence for the reviewer\n";
    fixture.json(
        &[
            "work",
            "return",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        worker_artifact.as_bytes(),
    );
    fixture.json(&["work", "check", &work], b"");
    let reviewer = fixture.next(&work);
    assert_eq!(reviewer["role"], "reviewer");

    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let before_action = fs::read(fixture.root.join(&ledger)).unwrap();
    let output = fixture.call(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(output.stdout.len() <= 8192, "{} bytes", output.stdout.len());
    let action: Value = serde_json::from_slice(&output.stdout).unwrap();
    fn has_reconstructable_action_command(value: &Value) -> bool {
        match value {
            Value::Object(object) => {
                if object.get("argv").and_then(Value::as_array).is_some() {
                    assert!(object["sameConfigRequired"].is_boolean());
                    assert!(object["sameExecutableRequired"].is_boolean());
                    if object["sameConfigRequired"] == true
                        || object["sameExecutableRequired"] == true
                    {
                        return true;
                    }
                }
                object.values().any(has_reconstructable_action_command)
            }
            Value::Array(values) => values.iter().any(has_reconstructable_action_command),
            _ => false,
        }
    }
    assert!(
        has_reconstructable_action_command(&action["effectiveAction"]),
        "long-path fixture no longer exercises bounded argv reconstruction"
    );

    fn expand_action_commands(value: &Value, executable: &str, config: &str) -> Value {
        match value {
            Value::Object(object) => {
                let mut expanded = object
                    .iter()
                    .map(|(key, child)| {
                        (
                            key.clone(),
                            expand_action_commands(child, executable, config),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                if let Some(argv) = expanded.get("argv").and_then(Value::as_array) {
                    if expanded["sameExecutableRequired"] == true
                        || expanded["sameConfigRequired"] == true
                    {
                        let mut argv = argv.clone();
                        if expanded["sameExecutableRequired"] == true {
                            argv.insert(0, json!(executable));
                        }
                        if expanded["sameConfigRequired"] == true {
                            argv.push(json!(config));
                        }
                        expanded.insert("argv".into(), Value::Array(argv));
                        expanded.insert("sameExecutableRequired".into(), json!(false));
                        expanded.insert("sameConfigRequired".into(), json!(false));
                    }
                }
                Value::Object(expanded)
            }
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|child| expand_action_commands(child, executable, config))
                    .collect(),
            ),
            value => value.clone(),
        }
    }
    assert_eq!(action["recipient"]["role"], "reviewer");
    assert_eq!(action["binding"], reviewer["current"]["binding"]);
    assert_eq!(action["decision"]["goal"], reviewer["current"]["goal"]);
    assert_eq!(action["decision"]["result"], reviewer["current"]["result"]);
    assert_eq!(
        action["decision"]["obligations"],
        reviewer["packet"]["context"]["obligations"]
    );
    assert_eq!(action["instructions"]["complete"], false);
    assert_eq!(action["effectiveAction"]["formState"], "pending_assignment");
    assert_eq!(
        action["effectiveAction"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|choice| choice["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["approved", "rework", "blocked", "unavailable"]
    );
    assert_eq!(action["details"].as_object().unwrap().len(), 1);
    assert_eq!(action["details"]["grouped"]["readOnly"], true);
    assert_eq!(
        action["details"]["grouped"],
        reviewer["current"]["details"]["grouped"]
    );
    let mut original_navigation = action.clone();
    original_navigation["details"] = reviewer["current"]["details"].clone();
    original_navigation["details"]
        .as_object_mut()
        .unwrap()
        .remove("grouped");
    assert!(
        serde_json::to_vec(&original_navigation).unwrap().len() + 1 > 8192,
        "fixture no longer reproduces the oversized complete action view"
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before_action);

    let grouped_route = action["details"]["grouped"].clone();
    let expanded = fixture.run_route(&grouped_route);
    assert!(expanded.status.success(), "{}", text(&expanded));
    let grouped: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(grouped["complete"], true);
    assert_eq!(grouped["binding"], action["binding"]);
    assert_eq!(grouped["recipient"]["role"], "reviewer");
    assert_eq!(grouped["recipientContext"]["complete"], true);
    let profile = &grouped["recipientContext"]["profile"];
    assert_eq!(profile["complete"], true);
    assert_eq!(profile["sha256"], reviewer["packet"]["profileSha256"]);
    assert!(!profile["content"].as_str().unwrap().is_empty());
    let rules = grouped["recipientContext"]["rules"].as_array().unwrap();
    let delivered_rule = rules
        .iter()
        .find(|rule| rule["path"] == "AGENTS.md")
        .unwrap();
    assert_eq!(delivered_rule["content"], project_rule);
    assert_eq!(delivered_rule["complete"], true);
    assert!(rules.iter().all(|rule| rule["complete"] == true));
    assert_eq!(grouped["sections"]["evidence"]["complete"], true);
    assert!(grouped["sections"]["evidence"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["artifact"]["content"] == worker_artifact));
    assert!(grouped["sections"]["evidence"]["checkEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["kind"] == "check" && check["status"] == "passed"));
    let executable = fs::canonicalize(&fixture.executable)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let config = fs::canonicalize(fixture.root.join("exitbind.json"))
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        expand_action_commands(&grouped["actionForms"]["choices"], &executable, &config),
        expand_action_commands(&action["effectiveAction"]["choices"], &executable, &config),
        "bounded action view changed choices beyond its reconstructable command paths"
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before_action);

    fixture.execute_choice(
        &fixture.choice(&reviewer, "approved"),
        &[("<REASON>", "review completed")],
        b"review completed\n",
    );
    let after_review = fs::read(fixture.root.join(&ledger)).unwrap();
    let stale = fixture.run_route(&grouped_route);
    assert!(
        !stale.status.success(),
        "stale route was accepted: {}",
        text(&stale)
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), after_review);
}

#[test]
fn marked_basis_keeps_the_executable_contradiction_route() {
    let fixture = Fixture::new("action-view-basis");
    let basis = r#"{"version":1,"constraints":["preserve history"],"openZones":["worker implementation"],"decisiveCases":["return a contradiction"]}"#;
    let begun = fixture.json(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "exercise marked basis",
            "--check-command",
            "true",
            "--basis",
            basis,
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = begun["work"].as_str().unwrap();
    let initial = fixture.next(work);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope agreed")],
        b"marked scope\n",
    );
    let worker = fixture.next(work);
    let result = fixture.execute_choice(
        &fixture.choice(&worker, "contradiction"),
        &[],
        b"The marked constraint is contradicted.\n",
    );
    assert_eq!(result["next"]["role"], "lead");
}

#[test]
fn worker_permit_form_uses_stable_binding_request_id_and_replays_exactly() {
    let fixture = Fixture::new("action-form-permit-id");
    let (work, initial) = fixture.begin("true", false);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope agreed")],
        b"scope\n",
    );
    let worker = fixture.next(&work);
    let form = &worker["current"]["actionForm"]["beforeEditing"];
    let operation = &form["inputConstraints"]["OPERATION"];
    assert_eq!(operation["type"], "string");
    assert_eq!(operation["unit"], "UTF-8 bytes");
    assert_eq!(operation["minimumBytes"], 1);
    assert_eq!(operation["maximumBytes"], 120);
    assert_eq!(operation["nonBlank"], true);
    let binding = worker["current"]["binding"].as_str().unwrap();
    let request_id = format!("permit-{binding}");
    let argv = form["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(argv[1], "work");
    assert_eq!(argv[2], "permit");
    let request_index = argv
        .iter()
        .position(|argument| argument == "--request-id")
        .unwrap();
    assert_eq!(argv[request_index + 1], request_id);
    let operation_index = argv
        .iter()
        .position(|argument| argument == "--operation")
        .unwrap();
    assert_eq!(argv[operation_index + 1], "<OPERATION>");
    assert_eq!(
        fixture.next(&work)["current"]["actionForm"]["beforeEditing"]["command"]["argv"],
        form["command"]["argv"]
    );

    let mut executable = argv.clone();
    executable[operation_index + 1] = "edit source".into();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let first = fixture.run_argv(&executable, b"");
    assert!(first.status.success(), "{}", text(&first));
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["allowed"], true);
    assert!(first["currentGrant"].is_object(), "{first}");
    let after_first = fs::read(fixture.root.join(&ledger)).unwrap();
    let replay = fixture.run_argv(&executable, b"");
    assert!(replay.status.success(), "{}", text(&replay));
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["idempotent"], true);
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), after_first);
    let current = fixture.next(&work)["current"]["actionForm"].clone();
    assert!(current["currentGrant"].is_object());
    assert!(current.get("beforeEditing").is_none());
}

#[test]
fn permit_operation_bytes_refuse_before_effect_and_accept_exact_boundaries() {
    for valid in ["x".to_owned(), "x".repeat(120), "가".repeat(40)] {
        let fixture = Fixture::new("action-form-operation-bytes");
        let (work, initial) = fixture.begin("true", false);
        fixture.execute_choice(
            &fixture.choice(&initial, "scoped"),
            &[("<REASON>", "scope agreed")],
            b"bounded operation test\n",
        );
        let worker = fixture.next(&work);
        let form = &worker["current"]["actionForm"]["beforeEditing"];
        let mut argv = form["command"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let index = argv.iter().position(|v| v == "--operation").unwrap() + 1;
        let before = fixture.next(&work);
        for rejected in [
            "".to_owned(),
            " \t".to_owned(),
            "x".repeat(121),
            format!("{}x", "가".repeat(40)),
        ] {
            argv[index] = rejected;
            let out = fixture.run_argv(&argv, b"");
            assert!(!out.status.success(), "{}", text(&out));
            assert!(text(&out).contains("at most 120 bytes"));
            assert_eq!(fixture.next(&work), before, "refusal recorded an effect");
        }
        let mut malformed = argv.clone();
        malformed[4] = "sma_malformed".into();
        malformed[index] = valid.clone();
        assert!(!fixture.run_argv(&malformed, b"").status.success());
        assert_eq!(fixture.next(&work), before);
        argv[index] = valid;
        let out = fixture.run_argv(&argv, b"");
        assert!(out.status.success(), "{}", text(&out));
        let admitted: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(admitted["allowed"], true);
        let saved = fixture.next(&work);
        let out = fixture.run_argv(&argv, b"");
        assert!(out.status.success(), "{}", text(&out));
        let replay: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(replay["idempotent"], true);
        assert_eq!(
            fixture.next(&work),
            saved,
            "lost-reply recovery repeated an effect"
        );
    }
}

#[test]
fn stale_form_binding_rejects_same_assignment_after_goal_revision() {
    let fixture = Fixture::new("action-form-binding").with_codex_session();
    fixture.json(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "action-form-goal",
            "--goal",
            "preserve the current assignment",
            "--obligation",
            "record the first scope",
        ],
        b"",
    );
    let (work, initial) = fixture.begin("true", false);
    let action_view = fixture.json(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    assert_eq!(action_view["goalAssociation"]["kind"], "journey_member");
    assert_eq!(action_view["goalAssociation"]["work"], work);
    let grouped = fixture.json(&["work", "detail", &work, "--json"], b"");
    assert_eq!(grouped["goalAssociation"]["kind"], "journey_member");
    assert_eq!(grouped["goalAssociation"]["work"], work);
    let old_assignment = initial["assignment"].clone();
    let stale = fixture.choice(&initial, "scoped");
    fixture.json(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "action-form-goal",
            "--goal",
            "the goal has been revised while the assignment remains pending",
            "--obligation",
            "record the revised scope",
        ],
        b"",
    );
    let mut argv = stale["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for argument in &mut argv {
        if argument == "<REASON>" {
            *argument = "stale scope command".into();
        }
    }
    let refused = fixture.run_argv(&argv, b"scope\n");
    assert!(
        !refused.status.success(),
        "stale form executed: {}",
        text(&refused)
    );
    assert!(text(&refused).contains("binding"), "{}", text(&refused));
    let current = fixture.next(&work);
    assert_eq!(current["assignment"], old_assignment);
    assert_eq!(current["role"], "lead");
}

#[test]
fn failed_check_form_offers_rework_without_reviewer_disposition() {
    let fixture = Fixture::new("action-form-check");
    let (work, initial) = fixture.begin("false", false);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope recorded")],
        b"scope\n",
    );
    let worker = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation\n",
    );
    let checked = fixture.call(&["work", "check", &work], b"");
    assert!(!checked.status.success(), "failed check returned success");
    let recorded: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(recorded["status"], "recorded");
    let lead = fixture.next(&work);
    let form = &lead["current"]["actionForm"];
    assert_eq!(form["state"], "failed_check");
    assert_eq!(
        form["choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|choice| choice["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "rework",
            "blocked",
            "environmental",
            "preview replacement of an explicitly defective frozen check"
        ]
    );
    let preview = &form["choices"][3]["command"]["argv"];
    let preview = preview.as_array().unwrap();
    assert!(preview.iter().any(|arg| arg == "replace-check"));
    assert!(preview
        .windows(2)
        .any(|args| args == [json!("--classification"), json!("defective-contract")]));
    assert!(!preview.iter().any(|arg| arg == "--apply"));
    assert!(lead["packet"].get("pendingDisposition").is_none());
    let reworked = fixture.execute_choice(
        &fixture.choice(&lead, "rework"),
        &[("<REASON>", "check failure requires repair")],
        b"repair plan\n",
    );
    assert_eq!(reworked["next"]["role"], "worker");
    assert_eq!(fixture.next(&work)["packet"]["attempt"], 2);
}

#[test]
fn stale_failed_check_exposes_only_current_lead_recovery() {
    let fixture = Fixture::new("action-form-stale-failed");
    let check = fixture.counting_check("failed", 1);
    let (work, initial) = fixture.begin_with_policy(&check, "required");
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope recorded")],
        b"scope\n",
    );
    let worker = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation attempt one\n",
    );
    assert_eq!(fixture.next(&work)["action"], "check");
    let checked = fixture.call(&["work", "check", &work], b"");
    assert!(
        !checked.status.success(),
        "failed check unexpectedly passed"
    );
    assert_eq!(fixture.check_count("failed"), b"x");
    fixture.assert_observed_check_link(&work);
    let current_lead = fixture.next(&work);
    assert_eq!(current_lead["role"], "lead");
    assert_eq!(current_lead["action"], "lead_decision");
    let old_check = fixture
        .events(&work)
        .into_iter()
        .find(|event| event["action"] == "check")
        .unwrap();
    let old_target = old_check["targetEventSha256"].as_str().unwrap();
    fs::write(
        fixture.root.join("input-drift.txt"),
        "changed after check\n",
    )
    .unwrap();

    let lead = fixture.next(&work);
    assert_eq!(lead["action"], "lead_decision");
    assert_eq!(lead["current"]["actionForm"]["state"], "failed_check");
    assert_eq!(lead["staleEndedCheck"]["status"], "failed");
    let labels = lead["current"]["actionForm"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["label"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "rework",
            "blocked",
            "preview replacement of an explicitly defective frozen check"
        ]
    );
    assert!(!labels.contains(&"accepted"));

    let ledger = fixture.ledger_path(&work);
    let before_replay = fs::read(&ledger).unwrap();
    let old_target = old_target.to_owned();
    let ledger_arg = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let replay = fixture.call(
        &["run", "observe-check", &ledger_arg, "--target", &old_target],
        b"",
    );
    assert!(
        !replay.status.success(),
        "stale replay unexpectedly succeeded"
    );
    assert_eq!(fixture.check_count("failed"), b"x");
    assert_eq!(fs::read(&ledger).unwrap(), before_replay);

    let reworked = fixture.execute_choice(
        &fixture.choice(&lead, "rework"),
        &[("<REASON>", "stale failed check requires repair")],
        b"repair plan\n",
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let next_worker = fixture.next(&work);
    assert_eq!(next_worker["packet"]["attempt"], 2);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            next_worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation attempt two\n",
    );
    let new_target_action = fixture.next(&work);
    assert_eq!(new_target_action["action"], "check");
    assert!(new_target_action.get("staleEndedCheck").is_none());
    let new_check = fixture.call(&["work", "check", &work], b"");
    assert!(
        !new_check.status.success(),
        "fresh failing check unexpectedly passed"
    );
    assert_eq!(fixture.check_count("failed"), b"xx");
    let events = fixture.events(&work);
    let checked_targets = events
        .iter()
        .filter(|event| event["action"] == "check")
        .map(|event| event["targetEventSha256"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(checked_targets.len(), 2);
    assert_eq!(checked_targets[0], old_target);
    assert_ne!(checked_targets[1], old_target);
    assert_eq!(fixture.ledger_path(&work), ledger);
}

#[test]
fn stale_successful_check_preserves_reviewer_rework_disposition() {
    let fixture = Fixture::new("action-form-stale-success-review");
    let check = fixture.counting_check("successful", 0);
    let (work, initial) = fixture.begin(&check, true);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope recorded")],
        b"scope\n",
    );
    let worker = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation attempt one\n",
    );
    fixture.json(&["work", "check", &work], b"");
    assert_eq!(fixture.check_count("successful"), b"x");
    fixture.assert_observed_check_link(&work);
    let old_check = fixture
        .events(&work)
        .into_iter()
        .find(|event| event["action"] == "check")
        .unwrap();
    let old_target = old_check["targetEventSha256"].as_str().unwrap().to_owned();
    let reviewer = fixture.next(&work);
    assert_eq!(reviewer["role"], "reviewer");
    fixture.json(
        &[
            "work",
            "return",
            &work,
            reviewer["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        b"finding requires repair\n",
    );
    let pending_finding = fixture.next(&work);
    assert_eq!(pending_finding["role"], "lead");
    assert_eq!(pending_finding["action"], "lead_decision");
    assert_eq!(
        pending_finding["current"]["actionForm"]["state"],
        "pending_review_finding"
    );
    fs::write(
        fixture.root.join("input-drift.txt"),
        "changed after check\n",
    )
    .unwrap();

    let lead = fixture.next(&work);
    assert_eq!(lead["action"], "lead_decision");
    assert_eq!(lead["staleEndedCheck"]["status"], "passed");
    assert_eq!(
        lead["current"]["actionForm"]["state"],
        "pending_review_finding"
    );
    let labels = lead["current"]["actionForm"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["label"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["repair", "defer", "reject", "supersede"]);
    let ledger = fixture.ledger_path(&work);
    let before_replay = fs::read(&ledger).unwrap();
    let ledger_arg = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let replay = fixture.call(
        &["run", "observe-check", &ledger_arg, "--target", &old_target],
        b"",
    );
    assert!(
        !replay.status.success(),
        "stale replay unexpectedly succeeded"
    );
    assert_eq!(fixture.check_count("successful"), b"x");
    assert_eq!(fs::read(&ledger).unwrap(), before_replay);

    let repaired = fixture.execute_choice(
        &fixture.choice(&lead, "repair"),
        &[
            ("<REASON>", "repair the reported finding"),
            ("<REPAIR_BOUNDARY>", "the identified implementation path"),
            ("<REGRESSION>", "the focused regression passes"),
        ],
        b"",
    );
    assert_eq!(repaired["next"]["role"], "worker");
    let next_worker = fixture.next(&work);
    assert_eq!(next_worker["packet"]["attempt"], 2);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            next_worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation attempt two\n",
    );
    assert_eq!(fixture.next(&work)["action"], "check");
    fixture.json(&["work", "check", &work], b"");
    assert_eq!(fixture.check_count("successful"), b"xx");
    let events = fixture.events(&work);
    let checked_targets = events
        .iter()
        .filter(|event| event["action"] == "check")
        .map(|event| event["targetEventSha256"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(checked_targets.len(), 2);
    assert_eq!(checked_targets[0], old_target);
    assert_ne!(checked_targets[1], old_target);
    assert_eq!(fixture.ledger_path(&work), ledger);
}

#[test]
fn stale_successful_check_cannot_offer_acceptance_or_reopen_terminal_work() {
    let fixture = Fixture::new("action-form-stale-success-lead");
    let check = fixture.counting_check("successful", 0);
    let (work, initial) = fixture.begin_with_policy(&check, "omitted");
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope recorded")],
        b"scope\n",
    );
    let worker = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation attempt one\n",
    );
    fixture.json(&["work", "check", &work], b"");
    assert_eq!(fixture.check_count("successful"), b"x");
    fixture.assert_observed_check_link(&work);
    let current_lead = fixture.next(&work);
    assert_eq!(current_lead["role"], "lead");
    assert_eq!(current_lead["action"], "lead_decision");
    fs::write(
        fixture.root.join("input-drift.txt"),
        "changed after check\n",
    )
    .unwrap();

    let stale = fixture.next(&work);
    assert_eq!(stale["staleEndedCheck"]["status"], "passed");
    assert_eq!(
        stale["current"]["actionForm"]["state"],
        "stale_successful_check"
    );
    let labels = stale["current"]["actionForm"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["label"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["rework", "blocked"]);
    assert!(!labels.contains(&"accepted"));

    fixture.execute_choice(
        &fixture.choice(&stale, "rework"),
        &[("<REASON>", "current inputs require a fresh attempt")],
        b"rework plan\n",
    );
    let next_worker = fixture.next(&work);
    assert_eq!(next_worker["packet"]["attempt"], 2);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            next_worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation attempt two\n",
    );
    assert_eq!(fixture.next(&work)["action"], "check");
    fixture.json(&["work", "check", &work], b"");
    assert_eq!(fixture.check_count("successful"), b"xx");
    let current_lead = fixture.next(&work);
    assert_eq!(
        current_lead["current"]["actionForm"]["state"],
        "lead_acceptance"
    );
    let accepted = fixture.execute_choice(
        &fixture.choice(&current_lead, "accepted"),
        &[("<REASON>", "the current result satisfies the scope")],
        b"accepted current result\n",
    );
    assert_eq!(accepted["presentation"]["terminal"], "EXIT READY");
    fs::write(
        fixture.root.join("input-drift.txt"),
        "changed after acceptance\n",
    )
    .unwrap();
    let terminal = fixture.next(&work);
    assert_eq!(terminal["action"], "done");
    assert_eq!(terminal["current"]["phase"]["status"], "accepted");
}

#[test]
fn adverse_review_form_executes_repair_and_rejects_its_stale_replay() {
    let fixture = Fixture::new("action-form-review");
    let (work, initial) = fixture.begin("true", true);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope recorded")],
        b"scope\n",
    );
    let worker = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation\n",
    );
    fixture.json(&["work", "check", &work], b"");
    let reviewer = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            reviewer["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        b"finding\n",
    );
    let lead = fixture.next(&work);
    let form = &lead["current"]["actionForm"];
    assert_eq!(form["state"], "pending_review_finding");
    assert_eq!(
        form["choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|choice| choice["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["repair", "defer", "reject", "supersede"]
    );
    let repair = fixture.choice(&lead, "repair");
    assert_eq!(repair["stdin"]["required"], false);
    let mut argv = repair["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for argument in &mut argv {
        *argument = match argument.as_str() {
            "<REASON>" => "repair the reported parser defect".into(),
            "<REPAIR_BOUNDARY>" => "parser line endings only".into(),
            "<REGRESSION>" => "CRLF parser check passes".into(),
            other => other.into(),
        };
    }
    let decided = fixture.run_argv(&argv, b"");
    assert!(decided.status.success(), "{}", text(&decided));
    let decided: Value = serde_json::from_slice(&decided.stdout).unwrap();
    assert_eq!(decided["next"]["role"], "worker");
    let stale = fixture.run_argv(&argv, b"");
    assert!(
        !stale.status.success(),
        "stale descriptor executed: {}",
        text(&stale)
    );
}

#[test]
fn final_acceptance_form_executes_after_current_check_and_review() {
    let fixture = Fixture::new("action-form-acceptance");
    let (work, initial) = fixture.begin("true", true);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope agreed")],
        b"scope",
    );
    let worker = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implemented result",
    );
    fixture.json(&["work", "check", &work], b"");
    let reviewer = fixture.next(&work);
    fixture.json(
        &[
            "work",
            "return",
            &work,
            reviewer["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        b"independent review",
    );
    let lead = fixture.next(&work);
    assert_eq!(lead["current"]["actionForm"]["state"], "lead_acceptance");
    let result = fixture.execute_choice(
        &fixture.choice(&lead, "accepted"),
        &[("<REASON>", "current evidence satisfies the scope")],
        b"lead acceptance",
    );
    assert_eq!(result["presentation"]["terminal"], "EXIT READY");
    let done = fixture.json(&["work", "next", &work, "--json"], b"");
    assert_eq!(done["effectiveAction"]["state"], "done");
    assert_eq!(done["effectiveAction"]["exists"], false);
}

#[test]
fn failed_preservation_check_uses_the_same_bound_rework_form() {
    let fixture = Fixture::new("action-form-preservation");
    let started = fixture.json(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preserve identity",
            "--check-command",
            "true",
            "--preserve-requirement",
            "identity:keep the record",
            "--preservation-check-command",
            "false",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    let initial = fixture.next(work);
    fixture.execute_choice(
        &fixture.choice(&initial, "scoped"),
        &[("<REASON>", "scope agreed")],
        b"scope",
    );
    let worker = fixture.next(work);
    fixture.json(
        &[
            "work",
            "return",
            work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"implementation",
    );
    fixture.json(&["work", "check", work], b"");
    let checked = fixture.call(&["work", "check", work], b"");
    assert!(
        !checked.status.success(),
        "failed preservation check returned success"
    );
    let lead = fixture.next(work);
    assert_eq!(lead["progress"]["reason"]["code"], "preservation_failed");
    assert_eq!(lead["current"]["actionForm"]["state"], "failed_check");
    let repaired = fixture.execute_choice(
        &fixture.choice(&lead, "rework"),
        &[("<REASON>", "repair the preservation counterexample")],
        b"preservation repair boundary",
    );
    assert_eq!(repaired["next"]["role"], "worker");
}
