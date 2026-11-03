//! Disposable native Goal owner-correction scenarios; no host Goal state is used.
use super::*;
use sha2::{Digest, Sha256};

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut fields = object.iter().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(right.0));
            format!(
                "{{{}}}",
                fields
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => value.to_string(),
    }
}

fn paths(project: &Project) -> (PathBuf, PathBuf) {
    let store = &project.native_session.as_ref().unwrap().1;
    let root = fs::read_dir(store)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("session-")
        })
        .expect("native fixture root");
    (
        root.join("owner.json"),
        root.join(".exitbind/session-goal.jsonl"),
    )
}

struct Fixture {
    old: Project,
    target: Project,
    relocations: String,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let old = Project::new(label).with_native_session(&format!("{label}-session"));
        old.setup();
        let mut target = Project::new(&format!("{label}-target"));
        target.native_session = old.native_session.clone();
        let source = fs::read(old.root.join("request.txt")).unwrap();
        fs::write(target.root.join("relocated.txt"), &source).unwrap();
        target.file("check.sh", "set -eu\ntest -s relocated.txt\n");
        let relocations = json!([{"originalRef":"request.txt","recordedSha256":sha(&source),
            "targetPath":"relocated.txt"}])
        .to_string();
        Self {
            old,
            target,
            relocations,
        }
    }

    fn preview(&self, id: &str) -> Value {
        let config = self.target.root.join("exitbind.json");
        self.old.value(
            &[
                "goal",
                "correct-owner",
                "--target-config",
                config.to_str().unwrap(),
                "--relocations",
                &self.relocations,
                "--request-id",
                id,
                "--reason",
                "Correct the selected project",
                "--old-writers-quiesced",
                "--json",
            ],
            None,
        )
    }

    fn execute(&self, preview: &Value, field: &str) -> Output {
        let argv = preview[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        self.old.call_exact_argv(&argv)
    }
}

