//! Real CLI routing and the existing memory lifecycle, without a distributor.
#![cfg(unix)]
#[path = "specialist_routing/consumer_regressions.rs"]
mod consumer_regressions;
mod support;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Output};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn input(&self, args: &[&str], input: &str) -> Value {
        use std::io::Write;
        use std::process::Stdio;
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&self.root).args(args);
        if args.first() != Some(&"hook-run") {
            command.args(["--config", "exitbind.json"]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input.as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn worker(&self, task: &str) -> String {
        let begun = self.ok(&[
            "work",
            "begin",
            "change",
            "--goal",
            task,
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "records:Check records.",
            "--preservation-check-command",
            "true",
        ]);
        let work = begun["work"].as_str().unwrap();
        let assignment = begun["next"]["assignment"].as_str().unwrap();
        self.input(
            &["work", "return", work, assignment, "--outcome", "scoped"],
            "Fixture scope",
        );
        work.to_owned()
    }

    fn new() -> Self {
        Self::with_label("specialist-routing")
    }

    fn with_label(label: &str) -> Self {
        let root = support::temp(label);
        support::git_topology::repository(&root);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--skip-skills", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let f = Self { root };
        let mut c = f.config();
        let mut worker = c["agents"]
            .as_object_mut()
            .unwrap()
            .remove("worker")
            .unwrap();
        worker["purpose"] = json!("Bounded record aggregation");
        worker["nativeName"] = json!("record_specialist");
        worker["runtime"] = json!({"host":"codex","fallback":"none"});
        worker["commands"] = json!(["python3"]);
        worker["memoryRead"] = json!(["specialist.record-specialist.v1"]);
        worker["memoryWrite"] = json!(["specialist.record-specialist.v1"]);
        worker["crossContext"] = json!("protocol-only");
        c["agents"]["record-specialist"] = worker;
        c["workflows"]["change"]["workers"] = json!(["record-specialist"]);
        c["memory"] = json!({"root":".exitbind/memory","maxItems":8,"maxBytes":16384,
            "protocolScopes":["specialist.record-specialist.v1"],"syntheticScopes":[]});
        c["agents"]["reviewer"]["memoryReview"] = json!(["specialist.record-specialist.v1"]);
        c["agents"]["lead"]["memoryPromote"] = json!(["specialist.record-specialist.v1"]);
        c["agents"]["lead"]["memoryRevoke"] = json!(["specialist.record-specialist.v1"]);
        f.save(&c);
        f
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("exitbind.json")).unwrap()).unwrap()
    }
    fn save(&self, c: &Value) {
        fs::write(self.root.join("exitbind.json"), c.to_string()).unwrap();
    }
    fn call(&self, args: &[&str]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"]);
        if (args.first() != Some(&"memory")
            && !(args.first() == Some(&"work") && matches!(args.get(1), Some(&"bind" | &"child"))))
            || (args.first() == Some(&"memory")
                && matches!(args.get(1), Some(&"resolve" | &"inspect")))
        {
            command.arg("--json");
        }
        command.output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.call(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn route(&self, task: &str) -> Value {
        self.ok(&[
            "project",
            "route",
            "--task",
            task,
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex",
        ])
    }
    fn draft(&self, source: &str, revision: &str) {
        let identity = self.ok(&["project", "context"])["project"]["memoryIdentity"].clone();
        let v = json!({"version":1,"id":"presence","revision":revision,"kind":"skill","author":"record-specialist",
            "projectIdentity":identity,"taskTerms":["aggregation"],"hosts":["codex"],
            "summary":"Separate observed zero from unavailable values.","preconditions":["Schema distinguishes absence."],
            "procedure":["Track presence and sum independently."],"verification":["Exercise all-missing and observed-zero groups."],
            "requiredCommands":["python3"],"evidence":["task-a-report.md"],"guards":[]});
        fs::write(self.root.join(source), v.to_string()).unwrap();
    }
    fn propose(&self, source: &str, ledger: &str) {
        self.ok(&[
            "memory",
            "propose",
            "record-specialist",
            source,
            "--scope",
            "specialist.record-specialist.v1",
            "--expires-at",
            "2100-01-01T00:00:00Z",
            "--ledger",
            ledger,
        ]);
    }
    fn qualify(&self, ledger: &str) {
        self.ok(&["memory", "review", "reviewer", ledger]);
        self.ok(&["memory", "promote", "lead", ledger]);
    }
}

#[test]
fn typed_form_returns_inactive_shape_and_independent_lifecycle_actors() {
    let f = Fixture::new();
    let form = f.ok(&[
        "memory",
        "form",
        "--scope",
        "specialist.record-specialist.v1",
    ]);
    assert_eq!(form["template"]["author"], "record-specialist");
    assert_eq!(form["effect"], "no-change");
    assert!(form["template"]["projectIdentity"]
        .as_str()
        .unwrap()
        .starts_with("root:"));
    f.draft("draft.json", "r1");
    // The form's propose route runs as printed once its slots are filled.
    let propose: Vec<String> = form["lifecycle"]["propose"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| match word.as_str().unwrap() {
            "<SOURCE>" => "draft.json",
            "<NEW_LEDGER>" => ".exitbind/memory/form.jsonl",
            "<RFC3339_EXPIRY>" => "2100-01-01T00:00:00Z",
            other => other,
        })
        .map(str::to_owned)
        .collect();
    let out = support::git_topology::command(&propose[0])
        .current_dir(&f.root)
        .args(&propose[1..])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let proposed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(proposed["nextAction"]["command"][3], "reviewer");
    assert_eq!(f.route("aggregation")["snapshot"]["experience"], json!([]));
    let reviewed = f.ok(&[
        "memory",
        "review",
        "reviewer",
        ".exitbind/memory/form.jsonl",
    ]);
    assert_eq!(reviewed["nextAction"]["command"][3], "lead");
    assert_eq!(f.route("aggregation")["snapshot"]["experience"], json!([]));
}

#[test]
fn generic_scopes_and_content_aliases_cannot_reintroduce_specialist_sources() {
    let f = Fixture::new();
    let mut c = f.config();
    for right in ["memoryRead", "memoryWrite", "memoryReview", "memoryPromote"] {
        let name = match right {
            "memoryReview" => "reviewer",
            "memoryPromote" => "lead",
            _ => "record-specialist",
        };
        c["agents"][name][right] = json!(["specialist.record-specialist.v1", "ordinary"]);
    }
    c["memory"]["protocolScopes"] = json!(["specialist.record-specialist.v1", "ordinary"]);
    f.save(&c);
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/specialist.jsonl");
    f.qualify(".exitbind/memory/specialist.jsonl");
    fs::copy(f.root.join("draft.json"), f.root.join("copy.json")).unwrap();
    f.draft("only-generic.json", "r2");
    fs::write(
        f.root.join("ordinary.txt"),
        "Ordinary taskTerms and procedure words remain useful.",
    )
    .unwrap();
    for (source, ledger) in [
        ("draft.json", "alias"),
        ("copy.json", "copy"),
        ("only-generic.json", "shape"),
        ("ordinary.txt", "ordinary"),
    ] {
        let path = format!(".exitbind/memory/{ledger}.jsonl");
        f.ok(&[
            "memory",
            "propose",
            "record-specialist",
            source,
            "--scope",
            "ordinary",
            "--ledger",
            &path,
        ]);
        f.qualify(&path);
    }
    let selected = f.ok(&[
        "memory",
        "resolve",
        "record-specialist",
        "--task",
        "unrelated formatting",
    ]);
    assert_eq!(selected["references"].as_array().unwrap().len(), 1);
    assert_eq!(selected["references"][0]["sourcePath"], "ordinary.txt");
    f.ok(&[
        "memory",
        "revoke",
        "lead",
        ".exitbind/memory/specialist.jsonl",
    ]);
    let selected = f.ok(&[
        "memory",
        "resolve",
        "record-specialist",
        "--task",
        "aggregation",
    ]);
    assert_eq!(selected["references"].as_array().unwrap().len(), 1);
    assert!(f.ok(&["memory", "inspect", ".exitbind/memory/alias.jsonl"])["items"].is_array());
}

#[test]
fn work_detail_and_native_prompt_use_the_same_current_qualified_version() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/skill.jsonl");
    f.qualify(".exitbind/memory/skill.jsonl");
    let expected = f.route("record aggregation")["snapshot"]["experience"].clone();
    for (task, applicable) in [
        ("record aggregation", true),
        ("unrelated formatting", false),
    ] {
        let work = f.worker(task);
        let detail = f.ok(&["work", "detail", &work]);
        let experience = &detail["recipientContext"]["specialistExperience"]["experience"];
        assert_eq!(
            *experience,
            if applicable {
                expected.clone()
            } else {
                json!([])
            }
        );
        let capture = f.root.join("prompt");
        let fake = f.root.join("fake-codex");
        fs::write(&fake, format!("#!/bin/sh\ncat > '{}'\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"fixture-thread\"}}' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"outcome\\\":\\\"completed\\\",\\\"summary\\\":\\\"fixture captured\\\",\\\"reason\\\":\\\"\\\"}}\"}}}}' '{{\"type\":\"turn.completed\",\"status\":\"completed\",\"usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}'\n", capture.display())).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        f.ok(&["work", "act", &work, "--codex-bin", fake.to_str().unwrap()]);
        let prompt = fs::read_to_string(capture).unwrap();
        assert_eq!(
            prompt.contains("Track presence and sum independently."),
            applicable
        );
        assert!(prompt.contains("CURRENT QUALIFIED SPECIALIST EXPERIENCE"));
    }
}

