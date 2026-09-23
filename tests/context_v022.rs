mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("context-v022");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        Self { root, config }
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn return_body(&self, work: &str, assignment: &str, outcome: &str, body: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(["work", "return", work, assignment, "--outcome", outcome])
            .arg("--config")
            .arg(&self.config)
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }

    fn rewrite_last<F>(&self, ledger: &str, mutate: F)
    where
        F: FnOnce(&mut Value),
    {
        let path = self.root.join(ledger);
        let mut lines: Vec<Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let event = lines.last_mut().unwrap();
        mutate(event);
        if let Some(governor) = event
            .get_mut("governorEvent")
            .and_then(Value::as_object_mut)
        {
            if governor.contains_key("eventSha256") {
                governor.remove("eventSha256");
                let digest = Sha256::digest(serde_json::to_vec(governor).unwrap());
                let mut digest_hex = String::with_capacity(digest.len() * 2);
                for byte in digest {
                    write!(&mut digest_hex, "{byte:02x}").unwrap();
                }
                governor.insert("eventSha256".into(), json!(digest_hex));
            }
        }
        event.as_object_mut().unwrap().remove("eventSha256");
        let digest = Sha256::digest(serde_json::to_vec(event).unwrap());
        let mut digest_hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut digest_hex, "{byte:02x}").unwrap();
        }
        event["eventSha256"] = json!(digest_hex);
        fs::write(
            path,
            lines
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
                + "\n",
        )
        .unwrap();
    }

    fn artifact_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = fs::read_dir(self.root.join(".exitbind/artifacts"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn assert_no_transport_paths(value: &Value) {
    match value {
        Value::Object(object) => {
            for key in [
                "artifactPathHint",
                "artifactRootHint",
                "ledgerPath",
                "path",
                "profilePath",
                "root",
                "sourcePath",
            ] {
                assert!(!object.contains_key(key), "transport path leaked: {key}");
            }
            object.values().for_each(assert_no_transport_paths);
        }
        Value::Array(values) => values.iter().for_each(assert_no_transport_paths),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn assert_resolver_backed_references(value: &Value) {
    match value {
        Value::Object(object) => {
            if object
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with("ref:"))
            {
                assert_eq!(object.get("exact"), Some(&json!(true)));
                assert!(matches!(
                    object.get("kind").and_then(Value::as_str),
                    Some(
                        "ledger_history"
                            | "ledger_event"
                            | "check_log"
                            | "evidence"
                            | "stdout"
                            | "stderr",
                    )
                ));
            }
            if object.contains_key("path") {
                assert!(!object
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.starts_with("ref:")));
            }
            object.values().for_each(assert_resolver_backed_references);
        }
        Value::Array(values) => values.iter().for_each(assert_resolver_backed_references),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn resolver_ids(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                if id.starts_with("ref:") {
                    ids.push(id.to_owned());
                }
            }
            object.values().for_each(|value| resolver_ids(value, ids));
        }
        Value::Array(values) => values.iter().for_each(|value| resolver_ids(value, ids)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| {
                        format!(
                            "{}:{}",
                            serde_json::to_string(key).unwrap(),
                            canonical(&object[key])
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn refresh_digest(value: &mut Value) {
    value.as_object_mut().unwrap().remove("digest");
    let digest = Sha256::digest(canonical(value).as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    value["digest"] = json!(encoded);
}

fn refresh_event_hash(value: &mut Value) {
    value.as_object_mut().unwrap().remove("eventSha256");
    let digest = Sha256::digest(canonical(value).as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    value["eventSha256"] = json!(encoded);
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
    event.as_object_mut().unwrap().remove("governor");
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
    assert!(started["next"]["packet"]["context"]["digest"].is_string());
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
    let mut returner = Command::new(env!("CARGO_BIN_EXE_exitbind"));
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
    let mut completion = Command::new(env!("CARGO_BIN_EXE_exitbind"));
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"worker mutation\n")
        .unwrap();
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
    let context = &checked["next"]["packet"]["context"];
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

#[test]
fn request_id_replays_exact_permit_without_new_event_or_spent() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "replay a mutation request",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let args = [
        "work",
        "permit",
        work.as_str(),
        assignment.as_str(),
        "--operation",
        "edit",
        "--request-id",
        "request-1",
    ];
    let first = fixture.call(&args);
    assert!(first.status.success(), "{first:?}");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["event"]["requestId"], "request-1");
    assert_eq!(
        first["event"]["requestDigest"],
        first["event"]["governorEvent"]["requestDigest"]
    );
    assert_eq!(
        first["event"]["requestId"],
        first["event"]["governorEvent"]["requestId"]
    );
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let replay = fixture.call(&args);
    assert!(replay.status.success(), "{replay:?}");
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["idempotent"], true);
    assert_eq!(
        replay["event"]["eventSha256"],
        first["event"]["eventSha256"]
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        1
    );
}

#[test]
fn request_id_conflicting_operation_refuses_without_ledger_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject a conflicting mutation request",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let conflict = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "different-edit",
        "--request-id",
        "request-1",
    ]);
    assert!(!conflict.status.success(), "{conflict:?}");
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("request-id conflict"));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        1
    );
}

