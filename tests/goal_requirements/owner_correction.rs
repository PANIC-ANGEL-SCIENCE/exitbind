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
    let token = work.strip_prefix("smw_").expect("native Work handle");
    let companion_path = fixture
        .target
        .root
        .join(format!(".exitbind/runs/work-{token}.session-goal.json"));
    let companion: Value = serde_json::from_slice(&fs::read(companion_path).unwrap()).unwrap();
    let marker: Value = serde_json::from_slice(&fs::read(&owner).unwrap()).unwrap();
    assert_eq!(companion["kind"], "native_session_goal_context");
    assert_eq!(companion["work"], work);
    assert_eq!(companion["parentGoalId"], "import");
    assert_eq!(companion["sessionKey"], marker["sessionKey"]);
    let attached = fixture
        .target
        .value(&["work", "next", &work, "--json", "--full"], None);
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
        .value(&["work", "next", &work, "--json", "--full"], None);
    let progress = &next["presentation"]["goalProgress"];
    assert_eq!(progress["overall"], "unavailable", "{next}");
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

fn historical_preview(fixture: &Fixture, selection: &str, id: &str) -> Output {
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
            "Correct the selected project and retain prior Work without credit",
            "--old-writers-quiesced",
            "--uncredited-members",
            selection,
            "--source-writers-quiesced",
            "--json",
        ],
        None,
    )
}

#[test]
fn historical_owner_correction_requires_exact_target_set_and_preserves_work() {
    let fixture = Fixture::new("owner-correction-historical-set");
    let first = fixture
        .target
        .begin_ordinary("First linked Work", "true", &[]);
    let second = fixture
        .target
        .begin_ordinary("Second linked Work", "true", &[]);
    let (_, journal) = paths(&fixture.old);
    let before_goal = fs::read(&journal).unwrap();
    let before_members = support::goal_journal::latest(&journal)["journeyMembers"].clone();
    let work_path = |work: &str| {
        fixture
            .target
            .root
            .join(".exitbind/runs")
            .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()))
    };
    let before_first = fs::read(work_path(&first)).unwrap();
    let before_second = fs::read(work_path(&second)).unwrap();
    let partial = json!([first]).to_string();
    assert!(!historical_preview(&fixture, &partial, "partial")
        .status
        .success());
    let selected = json!([first, second]).to_string();
    let wrong_owner = fixture.target.call(
        &[
            "goal",
            "correct-owner",
            "--target-config",
            fixture.target.root.join("exitbind.json").to_str().unwrap(),
            "--relocations",
            &fixture.relocations,
            "--request-id",
            "wrong-owner",
            "--reason",
            "Refuse an incorrect owner",
            "--old-writers-quiesced",
            "--uncredited-members",
            &selected,
            "--source-writers-quiesced",
            "--json",
        ],
        None,
    );
    assert!(!wrong_owner.status.success());
    let refusal = format!(
        "{}{}",
        String::from_utf8_lossy(&wrong_owner.stdout),
        String::from_utf8_lossy(&wrong_owner.stderr)
    );
    assert!(
        refusal.contains("owned by another configured project"),
        "{refusal}"
    );
    assert_eq!(fs::read(&journal).unwrap(), before_goal);
    let preview = value(&historical_preview(&fixture, &selected, "historical-set"));
    assert_eq!(preview["decision"]["version"], 2);
    assert_eq!(
        preview["decision"]["uncreditedMembers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(fs::read(&journal).unwrap(), before_goal);
    let applied = value(&fixture.execute(&preview, "applyArgv"));
    assert_eq!(applied["kind"], "native_goal_owner_correction_applied");
    let after = support::goal_journal::latest(&journal);
    assert_eq!(after["journeyMembers"], before_members);
    assert_eq!(fs::read(work_path(&first)).unwrap(), before_first);
    assert_eq!(fs::read(work_path(&second)).unwrap(), before_second);
    let before_supersede_goal = fs::read(&journal).unwrap();
    let before_supersede_runs = fixture.target.run_entries();
    let successor = fixture.target.call(
        &[
            "work",
            "supersede",
            &first,
            "--reason",
            "Try to continue the retained historical Work",
            "--json",
        ],
        None,
    );
    assert!(!successor.status.success(), "{successor:?}");
    let refusal = format!(
        "{}{}",
        String::from_utf8_lossy(&successor.stdout),
        String::from_utf8_lossy(&successor.stderr)
    );
    assert!(
        refusal.contains("owner") || refusal.contains("context"),
        "{refusal}"
    );
    assert_eq!(fs::read(&journal).unwrap(), before_supersede_goal);
    assert_eq!(fs::read(work_path(&first)).unwrap(), before_first);
    assert_eq!(fixture.target.run_entries(), before_supersede_runs);
    let status = fixture.target.status();
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"], "blocked",
        "{status}"
    );
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["members"]["ownerCorrectionUncredited"],
        2
    );
}

