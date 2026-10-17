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
    let raw = fs::read_to_string(&path).unwrap();
    let mut lines = raw.lines().map(str::to_owned).collect::<Vec<_>>();
    let latest = lines.last_mut().expect("fixture Goal record");
    let mut record: Value = serde_json::from_str(latest).unwrap();
    mutate(&mut record);
    let object = record.as_object_mut().unwrap();
    object.remove("eventSha256");
    let digest = format!("{:x}", Sha256::digest(canonical(&record).as_bytes()));
    record["eventSha256"] = json!(digest);
    *latest = serde_json::to_string(&record).unwrap();
    fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
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