#[test]
fn specialist_only_child_preparation_binds_and_rechecks_before_hook_presentation() {
    let f = Fixture::new();
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/skill.jsonl");
    f.qualify(".exitbind/memory/skill.jsonl");
    let work = f.worker("record aggregation");
    f.input(&["work", "record", &work], &json!({"action":"init","sourceRef":"fixture:scope","sourceText":"Check records.","requirements":[{"id":"records","text":"Check records."}]}).to_string());
    let token = f.ok(&["work", "continuation", &work])["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    f.ok(&[
        "work",
        "bind",
        &work,
        "--context",
        &token,
        "--host",
        "codex",
        "--session",
        "parent",
        "--host-version",
        "fixture",
    ]);
    let token = f.ok(&["work", "continuation", &work])["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let prepared = f.ok(&[
        "work",
        "child",
        "prepare",
        &work,
        "specialist-fixture",
        "--context",
        &token,
        "--agent-type",
        "record_specialist",
    ]);
    assert_eq!(prepared["selected"]["agent"], "record-specialist");
    let intent = prepared["intent"].as_str().unwrap();
    let context = f.ok(&["work", "child", "context", &work, intent]);
    assert_eq!(
        context["specialistExperience"]["experience"][0]["revision"],
        "r1"
    );
    let hook = f.input(&["hook-run"], &json!({"hook_event_name":"SubagentStart","cwd":f.root,"session_id":"parent","agent_id":"child","agent_type":"record_specialist"}).to_string());
    assert!(hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("Track presence and sum independently."));
    f.ok(&["memory", "revoke", "lead", ".exitbind/memory/skill.jsonl"]);
    assert!(!f
        .call(&["work", "child", "context", &work, intent])
        .status
        .success());
    let hook = f.input(&["hook-run"], &json!({"hook_event_name":"SubagentStart","cwd":f.root,"session_id":"parent","agent_id":"child","agent_type":"record_specialist"}).to_string());
    assert!(!hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("Track presence and sum independently."));
}

