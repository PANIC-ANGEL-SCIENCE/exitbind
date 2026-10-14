use super::*;

#[test]
fn preview_and_apply_create_one_immutable_successor_without_running_new_check() {
    let f = Fixture::new();
    install_narrow_boundary(&f);
    fs::write(f.root.join("preserved-evidence"), b"retain").unwrap();
    let (work, source) = f.begin_failed_check_with_boundary(
        "exec sh checker.sh",
        "repair the checker",
        ".agents/boundaries/task.json",
    );
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap().to_owned();
    let source_sha = source["eventSha256"].as_str().unwrap();
    let before = fs::read(f.ledger(&work)).unwrap();
    let predecessor_start: Value =
        serde_json::from_slice(before.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    let predecessor_events: Vec<Value> = std::str::from_utf8(&before)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let predecessor_state = f.ok(&[
        "run",
        "inspect",
        &format!(".exitbind/runs/work-{}.jsonl", &work[4..]),
        "--json",
    ]);

    let preview_args = replacement_args(&work, &binding);
    let preview = f.ok(&refs(&preview_args));
    assert_eq!(preview["state"], "preview");
    assert_eq!(preview["decision"]["classification"], "defective_contract");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    assert_eq!(
        fs::read(f.root.join("preserved-evidence")).unwrap(),
        b"retain"
    );

    let apply_args = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = f.ok(&refs(&apply_args));
    assert_eq!(applied["state"], "successor_created");
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "predecessor JSONL stays byte-identical"
    );
    assert_eq!(
        fs::read(f.root.join("preserved-evidence")).unwrap(),
        b"retain",
        "replacement check must never be run by continuation"
    );
    let successor = applied["successor"]["work"].as_str().unwrap().to_owned();
    let successor_events: Vec<Value> = fs::read_to_string(f.ledger(&successor))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let start = &successor_events[0];
    assert_eq!(
        successor_events.len(),
        1,
        "continuation writes only a new start, never a copied pass or review"
    );
    assert_eq!(
        start["checkReplacement"]["source"]["eventSha256"],
        source_sha
    );
    assert_eq!(
        start["plan"], predecessor_start["plan"],
        "replacement preserves the complete frozen plan, including every role boundary and provenance"
    );
    assert_eq!(
        start["checkReplacement"]["classification"],
        "defective_contract"
    );
    assert_eq!(
        start["checkPolicy"]["commandSha256"], preview["decision"]["newPolicy"]["commandSha256"],
        "the committed start policy matches the previewed replacement decision"
    );
    assert_eq!(
        start["checkReplacement"]["oldPolicy"]["commandSha256"],
        predecessor_start["checkPolicy"]["commandSha256"]
    );
    assert_eq!(
        start["checkReplacement"]["oldPolicy"]["origin"],
        predecessor_start["checkPolicy"]["origin"]
    );
    assert_eq!(
        start["checkReplacement"]["newPolicy"],
        serde_json::json!({"commandSha256":start["checkPolicy"]["commandSha256"],"origin":start["checkPolicy"]["origin"]}),
        "the replacement marker and actual frozen successor policy are identical"
    );
    assert_eq!(
        start["checkReplacement"]["predecessorHeadEventSha256"],
        predecessor_events.last().unwrap()["eventSha256"],
        "the replacement binds the exact immutable predecessor head"
    );
    assert_eq!(
        start["supersedes"]["ledgerPath"],
        format!(".exitbind/runs/work-{}.jsonl", &work[4..])
    );
    let lead_owner = &start["checkReplacement"]["leadOwner"];
    assert!(
        predecessor_start["plan"]["stages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stage| stage["agents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|agent| agent["role"] == "lead"
                    && agent["name"] == lead_owner["agent"]
                    && agent["nativeTaskName"] == lead_owner["nativeTaskName"])),
        "the marker owner is exactly one configured Lead"
    );
    assert_eq!(
        start["reviewPolicy"], predecessor_start["reviewPolicy"],
        "the predecessor's full review decision is preserved without reconstruction"
    );
    assert_eq!(
        start.get("basis"),
        predecessor_start.get("basis"),
        "replacement preserves the exact optional basis"
    );
    assert_eq!(
        start.get("preservation"),
        predecessor_start.get("preservation"),
        "replacement preserves the exact optional preservation contract"
    );
    assert_eq!(
        start["budget"],
        fs::read_to_string(f.ledger(&work))
            .unwrap()
            .lines()
            .next()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["budget"].clone())
            .unwrap()
    );
    let carried = &start["governorCarry"]["accounting"];
    let spent_before = predecessor_state["governor"]["spent"].as_u64().unwrap();
    assert!(
        spent_before > 0,
        "the fixture must spend some of the same-Goal budget"
    );
    assert_eq!(
        carried["spent"].as_u64(),
        Some(spent_before),
        "successor accounting includes the predecessor's already-spent work"
    );
    assert_eq!(
        carried["budget"], predecessor_state["governor"]["budget"],
        "continuation cannot reset or widen the total budget"
    );
    assert_eq!(
        carried["noInformationStreak"],
        predecessor_state["governor"]["noInformationStreak"]
    );
    assert_eq!(
        carried["postReplanSpent"],
        predecessor_state["governor"]["postReplanSpent"]
    );

    let claim_path = format!("{}.supersede", f.ledger(&work).display());
    let claim: Value = serde_json::from_slice(&fs::read(claim_path).unwrap()).unwrap();
    assert_eq!(claim["checkReplacement"], start["checkReplacement"]);
    assert_eq!(
        claim["oldHeadEventSha256"],
        start["supersedes"]["headEventSha256"]
    );
    assert_eq!(
        claim["checkReplacement"]["newPolicy"]["origin"],
        start["checkPolicy"]["origin"]
    );

    let relation = f.ok(&["work", "detail", &successor, "--json"]);
    assert_eq!(relation["checkReplacement"]["predecessor"]["work"], work);
    assert_eq!(
        relation["checkReplacement"]["source"]["eventSha256"],
        source_sha
    );
    let successor_ledger = f.ledger(&successor);
    let original_successor = fs::read(&successor_ledger).unwrap();
    let mut stripped = start.clone();
    stripped.as_object_mut().unwrap().remove("checkReplacement");
    stripped.as_object_mut().unwrap().remove("eventSha256");
    stripped["eventSha256"] = json!(digest_value(&stripped));
    fs::write(
        &successor_ledger,
        format!("{}\n", serde_json::to_string(&stripped).unwrap()),
    )
    .unwrap();
    let rejected = f.call(&["work", "detail", &successor, "--json"]);
    assert!(
        !rejected.status.success(),
        "a rehashed successor cannot discard its claimed check replacement marker"
    );
    let rejected_json: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert!(
        rejected_json["error"]
            .as_str()
            .unwrap()
            .contains("check replacement marker is missing or differs"),
        "{rejected:?}"
    );
    fs::write(&successor_ledger, &original_successor).unwrap();
    for strip_manifest in [false, true] {
        let mut tampered = start.clone();
        if strip_manifest {
            tampered["plan"]
                .as_object_mut()
                .unwrap()
                .remove("boundaryManifest");
        } else {
            let worker = tampered["plan"]["stages"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .flat_map(|stage| stage["agents"].as_array_mut().unwrap())
                .find(|agent| agent["role"] == "worker")
                .unwrap();
            worker["declaredBoundary"]["write"] = json!(["**"]);
        }
        tampered.as_object_mut().unwrap().remove("eventSha256");
        tampered["eventSha256"] = json!(digest_value(&tampered));
        fs::write(
            &successor_ledger,
            format!("{}\n", serde_json::to_string(&tampered).unwrap()),
        )
        .unwrap();
        let rejected = f.call(&["work", "detail", &successor, "--json"]);
        assert!(!rejected.status.success(), "{rejected:?}");
        let rejected_json: Value = serde_json::from_slice(&rejected.stdout).unwrap();
        assert!(
            rejected_json["error"].as_str().unwrap().contains(
                "check replacement successor plan differs from the frozen predecessor plan"
            ),
            "{rejected:?}"
        );
        fs::write(&successor_ledger, &original_successor).unwrap();
    }
    let retry = f.ok(&refs(&apply_args));
    assert_eq!(retry["state"], "existing_successor_recovered");
    let mut changed = apply_args.clone();
    let reason_index = changed.iter().position(|arg| arg == "--reason").unwrap() + 1;
    changed[reason_index] = "different reason".into();
    assert!(
        !f.call(&refs(&changed)).status.success(),
        "different relation must not create another successor"
    );
}

