use super::*;
use sha2::{Digest, Sha256};

fn native_journal(project: &Project) -> PathBuf {
    fn visit(root: &Path) -> Option<PathBuf> {
        for entry in fs::read_dir(root).ok()? {
            let path = entry.ok()?.path();
            if path
                .file_name()
                .is_some_and(|name| name == "session-goal.jsonl")
            {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = visit(&path) {
                    return Some(found);
                }
            }
        }
        None
    }
    let store = &project
        .native_session
        .as_ref()
        .expect("native test project")
        .1;
    visit(store).expect("fixture native Goal journal")
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&object[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn reseal_latest_record(project: &Project, mutate: impl FnOnce(&mut Value)) {
    let path = native_journal(project);
    let mut records = support::goal_journal::records(&path);
    let record = records.last_mut().expect("fixture Goal record");
    mutate(record);
    record.as_object_mut().unwrap().remove("eventSha256");
    let digest = format!("{:x}", Sha256::digest(canonical(record).as_bytes()));
    record["eventSha256"] = json!(digest);
    support::goal_journal::write(&path, &records);
}

fn owner_and_source(label: &str) -> (Project, Project) {
    let owner = Project::new(label).with_native_session(&format!("{label}-session"));
    incorporate_native_generic_goal(
        &owner,
        "original-conversation",
        "Complete the original conversation work with current accepted evidence",
    );
    let mut source = Project::new(&format!("{label}-source"));
    source.native_session = owner.native_session.clone();
    owner.file("owner-check.sh", "set -eu\ntest -f owner-input.txt\n");
    source.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    source.file("source-input.txt", "original source evidence\n");
    (owner, source)
}

fn begin_in(project: &Project, config: &Path, goal: &str, check: &str, extra: &[&str]) -> String {
    let mut args = vec![
        "work",
        "begin",
        "change",
        "--goal",
        goal,
        "--check-command",
        check,
        "--review-policy",
        "required",
    ];
    args.extend_from_slice(extra);
    project.value_with_config(&args, None, config)["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn recovery_preview(owner: &Project, source: &Project, historical: &str, request: &str) -> Value {
    owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            historical,
            "--request-id",
            request,
            "--reason",
            "Retain the verified historical fixture without current credit",
        ],
        None,
    )
}

fn group_preview(owner: &Project, source: &Project, request: &str) -> Value {
    owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--upsert-group",
            "--request-id",
            request,
            "--reason",
            "Preserve this exact selected source group",
        ],
        None,
    )
}

fn apply_recovery(owner: &Project, preview: &Value) -> Value {
    let argv = preview["applyArgv"]
        .as_array()
        .expect("preview emits exact apply argv");
    let args = argv
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(
        args.len(),
        argv.len(),
        "every exact argv element is a string"
    );
    assert_eq!(
        Path::new(&args[0]),
        Path::new(env!("CARGO_BIN_EXE_exitbind"))
    );
    let output = owner.call_exact_argv(&args);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn source_member(source: &Project) -> String {
    let config = source.root.join("exitbind.json");
    let work = begin_in(
        source,
        &config,
        "Validate source evidence",
        "sh check.sh",
        &[],
    );
    source.drive_with_config(&work, true, &config);
    work
}

fn blocked_source_member(source: &Project, config: &Path) -> String {
    let blocked = begin_in(
        source,
        config,
        "Abandoned observer direction",
        "sh check.sh",
        &[],
    );
    let mut next = source.value_with_config(
        &["work", "next", &blocked, "--json", "--full"],
        None,
        config,
    );
    while next["next"]["action"] == "lead_decision" {
        source.return_stage_with_config(&blocked, &next["next"], "scoped", config);
        next = source.value_with_config(
            &["work", "next", &blocked, "--json", "--full"],
            None,
            config,
        );
    }
    assert_eq!(next["next"]["role"], "worker");
    source.value_with_config(
        &[
            "work",
            "return",
            &blocked,
            next["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some("This distinct observer direction is blocked; it is not accepted.\n"),
        config,
    );
    blocked
}

fn work_ledger(source: &Project, work: &str) -> PathBuf {
    source
        .root
        .join(".exitbind/runs")
        .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()))
}

fn retirement_request(
    owner: &Project,
    config: &Path,
    work: &str,
    replacement: &str,
    request_id: &str,
) -> Output {
    let selection = json!([{"work":work,"reason":"This direction was explicitly abandoned",
        "replacement":replacement}])
    .to_string();
    owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            config.to_str().unwrap(),
            "--retirements",
            &selection,
            "--source-writers-quiesced",
            "--request-id",
            request_id,
            "--reason",
            "Retain the original Work without accepted credit",
            "--json",
        ],
        None,
    )
}

fn assert_json_refusal(output: &Output, cause: &str) {
    assert!(!output.status.success(), "{output:?}");
    let response: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON refusal: {error}: {output:?}"));
    let diagnostic = response["error"].as_str().expect("JSON refusal error");
    assert!(diagnostic.contains(cause), "{diagnostic}");
}

fn assert_retirement_refused_without_changes(
    owner: &Project,
    source: &Project,
    config: &Path,
    work: &str,
    replacement: &str,
    request_id: &str,
    cause: &str,
) {
    let goal_path = native_journal(owner);
    let work_path = work_ledger(source, work);
    let before_goal = fs::read(&goal_path).unwrap();
    let before_work = fs::read(&work_path).unwrap();
    let before = owner.status();
    assert!(before["eventSha256"].is_string());
    assert!(before["journeyMembers"].is_array());
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let before_run = source.value_with_config(&["run", "inspect", &ledger, "--json"], None, config);
    assert_eq!(before_run["status"], "blocked");
    let refused = retirement_request(owner, config, work, replacement, request_id);
    assert_json_refusal(&refused, cause);
    assert_eq!(fs::read(&goal_path).unwrap(), before_goal);
    assert_eq!(fs::read(&work_path).unwrap(), before_work);
    let after = owner.status();
    assert_eq!(after["eventSha256"], before["eventSha256"]);
    assert_eq!(after["journeyMembers"], before["journeyMembers"]);
    let after_run = source.value_with_config(&["run", "inspect", &ledger, "--json"], None, config);
    assert_eq!(after_run["governor"], before_run["governor"]);
    assert_eq!(fs::read(goal_path).unwrap(), before_goal);
    assert_eq!(fs::read(work_path).unwrap(), before_work);
}

fn accept_recovery_integration(source: &Project, config: &Path) -> String {
    let integration = begin_in(
        source,
        config,
        "Integrate the exact recovered Source journey",
        "sh check.sh",
        &["--artifact", "check.sh", "--journey-integration"],
    );
    source.drive_with_config(&integration, true, config);
    integration
}

fn integration_header(source: &Project, config: &Path, work: &str) -> Value {
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let inspection = source.value_with_config(&["run", "inspect", &ledger, "--json"], None, config);
    let constraints = inspection["events"][0]["basis"]["constraints"]
        .as_array()
        .unwrap();
    let marker = constraints
        .iter()
        .filter_map(Value::as_str)
        .find_map(|item| item.strip_prefix("exitbind.session-journey-integration.v1.header:"))
        .expect("integration has its frozen header");
    serde_json::from_str(marker).unwrap()
}

#[test]
fn explicit_group_upserts_keep_independent_integrations_and_block_a_pending_member() {
    let (owner, first) = owner_and_source("native-recovery-multiple-groups");
    let first_work = source_member(&first);
    let mut second = Project::new("native-recovery-multiple-second");
    second.native_session = owner.native_session.clone();
    second.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    second.file("source-input.txt", "second source evidence\n");
    let second_work = source_member(&second);
    let mut pending = Project::new("native-recovery-multiple-pending");
    pending.native_session = owner.native_session.clone();
    pending.file("check.sh", "set -eu\ntrue\n");
    let pending_config = pending.root.join("exitbind.json");
    let pending_work = begin_in(
        &pending,
        &pending_config,
        "Pending source",
        "sh check.sh",
        &[],
    );

    let first_preview = group_preview(&owner, &first, "first-group");
    let first_apply = first_preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let first_event = apply_recovery(&owner, &first_preview)["event"].clone();
    assert_eq!(first_event["nativeGoalRecovery"]["version"], 2);
    let first_binding = first_event["nativeGoalRecovery"]["active"]["decisionSha256"].clone();
    let before_integration = owner.status();
    assert_eq!(
        before_integration["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    let first_integration = accept_recovery_integration(&first, &first.root.join("exitbind.json"));
    let header = integration_header(
        &first,
        &first.root.join("exitbind.json"),
        &first_integration,
    );
    assert_eq!(header["v"], 3);
    assert_eq!(header["r"], first_binding);
    assert_eq!(header["g"].as_str().unwrap().len(), 64);

    let second_preview = group_preview(&owner, &second, "second-group");
    assert_eq!(second_preview["mode"], "group-upsert");
    assert_eq!(
        second_preview["preservedGroups"][0]["decisionSha256"],
        first_binding
    );
    assert!(second_preview["ownerConfig"]["configSha256"].is_string());
    let second_event = apply_recovery(&owner, &second_preview)["event"].clone();
    assert_eq!(
        second_event["nativeGoalRecovery"]["active"]["decisionSha256"],
        first_binding
    );
    assert_eq!(
        second_event["nativeGoalRecovery"]["groups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let after_second = owner.status();
    let journey = &after_second["goalProgress"]["journeyCompletion"];
    let unresolved = journey["unresolved"].as_array().unwrap();
    assert!(
        !unresolved.iter().any(|item| item["work"] == first_work),
        "{journey}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == second_work),
        "{journey}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == pending_work),
        "{journey}"
    );

    let before_replay = fs::read(native_journal(&owner)).unwrap();
    let replay = owner.call_exact_argv(&first_apply);
    assert!(replay.status.success(), "{replay:?}");
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["kind"], "native_goal_recovery_replayed");
    assert_eq!(replay["event"], first_event);
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_replay);

    let second_integration =
        accept_recovery_integration(&second, &second.root.join("exitbind.json"));
    let current = owner.status();
    let journey = &current["goalProgress"]["journeyCompletion"];
    let unresolved = journey["unresolved"].as_array().unwrap();
    assert!(
        !unresolved
            .iter()
            .any(|item| item["work"] == first_work || item["work"] == second_work),
        "{journey}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == pending_work),
        "{journey}"
    );
    assert_eq!(journey["state"], "blocked");
    let close = owner.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &second_integration,
        ],
        None,
    );
    assert!(
        !close.status.success(),
        "pending Source must still block closure"
    );
    assert_ne!(first_integration, second_integration);

    let second_reselection = group_preview(&owner, &second, "second-group-reselected");
    apply_recovery(&owner, &second_reselection);
    let reselected = owner.status();
    let unresolved = reselected["goalProgress"]["journeyCompletion"]["unresolved"]
        .as_array()
        .unwrap();
    assert!(
        !unresolved.iter().any(|item| item["work"] == first_work),
        "{reselected}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == second_work),
        "{reselected}"
    );
    accept_recovery_integration(&second, &second.root.join("exitbind.json"));

    let mut changed_request = first_apply.clone();
    let reason = changed_request
        .iter()
        .position(|item| item == "--reason")
        .unwrap();
    changed_request[reason + 1] = "A different selection under the same request".into();
    let before_conflict = fs::read(native_journal(&owner)).unwrap();
    assert!(!owner.call_exact_argv(&changed_request).status.success());
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_conflict);

    let alias = second.root.join("group-alias.json");
    fs::copy(second.root.join("exitbind.json"), &alias).unwrap();
    let before_alias = fs::read(native_journal(&owner)).unwrap();
    let alias_result = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            alias.to_str().unwrap(),
            "--upsert-group",
            "--request-id",
            "alias",
            "--reason",
            "Refuse a duplicate identity",
        ],
        None,
    );
    assert!(!alias_result.status.success());
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_alias);
    fs::remove_file(alias).unwrap();
    let after_alias = owner.status();
    let unresolved = after_alias["goalProgress"]["journeyCompletion"]["unresolved"]
        .as_array()
        .unwrap();
    assert!(
        !unresolved
            .iter()
            .any(|item| item["work"] == first_work || item["work"] == second_work),
        "{after_alias}"
    );

    let second_config = second.root.join("exitbind.json");
    let original_config = fs::read(&second_config).unwrap();
    let mut changed_config = original_config.clone();
    changed_config.push(b'\n');
    fs::write(&second_config, changed_config).unwrap();
    let stale_config = owner.status();
    let unresolved = stale_config["goalProgress"]["journeyCompletion"]["unresolved"]
        .as_array()
        .unwrap();
    assert!(
        !unresolved.iter().any(|item| item["work"] == first_work),
        "{stale_config}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == second_work),
        "{stale_config}"
    );
    fs::write(&second_config, original_config).unwrap();
    let restored = owner.status();
    let unresolved = restored["goalProgress"]["journeyCompletion"]["unresolved"]
        .as_array()
        .unwrap();
    assert!(
        !unresolved
            .iter()
            .any(|item| item["work"] == first_work || item["work"] == second_work),
        "{restored}"
    );

    let owner_config = owner.root.join("exitbind.json");
    let original_owner = fs::read(&owner_config).unwrap();
    let owner_preview = group_preview(&owner, &second, "owner-config-race");
    let mut changed_owner = original_owner.clone();
    changed_owner.push(b'\n');
    fs::write(&owner_config, changed_owner).unwrap();
    let changed = owner.status();
    let unresolved = changed["goalProgress"]["journeyCompletion"]["unresolved"]
        .as_array()
        .unwrap();
    assert!(
        unresolved.iter().any(|item| item["work"] == first_work),
        "{changed}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == second_work),
        "{changed}"
    );
    let before_owner_race = fs::read(native_journal(&owner)).unwrap();
    let owner_args = owner_preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(!owner.call_exact_argv(&owner_args).status.success());
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_owner_race);
    fs::write(owner_config, original_owner).unwrap();

    let stale_preview = group_preview(&owner, &second, "stale-group");
    let mut later = Project::new("native-recovery-multiple-later");
    later.native_session = owner.native_session.clone();
    later.file("check.sh", "set -eu\ntrue\n");
    let later_config = later.root.join("exitbind.json");
    begin_in(
        &later,
        &later_config,
        "A changed shared journey",
        "sh check.sh",
        &[],
    );
    let before_stale = fs::read(native_journal(&owner)).unwrap();
    let stale_args = stale_preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(!owner.call_exact_argv(&stale_args).status.success());
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_stale);
    let changed = owner.status();
    let unresolved = changed["goalProgress"]["journeyCompletion"]["unresolved"]
        .as_array()
        .unwrap();
    assert!(
        unresolved.iter().any(|item| item["work"] == first_work),
        "{changed}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == second_work),
        "{changed}"
    );
}