fn value(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn install_pending_marker(project: &Project, preview: &Value) {
    let (owner_path, _) = paths(project);
    let original: Value = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    assert_eq!(original["protocol"], 1);
    let original_sha = original["sha256"].clone();
    let mut marker = json!({"kind":"native_session_goal_owner", "protocol":2,
        "host":original["host"], "sessionKey":original["sessionKey"],
        "original":original, "previousOwnerRecordSha256":original_sha,
        "baseDecisionSha256":null,"pending":preview["decision"],"request":preview["request"]});
    let digest = sha(canonical(&marker).as_bytes());
    marker["sha256"] = json!(digest);
    fs::write(owner_path, marker.to_string()).unwrap();
}

#[test]
fn owner_correction_preserves_journal_and_prepares_checked_target_work() {
    let fixture = Fixture::new("owner-correction-preserve");
    let (owner_path, journal) = paths(&fixture.old);
    let before_owner: Value = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    let before_raw = fs::read(&journal).unwrap();
    let before = support::goal_journal::latest(&journal);
    let store = fixture.old.native_store_snapshot();
    let preview = fixture.preview("preserve-1");
    assert_eq!(preview["readOnly"], true);
    assert_eq!(store, fixture.old.native_store_snapshot());
    assert_eq!(preview["decision"]["oldWritersQuiesced"], true);
    let applied = value(&fixture.execute(&preview, "applyArgv"));
    assert_eq!(applied["kind"], "native_goal_owner_correction_applied");
    let after_raw = fs::read(&journal).unwrap();
    assert!(after_raw.starts_with(&before_raw));
    let after = support::goal_journal::latest(&journal);
    assert_eq!(after["goalId"], before["goalId"]);
    assert_eq!(
        after["nativeGoalOwnerCorrections"][0]["reason"],
        "Correct the selected project"
    );
    assert_eq!(after["goal"], before["goal"]);
    assert_eq!(after["source"], before["source"]);
    assert_eq!(after["categories"], before["categories"]);
    assert_eq!(after["continuation"], before["continuation"]);
    assert_eq!(
        after["revision"].as_u64(),
        before["revision"].as_u64().map(|value| value + 1)
    );
    let owner: Value = serde_json::from_slice(&fs::read(owner_path).unwrap()).unwrap();
    assert_eq!(owner["protocol"], 2);
    assert_eq!(owner["original"], before_owner);
    assert_eq!(owner["sessionKey"], before_owner["sessionKey"]);
    fixture.old.reject(
        &["goal", "cover", "--goal-id", "import"],
        "owned by another configured project",
    );
    let replay = value(&fixture.execute(&preview, "applyArgv"));
    assert_eq!(replay["kind"], "native_goal_owner_correction_replayed");
    assert_eq!(fs::read(&journal).unwrap(), after_raw);
    let mut changed: Vec<String> = preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect();
    let index = changed.iter().position(|word| word == "--reason").unwrap();
    changed[index + 1] = "A different reason".into();
    assert!(!fixture.old.call_exact_argv(&changed).status.success());
    assert_eq!(fs::read(&journal).unwrap(), after_raw);
    let prepared = fixture.target.value(
        &[
            "goal",
            "prepare",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh R1",
            "--json",
        ],
        None,
    );
    assert_eq!(prepared["selectedRequirementIds"], json!(["R1"]));
    fixture.target.file("relocated.txt", "source drift\n");
    let drifted = fixture.target.status();
    assert_eq!(
        drifted["requirements"]["sourceStatus"]["state"],
        "unavailable"
    );
    assert_eq!(drifted["continuation"], before["continuation"]);
    assert_eq!(drifted["requirements"]["nextAction"], Value::Null);
    fs::copy(
        fixture.old.root.join("request.txt"),
        fixture.target.root.join("relocated.txt"),
    )
    .unwrap();
    let work = fixture.target.begin("R1", "sh check.sh R1");
    fixture.target.drive(&work, true);
}

#[test]
fn corrected_owner_survives_config_edits_and_retains_identity_checks() {
    let fixture = Fixture::new("owner-correction-config-edits");
    let (_, journal) = paths(&fixture.old);
    let preview = fixture.preview("config-edits");
    let applied = value(&fixture.execute(&preview, "applyArgv"));
    let saved = applied["event"]["nativeGoalOwnerCorrections"][0].clone();
    let committed = fs::read(&journal).unwrap();

    let config_path = fixture.target.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["$schema"] = json!("https://example.invalid/updated-schema.json");
    config["agents"]["worker"]["displayName"] = json!("A changed agent label");
    let changed_config = serde_json::to_string_pretty(&config).unwrap() + "\n";
    fs::write(&config_path, &changed_config).unwrap();
    assert_eq!(
        value(&fixture.execute(&preview, "applyArgv"))["kind"],
        "native_goal_owner_correction_replayed"
    );
    assert_eq!(fs::read(&journal).unwrap(), committed);
    assert_eq!(
        fixture.target.status()["requirements"]["sourceStatus"]["state"],
        "current"
    );

    fixture.target.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            "Reject invalid input",
            "--artifact",
            "relocated.txt",
        ],
        None,
    );
    assert_eq!(row(&fixture.target.status(), "R1")["revision"], 2);
    fixture
        .target
        .value(&["goal", "cover", "--goal-id", "import"], None);
    let prepared = fixture.target.value(
        &[
            "goal",
            "prepare",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh all",
            "--scope",
            "integration",
            "--json",
        ],
        None,
    );
    assert_eq!(
        prepared["selectedRequirementIds"],
        json!(["R1", "R2", "R3"])
    );

    // The foreign cwd has its own project. The corrected owner has no Work
    // hint yet, so Stop must resolve its saved owner path after the edit.
    let (session, store) = fixture.target.native_session.as_ref().unwrap();
    let payload = json!({"hook_event_name":"Stop", "cwd":fixture.old.root,
        "session_id":session, "stop_hook_active":false, "last_assistant_message":"A side answer."});
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&fixture.old.root)
        .args(["hook-run", "--host", "codex"])
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CODEX_THREAD_ID", session)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let before_stop = fixture.old.native_store_snapshot();
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let stopped = child.wait_with_output().unwrap();
    assert!(stopped.status.success(), "{stopped:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&stopped.stdout).unwrap(),
        json!({"decision":"block", "reason":"The session Goal is open. Continue the authorized work."})
    );
    assert_eq!(fixture.old.native_store_snapshot(), before_stop);

    let alternate = fixture.target.root.join("alternate-config.json");
    fs::write(&alternate, &changed_config).unwrap();
    let before_refusals = fixture.old.native_store_snapshot();
    let wrong_path = fixture.target.call_with_config(
        &["goal", "cover", "--goal-id", "import"],
        None,
        &alternate,
    );
    assert!(
        !wrong_path.status.success(),
        "a different config path must not acquire the corrected owner"
    );
    assert!(
        String::from_utf8_lossy(&wrong_path.stderr).contains("owner configuration does not match"),
        "{wrong_path:?}"
    );
    config["project"]["root"] = json!(fixture.old.root);
    fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
    let wrong_root = fixture
        .target
        .call(&["goal", "cover", "--goal-id", "import"], None);
    assert!(
        !wrong_root.status.success(),
        "a different project root must not acquire the corrected owner"
    );
    assert!(
        String::from_utf8_lossy(&wrong_root.stderr).contains("owned by another configured project"),
        "{wrong_root:?}"
    );
    assert_eq!(fixture.old.native_store_snapshot(), before_refusals);
    fs::write(&config_path, changed_config).unwrap();
    fs::remove_file(alternate).unwrap();

    let work = fixture.target.begin("R1,R2,R3", "sh check.sh all");
    fixture.target.drive(&work, true);
    fixture.target.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &work,
        ],
        None,
    );
    let closed = fixture.target.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &work,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["nativeGoalOwnerCorrections"][0], saved);
    assert!(fs::read(&journal).unwrap().starts_with(&committed));
}

