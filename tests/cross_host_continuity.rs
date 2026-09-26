mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    work: String,
}

impl Fixture {
    fn new(label: &str) -> Self {
        Self::with_preservation_command(label, "true")
    }

    fn with_preservation_command(label: &str, preservation_command: &str) -> Self {
        let root = support::temp(label);
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let started = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&root)
            .args([
                "work",
                "begin",
                "change",
                "--goal",
                "Finish two requirements on one work",
                "--check-command",
                "true",
                "--preserve-requirement",
                "first:First requirement.",
                "--preservation-check-command",
                preservation_command,
                "--config",
                "exitbind.json",
            ])
            .output()
            .unwrap();
        assert!(started.status.success(), "{started:?}");
        let work = serde_json::from_slice::<Value>(&started.stdout).unwrap()["work"]
            .as_str()
            .unwrap()
            .to_owned();
        Self { root, work }
    }
    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .output()
            .unwrap()
    }
    fn record(&self, value: Value) -> Output {
        self.record_work(&self.work, value)
    }

    fn record_work(&self, work: &str, value: Value) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(["work", "record", work, "--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&value).unwrap())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    fn ok(&self, value: Value) -> Value {
        let output = self.record(value);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn view(&self) -> Value {
        let output = self.call(&["work", "continuation", &self.work]);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn history(&self) -> Vec<u8> {
        fs::read(self.root.join(".exitbind/session-goal.jsonl")).unwrap()
    }
    fn initialize(&self) {
        self.ok(json!({"action":"init","sourceRef":"fixture:requirements-v1",
            "sourceText":"First requirement. Second requirement.",
            "requirements":[{"id":"first","text":"First requirement."},{"id":"second","text":"Second requirement."}]}));
    }
    fn return_stage(&self, outcome: &str, body: &str) -> Value {
        let next = self.call(&["work", "next", &self.work, "--full"]);
        assert!(next.status.success(), "{next:?}");
        let next: Value = serde_json::from_slice(&next.stdout).unwrap();
        let assignment = next["next"]["assignment"].as_str().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args([
                "work",
                "return",
                &self.work,
                assignment,
                "--outcome",
                outcome,
                "--config",
                "exitbind.json",
            ])
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
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn sha(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[test]
fn continuation_attaches_to_existing_goal_without_replacing_its_history() {
    let f = Fixture::new("w2ch-existing-goal-attach");
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Existing goal",
        "--obligation",
        "Existing obligation",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let before = f.history();
    let input = json!({"action":"init","expectedRevision":1,
        "sourceRef":"fixture:requirements-v1",
        "sourceText":"First requirement. Second requirement.",
        "requirements":[{"id":"first","text":"First requirement."},
            {"id":"second","text":"Second requirement."}]});
    let attached = f.ok(input.clone());
    assert_eq!(attached["goalRevision"], 2);
    assert_eq!(attached["effect"], "appended");
    let after = f.history();
    assert!(after.starts_with(&before));
    let status = f.call(&["goal", "status", "--json"]);
    assert!(status.status.success(), "{status:?}");
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["obligations"][0]["id"], "Existing obligation");
    assert_eq!(status["goal"], "Existing goal");
    assert_eq!(f.ok(input.clone())["effect"], "unchanged");
    assert_eq!(f.history(), after);
    f.ok(
        json!({"action":"bind","expectedRevision":2,"expectedBindingRevision":0,
        "host":"codex","session":"same-work","hostVersion":"0.156.1"}),
    );
    f.ok(
        json!({"action":"refine","expectedRevision":3,"bindingRevision":1,
        "id":"first","text":"Refined first requirement.",
        "sourceRef":"operator:refinement","expectedRequirementRevision":1}),
    );
    let refined = f.history();
    assert_eq!(f.ok(input.clone())["effect"], "unchanged");
    assert_eq!(f.history(), refined);
    let mut conflict = input;
    conflict["sourceRef"] = json!("fixture:conflict");
    assert!(!f.record(conflict).status.success());
    assert_eq!(f.history(), refined);
}

#[test]
fn stale_attach_is_refused_before_a_goal_history_append() {
    let f = Fixture::new("w2ch-stale-attach");
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Existing goal",
        "--obligation",
        "Existing obligation",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let before = f.history();
    let refused = f.record(json!({"action":"init","expectedRevision":0,
        "sourceRef":"fixture:requirements-v1","sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}));
    assert!(!refused.status.success());
    assert_eq!(f.history(), before);
}

#[test]
fn closed_goal_cannot_be_reopened_by_continuation_init() {
    let f = Fixture::new("w2ch-closed-attach");
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Finished goal",
        "--none-applicable",
        "obligations,findings,blockers,decisions,externalActions",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let closed = f.call(&["goal", "close", "--goal-id", &f.work, "--direct"]);
    assert!(closed.status.success(), "{closed:?}");
    let before = f.history();
    let refused = f.record(json!({"action":"init","expectedRevision":2,
        "sourceRef":"fixture:requirements-v1","sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}));
    assert!(!refused.status.success());
    assert_eq!(f.history(), before);
}

#[test]
fn explicitly_incorporated_successor_keeps_predecessor_history() {
    let f = Fixture::new("w2ch-attached-successor");
    f.initialize();
    let predecessor = f.history();
    let started = f.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "Successor work",
        "--check-command",
        "true",
        "--review-policy",
        "required",
    ]);
    assert!(started.status.success(), "{started:?}");
    let successor: Value = serde_json::from_slice(&started.stdout).unwrap();
    let work = successor["work"].as_str().unwrap();
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        work,
        "--goal",
        "Successor work",
        "--obligation",
        "Successor obligation",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let incorporated: Value = serde_json::from_slice(&incorporated.stdout).unwrap();
    let revision = incorporated["revision"].as_u64().unwrap();
    let attached = f.record_work(
        work,
        json!({"action":"init","expectedRevision":revision,
        "sourceRef":"fixture:successor","sourceText":"Successor requirement.",
        "requirements":[{"id":"successor","text":"Successor requirement."}]}),
    );
    assert!(attached.status.success(), "{attached:?}");
    let after = f.history();
    assert!(after.starts_with(&predecessor));
    let viewed = f.call(&["work", "continuation", work]);
    assert!(viewed.status.success(), "{viewed:?}");
    let viewed: Value = serde_json::from_slice(&viewed.stdout).unwrap();
    assert_eq!(viewed["work"], work);
    assert_eq!(viewed["requirements"][0]["requirement"]["id"], "successor");
    let first: Value =
        serde_json::from_slice(predecessor.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    assert_eq!(first["goalId"], f.work);
}

#[test]
fn host_handoff_fences_stale_writers_and_preserves_uncertain_operations() {
    let f = Fixture::new("w2ch-binding");
    f.initialize();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(f.root.join(".exitbind/session-goal.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
    let view = f.view();
    assert_eq!(view["source"]["coverageConfirmed"], false);
    assert_eq!(view["requirements"].as_array().unwrap().len(), 2);
    assert_eq!(view["wholeGoalReady"], false);

    let first = f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"codex-1","hostVersion":"0.156.1"}),
    );
    assert_eq!(first["goalRevision"], 2);
    assert_eq!(first["effect"], "appended");
    assert_eq!(f.view()["binding"]["revision"], 1);
    let before = f.history();
    let duplicate = f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"codex-1","hostVersion":"0.156.1"}),
    );
    assert_eq!(duplicate["goalRevision"], 2);
    assert_eq!(duplicate["effect"], "unchanged");
    assert_eq!(f.history(), before);
    assert!(!f
        .record(
            json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"claude","session":"claude-1","hostVersion":"2.1.280"})
        )
        .status
        .success());
    assert_eq!(f.history(), before);

    f.ok(
        json!({"action":"correct","expectedRevision":2,"bindingRevision":1,
        "id":"no-publication","text":"Do not publish this fixture","scope":"publication",
        "sourceRef":"operator:fixture","supersedes":null}),
    );
    let params = sha("read-only probe v1");
    f.ok(
        json!({"action":"intent","expectedRevision":3,"bindingRevision":1,
        "id":"probe","parametersSha256":params,"description":"Read-only native probe"}),
    );
    let before = f.history();
    f.ok(
        json!({"action":"intent","expectedRevision":3,"bindingRevision":1,
        "id":"probe","parametersSha256":params,"description":"Read-only native probe"}),
    );
    assert_eq!(f.history(), before);
    assert!(!f
        .record(
            json!({"action":"intent","expectedRevision":4,"bindingRevision":1,
        "id":"probe","parametersSha256":sha("different"),"description":"Read-only native probe"})
        )
        .status
        .success());
    assert_eq!(f.history(), before);

    f.ok(
        json!({"action":"bind","expectedRevision":4,"expectedBindingRevision":1,
        "host":"claude","session":"claude-1","hostVersion":"2.1.280"}),
    );
    assert!(!f
        .record(
            json!({"action":"intent","expectedRevision":5,"bindingRevision":1,
        "id":"stale","parametersSha256":params,"description":"stale writer"})
        )
        .status
        .success());
    assert!(!f
        .record(
            json!({"action":"observe","expectedRevision":5,"bindingRevision":2,
        "id":"probe","status":"failed","handle":null,"resultEventSha256":null})
        )
        .status
        .success());

    f.ok(json!({"action":"child","expectedRevision":5,"bindingRevision":2,
        "assignment":"inspect second","nativeChild":"claude-child-1",
        "resultText":"Second requirement is still open.","resultSha256":sha("Second requirement is still open.")}));
    f.ok(
        json!({"action":"bind","expectedRevision":6,"expectedBindingRevision":2,
        "host":"codex","session":"codex-2","hostVersion":"0.156.1"}),
    );
    let view = f.view();
    assert_eq!(view["corrections"][0]["id"], "no-publication");
    assert_eq!(view["operations"][0]["status"], "uncertain");
    assert_eq!(view["children"][0]["nativeChild"], "claude-child-1");
    assert_eq!(view["binding"]["revision"], 3);
    assert_eq!(view["wholeGoalReady"], false);
    let entry = f.call(&["work", "next", &f.work]);
    assert!(entry.status.success(), "{entry:?}");
    let entry: Value = serde_json::from_slice(&entry.stdout).unwrap();
    assert_eq!(
        entry["continuation"]["corrections"][0]["id"],
        "no-publication"
    );
}

#[test]
fn exact_check_supports_one_revision_without_closing_other_requirements() {
    let f = Fixture::new("w2ch-support");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"codex-1","hostVersion":"0.156.1"}),
    );
    f.return_stage("scoped", "Bounded scope");
    f.return_stage("completed", "A completed worker result");
    let first_check = f.call(&["work", "check", &f.work]);
    assert!(first_check.status.success(), "{first_check:?}");
    let first_check: Value = serde_json::from_slice(&first_check.stdout).unwrap();
    let checked = f.call(&["work", "check", &f.work]);
    assert!(checked.status.success(), "{checked:?}");
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let event = &checked["event"];
    assert_eq!(event["action"], "check");
    assert_eq!(event["result"]["code"], 0);
    let event_sha = event["eventSha256"].as_str().unwrap();
    let exact = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        event_sha,
    ]);
    assert!(exact.status.success(), "{exact:?}");
    let exact_event: Value = serde_json::from_slice(&exact.stdout).unwrap();
    assert_eq!(exact_event["event"]["requirementId"], "first");
    let current = f.view();
    assert_eq!(
        exact_event["event"]["inputsSha256"],
        current["currentInputsSha256"]
    );
    let conditions = current["currentConditionsSha256"].as_str().unwrap();
    let before = f.history();
    assert!(!f
        .record(
            json!({"action":"support","expectedRevision":2,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":first_check["event"]["eventSha256"],"conditionsSha256":conditions})
        )
        .status
        .success());
    assert_eq!(f.history(), before);
    f.ok(
        json!({"action":"support","expectedRevision":2,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event_sha,"conditionsSha256":conditions}),
    );
    let view = f.view();
    assert_eq!(view["requirements"][0]["unresolved"], false);
    assert_eq!(view["requirements"][1]["unresolved"], true);
    assert_eq!(view["wholeGoalReady"], false);
    let before = f.history();
    let exact_again = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        event_sha,
    ]);
    assert!(exact_again.status.success(), "{exact_again:?}");
    assert_eq!(exact.stdout, exact_again.stdout);
    assert_eq!(f.history(), before);
    f.ok(
        json!({"action":"refine","expectedRevision":3,"bindingRevision":1,
        "id":"first","text":"First requirement with changed condition.",
        "sourceRef":"operator:revision","expectedRequirementRevision":1}),
    );
    let revised = f.view();
    assert_eq!(revised["requirements"][0]["requirement"]["revision"], 2);
    assert_eq!(revised["requirements"][0]["unresolved"], true);
    assert_eq!(
        revised["source"]["text"],
        "First requirement. Second requirement."
    );
    let before = f.history();
    let refusal = f.record(
        json!({"action":"support","expectedRevision":4,"bindingRevision":1,
            "requirementId":"first","requirementRevision":2,
            "resultEventSha256":event_sha,
            "conditionsSha256":revised["currentConditionsSha256"]}),
    );
    assert!(!refusal.status.success());
    assert!(String::from_utf8_lossy(&refusal.stderr)
        .contains("check predates the current requirement revision"));
    assert_eq!(f.history(), before);
    assert_eq!(f.view()["requirements"][0]["unresolved"], true);
}