#[test]
fn explicit_v2_migration_keeps_a_current_v1_group_integration() {
    let (owner, first) = owner_and_source("native-recovery-v1-to-v2");
    let first_work = source_member(&first);
    let mut second = Project::new("native-recovery-v1-to-v2-second");
    second.native_session = owner.native_session.clone();
    second.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    second.file("source-input.txt", "second source evidence\n");
    let second_work = source_member(&second);
    let legacy = recovery_preview(&owner, &first, "", "legacy-first");
    let legacy_event = apply_recovery(&owner, &legacy)["event"].clone();
    assert_eq!(legacy_event["nativeGoalRecovery"]["version"], 1);
    let integration = accept_recovery_integration(&first, &first.root.join("exitbind.json"));
    let header = integration_header(&first, &first.root.join("exitbind.json"), &integration);
    assert_eq!(header["v"], 2);
    assert!(header.get("g").is_none());
    let before = owner.status();
    assert!(
        !before["goalProgress"]["journeyCompletion"]["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["work"] == first_work),
        "{before}"
    );
    let migration = group_preview(&owner, &second, "add-second");
    let applied = apply_recovery(&owner, &migration)["event"].clone();
    assert_eq!(applied["nativeGoalRecovery"]["version"], 2);
    assert_eq!(
        applied["nativeGoalRecovery"]["active"],
        legacy_event["nativeGoalRecovery"]["active"]
    );
    let current = owner.status();
    let completion = &current["goalProgress"]["journeyCompletion"];
    assert!(
        !completion["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["work"] == first_work),
        "{completion}"
    );
    assert!(
        completion["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["work"] == second_work),
        "{completion}"
    );
    assert_eq!(completion["integrationWork"], integration);
}

#[test]
fn concurrent_group_applies_share_the_goal_head_guard() {
    let (owner, first) = owner_and_source("native-recovery-group-race");
    source_member(&first);
    let mut second = Project::new("native-recovery-group-race-second");
    second.native_session = owner.native_session.clone();
    second.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    second.file("source-input.txt", "second source evidence\n");
    source_member(&second);
    let before = owner.status();
    let previews = [
        group_preview(&owner, &first, "race-first"),
        group_preview(&owner, &second, "race-second"),
    ];
    let args = previews.map(|preview| {
        preview["applyArgv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    });
    let outcomes = std::thread::scope(|scope| {
        let first = scope.spawn(|| owner.call_exact_argv(&args[0]));
        let second = scope.spawn(|| owner.call_exact_argv(&args[1]));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.status.success())
            .count(),
        1,
        "{outcomes:?}"
    );
    let after = owner.status();
    assert_eq!(
        after["revision"].as_u64(),
        Some(before["revision"].as_u64().unwrap() + 1)
    );
    assert_eq!(after["nativeGoalRecovery"]["version"], 2);
    assert!(after["nativeGoalRecovery"].get("groups").is_none());
    assert_eq!(
        after["nativeGoalRecovery"]["requests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn group_upsert_refuses_foreign_owner_and_wrong_endpoint_without_writes() {
    let (owner, source) = owner_and_source("native-recovery-group-refusals");
    let work = source_member(&source);
    let journal = native_journal(&owner);
    let before = fs::read(&journal).unwrap();
    let source_config = source.root.join("exitbind.json");
    let owner_config = owner.root.join("exitbind.json");
    let mut foreign = Project::new("native-recovery-group-foreign");
    foreign.native_session = owner.native_session.clone();
    let refused = foreign.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source_config.to_str().unwrap(),
            "--upsert-group",
            "--request-id",
            "foreign",
            "--reason",
            "Preserve the fixed owner",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(fs::read(&journal).unwrap(), before);
    let symlink = source.root.join("source-alias.json");
    std::os::unix::fs::symlink(&source_config, &symlink).unwrap();
    let refused = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            symlink.to_str().unwrap(),
            "--upsert-group",
            "--request-id",
            "path-alias",
            "--reason",
            "Use the exact source path",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(fs::read(&journal).unwrap(), before);
    fs::remove_file(symlink).unwrap();
    let mut selection = serde_json::Map::new();
    selection.insert(work, json!(owner_config.to_str().unwrap()));
    let selection = Value::Object(selection).to_string();
    let refused = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source_config.to_str().unwrap(),
            "--endpoint-configs",
            &selection,
            "--upsert-group",
            "--request-id",
            "wrong-endpoint",
            "--reason",
            "Preserve the exact endpoint",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(fs::read(&journal).unwrap(), before);
    let refused = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            owner_config.to_str().unwrap(),
            "--upsert-group",
            "--request-id",
            "owner-as-source",
            "--reason",
            "Refuse owner substitution",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(fs::read(journal).unwrap(), before);
}

#[test]
#[ignore = "requires an independently pinned pre-group recovery executable"]
fn old_producer_cannot_drop_v2_groups_or_claim_goal_readiness() {
    let old =
        std::env::var("EXITBIND_TEST_PREVIOUS_EXECUTABLE").expect("set the previous executable");
    let expected = std::env::var("EXITBIND_TEST_PREVIOUS_SHA256").expect("set its verified digest");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&old).unwrap())),
        expected
    );
    let (owner, source) = owner_and_source("native-recovery-old-group-reader");
    let source_work = source_member(&source);
    let mut second = Project::new("native-recovery-old-group-second");
    second.native_session = owner.native_session.clone();
    second.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    second.file("source-input.txt", "second source evidence\n");
    source_member(&second);
    let first = group_preview(&owner, &source, "old-group-first");
    apply_recovery(&owner, &first);
    let next = group_preview(&owner, &second, "old-group-second");
    apply_recovery(&owner, &next);
    let owner_config = owner.root.join("exitbind.json");
    let source_config = source.root.join("exitbind.json");
    let journal = native_journal(&owner);
    let before = fs::read(&journal).unwrap();
    let (session, store) = owner.native_session.as_ref().unwrap();
    let hook_payload = json!({"hook_event_name":"Stop", "cwd":owner.root,
        "session_id":session, "stop_hook_active":false, "last_assistant_message":"Done."})
    .to_string();
    let run_hook = |executable: &str| {
        let mut command = support::git_topology::command(executable);
        command
            .current_dir(&owner.root)
            .args(["hook-run", "--host", "codex"])
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .env_remove("CLAUDE_ENV_FILE")
            .env("CODEX_THREAD_ID", session)
            .env("EXITBIND_SESSION_GOALS_DIR", store)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(hook_payload.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let old_stop = run_hook(&old);
    assert!(old_stop.status.success());
    assert!(
        old_stop.stdout.is_empty(),
        "the previous hook's known fail-open must remain visible"
    );
    let compatible_stop = run_hook(env!("CARGO_BIN_EXE_exitbind"));
    assert!(compatible_stop.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&compatible_stop.stdout).unwrap()["decision"],
        "block"
    );
    assert_eq!(fs::read(&journal).unwrap(), before);
    let old_call = |args: &[&str], config: &Path| {
        let mut argv = vec![old.clone()];
        argv.extend(args.iter().map(|item| (*item).to_owned()));
        argv.extend(["--config".into(), config.to_str().unwrap().into()]);
        owner.call_exact_argv(&argv)
    };
    let status = old_call(&["goal", "status", "--json"], &owner_config);
    if status.status.success() {
        let value: Value = serde_json::from_slice(&status.stdout).unwrap();
        assert_ne!(
            value["goalProgress"]["journeyCompletion"]["state"],
            "current"
        );
        assert_ne!(value["closure"]["closed"], true);
    }
    for args in [
        vec![
            "goal",
            "recover",
            "--source-config",
            source_config.to_str().unwrap(),
            "--request-id",
            "old-recover",
            "--reason",
            "Old recovery must not replace groups",
        ],
        vec![
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--direct",
        ],
        vec![
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &source_work,
        ],
    ] {
        let output = old_call(&args, &owner_config);
        assert!(
            !output.status.success(),
            "old protected action unexpectedly succeeded: {args:?}"
        );
        assert_eq!(fs::read(&journal).unwrap(), before);
    }
    let runs = source.root.join(".exitbind/runs");
    let work_files = || {
        let mut paths = fs::read_dir(&runs)
            .unwrap()
            .map(|item| item.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with("work-"))
            .collect::<Vec<_>>();
        paths.sort();
        paths
    };
    let before_works = work_files();
    let old_begin = old_call(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Old integration",
            "--check-command",
            "sh check.sh",
            "--review-policy",
            "required",
            "--artifact",
            "check.sh",
            "--journey-integration",
        ],
        &source_config,
    );
    assert!(!old_begin.status.success());
    assert_eq!(work_files(), before_works);
    assert_eq!(fs::read(&journal).unwrap(), before);
}

#[test]
#[ignore = "requires an independently pinned pre-group recovery executable"]
fn old_integration_return_cannot_create_accepted_group_authority() {
    let old =
        std::env::var("EXITBIND_TEST_PREVIOUS_EXECUTABLE").expect("set the previous executable");
    let expected = std::env::var("EXITBIND_TEST_PREVIOUS_SHA256").expect("set its verified digest");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&old).unwrap())),
        expected
    );
    let (owner, source) = owner_and_source("native-recovery-old-group-return");
    source_member(&source);
    let preview = group_preview(&owner, &source, "old-return-group");
    apply_recovery(&owner, &preview);
    let config = source.root.join("exitbind.json");
    let integration = begin_in(
        &source,
        &config,
        "Integrate current source",
        "sh check.sh",
        &["--artifact", "check.sh", "--journey-integration"],
    );
    let lead = source.value_with_config(
        &["work", "next", &integration, "--json", "--full"],
        None,
        &config,
    );
    source.return_stage_with_config(&integration, &lead["next"], "scoped", &config);
    let worker = source.value_with_config(
        &["work", "next", &integration, "--json", "--full"],
        None,
        &config,
    );
    assert_eq!(worker["next"]["role"], "worker");
    let assignment = worker["next"]["assignment"].as_str().unwrap();
    source.value_with_config(
        &[
            "work",
            "permit",
            &integration,
            assignment,
            "--operation",
            "Return fixture evidence",
        ],
        None,
        &config,
    );
    let before_goal = fs::read(native_journal(&owner)).unwrap();
    let (session, store) = source.native_session.as_ref().unwrap();
    let mut command = support::git_topology::command(&old);
    command
        .current_dir(&source.root)
        .args([
            "work",
            "return",
            &integration,
            assignment,
            "--outcome",
            "completed",
            "--config",
        ])
        .arg(&config)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env_remove("CLAUDE_ENV_FILE")
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Err(error) = child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Old worker result fixture\n")
    {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    let _old_return = child.wait_with_output().unwrap();
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &integration[4..]);
    let state = source.value_with_config(&["run", "inspect", &ledger, "--json"], None, &config);
    assert_ne!(
        state["status"], "accepted",
        "a worker return cannot supply check/review/Lead acceptance"
    );
    assert_ne!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
}