#[test]
fn historical_owner_correction_needs_fresh_reviewed_named_journey_integration() {
    let fixture = Fixture::new("owner-correction-historical-close");
    let first = fixture
        .target
        .begin_ordinary("Original unfinished Work", "true", &[]);
    let second = fixture
        .target
        .begin_ordinary("Original independent Work", "true", &[]);
    fixture.target.drive(&second, true);
    let (_, journal) = paths(&fixture.old);
    let raw_second = fs::read_to_string(fixture.target.root.join(".exitbind/runs").join(format!(
        "work-{}.jsonl",
        second.strip_prefix("smw_").unwrap()
    )))
    .unwrap();
    let head: Value = serde_json::from_str(raw_second.lines().last().unwrap()).unwrap();
    let old_head = head["eventSha256"].as_str().unwrap().to_owned();
    let goal = "Import names safely and show a count";
    for (question, reference) in [
        ("Prior Work handle", second.as_str()),
        ("Prior Work head", old_head.as_str()),
        ("Prior published result", "https://example.invalid/prior"),
    ] {
        fixture.old.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                goal,
                "--decision",
                question,
                "--disposition",
                "accepted",
                "--result-ref",
                reference,
            ],
            None,
        );
    }
    let before = support::goal_journal::latest(&journal);
    let selection = json!([first, second]).to_string();
    let preview = value(&historical_preview(
        &fixture,
        &selection,
        "historical-close",
    ));
    value(&fixture.execute(&preview, "applyArgv"));
    let after = support::goal_journal::latest(&journal);
    assert_eq!(after["decisions"], before["decisions"]);
    assert_eq!(after["journeyMembers"], before["journeyMembers"]);
    assert_eq!(
        fixture.target.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    let pre_integration = fixture.target.call(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Integrate before new member",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh all",
            "--review-policy",
            "required",
            "--scope",
            "integration",
            "--journey-integration",
        ],
        None,
    );
    assert!(!pre_integration.status.success(), "{pre_integration:?}");
    let validation = fixture.target.begin_ordinary(
        "Fresh named validation",
        "sh check.sh all",
        &[
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
        ],
    );
    fixture.target.drive(&validation, true);
    let validation_ledger = fs::read_to_string(fixture.target.root.join(".exitbind/runs").join(
        format!("work-{}.jsonl", validation.strip_prefix("smw_").unwrap()),
    ))
    .unwrap();
    let validation_head: Value =
        serde_json::from_str(validation_ledger.lines().last().unwrap()).unwrap();
    let validation_head = validation_head["eventSha256"].as_str().unwrap().to_owned();
    fixture.target.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &validation,
        ],
        None,
    );
    fixture.target.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &validation,
        ],
        "journey",
    );
    let integration = fixture.target.begin_ordinary(
        "Fresh named journey integration",
        "sh check.sh all",
        &[
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--scope",
            "integration",
            "--journey-integration",
        ],
    );
    fixture.target.drive(&integration, true);
    assert_eq!(
        fixture.target.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    for question in [
        "Prior Work handle",
        "Prior Work head",
        "Prior published result",
    ] {
        fixture.target.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                goal,
                "--decision",
                question,
                "--disposition",
                "open",
            ],
            None,
        );
        fixture.target.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                goal,
                "--decision",
                question,
                "--disposition",
                "accepted",
                "--result-ref",
                &validation,
            ],
            None,
        );
    }
    // Isolate each historical spelling so another unresolved decision cannot
    // accidentally explain its refusal.
    for (question, reference) in [
        ("Prior Work handle", second.as_str()),
        ("Prior Work head", old_head.as_str()),
        ("Prior published result", "https://example.invalid/prior"),
        ("Fresh Work head", validation_head.as_str()),
    ] {
        fixture.target.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                goal,
                "--decision",
                question,
                "--disposition",
                "accepted",
                "--result-ref",
                reference,
            ],
            None,
        );
        let refused = fixture.target.call(
            &[
                "goal",
                "close",
                "--goal-id",
                "import",
                "--result-ref",
                &integration,
            ],
            None,
        );
        assert!(
            !refused.status.success(),
            "historical result {question} must not close the Goal"
        );
        fixture.target.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                goal,
                "--decision",
                question,
                "--disposition",
                "open",
            ],
            None,
        );
        fixture.target.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                goal,
                "--decision",
                question,
                "--disposition",
                "accepted",
                "--result-ref",
                &validation,
            ],
            None,
        );
    }
    let closed = fixture.target.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &integration,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    let history = support::goal_journal::records(&journal);
    assert!(history
        .iter()
        .any(|item| item["decisions"] == before["decisions"]));
}