#[test]
fn ordinary_recipient_keeps_complete_large_profile_with_specialist_memory() {
    assert_complete_specialist_recipient(Fixture::new());
}

#[test]
fn long_action_paths_keep_complete_large_profile_with_specialist_memory() {
    let label = format!("specialist-routing-{}", "long-path-".repeat(10));
    assert_complete_specialist_recipient(Fixture::with_label(&label));
}

fn assert_complete_specialist_recipient(f: Fixture) {
    let profile = f.config()["agents"]["record-specialist"]["profile"]
        .as_str()
        .unwrap()
        .to_owned();
    let content = "Complete ordinary profile guidance.\n".repeat(500);
    assert!(content.len() > 16 * 1024 && content.len() < 64 * 1024);
    fs::write(f.root.join(profile), &content).unwrap();
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/skill.jsonl");
    f.qualify(".exitbind/memory/skill.jsonl");
    let routed = f.route("record aggregation");
    assert_eq!(routed["snapshot"]["profile"]["complete"], false);
    assert_eq!(routed["snapshot"]["experience"][0]["revision"], "r1");
    let work = f.worker("record aggregation");
    let detail = f.ok(&["work", "detail", &work]);
    assert_eq!(detail["recipientContext"]["profile"]["complete"], false);
    assert_eq!(
        detail["recipientContext"]["specialistExperience"]["experience"][0]["revision"],
        "r1"
    );
    let action_output = f.call(&["work", "detail", &work, "--view", "action", "--json"]);
    assert!(action_output.status.success(), "{action_output:?}");
    let action: Value = serde_json::from_slice(&action_output.stdout).unwrap();
    // Navigation (everything except the delivered guidance block) keeps the
    // compact bound; the guidance block keeps its recipient budget.
    let mut navigation = action.clone();
    let guidance = navigation.as_object_mut().unwrap().remove("guidance");
    let navigation_bytes = serde_json::to_vec(&navigation).unwrap().len() + 1;
    assert!(
        navigation_bytes <= 8 * 1024,
        "compact action navigation exceeded 8192 bytes: {navigation_bytes}"
    );
    if let Some(guidance) = guidance {
        assert!(
            guidance["usedBytes"].as_u64().unwrap() <= guidance["budgetBytes"].as_u64().unwrap(),
            "delivered guidance exceeded its recipient budget: {guidance}"
        );
        // The two parts account for every emitted byte (`,"guidance":`).
        let guidance_bytes = serde_json::to_vec(&guidance).unwrap().len();
        assert_eq!(
            navigation_bytes + ",\"guidance\":".len() + guidance_bytes,
            action_output.stdout.len()
        );
    }
    assert_eq!(action["compact"], true);
    // The bounded action may carry a direct section route or a grouped route.
    // Full next retains the exact current section route when grouped fallback
    // omits the direct recipient route.
    let mut reference = action["details"]["recipient"]["reference"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            assert!(action["details"]["grouped"].is_object(), "{action}");
            let full_next = f.ok(&["work", "next", &work, "--full"]);
            full_next["next"]["current"]["details"]["recipient"]["reference"]
                .as_str()
                .expect("full next must retain the current recipient section")
                .to_owned()
        });
    let mut bytes = Vec::new();
    loop {
        let page = f.ok(&["work", "expand", &work, &reference]);
        bytes.extend(support::page_bytes(&page));
        if page["next"].is_null() {
            break;
        }
        reference = page["next"]["reference"].as_str().unwrap().to_owned();
    }
    let expanded: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(expanded["profile"]["content"], content);
    assert_eq!(expanded["profile"]["complete"], true);
    assert_eq!(
        expanded["specialistExperience"]["experience"][0]["revision"],
        "r1"
    );
}