#[test]
fn native_child_text_is_exact_and_retries_cannot_replace_it() {
    let f = Fixture::new("w2ch-child-exact");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"claude","session":"claude-1","hostVersion":"2.1.280"}),
    );
    let text = "Observed child result. ".repeat(180);
    assert!(text.len() > 2048);
    let input = json!({"action":"child","expectedRevision":2,"bindingRevision":1,
        "assignment":"inspect second","nativeChild":"native-agent-1",
        "resultText":text,"resultSha256":sha(&text)});
    f.ok(input.clone());
    let before = f.history();
    let view = f.view();
    assert_eq!(view["children"][0]["resultText"], text);
    f.ok(input);
    assert_eq!(f.history(), before);
    assert!(!f
        .record(
            json!({"action":"child","expectedRevision":3,"bindingRevision":1,
        "assignment":"inspect second","nativeChild":"native-agent-1",
        "resultText":"different","resultSha256":sha("different")})
        )
        .status
        .success());
    assert_eq!(f.history(), before);
}

#[test]
fn child_ids_are_scoped_to_native_sessions() {
    let f = Fixture::new("w2ch-child-origin");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let first = "first child result";
    f.ok(
        json!({"action":"child","expectedRevision":2,"bindingRevision":1,
        "assignment":"inspect","nativeChild":"local-1","resultText":first,
        "resultSha256":sha(first)}),
    );
    f.ok(
        json!({"action":"bind","expectedRevision":3,"expectedBindingRevision":1,
        "host":"claude","session":"s2","hostVersion":"2.1.280"}),
    );
    let second = "different child result";
    let input = json!({"action":"child","expectedRevision":4,"bindingRevision":2,
        "assignment":"inspect","nativeChild":"local-1","resultText":second,
        "resultSha256":sha(second)});
    f.ok(input.clone());
    let view = f.view();
    assert_eq!(view["children"].as_array().unwrap().len(), 2);
    assert_ne!(
        view["children"][0]["childKey"],
        view["children"][1]["childKey"]
    );
    assert_eq!(view["children"][0]["origin"]["session"], "s1");
    assert_eq!(view["children"][1]["origin"]["session"], "s2");
    let before = f.history();
    f.ok(input);
    assert_eq!(f.history(), before);
}

