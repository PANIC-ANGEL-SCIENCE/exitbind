use super::*;

#[test]
fn agent_facing_projection_strips_memory_transport_paths() {
    let fixture = Fixture::new();
    let config: Value = serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
    let mut config = config;
    config["memory"] = json!({
        "root": ".exitbind/memory",
        "maxItems": 8,
        "maxBytes": 4096,
        "protocolScopes": ["invariants"],
        "syntheticScopes": []
    });
    for agent in ["lead", "worker"] {
        config["agents"][agent]["memoryRead"] = json!(["invariants"]);
        config["agents"][agent]["crossContext"] = json!("protocol-only");
    }
    config["agents"]["lead"]["memoryWrite"] = json!(["invariants"]);
    config["agents"]["lead"]["memoryReview"] = json!(["invariants"]);
    config["agents"]["lead"]["memoryPromote"] = json!(["invariants"]);
    fs::write(
        &fixture.config,
        format!("{}\n", serde_json::to_string_pretty(&config).unwrap()),
    )
    .unwrap();
    fs::write(fixture.root.join("memory.md"), "accepted invariant\n").unwrap();
    let ledger = ".exitbind/memory/invariant.jsonl";
    for args in [
        vec![
            "memory",
            "propose",
            "lead",
            "memory.md",
            "--scope",
            "invariants",
            "--ledger",
            ledger,
        ],
        vec!["memory", "review", "lead", ledger],
        vec!["memory", "promote", "lead", ledger],
    ] {
        let output = fixture.call(&args);
        assert!(output.status.success(), "{args:?}: {output:?}");
    }

    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "opaque memory projection",
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
    let next = fixture.json(&["work", "next", &work]);

    assert_no_transport_paths(&started);
    assert_no_transport_paths(&next);
    assert_resolver_backed_references(&started);
    assert_resolver_backed_references(&next);
    assert!(next["next"]["packet"]["memoryReferences"].is_array());
}

#[test]
fn historical_v2_context_is_accepted_as_stale_not_malformed() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "historical opaque context",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap();
    let residual = fixture.json(&["work", "next", work])["residual"].clone();
    let packet_path = fixture.root.join(".exitbind/legacy-residual.json");
    let mut legacy = residual;
    legacy["context"]["version"] = json!(2);
    refresh_digest(&mut legacy["context"]);
    fs::write(&packet_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let packet_path = packet_path.to_str().unwrap();
    let validated = fixture.json(&["work", "validate", work, "--packet", packet_path]);
    assert_eq!(validated["result"], "refresh_required");
    assert_eq!(validated["reason"], "context_projection_changed");
}

