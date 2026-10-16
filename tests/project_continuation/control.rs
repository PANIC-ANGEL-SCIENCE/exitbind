//! Real CLI boundaries with deterministic roles/providers; no useful native or
//! independent semantic-review claims are made by these fixtures.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn project(label: &str) -> Project {
    let mut project = Project::new(label, "export");
    let path = project.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for agent in config["agents"].as_object_mut().unwrap().values_mut() {
        agent["write"] = json!(["src/**"]);
    }
    fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    project.spec["mandate"]["maxActions"] = json!(64);
    project.save_spec();
    project
}
fn ledger(work: &str) -> String {
    format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    )
}
fn next(p: &Project, w: &str) -> Value {
    p.value(&["work", "next", w, "--full"])["next"].clone()
}
fn choice(p: &Project, w: &str, label: &str) -> Value {
    let d = p.value(&["work", "detail", w, "--view", "action"]);
    let f = d.get("actionForms").unwrap_or(&d["effectiveAction"]);
    f["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == label)
        .unwrap_or_else(|| panic!("missing {label}: {d}"))
        .clone()
}
fn returned(p: &Project, w: &str, label: &str) -> Value {
    p.execute_form(
        &choice(p, w, label),
        Some("Deterministic runtime boundary fixture.\n"),
    )
}
fn worker(p: &Project, w: &str) -> String {
    returned(p, w, "scoped");
    assert_eq!(next(p, w)["role"], "worker");
    next(p, w)["assignment"].as_str().unwrap().into()
}
fn permit(p: &Project, w: &str, a: &str) {
    assert_eq!(
        p.value(&[
            "work",
            "permit",
            w,
            a,
            "--operation",
            "Prepare bounded fixture result"
        ])["allowed"],
        true
    );
}
fn check_stage(p: &Project, w: &str) {
    let a = worker(p, w);
    permit(p, w, &a);
    returned(p, w, "completed");
    assert_eq!(next(p, w)["action"], "check");
}
fn review_stage(p: &Project, w: &str) {
    check_stage(p, w);
    for _ in 0..4 {
        if next(p, w)["action"] != "check" {
            break;
        }
        p.value(&["work", "check", w]);
    }
    assert_eq!(next(p, w)["role"], "reviewer");
}
fn stop(p: &Project) {
    p.value(&[
        "project",
        "advance",
        "stop",
        "advance-records",
        "--reason",
        "Owner stopped this finite fixture.",
    ]);
}
fn refused(out: Output, why: &str) {
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        message(&out).to_ascii_lowercase().contains(why),
        "expected {why}: {out:?}"
    );
}
fn provider(p: &Project, body: &str) -> String {
    let path = p.root.join(".planning/native.sh");
    fs::write(
        &path,
        format!("#!/bin/sh\ncat >/dev/null\nprintf x >> .planning/launches\n{body}\n"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.to_str().unwrap().into()
}
fn complete_provider(p: &Project) -> String {
    provider(
        p,
        r#"printf '%s\n' '{"type":"thread.started","thread_id":"runtime-fixture"}' '{"type":"item.completed","item":{"type":"agent_message","text":"{\"outcome\":\"completed\",\"summary\":\"bounded fixture result\",\"reason\":\"\"}"}}' '{"type":"turn.completed","status":"completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'"#,
    )
}
fn account(p: &Project) -> Value {
    let paths = fs::read_dir(p.root.join(".exitbind/artifacts/project-continuation"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1);
    let raw = fs::read_to_string(&paths[0]).unwrap();
    let record: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
    assert_eq!(record["version"], 2);
    record["account"].clone()
}

#[test]
fn stop_source_change_and_expiry_refuse_fresh_worker_effects_and_native_launch() {
    for hold in ["stop", "source", "expired"] {
        let mut p = project("continuation-controlled-worker");
        let expiry = chrono::Utc::now() + chrono::Duration::seconds(20);
        if hold == "expired" {
            p.spec["mandate"]["expiresAt"] = json!(expiry.to_rfc3339());
            p.save_spec();
        }
        p.select();
        let w = p.admit()["work"].as_str().unwrap().to_owned();
        let a = worker(&p, &w);
        permit(&p, &w, &a);
        let fake = complete_provider(&p);
        match hold {
            "stop" => stop(&p),
            "source" => fs::write(p.root.join("docs/brief.md"), "Changed direction.\n").unwrap(),
            _ => {
                while chrono::Utc::now() <= expiry {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        let original = fs::read(p.work_path(&w)).unwrap();
        refused(
            p.call(&[
                "work",
                "permit",
                &w,
                &a,
                "--operation",
                "New mutation after hold",
            ]),
            hold,
        );
        refused(
            p.call_input(
                &[
                    "work",
                    "write",
                    &w,
                    &a,
                    "src/refused.txt",
                    "--operation",
                    "held-effect",
                    "--expected-sha256",
                    "absent",
                ],
                b"must not appear\n",
            ),
            hold,
        );
        refused(p.call(&["work", "act", &w, "--codex-bin", &fake]), hold);
        assert!(!p.root.join("src/refused.txt").exists());
        assert!(!p.root.join(".planning/launches").exists());
        assert_eq!(fs::read(p.work_path(&w)).unwrap(), original);
        assert_eq!(
            next(&p, &w)["assignment"],
            a,
            "read-only recovery remains available"
        );
    }
}

#[test]
fn stop_refuses_checks_pre_return_review_and_final_acceptance_without_credit() {
    for phase in ["check", "pre-return", "review", "accept"] {
        let mut p = project("continuation-controlled-return");
        if phase == "pre-return" {
            p.plan["preReturn"] =
                json!({"command":"sh checks.sh","timeoutMs":1000,"captureLimitBytes":4096});
            p.save_plan();
        }
        p.select();
        let w = p.admit()["work"].as_str().unwrap().to_owned();
        match phase {
            "check" => check_stage(&p, &w),
            "review" => review_stage(&p, &w),
            "accept" => {
                review_stage(&p, &w);
                returned(&p, &w, "approved");
            }
            _ => {
                let a = worker(&p, &w);
                permit(&p, &w, &a);
            }
        }
        let form = if phase == "check" {
            None
        } else {
            Some(choice(
                &p,
                &w,
                match phase {
                    "review" => "approved",
                    "accept" => "accepted",
                    _ => "completed",
                },
            ))
        };
        stop(&p);
        let before = fs::read(p.work_path(&w)).unwrap();
        let output = match form {
            Some(f) => p.form_output(
                &f,
                Some("A hold cannot earn result or acceptance credit.\n"),
            ),
            None => p.call(&["work", "check", &w]),
        };
        refused(output, "stop");
        assert_eq!(
            fs::read(p.work_path(&w)).unwrap(),
            before,
            "{phase} must not record completion"
        );
        if phase == "pre-return" {
            assert!(!p.root.join(".exitbind/pre-return").exists());
        }
        assert_ne!(
            p.value(&["run", "status", ledger(&w).as_str()])["status"],
            "accepted"
        );
    }
}

#[test]
fn associated_review_cannot_be_waived_and_saved_effect_replay_has_no_new_debit() {
    let p = project("continuation-controlled-effect-replay");
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    let a = worker(&p, &w);
    permit(&p, &w, &a);
    refused(
        p.call(&[
            "run",
            "review-policy",
            "lead",
            ledger(&w).as_str(),
            "--decision",
            "omitted",
            "--reason",
            "Attempt to waive independent review",
        ]),
        "review",
    );
    let args = [
        "work",
        "write",
        &w,
        &a,
        "src/saved.txt",
        "--operation",
        "saved-effect",
        "--expected-sha256",
        "absent",
    ];
    assert_eq!(
        successful(p.call_input(&args, b"saved exact bytes\n"))["effect"],
        "file-replaced"
    );
    stop(&p);
    let before = account(&p);
    let replay = successful(p.call_input(&args, b"saved exact bytes\n"));
    assert_eq!(replay["replay"], true);
    assert_eq!(replay["effect"], "no-change");
    assert_eq!(account(&p), before);
    fs::write(p.root.join("src/saved.txt"), "changed independently\n").unwrap();
    assert!(!p.call_input(&args, b"saved exact bytes\n").status.success());
    assert_eq!(
        fs::read(p.root.join("src/saved.txt")).unwrap(),
        b"changed independently\n"
    );
}

#[test]
fn completed_native_result_replays_under_stop_without_another_provider_or_debit() {
    let p = project("continuation-controlled-native-replay");
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    let a = worker(&p, &w);
    permit(&p, &w, &a);
    let fake = complete_provider(&p);
    assert_eq!(
        p.value(&["work", "act", &w, "--codex-bin", &fake])["effect"],
        "recorded"
    );
    assert_eq!(fs::read(p.root.join(".planning/launches")).unwrap(), b"x");
    stop(&p);
    let before = account(&p);
    let original = fs::read(p.work_path(&w)).unwrap();
    let replay = p.value(&["work", "act", &w, "--resume", "--operation", &a]);
    assert!(
        replay.to_string().contains("replay") || replay["effect"] == "no-change",
        "{replay}"
    );
    assert_eq!(fs::read(p.root.join(".planning/launches")).unwrap(), b"x");
    assert_eq!(fs::read(p.work_path(&w)).unwrap(), original);
    assert_eq!(account(&p), before);
}

#[test]
fn unknown_native_execution_is_inspectable_and_never_blindly_relaunched() {
    let p = project("continuation-controlled-native-unknown");
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    let a = worker(&p, &w);
    permit(&p, &w, &a);
    let fake=provider(&p,"printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"unresolved-fixture\"}'\nsleep 4 &\nexit 1");
    let first = p.call(&["work", "act", &w, "--codex-bin", &fake]);
    assert!(!first.status.success(), "{first:?}");
    assert_eq!(fs::read(p.root.join(".planning/launches")).unwrap(), b"x");
    assert_ne!(
        p.value(&["work", "act", &w, "--inspect"])["reconciliation"],
        "replay_ready"
    );
    let retry = p.call(&["work", "act", &w, "--resume"]);
    assert!(
        !retry.status.success(),
        "unknown operation may not launch: {retry:?}"
    );
    assert_eq!(fs::read(p.root.join(".planning/launches")).unwrap(), b"x");
    std::thread::sleep(std::time::Duration::from_millis(2200));
}

#[test]
fn stopped_mandate_does_not_govern_an_explicitly_unassociated_work() {
    let p = project("continuation-controlled-ordinary");
    p.select();
    p.admit();
    stop(&p);
    let before = account(&p);
    let start = p.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "Unrelated ordinary fixture",
        "--check-command",
        "sh checks.sh",
        "--review-policy",
        "required",
    ]);
    let w = start["work"].as_str().unwrap();
    let raw = fs::read_to_string(p.work_path(w)).unwrap();
    let event: Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
    assert!(event.get("projectContinuation").is_none());
    p.accept_fixture_work(w);
    assert_eq!(
        p.value(&["run", "status", ledger(w).as_str()])["status"],
        "accepted"
    );
    assert_eq!(account(&p), before);
}

#[test]
fn actual_action_budget_exhaustion_survives_new_request_ids_and_policy_revision() {
    let mut p = project("continuation-controlled-actual-budget");
    p.spec["mandate"]["maxActions"] = json!(2);
    p.plan["estimatedActions"] = json!(1);
    p.save_spec();
    p.save_plan();
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    assert_eq!(account(&p)["operations"].as_array().unwrap().len(), 1);
    let assignment = worker(&p, &w);
    let before = account(&p);
    assert_eq!(before["operations"].as_array().unwrap().len(), 2);
    for request in ["first-budget-request", "renamed-budget-request"] {
        refused(
            p.call(&[
                "work",
                "permit",
                &w,
                &assignment,
                "--operation",
                "Fresh bounded mutation",
                "--request-id",
                request,
            ]),
            "budget",
        );
        assert_eq!(account(&p), before);
    }
    p.spec["mandate"]["revision"] = json!(2);
    p.save_spec();
    p.select();
    assert_eq!(p.status()["budget"]["actualActions"], 2);
    p.plan["taskId"] = json!("new-plan-cannot-reset-spent-budget");
    p.plan["id"] = json!("renamed-plan");
    p.save_plan();
    refused(
        p.call(&["project", "advance", "admit", SPEC, "--proposal", PLAN]),
        "budget",
    );
    assert_eq!(account(&p), before);
    assert_eq!(p.runs().len(), 1);
}

fn start(p: &Project, w: &str) -> Value {
    let raw = fs::read_to_string(p.work_path(w)).unwrap();
    serde_json::from_str(raw.lines().next().unwrap()).unwrap()
}
fn supersede(p: &Project, w: &str, new_ledger: &str, command: &str) -> Output {
    let old = start(p, w);
    let basis = serde_json::to_string(&old["basis"]).unwrap();
    let preservation = format!("project-preservation:{PRESERVE}");
    p.call(&[
        "run",
        "supersede",
        &ledger(w),
        "--workflow",
        "change",
        "--goal",
        old["goal"].as_str().unwrap(),
        "--ledger",
        new_ledger,
        "--check-command",
        command,
        "--preserve-requirement",
        &preservation,
        "--preservation-check-command",
        "sh checks.sh",
        "--basis",
        &basis,
        "--review-policy",
        "required",
    ])
}

#[test]
fn successor_preserves_association_and_spent_authority_with_one_parallel_work() {
    let mut p = project("continuation-controlled-successor");
    p.spec["mandate"]["maxParallel"] = json!(1);
    p.save_spec();
    p.select();
    let old = p.admit()["work"].as_str().unwrap().to_owned();
    let original = start(&p, &old);
    let before = account(&p);
    let destination = format!(".exitbind/runs/work-{}.jsonl", "a".repeat(64));
    successful(supersede(&p, &old, &destination, "sh checks.sh"));
    let new = format!("smw_{}", "a".repeat(64));
    let replacement = start(&p, &new);
    for field in [
        "projectContinuation",
        "goal",
        "plan",
        "checkPolicy",
        "preservation",
        "preReturnPolicy",
    ] {
        assert_eq!(replacement[field], original[field], "{field}");
    }
    let after = account(&p);
    let prior = before["operations"].as_array().unwrap();
    let operations = after["operations"].as_array().unwrap();
    assert!(operations.starts_with(prior));
    assert_eq!(operations.len(), prior.len() + 1);
    assert_eq!(operations.last().unwrap()["intent"]["predecessor"], old);
    let a = worker(&p, &new);
    permit(&p, &new, &a);
    refused(
        p.call(&[
            "work",
            "act",
            &old,
            "--outcome",
            "scoped",
            "--reason",
            "Old Work cannot regain authority",
        ]),
        "superseded",
    );
}

#[test]
fn successor_cannot_refreeze_a_broader_checker_or_drop_required_review() {
    for change in ["checker", "review"] {
        let p = project("continuation-controlled-successor-expansion");
        p.select();
        let old = p.admit()["work"].as_str().unwrap().to_owned();
        let original = fs::read(p.work_path(&old)).unwrap();
        let destination = format!(".exitbind/runs/work-{}.jsonl", "b".repeat(64));
        let output = if change == "checker" {
            supersede(&p, &old, &destination, "sh checks.sh; publish --external")
        } else {
            let original = start(&p, &old);
            let basis = serde_json::to_string(&original["basis"]).unwrap();
            let preservation = format!("project-preservation:{PRESERVE}");
            p.call(&[
                "run",
                "supersede",
                &ledger(&old),
                "--workflow",
                "change",
                "--goal",
                original["goal"].as_str().unwrap(),
                "--ledger",
                &destination,
                "--check-command",
                "sh checks.sh",
                "--preserve-requirement",
                &preservation,
                "--preservation-check-command",
                "sh checks.sh",
                "--basis",
                &basis,
                "--review-policy",
                "omitted",
            ])
        };
        assert!(
            !output.status.success(),
            "{change} expansion unexpectedly succeeded: {output:?}"
        );
        assert!(!p.root.join(&destination).exists());
        assert_eq!(fs::read(p.work_path(&old)).unwrap(), original);
    }
}

#[test]
fn stop_during_silent_native_wait_interrupts_before_a_late_effect() {
    let p = project("continuation-controlled-silent-stop");
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    let a = worker(&p, &w);
    permit(&p, &w, &a);
    let fake = provider(
        &p,
        "printf started > .planning/running\nsleep 8\nprintf late > src/late-effect.txt",
    );
    let mut child = p
        .command()
        .args([
            "work",
            "act",
            &w,
            "--codex-bin",
            &fake,
            "--timeout-ms",
            "20000",
            "--config",
            "exitbind.json",
            "--json",
        ])
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !p.root.join(".planning/running").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let actually_started = p.root.join(".planning/running").exists();
    stop(&p);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(7);
    let mut bounded = true;
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            bounded = false;
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        actually_started,
        "native fixture did not reach actual launch: {output:?}"
    );
    assert!(
        bounded,
        "owned native cleanup exceeded its bounded wait: {output:?}"
    );
    assert!(!output.status.success(), "{output:?}");
    assert!(!p.root.join("src/late-effect.txt").exists());
    let directory = p.root.join(".exitbind/native-actions").join(&w);
    let entries = fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "json")
                && !p
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .ends_with(".schema.json")
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    let journal: Value = serde_json::from_slice(&fs::read(&entries[0]).unwrap()).unwrap();
    let facts = &journal["controlledInterruption"];
    assert_eq!(facts["spawned"], true);
    assert_eq!(facts["terminationRequested"], true);
    assert!(facts["actualProcessOutcome"].is_object() || facts["uncertainty"].is_array());
}

#[test]
fn committed_start_recovers_its_spent_admission_without_restart_even_after_stop() {
    let p = project("continuation-saved-start-accounting");
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    let journal = fs::read_dir(p.root.join(".exitbind/artifacts/project-continuation"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let raw = fs::read_to_string(&journal).unwrap();
    let lines: Vec<_> = raw.lines().collect();
    let previous: Value = serde_json::from_str(lines[lines.len() - 2]).unwrap();
    assert_eq!(previous["account"]["operations"][0]["outcome"], "reserved");
    // Controlled crash-window image: retain the valid admitted account prefix
    // while the actual committed Work start remains untouched.
    fs::write(
        &journal,
        format!("{}\n", lines[..lines.len() - 1].join("\n")),
    )
    .unwrap();
    let work_bytes = fs::read(p.work_path(&w)).unwrap();
    stop(&p);
    let before = account(&p);
    let state = p.state();
    let preview = p.value(&[
        "project",
        "advance",
        "recover",
        "advance-records",
        "--task",
        "preserve-records",
    ]);
    assert_eq!(preview["readOnly"], true);
    assert_eq!(p.state(), state);
    assert_eq!(preview["outcome"], "reserved");
    let observed = p.apply(&preview);
    assert_eq!(observed["effect"], "saved_start_observed");
    let after = account(&p);
    assert_eq!(
        after["operations"].as_array().unwrap().len(),
        before["operations"].as_array().unwrap().len()
    );
    assert_eq!(after["operations"][0]["outcome"], "succeeded");
    assert_eq!(fs::read(p.work_path(&w)).unwrap(), work_bytes);
    let preview = p.value(&[
        "project",
        "advance",
        "recover",
        "advance-records",
        "--task",
        "preserve-records",
    ]);
    let state = p.state();
    p.apply(&preview);
    assert_eq!(p.state(), state);
}

fn lose_last_account_observation(p: &Project, kind: &str) {
    let journal = fs::read_dir(p.root.join(".exitbind/artifacts/project-continuation"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let raw = fs::read_to_string(&journal).unwrap();
    let lines: Vec<_> = raw.lines().collect();
    let previous: Value = serde_json::from_str(lines[lines.len() - 2]).unwrap();
    let operations = previous["account"]["operations"].as_array().unwrap();
    assert_eq!(operations.last().unwrap()["intent"]["kind"], kind);
    assert_eq!(operations.last().unwrap()["outcome"], "reserved");
    // A valid crash-window prefix, with the actual committed owner untouched.
    fs::write(
        journal,
        format!("{}\n", lines[..lines.len() - 1].join("\n")),
    )
    .unwrap();
}

#[test]
fn committed_grant_replay_repairs_only_its_existing_debit_after_stop() {
    let p = project("continuation-grant-account-recovery");
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    let a = worker(&p, &w);
    let args = [
        "work",
        "permit",
        &w,
        &a,
        "--operation",
        "One bounded fixture effect",
        "--request-id",
        "stable-grant",
    ];
    p.value(&args);
    lose_last_account_observation(&p, "mutation");
    stop(&p);
    let bytes = fs::read(p.work_path(&w)).unwrap();
    let count = account(&p)["operations"].as_array().unwrap().len();
    assert_eq!(p.value(&args)["idempotent"], true);
    let after = account(&p);
    assert_eq!(after["operations"].as_array().unwrap().len(), count);
    assert_eq!(
        after["operations"].as_array().unwrap().last().unwrap()["outcome"],
        "succeeded"
    );
    assert_eq!(fs::read(p.work_path(&w)).unwrap(), bytes);
}

#[test]
fn work_check_saved_readback_repairs_only_its_existing_debit_after_stop() {
    let p = project("continuation-check-account-recovery");
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    review_stage(&p, &w);
    lose_last_account_observation(&p, "check");
    stop(&p);
    let bytes = fs::read(p.work_path(&w)).unwrap();
    let count = account(&p)["operations"].as_array().unwrap().len();
    p.value(&["work", "check", &w]);
    let after = account(&p);
    assert_eq!(after["operations"].as_array().unwrap().len(), count);
    assert_eq!(
        after["operations"].as_array().unwrap().last().unwrap()["outcome"],
        "succeeded"
    );
    assert_eq!(fs::read(p.work_path(&w)).unwrap(), bytes);
}

#[test]
fn committed_review_lead_and_policy_decisions_recover_without_new_events() {
    for phase in ["review", "lead", "policy"] {
        let p = project("continuation-decision-account-recovery");
        p.select();
        let w = p.admit()["work"].as_str().unwrap().to_owned();
        if phase == "policy" {
            p.value(&[
                "run",
                "review-policy",
                "lead",
                ledger(&w).as_str(),
                "--decision",
                "required",
                "--reason",
                "Keep the selected independent review",
            ]);
        } else {
            review_stage(&p, &w);
            returned(&p, &w, "approved");
            if phase == "lead" {
                returned(&p, &w, "accepted");
            }
        }
        lose_last_account_observation(&p, "effect");
        stop(&p);
        let bytes = fs::read(p.work_path(&w)).unwrap();
        let account_before = account(&p);
        let state = p.state();
        let preview = p.value(&[
            "project",
            "advance",
            "recover",
            "advance-records",
            "--task",
            "preserve-records",
        ]);
        assert_eq!(p.state(), state, "preview stays read-only");
        assert_eq!(
            preview["committedDecisionRecovery"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{phase}: {preview}"
        );
        assert_eq!(p.apply(&preview)["decisionsObserved"], 1);
        let after = account(&p);
        assert_eq!(
            after["operations"].as_array().unwrap().len(),
            account_before["operations"].as_array().unwrap().len()
        );
        assert_eq!(
            after["operations"].as_array().unwrap().last().unwrap()["outcome"],
            "succeeded"
        );
        assert_eq!(
            fs::read(p.work_path(&w)).unwrap(),
            bytes,
            "{phase} added no event"
        );
    }
}

#[test]
fn explicit_saved_check_recovery_preserves_a_known_nonzero_failure() {
    let p = project("continuation-failed-check-account-recovery");
    fs::write(p.root.join("checks.sh"), "exit 7\n").unwrap();
    p.select();
    let w = p.admit()["work"].as_str().unwrap().to_owned();
    check_stage(&p, &w);
    let _ = p.call(&["work", "check", &w]);
    let events = fs::read_to_string(p.work_path(&w)).unwrap();
    let check: Value = serde_json::from_str(events.lines().last().unwrap()).unwrap();
    assert_eq!(check["action"], "check");
    assert_eq!(check["result"]["code"], 7);
    lose_last_account_observation(&p, "check");
    stop(&p);
    let bytes = fs::read(p.work_path(&w)).unwrap();
    let preview = p.value(&[
        "project",
        "advance",
        "recover",
        "advance-records",
        "--task",
        "preserve-records",
    ]);
    assert_eq!(
        preview["committedDecisionRecovery"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(p.apply(&preview)["decisionsObserved"], 1);
    assert_eq!(
        account(&p)["operations"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["outcome"],
        "failed"
    );
    assert_eq!(fs::read(p.work_path(&w)).unwrap(), bytes);
}