#[test]
fn historical_owner_correction_rejects_stale_preview_and_recovers_pending_barrier() {
    let fixture = Fixture::new("owner-correction-historical-retry");
    let work = fixture
        .target
        .begin_ordinary("Retain the original Work", "true", &[]);
    let selection = json!([work]).to_string();
    let (owner_path, journal) = paths(&fixture.old);
    let ledger = fixture
        .target
        .root
        .join(".exitbind/runs")
        .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()));
    let work_before = fs::read(&ledger).unwrap();
    let stale = value(&historical_preview(
        &fixture,
        &selection,
        "historical-stale",
    ));
    fixture.old.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "import",
            "--goal",
            "Import names safely and show a count",
            "--finding",
            "A later recorded observation",
        ],
        None,
    );
    let later = fs::read(&journal).unwrap();
    assert!(!fixture.execute(&stale, "applyArgv").status.success());
    assert_eq!(fs::read(&journal).unwrap(), later);
    assert_eq!(fs::read(&ledger).unwrap(), work_before);

    let preview = value(&historical_preview(
        &fixture,
        &selection,
        "historical-pending",
    ));
    let original_owner: Value = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    install_pending_marker(&fixture.old, &preview);
    let current = fixture.old.status();
    assert_eq!(
        current["ownerCorrectionRecovery"]["applyArgv"],
        preview["applyArgv"]
    );
    assert_eq!(
        current["ownerCorrectionRecovery"]["abortArgv"],
        preview["abortArgv"]
    );
    let config = fixture.target.root.join("exitbind.json");
    let unavailable_config = fixture.target.root.join("exitbind.json.unavailable");
    fs::rename(&config, &unavailable_config).unwrap();
    let pending_store = fixture.old.native_store_snapshot();
    assert!(!fixture.execute(&preview, "applyArgv").status.success());
    assert_eq!(fixture.old.native_store_snapshot(), pending_store);
    let mut wrong_binding = preview["abortArgv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let index = wrong_binding
        .iter()
        .position(|item| item == "--current-binding")
        .unwrap();
    wrong_binding[index + 1] = "a".repeat(64);
    assert!(!fixture.old.call_exact_argv(&wrong_binding).status.success());
    assert_eq!(fixture.old.native_store_snapshot(), pending_store);
    assert_eq!(
        value(&fixture.execute(&preview, "abortArgv"))["kind"],
        "native_goal_owner_correction_aborted"
    );
    let restored_owner: Value = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    assert_eq!(restored_owner["original"], original_owner);
    assert!(restored_owner["pending"].is_null());
    assert_eq!(fs::read(&journal).unwrap(), later);
    assert_eq!(fs::read(&ledger).unwrap(), work_before);
    fs::rename(unavailable_config, config).unwrap();
    assert!(!fixture.execute(&preview, "applyArgv").status.success());
    assert_eq!(fs::read(&journal).unwrap(), later);
    let retry = value(&historical_preview(
        &fixture,
        &selection,
        "historical-after-abort",
    ));
    let applied = value(&fixture.execute(&retry, "applyArgv"));
    assert_eq!(applied["kind"], "native_goal_owner_correction_applied");
    assert_eq!(fs::read(&ledger).unwrap(), work_before);
    assert_eq!(
        value(&fixture.execute(&retry, "applyArgv"))["kind"],
        "native_goal_owner_correction_replayed"
    );
}

