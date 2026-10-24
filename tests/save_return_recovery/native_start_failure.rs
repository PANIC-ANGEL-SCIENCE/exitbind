use super::*;

#[test]
fn host_write_denial_is_classified_and_relaunches_with_another_sandbox() {
    let fixture = Fixture::new("native-start-host-denial");
    let (work, worker) = fixture.pending_worker("native host write denial");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("fake-host-denial-codex");
    fs::write(
        &executable,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' 'write failed: Permission denied (os error 13)' 'private stderr body' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

    let failed = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!failed.status.success(), "{}", text(&failed));
    assert!(!text(&failed).contains("private stderr body"));
    assert!(
        text(&failed).contains("the host denied the provider's write"),
        "{}",
        text(&failed)
    );
    assert!(
        text(&failed).contains("--sandbox-mode danger-full-access"),
        "the other sandbox is printed: {}",
        text(&failed)
    );

    let journal: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join(".exitbind/native-actions")
                .join(&work)
                .join(format!("{assignment}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(journal["status"], "not_started");
    assert!(journal["threadId"].is_null());
    assert!(journal["observation"]["diagnostic"]["codes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|code| code == "host_permission_denied"));

    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["reconciliation"], "relaunch_ready");
    assert_eq!(inspected["nextAction"]["safe"], true);
    assert!(
        inspected["nextAction"]["reason"]
            .as_str()
            .unwrap()
            .contains("another sandbox"),
        "{inspected}"
    );
    let command = inspected["nextAction"]["command"].as_array().unwrap();
    assert!(command.iter().any(|word| word == "--sandbox-mode"));
    assert!(command.iter().any(|word| word == "danger-full-access"));

    // The printed relaunch starts the same assignment in the other sandbox.
    let bin = fixture.root.join("sandbox-bin");
    fs::create_dir(&bin).unwrap();
    let codex = bin.join("codex");
    fs::write(
        &codex,
        r##"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"sandbox-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"{\"outcome\":\"completed\",\"summary\":\"relaunched\",\"reason\":\"\"}"}}'
printf '%s\n' '{"type":"turn.completed","status":"completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'
"##,
    )
    .unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).unwrap();
    let route: Vec<&str> = command.iter().map(|word| word.as_str().unwrap()).collect();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let relaunched = support::git_topology::command(route[0])
        .current_dir(&fixture.root)
        .args(&route[1..])
        .env("EXITBIND_NATIVE_RECOVERY_TEST", "1")
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(relaunched.status.success(), "{}", text(&relaunched));
    let journal: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join(".exitbind/native-actions")
                .join(&work)
                .join(format!("{assignment}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(journal["status"], "completed");
    assert_eq!(journal["threadId"], "sandbox-thread");
    let after = fixture.value(&["work", "next", &work], b"");
    assert_eq!(after["work"], work.as_str());
    assert_eq!(after["next"]["action"], "check");
}

#[test]
fn a_provider_thread_refuses_the_start_relaunch() {
    let fixture = Fixture::new("native-start-thread-refusal");
    let (work, worker) = fixture.pending_worker("native thread refusal");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("fake-thread-failure-codex");
    fs::write(
        &executable,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"kept-thread\"}'\nprintf '%s\\n' 'Permission denied (os error 13)' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

    let failed = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!failed.status.success(), "{}", text(&failed));
    assert!(
        text(&failed).contains("native assignment started a provider thread"),
        "{}",
        text(&failed)
    );
    let journal: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join(".exitbind/native-actions")
                .join(&work)
                .join(format!("{assignment}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(journal["status"], "started");
    assert_eq!(journal["threadId"], "kept-thread");

    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["reconciliation"], "resume_ready");
    assert!(inspected["nextAction"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .any(|word| word == "--resume"));

    // A thread exists, so the start relaunch never applies: a plain act is
    // refused towards --resume, and the another-sandbox form is refused as a
    // changed execution parameter without changing the journal.
    let refused = fixture.call(&["work", "act", &work], b"");
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("native assignment is already started"),
        "{}",
        text(&refused)
    );
    let changed = fixture.call(
        &["work", "act", &work, "--sandbox-mode", "danger-full-access"],
        b"",
    );
    assert!(!changed.status.success(), "{}", text(&changed));
    assert!(
        text(&changed).contains("refuses changed execution parameters"),
        "{}",
        text(&changed)
    );
    let after: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join(".exitbind/native-actions")
                .join(&work)
                .join(format!("{assignment}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(after, journal);
}