#[test]
fn fresh_or_compacted_consumer_gets_current_packet_blockers_and_resolvers() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "fresh consumer minimum",
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
    let next_output = fixture.call(&["work", "next", &work]);
    assert!(next_output.status.success(), "{next_output:?}");
    let next: Value = serde_json::from_slice(&next_output.stdout).unwrap();
    let resumed = fixture.json(&["work", "resume"]);
    let packet = &resumed["residual"];
    let next_context = &next["next"]["packet"]["context"];
    let context = &packet["context"];

    assert_eq!(next["work"], work);
    assert_eq!(resumed["work"], work);
    assert_no_transport_paths(&next);
    assert_no_transport_paths(&resumed);
    assert_resolver_backed_references(&next);
    assert_resolver_backed_references(&resumed);

    // These are the pre-context packet fields consumed by older JSON clients;
    // context is additive and does not replace the raw packet shape.
    for field in [
        "version",
        "work",
        "workflow",
        "goal",
        "historical",
        "snapshot",
        "alreadyEstablished",
        "stillValid",
        "remaining",
        "next",
        "doNotRepeat",
        "invalidation",
    ] {
        assert!(
            packet.get(field).is_some(),
            "legacy packet field missing: {field}"
        );
    }
    assert_eq!(packet["version"], 2);
    assert!(packet["snapshot"]["eventCount"].is_u64());
    assert!(packet["snapshot"]["headEventSha256"].is_string());
    assert!(packet["snapshot"]["inputsSha256"].is_string());
    assert!(!packet["remaining"].as_array().unwrap().is_empty());
    assert!(packet["humanHelp"]["whatRemains"].is_array());
    assert!(packet["humanHelp"]["nextAction"]["actor"].is_string());
    assert!(packet["humanHelp"]["nextAction"]["summary"].is_string());
    assert!(packet["humanHelp"]["nextAction"]["command"].is_object());

    for field in [
        "version",
        "run",
        "subject",
        "goal",
        "scope",
        "obligations",
        "evidence",
        "loop",
        "next",
        "expansions",
        "recovery",
        "digest",
    ] {
        assert!(
            context.get(field).is_some(),
            "context field missing: {field}"
        );
    }
    assert_eq!(context["version"], 3);
    assert_eq!(context["scope"]["freshness"], "current");
    assert_eq!(context["next"]["freshness"], "current");
    assert_eq!(next_context, context);
    assert_eq!(context["recovery"]["next"], context["next"]);
    assert!(!context["recovery"]["missing"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(context["recovery"]["stale"], json!([]));

    let mut ids = Vec::new();
    resolver_ids(context, &mut ids);
    ids.sort();
    ids.dedup();
    assert!(!ids.is_empty());
    for id in ids {
        let expanded = fixture.call(&["work", "expand", &work, &id]);
        assert!(expanded.status.success(), "{id}: {expanded:?}");
        let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
        assert_eq!(expanded["valid"], true, "{id}: {expanded}");
    }

    let resumed_context = &resumed["residual"]["context"];
    assert_eq!(resumed_context["digest"], context["digest"]);
    assert_eq!(resumed_context["next"], context["next"]);
}

#[test]
fn worker_packet_carries_resolved_preservation_assignment_before_artifact() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preserve worker packet assignment",
        "--check-command",
        "true",
        "--preserve-requirement",
        "canonical:preserve canonical fields",
        "--preservation-check-command",
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

    let worker = fixture.json(&["work", "next", &work]);
    let packet = &worker["next"]["packet"];
    let preservation = &packet["preservationAssignment"];
    assert_eq!(preservation["route"], "FORMAL");
    assert_eq!(preservation["quality"], "FULL");
    assert_eq!(
        preservation["resolvedBy"],
        "accepted_preservation_requirements"
    );
    assert_eq!(preservation["enforcement"], "recorded_not_enforced");
    assert_eq!(preservation["version"], 1);
    assert_eq!(preservation["source"], "canonical_state");
    assert_eq!(preservation["nonGoals"], json!([]));
    assert_eq!(preservation["requirements"][0]["id"], "canonical");
    assert_eq!(
        preservation["requirements"][0]["text"],
        "preserve canonical fields"
    );
    assert_eq!(preservation["requirements"][0]["command"], "true");
    assert_eq!(
        preservation["requirements"][0]["commandSha256"],
        "b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b"
    );
    assert_eq!(preservation["requirements"][0]["origin"], "local_report");
    assert!(packet["humanHelp"].is_null());
    assert!(packet["artifactPathHint"].is_null());
    assert!(worker["next"]["check"].is_null());
}