#[test]
fn pending_barrier_can_be_aborted_or_retried_without_a_second_owner_commit() {
    let fixture = Fixture::new("owner-correction-interruption");
    let (_, journal) = paths(&fixture.old);
    let before = fs::read(&journal).unwrap();
    let first = fixture.preview("pending-abort");
    install_pending_marker(&fixture.old, &first);
    let recovered = fixture.old.status();
    assert_eq!(
        recovered["ownerCorrectionRecovery"]["applyArgv"],
        first["applyArgv"]
    );
    assert_eq!(
        recovered["ownerCorrectionRecovery"]["abortArgv"],
        first["abortArgv"]
    );
    fixture.old.reject(
        &["goal", "cover", "--goal-id", "import"],
        "correction is pending",
    );
    fixture
        .target
        .file("relocated.txt", "temporarily unavailable bytes\n");
    let before_retry = fixture.old.native_store_snapshot();
    let failed_retry = fixture.execute(&first, "applyArgv");
    assert!(!failed_retry.status.success(), "{failed_retry:?}");
    let refusal: Value = serde_json::from_slice(&failed_retry.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON refusal: {error}; {failed_retry:?}"));
    let diagnostic = refusal["error"]
        .as_str()
        .expect("JSON refusal has an error");
    assert!(
        diagnostic.contains(&format!("retry exact argv {}", first["applyArgv"]))
            && diagnostic.contains(&format!("abort exact argv {}", first["abortArgv"])),
        "unexpected recovery refusal: {diagnostic}"
    );
    assert_eq!(fixture.old.native_store_snapshot(), before_retry);
    assert_eq!(
        value(&fixture.execute(&first, "abortArgv"))["kind"],
        "native_goal_owner_correction_aborted"
    );
    assert_eq!(fs::read(&journal).unwrap(), before);
    fs::copy(
        fixture.old.root.join("request.txt"),
        fixture.target.root.join("relocated.txt"),
    )
    .unwrap();
    assert!(
        !fixture.execute(&first, "applyArgv").status.success(),
        "aborted preview is not reusable"
    );
    let (owner_path, _) = paths(&fixture.old);
    // Start from a fresh v1 fixture for a simulated stop immediately after barrier installation.
    let fixture2 = Fixture::new("owner-correction-interruption-retry");
    let second = fixture2.preview("pending-retry");
    install_pending_marker(&fixture2.old, &second);
    let (_, journal2) = paths(&fixture2.old);
    let old_prefix = fs::read(&journal2).unwrap();
    let applied = value(&fixture2.execute(&second, "applyArgv"));
    assert_eq!(applied["kind"], "native_goal_owner_correction_applied");
    assert!(fs::read(&journal2).unwrap().starts_with(&old_prefix));
    assert_eq!(
        value(&fixture2.execute(&second, "applyArgv"))["kind"],
        "native_goal_owner_correction_replayed"
    );
    assert!(!fixture2.execute(&second, "abortArgv").status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(owner_path).unwrap()).unwrap()["protocol"],
        2
    );
}