#[test]
fn owner_status_offers_a_read_only_foreign_source_preview_without_credit() {
    let (owner, source) = owner_and_source("native-recovery-status-form");
    let work = source_member(&source);
    let journal = native_journal(&owner);
    let before_goal = fs::read(&journal).unwrap();
    let work_ledger = source
        .root
        .join(".exitbind/runs")
        .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()));
    let before_work = fs::read(&work_ledger).unwrap();
    let status = owner.status();
    let journey = &status["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "blocked");
    assert!(status["goalProgress"]["nextAction"].is_null());
    let form = &journey["recoveryForm"];
    assert_eq!(form["kind"], "native_goal_recovery_preview");
    assert_eq!(form["readOnly"], true);
    assert_eq!(form["integrationRequired"], true);
    assert_eq!(form["sourceWorks"], json!([work]));
    assert_eq!(form["sameConfigRequired"], false);
    assert_eq!(form["sameExecutableRequired"], false);
    let mut argv = form["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for word in &mut argv {
        match word.as_str() {
            "<REQUEST_ID>" => *word = "status-preview".into(),
            "<REASON>" => *word = "Inspect the source binding".into(),
            _ => {}
        }
    }
    assert!(!argv.iter().any(|word| word == "--apply"));
    // Give the existing display a recognizable value so that a redundant
    // post-preview refresh cannot pass merely because both writes shared a
    // timestamp. The display belongs to this disposable native session.
    let display_dir = owner.native_session.as_ref().unwrap().1.join("display");
    let displays = fs::read_dir(display_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    assert_eq!(displays.len(), 1);
    let display_path = &displays[0];
    let mut display: Value = serde_json::from_slice(&fs::read(display_path).unwrap()).unwrap();
    display["writtenAt"] = json!("preview-display-sentinel");
    let before_display = serde_json::to_vec(&display).unwrap();
    fs::write(display_path, &before_display).unwrap();
    let preview = owner.call_exact_argv(&argv);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["kind"], "native_goal_recovery_preview");
    assert_eq!(preview["readOnly"], true);
    assert_eq!(
        preview["decision"]["sourceMembers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        preview["decision"]["sourceMembers"][0]["member"]["work"],
        work
    );
    assert_eq!(fs::read(&journal).unwrap(), before_goal);
    assert_eq!(fs::read(&work_ledger).unwrap(), before_work);
    assert_eq!(fs::read(display_path).unwrap(), before_display);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
}

#[test]
fn recovery_form_keeps_pending_and_ambiguous_sources_uncredited() {
    let (owner, source) = owner_and_source("native-recovery-status-negative");
    let absent = owner.status();
    assert!(absent["goalProgress"]["journeyCompletion"]["recoveryForm"].is_null());
    let config = source.root.join("exitbind.json");
    let pending = begin_in(&source, &config, "Pending Source", "sh check.sh", &[]);
    let status = owner.status();
    let form = &status["goalProgress"]["journeyCompletion"]["recoveryForm"];
    assert_eq!(form["sourceWorks"], json!([pending]));
    assert_eq!(form["integrationRequired"], true);
    assert!(status["goalProgress"]["nextAction"].is_null());
    let foreign = source.value(&["goal", "status", "--json"], None);
    assert_eq!(foreign["contextOnly"], true);
    assert!(foreign["goalProgress"]["journeyCompletion"]["recoveryForm"].is_null());

    let bytes = fs::read(&config).unwrap();
    fs::write(&config, b"{").unwrap();
    let invalid = owner.status();
    assert!(invalid["goalProgress"]["journeyCompletion"]["recoveryForm"].is_null());
    fs::write(&config, bytes).unwrap();

    let mut second = Project::new("native-recovery-status-second-source");
    second.native_session = owner.native_session.clone();
    second.file("check.sh", "set -eu\ntrue\n");
    let second_config = second.root.join("exitbind.json");
    begin_in(
        &second,
        &second_config,
        "Another Source",
        "sh check.sh",
        &[],
    );
    let ambiguous = owner.status();
    assert!(ambiguous["goalProgress"]["journeyCompletion"]["recoveryForm"].is_null());
    assert_eq!(
        ambiguous["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
}

#[test]
fn uncredited_retirement_refuses_a_still_active_source_work() {
    let (owner, source) = owner_and_source("native-recovery-retirement-active");
    let config = source.root.join("exitbind.json");
    let replacement = source_member(&source);
    let active = begin_in(
        &source,
        &config,
        "An active source Work",
        "sh check.sh",
        &[],
    );
    let initial = recovery_preview(&owner, &source, "", "active-base-recovery");
    apply_recovery(&owner, &initial);
    let before_goal = fs::read(native_journal(&owner)).unwrap();
    let active_ledger = source.root.join(".exitbind/runs").join(format!(
        "work-{}.jsonl",
        active.strip_prefix("smw_").unwrap()
    ));
    let before_work = fs::read(&active_ledger).unwrap();
    let selection = json!([{"work":active, "reason":"Do not retire active work",
        "replacement":replacement}])
    .to_string();
    let refused = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            config.to_str().unwrap(),
            "--retirements",
            &selection,
            "--source-writers-quiesced",
            "--request-id",
            "active-retirement",
            "--reason",
            "Active work is not an eligible abandoned direction",
            "--json",
        ],
        None,
    );
    assert_json_refusal(&refused, "terminal blocked");
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    assert_eq!(fs::read(active_ledger).unwrap(), before_work);
}

#[test]
fn uncredited_retirement_refuses_a_real_late_pending_admission() {
    use std::os::unix::fs::symlink;

    struct RestoreRuns {
        runs: PathBuf,
        saved: PathBuf,
    }
    impl Drop for RestoreRuns {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.runs);
            if self.saved.exists() {
                fs::rename(&self.saved, &self.runs).expect("restore fixture Work directory");
            }
        }
    }

    let (owner, source) = owner_and_source("native-recovery-retirement-pending-admission");
    let config = source.root.join("exitbind.json");
    let replacement = source_member(&source);
    let blocked = blocked_source_member(&source, &config);
    let initial = recovery_preview(&owner, &source, "", "pending-base-recovery");
    apply_recovery(&owner, &initial);

    let safe = retirement_request(
        &owner,
        &config,
        &blocked,
        &replacement,
        "pending-retirement-selection",
    );
    assert!(safe.status.success(), "{safe:?}");
    let safe: Value = serde_json::from_slice(&safe.stdout).unwrap();
    assert_eq!(safe["readOnly"], true);
    let apply = safe["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(apply.iter().any(|arg| arg == "--apply"));

    // `work begin` first admits its exact native Goal intent, then creates the
    // Run. The outside-pointing fixture symlink lets readiness pass but makes
    // the actual Run path fail its existing state-root confinement check.
    let runs = source.root.join(".exitbind/runs");
    let saved = source.root.join(".exitbind/runs-retained-for-admission");
    let outside = source.root.parent().unwrap().join("outside-runs");
    fs::create_dir(&outside).unwrap();
    fs::rename(&runs, &saved).unwrap();
    let restore = RestoreRuns {
        runs: runs.clone(),
        saved,
    };
    symlink(&outside, &runs).unwrap();
    let failed = source.call_with_config(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A pending fixture admission",
            "--check-command",
            "sh check.sh",
            "--review-policy",
            "required",
            "--json",
        ],
        None,
        &config,
    );
    assert!(!failed.status.success(), "{failed:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(
        diagnostic.contains("native journey admission remains unresolved"),
        "{diagnostic}"
    );
    drop(restore);
    assert!(runs.is_dir());
    assert!(!fs::symlink_metadata(&runs)
        .unwrap()
        .file_type()
        .is_symlink());
    let pending = owner.status();
    assert_eq!(pending["journeyAdmissions"].as_array().unwrap().len(), 1);
    assert_eq!(
        pending["goalProgress"]["journeyCompletion"]["pendingAdmissions"],
        1
    );
    let pending_work = pending["journeyAdmissions"][0]["work"].as_str().unwrap();
    assert!(!work_ledger(&source, pending_work).exists());
    let before_goal = fs::read(native_journal(&owner)).unwrap();
    let before_work = fs::read(work_ledger(&source, &blocked)).unwrap();
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &blocked[4..]);
    let before_run =
        source.value_with_config(&["run", "inspect", &ledger, "--json"], None, &config);

    // This is the exact previously emitted apply argv; the admitted Work is a
    // later real state change, not a fabricated supposedly-current binding.
    let refused = owner.call_exact_argv(&apply);
    assert!(!refused.status.success(), "{refused:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        diagnostic.contains("pending native Goal admission prevents retirement"),
        "{diagnostic}"
    );
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    assert_eq!(
        fs::read(work_ledger(&source, &blocked)).unwrap(),
        before_work
    );
    let after_apply = owner.status();
    assert_eq!(after_apply["eventSha256"], pending["eventSha256"]);
    assert_eq!(after_apply["journeyMembers"], pending["journeyMembers"]);
    assert_eq!(
        after_apply["journeyAdmissions"],
        pending["journeyAdmissions"]
    );
    let after_run = source.value_with_config(&["run", "inspect", &ledger, "--json"], None, &config);
    assert_eq!(after_run["governor"], before_run["governor"]);
    assert_retirement_refused_without_changes(
        &owner,
        &source,
        &config,
        &blocked,
        &replacement,
        "pending-fresh-preview",
        "pending native Goal admission prevents retirement",
    );
}

