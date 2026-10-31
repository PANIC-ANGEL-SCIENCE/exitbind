use super::*;

#[test]
fn opaque_oracle_binds_public_operands_to_the_fixture_and_their_roles() {
    let fixture = Fixture::new_single();
    let other = Fixture::new_single();
    let executable = fs::canonicalize(env!("CARGO_BIN_EXE_exitbind")).unwrap();
    let config = fs::canonicalize(fixture.root.join("exitbind.json")).unwrap();
    let other_config = fs::canonicalize(other.root.join("exitbind.json")).unwrap();
    assert!(
        serde_json::from_slice::<Value>(&fs::read(&other_config).unwrap())
            .unwrap()
            .is_object()
    );
    let exe = executable.to_str().unwrap();
    let cfg = config.to_str().unwrap();
    let valid = serde_json::json!({"effectiveAction": {"detail": {"command":
        [exe, "work", "detail", "smw_example", "--config", cfg]}}});
    assert_opaque_envelope(&valid, &fixture.root);

    // Bounded forms can omit either invocation operand or the entire route.
    for route in [
        serde_json::json!([exe, "work", "detail", "smw_example", "--config"]),
        serde_json::json!(["work", "detail", "smw_example", "--config", cfg]),
        serde_json::json!(["work", "detail", "smw_example", "--config"]),
        Value::Null,
    ] {
        let mut bounded = valid.clone();
        bounded["effectiveAction"]["detail"]["command"] = route;
        assert_opaque_envelope(&bounded, &fixture.root);
    }

    let rejects = |label: &str, value: &Value, expected: &str| {
        let error = std::panic::catch_unwind(|| assert_opaque_envelope(value, &fixture.root))
            .expect_err(label);
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(message.contains(expected), "{label}: {message}");
    };
    let mut forged_executable = valid.clone();
    forged_executable["effectiveAction"]["detail"]["command"][0] = serde_json::json!("/bin/sh");
    assert!(std::path::Path::new("/bin/sh").is_file());
    rejects(
        "different existing executable",
        &forged_executable,
        "command executable differs",
    );
    let alias_text = format!(
        "{}/./{}",
        executable.parent().unwrap().display(),
        executable.file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(fs::canonicalize(&alias_text).unwrap(), executable);
    let mut spelling_alias = valid.clone();
    spelling_alias["effectiveAction"]["detail"]["command"][0] = serde_json::json!(alias_text);
    rejects(
        "different executable spelling",
        &spelling_alias,
        "command executable differs",
    );

    let mut forged_config = valid.clone();
    forged_config["effectiveAction"]["detail"]["command"][5] = serde_json::json!(other_config);
    rejects(
        "different valid configuration",
        &forged_config,
        "command config differs",
    );

    let mut misplaced = valid.clone();
    misplaced["effectiveAction"]["detail"]["command"][1] = serde_json::json!(exe);
    rejects(
        "copied executable in another argument",
        &misplaced,
        "misplaced invocation operand",
    );
    let mut neighbor = valid.clone();
    neighbor["effectiveAction"]["detail"]["unknown"] = serde_json::json!(cfg);
    rejects(
        "configuration copied to unknown field",
        &neighbor,
        "misplaced invocation operand",
    );
    let object_index = serde_json::json!({"effectiveAction":{"detail":{"command":{"0":exe}}}});
    rejects(
        "object key cannot impersonate index",
        &object_index,
        "misplaced invocation operand",
    );
    let slash_key = serde_json::json!({"effectiveAction/detail/command":[exe]});
    rejects(
        "slash key cannot impersonate path",
        &slash_key,
        "misplaced invocation operand",
    );
    let mut unknown = valid.clone();
    unknown["effectiveAction"]["detail"]["unknown"] = serde_json::json!("state/hidden");
    rejects("neighboring private marker", &unknown, "leaked state/");
    let warning = serde_json::json!({"effectiveAction":{"warnings":[{"nextAction":{"command":[exe,"work","detail","smw_example","--config",cfg]}}]}});
    assert_opaque_envelope(&warning, &fixture.root);
    let mut warning_neighbor = warning;
    warning_neighbor["effectiveAction"]["warnings"][0]["nextAction"]["unknown"] =
        serde_json::json!(cfg);
    rejects(
        "unknown warning neighbor",
        &warning_neighbor,
        "misplaced invocation operand",
    );

    let private = fixture.root.join(".exitbind");
    fs::create_dir_all(&private).unwrap();
    fs::write(private.join("hidden"), b"private").unwrap();
    let alias = fixture.root.join("public-alias");
    std::os::unix::fs::symlink(&private, &alias).unwrap();
    let mut private_path = valid.clone();
    private_path["unknown"] = serde_json::json!(private.join("hidden"));
    rejects("direct private root", &private_path, "exposed private path");
    let mut private_alias = valid.clone();
    private_alias["unknown"] = serde_json::json!(alias.join("hidden"));
    rejects(
        "symlink into private root",
        &private_alias,
        "exposed private path",
    );
    let mut missing_child = valid.clone();
    missing_child["unknown"] = serde_json::json!(alias.join("not-created"));
    rejects(
        "missing child through private alias",
        &missing_child,
        "exposed private path",
    );
    let dangling = fixture.root.join("dangling-alias");
    std::os::unix::fs::symlink(private.join("not-created"), &dangling).unwrap();
    let mut dangling_alias = valid.clone();
    dangling_alias["unknown"] = serde_json::json!(dangling);
    rejects(
        "dangling alias into private root",
        &dangling_alias,
        "exposed private path",
    );
    let cycle_a = fixture.root.join("cycle-a");
    let cycle_b = fixture.root.join("cycle-b");
    std::os::unix::fs::symlink("cycle-b", &cycle_a).unwrap();
    std::os::unix::fs::symlink("cycle-a", cycle_b).unwrap();
    let mut cycle = valid;
    cycle["unknown"] = serde_json::json!(cycle_a);
    rejects("unresolved alias loop", &cycle, "unresolved alias");
}

#[test]
fn opaque_oracle_accepts_real_bounded_producer_routes() {
    const ROUTE_BUDGET: usize = 1024;
    const MAC_PATH_LIMIT: usize = 1024;
    const STATE_PATH_HEADROOM: usize = 256;
    const STATE_FILENAME_HEADROOM: usize = 64;
    let base = support::temp("opaque-route-fallback");
    let mut root = base.join("state");
    // Quotes cost one filesystem byte and two JSON bytes. Pad only as much as
    // this actual config operand needs to exceed the serialized route budget.
    let config_json_bytes = loop {
        let config = root.join("exitbind.json");
        let bytes = serde_json::to_vec(config.to_str().expect("UTF-8 config path"))
            .unwrap()
            .len();
        if bytes > ROUTE_BUDGET {
            break bytes;
        }
        let quotes = ((ROUTE_BUDGET + 2 - bytes) / 2).min(200);
        assert!(
            (1..255).contains(&quotes),
            "invalid component size: {quotes}"
        );
        root.push("\"".repeat(quotes));
    };
    let config = root.join("exitbind.json");
    let root_bytes = root.as_os_str().len();
    let config_bytes = config.as_os_str().len();
    assert!(
        config_json_bytes > ROUTE_BUDGET,
        "config JSON: {config_json_bytes} B"
    );
    assert!(
        root_bytes + STATE_PATH_HEADROOM < MAC_PATH_LIMIT && config_bytes < MAC_PATH_LIMIT,
        "fixture paths leave insufficient Mac PATH_MAX headroom: root={root_bytes} B, \
         config={config_bytes} B, config JSON={config_json_bytes} B, \
         state reserve={STATE_PATH_HEADROOM} B, base={}",
        base.display()
    );
    eprintln!("route fixture: root={root_bytes} B, config={config_bytes} B, config JSON={config_json_bytes} B");
    fs::create_dir_all(&root).unwrap();
    let executable = env!("CARGO_BIN_EXE_exitbind");
    let init = support::git_topology::command(executable)
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let call = |args: &[&str], input: Option<&[u8]>| {
        let mut command = support::git_topology::command(executable);
        command
            .current_dir(&root)
            .args(args)
            .arg("--config")
            .arg(&config);
        let output = if let Some(input) = input {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        };
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let started = call(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "bounded route",
            "--check-command",
            "true",
        ],
        None,
    );
    assert_opaque_envelope(&started, &root);
    let work = started["work"].as_str().unwrap();
    let assignment = started["next"]["assignment"].as_str().unwrap();
    let scoped = call(
        &["work", "return", work, assignment, "--outcome", "scoped"],
        Some(b"scope"),
    );
    assert_opaque_envelope(&scoped, &root);
    assert_eq!(scoped["continuation"]["sameConfigRequired"], true,
        "fixture must exercise the producer's bounded continuation route; config {config_bytes} filesystem bytes, {config_json_bytes} JSON bytes; scoped={scoped}");
    assert!(scoped["continuation"]["command"].is_array(), "{scoped}");

    let mut state_paths = vec![root.join(".exitbind")];
    let mut longest_state_path = 0;
    while let Some(path) = state_paths.pop() {
        let bytes = path.as_os_str().len();
        longest_state_path = longest_state_path.max(bytes);
        assert!(
            bytes + STATE_FILENAME_HEADROOM < MAC_PATH_LIMIT,
            "generated state path is {bytes} B with {STATE_FILENAME_HEADROOM} B reserved: {}",
            path.display()
        );
        if fs::symlink_metadata(&path).unwrap().is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                state_paths.push(entry.unwrap().path());
            }
        }
    }
    eprintln!("longest generated state path: {longest_state_path} B");
}