#[test]
fn stale_preview_source_config_and_goal_refuse_before_any_owner_effect() {
    let fixture = Fixture::new("owner-correction-stale");
    let preview = fixture.preview("stale-source");
    fixture
        .target
        .file("relocated.txt", "different source bytes\n");
    let before = fixture.old.native_store_snapshot();
    assert!(!fixture.execute(&preview, "applyArgv").status.success());
    assert_eq!(fixture.old.native_store_snapshot(), before);
    fs::copy(
        fixture.old.root.join("request.txt"),
        fixture.target.root.join("relocated.txt"),
    )
    .unwrap();
    let preview = fixture.preview("stale-config");
    let target_config = fixture.target.root.join("exitbind.json");
    let original = fs::read_to_string(&target_config).unwrap();
    fs::write(&target_config, format!("{original}\n")).unwrap();
    let before = fixture.old.native_store_snapshot();
    assert!(!fixture.execute(&preview, "applyArgv").status.success());
    assert_eq!(fixture.old.native_store_snapshot(), before);
    fs::write(target_config, original).unwrap();
    let preview = fixture.preview("stale-goal");
    fixture.old.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "import",
            "--goal",
            "Import names safely and show a count",
            "--finding",
            "A later finding",
        ],
        None,
    );
    let before = fixture.old.native_store_snapshot();
    assert!(!fixture.execute(&preview, "applyArgv").status.success());
    assert_eq!(fixture.old.native_store_snapshot(), before);
}

#[test]
fn same_original_path_with_distinct_recorded_snapshots_is_relocated_by_hash() {
    let mut fixture = Fixture::new("owner-correction-variants");
    let original = fs::read(fixture.old.root.join("request.txt")).unwrap();
    let revised = format!(
        "Reject malformed input before persistence.\n{}\n{}",
        REQUIREMENTS[1].1, REQUIREMENTS[2].1
    );
    fixture.old.file("request.txt", &revised);
    fixture.old.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            "Reject malformed input before persistence.",
            "--artifact",
            "request.txt",
        ],
        None,
    );
    fixture.target.file("revised.txt", &revised);
    fixture.relocations = json!([
        {"originalRef":"request.txt","recordedSha256":sha(&original),"targetPath":"relocated.txt"},
        {"originalRef":"request.txt","recordedSha256":sha(revised.as_bytes()),"targetPath":"revised.txt"}
    ]).to_string();
    let (_, journal) = paths(&fixture.old);
    let before = support::goal_journal::latest(&journal);
    let preview = fixture.preview("two-variants");
    value(&fixture.execute(&preview, "applyArgv"));
    let after = support::goal_journal::latest(&journal);
    assert_eq!(after["continuation"], before["continuation"]);
    assert_eq!(after["continuation"]["requirements"][0]["revision"], 2);
    assert_eq!(
        after["continuation"]["requirements"][0]["history"][0]["revision"],
        1
    );
    fixture
        .target
        .value(&["goal", "cover", "--goal-id", "import"], None);
    fixture.target.value(
        &[
            "goal",
            "prepare",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh R1",
        ],
        None,
    );
}