#[test]
fn uncredited_retirement_refuses_an_actual_prepared_child() {
    let (owner, source) = owner_and_source("native-recovery-retirement-child");
    let config = source.root.join("exitbind.json");
    let replacement = source_member(&source);
    let work = begin_in(
        &source,
        &config,
        "The selected direction has a prepared child",
        "sh check.sh",
        &[
            "--preserve-requirement",
            "docs:Check docs.",
            "--preservation-check-command",
            "true",
        ],
    );
    let intake = json!({"action":"init","sourceRef":"owner:request",
        "sourceText":"Check docs.","requirements":[{"id":"docs","text":"Check docs."}]})
    .to_string();
    source.value_with_config(&["work", "record", &work], Some(&intake), &config);
    let continuation =
        source.value_with_config(&["work", "continuation", &work, "--json"], None, &config);
    let mutation_context_value = continuation["mutationContext"]["token"].as_str().unwrap();
    source.value_with_config(
        &[
            "work",
            "bind",
            &work,
            "--context",
            mutation_context_value,
            "--host",
            "codex",
            "--session",
            "retirement-child-session",
            "--host-version",
            "fixture",
        ],
        None,
        &config,
    );
    let continuation =
        source.value_with_config(&["work", "continuation", &work, "--json"], None, &config);
    let mutation_context_value = continuation["mutationContext"]["token"].as_str().unwrap();
    let prepared = source.value_with_config(
        &[
            "work",
            "child",
            "prepare",
            &work,
            "Check the documentation",
            "--context",
            mutation_context_value,
        ],
        None,
        &config,
    );
    assert_eq!(prepared["effect"], "prepared");
    let intent = prepared["intent"].as_str().unwrap();
    let mut next =
        source.value_with_config(&["work", "next", &work, "--json", "--full"], None, &config);
    while next["next"]["action"] == "lead_decision" {
        source.return_stage_with_config(&work, &next["next"], "scoped", &config);
        next =
            source.value_with_config(&["work", "next", &work, "--json", "--full"], None, &config);
    }
    assert_eq!(next["next"]["role"], "worker");
    source.return_stage_with_config(&work, &next["next"], "blocked", &config);
    let continuation =
        source.value_with_config(&["work", "continuation", &work, "--json"], None, &config);
    assert!(
        continuation["preparedChildren"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["intent"] == intent && item["state"] == "prepared"),
        "{continuation}"
    );
    let initial = recovery_preview(&owner, &source, "", "child-base-recovery");
    apply_recovery(&owner, &initial);
    let child_refusal =
        format!("a live, prepared or retained child prevents retirement: child {intent}");
    assert_retirement_refused_without_changes(
        &owner,
        &source,
        &config,
        &work,
        &replacement,
        "child-retirement-preview",
        &child_refusal,
    );
    let after = source.value_with_config(&["work", "continuation", &work, "--json"], None, &config);
    assert_eq!(after["preparedChildren"], continuation["preparedChildren"]);
}

