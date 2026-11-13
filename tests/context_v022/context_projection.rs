use super::*;

fn assert_embedded_memory_delivery(response: &Value, agent: &str) -> Value {
    let delivery = &response["currentDelivery"];
    assert_eq!(delivery["complete"], true, "{response}");
    let recipient = &delivery["recipientContext"];
    assert_eq!(recipient["agent"], agent);
    let guidance = &recipient["guidance"];
    assert_no_transport_paths(guidance);
    let item = guidance["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["reference"]["scope"] == "invariants")
        .expect("selected memory must be delivered");
    assert_eq!(item["content"], "accepted invariant\n");
    let reference = &item["reference"];
    assert!(reference["itemId"].is_string());
    assert!(reference["sourceSha256"].is_string());
    assert!(reference["ledgerHeadSha256"].is_string());
    reference.clone()
}

fn return_with_output(
    fixture: &Fixture,
    work: &str,
    assignment: &str,
    outcome: &str,
    body: &[u8],
) -> Output {
    use std::io::Write;
    let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["work", "return", work, assignment, "--outcome", outcome])
        .arg("--config")
        .arg(&fixture.config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(body).unwrap();
    child.wait_with_output().unwrap()
}

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
    for agent in ["lead", "worker", "reviewer"] {
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
    let selected = assert_embedded_memory_delivery(&started, "lead");
    assert_eq!(selected, started["next"]["packet"]["memoryReferences"][0]);

    // These typed paths identify the delivered profile and observed checker,
    // not memory transport. Check them before applying the strict scan.
    let profile = &started["currentDelivery"]["recipientContext"]["profile"];
    assert_eq!(profile["path"], "exitbind/agents/lead.md");
    assert_eq!(profile["complete"], true);
    assert!(profile["sha256"].is_string());
    let checker = &started["beginPreparation"]["checker"];
    assert_eq!(checker["program"], "true");
    assert_eq!(checker["state"], "runnable");
    let checker_path = checker["path"].as_str().unwrap();
    assert!(std::path::Path::new(checker_path).is_absolute());
    assert_eq!(
        started["currentDelivery"]["sections"]["assignment"]["checkPreparation"]["path"],
        checker_path
    );
    let mut transport_checked = started.clone();
    for pointer in [
        "/beginPreparation/checker",
        "/currentDelivery/recipientContext/profile",
        "/currentDelivery/sections/assignment/checkPreparation",
    ] {
        transport_checked
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("path");
    }
    assert_no_transport_paths(&transport_checked);
    // Ensure the strict selected-memory scan still catches a real leak.
    for key in ["ledgerPath", "sourcePath", "path", "root"] {
        let mut leaked = selected.clone();
        leaked[key] = json!("private/transport/location");
        assert!(std::panic::catch_unwind(|| assert_no_transport_paths(&leaked)).is_err());
    }

    // Explicit detail retains its established native reference shape, while
    // the public copy carries the same body and stable selected identity.
    let direct = fixture.json(&["work", "detail", &work, "--json"]);
    let direct_item = &direct["recipientContext"]["guidance"]["items"][0];
    assert_eq!(direct_item["reference"]["ledgerPath"], ledger);
    assert_eq!(direct_item["reference"]["sourcePath"], "memory.md");
    assert_eq!(direct_item["reference"]["itemId"], selected["itemId"]);
    assert_eq!(direct_item["content"], "accepted invariant\n");
    let resolved = fixture.json(&[
        "project",
        "context",
        "memory",
        selected["itemId"].as_str().unwrap(),
        "--agent",
        "lead",
        "--task",
        "opaque memory projection",
        "--expected-sha256",
        selected["sourceSha256"].as_str().unwrap(),
        "--json",
    ]);
    assert_eq!(resolved["content"], "accepted invariant\n");

    let scoped = return_with_output(
        &fixture,
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let scoped: Value = serde_json::from_slice(&scoped.stdout).unwrap();
    assert_embedded_memory_delivery(&scoped, "worker");
    let next = fixture.json(&["work", "next", &work]);

    assert_no_transport_paths(&next);
    assert_resolver_backed_references(&started);
    assert_resolver_backed_references(&next);
    assert!(next["next"]["packet"]["memoryReferences"].is_array());
    let assignment = next["next"]["assignment"].as_str().unwrap();
    let permit = fixture.json(&[
        "work",
        "permit",
        &work,
        assignment,
        "--operation",
        "Verify memory delivery",
    ]);
    assert_eq!(permit["allowed"], true);
    assert_embedded_memory_delivery(&permit, "worker");
    let completed =
        return_with_output(&fixture, &work, assignment, "completed", b"worker result\n");
    assert!(completed.status.success(), "{completed:?}");
    let checked = fixture.json(&["work", "check", &work]);
    assert_embedded_memory_delivery(&checked, "reviewer");

    let explicit = fixture.json(&[
        "work",
        "begin",
        "change",
        "--detail",
        "--goal",
        "opaque memory projection",
        "--check-command",
        "true",
    ]);
    assert_eq!(explicit["kind"], "work_detail");
    assert_eq!(
        explicit["recipientContext"]["guidance"]["items"][0]["reference"]["sourcePath"],
        "memory.md"
    );
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

#[test]
fn bounded_log_tail_and_continuations_keep_late_error_exact_and_current() {
    let fixture = Fixture::new();
    let marker = "DECISIVE ERROR: validator rejected the final entry";
    let mut original = "x".repeat(65_535);
    original.push('🙂'); // The legacy first chunk ends inside this character.
    original.push_str(&"y".repeat(7_500));
    let mut boundary_offsets = Vec::new();
    for quotes in (2400..=3400).step_by(25) {
        boundary_offsets.push(original.len());
        original.push_str(&"\"".repeat(quotes));
        original.push_str(&"z".repeat(4096 - quotes));
    }
    original.push('\n');
    original.push_str(marker);
    original.push('\n');
    fs::write(fixture.root.join("large-check.log"), original.as_bytes()).unwrap();
    let mut binary = vec![0xff; 9_000];
    binary.extend_from_slice(b" BINARY END\n");
    fs::write(fixture.root.join("binary-check.log"), &binary).unwrap();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "late check error",
        "--check-command",
        "cat large-check.log; cat binary-check.log >&2; exit 1",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n"
        )
        .status
        .success());
    let worker = fixture.json(&["work", "next", &work]);
    assert!(fixture
        .return_body(
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "completed",
            b"worker\n"
        )
        .status
        .success());
    let failed = fixture.call(&["work", "check", &work]);
    assert!(!failed.status.success(), "{failed:?}");

    // Root sees bounded navigation without receiving the large original.
    let compact = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["work", "next", &work, "--json", "--config"])
        .arg(&fixture.config)
        .output()
        .unwrap();
    assert!(compact.status.success(), "{compact:?}");
    assert!(compact.stdout.len() <= 8 * 1024, "{}", compact.stdout.len());
    assert!(!String::from_utf8_lossy(&compact.stdout).contains(marker));
    let compact: Value = serde_json::from_slice(&compact.stdout).unwrap();
    let log_ref = compact["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "check_log")
        .expect("current check-log reference");
    let base = log_ref["id"].as_str().unwrap().to_owned();
    let tail = log_ref["boundedTail"]["stdout"].clone();
    let mut largest = 0;
    for offset in boundary_offsets {
        let reference = format!("{base}:log:stdout:offset:{offset}");
        let output = fixture.call(&["work", "expand", &work, &reference]);
        assert!(output.status.success(), "{offset}: {output:?}");
        largest = largest.max(output.stdout.len());
        assert!(
            output.stdout.len() <= 8 * 1024,
            "{offset}: {} bytes",
            output.stdout.len()
        );
    }
    assert!(
        largest > 7900,
        "no near-boundary response observed: {largest}"
    );
    let invoke = |route: &Value| {
        let mut command = route["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        if route["sameExecutableRequired"] == true {
            command.insert(0, env!("CARGO_BIN_EXE_exitbind").to_owned());
        }
        if route["sameConfigRequired"] == true {
            command.push(fixture.config.to_str().unwrap().to_owned());
        }
        let mut child = support::git_topology::command(&command[0]);
        child.current_dir(&fixture.root).args(&command[1..]);
        child.output().unwrap()
    };
    let tail_output = invoke(&tail);
    assert!(tail_output.status.success(), "{tail_output:?}");
    assert!(
        tail_output.stdout.len() <= 8 * 1024,
        "{}",
        tail_output.stdout.len()
    );
    let tail_value: Value = serde_json::from_slice(&tail_output.stdout).unwrap();
    assert_eq!(tail_value["encoding"], "utf-8");
    assert!(tail_value["contentText"].as_str().unwrap().contains(marker));
    assert_eq!(tail_value["next"], Value::Null);

    let legacy = fixture.call(&["work", "expand", &work, &base]);
    assert!(legacy.status.success(), "{legacy:?}");
    let legacy: Value = serde_json::from_slice(&legacy.stdout).unwrap();
    assert_eq!(legacy["stdout"]["displayedBytes"], 65_536);
    assert_eq!(legacy["stdout"]["truncated"], true);
    assert!(legacy["stdout"]["contentText"].is_null());
    let decode_hex = |text: &str| -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    };
    let binary_tail = invoke(&log_ref["boundedTail"]["stderr"]);
    assert!(binary_tail.status.success(), "{binary_tail:?}");
    assert!(
        binary_tail.stdout.len() <= 8 * 1024,
        "{}",
        binary_tail.stdout.len()
    );
    let binary_tail: Value = serde_json::from_slice(&binary_tail.stdout).unwrap();
    assert_eq!(binary_tail["encoding"], "hex");
    let binary_page = decode_hex(binary_tail["contentHex"].as_str().unwrap());
    assert!(binary_page.ends_with(b" BINARY END\n"));
    let binary_offset = binary_tail["offset"].as_u64().unwrap() as usize;
    assert_eq!(binary_page, binary[binary_offset..]);
    let mut rebuilt = decode_hex(legacy["stdout"]["contentHex"].as_str().unwrap());
    let mut route = legacy["stdout"]["continuation"]["next"].clone();
    let mut first = true;
    while !route.is_null() {
        let output = invoke(&route);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.len() <= 8 * 1024, "{}", output.stdout.len());
        let page: Value = serde_json::from_slice(&output.stdout).unwrap();
        let bytes = if page["encoding"] == "utf-8" {
            page["contentText"].as_str().unwrap().as_bytes().to_vec()
        } else {
            decode_hex(page["contentHex"].as_str().unwrap())
        };
        if first {
            assert_eq!(page["offset"], 65_535);
            assert!(page["contentText"].as_str().unwrap().starts_with('🙂'));
            first = false;
        }
        let start = page["offset"].as_u64().unwrap() as usize;
        assert!(start <= rebuilt.len());
        let overlap = rebuilt.len() - start;
        assert!(overlap <= bytes.len());
        assert_eq!(&rebuilt[start..], &bytes[..overlap]);
        rebuilt.extend_from_slice(&bytes[overlap..]);
        route = page["next"].clone();
    }
    assert_eq!(rebuilt, original.as_bytes());

    let tail_ref = tail["command"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|word| word.as_str().filter(|word| word.contains(":log:")))
        .unwrap();
    let suffix = tail_ref.split_once(":log:").unwrap().1;
    let wrong = format!("ref:{}:log:{suffix}", "0".repeat(64));
    assert!(!fixture
        .call(&["work", "expand", &work, &wrong])
        .status
        .success());
    let bad_stream = format!("{base}:log:unknown:offset:0");
    assert!(!fixture
        .call(&["work", "expand", &work, &bad_stream])
        .status
        .success());
    let artifact = fixture
        .root
        .join(legacy["event"]["stdout"]["path"].as_str().unwrap());
    fs::write(&artifact, b"changed after capture").unwrap();
    assert!(
        !invoke(&tail).status.success(),
        "modified evidence must not expand"
    );
    fs::write(&artifact, original.as_bytes()).unwrap();
    let second = fixture.call(&["work", "check", &work]);
    assert!(!second.status.success(), "the fixture checker still fails");
    assert!(
        !invoke(&tail).status.success(),
        "old ledger reference must be stale"
    );
}