#[test]
fn source_escape_missing_bytes_and_symlink_are_refused_read_only() {
    let mut fixture = Fixture::new("owner-correction-paths");
    let original = fs::read(fixture.old.root.join("request.txt")).unwrap();
    let store = fixture.old.native_store_snapshot();
    let config = fixture.target.root.join("exitbind.json");
    for target in ["missing.txt", "../outside.txt"] {
        fixture.relocations = json!([{"originalRef":"request.txt","recordedSha256":sha(&original),"targetPath":target}]).to_string();
        let output = fixture.old.call(
            &[
                "goal",
                "correct-owner",
                "--target-config",
                config.to_str().unwrap(),
                "--relocations",
                &fixture.relocations,
                "--request-id",
                "unsafe-path",
                "--reason",
                "correct owner",
                "--old-writers-quiesced",
            ],
            None,
        );
        assert!(!output.status.success(), "{output:?}");
        assert_eq!(fixture.old.native_store_snapshot(), store);
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            fixture.target.root.join("relocated.txt"),
            fixture.target.root.join("linked.txt"),
        )
        .unwrap();
        fixture.relocations = json!([{"originalRef":"request.txt","recordedSha256":sha(&original),"targetPath":"linked.txt"}]).to_string();
        let output = fixture.old.call(
            &[
                "goal",
                "correct-owner",
                "--target-config",
                config.to_str().unwrap(),
                "--relocations",
                &fixture.relocations,
                "--request-id",
                "unsafe-link",
                "--reason",
                "correct owner",
                "--old-writers-quiesced",
            ],
            None,
        );
        assert!(!output.status.success(), "{output:?}");
        assert_eq!(fixture.old.native_store_snapshot(), store);
    }
}

fn correction_call(fixture: &Fixture, id: &str) -> Output {
    let config = fixture.target.root.join("exitbind.json");
    fixture.old.call(
        &[
            "goal",
            "correct-owner",
            "--target-config",
            config.to_str().unwrap(),
            "--relocations",
            &fixture.relocations,
            "--request-id",
            id,
            "--reason",
            "correct owner",
            "--old-writers-quiesced",
        ],
        None,
    )
}

fn reseal_last(project: &Project, change: impl FnOnce(&mut Value)) {
    let (_, journal) = paths(project);
    let mut records = support::goal_journal::records(&journal);
    let record = records.last_mut().unwrap();
    change(record);
    record.as_object_mut().unwrap().remove("eventSha256");
    let digest = sha(canonical(record).as_bytes());
    record["eventSha256"] = json!(digest);
    support::goal_journal::write(&journal, &records);
}

#[test]
fn pending_admission_retired_mapping_closed_and_member_states_refuse_without_effects() {
    let pending = Fixture::new("owner-correction-pending-admission");
    reseal_last(&pending.old, |record| {
        record["journeyMembershipProtocol"] = json!(2);
        record["journeyMembers"] = json!([]);
        record["journeyIntegrations"] = json!([]);
        record["journeyAdmissions"] = json!([{
            "work":format!("smw_{}", "a".repeat(64)), "startIntentSha256":"b".repeat(64),
            "parentGoalId":"import", "parentObservedGoalEventSha256":"c".repeat(64),
            "workGoalSha256":"d".repeat(64), "host":"codex", "sessionKey":"e".repeat(64),
            "ownerIdentitySha256":"f".repeat(64), "ownerProjectId":"fixture",
            "projectIdentitySha256":"1".repeat(64), "localIdentitySha256":"2".repeat(64),
            "configPath":"/fixture/exitbind.json", "configSha256":"3".repeat(64),
            "productRoot":"/fixture", "stateRoot":"/fixture"
        }]);
    });
    let before = pending.old.native_store_snapshot();
    assert!(!correction_call(&pending, "pending").status.success());
    assert_eq!(pending.old.native_store_snapshot(), before);

    let mapping = Fixture::new("owner-correction-retired-mapping");
    reseal_last(&mapping.old, |record| {
        let requirement = &record["continuation"]["requirements"][0];
        let reference = json!({"id":requirement["id"],"revision":requirement["revision"],
            "text":requirement["text"],"sourceSha256":requirement["source"]["sha256"]});
        record["continuation"]["mappings"] = json!([{"work":format!("smw_{}", "a".repeat(64)),
            "requirement":reference,"startEventSha256":"b".repeat(64),"active":false}]);
    });
    let before = mapping.old.native_store_snapshot();
    assert!(!correction_call(&mapping, "retired").status.success());
    assert_eq!(mapping.old.native_store_snapshot(), before);

    let closed = Fixture::new("owner-correction-closed");
    reseal_last(&closed.old, |record| {
        record["closure"]["closed"] = json!(true)
    });
    let before = closed.old.native_store_snapshot();
    assert!(!correction_call(&closed, "closed").status.success());
    assert_eq!(closed.old.native_store_snapshot(), before);

    let member = Fixture::new("owner-correction-member");
    member
        .old
        .begin_ordinary("A real associated Work", "true", &[]);
    let before = member.old.native_store_snapshot();
    assert!(!correction_call(&member, "member").status.success());
    assert_eq!(member.old.native_store_snapshot(), before);
}