// The checker kills only its own capture capsule. Run this one Linux case in
// an isolated subreaper process, so the exact orphaned fixture shell can be
// reaped without claiming or signalling any unrelated process.
#[cfg(target_os = "linux")]
fn isolated_retirement_unknown(test: &str) -> bool {
    if std::env::var("EXITBIND_RETIREMENT_REAPER_FIXTURE").as_deref() == Ok(test) {
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0
        );
        return false;
    }
    let output = support::temp("retirement-unknown-controller");
    let mut child = support::git_topology::command(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env("EXITBIND_RETIREMENT_REAPER_FIXTURE", test)
        .stdout(fs::File::create(output.join("stdout")).unwrap())
        .stderr(fs::File::create(output.join("stderr")).unwrap())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("owned retirement unknown fixture exceeded its deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let stdout = fs::read_to_string(output.join("stdout")).unwrap();
    let stderr = fs::read_to_string(output.join("stderr")).unwrap();
    assert!(status.success(), "{status}; {stdout}; {stderr}");
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
    true
}

#[cfg(target_os = "linux")]
struct RetirementOwnedShell(i32);

#[cfg(target_os = "linux")]
impl Drop for RetirementOwnedShell {
    fn drop(&mut self) {
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                self.0 as u32,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } == 0
        {
            if unsafe { info.si_pid() } == 0 {
                unsafe {
                    libc::kill(self.0, libc::SIGKILL);
                }
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if unsafe { libc::waitpid(self.0, std::ptr::null_mut(), libc::WNOHANG) } == self.0 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("the owned retirement fixture shell was not reaped");
        }
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn uncredited_retirement_refuses_a_real_unknown_check() {
    const TEST: &str =
        "goal_requirements::native_recovery::uncredited_retirement_refuses_a_real_unknown_check";
    if isolated_retirement_unknown(TEST) {
        return;
    }
    let (owner, source) = owner_and_source("native-recovery-retirement-unknown-check");
    source.file(
        "unknown-check.sh",
        "printf '%s' $$ > .exitbind/unknown-child; kill -KILL $PPID; exit 0\n",
    );
    let config = source.root.join("exitbind.json");
    let replacement = source_member(&source);
    let work = begin_in(
        &source,
        &config,
        "The check has an unknown termination",
        "exec sh unknown-check.sh",
        &[],
    );
    let initial = recovery_preview(&owner, &source, "", "unknown-base-recovery");
    apply_recovery(&owner, &initial);
    let mut next =
        source.value_with_config(&["work", "next", &work, "--json", "--full"], None, &config);
    while next["next"]["action"] == "lead_decision" {
        source.return_stage_with_config(&work, &next["next"], "scoped", &config);
        next =
            source.value_with_config(&["work", "next", &work, "--json", "--full"], None, &config);
    }
    assert_eq!(next["next"]["role"], "worker");
    source.return_stage_with_config(&work, &next["next"], "completed", &config);
    let failure = source.call_with_config(
        &["work", "check", &work, "--timeout-ms", "2000"],
        None,
        &config,
    );
    assert!(!failure.status.success(), "{failure:?}");
    let failure: Value = serde_json::from_slice(&failure.stdout).unwrap();
    let facts = &failure["observationFailure"]["facts"];
    assert!(facts.get("process").is_some() && facts["process"].is_null());
    assert_eq!(facts["groupEnded"], false);
    assert_eq!(facts["termination"], "unknown");
    let pid = fs::read_to_string(source.root.join(".exitbind/unknown-child"))
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let owned = RetirementOwnedShell(pid);
    let lead =
        source.value_with_config(&["work", "next", &work, "--json", "--full"], None, &config);
    assert_eq!(lead["next"]["role"], "lead");
    assert!(lead["next"]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|outcome| outcome == "blocked"));
    source.return_stage_with_config(&work, &lead["next"], "blocked", &config);
    let diagnostic =
        source.value_with_config(&["work", "recover-check", &work, "--json"], None, &config);
    assert_eq!(diagnostic["effect"], "no-change");
    assert_eq!(diagnostic["status"], "historical_unknown_termination");
    assert_eq!(diagnostic["observation"]["termination"], "unknown");
    assert_eq!(diagnostic["checkRecorded"], false);
    drop(owned);
    assert_retirement_refused_without_changes(
        &owner,
        &source,
        &config,
        &work,
        &replacement,
        "unknown-retirement-preview",
        "an active, unknown or retained Work operation prevents retirement",
    );
}

#[cfg(target_os = "linux")]
struct RetirementPreReturnCaller {
    child: Option<std::process::Child>,
    started: PathBuf,
    release: PathBuf,
    finished: PathBuf,
}

#[cfg(target_os = "linux")]
impl Drop for RetirementPreReturnCaller {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"cleanup");
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if self.started.exists() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !self.finished.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if !self.finished.exists() && !std::thread::panicking() {
                panic!("owned pre-return fixture did not finish after release");
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn uncredited_retirement_refuses_a_real_unresolved_pre_return() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;

    let (owner, source) = owner_and_source("native-recovery-retirement-pre-return");
    source.file("pre-return-check.sh", "set -eu\nprintf x >> .exitbind/pre-return-count\n: > .exitbind/pre-return-started\nwhile test ! -f .exitbind/pre-return-release; do sleep 0.05; done\n: > .exitbind/pre-return-ended\n");
    let config = source.root.join("exitbind.json");
    let mut settings: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    settings["project"]["verificationPreparation"] =
        json!({"version":1,"pythonBytecode":"disabled","scratch":"taskOwned"});
    fs::write(&config, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
    let replacement = source_member(&source);
    let work = begin_in(
        &source,
        &config,
        "The pre-return result is unresolved",
        "sh check.sh",
        &[
            "--pre-return-command",
            "sh pre-return-check.sh",
            "--pre-return-timeout-ms",
            "20000",
        ],
    );
    let initial = recovery_preview(&owner, &source, "", "pre-return-base-recovery");
    apply_recovery(&owner, &initial);
    let mut next =
        source.value_with_config(&["work", "next", &work, "--json", "--full"], None, &config);
    while next["next"]["action"] == "lead_decision" {
        source.return_stage_with_config(&work, &next["next"], "scoped", &config);
        next =
            source.value_with_config(&["work", "next", &work, "--json", "--full"], None, &config);
    }
    assert_eq!(next["next"]["role"], "worker");
    let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
    let (session, store) = source.native_session.as_ref().unwrap();
    let outside = source.root.parent().unwrap();
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&source.root)
        .args([
            "work",
            "return",
            &work,
            &assignment,
            "--outcome",
            "completed",
            "--config",
        ])
        .arg(&config)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env_remove("EXITBIND_SESSION_GOALS_DIR")
        .env_remove("CLAUDE_ENV_FILE")
        .env_remove("XDG_STATE_HOME")
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .stdin(Stdio::piped())
        .stdout(fs::File::create(outside.join("pre-return-caller.stdout")).unwrap())
        .stderr(fs::File::create(outside.join("pre-return-caller.stderr")).unwrap());
    let child = command.spawn().unwrap();
    let mut caller = RetirementPreReturnCaller {
        child: Some(child),
        started: source.root.join(".exitbind/pre-return-started"),
        release: source.root.join(".exitbind/pre-return-release"),
        finished: source.root.join(".exitbind/pre-return-ended"),
    };
    caller
        .child
        .as_mut()
        .unwrap()
        .stdin
        .take()
        .unwrap()
        .write_all(b"The complete fixture worker result.\n")
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !caller.started.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "pre-return did not start"
        );
        assert!(
            caller.child.as_mut().unwrap().try_wait().unwrap().is_none(),
            "caller exited before admission"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let admitted = source.value_with_config(&["run", "inspect", &ledger, "--json"], None, &config);
    let attempt = admitted["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["action"] == "pre_return_admitted")
        .and_then(|event| event["admission"]["attemptId"].as_str())
        .expect("real public pre-return admission")
        .to_owned();
    caller.child.as_mut().unwrap().kill().unwrap();
    let status = caller.child.take().unwrap().wait().unwrap();
    assert_eq!(status.signal(), Some(9));
    fs::write(&caller.release, b"finish the owned checker").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let observation = source.value_with_config(
            &["work", "recover-pre-return", &work, "--json"],
            None,
            &config,
        );
        if observation["ownerDecision"]["snapshot"]["evidence"]["type"] == "owned_observer_v1" {
            let facts = &observation["ownerDecision"]["snapshot"]["evidence"]["facts"];
            assert_eq!(facts["ownedChildrenDrained"], true);
            assert_eq!(facts["checkCredit"], false);
            assert_eq!(facts["nativeExit"], "unknown");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "owned observer did not finish: {observation}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(caller.finished.exists());
    let read = source.value_with_config(
        &["run", "pre-return", "read", &work, &attempt, "--json"],
        None,
        &config,
    );
    assert!(
        read.get("result").is_some() && read["result"].is_null(),
        "{read}"
    );
    let held = format!(
        "hld_{}_{:x}_{:x}.bin",
        &work[4..],
        Sha256::digest(assignment.as_bytes()),
        Sha256::digest(b"The complete fixture worker result.\n")
    );
    let held = source.root.join(".exitbind/held").join(held);
    let held_before = fs::read(&held).unwrap();
    assert_eq!(held_before, b"The complete fixture worker result.\n");
    let blocked = source.call_with_config(
        &[
            "work",
            "return",
            &work,
            &assignment,
            "--outcome",
            "blocked",
            "--json",
        ],
        Some("The interrupted pre-return attempt remains uncredited.\n"),
        &config,
    );
    assert!(blocked.status.success(), "{blocked:?}");
    assert_eq!(
        fs::read(source.root.join(".exitbind/pre-return-count")).unwrap(),
        b"x"
    );
    assert_retirement_refused_without_changes(
        &owner,
        &source,
        &config,
        &work,
        &replacement,
        "pre-return-retirement-preview",
        "an unresolved pre-return attempt prevents retirement",
    );
    let retained = source.value_with_config(
        &["run", "pre-return", "read", &work, &attempt, "--json"],
        None,
        &config,
    );
    assert!(
        retained.get("result").is_some() && retained["result"].is_null(),
        "{retained}"
    );
    assert_eq!(retained, read);
    assert_eq!(fs::read(held).unwrap(), held_before);
    assert_eq!(
        fs::read(source.root.join(".exitbind/pre-return-count")).unwrap(),
        b"x"
    );
}

#[test]
fn explicit_uncredited_retirement_preserves_members_and_requires_a_new_integration() {
    let (owner, source) = owner_and_source("native-recovery-uncredited-retirement");
    let config = source.root.join("exitbind.json");
    let replacement = source_member(&source);
    let blocked = blocked_source_member(&source, &config);
    let initial = recovery_preview(&owner, &source, "", "uncredited-base-recovery");
    apply_recovery(&owner, &initial);
    let old_work = source.root.join(".exitbind/runs").join(format!(
        "work-{}.jsonl",
        blocked.strip_prefix("smw_").unwrap()
    ));
    let before_work = fs::read(&old_work).unwrap();
    let before_goal = fs::read(native_journal(&owner)).unwrap();
    let members = owner.status()["journeyMembers"].clone();
    let selected = json!([{"work":blocked.as_str(),"reason":"The owner selected the already accepted alternative", "replacement":replacement.as_str()}]).to_string();
    let args = [
        "goal",
        "recover",
        "--source-config",
        config.to_str().unwrap(),
        "--retirements",
        &selected,
        "--request-id",
        "uncredited-1",
        "--reason",
        "Select the alternative explicitly",
        "--json",
    ];
    let without_attestation = owner.call(&args, None);
    assert_json_refusal(&without_attestation, "source-writers-quiesced");
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    let mut attested = args.to_vec();
    attested.push("--source-writers-quiesced");

    // A retained result from any assignment remains an unresolved fact. Use
    // the real held-file naming format with a different fixture assignment.
    let held_dir = source.root.join(".exitbind/held");
    fs::create_dir_all(&held_dir).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&held_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let retained = b"an earlier assignment's retained result";
    let held = held_dir.join(format!(
        "hld_{}_{:x}_{:x}.bin",
        blocked.strip_prefix("smw_").unwrap(),
        Sha256::digest(b"an earlier assignment"),
        Sha256::digest(retained)
    ));
    fs::write(&held, retained).unwrap();
    let held_refusal = owner.call(&attested, None);
    assert_json_refusal(&held_refusal, "retained result");
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    assert_eq!(fs::read(&old_work).unwrap(), before_work);
    fs::remove_file(&held).unwrap();

    let wrong_state = json!([{"work":replacement.as_str(), "reason":"not blocked", "replacement":blocked.as_str()}]).to_string();
    let wrong_args = [
        "goal",
        "recover",
        "--source-config",
        config.to_str().unwrap(),
        "--retirements",
        &wrong_state,
        "--source-writers-quiesced",
        "--request-id",
        "uncredited-wrong-state",
        "--reason",
        "Reject accepted Work",
        "--json",
    ];
    let refused = owner.call(&wrong_args, None);
    assert_json_refusal(&refused, "terminal blocked");
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    let foreign = Project::new("native-recovery-retirement-foreign");
    let foreign_config = foreign.root.join("exitbind.json");
    let foreign_work = begin_in(&foreign, &foreign_config, "Foreign work", "true", &[]);
    let foreign_selection = json!([{"work":blocked.as_str(), "reason":"foreign replacement",
        "replacement":foreign_work.as_str()}])
    .to_string();
    let foreign_args = [
        "goal",
        "recover",
        "--source-config",
        config.to_str().unwrap(),
        "--retirements",
        &foreign_selection,
        "--source-writers-quiesced",
        "--request-id",
        "uncredited-foreign",
        "--reason",
        "Reject foreign work",
        "--json",
    ];
    let refused = owner.call(&foreign_args, None);
    assert!(!refused.status.success());
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    assert_eq!(fs::read(&old_work).unwrap(), before_work);
    let output = owner.call(&attested, None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let preview: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(preview["readOnly"], true);
    assert_eq!(
        preview["retirementSelection"]["items"][0]["member"]["work"],
        blocked
    );
    assert_eq!(
        preview["retirementSelection"]["items"][0]["replacement"]["member"]["work"],
        replacement
    );
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    // An intervening owner observation invalidates the exact preview binding.
    incorporate_native_generic_goal(
        &owner,
        "original-conversation",
        "Complete the original conversation work with current accepted evidence",
    );
    let changed_goal = fs::read(native_journal(&owner)).unwrap();
    let stale_argv = preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let stale = owner.call_exact_argv(&stale_argv);
    assert!(!stale.status.success());
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), changed_goal);
    assert_eq!(fs::read(&old_work).unwrap(), before_work);
    let id = attested
        .iter()
        .position(|arg| *arg == "uncredited-1")
        .unwrap();
    attested[id] = "uncredited-2";
    let fresh = owner.call(&attested, None);
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stderr)
    );
    let preview: Value = serde_json::from_slice(&fresh.stdout).unwrap();
    let applied = apply_recovery(&owner, &preview);
    assert_eq!(applied["kind"], "native_goal_recovery_applied");
    let after = owner.status();
    assert_eq!(after["journeyMembers"], members);
    assert_eq!(
        after["goalProgress"]["journeyCompletion"]["members"]["retiredUncredited"],
        1
    );
    assert_eq!(
        after["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(fs::read(&old_work).unwrap(), before_work);

    // An ordinary owner update with the same Goal text preserves the exact
    // selection; it is still bound to the same explicitly selected scope.
    incorporate_native_generic_goal(
        &owner,
        "original-conversation",
        "Complete the original conversation work with current accepted evidence",
    );
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["members"]["retiredUncredited"],
        1
    );
    let integration = accept_recovery_integration(&source, &config);
    let current = owner.status();
    assert_eq!(
        current["goalProgress"]["journeyCompletion"]["state"], "current",
        "{current}"
    );
    assert_eq!(
        current["goalProgress"]["journeyCompletion"]["members"]["retiredUncredited"],
        1
    );
    assert_eq!(fs::read(&old_work).unwrap(), before_work);
    let before_close = fs::read(native_journal(&owner)).unwrap();
    let closed = owner.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &integration,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["closure"]["resultRefs"], json!([integration]));
    assert_eq!(closed["previousEventSha256"], current["eventSha256"]);
    assert_eq!(
        closed["revision"].as_u64(),
        Some(current["revision"].as_u64().unwrap() + 1)
    );
    let after_close = fs::read(native_journal(&owner)).unwrap();
    assert!(after_close.starts_with(&before_close) && after_close.len() > before_close.len());
    assert_eq!(fs::read(&old_work).unwrap(), before_work);

    // A later explicit selection with a different reason is a different
    // integration binding even when the same original members remain.
    incorporate_native_generic_goal(
        &owner,
        "original-conversation",
        "Complete the original conversation work with current accepted evidence",
    );
    let changed_selection = json!([{"work":blocked.as_str(), "reason":"A newly stated owner decision", "replacement":replacement.as_str()}]).to_string();
    let changed = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            config.to_str().unwrap(),
            "--retirements",
            &changed_selection,
            "--source-writers-quiesced",
            "--request-id",
            "uncredited-3",
            "--reason",
            "Record the new selection explicitly",
            "--json",
        ],
        None,
    );
    assert_ne!(changed["currentBinding"], preview["currentBinding"]);
    apply_recovery(&owner, &changed);
    let changed_status = owner.status();
    assert_eq!(
        changed_status["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(fs::read(&old_work).unwrap(), before_work);
    incorporate_native_generic_goal(
        &owner,
        "original-conversation",
        "A materially different conversation Goal",
    );
    let new_goal = owner.status();
    assert!(new_goal.get("nativeGoalRetirementSelection").is_none());
    assert_ne!(
        new_goal["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert_eq!(fs::read(&old_work).unwrap(), before_work);
}

#[test]
#[ignore = "requires an independently pinned pre-retirement 0.31.28 executable"]
fn old_reader_and_old_writer_do_not_restore_retirement_authority() {
    let old = std::env::var("EXITBIND_TEST_PREVIOUS_EXECUTABLE")
        .expect("set EXITBIND_TEST_PREVIOUS_EXECUTABLE to the exact old producer");
    let expected_sha = std::env::var("EXITBIND_TEST_PREVIOUS_SHA256")
        .expect("set EXITBIND_TEST_PREVIOUS_SHA256 to the verified old producer digest");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&old).unwrap())),
        expected_sha
    );
    let (owner, source) = owner_and_source("native-recovery-old-writer");
    let config = source.root.join("exitbind.json");
    let replacement = source_member(&source);
    let blocked = blocked_source_member(&source, &config);
    let initial = recovery_preview(&owner, &source, "", "old-reader-base-recovery");
    apply_recovery(&owner, &initial);
    let selected = json!([{"work":blocked.as_str(), "reason":"Select the accepted alternative", "replacement":replacement.as_str()}]).to_string();
    let preview = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            config.to_str().unwrap(),
            "--retirements",
            &selected,
            "--source-writers-quiesced",
            "--request-id",
            "old-reader-selection",
            "--reason",
            "Preserve the original blocked Work",
            "--json",
        ],
        None,
    );
    apply_recovery(&owner, &preview);
    let integration = accept_recovery_integration(&source, &config);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    let owner_config = owner
        .root
        .join("exitbind.json")
        .to_str()
        .unwrap()
        .to_owned();
    let old_status = vec![
        old.clone(),
        "goal".into(),
        "status".into(),
        "--json".into(),
        "--config".into(),
        owner_config.clone(),
    ];
    let response = owner.call_exact_argv(&old_status);
    assert!(
        response.status.success(),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    let old_view: Value = serde_json::from_slice(&response.stdout).unwrap();
    assert_eq!(old_view["journeyMembers"].as_array().unwrap().len(), 2);
    assert_ne!(
        old_view["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    // Old recover clones the complete raw Goal and changes the recovery
    // decision without knowing that the carried retirement is now stale.
    let before_old_recovery = owner.status();
    let original_selection = before_old_recovery["nativeGoalRetirementSelection"].clone();
    let original_members = before_old_recovery["journeyMembers"].clone();
    let original_ledgers = [&replacement, &blocked, &integration]
        .into_iter()
        .map(|work| (work, fs::read(work_ledger(&source, work)).unwrap()))
        .collect::<Vec<_>>();
    let blocked_ledger = format!(".exitbind/runs/work-{}.jsonl", &blocked[4..]);
    let before_run = source.value_with_config(
        &["run", "inspect", &blocked_ledger, "--json"],
        None,
        &config,
    );
    for request_id in ["old-recovery-1", "old-recovery-2"] {
        let before_preview = fs::read(native_journal(&owner)).unwrap();
        let old_preview = vec![
            old.clone(),
            "goal".into(),
            "recover".into(),
            "--source-config".into(),
            config.to_str().unwrap().into(),
            "--request-id".into(),
            request_id.into(),
            "--reason".into(),
            "Observe a recovery written by the previous producer".into(),
            "--json".into(),
            "--config".into(),
            owner_config.clone(),
        ];
        let output = owner.call_exact_argv(&old_preview);
        assert!(output.status.success(), "{output:?}");
        let recovery: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(recovery["readOnly"], true);
        assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_preview);
        let apply_argv = recovery["applyArgv"]
            .as_array()
            .expect("old preview emits its apply argv")
            .iter()
            .map(|part| part.as_str().expect("complete old argv").to_owned())
            .collect::<Vec<_>>();
        assert_eq!(apply_argv.first(), Some(&old));
        assert!(apply_argv.iter().any(|part| part == "--apply"));
        let output = owner.call_exact_argv(&apply_argv);
        assert!(output.status.success(), "{output:?}");
        let applied: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(applied["kind"], "native_goal_recovery_applied");
        assert_eq!(
            applied["event"]["nativeGoalRetirementSelection"],
            original_selection
        );
        assert_ne!(
            applied["event"]["nativeGoalRecovery"]["active"]["decisionSha256"],
            original_selection["recoveryDecisionSha256"]
        );
        let after_apply = fs::read(native_journal(&owner)).unwrap();
        assert!(
            after_apply.starts_with(&before_preview) && after_apply.len() > before_preview.len()
        );
        let status = owner.status();
        assert_eq!(fs::read(native_journal(&owner)).unwrap(), after_apply);
        assert_eq!(status["nativeGoalRetirementSelection"], original_selection);
        assert_eq!(status["journeyMembers"], original_members);
        let completion = &status["goalProgress"]["journeyCompletion"];
        assert_eq!(completion["state"], "blocked", "{completion}");
        assert_eq!(
            completion["members"]["retiredUncredited"]
                .as_u64()
                .unwrap_or(0),
            0
        );
        for (work, bytes) in &original_ledgers {
            assert_eq!(&fs::read(work_ledger(&source, work)).unwrap(), bytes);
        }
        let after_run = source.value_with_config(
            &["run", "inspect", &blocked_ledger, "--json"],
            None,
            &config,
        );
        assert_eq!(after_run["governor"], before_run["governor"]);
    }

    let stale_journal = fs::read(native_journal(&owner)).unwrap();
    let refused = owner.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &integration,
            "--json",
        ],
        None,
    );
    assert_json_refusal(
        &refused,
        "journey_members_need_current_acceptance_or_checked_integration",
    );
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), stale_journal);
    let stale_replay = apply_recovery(&owner, &preview);
    assert_eq!(stale_replay["kind"], "native_goal_recovery_replayed");
    assert_eq!(stale_replay["historical"], true);
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), stale_journal);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );

    // A fresh explicit selection is necessary, and its old integration is no
    // substitute for a checked integration against the new decision.
    let reselection = retirement_request(
        &owner,
        &config,
        &blocked,
        &replacement,
        "old-reader-reselection",
    );
    assert!(reselection.status.success(), "{reselection:?}");
    let reselection: Value = serde_json::from_slice(&reselection.stdout).unwrap();
    apply_recovery(&owner, &reselection);
    let selected_again = owner.status();
    assert_eq!(selected_again["journeyMembers"], original_members);
    assert_eq!(
        selected_again["goalProgress"]["journeyCompletion"]["members"]["retiredUncredited"],
        1
    );
    assert_eq!(
        selected_again["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    let new_integration = accept_recovery_integration(&source, &config);
    let integrated = owner.status();
    assert_eq!(integrated["journeyMembers"], original_members);
    assert_eq!(
        integrated["goalProgress"]["journeyCompletion"]["integrationWork"],
        new_integration
    );
    assert_eq!(
        integrated["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    for (work, bytes) in &original_ledgers {
        assert_eq!(&fs::read(work_ledger(&source, work)).unwrap(), bytes);
    }

    let old_append = vec![
        old,
        "goal".into(),
        "incorporate".into(),
        "--goal-id".into(),
        "original-conversation".into(),
        "--goal".into(),
        "Complete the original conversation work with current accepted evidence".into(),
        "--config".into(),
        owner_config,
    ];
    let output = owner.call_exact_argv(&old_append);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let after_old = owner.status();
    assert!(after_old.get("nativeGoalRetirementSelection").is_none());
    assert_eq!(after_old["journeyMembers"].as_array().unwrap().len(), 2);
    assert_eq!(
        after_old["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    let replay = apply_recovery(&owner, &preview);
    assert_eq!(replay["kind"], "native_goal_recovery_replayed");
    assert_eq!(replay["historical"], true);
    let latest = owner.status();
    assert!(latest.get("nativeGoalRetirementSelection").is_none());
    assert_eq!(
        latest["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
}

#[test]
fn uncredited_retirement_refuses_an_existing_successor_claim() {
    let (owner, source) = owner_and_source("native-recovery-retirement-claim");
    let config = source.root.join("exitbind.json");
    let replacement = source_member(&source);
    let blocked = blocked_source_member(&source, &config);
    let initial = recovery_preview(&owner, &source, "", "claim-base-recovery");
    apply_recovery(&owner, &initial);
    let old_ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        blocked.strip_prefix("smw_").unwrap()
    );
    let successor_ledger = format!(".exitbind/runs/work-{}.jsonl", "d".repeat(64));
    let claim = source.call_with_config(
        &[
            "run",
            "supersede",
            &old_ledger,
            "--workflow",
            "change",
            "--goal",
            "Abandoned observer direction",
            "--ledger",
            &successor_ledger,
            "--review-policy",
            "required",
            "--json",
        ],
        None,
        &config,
    );
    assert!(
        claim.status.success(),
        "{}",
        String::from_utf8_lossy(&claim.stderr)
    );
    let before_goal = fs::read(native_journal(&owner)).unwrap();
    let before_work = fs::read(source.root.join(&old_ledger)).unwrap();
    let selected = json!([{"work":blocked.as_str(), "reason":"Do not discard a claim", "replacement":replacement.as_str()}]).to_string();
    let refused = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            config.to_str().unwrap(),
            "--retirements",
            &selected,
            "--source-writers-quiesced",
            "--request-id",
            "claim-refusal",
            "--reason",
            "Inspect the existing claim",
            "--json",
        ],
        None,
    );
    assert_json_refusal(&refused, "claimed successor");
    assert_eq!(fs::read(native_journal(&owner)).unwrap(), before_goal);
    assert_eq!(fs::read(source.root.join(old_ledger)).unwrap(), before_work);
}

#[path = "native_recovery/semantic_repair.rs"]
mod semantic_repair;

#[test]
fn native_recovery_connects_distinct_source_and_preserves_historical_members_through_owner_closure()
{
    let (owner, source) = owner_and_source("native-recovery-distinct-source");
    owner.file("owner-input.txt", "owner fixture\n");
    let old_fixture = begin_in(
        &owner,
        &owner.root.join("exitbind.json"),
        "Retain the old fixture",
        "sh owner-check.sh",
        &[],
    );
    owner.drive(&old_fixture, true);
    let home_member = begin_in(
        &owner,
        &owner.root.join("exitbind.json"),
        "Complete a real owner-project contribution",
        "sh owner-check.sh",
        &[],
    );
    owner.drive(&home_member, true);
    let source_work = source_member(&source);

    let preview = recovery_preview(&owner, &source, &old_fixture, "connect-source-once");
    assert_eq!(preview["kind"], "native_goal_recovery_preview");
    assert_eq!(
        preview["decision"]["sourceMembers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        preview["decision"]["sourceMembers"][0]["member"]["work"],
        source_work
    );
    assert_eq!(
        preview["decision"]["historicalFixtures"][0]["work"],
        old_fixture
    );
    let journal_before_apply = fs::read(native_journal(&owner)).unwrap();
    let before_apply = owner.status();
    let members_before_apply = before_apply["journeyMembers"].clone();
    let admissions_before_apply = before_apply["journeyAdmissions"].clone();
    let integrations_before_apply = before_apply["journeyIntegrations"].clone();
    let applied = apply_recovery(&owner, &preview);
    assert_eq!(applied["kind"], "native_goal_recovery_applied");
    let after_apply = owner.status();
    let journal_after_apply = fs::read(native_journal(&owner)).unwrap();
    assert!(
        journal_after_apply.starts_with(&journal_before_apply),
        "recovery appends without rewriting prior Goal history bytes"
    );
    assert_eq!(after_apply["journeyMembers"], members_before_apply);
    assert_eq!(after_apply["journeyAdmissions"], admissions_before_apply);
    assert_eq!(
        after_apply["journeyIntegrations"],
        integrations_before_apply
    );
    assert_eq!(after_apply["journeyMembers"].as_array().unwrap().len(), 3);
    assert_eq!(
        after_apply["goalProgress"]["journeyCompletion"]["members"]["historical"],
        1
    );
    assert_eq!(
        after_apply["goalProgress"]["journeyCompletion"]["members"]["total"],
        3
    );

    let close_without_source_integration = owner.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &source_work,
        ],
        None,
    );
    assert!(
        !close_without_source_integration.status.success(),
        "current accepted Source inputs still require the recovery integration"
    );

    source.file("source-input.txt", "changed source evidence\n");
    source.file(
        "check.sh",
        "set -eu\ntest \"$(cat source-input.txt)\" = 'changed source evidence'\n",
    );
    let integration_config = source.root.join("exitbind-current-integration.json");
    let mut integration_settings: Value =
        serde_json::from_slice(&fs::read(source.root.join("exitbind.json")).unwrap()).unwrap();
    integration_settings["orchestration"]["maxParallel"] = json!(3);
    fs::write(
        &integration_config,
        serde_json::to_vec_pretty(&integration_settings).unwrap(),
    )
    .unwrap();
    let integration = begin_in(
        &source,
        &integration_config,
        "Recheck all current source members",
        "sh check.sh",
        &["--artifact", "check.sh", "--journey-integration"],
    );
    source.drive_with_config(&integration, true, &integration_config);
    let current = owner.status();
    let journey = &current["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "current", "owner readiness: {journey}");
    assert_eq!(journey["members"]["historical"], 1);
    assert_eq!(journey["integrationWork"], integration);
    let unrelated = Project::new("native-recovery-unrelated-result");
    unrelated.file("check.sh", "set -eu\nprintf unrelated\n");
    let unrelated_work = begin_in(
        &unrelated,
        &unrelated.root.join("exitbind.json"),
        "Unrelated project result",
        "sh check.sh",
        &[],
    );
    unrelated.drive(&unrelated_work, true);
    let unrelated_close = owner.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &unrelated_work,
        ],
        None,
    );
    assert!(!unrelated_close.status.success());
    assert!(String::from_utf8_lossy(&unrelated_close.stderr).contains("not a governed result"));
    let closed = owner.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &integration,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["journeyMembers"][0]["work"], old_fixture);
    assert_eq!(closed["journeyMembers"][1]["work"], home_member);
    assert_eq!(closed["journeyMembers"][2]["work"], source_work);
}

