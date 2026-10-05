//! Real project selection and combined Work mechanics; role returns are fixtures.
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    config: String,
}
impl Fixture {
    fn new(custom: bool) -> Self {
        let root = support::temp("architecture-adoption");
        support::git_topology::repository(&root);
        let initialized = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(initialized.status.success(), "{initialized:?}");
        fs::create_dir_all(root.join("src/core")).unwrap();
        fs::create_dir_all(root.join("src/other")).unwrap();
        fs::write(root.join("src/core/mod.rs"), "FORBIDDEN\n").unwrap();
        fs::write(root.join("src/other/mod.rs"), "unrelated\n").unwrap();
        fs::write(root.join("architecture.json"), contract().to_string()).unwrap();
        let mut configuration: Value =
            serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
        for role in ["worker", "reviewer"] {
            configuration["agents"][role]["observe"] = json!(["src/core/**"]);
            configuration["agents"][role]["write"] = if role == "worker" {
                json!(["src/core/**"])
            } else {
                json!([])
            };
        }
        configuration["agents"]["worker"]["purpose"] = json!("preserve this custom purpose");
        let config = if custom {
            "custom.json"
        } else {
            "exitbind.json"
        }
        .to_string();
        fs::write(root.join(&config), configuration.to_string()).unwrap();
        Self { root, config }
    }
    fn call(&self, args: &[&str], body: &[u8]) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", &self.config])
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], body: &[u8]) -> Value {
        let result = self.call(args, body);
        assert!(result.status.success(), "{args:?}: {result:?}");
        parse(&result)
    }
    fn preview(&self) -> Value {
        self.ok(
            &[
                "project",
                "architecture",
                "select",
                "architecture.json",
                "--decision",
                "reviewed",
                "--reason",
                "project review selected core boundaries",
                "--json",
            ],
            b"",
        )
    }
    fn execute(&self, command: &Value) -> Output {
        let args = command
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>();
        support::git_topology::command(args[0])
            .args(&args[1..])
            .current_dir(&self.root)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .output()
            .unwrap()
    }
    fn select(&self) -> Value {
        let result = self.execute(&self.preview()["apply"]["command"]);
        assert!(result.status.success(), "{result:?}");
        parse(&result)
    }
    fn next(&self, work: &str) -> Value {
        self.ok(&["work", "next", work, "--full"], b"")["next"].clone()
    }
    fn give(&self, work: &str, next: &Value, outcome: &str) -> Value {
        self.ok(
            &[
                "work",
                "return",
                work,
                next["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            outcome.as_bytes(),
        )
    }
    fn begin(&self, command: &str) -> String {
        let start = self.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "repair the core literal violation",
                "--review-policy",
                "required",
                "--check-command",
                command,
            ],
            b"",
        );
        let work = start["work"].as_str().unwrap().to_owned();
        let worker = self.give(&work, &start["next"], "scoped")["next"].clone();
        let detail = self.ok(&["work", "detail", &work, "--json"], b"");
        let text = detail.to_string();
        assert!(text.contains("reviewed-core-v1"));
        assert!(!text.contains("UNRELATED SUMMARY"));
        self.give(&work, &worker, "completed");
        work
    }
    fn architecture_command(&self) -> String {
        format!(
            "'{}' project architecture check --json --config '{}'",
            env!("CARGO_BIN_EXE_exitbind").replace('\'', "'\\''"),
            self.config.replace('\'', "'\\''")
        )
    }
    fn permit_and_repair(&self, work: &str, worker: &Value) {
        let granted = self.ok(
            &[
                "work",
                "permit",
                work,
                worker["assignment"].as_str().unwrap(),
                "--operation",
                "repair-core",
            ],
            b"",
        );
        assert_eq!(granted["allowed"], true);
        fs::write(self.root.join("src/core/mod.rs"), "APPROVED\n").unwrap();
        self.give(work, worker, "completed");
    }
    fn finish(&self, work: &str) {
        let checked = self.ok(&["work", "check", work], b"");
        assert_eq!(checked["result"]["code"], 0);
        let reviewer = self.next(work);
        assert_eq!(reviewer["role"], "reviewer");
        let detail = self
            .ok(&["work", "detail", work, "--json"], b"")
            .to_string();
        assert!(detail.contains("reviewed-core-v1"));
        assert!(!detail.contains("UNRELATED SUMMARY"));
        let lead = self.give(work, &reviewer, "approved")["next"].clone();
        self.give(work, &lead, "accepted");
        assert_eq!(self.next(work)["status"], "accepted");
        let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
        self.ok(
            &[
                "receipt",
                &ledger,
                "--output",
                ".exitbind/receipts/architecture.json",
                "--json",
            ],
            b"",
        );
        let verified = self.ok(
            &["verify", ".exitbind/receipts/architecture.json", "--json"],
            b"",
        );
        assert_eq!(verified["valid"], true);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained architecture fixture {}", self.root.display());
        } else {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn parse(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"))
}
fn contract() -> Value {
    json!({"version":1,"revision":"reviewed-core-v1", "responsibilities":[
    {"id":"core","summary":"core behavior","paths":["src/core"]},
    {"id":"other","summary":"UNRELATED SUMMARY","paths":["src/other"]}],
    "dependencies":[],"interfaces":[],"checks":[{"id":"core-rule","responsibility":"core","path":"src/core/mod.rs","assertion":"excludes","literal":"FORBIDDEN"}]})
}

#[test]
fn preview_apply_custom_config_and_returned_noop_action_preserve_exact_other_fields() {
    let f = Fixture::new(true);
    let before = fs::read(f.root.join(&f.config)).unwrap();
    let preview = f.preview();
    assert_eq!(preview["state"], "preview");
    assert_eq!(fs::read(f.root.join(&f.config)).unwrap(), before);
    assert!(!f.root.join("checker-executed").exists());
    let applied = f.execute(&preview["apply"]["command"]);
    assert!(applied.status.success(), "{applied:?}");
    let applied = parse(&applied);
    assert_eq!(applied["state"], "applied");
    let mut after: Value =
        serde_json::from_slice(&fs::read(f.root.join(&f.config)).unwrap()).unwrap();
    assert_eq!(
        after["project"]["architectureContract"]["revision"],
        "reviewed-core-v1"
    );
    after["project"]
        .as_object_mut()
        .unwrap()
        .remove("architectureContract");
    assert_eq!(after, serde_json::from_slice::<Value>(&before).unwrap());
    let bytes = fs::read(f.root.join(&f.config)).unwrap();
    let noop = f.execute(&applied["apply"]["command"]);
    assert!(noop.status.success(), "{noop:?}");
    assert_eq!(parse(&noop)["state"], "unchanged");
    assert_eq!(fs::read(f.root.join(&f.config)).unwrap(), bytes);
    assert!(
        !f.execute(&preview["apply"]["command"]).status.success(),
        "consumed preview is stale"
    );
    let inspected = f.ok(&["project", "architecture", "--json"], b"");
    assert_eq!(inspected["provenance"]["revision"], "reviewed-core-v1");
}

#[test]
fn preview_refuses_source_configuration_project_decision_and_unsafe_drift() {
    let f = Fixture::new(false);
    let preview = f.preview();
    let original = fs::read(f.root.join("architecture.json")).unwrap();
    fs::write(
        f.root.join("architecture.json"),
        [original.as_slice(), b"\n"].concat(),
    )
    .unwrap();
    let before = fs::read(f.root.join(&f.config)).unwrap();
    assert!(!f.execute(&preview["apply"]["command"]).status.success());
    assert_eq!(fs::read(f.root.join(&f.config)).unwrap(), before);
    fs::write(f.root.join("architecture.json"), original).unwrap();
    fs::write(f.root.join(&f.config), [before.as_slice(), b"\n"].concat()).unwrap();
    assert!(!f.execute(&preview["apply"]["command"]).status.success());
    let other = Fixture::new(false);
    let other_preview = other.preview();
    let mut command = other_preview["apply"]["command"].clone();
    let args = command.as_array_mut().unwrap();
    let i = args.iter().position(|a| a == "--current-binding").unwrap();
    args[i + 1] = preview["currentBinding"].clone();
    assert!(!other.execute(&command).status.success());
    let mut command = f.preview()["apply"]["command"].clone();
    let args = command.as_array_mut().unwrap();
    let i = args.iter().position(|a| a == "--reason").unwrap();
    args[i + 1] = json!("different project decision");
    assert!(!f.execute(&command).status.success());
    for source in [
        "../architecture.json",
        "/tmp/architecture.json",
        "missing.json",
    ] {
        assert!(!f
            .call(
                &[
                    "project",
                    "architecture",
                    "select",
                    source,
                    "--decision",
                    "reviewed",
                    "--reason",
                    "reviewed"
                ],
                b""
            )
            .status
            .success());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            f.root.join("architecture.json"),
            f.root.join("symlink.json"),
        )
        .unwrap();
        assert!(!f
            .call(
                &[
                    "project",
                    "architecture",
                    "select",
                    "symlink.json",
                    "--decision",
                    "reviewed",
                    "--reason",
                    "reviewed"
                ],
                b""
            )
            .status
            .success());
    }
    let mut invalid = contract();
    invalid["command"] = json!("touch checker-executed");
    fs::write(f.root.join("architecture.json"), invalid.to_string()).unwrap();
    assert!(!f
        .call(
            &[
                "project",
                "architecture",
                "select",
                "architecture.json",
                "--decision",
                "reviewed",
                "--reason",
                "reviewed"
            ],
            b""
        )
        .status
        .success());
    assert!(!f.root.join("checker-executed").exists());
}