#[test]
fn stale_request_id_is_not_replayed_after_assignment_advances() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "refuse a stale mutation request",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());
    fs::write(fixture.root.join("product.txt"), b"advanced\n").unwrap();
    assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    assert!(fixture
        .return_body(&work, &assignment, "completed", b"worker\n")
        .status
        .success());
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let stale = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-1",
    ]);
    assert!(
        !stale.status.success(),
        "stale request was replayed: {stale:?}"
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn invalid_request_id_is_refused_before_ledger_append() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "validate mutation request ids",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let empty = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "",
    ]);
    assert!(!empty.status.success());
    let oversized = "x".repeat(121);
    let oversized_call = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        oversized.as_str(),
    ]);
    assert!(!oversized_call.status.success());
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn blocked_request_id_replay_remains_refused_without_a_second_event() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preserve a durable blocked refusal",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let mut assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    for request_id in ["request-1", "request-2"] {
        let permit = fixture.call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            request_id,
        ]);
        assert!(permit.status.success(), "{permit:?}");
        fs::write(fixture.root.join("product.txt"), request_id).unwrap();
        assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
        if request_id == "request-2" {
            let replanned = fixture.call(&[
                "work",
                "replan",
                &work,
                &assignment,
                "--hypothesis",
                "bounded blocked replay",
            ]);
            assert!(replanned.status.success(), "{replanned:?}");
            assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
                .as_str()
                .unwrap()
                .to_owned();
        }
    }
    let third = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-3",
    ]);
    assert!(third.status.success(), "{third:?}");
    fs::write(fixture.root.join("product.txt"), b"request-3").unwrap();
    assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let blocked = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-4",
    ]);
    assert!(
        !blocked.status.success(),
        "blocked permit unexpectedly succeeded"
    );
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("durably blocked"));
    let after_block = fs::read(fixture.root.join(&ledger)).unwrap();
    let replay = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-4",
    ]);
    assert!(
        !replay.status.success(),
        "blocked request replay escaped refusal"
    );
    assert!(String::from_utf8_lossy(&replay.stderr).contains("durably blocked"));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), after_block);
}