#[test]
fn native_recovery_retains_fifteen_explicit_historical_fixtures_without_current_credit() {
    let (owner, source) = owner_and_source("native-recovery-fifteen-fixtures");
    let mut fixture_works = Vec::new();
    for index in 0..15 {
        let work = begin_in(
            &owner,
            &owner.root.join("exitbind.json"),
            &format!("Deterministic historical fixture {index}"),
            "true",
            &[],
        );
        owner.drive(&work, true);
        fixture_works.push(work);
    }
    let source_work = source_member(&source);
    let selected = fixture_works.join(",");
    let preview = recovery_preview(&owner, &source, &selected, "retain-fifteen-fixtures");
    assert_eq!(
        preview["decision"]["historicalFixtures"]
            .as_array()
            .unwrap()
            .len(),
        15
    );
    let applied = apply_recovery(&owner, &preview);
    assert_eq!(applied["kind"], "native_goal_recovery_applied");
    let before_integration = owner.status();
    assert_eq!(
        before_integration["goalProgress"]["journeyCompletion"]["state"], "blocked",
        "a recovered current Source result still requires a fresh integration"
    );
    assert_eq!(
        before_integration["goalProgress"]["journeyCompletion"]["members"]["historical"],
        15
    );
    let integration = accept_recovery_integration(&source, &source.root.join("exitbind.json"));
    let journey = owner.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(journey["state"], "current");
    assert_eq!(journey["members"]["total"], 16);
    assert_eq!(journey["members"]["historical"], 15);
    assert_eq!(journey["members"]["current"], 1);
    assert_eq!(journey["integrationWork"], integration);
    assert_eq!(
        journey["members"]["retainedHistorical"]
            .as_array()
            .unwrap()
            .len(),
        15
    );
    assert!(journey["members"]["retainedHistorical"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["state"] == "historical"));
    assert_eq!(
        owner.status()["journeyMembers"].as_array().unwrap().len(),
        16
    );
    assert!(owner.status()["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|member| member["work"] == source_work));
}

#[test]
fn native_recovery_replay_after_later_append_returns_exact_saved_event() {
    let (owner, source) = owner_and_source("native-recovery-replay");
    let source_work = source_member(&source);
    let preview = recovery_preview(&owner, &source, "", "stable-recovery-request");
    let applied = apply_recovery(&owner, &preview);
    let saved = applied["event"].clone();
    owner.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "original-conversation",
            "--goal",
            "Complete the original conversation work with current accepted evidence",
            "--finding",
            "A later same-goal append remains preserved",
            "--disposition",
            "open",
        ],
        None,
    );
    let replay = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--request-id",
            "stable-recovery-request",
            "--reason",
            "Retain the verified historical fixture without current credit",
            "--current-binding",
            preview["currentBinding"].as_str().unwrap(),
            "--apply",
        ],
        None,
    );
    assert_eq!(replay["kind"], "native_goal_recovery_replayed");
    assert_eq!(replay["event"], saved);
    assert_ne!(
        replay["event"]["eventSha256"],
        owner.status()["eventSha256"]
    );
    assert_eq!(owner.status()["journeyMembers"][0]["work"], source_work);
    let later_source_member = source_member(&source);
    let blocked = owner.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(blocked["state"], "blocked");
    assert!(blocked["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| {
            item["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("Source Goal membership changed"))
        }));
    assert!(owner.status()["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|member| member["work"] == later_source_member));
    let replay_after_new_member = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--request-id",
            "stable-recovery-request",
            "--reason",
            "Retain the verified historical fixture without current credit",
            "--current-binding",
            preview["currentBinding"].as_str().unwrap(),
            "--apply",
        ],
        None,
    );
    assert_eq!(
        replay_after_new_member["kind"],
        "native_goal_recovery_replayed"
    );
    assert_eq!(replay_after_new_member["event"], saved);
    let altered_replay = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--request-id",
            "stable-recovery-request",
            "--reason",
            "Altered arguments must not reuse a saved request",
            "--current-binding",
            preview["currentBinding"].as_str().unwrap(),
            "--apply",
        ],
        None,
    );
    assert!(!altered_replay.status.success());
    assert!(String::from_utf8_lossy(&altered_replay.stderr)
        .contains("already used with different recovery arguments"));
}