#[test]
fn successful_work_envelopes_are_opaque_and_expandable() {
    let fixture = Fixture::new_single();
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "opaque successful envelope",
            "--check-command",
            "printf out; printf err >&2",
        ],
        None,
    );
    assert_opaque_envelope(&started, &fixture.root);
    let work = started["work"].as_str().unwrap().to_owned();
    let history_id = started["next"]["packet"]["context"]["expansions"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let expanded_history = fixture.call(&["work", "expand", &work, &history_id], None);
    assert!(expanded_history.status.success(), "{expanded_history:?}");

    let scoped = fixture.value(
        &[
            "work",
            "return",
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    assert_opaque_envelope(&scoped, &fixture.root);
    let worker = fixture.value(&["work", "next", &work], None);
    assert_opaque_envelope(&worker, &fixture.root);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    assert_opaque_envelope(&completed, &fixture.root);
    let checked = fixture.value(&["work", "check", &work], None);
    assert_opaque_envelope(&checked, &fixture.root);
    let current = fixture.value(&["work", "next", &work], None);
    let log_id = current["next"]["packet"]["context"]["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("checkLogRef"))
        .and_then(|reference| reference["id"].as_str())
        .unwrap()
        .to_owned();
    let expanded_log = fixture.call(&["work", "expand", &work, &log_id], None);
    assert!(expanded_log.status.success(), "{expanded_log:?}");
    let expanded_log: Value = serde_json::from_slice(&expanded_log.stdout).unwrap();
    assert_eq!(expanded_log["stdout"]["contentHex"], "6f7574");
    assert_eq!(expanded_log["stderr"]["contentHex"], "657272");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_opaque_envelope(&resumed, &fixture.root);
    let packet_path = fixture.root.join(".exitbind/residual.json");
    fs::write(
        &packet_path,
        serde_json::to_vec(&resumed["residual"]).unwrap(),
    )
    .unwrap();
    let packet_path = packet_path.to_str().unwrap();
    let validated = fixture.value(&["work", "validate", &work, "--packet", packet_path], None);
    assert_opaque_envelope(&validated, &fixture.root);
    assert_eq!(validated["result"], "usable");
}

#[test]
fn work_discovery_diagnostics_are_table_driven_and_fail_closed() {
    for case in [
        "drift",
        "artifact_drift",
        "memory_drift",
        "profile_drift",
        "ambiguity",
        "corruption",
        "explicit",
        "stale",
    ] {
        match case {
            "drift" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                // The strict config warning below is the legacy contract.
                support::unmark_work(&fixture.root, work);
                let ledger = fixture.ledger(work);
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                let config = fixture.root.join("exitbind.json");
                let mut bytes = fs::read(&config).unwrap();
                bytes.push(b'\n');
                fs::write(&config, bytes).unwrap();
                let output = fixture.call(&["work", "next", work, "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "config_drift"
                );
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value, &fixture.root);
            }
            "artifact_drift" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic artifact drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap().to_owned();
                fixture.value(
                    &[
                        "work",
                        "return",
                        &work,
                        begin["next"]["assignment"].as_str().unwrap(),
                        "--outcome",
                        "scoped",
                    ],
                    Some(b"scope"),
                );
                let worker = fixture.value(&["work", "next", &work], None);
                fixture.value(
                    &[
                        "work",
                        "return",
                        &work,
                        worker["next"]["assignment"].as_str().unwrap(),
                        "--outcome",
                        "completed",
                    ],
                    Some(b"worker result"),
                );
                let ledger = fixture.ledger(&work);
                let artifact = fs::read_to_string(fixture.root.join(&ledger))
                    .unwrap()
                    .lines()
                    .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                    .find(|event| event["action"] == "submit" && event["role"] == "worker")
                    .and_then(|event| event["artifact"]["path"].as_str().map(str::to_owned))
                    .unwrap();
                fs::write(fixture.root.join(&artifact), b"changed artifact").unwrap();
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                let output = fixture.call(&["work", "next", &work, "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "artifact_drift"
                );
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value, &fixture.root);
            }
            "memory_drift" => {
                let fixture = Fixture::new_single();
                let config_path = fixture.root.join("exitbind.json");
                let mut config: Value =
                    serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
                config["memory"] = serde_json::json!({
                    "root": ".exitbind/memory",
                    "maxItems": 8,
                    "maxBytes": 32768,
                    "protocolScopes": ["invariants"],
                    "syntheticScopes": ["synthetic"]
                });
                config["agents"]["lead"]["memoryRead"] = serde_json::json!(["invariants"]);
                config["agents"]["lead"]["crossContext"] = serde_json::json!("protocol-only");
                config["agents"]["worker"]["memoryWrite"] = serde_json::json!(["invariants"]);
                config["agents"]["worker"]["memoryReview"] = serde_json::json!(["invariants"]);
                config["agents"]["lead"]["memoryPromote"] = serde_json::json!(["invariants"]);
                fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
                fs::write(fixture.root.join("memory.md"), b"accepted invariant\n").unwrap();
                fs::create_dir_all(fixture.root.join(".exitbind/memory")).unwrap();
                for args in [
                    vec![
                        "memory",
                        "propose",
                        "worker",
                        "memory.md",
                        "--scope",
                        "invariants",
                        "--ledger",
                        ".exitbind/memory/invariant.jsonl",
                    ],
                    vec![
                        "memory",
                        "review",
                        "worker",
                        ".exitbind/memory/invariant.jsonl",
                    ],
                    vec![
                        "memory",
                        "promote",
                        "lead",
                        ".exitbind/memory/invariant.jsonl",
                    ],
                ] {
                    let output = fixture.call(&args, None);
                    assert!(output.status.success(), "{args:?}: {output:?}");
                }
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic memory drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap().to_owned();
                // The memory_drift warning below is the legacy contract.
                support::unmark_work(&fixture.root, &work);
                let ledger = fixture.ledger(&work);
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                fs::write(fixture.root.join("memory.md"), b"changed memory\n").unwrap();
                let output = fixture.call(&["work", "next", &work, "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "memory_drift"
                );
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value, &fixture.root);
            }
            "profile_drift" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic profile drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                // The profile_drift warning below is the legacy contract.
                support::unmark_work(&fixture.root, work);
                let config: Value =
                    serde_json::from_slice(&fs::read(fixture.root.join("exitbind.json")).unwrap())
                        .unwrap();
                let profile = config["agents"]["lead"]["profile"].as_str().unwrap();
                fs::remove_file(fixture.root.join(profile)).unwrap();
                let ledger = fixture.ledger(work);
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                let output = fixture.call(&["work", "resume", "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["status"], "resumed");
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "profile_drift"
                );
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value, &fixture.root);
            }
            "ambiguity" => {
                let fixture = Fixture::new_single();
                let first = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic one",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let second = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic two",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let first = fixture.pin_work(
                    first["work"].as_str().unwrap(),
                    "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                );
                let second = fixture.pin_work(
                    second["work"].as_str().unwrap(),
                    "smw_0000000000000000000000000000000000000000000000000000000000000001",
                );
                let ledgers = [fixture.ledger(&first), fixture.ledger(&second)];
                // Diagnose the legacy discovery path: no current-work focus.
                fs::remove_file(fixture.root.join(".exitbind/current-work.json")).unwrap();
                let before = ledgers
                    .iter()
                    .map(|ledger| fs::read(fixture.root.join(ledger)).unwrap())
                    .collect::<Vec<_>>();
                let value = fixture.value(&["work", "resume", "--json"], None);
                let repeat = fixture.value(&["work", "resume", "--json"], None);
                assert_eq!(value.to_string(), repeat.to_string());
                assert_eq!(value["status"], "ambiguous");
                assert_eq!(value["reason"]["code"], "ambiguous_candidates");
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "choose_explicit_handle");
                assert_eq!(value["nextAction"]["safe"], true);
                let mut expected = vec![serde_json::json!(first), serde_json::json!(second)];
                expected.sort_by_key(Value::to_string);
                assert_eq!(
                    value["works"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|candidate| candidate["work"].clone())
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_opaque_reference(
                    &value["reference"],
                    serde_json::json!({"works": expected}),
                );
                assert_opaque_envelope(&value, &fixture.root);
                for (ledger, expected) in ledgers.iter().zip(before) {
                    assert_eq!(fs::read(fixture.root.join(ledger)).unwrap(), expected);
                }
            }
            "corruption" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic corruption",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                let ledger = fixture.ledger(work);
                fs::write(fixture.root.join(&ledger), b"not-json\n").unwrap();
                let output = fixture.call(&["work", "next", work, "--json"], None);
                assert_eq!(output.status.code(), Some(1));
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "corrupt_ledger");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "inspect");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_opaque_envelope(&value, &fixture.root);
            }
            "explicit" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic explicit",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                let value = fixture.value(&["work", "next", work, "--json"], None);
                assert_eq!(value["reason"]["code"], "explicit_handle");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_opaque_envelope(&value, &fixture.root);
            }
            "stale" => {
                let fixture = Fixture::new_single();
                let work = "smw_0000000000000000000000000000000000000000000000000000000000000000";
                let output = fixture.call(&["work", "next", work, "--json"], None);
                assert_eq!(output.status.code(), Some(1));
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "stale_handle");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "inspect");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_opaque_envelope(&value, &fixture.root);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn marked_no_op_config_edit_does_not_invalidate_the_work() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "marked diagnostic drift",
            "--check-command",
            "true",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap();
    let ledger = fixture.ledger(work);
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let config = fixture.root.join("exitbind.json");
    let mut bytes = fs::read(&config).unwrap();
    bytes.push(b'\n');
    fs::write(&config, bytes).unwrap();
    let output = fixture.call(&["work", "next", work, "--json"], None);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["effect"], "no-change");
    assert!(
        !value["next"]["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|warning| warning["classification"] == "config_drift"),
        "{value}"
    );
    assert_eq!(value["nextAction"]["type"], "continue");
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}