#[test]
fn reducer_rejects_rehashed_request_digest_and_duplicate_request_scope() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject forged request identity history",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());

    let path = fixture.root.join(&ledger);
    let mut lines: Vec<Value> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let last = lines.last_mut().unwrap();
    last["requestDigest"] = json!("f".repeat(64));
    last["governorEvent"]["requestDigest"] = last["requestDigest"].clone();
    refresh_event_hash(&mut last["governorEvent"]);
    refresh_event_hash(last);
    fs::write(
        &path,
        lines
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let forged = fixture.call(&["run", "inspect", &ledger]);
    assert!(
        !forged.status.success(),
        "forged request digest was accepted"
    );
    assert!(String::from_utf8_lossy(&forged.stderr).contains("request digest"));

    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject duplicate request identity history",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());
    let path = fixture.root.join(&ledger);
    let mut lines: Vec<Value> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut duplicate = lines.last().unwrap().clone();
    duplicate["previousEventSha256"] = lines.last().unwrap()["eventSha256"].clone();
    refresh_event_hash(&mut duplicate);
    lines.push(duplicate);
    fs::write(
        &path,
        lines
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let duplicate = fixture.call(&["run", "inspect", &ledger]);
    assert!(
        !duplicate.status.success(),
        "duplicate request identity was accepted"
    );
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("duplicate governor request"));
}

#[test]
fn legacy_permit_without_request_id_keeps_historical_event_shape() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preserve legacy mutation permits",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
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
    let value: Value = serde_json::from_slice(&permit.stdout).unwrap();
    assert!(value["event"]["requestId"].is_null());
    assert!(value["event"]["requestDigest"].is_null());
    assert!(value["event"]["governorEvent"]["requestId"].is_null());
    assert_eq!(value["governor"]["spent"], 1);
}

#[test]
fn public_pre_mutation_permit_persists_before_product_edit_and_refuses_fourth() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "consume pre-mutation units atomically",
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
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let worker = fixture.json(&["work", "next", &work]);
    let mut assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let mut tampered_assignment = assignment.clone();
    tampered_assignment.pop();
    tampered_assignment.push(if assignment.ends_with('0') { '1' } else { '0' });
    assert_ne!(tampered_assignment, assignment);
    let tampered = fixture.call(&[
        "work",
        "permit",
        &work,
        &tampered_assignment,
        "--operation",
        "edit",
    ]);
    assert!(
        !tampered.status.success(),
        "tampered assignment was accepted"
    );
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        0
    );
    for unit in 1..=4 {
        if unit == 3 {
            let replanned = fixture.call(&[
                "work",
                "replan",
                &work,
                &assignment,
                "--hypothesis",
                "new bounded hypothesis",
            ]);
            assert!(replanned.status.success(), "{replanned:?}");
            assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
                .as_str()
                .unwrap()
                .to_owned();
        }
        let before_permit = fs::read(fixture.root.join(&ledger)).unwrap();
        let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
        if unit < 4 {
            assert!(permit.status.success(), "{permit:?}");
            let value: Value = serde_json::from_slice(&permit.stdout).unwrap();
            assert_eq!(value["allowed"], true);
            let inspected = fixture.json(&["run", "inspect", &ledger]);
            assert_eq!(inspected["governor"]["spent"], unit);
            fs::write(
                fixture.root.join("product.txt"),
                format!("permitted product edit {unit}\n"),
            )
            .unwrap();
            let resumed = fixture.json(&["work", "resume"]);
            assert_eq!(resumed["status"], "resumed");
        } else {
            assert!(!permit.status.success(), "fourth permit bypassed the bound");
            assert_eq!(
                fs::read(fixture.root.join("product.txt")).unwrap(),
                b"permitted product edit 3\n"
            );
            assert_ne!(fs::read(fixture.root.join(&ledger)).unwrap(), before_permit);
            let inspected = fixture.json(&["run", "inspect", &ledger]);
            assert_eq!(inspected["governor"]["spent"], 3);
            assert_eq!(inspected["governor"]["state"], "blocked");
        }
    }

    // The owning action refuses cross-run and malformed operation inputs
    // before it reaches the ledger append boundary.
    let other = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "cross-run permit refusal",
        "--check-command",
        "true",
    ]);
    let cross = fixture.call(&[
        "work",
        "permit",
        other["work"].as_str().unwrap(),
        &assignment,
        "--operation",
        "edit",
    ]);
    assert!(!cross.status.success());
    let malformed = fixture.call(&["work", "permit", &work, &assignment, "--operation", ""]);
    assert!(!malformed.status.success());
}