#[test]
fn native_recovery_binds_multiple_original_and_explicit_endpoint_configurations() {
    let (owner, source) = owner_and_source("native-recovery-endpoint-configs");
    let original_config = source.root.join("exitbind.json");
    let alternate_config = source.root.join("exitbind-alternate.json");
    let mut alternate: Value =
        serde_json::from_slice(&fs::read(&original_config).unwrap()).unwrap();
    alternate["orchestration"]["maxParallel"] = json!(3);
    fs::write(
        &alternate_config,
        serde_json::to_vec_pretty(&alternate).unwrap(),
    )
    .unwrap();
    let first = begin_in(
        &source,
        &original_config,
        "First source contribution",
        "sh check.sh",
        &[],
    );
    source.drive_with_config(&first, true, &original_config);
    let second = begin_in(
        &source,
        &alternate_config,
        "Second source contribution",
        "sh check.sh",
        &[],
    );
    source.drive_with_config(&second, true, &alternate_config);

    let endpoint_configs = serde_json::to_string(&std::collections::BTreeMap::from([
        (first.clone(), original_config.to_str().unwrap().to_owned()),
        (
            second.clone(),
            alternate_config.to_str().unwrap().to_owned(),
        ),
    ]))
    .unwrap();
    let wrong_root = serde_json::to_string(&std::collections::BTreeMap::from([(
        first.clone(),
        owner
            .root
            .join("exitbind.json")
            .to_str()
            .unwrap()
            .to_owned(),
    )]))
    .unwrap();
    let wrong_root_result = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            original_config.to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--endpoint-configs",
            &wrong_root,
            "--request-id",
            "wrong-endpoint-root",
            "--reason",
            "Reject an endpoint config from the wrong project root",
        ],
        None,
    );
    assert!(!wrong_root_result.status.success());
    assert!(String::from_utf8_lossy(&wrong_root_result.stderr)
        .contains("does not exactly match its member"));
    let preview = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            original_config.to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--endpoint-configs",
            &endpoint_configs,
            "--request-id",
            "two-endpoint-configs",
            "--reason",
            "Bind each source member to its explicit accepted endpoint configuration",
        ],
        None,
    );
    let selected = preview["decision"]["sourceMembers"].as_array().unwrap();
    assert_eq!(selected.len(), 2);
    let bindings = selected
        .iter()
        .map(|item| {
            (
                item["member"]["work"].as_str().unwrap().to_owned(),
                item["endpointConfig"]["path"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        bindings[&first],
        original_config.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        bindings[&second],
        alternate_config.canonicalize().unwrap().to_str().unwrap()
    );
    apply_recovery(&owner, &preview);
    let before_integration = owner.status();
    assert_eq!(
        before_integration["goalProgress"]["journeyCompletion"]["state"], "blocked",
        "both accepted Source members still require a fresh integration"
    );
    let integration = accept_recovery_integration(&source, &original_config);
    let status = owner.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"], "current",
        "{status}"
    );
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["members"]["current"], 2,
        "both accepted Source members retain current status after integration"
    );
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["integrationWork"],
        integration
    );

    let mut changed = alternate.clone();
    changed["orchestration"]["maxParallel"] = json!(4);
    fs::write(
        &alternate_config,
        serde_json::to_vec_pretty(&changed).unwrap(),
    )
    .unwrap();
    let status = owner.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"], "blocked",
        "{status}"
    );
    fs::write(
        &alternate_config,
        serde_json::to_vec_pretty(&alternate).unwrap(),
    )
    .unwrap();
    let status = owner.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"], "current",
        "{status}"
    );
    fs::remove_file(&alternate_config).unwrap();
    let status = owner.status();
    let missing_endpoint = status["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(missing_endpoint["state"], "blocked", "{status}");
    assert!(missing_endpoint["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| {
            item["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("endpoint configuration is unavailable"))
        }));
    fs::write(
        &alternate_config,
        serde_json::to_vec_pretty(&alternate).unwrap(),
    )
    .unwrap();
    let status = owner.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"], "current",
        "{status}"
    );
    let missing_source = source.root.join("exitbind.json.missing");
    fs::rename(&original_config, &missing_source).unwrap();
    let status = owner.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"], "blocked",
        "{status}"
    );
    fs::rename(&missing_source, &original_config).unwrap();
    let status = owner.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"], "current",
        "{status}"
    );
}