#[test]
fn role_context_keeps_current_and_previous_attempts_bounded() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "bound recovery history",
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
    let mut worker_lengths = Vec::new();
    for round in 0..2 {
        let worker = fixture.json(&["work", "next", &work]);
        worker_lengths.push((
            worker["next"]["packet"]["context"]["evidence"]
                .as_array()
                .unwrap()
                .len(),
            worker["next"]["packet"]["context"]["expansions"]
                .as_array()
                .unwrap()
                .len(),
            worker["next"]["packet"]["upstreamArtifacts"]
                .as_array()
                .unwrap()
                .len(),
        ));
        let completed = fixture.return_body(
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "completed",
            format!("worker {round}\n").as_bytes(),
        );
        assert!(completed.status.success(), "{completed:?}");
        fixture.json(&["work", "check", &work]);
        let reviewer = fixture.json(&["work", "next", &work]);
        let reviewed = fixture.return_body(
            &work,
            reviewer["next"]["assignment"].as_str().unwrap(),
            "approved",
            b"review\n",
        );
        assert!(reviewed.status.success(), "{reviewed:?}");
        let lead = fixture.json(&["work", "next", &work]);
        let reworked = fixture.return_body(
            &work,
            lead["next"]["assignment"].as_str().unwrap(),
            "rework",
            b"rework\n",
        );
        assert!(reworked.status.success(), "{reworked:?}");
    }
    assert!(worker_lengths
        .iter()
        .all(|(_, expansions, _)| *expansions == 1));
    assert!(worker_lengths.iter().all(|(_, _, upstream)| *upstream <= 3));
    assert!(worker_lengths.iter().all(|(evidence, _, _)| *evidence <= 2));
    let fresh = fixture.json(&["work", "resume"]);
    assert_eq!(fresh["status"], "resumed");
    assert_eq!(
        fresh["residual"]["context"]["expansions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!fresh["residual"]["context"]["recovery"]["stale"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn pre_v022_ledger_projects_governor_as_not_applicable() {
    let fixture = Fixture::new();
    let token = "5".repeat(64);
    let ledger = format!(".exitbind/runs/work-{token}.jsonl");
    let _started = fixture.json(&[
        "run",
        "start",
        "change",
        "--goal",
        "historical governor projection",
        "--ledger",
        &ledger,
        "--check-command",
        "true",
    ]);
    let mut event: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&ledger))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    event["version"] = json!(5);
    for field in [
        "governor",
        "guidanceContinuityProtocol",
        "guidanceConfigSha256",
        "verificationInputContract",
    ] {
        event.as_object_mut().unwrap().remove(field);
    }
    let mut unsigned = event.clone();
    unsigned.as_object_mut().unwrap().remove("eventSha256");
    let digest = Sha256::digest(serde_json::to_vec(&unsigned).unwrap());
    use std::fmt::Write as _;
    let mut digest_hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut digest_hex, "{byte:02x}").unwrap();
    }
    event["eventSha256"] = json!(digest_hex);
    fs::write(
        fixture.root.join(&ledger),
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();
    let work = format!("smw_{token}");
    let next = fixture.json(&["work", "next", &work]);
    assert_eq!(next["next"]["packet"]["context"]["loop"]["enabled"], false);
    assert_eq!(
        next["next"]["packet"]["context"]["loop"]["state"],
        "not_applicable"
    );
    assert!(next["next"]["packet"]["context"]["loop"]["spent"].is_null());
}