#[test]
fn actual_work_recipient_excludes_wrong_host_command_guard_and_expired_experience() {
    for case in ["host", "command", "guard", "expiry"] {
        let f = Fixture::new();
        f.draft("draft.json", "r1");
        let mut v: Value =
            serde_json::from_slice(&fs::read(f.root.join("draft.json")).unwrap()).unwrap();
        match case {
            "host" => v["hosts"] = json!(["claude"]),
            "command" => v["requiredCommands"] = json!(["undeclared-command"]),
            "guard" => {
                fs::write(f.root.join("guard.txt"), "one").unwrap();
                v["guards"] = json!([{"path":"guard.txt","sha256":"7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed"}]);
            }
            _ => (),
        }
        fs::write(f.root.join("draft.json"), v.to_string()).unwrap();
        f.ok(&[
            "memory",
            "propose",
            "record-specialist",
            "draft.json",
            "--scope",
            "specialist.record-specialist.v1",
            "--ledger",
            ".exitbind/memory/skill.jsonl",
            "--expires-at",
            if case == "expiry" {
                "2000-01-01T00:00:00Z"
            } else {
                "2100-01-01T00:00:00Z"
            },
        ]);
        f.qualify(".exitbind/memory/skill.jsonl");
        if case == "guard" {
            fs::write(f.root.join("guard.txt"), "two").unwrap();
        }
        let work = f.worker("record aggregation");
        let detail = f.ok(&["work", "detail", &work]);
        assert_eq!(
            detail["recipientContext"]["specialistExperience"]["experience"],
            json!([]),
            "{case}"
        );
        assert!(!serde_json::to_string(&detail["recipientContext"])
            .unwrap()
            .contains("Track presence and sum independently."));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn draft_is_inactive_and_reviewed_owned_skill_is_current_and_bounded() {
    let f = Fixture::new();
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/r1.jsonl");
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    let denied = f.call(&[
        "memory",
        "review",
        "record-specialist",
        ".exitbind/memory/r1.jsonl",
    ]);
    assert!(!denied.status.success());
    f.qualify(".exitbind/memory/r1.jsonl");
    let first = f.route("record aggregation");
    assert_eq!(first["selected"], "record-specialist");
    assert_eq!(first["snapshot"]["experience"][0]["revision"], "r1");
    assert_eq!(first, f.route("record aggregation")); // lost-reply read is idempotent
    assert_eq!(
        f.route("format invoice documentation")["snapshot"]["experience"],
        json!([])
    );
    assert_eq!(
        first["snapshot"]["declaredBoundary"]["commands"],
        json!(["python3"])
    );
    f.ok(&["memory", "revoke", "lead", ".exitbind/memory/r1.jsonl"]);
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    assert!(!f
        .call(&["memory", "promote", "lead", ".exitbind/memory/r1.jsonl"])
        .status
        .success());
    fs::copy(f.root.join("draft.json"), f.root.join("relocated.json")).unwrap();
    f.propose("relocated.json", ".exitbind/memory/reintroduced.jsonl");
    f.qualify(".exitbind/memory/reintroduced.jsonl");
    let revived = f.call(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!revived.status.success());
    assert!(String::from_utf8(revived.stdout)
        .unwrap()
        .contains("revoked specialist source"));
}

#[test]
fn routing_ambiguity_no_match_and_incompatible_host_do_not_expand_rights() {
    let f = Fixture::new();
    let mut c = f.config();
    c["agents"]["other"] = c["agents"]["record-specialist"].clone();
    c["agents"]["other"]["nativeName"] = json!("other_specialist");
    c["workflows"]["change"]["workers"] = json!(["record-specialist", "other"]);
    f.save(&c);
    let ambiguous = f.route("record aggregation");
    assert_eq!(ambiguous["state"], "lead_choice");
    assert!(ambiguous["snapshot"].is_null());
    assert_eq!(
        f.ok(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex",
            "--agent",
            "record-specialist"
        ])["selected"],
        "record-specialist"
    );
    let none = f.ok(&[
        "project",
        "route",
        "--task",
        "paint",
        "--purpose",
        "painting",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert_eq!(none["state"], "lead_held");
    let incompatible = f.ok(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "claude",
    ]);
    assert_eq!(incompatible["state"], "lead_held");
    assert!(!f
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "reviewer",
            "--host",
            "codex",
            "--agent",
            "record-specialist"
        ])
        .status
        .success());
    assert_eq!(f.config(), c);
}