#[test]
fn native_recovery_follows_the_exact_accepted_source_successor_under_selected_endpoint_config() {
    let (owner, source) = owner_and_source("native-recovery-successor-chain");
    let original_config = source.root.join("exitbind.json");
    let task_goal = "Validate source evidence through its exact accepted successor";
    let original = begin_in(&source, &original_config, task_goal, "sh check.sh", &[]);
    let mut original_detail = source.value_with_config(
        &["work", "next", &original, "--json", "--full"],
        None,
        &original_config,
    );
    while original_detail["next"]["action"] == "lead_decision" {
        let next = original_detail["next"].clone();
        let scoped = next["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "scoped");
        source.return_stage_with_config(
            &original,
            &next,
            if scoped { "scoped" } else { "accepted" },
            &original_config,
        );
        original_detail = source.value_with_config(
            &["work", "next", &original, "--json", "--full"],
            None,
            &original_config,
        );
    }
    assert_eq!(original_detail["next"]["role"], "worker");
    source.value_with_config(
        &[
            "work",
            "return",
            &original,
            original_detail["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some("Blocked predecessor fixture; create its accepted same-Goal successor.\n"),
        &original_config,
    );
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    let original_member = owner.status()["journeyMembers"][0].clone();

    let endpoint_config = source.root.join("exitbind-successor.json");
    let mut alternate: Value =
        serde_json::from_slice(&fs::read(&original_config).unwrap()).unwrap();
    alternate["orchestration"]["maxParallel"] = json!(3);
    fs::write(
        &endpoint_config,
        serde_json::to_vec_pretty(&alternate).unwrap(),
    )
    .unwrap();
    let old_ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        original.strip_prefix("smw_").unwrap()
    );
    let successor_token = "c".repeat(64);
    let successor_ledger = format!(".exitbind/runs/work-{successor_token}.jsonl");
    let superseded = source.call_with_config(
        &[
            "run",
            "supersede",
            &old_ledger,
            "--workflow",
            "change",
            "--goal",
            task_goal,
            "--ledger",
            &successor_ledger,
            "--review-policy",
            "required",
            "--json",
        ],
        None,
        &endpoint_config,
    );
    assert!(
        superseded.status.success(),
        "{}{}",
        String::from_utf8_lossy(&superseded.stdout),
        String::from_utf8_lossy(&superseded.stderr)
    );
    let superseded: Value = serde_json::from_slice(&superseded.stdout).unwrap();
    let successor = superseded["work"].as_str().unwrap();
    assert_eq!(successor, format!("smw_{successor_token}"));
    source.drive_with_config(successor, true, &endpoint_config);

    let endpoint_map = serde_json::to_string(&std::collections::BTreeMap::from([(
        original.clone(),
        endpoint_config.to_str().unwrap().to_owned(),
    )]))
    .unwrap();
    let preview = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            original_config.to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--endpoint-configs",
            &endpoint_map,
            "--request-id",
            "accepted-successor-chain",
            "--reason",
            "Bind the exact accepted same-Goal successor and its selected config",
        ],
        None,
    );
    let member = &preview["decision"]["sourceMembers"][0]["member"];
    assert_eq!(member["work"], original);
    assert_eq!(member, &original_member);
    assert_eq!(
        preview["decision"]["sourceMembers"][0]["endpointConfig"]["path"],
        endpoint_config.canonicalize().unwrap().to_str().unwrap()
    );
    apply_recovery(&owner, &preview);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked",
        "an accepted exact successor still requires a fresh integration"
    );
    let integration = accept_recovery_integration(&source, &endpoint_config);
    let status = owner.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["integrationWork"],
        integration
    );
    assert_eq!(status["journeyMembers"][0], original_member);

    // Keep the actual blocked predecessor and accepted same-Goal successor
    // while adding a second source group. The successor is never a new member.
    let original_bytes = fs::read(work_ledger(&source, &original)).unwrap();
    let mut other = Project::new("native-recovery-successor-other-group");
    other.native_session = owner.native_session.clone();
    other.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    other.file("source-input.txt", "other accepted source\n");
    let other_work = source_member(&other);
    let group = group_preview(&owner, &other, "successor-other-group");
    apply_recovery(&owner, &group);
    let stale = owner.status();
    assert_eq!(
        stale["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    let fresh_first = accept_recovery_integration(&source, &endpoint_config);
    let fresh_other = accept_recovery_integration(&other, &other.root.join("exitbind.json"));
    let current = owner.status();
    assert_eq!(
        current["goalProgress"]["journeyCompletion"]["state"], "current",
        "{current}"
    );
    let members = current["journeyMembers"].as_array().unwrap();
    assert!(members.iter().any(|member| member["work"] == original));
    assert!(members.iter().any(|member| member["work"] == other_work));
    assert!(!members.iter().any(|member| member["work"] == successor));
    assert_ne!(fresh_first, fresh_other);
    assert_eq!(
        fs::read(work_ledger(&source, &original)).unwrap(),
        original_bytes
    );
}

#[test]
fn native_recovery_rejects_stale_foreign_and_tampered_selections() {
    let (owner, source) = owner_and_source("native-recovery-bindings");
    let source_work = source_member(&source);
    let source_as_fixture = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            &source_work,
            "--request-id",
            "source-cannot-be-historical",
            "--reason",
            "A required Source contributor cannot be classified as historical",
        ],
        None,
    );
    assert!(!source_as_fixture.status.success());
    assert!(String::from_utf8_lossy(&source_as_fixture.stderr)
        .contains("a current Source contributor cannot be selected as historical"));
    let duplicate = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            &format!("{source_work},{source_work}"),
            "--request-id",
            "duplicate-selection",
            "--reason",
            "Retain the verified historical fixture without current credit",
        ],
        None,
    );
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("unique exact Work IDs"));

    let stale = recovery_preview(&owner, &source, "", "stale-recovery-request");
    owner.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "original-conversation",
            "--goal",
            "Complete the original conversation work with current accepted evidence",
            "--finding",
            "Concurrent Goal change",
            "--disposition",
            "open",
        ],
        None,
    );
    let stale_apply = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--request-id",
            "stale-recovery-request",
            "--reason",
            "Retain the verified historical fixture without current credit",
            "--current-binding",
            stale["currentBinding"].as_str().unwrap(),
            "--apply",
        ],
        None,
    );
    assert!(!stale_apply.status.success());
    assert_eq!(
        String::from_utf8_lossy(&stale_apply.stderr).trim(),
        "exitbind: --current-binding does not match the exact current recovery preview",
        "{}{}",
        String::from_utf8_lossy(&stale_apply.stdout),
        String::from_utf8_lossy(&stale_apply.stderr)
    );

    let fresh = recovery_preview(&owner, &source, "", "tampered-recovery-request");
    let wrong_source = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            owner.root.join("exitbind.json").to_str().unwrap(),
            "--request-id",
            "tampered-recovery-request",
            "--reason",
            "Retain the verified historical fixture without current credit",
            "--current-binding",
            fresh["currentBinding"].as_str().unwrap(),
            "--apply",
        ],
        None,
    );
    assert!(
        !wrong_source.status.success(),
        "a config that does not select the recovered Source cannot apply"
    );

    let mut foreign = Project::new("native-recovery-foreign-owner");
    foreign.native_session = owner.native_session.clone();
    let foreign_result = foreign.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--request-id",
            "foreign",
            "--reason",
            "Retain the verified historical fixture without current credit",
        ],
        None,
    );
    assert!(!foreign_result.status.success());
    assert!(String::from_utf8_lossy(&foreign_result.stderr)
        .contains("owned by another configured project"));
}

#[test]
fn native_recovery_refuses_named_goal_and_unaccepted_source_evidence() {
    let named =
        Project::new("native-recovery-named-goal").with_native_session("named-recovery-session");
    named.setup();
    assert!(named.status()["goalProgress"]["journeyCompletion"]["recoveryForm"].is_null());
    let named_result = named.call(
        &[
            "goal",
            "recover",
            "--source-config",
            named.root.join("exitbind.json").to_str().unwrap(),
            "--request-id",
            "named-refusal",
            "--reason",
            "Retain the verified historical fixture without current credit",
        ],
        None,
    );
    assert!(!named_result.status.success());
    assert!(String::from_utf8_lossy(&named_result.stderr).contains("named-requirement Goals"));

    let (owner, source) = owner_and_source("native-recovery-pending-source");
    let pending = begin_in(
        &source,
        &source.root.join("exitbind.json"),
        "Pending source contribution",
        "sh check.sh",
        &[],
    );
    let preview = recovery_preview(&owner, &source, "", "pending-source-is-required");
    let applied = apply_recovery(&owner, &preview);
    assert_eq!(applied["kind"], "native_goal_recovery_applied");
    let status = owner.status();
    let journey = &status["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "blocked");
    assert!(journey["recoveryForm"].is_null());
    assert_eq!(journey["members"]["total"], 1);
    assert_eq!(journey["members"]["historical"], 0);
    assert!(journey["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["work"] == pending));
    let close = owner.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &pending,
        ],
        None,
    );
    assert!(
        !close.status.success(),
        "recovery must not manufacture acceptance or closure evidence"
    );
}

#[test]
fn native_recovery_rejects_unsupported_malformed_and_post_apply_named_state() {
    let (owner, source) = owner_and_source("native-recovery-invalid-extension");
    source_member(&source);
    let preview = recovery_preview(&owner, &source, "", "invalid-extension-recovery");
    apply_recovery(&owner, &preview);

    let original_journal = fs::read(native_journal(&owner)).unwrap();
    reseal_latest_record(&owner, |record| {
        record["nativeGoalRecovery"]["version"] = json!(999);
    });
    let unsupported = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--request-id",
            "unsupported-extension",
            "--reason",
            "Inspect an unsupported extension",
        ],
        None,
    );
    assert!(!unsupported.status.success());
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("invalid or unsupported"));

    fs::write(native_journal(&owner), &original_journal).unwrap();
    reseal_latest_record(&owner, |record| {
        record["nativeGoalRecovery"]["unexpectedFutureField"] = json!(true);
    });
    let malformed = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--request-id",
            "malformed-extension",
            "--reason",
            "Inspect a malformed extension",
        ],
        None,
    );
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("malformed or has unknown fields"));

    fs::write(native_journal(&owner), &original_journal).unwrap();
    reseal_latest_record(&owner, |record| {
        record["continuation"] = json!({"kind":"goal_requirements"});
    });
    let named = owner.call(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--request-id",
            "invalid-extension-recovery",
            "--reason",
            "Retain the verified historical fixture without current credit",
            "--current-binding",
            preview["currentBinding"].as_str().unwrap(),
            "--apply",
        ],
        None,
    );
    assert!(!named.status.success());
    assert_eq!(
        String::from_utf8_lossy(&named.stderr).trim(),
        "exitbind: native Goal recovery cannot be used by a named-requirement Goal",
        "{}{}",
        String::from_utf8_lossy(&named.stdout),
        String::from_utf8_lossy(&named.stderr)
    );
}