#[test]
fn replacement_preserves_narrowed_plan_and_refuses_boundary_drift_without_claim() {
    for remove_manifest in [false, true] {
        let f = Fixture::new();
        install_narrow_boundary(&f);
        let (work, _) = f.begin_failed_check_with_boundary(
            "exec sh checker.sh",
            "the check needs a narrowed worker boundary",
            ".agents/boundaries/task.json",
        );
        let binding = f.ok(&["work", "next", &work, "--json"])["current"]["binding"]
            .as_str()
            .unwrap()
            .to_owned();
        let preview = f.ok(&refs(&replacement_args(&work, &binding)));
        let apply = preview["applyCommand"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let boundary_path = f.root.join(".agents/boundaries/task.json");
        let original_boundary = fs::read(&boundary_path).unwrap();
        if remove_manifest {
            fs::remove_file(&boundary_path).unwrap();
        } else {
            fs::write(
                &boundary_path,
                br#"{"version":1,"agents":{"worker":{"observe":["src/**"],"write":["src/**"]}}}"#,
            )
            .unwrap();
        }
        let before_runs = run_entries(&f);
        let before_ledger = fs::read(f.ledger(&work)).unwrap();
        let refused = f.call(&refs(&apply));
        assert!(!refused.status.success(), "{refused:?}");
        let diagnostic = if refused.stdout.is_empty() {
            String::from_utf8_lossy(&refused.stderr).into_owned()
        } else {
            let refusal: Value = serde_json::from_slice(&refused.stdout).unwrap();
            refusal["error"].as_str().unwrap().to_owned()
        };
        assert!(
            diagnostic.contains(if remove_manifest {
                "run boundary manifest is absent after run start"
            } else {
                "run boundary manifest drift detected"
            }),
            "{refused:?}"
        );
        assert_eq!(
            run_entries(&f),
            before_runs,
            "no claim or successor was added"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), before_ledger);

        fs::write(&boundary_path, original_boundary).unwrap();
        let recovered = f.ok(&refs(&apply));
        assert_eq!(recovered["state"], "successor_created");
        let successor = recovered["successor"]["work"].as_str().unwrap();
        let successor_start: Value = serde_json::from_slice(
            fs::read(f.ledger(successor))
                .unwrap()
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap(),
        )
        .unwrap();
        let predecessor_start: Value =
            serde_json::from_slice(before_ledger.split(|byte| *byte == b'\n').next().unwrap())
                .unwrap();
        assert_eq!(successor_start["plan"], predecessor_start["plan"]);
        assert_eq!(
            successor_start["plan"]["boundaryManifest"]["path"],
            ".agents/boundaries/task.json"
        );
        let worker = successor_start["plan"]["stages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|stage| stage["agents"].as_array().unwrap())
            .find(|agent| agent["role"] == "worker")
            .unwrap();
        assert_eq!(worker["declaredBoundary"]["observe"], json!(["src/a.txt"]));
        assert_eq!(worker["declaredBoundary"]["write"], json!(["src/a.txt"]));
    }
}

#[test]
fn successor_scoped_return_completes_full_fresh_review_lifecycle() {
    let f = Fixture::new();
    install_narrow_boundary(&f);
    let (predecessor, _) = f.begin_failed_check_with_boundary(
        "exec sh checker.sh",
        "repair the defective checker contract",
        ".agents/boundaries/task.json",
    );
    let predecessor_bytes = fs::read(f.ledger(&predecessor)).unwrap();
    let lead = f.ok(&["work", "next", &predecessor, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap();
    let mut replacement = replacement_args(&predecessor, binding);
    let check_index = replacement
        .iter()
        .position(|argument| argument == "--check-command")
        .unwrap();
    replacement[check_index + 1] = "test -f checker.sh".into();
    let preview = f.ok(&refs(&replacement));
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = f.ok(&refs(&apply));
    let successor = applied["successor"]["work"].as_str().unwrap().to_owned();
    assert_eq!(fs::read(f.ledger(&predecessor)).unwrap(), predecessor_bytes);

    let successor_start: Value = serde_json::from_slice(
        fs::read(f.ledger(&successor))
            .unwrap()
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    let predecessor_start: Value = serde_json::from_slice(
        predecessor_bytes
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(successor_start["plan"], predecessor_start["plan"]);

    let lead = f.full_next(&successor);
    assert_eq!(lead["next"]["role"], "lead", "{lead}");
    assert_eq!(lead["next"]["action"], "lead_decision");
    f.ok(&[
        "work",
        "return",
        &successor,
        lead["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let scoped_event = f.events(&successor).pop().unwrap();
    assert_eq!(scoped_event["action"], "submit");
    assert_eq!(scoped_event["role"], "lead");
    assert_eq!(scoped_event["outcome"], "scoped");
    assert_eq!(
        f.ok(&["work", "detail", &successor, "--json"])["checkReplacement"]["state"],
        "verified"
    );

    let worker = f.full_next(&successor);
    assert_eq!(worker["next"]["role"], "worker");
    assert_eq!(worker["next"]["action"], "spawn");
    f.ok(&[
        "work",
        "return",
        &successor,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    let completed_event = f.events(&successor).pop().unwrap();
    assert_eq!(completed_event["action"], "submit");
    assert_eq!(completed_event["role"], "worker");
    assert_eq!(completed_event["outcome"], "completed");
    let check = f.ok(&["work", "check", &successor, "--timeout-ms", "2000"]);
    assert_eq!(check["result"]["code"], 0, "{check}");

    let reviewer = f.full_next(&successor);
    assert_eq!(reviewer["next"]["role"], "reviewer");
    assert_eq!(reviewer["next"]["action"], "spawn");
    f.ok(&[
        "work",
        "return",
        &successor,
        reviewer["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "approved",
    ]);
    let reviewed_event = f.events(&successor).pop().unwrap();
    assert_eq!(reviewed_event["action"], "submit");
    assert_eq!(reviewed_event["role"], "reviewer");
    assert_eq!(reviewed_event["outcome"], "approved");
    let lead = f.full_next(&successor);
    assert_eq!(lead["next"]["role"], "lead");
    assert_eq!(lead["next"]["action"], "lead_decision");
    f.ok(&[
        "work",
        "return",
        &successor,
        lead["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "accepted",
    ]);
    let accepted_event = f.events(&successor).pop().unwrap();
    assert_eq!(accepted_event["action"], "submit");
    assert_eq!(accepted_event["role"], "lead");
    assert_eq!(accepted_event["outcome"], "accepted");
    let terminal = f.full_next(&successor);
    assert_eq!(terminal["next"]["action"], "done");
    assert_eq!(
        f.ok(&["work", "detail", &successor, "--json"])["checkReplacement"]["state"],
        "verified"
    );
    assert_eq!(fs::read(f.ledger(&predecessor)).unwrap(), predecessor_bytes);
}
