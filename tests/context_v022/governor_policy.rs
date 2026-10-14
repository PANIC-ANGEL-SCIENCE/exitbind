use super::*;

#[test]
fn cooperative_checkpoint_cli_refuses_after_a_replayed_bound() {
    let fixture = Fixture::new();
    let events = fixture.root.join("governor.json");
    fs::write(&events, "[]").unwrap();
    let reduced = Command::new(env!("CARGO_BIN_EXE_exitbind"))
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
    let checkpoint = Command::new(env!("CARGO_BIN_EXE_exitbind"))
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
