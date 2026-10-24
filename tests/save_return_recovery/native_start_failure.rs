use super::*;

#[test]
fn host_write_denial_is_classified_and_relaunches_with_another_sandbox() {
    let fixture = Fixture::new("native-start-host-denial");
    let (work, worker) = fixture.pending_worker("native host write denial");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("fake-host-denial-codex");
    // The first launch prints the sandbox's errno denial. A relaunch of the
    // same recorded executable then starts a provider thread, so the test
    // proves the route reuses that binary.
    fs::write(
        &executable,
        "#!/bin/sh\ncat >/dev/null\nif [ -f \"$0.ran\" ]; then\n\
         printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"sandbox-thread\"}'\n\
         printf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"outcome\\\":\\\"completed\\\",\\\"summary\\\":\\\"relaunched\\\",\\\"reason\\\":\\\"\\\"}\"}}'\n\
         printf '%s\\n' '{\"type\":\"turn.completed\",\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}'\n\
         exit 0\nfi\n\
         : > \"$0.ran\"\n\
         printf '%s\\n' 'write failed: Permission denied (os error 13)' 'private stderr body' >&2\nexit 1\n",
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
        text(&failed).contains("the host printed a sandbox write denial"),
        "{}",
        text(&failed)
    );
    // The cause is stated honestly: the same errno text also covers provider
    // config, auth and session reads, which no sandbox can repair.
    assert!(
        text(&failed).contains("cannot be told apart"),
        "{}",
        text(&failed)
    );
    assert!(
        text(&failed).contains("--sandbox-mode <SANDBOX_MODE>"),
        "the sandbox stays a placeholder: {}",
        text(&failed)
    );
    assert!(!text(&failed).contains("--sandbox-mode danger-full-access"));
    assert!(
        text(&failed).contains("fill SANDBOX_MODE with one of read-only, danger-full-access"),
        "{}",
        text(&failed)
    );
    assert!(text(&failed).contains("workspace-write is the recorded sandbox"));

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
    assert_eq!(journal["request"]["sandbox"], "workspace-write");
    assert!(journal["observation"]["diagnostic"]["codes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|code| code == "host_permission_denied"));

    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["reconciliation"], "relaunch_ready");
    // No sandbox is chosen by the product: the human must fill it.
    assert_eq!(inspected["nextAction"]["safe"], false);
    assert!(
        inspected["nextAction"]["reason"]
            .as_str()
            .unwrap()
            .contains("the human may choose another sandbox"),
        "{inspected}"
    );
    let next_action = &inspected["nextAction"];
    assert_eq!(
        next_action["placeholders"],
        serde_json::json!(["SANDBOX_MODE"])
    );
    assert_eq!(
        next_action["inputConstraints"]["SANDBOX_MODE"]["allowedValues"],
        serde_json::json!(["read-only", "danger-full-access"])
    );
    assert_eq!(
        next_action["inputConstraints"]["SANDBOX_MODE"]["example"],
        "read-only"
    );
    let command = next_action["command"].as_array().unwrap();
    assert!(!command.iter().any(|word| word == "danger-full-access"));
    let slot = command
        .iter()
        .position(|word| word == "<SANDBOX_MODE>")
        .unwrap_or_else(|| panic!("no sandbox placeholder: {inspected}"));
    assert_eq!(command[slot - 1], "--sandbox-mode");
    let binary = command
        .iter()
        .position(|word| word == executable.to_str().unwrap())
        .unwrap_or_else(|| panic!("the recorded binary is missing: {inspected}"));
    assert_eq!(command[binary - 1], "--codex-bin");

    // A not_started relaunch keeps the override check: only the sandbox may
    // differ from the recorded operation.
    let other = fixture.root.join("other-codex");
    fs::write(&other, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
    for changed in [
        vec!["--codex-bin".to_owned(), other.to_str().unwrap().to_owned()],
        vec!["--model".to_owned(), "another-model".to_owned()],
    ] {
        let mut args = vec!["work", "act", work.as_str()];
        args.extend(changed.iter().map(String::as_str));
        let refused = fixture.call(&args, b"");
        assert!(!refused.status.success(), "{}", text(&refused));
        assert!(
            text(&refused).contains("native relaunch refuses changed execution parameters"),
            "{}",
            text(&refused)
        );
    }

    // The emitted relaunch starts the same assignment with the recorded
    // binary in the sandbox the human chose.
    let route: Vec<String> = command
        .iter()
        .map(|word| {
            if word == "<SANDBOX_MODE>" {
                "read-only".to_owned()
            } else {
                word.as_str().unwrap().to_owned()
            }
        })
        .collect();
    let route: Vec<&str> = route.iter().map(String::as_str).collect();
    let relaunched = support::git_topology::command(route[0])
        .current_dir(&fixture.root)
        .args(&route[1..])
        .env("EXITBIND_NATIVE_RECOVERY_TEST", "1")
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
fn a_second_not_started_relaunch_is_not_safe_to_repeat_automatically() {
    let fixture = Fixture::new("native-start-repeat");
    let (work, worker) = fixture.pending_worker("native repeat");
    let assignment = worker["assignment"].as_str().unwrap();
    let executable = fixture.root.join("always-git-precondition-codex");
    fs::write(
        &executable,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' \
         'Not inside a trusted directory and --skip-git-repo-check was not specified.' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let args = [
        "work",
        "act",
        &work,
        "--codex-bin",
        executable.to_str().unwrap(),
        "--sandbox-mode",
        "read-only",
        "--timeout-ms",
        "123456",
    ];

    let failed = fixture.call(&args, b"");
    assert!(!failed.status.success(), "{}", text(&failed));
    assert!(
        text(&failed).contains("native assignment did not start (not_started)"),
        "{}",
        text(&failed)
    );
    assert!(
        !text(&failed).contains("already used its one automatic relaunch"),
        "the first relaunch is the one automatic attempt: {}",
        text(&failed)
    );
    let journal_path = fixture
        .root
        .join(".exitbind/native-actions")
        .join(&work)
        .join(format!("{assignment}.json"));
    let original: Value = serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(original["status"], "not_started");
    assert_eq!(original["request"]["sandbox"], "read-only");
    assert_eq!(original["request"]["timeoutMs"], 123456);

    // The first relaunch is the one automatic attempt: it is safe, and it
    // carries the recorded sandbox and timeout rather than the defaults.
    let first = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(first["nextAction"]["safe"], true);
    let route: Vec<String> = first["nextAction"]["command"]
        .as_array()
        .unwrap_or_else(|| panic!("no printed relaunch route: {first}"))
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect();
    for pair in [["--sandbox-mode", "read-only"], ["--timeout-ms", "123456"]] {
        let position = route
            .iter()
            .position(|word| word == pair[0])
            .unwrap_or_else(|| panic!("route lacks {}: {route:?}", pair[0]));
        assert_eq!(
            route[position + 1],
            pair[1],
            "recorded value changed: {route:?}"
        );
    }
    let route: Vec<&str> = route.iter().map(String::as_str).collect();
    let relaunched = support::git_topology::command(route[0])
        .current_dir(&fixture.root)
        .args(&route[1..])
        .env("EXITBIND_NATIVE_RECOVERY_TEST", "1")
        .output()
        .unwrap();
    assert!(!relaunched.status.success(), "{}", text(&relaunched));
    assert!(
        text(&relaunched).contains("already used its one automatic relaunch"),
        "the repeated failure says the automatic relaunch is spent: {}",
        text(&relaunched)
    );

    // The same failure again is not safe for automatic repetition, the state
    // says why, and the one archived not-started journal is the only record.
    let second = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(second["nextAction"]["safe"], false);
    assert!(
        second["nextAction"]["reason"]
            .as_str()
            .unwrap()
            .contains("not safe"),
        "{second}"
    );
    let journal: Value = serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(
        journal["request"], original["request"],
        "the relaunched request equals the retained original request"
    );
    let archives = fs::read_dir(journal_path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".not-started."))
        .count();
    assert_eq!(
        archives, 1,
        "the archived first attempt is the durable record"
    );
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
