//! Action forms expose only the current Lead outcomes and executable, bound
//! commands.  These tests execute the descriptors instead of duplicating the
//! CLI spelling in the assertions.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        support::git_topology::repository(&root);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn json(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn begin(&self, check: &str, review: bool) -> (String, Value) {
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
        if review {
            args.extend(["--review-policy", "required"]);
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
        let mut argv = choice["command"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
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

    fn run_argv(&self, argv: &[String], input: &[u8]) -> Output {
        let mut child = support::git_topology::command(&argv[0])
            .current_dir(&self.root)
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
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
    let mut permit = before["effectiveAction"]["beforeEditing"]["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let i = permit.iter().position(|s| s == "<OPERATION>").unwrap();
    permit[i] = "edit source".into();
    let out = fixture.run_argv(&permit, b"");
    assert!(out.status.success(), "{}", text(&out));
    let permit: Value = serde_json::from_slice(&out.stdout).unwrap();
    let suffix = permit["nextAction"]["commandSuffix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect::<Vec<_>>();
    let output = fixture.call(&suffix, b"");
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
    let argv = old_return["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(!fixture.run_argv(&argv, b"result\n").status.success());
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), recorded);
    assert!(current["details"]["assignment"].is_object());
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
    let fixture = Fixture::new("action-form-binding");
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
        ["rework", "blocked"]
    );
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