#[test]
fn accumulated_children_keep_a_bounded_read_only_route() {
    let f = Fixture::new("w2ch-child-overflow");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"claude","session":"s1","hostVersion":"2.1.280"}),
    );
    let text = "X".repeat(8192);
    for index in 0..8 {
        let recorded = f.record(
            json!({"action":"child","expectedRevision":2+index,"bindingRevision":1,
            "assignment":"inspect","nativeChild":format!("child-{index}"),
            "resultText":text,"resultSha256":sha(&text)}),
        );
        assert!(recorded.status.success(), "{recorded:?}");
        assert!(
            recorded.stdout.len() <= 8 * 1024,
            "{}",
            recorded.stdout.len()
        );
    }
    let corrected = f.record(json!({"action":"correct","expectedRevision":10,
        "bindingRevision":1,"id":"one","text":"Keep the current correction.",
        "scope":"task","sourceRef":"operator:current"}));
    assert!(corrected.status.success(), "{corrected:?}");
    assert!(
        corrected.stdout.len() <= 8 * 1024,
        "{}",
        corrected.stdout.len()
    );
    let corrected: Value = serde_json::from_slice(&corrected.stdout).unwrap();
    let argv = corrected["nextAction"]["command"].as_array().unwrap();
    assert_eq!(corrected["nextAction"]["sameConfigRequired"], false);
    assert_eq!(corrected["nextAction"]["sameExecutableRequired"], false);
    let recovered = Command::new(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["goalRevision"], 11);
    fs::copy(f.root.join("exitbind.json"), f.root.join("alternate.json")).unwrap();
    let entry = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args(["work", "next", &f.work, "--config", "alternate.json"])
        .output()
        .unwrap();
    assert!(entry.status.success(), "{entry:?}");
    let entry: Value = serde_json::from_slice(&entry.stdout).unwrap();
    let route = &entry["continuation"];
    assert_eq!(route["requiresExpansion"], true);
    assert_eq!(route["sameConfigRequired"], false);
    assert_eq!(route["sameExecutableRequired"], false);
    let argv = route["command"].as_array().unwrap();
    let expanded = Command::new(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    let children = expanded["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|section| section["name"] == "children")
        .unwrap();
    let argv = children["command"].as_array().unwrap();
    let section = Command::new(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(section.status.success(), "{section:?}");
    let section: Value = serde_json::from_slice(&section.stdout).unwrap();
    let argv = section["items"][7]["command"].as_array().unwrap();
    let item = Command::new(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(item.status.success(), "{item:?}");
    let item: Value = serde_json::from_slice(&item.stdout).unwrap();
    assert_eq!(item["item"]["resultText"], text);
    let view = f.view();
    assert_eq!(view["requiresExpansion"], true);
    assert_eq!(view["sections"].as_array().unwrap().len(), 7);
    assert!(f.call(&["work", "next", &f.work]).status.success());
    assert!(f.call(&["work", "resume"]).status.success());
    let section = f.call(&["work", "continuation", &f.work, "--section", "children"]);
    assert!(section.status.success(), "{section:?}");
    let section: Value = serde_json::from_slice(&section.stdout).unwrap();
    assert_eq!(section["requiresExpansion"], true);
    assert_eq!(section["count"], 8);
    let item = f.call(&[
        "work",
        "continuation",
        &f.work,
        "--section",
        "children",
        "--index",
        "7",
    ]);
    assert!(item.status.success(), "{item:?}");
    let item: Value = serde_json::from_slice(&item.stdout).unwrap();
    assert_eq!(item["item"]["resultText"], text);
    assert_eq!(item["item"]["resultSha256"], sha(&text));
}

#[test]
fn missing_check_logs_invalidate_support_without_erasing_history() {
    let f = Fixture::with_preservation_command("w2ch-missing-check-log", "printf check-evidence");
    f.ok(
        json!({"action":"init","sourceRef":"fixture:requirements-v1",
        "sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}),
    );
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let source_sha = f.view()["source"]["sha256"].as_str().unwrap().to_owned();
    f.ok(
        json!({"action":"cover","expectedRevision":2,"bindingRevision":1,
        "sourceSha256":source_sha}),
    );
    f.return_stage("scoped", "Bounded scope");
    f.return_stage("completed", "A completed worker result");
    assert!(f.call(&["work", "check", &f.work]).status.success());
    let checked = f.call(&["work", "check", &f.work]);
    assert!(checked.status.success(), "{checked:?}");
    let event: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let view = f.view();
    f.ok(
        json!({"action":"support","expectedRevision":3,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event["event"]["eventSha256"],
        "conditionsSha256":view["currentConditionsSha256"]}),
    );
    assert_eq!(f.view()["requirements"][0]["unresolved"], false);
    let completion = f.call(&[
        "goal",
        "incorporate",
        "--direct",
        "--goal-id",
        &f.work,
        "--goal",
        "Finish two requirements on one work",
        "--obligation",
        "first",
    ]);
    assert!(completion.status.success(), "{completion:?}");
    let exact = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        event["eventSha256"].as_str().unwrap(),
    ]);
    assert!(exact.status.success(), "{exact:?}");
    let exact: Value = serde_json::from_slice(&exact.stdout).unwrap();
    let path = exact["event"]["stdout"]["path"].as_str().unwrap();
    fs::remove_file(f.root.join(path)).unwrap();
    let after = f.view();
    assert_eq!(
        after["requirements"][0]["support"][0]["integrityCurrent"],
        false
    );
    assert_eq!(after["requirements"][0]["unresolved"], true);
    assert_eq!(after["wholeGoalReady"], false);
    let close = f.call(&["goal", "close", "--direct", "--goal-id", &f.work]);
    assert!(!close.status.success());
    assert!(
        String::from_utf8_lossy(&close.stderr)
            .contains("continuation support is stale or incomplete"),
        "{close:?}"
    );
    let before = f.history();
    let refusal = f.record(
        json!({"action":"support","expectedRevision":5,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event["event"]["eventSha256"],
        "conditionsSha256":after["currentConditionsSha256"]}),
    );
    assert!(!refusal.status.success());
    assert!(String::from_utf8_lossy(&refusal.stderr)
        .contains("check evidence is missing or unverified"));
    assert_eq!(f.history(), before);
}

#[test]
fn missing_accepted_result_artifact_invalidates_whole_goal_readiness() {
    let f = Fixture::new("w2ch-missing-accepted-artifact");
    f.ok(
        json!({"action":"init","sourceRef":"fixture:one-requirement",
        "sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}),
    );
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let source_sha = f.view()["source"]["sha256"].as_str().unwrap().to_owned();
    f.ok(
        json!({"action":"cover","expectedRevision":2,"bindingRevision":1,
        "sourceSha256":source_sha}),
    );
    f.return_stage("scoped", "Bounded scope");
    f.return_stage("completed", "A completed worker result");
    assert!(f.call(&["work", "check", &f.work]).status.success());
    let checked = f.call(&["work", "check", &f.work]);
    assert!(checked.status.success(), "{checked:?}");
    let event: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let conditions = f.view()["currentConditionsSha256"].clone();
    f.ok(
        json!({"action":"support","expectedRevision":3,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event["event"]["eventSha256"],
        "conditionsSha256":conditions}),
    );
    let accepted = loop {
        let next = f.call(&["work", "next", &f.work, "--full"]);
        assert!(next.status.success(), "{next:?}");
        let next: Value = serde_json::from_slice(&next.stdout).unwrap();
        let stage = &next["next"];
        if stage["outcomes"]
            .as_array()
            .is_some_and(|outcomes| outcomes.iter().any(|outcome| outcome == "accepted"))
        {
            break f.return_stage("accepted", "Accepted worker result");
        }
        match stage["action"].as_str().unwrap() {
            "check" => assert!(f.call(&["work", "check", &f.work]).status.success()),
            "spawn" if stage["role"] == "reviewer" => {
                f.return_stage("approved", "Reviewed worker result");
            }
            other => panic!("unexpected stage before acceptance: {other} {stage}"),
        }
    };
    let inspected = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        accepted["eventSha256"].as_str().unwrap(),
    ]);
    assert!(inspected.status.success(), "{inspected:?}");
    let inspected: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    let artifact = inspected["event"]["artifact"]["path"].as_str().unwrap();
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Finish two requirements on one work",
        "--obligation",
        "first",
        "--disposition",
        "accepted",
        "--result-ref",
        &f.work,
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let closed = f.call(&[
        "goal",
        "close",
        "--goal-id",
        &f.work,
        "--result-ref",
        &f.work,
    ]);
    assert!(closed.status.success(), "{closed:?}");
    assert_eq!(f.view()["wholeGoalReady"], true);
    let before = f.history();
    fs::remove_file(f.root.join(artifact)).unwrap();
    assert_eq!(f.view()["wholeGoalReady"], false);
    assert_eq!(f.history(), before);
}

#[test]
fn caller_reported_check_cannot_be_observed_requirement_support() {
    let f = Fixture::new("w2ch-reported-check");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    f.return_stage("scoped", "Bounded scope");
    let worker = f.return_stage("completed", "A completed worker result");
    assert!(f.call(&["work", "check", &f.work]).status.success());
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        f.work.strip_prefix("smw_").unwrap()
    );
    let reported = f.call(&[
        "run",
        "record-check",
        &ledger,
        "--target",
        worker["eventSha256"].as_str().unwrap(),
        "--requirement",
        "first",
        "--check-command",
        "true",
        "--exit-code",
        "0",
    ]);
    assert!(reported.status.success(), "{reported:?}");
    let reported: Value = serde_json::from_slice(&reported.stdout).unwrap();
    let conditions = f.view()["currentConditionsSha256"].clone();
    let before = f.history();
    let refusal = f.record(
        json!({"action":"support","expectedRevision":2,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":reported["event"]["eventSha256"],
        "conditionsSha256":conditions}),
    );
    assert!(!refusal.status.success());
    assert!(String::from_utf8_lossy(&refusal.stderr)
        .contains("support requires a passing check bound to the named requirement"));
    assert_eq!(f.history(), before);
}

#[test]
fn successor_goal_does_not_inherit_a_different_works_continuation() {
    let f = Fixture::new("w2ch-successor");
    f.initialize();
    let changed = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        "goal-b",
        "--goal",
        "New goal",
        "--obligation",
        "new obligation",
    ]);
    assert!(changed.status.success(), "{changed:?}");
    let changed: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert!(changed["continuation"].is_null());
    assert!(f.call(&["work", "next", &f.work]).status.success());
}