fn concurrent_command(
    project: &Project,
    executable: &str,
    args: &[String],
) -> std::process::Command {
    let (identity, store) = project.native_session.as_ref().unwrap();
    let mut command = support::git_topology::command(executable);
    command
        .current_dir(&project.root)
        .args(args)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env_remove("CLAUDE_ENV_FILE")
        .env_remove("XDG_STATE_HOME")
        .env("CODEX_THREAD_ID", identity)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn correction_and_preselected_requirement_do_not_commit_under_different_owners() {
    let fixture = Fixture::new("owner-correction-race");
    let preview = fixture.preview("race");
    let argv = preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let config = fixture.old.root.join("exitbind.json");
    let mutation = [
        "goal",
        "require",
        "--goal-id",
        "import",
        "--requirement",
        "R4",
        "--obligation",
        REQUIREMENTS[0].1,
        "--artifact",
        "request.txt",
        "--config",
        config.to_str().unwrap(),
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect::<Vec<_>>();
    let correction = concurrent_command(&fixture.old, &argv[0], &argv[1..])
        .spawn()
        .unwrap();
    let writer = concurrent_command(&fixture.old, env!("CARGO_BIN_EXE_exitbind"), &mutation)
        .spawn()
        .unwrap();
    let correction = correction.wait_with_output().unwrap();
    let writer = writer.wait_with_output().unwrap();
    assert_ne!(
        correction.status.success(),
        writer.status.success(),
        "correction={correction:?}; writer={writer:?}"
    );
    let (_, journal) = paths(&fixture.old);
    let records = support::goal_journal::records(&journal);
    let latest = records.last().unwrap();
    if correction.status.success() {
        assert!(latest["nativeGoalOwnerCorrections"].is_array());
        assert!(!latest["continuation"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|requirement| requirement["id"] == "R4"));
    } else {
        assert!(latest["continuation"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|requirement| requirement["id"] == "R4"));
        assert!(latest.get("nativeGoalOwnerCorrections").is_none());
    }
}

#[test]
fn tampered_reader_barrier_never_grants_either_owner() {
    let fixture = Fixture::new("owner-correction-tampered-marker");
    let preview = fixture.preview("tamper");
    install_pending_marker(&fixture.old, &preview);
    let (owner_path, _) = paths(&fixture.old);
    let mut marker: Value = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    marker["pending"]["reason"] = json!("unrecorded replacement");
    fs::write(owner_path, marker.to_string()).unwrap();
    let before = fixture.old.native_store_snapshot();
    assert!(!fixture.execute(&preview, "applyArgv").status.success());
    assert!(!fixture.execute(&preview, "abortArgv").status.success());
    assert_eq!(fixture.old.native_store_snapshot(), before);
}

#[test]
fn correction_racing_work_admission_never_records_a_stale_owner() {
    let fixture = Fixture::new("owner-correction-work-race");
    let preview = fixture.preview("work-race");
    let (owner_path, journal) = paths(&fixture.old);
    let initial: Value = serde_json::from_slice(&fs::read(owner_path).unwrap()).unwrap();
    let argv = preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let config = fixture.old.root.join("exitbind.json");
    let work = [
        "work",
        "begin",
        "change",
        "--goal",
        "Concurrent context-only Work",
        "--check-command",
        "true",
        "--review-policy",
        "omitted",
        "--config",
        config.to_str().unwrap(),
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect::<Vec<_>>();
    let correction = concurrent_command(&fixture.old, &argv[0], &argv[1..])
        .spawn()
        .unwrap();
    let work = concurrent_command(&fixture.old, env!("CARGO_BIN_EXE_exitbind"), &work)
        .spawn()
        .unwrap();
    let correction = correction.wait_with_output().unwrap();
    let work = work.wait_with_output().unwrap();
    let records = support::goal_journal::records(&journal);
    let mut effective = initial["ownerIdentitySha256"].clone();
    let mut observed_admission = false;
    let mut observed_correction = false;
    for record in records {
        if let Some(correction) = record["nativeGoalOwnerCorrections"]
            .as_array()
            .and_then(|items| items.last())
        {
            effective = correction["targetConfig"]["localIdentitySha256"].clone();
            observed_correction = true;
        }
        for field in ["journeyAdmissions", "journeyMembers"] {
            for item in record[field].as_array().into_iter().flatten() {
                observed_admission = true;
                assert_eq!(item["ownerIdentitySha256"], effective, "{field}: {item}");
            }
        }
    }
    assert!(
        observed_correction || observed_admission,
        "correction={correction:?}; work={work:?}"
    );
    if correction.status.success() {
        assert!(observed_correction);
    }
    if !correction.status.success() && observed_admission {
        assert!(!observed_correction);
    }
}

#[test]
fn restoring_a_legacy_owner_after_commit_cannot_hide_the_correction() {
    let fixture = Fixture::new("owner-correction-marker-downgrade");
    let (owner, journal) = paths(&fixture.old);
    let original = fs::read(&owner).unwrap();
    let preview = fixture.preview("downgrade");
    value(&fixture.execute(&preview, "applyArgv"));
    let work = fixture
        .target
        .begin_ordinary("Retain the native Work association", "true", &[]);
    let attached = fixture
        .target
        .value(&["work", "next", &work, "--json"], None);
    assert_eq!(
        attached["presentation"]["goalProgress"]["goal"], "Import names safely and show a count",
        "{attached}"
    );
    let journal_after = fs::read(&journal).unwrap();
    fs::write(&owner, original).unwrap();
    assert!(!fixture
        .old
        .call(&["goal", "status", "--json"], None)
        .status
        .success());
    assert!(!fixture
        .target
        .call(&["goal", "status", "--json"], None)
        .status
        .success());
    let next = fixture
        .target
        .value(&["work", "next", &work, "--json"], None);
    let progress = &next["presentation"]["goalProgress"];
    assert_eq!(
        progress["decomposition"]["reason"], "native_session_goal_context_invalid",
        "{next}"
    );
    assert!(progress.get("journeyCompletion").is_none(), "{next}");
    assert!(!fixture
        .target
        .call(&["goal", "close", "--goal-id", "import"], None)
        .status
        .success());
    assert_eq!(fs::read(journal).unwrap(), journal_after);
}

#[test]
fn non_native_context_cannot_select_or_correct_a_session_goal() {
    let mut fixture = Fixture::new("owner-correction-non-native");
    let before = fixture.old.native_store_snapshot();
    fixture.old.native_session = None;
    let output = correction_call(&fixture, "non-native");
    assert!(!output.status.success());
    fixture.old.native_session = fixture.target.native_session.clone();
    assert_eq!(fixture.old.native_store_snapshot(), before);
}

#[test]
fn a_committed_owner_change_is_reversed_only_by_another_bound_event() {
    let fixture = Fixture::new("owner-correction-reverse");
    let (_, journal) = paths(&fixture.old);
    let first = fixture.preview("forward");
    value(&fixture.execute(&first, "applyArgv"));
    let forward_prefix = fs::read(&journal).unwrap();
    let target_config = fixture.target.root.join("exitbind.json");
    let target_bytes = fs::read_to_string(&target_config).unwrap();
    fs::write(&target_config, format!("{target_bytes}\n")).unwrap();
    let source = fs::read(fixture.old.root.join("request.txt")).unwrap();
    let relocation = json!([{"originalRef":"request.txt","recordedSha256":sha(&source),
        "targetPath":"request.txt"}])
    .to_string();
    let old_config = fixture.old.root.join("exitbind.json");
    let preview = fixture.target.value(
        &[
            "goal",
            "correct-owner",
            "--target-config",
            old_config.to_str().unwrap(),
            "--relocations",
            &relocation,
            "--request-id",
            "reverse",
            "--reason",
            "Reverse the correction",
            "--old-writers-quiesced",
            "--json",
        ],
        None,
    );
    assert_ne!(
        preview["decision"]["oldConfig"]["configSha256"],
        first["decision"]["targetConfig"]["configSha256"]
    );
    assert_eq!(
        preview["decision"]["oldConfig"]["path"],
        first["decision"]["targetConfig"]["path"]
    );
    let argv = preview["applyArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = value(&fixture.target.call_exact_argv(&argv));
    assert_eq!(applied["kind"], "native_goal_owner_correction_applied");
    assert!(fs::read(&journal).unwrap().starts_with(&forward_prefix));
    let current = support::goal_journal::latest(&journal);
    assert_eq!(
        current["nativeGoalOwnerCorrections"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(fixture.old.status()["goalId"], "import");
    fixture.target.reject(
        &["goal", "cover", "--goal-id", "import"],
        "owned by another configured project",
    );
}

#[test]
fn recovery_keeps_corrected_owner_after_the_goal_changes() {
    let fixture = Fixture::new("owner-correction-next-goal-recovery");
    let preview = fixture.preview("owner-before-next-goal");
    value(&fixture.execute(&preview, "applyArgv"));
    fixture.target.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "next-goal",
            "--goal",
            "Continue with the next Goal",
        ],
        None,
    );
    let shown = fixture
        .target
        .value(&["session", "recovery", "show", "--json"], None);
    assert_eq!(shown["goal"]["goalId"], "next-goal");
    assert_eq!(fixture.target.status()["goalId"], "next-goal");
    fixture.old.reject(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "next-goal",
            "--goal",
            "An unauthorized update",
        ],
        "owned by another configured project",
    );
}

#[test]
fn recovery_show_refuses_over_limit_journals_for_legacy_and_corrected_owners() {
    for protocol in [1, 2] {
        let fixture = Fixture::new(&format!("owner-recovery-cli-bound-{protocol}"));
        if protocol == 2 {
            let preview = fixture.preview("before-limit-check");
            value(&fixture.execute(&preview, "applyArgv"));
        }
        let current = if protocol == 1 {
            &fixture.old
        } else {
            &fixture.target
        };
        let (_, journal) = paths(&fixture.old);
        assert_eq!(
            current.value(&["session", "recovery", "show", "--json"], None)["goal"]["goalId"],
            "import"
        );
        fs::write(&journal, "{}\n".repeat(4097)).unwrap();
        let records = current.call(&["session", "recovery", "show", "--json"], None);
        assert!(
            !records.status.success(),
            "protocol {protocol}: {records:?}"
        );
        assert!(
            String::from_utf8_lossy(&records.stdout).contains("4096 physical records"),
            "{records:?}"
        );
        let (session, _) = current.native_session.as_ref().unwrap();
        let payload = json!({"hook_event_name":"Stop","cwd":current.root,"session_id":session,
            "stop_hook_active":false,"last_assistant_message":"A side answer."});
        let mut command = concurrent_command(
            current,
            env!("CARGO_BIN_EXE_exitbind"),
            &["hook-run".into(), "--host".into(), "codex".into()],
        );
        command.stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        if let Err(error) = child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
        {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        let stop = child.wait_with_output().unwrap();
        assert!(stop.status.success(), "{stop:?}");
        assert!(
            stop.stdout.is_empty(),
            "an over-limit Goal cannot provide Stop owner credit"
        );
        fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&journal)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        let bytes = current.call(&["session", "recovery", "show", "--json"], None);
        assert!(!bytes.status.success(), "protocol {protocol}: {bytes:?}");
        assert!(
            String::from_utf8_lossy(&bytes.stdout).contains("byte bound"),
            "{bytes:?}"
        );
    }
}

#[test]
fn an_adopting_session_cannot_correct_another_sessions_goal() {
    let mut fixture = Fixture::new("owner-correction-adoption");
    fixture.old.begin("R1", "sh check.sh R1");
    fixture
        .old
        .set_native_session("owner-correction-adopted-session");
    let status = fixture.old.status();
    let route = status["goalProgress"]["adoptRoute"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    value(&fixture.old.call_exact_argv(&route));
    let before = fixture.old.native_store_snapshot();
    assert!(!correction_call(&fixture, "adopted").status.success());
    assert_eq!(fixture.old.native_store_snapshot(), before);
}