#[test]
fn pre_mutation_permits_are_not_double_counted_by_worker_completion() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "complete after cooperative mutation permits",
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
    let worker = fixture.json(&["work", "next", &work]);
    let mut assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );

    // Three independent CLI calls authorize product edits before any worker
    // completion. The third leaves the canonical governor at its hard bound.
    for unit in 1..=4 {
        if unit == 3 {
            let replanned = fixture.call(&[
                "work",
                "replan",
                &work,
                &assignment,
                "--hypothesis",
                "new bounded hypothesis",
            ]);
            assert!(replanned.status.success(), "{replanned:?}");
            assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
                .as_str()
                .unwrap()
                .to_owned();
        }
        let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
        if unit == 4 {
            assert!(!permit.status.success(), "fourth permit bypassed the bound");
            assert_eq!(
                fixture.json(&["run", "inspect", &ledger])["governor"]["state"],
                "blocked"
            );
            break;
        }
        assert!(permit.status.success(), "{permit:?}");
        let inspected = fixture.json(&["run", "inspect", &ledger]);
        assert_eq!(inspected["governor"]["spent"], unit);
        fs::write(
            fixture.root.join("product.txt"),
            format!("permitted completion edit {unit}\n"),
        )
        .unwrap();
        let resumed = fixture.json(&["work", "resume"]);
        assert_eq!(resumed["status"], "resumed");
    }

    // Once the fourth permit durably blocks the governor, the pending
    // completion cannot reopen or append another mutation.
    let artifacts_before = fixture.artifact_snapshot();
    let completed = fixture.return_body(&work, &assignment, "completed", b"worker completion\n");
    assert!(
        !completed.status.success(),
        "blocked completion was accepted"
    );
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 3);
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert_eq!(fixture.artifact_snapshot(), artifacts_before);
    let stale = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
    assert!(!stale.status.success(), "stale assignment was accepted");
}

fn granted_worker_completion() -> (Fixture, String, String) {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "fingerprint-changing completion",
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
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
    assert!(permit.status.success(), "{permit:?}");
    fs::write(fixture.root.join("product.txt"), b"fingerprint changed\n").unwrap();
    let resumed = fixture.json(&["work", "resume"]);
    assert_eq!(resumed["status"], "resumed");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let completed = fixture.return_body(&work, &assignment, "completed", b"worker result\n");
    assert!(completed.status.success(), "{completed:?}");
    (fixture, work, ledger)
}