#[test]
fn simultaneous_selection_applications_preserve_one_exact_configuration() {
    let f = Fixture::new(false);
    let command = f.preview()["apply"]["command"].clone();
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| f.execute(&command));
        let second = scope.spawn(|| f.execute(&command));
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(
        usize::from(first.status.success()) + usize::from(second.status.success()),
        1,
        "{first:?}; {second:?}"
    );
    let current = f.ok(&["project", "architecture", "--json"], b"");
    assert_eq!(current["provenance"]["revision"], "reviewed-core-v1");
    assert_eq!(
        f.execute(&f.preview()["apply"]["command"]).status.code(),
        Some(0)
    );
}

#[test]
fn ordinary_architecture_violation_repair_observed_check_review_and_verified_receipt() {
    let f = Fixture::new(false);
    f.select();
    let work = f.begin(&format!("sh -c 'exit 0' && {}", f.architecture_command()));
    let failed = f.call(&["work", "check", &work], b"");
    assert!(!failed.status.success());
    assert_eq!(parse(&failed)["result"]["code"], 1);
    let lead = f.next(&work);
    let worker = f.give(&work, &lead, "rework")["next"].clone();
    f.permit_and_repair(&work, &worker);
    f.finish(&work);
}

#[test]
fn selected_contract_observation_failure_recovers_through_same_work_and_required_review() {
    let f = Fixture::new(false);
    f.select();
    fs::write(
        f.root.join(".exitbind/inject-timeout"),
        b"fixture injection",
    )
    .unwrap();
    let work = f.begin(&format!("if [ -f .exitbind/inject-timeout ]; then printf incomplete; exec sleep 3; else exec {}; fi", f.architecture_command()));
    let failed = f.call(&["work", "check", &work, "--timeout-ms", "50"], b"");
    assert!(!failed.status.success());
    let facts = parse(&failed);
    assert_eq!(facts["checkRecorded"], false);
    assert_eq!(facts["observationFailure"]["facts"]["termination"], "ended");
    let lead = f.next(&work);
    let worker = f.ok(
        &[
            "work",
            "disposition",
            &work,
            lead["assignment"].as_str().unwrap(),
            "--decision",
            "repair",
            "--reason",
            "bounded observer failure in selected task",
            "--repair-boundary",
            "src/core only",
            "--regression",
            "current literal check must pass",
        ],
        b"",
    )["next"]
        .clone();
    // Fixture control clears its injected failure; permitted worker repairs the product file.
    fs::remove_file(f.root.join(".exitbind/inject-timeout")).unwrap();
    f.permit_and_repair(&work, &worker);
    f.finish(&work);
}