#[test]
fn work_projection_is_thin_exact_and_recoverable_without_replay() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preserve packet semantics",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    fixture.json(&[
        "goal",
        "incorporate",
        "--goal-id",
        &work,
        "--goal",
        "Preserve packet semantics for the user",
        "--obligation",
        "Keep the native current view available",
    ]);
    let started = fixture.json(&["work", "next", &work]);
    assert!(started["next"]["packet"]["context"]["digest"].is_string());
    assert_eq!(started["next"]["goalProgress"]["overall"], "in_progress");
    assert!(started["next"]["goalProgress"]["systemText"]
        .as_str()
        .unwrap()
        .contains("Tasks: 0/1 complete"));
    let residual = fixture.json(&["work", "next", &work]);
    let context = &residual["residual"]["context"];
    assert_eq!(context["version"], 3);
    assert_eq!(context["role"], "lead");
    assert_eq!(context["run"]["workflow"], "change");
    assert!(context["goal"].is_string());
    assert!(context["obligations"].is_array());
    assert!(context["evidence"].is_array());
    assert!(context["expansions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| {
            item["exact"] == true
                && item["sha256"].is_string()
                && item["id"].as_str().is_some_and(|id| id.starts_with("ref:"))
                && item["path"].is_null()
        }));
    assert!(context["digest"].as_str().unwrap().len() == 64);

    // Keep the saved packet under Exitbind's excluded state root so creating
    // the recovery artifact cannot change the tested-input fingerprint.
    let packet_path = fixture.root.join(".exitbind/residual.json");
    fs::write(
        &packet_path,
        serde_json::to_vec(&residual["residual"]).unwrap(),
    )
    .unwrap();
    let packet_path = packet_path.to_str().unwrap();
    let usable = fixture.json(&["work", "validate", &work, "--packet", packet_path]);
    assert_eq!(usable["result"], "usable");

    let mut tampered = residual["residual"].clone();
    tampered["context"]["goal"] = json!("weaker goal");
    fs::write(packet_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
    let refused = fixture.json(&["work", "validate", &work, "--packet", packet_path]);
    assert_eq!(refused["result"], "cannot_establish_applicability");
    assert_eq!(refused["reason"], "malformed_context_projection");

    let assignment = started["next"]["assignment"].as_str().unwrap();
    let mut returner = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    returner
        .current_dir(&fixture.root)
        .args(["work", "return", &work, assignment, "--outcome", "scoped"])
        .arg("--config")
        .arg(&fixture.config)
        .stdin(Stdio::null());
    let returned = returner.output().unwrap();
    assert!(returned.status.success(), "{returned:?}");
    fs::write(
        packet_path,
        serde_json::to_vec(&residual["residual"]).unwrap(),
    )
    .unwrap();
    let stale = fixture.json(&["work", "validate", &work, "--packet", packet_path]);
    assert_eq!(stale["result"], "refresh_required");
    // The canonical packet precedence remains intact: a ledger transition is
    // reported as ledger advancement even though its additive context is also
    // necessarily stale.
    assert_eq!(stale["reason"], "ledger_advanced");

    let resumed = fixture.json(&["work", "resume"]);
    assert_eq!(resumed["status"], "resumed");
    assert!(resumed["residual"]["context"]["digest"].is_string());

    // The cooperative governor is also persisted on the real run ledger: a
    // worker completion through the work façade advances its replayed state,
    // rather than living only in the standalone context reducer fixture.
    let worker = fixture.json(&["work", "next", &work]);
    let worker_assignment = worker["next"]["assignment"].as_str().unwrap();
    let mut completion = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    completion
        .current_dir(&fixture.root)
        .args([
            "work",
            "return",
            &work,
            worker_assignment,
            "--outcome",
            "completed",
        ])
        .arg("--config")
        .arg(&fixture.config)
        .stdin(Stdio::piped());
    let mut child = completion.spawn().unwrap();
    use std::io::Write;
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child.stdin.take().unwrap().write_all(b"worker mutation\n") {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 1);
}

#[test]
fn opaque_history_and_observed_check_log_expansions_are_exact_and_stale_bound() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "expand exact evidence",
        "--check-command",
        "printf out; printf err >&2",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let history = started["next"]["packet"]["context"]["expansions"][0].clone();
    let history_arg = history["id"].as_str().unwrap().to_owned();
    let expanded = fixture.call(&["work", "expand", &work, &history_arg]);
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["valid"], true);
    assert_eq!(expanded["encoding"], "hex");
    assert!(expanded["bytes"].as_u64().unwrap_or(0) > history["eventCount"].as_u64().unwrap_or(0));
    assert_eq!(
        expanded["contentHex"].as_str().unwrap().len() as u64,
        expanded["bytes"].as_u64().unwrap() * 2
    );

    let lead = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(lead.status.success(), "{lead:?}");
    let stale = fixture.call(&["work", "expand", &work, &history_arg]);
    assert!(
        !stale.status.success(),
        "stale reference unexpectedly expanded"
    );

    let worker = fixture.json(&["work", "next", &work]);
    let completed = fixture.return_body(
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "completed",
        b"worker\n",
    );
    assert!(completed.status.success(), "{completed:?}");
    let checked = fixture.json(&["work", "check", &work]);
    assert_eq!(checked["next"]["requiresExpansion"], true);
    let next = fixture.json(&["work", "next", &work]);
    let context = &next["next"]["packet"]["context"];
    let log_reference = context["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("checkLogRef"))
        .cloned()
        .expect("observed check log reference");
    let log_arg = log_reference["id"].as_str().unwrap().to_owned();
    let expanded_log = fixture.call(&["work", "expand", &work, &log_arg]);
    assert!(expanded_log.status.success(), "{expanded_log:?}");
    let expanded_log: Value = serde_json::from_slice(&expanded_log.stdout).unwrap();
    assert_eq!(expanded_log["stdout"]["contentHex"], "6f7574");
    assert_eq!(expanded_log["stderr"]["contentHex"], "657272");
}