#[test]
fn explicit_grant_binds_source_and_result_across_restart() {
    let (fixture, _work, ledger) = granted_worker_completion();
    let first = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(first["governor"]["spent"], 1);
    assert_eq!(
        first["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let replay = fixture.call(&["run", "inspect", &ledger]);
    assert!(replay.status.success(), "{replay:?}");
    let second: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(second["governor"]["spent"], 1);
    assert_eq!(
        second["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn discriminated_completion_rejects_missing_and_tampered_authorization() {
    for variant in [
        "payload",
        "ack",
        "mode",
        "nested-input",
        "unknown",
        "duplicate",
    ] {
        let (fixture, _work, ledger) = granted_worker_completion();
        fixture.rewrite_last(&ledger, |event| {
            if variant == "payload" {
                event.as_object_mut().unwrap().remove("governorEvent");
                return;
            }
            let governor = event["governorEvent"].as_object_mut().unwrap();
            match variant {
                "ack" => {
                    governor.remove("grantEventSha256s");
                    governor.remove("sourceInputsSha256s");
                }
                "mode" => {
                    governor.remove("authorizationMode");
                }
                "nested-input" => {
                    governor.insert("inputSha256".into(), json!("0".repeat(64)));
                }
                "unknown" => {
                    governor["grantEventSha256s"] = json!(["0".repeat(64)]);
                }
                "duplicate" => {
                    let grant = governor["grantEventSha256s"][0].clone();
                    let source = governor["sourceInputsSha256s"][0].clone();
                    governor["grantEventSha256s"] = json!([grant.clone(), grant]);
                    governor["sourceInputsSha256s"] = json!([source.clone(), source]);
                }
                _ => unreachable!(),
            }
        });
        let refused = fixture.call(&["run", "inspect", &ledger]);
        assert!(!refused.status.success(), "tampered {variant} replayed");
    }
}

#[test]
fn nonmutation_evidence_then_implicit_completion_is_typed() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "implicit after evidence",
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
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let artifact = ".exitbind/artifacts/explicit-evidence.md";
    fs::write(fixture.root.join(artifact), b"evidence\n").unwrap();
    let evidenced = fixture.call(&[
        "work",
        "evidence",
        &work,
        &assignment,
        "--artifact",
        artifact,
        "--artifact-root",
        "state",
    ]);
    assert!(evidenced.status.success(), "{evidenced:?}");
    let evidenced: Value = serde_json::from_slice(&evidenced.stdout).unwrap();
    assert_no_transport_paths(&evidenced);
    assert_resolver_backed_references(&evidenced);
    let next = fixture.json(&["work", "next", &work]);
    assert_no_transport_paths(&next);
    let packet_path = fixture.root.join(".exitbind/evidence-residual.json");
    fs::write(&packet_path, serde_json::to_vec(&next["residual"]).unwrap()).unwrap();
    let validated = fixture.json(&[
        "work",
        "validate",
        &work,
        "--packet",
        packet_path.to_str().unwrap(),
    ]);
    assert_eq!(validated["result"], "usable");
    assert_no_transport_paths(&validated);
    let completed = fixture.return_body(&work, &assignment, "completed", b"implicit result\n");
    assert!(completed.status.success(), "{completed:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let lines = fs::read_to_string(fixture.root.join(ledger)).unwrap();
    let event: Value = serde_json::from_str(lines.lines().last().unwrap()).unwrap();
    assert_eq!(event["governorEvent"]["authorizationMode"], "implicit");
    assert_eq!(event["governorEvent"]["grantEventSha256s"], json!([]));
}

#[test]
fn pre_discriminator_v8_marker_remains_readable() {
    let fixture = Fixture::new();
    let ledger = ".exitbind/runs/work-historical-v8.jsonl";
    let started = fixture.call(&[
        "run",
        "start",
        "change",
        "--goal",
        "historical v8 marker",
        "--ledger",
        ledger,
        "--check-command",
        "true",
    ]);
    assert!(started.status.success(), "{started:?}");
    let path = fixture.root.join(ledger);
    let mut event: Value =
        serde_json::from_str(fs::read_to_string(&path).unwrap().lines().next().unwrap()).unwrap();
    event["governor"]
        .as_object_mut()
        .unwrap()
        .remove("noInformationLimit");
    event["governor"]
        .as_object_mut()
        .unwrap()
        .remove("postReplanLimit");
    event["governor"]
        .as_object_mut()
        .unwrap()
        .remove("grantProtocol");
    event.as_object_mut().unwrap().remove("eventSha256");
    let digest = Sha256::digest(serde_json::to_vec(&event).unwrap());
    let mut digest_hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut digest_hex, "{byte:02x}").unwrap();
    }
    event["eventSha256"] = json!(digest_hex);
    fs::write(
        path,
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();
    let lead_artifact = ".exitbind/artifacts/historical-lead.md";
    fs::write(fixture.root.join(lead_artifact), b"historical lead\n").unwrap();
    let scoped = fixture.call(&[
        "run",
        "submit",
        "lead",
        ledger,
        "--outcome",
        "scoped",
        "--artifact",
        lead_artifact,
        "--artifact-root",
        "state",
    ]);
    assert!(scoped.status.success(), "{scoped:?}");
    let next = fixture.json(&["run", "next", ledger]);
    assert_eq!(next["assignments"][0]["role"], "worker");
    let worker_artifact = ".exitbind/artifacts/historical-worker.md";
    fs::write(fixture.root.join(worker_artifact), b"historical worker\n").unwrap();
    let completed = fixture.call(&[
        "run",
        "submit",
        "worker",
        ledger,
        "--outcome",
        "completed",
        "--artifact",
        worker_artifact,
        "--artifact-root",
        "state",
    ]);
    assert!(completed.status.success(), "{completed:?}");
    let inspected = fixture.call(&["run", "inspect", ledger]);
    assert!(inspected.status.success(), "{inspected:?}");
}

#[test]
fn lead_permit_is_refused_without_a_ledger_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "lead permit is not a worker grant",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap();
    let assignment = started["next"]["assignment"].as_str().unwrap();
    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let before = fs::read(&ledger).unwrap();
    let refused = fixture.call(&["work", "permit", work, assignment, "--operation", "edit"]);
    assert!(!refused.status.success());
    assert_eq!(fs::read(&ledger).unwrap(), before);
}

#[test]
fn stale_assignment_and_unknown_outcome_refuse_before_artifact_or_ledger_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject invalid worker returns",
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
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut stale = assignment.clone();
    stale.push('x');
    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let before = fs::read(&ledger).unwrap();
    let artifacts = fixture.artifact_snapshot();
    let refused = fixture.return_body(&work, &stale, "completed", b"must not persist\n");
    assert!(!refused.status.success(), "stale assignment was accepted");
    assert_eq!(fs::read(&ledger).unwrap(), before);
    assert_eq!(fixture.artifact_snapshot(), artifacts);

    let refused = fixture.return_body(&work, &assignment, "unknown", b"must not persist\n");
    assert!(!refused.status.success(), "unknown outcome was accepted");
    assert_eq!(fs::read(&ledger).unwrap(), before);
    assert_eq!(fixture.artifact_snapshot(), artifacts);
}

#[test]
fn conservative_completion_holds_before_artifact_or_append() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "conservative completion keeps worker bytes without appending",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    for unit in 1..=2 {
        assert!(fixture
            .call(&["work", "permit", &work, &assignment, "--operation", "edit"])
            .status
            .success());
        fs::write(
            fixture.root.join("product.txt"),
            format!("grant edit {unit}\n"),
        )
        .unwrap();
        assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    }
    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let before = fs::read(&ledger).unwrap();
    let artifacts_before = fixture.artifact_snapshot();
    let response = fixture.return_body(&work, &assignment, "completed", b"held worker result\n");
    assert!(response.status.success(), "{response:?}");
    let held = fixture.json(&["work", "next", &work]);
    assert!(held["next"]["held"]["reference"].is_string());
    assert_eq!(fs::read(&ledger).unwrap(), before);
    assert_eq!(fixture.artifact_snapshot(), artifacts_before);
}

#[test]
fn multiple_exact_grants_are_acknowledged_once_and_survive_replay() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "acknowledge exact grants",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    for unit in 1..=2 {
        assert!(fixture
            .call(&["work", "permit", &work, &assignment, "--operation", "edit"])
            .status
            .success());
        fs::write(
            fixture.root.join("product.txt"),
            format!("grant edit {unit}\n"),
        )
        .unwrap();
        assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    }
    let replanned = fixture.call(&[
        "work",
        "replan",
        &work,
        &assignment,
        "--hypothesis",
        "same worker, new plan",
    ]);
    assert!(replanned.status.success(), "{replanned:?}");
    let completed = fixture.return_body(&work, &assignment, "completed", b"worker\n");
    assert!(completed.status.success(), "{completed:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 2);
    assert_eq!(
        inspected["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let resumed = fixture.json(&["work", "resume"]);
    assert_eq!(resumed["status"], "resumed");
    let replayed = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(
        replayed["governor"]["consumedGrants"],
        inspected["governor"]["consumedGrants"]
    );
}