#[test]
fn historical_owner_correction_refuses_real_pending_admission_and_missing_attestation() {
    use std::os::unix::fs::symlink;

    struct RestoreRuns {
        runs: PathBuf,
        saved: PathBuf,
    }
    impl Drop for RestoreRuns {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.runs);
            if self.saved.exists() {
                fs::rename(&self.saved, &self.runs).expect("restore isolated Work directory");
            }
        }
    }
    let fixture = Fixture::new("owner-correction-historical-admission");
    let work = fixture
        .target
        .begin_ordinary("Existing target Work", "true", &[]);
    let selected = json!([work]).to_string();
    let target_config = fixture.target.root.join("exitbind.json");
    let before = fixture.old.native_store_snapshot();
    let missing_attestation = fixture.old.call(
        &[
            "goal",
            "correct-owner",
            "--target-config",
            target_config.to_str().unwrap(),
            "--relocations",
            &fixture.relocations,
            "--request-id",
            "without-source-attestation",
            "--reason",
            "Do not infer source writer quiescence",
            "--old-writers-quiesced",
            "--uncredited-members",
            &selected,
            "--json",
        ],
        None,
    );
    assert!(!missing_attestation.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&missing_attestation.stdout),
        String::from_utf8_lossy(&missing_attestation.stderr)
    );
    assert!(diagnostic.contains("quiescence"), "{diagnostic}");
    assert_eq!(fixture.old.native_store_snapshot(), before);
    let preview = value(&historical_preview(
        &fixture,
        &selected,
        "before-pending-admission",
    ));
    let (_, journal) = paths(&fixture.old);
    let ledger = fixture
        .target
        .root
        .join(".exitbind/runs")
        .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()));
    let work_before = fs::read(&ledger).unwrap();

    // Leave an actual recorded admission when the confined Run path refuses.
    let runs = fixture.target.root.join(".exitbind/runs");
    let saved = fixture
        .target
        .root
        .join(".exitbind/runs-retained-for-admission");
    let outside = fixture.target.root.parent().unwrap().join("outside-runs");
    fs::create_dir(&outside).unwrap();
    fs::rename(&runs, &saved).unwrap();
    let restore = RestoreRuns {
        runs: runs.clone(),
        saved,
    };
    symlink(&outside, &runs).unwrap();
    let failed = fixture.target.call(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A real pending admission",
            "--check-command",
            "true",
            "--review-policy",
            "required",
            "--json",
        ],
        None,
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
    let pending = fixture.old.status();
    assert_eq!(pending["journeyAdmissions"].as_array().unwrap().len(), 1);
    let before_refusal = fs::read(&journal).unwrap();
    let refused = historical_preview(&fixture, &selected, "pending-admission");
    assert!(!refused.status.success());
    assert!(!fixture.execute(&preview, "applyArgv").status.success());
    assert_eq!(fs::read(&journal).unwrap(), before_refusal);
    assert_eq!(fs::read(&ledger).unwrap(), work_before);
}