#[test]
fn wrong_recipient_and_foreign_project_never_receive_private_content() {
    let f = Fixture::new();
    let mut c = f.config();
    c["agents"]["other"] = c["agents"]["record-specialist"].clone();
    c["agents"]["other"]["nativeName"] = json!("other_specialist");
    c["agents"]["other"]["memoryRead"] = json!([]);
    c["workflows"]["change"]["workers"] = json!(["record-specialist", "other"]);
    f.save(&c);
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/r1.jsonl");
    f.qualify(".exitbind/memory/r1.jsonl");
    let other = f.ok(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
        "--agent",
        "other",
    ]);
    let encoded = other.to_string();
    assert!(!encoded.contains("presence"));
    assert!(!encoded.contains("draft.json"));
    let foreign = Fixture::new();
    fs::copy(f.root.join("draft.json"), foreign.root.join("draft.json")).unwrap();
    foreign.propose("draft.json", ".exitbind/memory/r1.jsonl");
    foreign.qualify(".exitbind/memory/r1.jsonl");
    assert!(!foreign
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex"
        ])
        .status
        .success());
}

#[test]
fn drift_guard_expiry_and_concurrent_revisions_fail_closed() {
    let f = Fixture::new();
    f.draft("one.json", "r1");
    let mut v: Value = serde_json::from_slice(&fs::read(f.root.join("one.json")).unwrap()).unwrap();
    fs::write(f.root.join("prerequisite.txt"), "one").unwrap();
    v["guards"] = json!([{"path":"prerequisite.txt","sha256":"7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed"}]);
    fs::write(f.root.join("one.json"), v.to_string()).unwrap();
    f.propose("one.json", ".exitbind/memory/one.jsonl");
    f.qualify(".exitbind/memory/one.jsonl");
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::write(f.root.join("prerequisite.txt"), "two").unwrap();
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    f.draft("two.json", "r2");
    f.propose("two.json", ".exitbind/memory/two.jsonl");
    f.qualify(".exitbind/memory/two.jsonl");
    assert!(!f
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex"
        ])
        .status
        .success());
    f.ok(&["memory", "revoke", "lead", ".exitbind/memory/one.jsonl"]);
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"][0]["revision"],
        "r2"
    );
    fs::write(f.root.join("two.json"), "{}").unwrap();
    assert!(!f
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex"
        ])
        .status
        .success());
}

