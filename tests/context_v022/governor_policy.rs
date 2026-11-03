use super::*;

#[test]
fn cooperative_checkpoint_cli_refuses_after_a_replayed_bound() {
    let fixture = Fixture::new();
    let events = fixture.root.join("governor.json");
    fs::write(&events, "[]").unwrap();
    let reduced = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["context", "reduce", "--events"])
        .arg(&events)
        .output()
        .unwrap();
    assert!(reduced.status.success(), "{reduced:?}");
    let state: Value = serde_json::from_slice(&reduced.stdout).unwrap();
    assert_eq!(state["spent"], 0);
    assert_eq!(state["state"], "ready");

    let state_path = fixture.root.join("state.json");
    let proposal_path = fixture.root.join("proposal.json");
    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    fs::write(&proposal_path, br#"{"unit":"worker-mutation"}"#).unwrap();
    let checkpoint = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["context", "checkpoint", "--state"])
        .arg(&state_path)
        .args(["--proposal"])
        .arg(&proposal_path)
        .output()
        .unwrap();
    assert!(checkpoint.status.success(), "{checkpoint:?}");
    let value: Value = serde_json::from_slice(&checkpoint.stdout).unwrap();
    assert_eq!(value["allowed"], true);
    assert_eq!(value["next"]["unit"], "worker-mutation");
}

#[test]
fn persisted_work_governor_refuses_a_fourth_worker_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "persist bounded worker mutations",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");

    for _ in 0..2 {
        let worker = fixture.json(&["work", "next", &work]);
        let permit = fixture.call(&[
            "work",
            "permit",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "edit",
        ]);
        assert!(permit.status.success(), "{permit:?}");
    }
    let worker = fixture.json(&["work", "next", &work]);
    let replanned = fixture.call(&[
        "work",
        "replan",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--hypothesis",
        "new bounded hypothesis",
    ]);
    assert!(replanned.status.success(), "{replanned:?}");
    let worker = fixture.json(&["work", "next", &work]);
    let third = fixture.call(&[
        "work",
        "permit",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--operation",
        "edit",
    ]);
    assert!(third.status.success(), "{third:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let after_third = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(after_third["governor"]["state"], "evidence_required");
    assert_eq!(after_third["governor"]["afterReplan"], true);
    let worker = fixture.json(&["work", "next", &work]);
    let fourth = fixture.call(&[
        "work",
        "permit",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--operation",
        "edit",
    ]);
    assert!(
        !fourth.status.success(),
        "fourth worker mutation bypassed bound"
    );
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 3);
    assert_eq!(inspected["governor"]["state"], "blocked");
}

#[test]
fn current_recovery_offers_evidence_without_counting_a_copy_as_progress() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "diagnose repeated failure",
        "--check-command",
        "true",
        "--review-policy",
        "required",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let next = fixture.json(&["work", "next", &work]);
    let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
    let artifacts = fixture.root.join(".exitbind/artifacts");
    for (name, body) in [
        ("first-observation", "observed: checker cannot find input"),
        ("same-observation", "observed: checker cannot find input"),
        (
            "new-observation",
            "observed: configured input path differs from the intended path",
        ),
    ] {
        fs::write(artifacts.join(name), body).unwrap();
    }
    let submit_evidence = |name: &str| {
        let path = format!(".exitbind/artifacts/{name}");
        fixture.json(&[
            "work",
            "evidence",
            &work,
            &assignment,
            "--artifact-root",
            "state",
            "--artifact",
            &path,
        ])
    };
    assert_eq!(
        submit_evidence("first-observation")["governor"]["state"],
        "ready"
    );
    for operation in ["first edit", "cosmetic retry"] {
        let result = fixture.json(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            operation,
        ]);
        assert_eq!(result["allowed"], true);
    }
    let detail = fixture.json(&["work", "detail", &work, "--json"]);
    assert_eq!(
        detail["actionForms"]["recovery"]["state"],
        "replan_required"
    );
    assert!(detail["actionForms"]["recovery"]["diagnosis"]
        .as_str()
        .unwrap()
        .contains("rewording"));
    let choices = detail["actionForms"]["choices"].as_array().unwrap();
    assert!(choices.iter().any(|choice| choice["label"] == "replan"));
    assert!(choices.iter().any(|choice| choice["label"] == "evidence"));
    let text_view = fixture.call(&["work", "detail", &work, "--view", "action"]);
    assert!(text_view.status.success(), "{text_view:?}");
    let text_view = String::from_utf8(text_view.stdout).unwrap();
    assert!(
        text_view.contains("A retry or rewording alone"),
        "{text_view}"
    );
    assert!(text_view.contains("Choice evidence:"), "{text_view}");
    let repeated = submit_evidence("same-observation");
    assert_eq!(repeated["governor"]["state"], "replan_required");
    assert_eq!(repeated["governor"]["noInformationStreak"], 2);
    let new = submit_evidence("new-observation");
    assert_eq!(new["governor"]["state"], "ready");
    assert_eq!(new["governor"]["noInformationStreak"], 0);
    assert_eq!(new["governor"]["spent"], 2);
}