#[test]
fn corrected_owner_emits_reviewed_integration_and_exact_close_routes() {
    let fixture = Fixture::new("owner-correction-reviewed-route");
    let old = fixture
        .target
        .begin_ordinary("Retained historical Work", "true", &[]);
    let preview = value(&historical_preview(
        &fixture,
        &json!([old]).to_string(),
        "reviewed-route",
    ));
    value(&fixture.execute(&preview, "applyArgv"));

    // The existing preparation route must not offer an unbound integration
    // while there is no fresh accepted journey member to freeze.
    let before = fixture.target.run_entries();
    let unavailable = fixture.target.value(
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
    assert!(unavailable["beginRoute"].is_null(), "{unavailable}");
    assert!(
        unavailable["journeyIntegrationOmitted"].is_string(),
        "{unavailable}"
    );
    assert_eq!(fixture.target.run_entries(), before);

    // A named integration without journey selection is a validation member.
    // Its earlier mapping must not be chosen as the eventual closure Work.
    let validation = fixture.target.begin_ordinary(
        "Fresh named validation",
        "sh check.sh all",
        &[
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--scope",
            "integration",
        ],
    );
    fixture.target.drive(&validation, true);
    fixture.target.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &validation,
        ],
        None,
    );

    // An accepted journey integration with omitted review cannot close.
    let omitted = fixture.target.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "An integration without review",
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
            "--journey-integration",
            "--review-policy",
            "omitted",
            "--json",
        ],
        None,
    )["work"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.target.drive(&omitted, true);
    fixture.target.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &omitted,
        ],
        "journey",
    );

    // A reviewed but non-named journey integration is current, yet it does
    // not satisfy the named Goal. A later eligible candidate must be chosen.
    let generic = fixture.target.begin_ordinary(
        "A reviewed journey without a named contract",
        "sh check.sh all",
        &["--artifact", "check.sh", "--journey-integration"],
    );
    fixture.target.drive(&generic, true);
    assert!(!fixture
        .target
        .call(
            &[
                "goal",
                "close",
                "--goal-id",
                "import",
                "--result-ref",
                &generic,
            ],
            None
        )
        .status
        .success());

    let next = fixture.target.status()["requirements"]["nextAction"].clone();
    let prepare_argv = next["command"]
        .as_array()
        .expect("current prepare route")
        .iter()
        .map(|item| match item.as_str().unwrap() {
            "<PROJECT_CHECKER>" => "check.sh".to_owned(),
            "<CHECK_COMMAND>" => "sh check.sh all".to_owned(),
            other => other.to_owned(),
        })
        .collect::<Vec<_>>();
    assert!(
        prepare_argv.iter().any(|word| word == "prepare"),
        "{prepare_argv:?}"
    );
    let prepared = value(&fixture.target.call_exact_argv(&prepare_argv));
    assert!(
        prepared["journeyIntegrationOmitted"].is_null(),
        "{prepared}"
    );
    let begin_argv = prepared["beginRoute"]["command"]
        .as_array()
        .expect("ready begin route")
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(
        begin_argv
            .iter()
            .any(|word| word == "--journey-integration"),
        "{begin_argv:?}"
    );
    let review = begin_argv
        .iter()
        .position(|word| word == "--review-policy")
        .unwrap();
    assert_eq!(begin_argv[review + 1], "required");
    let started = value(&fixture.target.call_exact_argv(&begin_argv));
    let integration = started["work"].as_str().unwrap().to_owned();
    fixture.target.drive(&integration, true);

    // The emitted close route must select the actual reviewed named journey
    // result even though the earlier ordinary integration remains mapped.
    let close = fixture.target.status()["requirements"]["nextAction"].clone();
    let close_argv = close["command"]
        .as_array()
        .expect("current close route")
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(
        close_argv.iter().any(|word| word == "close"),
        "{close_argv:?}"
    );
    let result = close_argv
        .iter()
        .position(|word| word == "--result-ref")
        .unwrap();
    assert_eq!(close_argv[result + 1], integration);
    assert_ne!(close_argv[result + 1], validation);
    let closed = value(&fixture.target.call_exact_argv(&close_argv));
    assert_eq!(closed["kind"], "goal_completion_response");
    assert_eq!(closed["completionEvent"]["kind"], "session_goal_completed");
    let status = fixture.target.status();
    assert_eq!(status["closure"]["closed"], true);
    assert_eq!(status["currentReadiness"]["state"], "current");
    assert_eq!(status["goalProgress"]["overall"], "complete");
    assert_eq!(status["presentation"]["terminal"], TERMINAL);
}

#[test]
fn historical_owner_correction_reserves_two_existing_admission_slots() {
    let fixture = Fixture::new("owner-correction-admission-capacity");
    let (owner_path, journal) = paths(&fixture.old);
    let mut works = Vec::new();
    for index in 0..30 {
        works.push(fixture.target.begin_ordinary(
            &format!("Historical target Work {index}"),
            "true",
            &[],
        ));
    }
    let selection = json!(works).to_string();
    let before_preview = fs::read(&journal).unwrap();
    let preview = value(&historical_preview(&fixture, &selection, "capacity-thirty"));
    assert_eq!(
        preview["decision"]["uncreditedMembers"]
            .as_array()
            .unwrap()
            .len(),
        30
    );
    assert_eq!(fs::read(&journal).unwrap(), before_preview);

    // A correctly hashed pending decision also cannot seal an oversized set.
    let owner_before = fs::read(&owner_path).unwrap();
    let mut forged = preview.clone();
    let mut extra = forged["decision"]["uncreditedMembers"][0].clone();
    extra["work"] = json!(format!("smw_{}", "f".repeat(64)));
    forged["decision"]["uncreditedMembers"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    let mut unsigned = forged["decision"].clone();
    unsigned.as_object_mut().unwrap().remove("previewSha256");
    unsigned.as_object_mut().unwrap().remove("decisionSha256");
    forged["decision"]["previewSha256"] = json!(sha(canonical(&unsigned).as_bytes()));
    let mut unsigned = forged["decision"].clone();
    unsigned.as_object_mut().unwrap().remove("decisionSha256");
    forged["decision"]["decisionSha256"] = json!(sha(canonical(&unsigned).as_bytes()));
    install_pending_marker(&fixture.old, &forged);
    let invalid = fixture.old.call(&["goal", "status", "--json"], None);
    assert!(!invalid.status.success(), "{invalid:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&invalid.stdout),
        String::from_utf8_lossy(&invalid.stderr)
    );
    assert!(
        diagnostic.contains("owner correction decision is invalid"),
        "{diagnostic}"
    );
    fs::write(owner_path, owner_before).unwrap();
    assert_eq!(fs::read(&journal).unwrap(), before_preview);

    // The existing journey admits both additional Works, while a correction
    // cannot consume either of the two slots needed after its cutover.
    for count in [31, 32] {
        works.push(fixture.target.begin_ordinary(
            &format!("Historical target Work {}", count - 1),
            "true",
            &[],
        ));
        let current = fs::read(&journal).unwrap();
        let selection = json!(works).to_string();
        let refused = historical_preview(&fixture, &selection, &format!("capacity-{count}"));
        assert!(!refused.status.success(), "{refused:?}");
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&refused.stdout),
            String::from_utf8_lossy(&refused.stderr)
        );
        assert!(
            diagnostic.contains("two free journey slots"),
            "{diagnostic}"
        );
        assert_eq!(fs::read(&journal).unwrap(), current);
        assert!(!fixture.execute(&preview, "applyArgv").status.success());
        assert_eq!(fs::read(&journal).unwrap(), current);
    }
}