#[test]
fn expired_items_and_symlinked_profile_preserve_host_layout() {
    let f = Fixture::new();
    f.draft("draft.json", "r1");
    f.ok(&[
        "memory",
        "propose",
        "record-specialist",
        "draft.json",
        "--scope",
        "specialist.record-specialist.v1",
        "--expires-at",
        "2000-01-01T00:00:00Z",
        "--ledger",
        ".exitbind/memory/old.jsonl",
    ]);
    f.qualify(".exitbind/memory/old.jsonl");
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    let profile = f.root.join("exitbind/agents/worker.md");
    let bytes = fs::read(&profile).unwrap();
    fs::remove_file(&profile).unwrap();
    fs::write(f.root.join("custom.md"), &bytes).unwrap();
    std::os::unix::fs::symlink(f.root.join("custom.md"), &profile).unwrap();
    let out = f.call(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!out.status.success());
    assert!(fs::symlink_metadata(&profile)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(f.root.join("custom.md")).unwrap(), bytes);
}

#[test]
fn unrelated_scopes_and_inaccessible_metadata_are_not_read_or_disclosed() {
    let f = Fixture::new();
    let mut c = f.config();
    c["agents"]["record-specialist"]["memoryRead"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["agents"]["record-specialist"]["memoryWrite"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["agents"]["reviewer"]["memoryReview"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["agents"]["lead"]["memoryPromote"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["memory"]["protocolScopes"] = json!(["specialist.record-specialist.v1", "private-other"]);
    f.save(&c);
    fs::write(
        f.root.join("private-filename.json"),
        "This is not specialist JSON",
    )
    .unwrap();
    f.ok(&[
        "memory",
        "propose",
        "record-specialist",
        "private-filename.json",
        "--scope",
        "private-other",
        "--ledger",
        ".exitbind/memory/other.jsonl",
    ]);
    f.qualify(".exitbind/memory/other.jsonl");
    // Even authorized but unrelated sources must never be opened by this consumer.
    fs::write(
        f.root.join("private-filename.json"),
        vec![b'x'; 1024 * 1024],
    )
    .unwrap();
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    std::os::unix::fs::symlink(
        "missing",
        f.root.join(".exitbind/memory/private-ledger-name.jsonl"),
    )
    .unwrap();
    let out = f.call(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!out.status.success());
    assert!(!String::from_utf8(out.stdout)
        .unwrap()
        .contains("private-ledger-name"));
    c["agents"]["record-specialist"]["memoryRead"] = json!([]);
    f.save(&c);
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
}

#[test]
fn oversized_own_source_refuses_without_private_name_and_help_names_route() {
    let f = Fixture::new();
    f.draft("private-source.json", "r1");
    f.propose("private-source.json", ".exitbind/memory/own.jsonl");
    f.qualify(".exitbind/memory/own.jsonl");
    fs::write(f.root.join("private-source.json"), vec![b'x'; 1024 * 1024]).unwrap();
    let out = f.call(&[
        "project",
        "route",
        "--task",
        "unrelated record task",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!out.status.success());
    let error = String::from_utf8(out.stdout).unwrap();
    assert!(error.contains("4096"));
    assert!(!error.contains("private-source"));
    let help = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["project", "route", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)
        .unwrap()
        .contains("--role worker|reviewer|adviser"));
}