#[test]
fn authorized_compound_check_preserves_earlier_failing_exit_and_launches_no_later_check() {
    let f = Fixture::new(false);
    f.select();
    fs::write(f.root.join("src/core/mod.rs"), "APPROVED").unwrap();
    let work = f.begin(&format!(
        "sh -c 'exit 7' && {} && printf later > .exitbind/later-check",
        f.architecture_command()
    ));
    let result = f.call(&["work", "check", &work], b"");
    assert!(!result.status.success());
    assert_eq!(parse(&result)["result"]["code"], 7);
    assert!(!f.root.join(".exitbind/later-check").exists());
}

#[test]
fn applied_new_selection_cannot_be_silently_adopted_by_active_frozen_work() {
    let f = Fixture::new(false);
    f.select();
    let work = f.begin(&format!(
        "printf launched > .exitbind/check-launched; {}",
        f.architecture_command()
    ));
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let original = fs::read(f.root.join(&ledger)).unwrap();
    let events = String::from_utf8(original.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let target = events.last().unwrap()["eventSha256"].as_str().unwrap();
    let mut revised = contract();
    revised["revision"] = json!("reviewed-core-v2");
    fs::write(f.root.join("architecture.json"), revised.to_string()).unwrap();
    assert_eq!(f.select()["state"], "applied");
    let detail = f.ok(&["work", "detail", &work, "--json"], b"");
    assert_eq!(detail["effectiveAction"]["readiness"], "BLOCKED");
    assert!(detail["effectiveAction"]["warnings"]
        .to_string()
        .contains("config_drift"));
    for args in [
        vec!["work", "check", &work],
        vec![
            "run",
            "observe-check",
            &ledger,
            "--target",
            target,
            "--json",
        ],
    ] {
        let refused = f.call(&args, b"");
        assert!(!refused.status.success(), "{refused:?}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("frozen current configuration")
                || String::from_utf8_lossy(&refused.stdout)
                    .contains("frozen current configuration")
        );
        assert_eq!(fs::read(f.root.join(&ledger)).unwrap(), original);
        assert!(!f.root.join(".exitbind/check-launched").exists());
    }
}

#[test]
fn marked_work_and_direct_observation_refuse_changed_selected_source_before_effects() {
    let f = Fixture::new(false);
    f.select();
    let work = f.begin(&format!(
        "printf launched > .exitbind/check-launched; {}",
        f.architecture_command()
    ));
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let original = fs::read(f.root.join(&ledger)).unwrap();
    let events = String::from_utf8(original.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let target = events.last().unwrap()["eventSha256"].as_str().unwrap();
    let mut changed = contract();
    changed["revision"] = json!("unselected-new-revision");
    fs::write(f.root.join("architecture.json"), changed.to_string()).unwrap();
    for args in [
        vec!["work", "check", &work],
        vec![
            "run",
            "observe-check",
            &ledger,
            "--target",
            target,
            "--json",
        ],
    ] {
        let refused = f.call(&args, b"");
        assert!(!refused.status.success(), "{refused:?}");
        assert_eq!(fs::read(f.root.join(&ledger)).unwrap(), original);
        assert!(!f.root.join(".exitbind/check-launched").exists());
    }
}

#[test]
fn selected_source_changed_during_observation_records_failure_without_check_credit() {
    let f = Fixture::new(false);
    f.select();
    let work = f.begin("printf changed > architecture.json; printf observed");
    let refused = f.call(&["work", "check", &work], b"");
    assert!(!refused.status.success(), "{refused:?}");
    let failure = parse(&refused);
    assert_eq!(failure["checkRecorded"], false);
    assert_eq!(failure["observationFailure"]["facts"]["process"]["code"], 0);
    assert_eq!(
        failure["observationFailure"]["facts"]["storageStage"],
        "commit"
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let events = fs::read_to_string(f.root.join(ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.iter().filter(|e| e["action"] == "check").count(), 0);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["action"] == "check_observation_failed")
            .count(),
        1
    );
    // Canonical reduction independently rejects a rehashed admission that
    // proposes another configuration, even without the runtime preflight.
    let index = events
        .iter()
        .position(|e| e["action"] == "check_observation")
        .unwrap();
    let mut proposal = events[index].clone();
    proposal["observation"]["binding"]["configSha256"] = json!("0".repeat(64));
    let object = proposal.as_object_mut().unwrap();
    object.remove("eventSha256");
    use sha2::{Digest, Sha256};
    proposal["eventSha256"] = json!(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&proposal).unwrap())
    ));
    let mut prefix = events[..index].to_vec();
    prefix.push(proposal);
    let proposed_ledger = ".exitbind/runs/mismatched-config.jsonl";
    fs::write(
        f.root.join(proposed_ledger),
        prefix
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let invalid = f.call(&["run", "inspect", proposed_ledger, "--json"], b"");
    assert!(!invalid.status.success(), "{invalid:?}");
    assert!(parse(&invalid)["error"]
        .as_str()
        .unwrap()
        .contains("frozen configuration"));
}

#[test]
fn committed_observation_replay_refuses_changed_selected_source_without_execution() {
    let f = Fixture::new(false);
    f.select();
    fs::write(f.root.join("src/core/mod.rs"), "APPROVED").unwrap();
    let work = f.begin(&format!(
        "printf launched >> .exitbind/check-count; {}",
        f.architecture_command()
    ));
    f.ok(&["work", "check", &work], b"");
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let original = fs::read(f.root.join(&ledger)).unwrap();
    let check: Value = serde_json::from_str(
        String::from_utf8(original.clone())
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    let target = check["targetEventSha256"].as_str().unwrap();
    fs::write(f.root.join("architecture.json"), "changed selected source").unwrap();
    for args in [
        vec!["work", "check", &work],
        vec![
            "run",
            "observe-check",
            &ledger,
            "--target",
            target,
            "--json",
        ],
    ] {
        let refused = f.call(&args, b"");
        assert!(!refused.status.success(), "{refused:?}");
        assert_eq!(fs::read(f.root.join(&ledger)).unwrap(), original);
        assert_eq!(
            fs::read(f.root.join(".exitbind/check-count")).unwrap(),
            b"launched"
        );
    }
}