#[test]
fn corrected_owner_prepares_one_validation_for_all_current_requirements() {
    let fixture = Fixture::new("owner-correction-single-validation");
    let old = fixture
        .target
        .begin_ordinary("Retained historical Work", "true", &[]);
    let preview = value(&historical_preview(
        &fixture,
        &json!([old]).to_string(),
        "one-validation",
    ));
    value(&fixture.execute(&preview, "applyArgv"));

    let status = fixture.target.status();
    let next = status["requirements"]["nextAction"].clone();
    let rows = status["requirements"]["requirements"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert_eq!(row["nextAction"]["command"], next["command"]);
        assert_eq!(
            row["nextAction"]["afterCurrentAcceptance"],
            next["afterCurrentAcceptance"]
        );
    }
    let prepare = next["command"]
        .as_array()
        .expect("validation preparation route")
        .iter()
        .map(|item| match item.as_str().unwrap() {
            "<PROJECT_CHECKER>" => "check.sh".to_owned(),
            "<CHECK_COMMAND>" => "sh check.sh all".to_owned(),
            other => other.to_owned(),
        })
        .collect::<Vec<_>>();
    assert!(prepare.iter().any(|word| word == "prepare"), "{prepare:?}");
    let requirement = prepare
        .iter()
        .position(|word| word == "--requirement")
        .unwrap();
    assert_eq!(prepare[requirement + 1], "R1,R2,R3");
    assert!(!prepare.iter().any(|word| word == "--scope"), "{prepare:?}");
    let assignment = next["afterCurrentAcceptance"]["command"]
        .as_array()
        .expect("existing assignment route")
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let prepared = value(&fixture.target.call_exact_argv(&prepare));
    let begin = prepared["beginRoute"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| match item.as_str().unwrap() {
            "<REVIEW_DECISION>" => "required".to_owned(),
            other => other.to_owned(),
        })
        .collect::<Vec<_>>();
    let started = value(&fixture.target.call_exact_argv(&begin));
    let validation = started["work"].as_str().unwrap().to_owned();
    fixture.target.drive(&validation, true);
    let assignment = assignment
        .into_iter()
        .map(|part| {
            if part == "<NEW_ACCEPTED_WORK>" {
                validation.clone()
            } else {
                part
            }
        })
        .collect::<Vec<_>>();
    value(&fixture.target.call_exact_argv(&assignment));
    let ready = fixture.target.status();
    assert_eq!(ready["requirements"]["coverageCurrent"], true, "{ready}");
    let integration = ready["requirements"]["nextAction"]["command"]
        .as_array()
        .expect("integration preparation route");
    assert!(
        integration.iter().any(|word| word == "prepare"),
        "{integration:?}"
    );
    let scope = integration
        .iter()
        .position(|word| word == "--scope")
        .unwrap();
    assert_eq!(integration[scope + 1], "integration");
}