#[test]
fn long_requirement_history_has_bounded_exact_read_and_refinement_limit() {
    let f = Fixture::new("w2ch-history-expansion");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let prior_text = "First requirement.";
    for index in 0..32_u64 {
        f.ok(json!({"action":"refine","expectedRevision":index + 2,
            "bindingRevision":1,"id":"first",
            "text":format!("revision-{index}-{}", "\n".repeat(1000)),
            "sourceRef":format!("operator:{index}-{}", "\n".repeat(240)),
            "expectedRequirementRevision":index + 1}));
    }
    let before = f.history();
    let rejected = f.record(json!({"action":"refine","expectedRevision":34,
        "bindingRevision":1,"id":"first","text":"one revision too many",
        "sourceRef":"operator:limit","expectedRequirementRevision":33}));
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr)
        .contains("requirement refinement history bound exceeded"));
    assert_eq!(f.history(), before);

    let summary = f.call(&["work", "continuation", &f.work]);
    assert!(summary.status.success(), "{summary:?}");
    assert!(summary.stdout.len() <= 64 * 1024);
    let summary: Value = serde_json::from_slice(&summary.stdout).unwrap();
    assert_eq!(summary["requiresExpansion"], true);

    let section = f.call(&["work", "continuation", &f.work, "--section", "requirements"]);
    assert!(section.status.success(), "{section:?}");
    assert!(section.stdout.len() <= 64 * 1024);
    let section: Value = serde_json::from_slice(&section.stdout).unwrap();
    assert_eq!(section["requiresExpansion"], true);

    let item = f.call(&[
        "work",
        "continuation",
        &f.work,
        "--section",
        "requirements",
        "--index",
        "0",
    ]);
    assert!(item.status.success(), "{item:?}");
    assert!(item.stdout.len() <= 64 * 1024);
    let item: Value = serde_json::from_slice(&item.stdout).unwrap();
    assert_eq!(item["requiresExpansion"], true);
    assert_eq!(item["historyCount"], 32);

    let history = f.call(&[
        "work",
        "continuation",
        &f.work,
        "--section",
        "requirements",
        "--index",
        "0",
        "--history-index",
        "0",
    ]);
    assert!(history.status.success(), "{history:?}");
    assert!(history.stdout.len() <= 64 * 1024);
    let history: Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(history["historyEntry"]["text"], prior_text);
    assert_eq!(history["historyIndex"], 0);
    fs::copy(f.root.join("exitbind.json"), f.root.join("alternate.json")).unwrap();
    let alternate = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args([
            "work",
            "continuation",
            &f.work,
            "--section",
            "requirements",
            "--index",
            "0",
            "--config",
            "alternate.json",
        ])
        .output()
        .unwrap();
    assert!(alternate.status.success(), "{alternate:?}");
    let alternate: Value = serde_json::from_slice(&alternate.stdout).unwrap();
    assert_eq!(alternate["historyReadSameConfigRequired"], false);
    assert_eq!(alternate["historyReadSameExecutableRequired"], false);
    let argv = alternate["historyRead"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| {
            if part == "N" {
                "0"
            } else {
                part.as_str().unwrap()
            }
        })
        .collect::<Vec<_>>();
    let read = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(read.status.success(), "{read:?}");
    let read: Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(read["historyEntry"]["text"], prior_text);
    assert_eq!(f.history(), before);
}
