use super::*;

#[test]
fn normal_create_replace_replay_and_check_progression() {
    let f = Fixture::new(false, false);
    f.permit();
    assert_eq!(f.edit_ok("read", "src/a.txt", b"")["exists"], false);
    let first = f.edit_ok("edit", "src/a.txt", b"hello");
    assert_eq!(first["effect"], "file-replaced");
    assert!(first.get("operation").is_none());
    let inode = fs::metadata(f.root.join("src/a.txt")).unwrap().ino();
    assert_eq!(
        f.edit_ok("edit", "src/a.txt", b"hello")["effect"],
        "no-change"
    );
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
    refused(f.edit("edit", "src/a.txt", b"different"), "conflicts");
    fs::set_permissions(f.root.join("src/a.txt"), fs::Permissions::from_mode(0o640)).unwrap();
    assert_eq!(f.edit_ok("refresh", "src/a.txt", b"")["content"], "hello");
    f.edit_ok("edit", "src/a.txt", b"next");
    assert_eq!(
        fs::metadata(f.root.join("src/a.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    f.ok(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
        ],
        b"source changed",
    );
    f.ok(&["work", "check", &f.work], b"");
    let next = f.ok(&["work", "next", &f.work, "--full"], b"");
    assert_eq!(next["next"]["role"], "lead");
    let lead = next["next"]["assignment"].as_str().unwrap();
    f.ok(
        &["work", "return", &f.work, lead, "--outcome", "accepted"],
        b"checked",
    );
    assert_eq!(
        f.edit_ok("edit", "src/a.txt", b"next")["effect"],
        "no-change"
    );
    refused(
        f.edit("edit", "src/new.txt", b"late"),
        "current worker assignment",
    );
}

#[test]
fn stale_and_absent_read_conflicts_preserve_external_creator() {
    let f = Fixture::new(false, false);
    f.permit();
    fs::write(f.root.join("src/a.txt"), "A").unwrap();
    assert_eq!(f.edit_ok("read", "src/a.txt", b"")["content"], "A");
    fs::write(f.root.join("src/a.txt"), "B").unwrap();
    let prepared = f.ok(&["work", "file", "prepare", &f.work, &f.assignment], b"");
    assert_eq!(prepared["tool"], f.tool.to_str().unwrap());
    assert_eq!(f.edit_ok("read", "src/a.txt", b"")["content"], "A");
    refused(
        f.edit("edit", "src/a.txt", b"based on A"),
        "differs from expected",
    );
    assert_eq!(fs::read_to_string(f.root.join("src/a.txt")).unwrap(), "B");
    assert_eq!(
        f.edit_ok("inspect", "src/a.txt", b"")["requestStatus"],
        "refused"
    );
    assert_eq!(f.edit_ok("refresh", "src/a.txt", b"")["content"], "B");
    f.edit_ok("edit", "src/a.txt", b"based on B");
    f.edit_ok("read", "src/new.txt", b"");
    fs::write(f.root.join("src/new.txt"), "other creator").unwrap();
    refused(
        f.edit("edit", "src/new.txt", b"creation"),
        "differs from expected",
    );
    assert_eq!(
        fs::read_to_string(f.root.join("src/new.txt")).unwrap(),
        "other creator"
    );
}

#[test]
fn focus_is_navigation_and_grants_and_config_cannot_be_inherited() {
    let f = Fixture::new(false, false);
    f.edit_ok("read", "src/a.txt", b"");
    refused(
        f.edit("edit", "src/a.txt", b"needs grant"),
        "mutation permit",
    );
    assert!(!f.root.join("src/a.txt").exists());
    f.permit();
    f.edit_ok("refresh", "src/a.txt", b"");
    let (other, assignment) = f.start();
    let tool = f.ok(&["work", "file", "prepare", &other, &assignment], b"");
    let other_tool = PathBuf::from(tool["tool"].as_str().unwrap());
    f.edit_ok("edit", "src/a.txt", b"first work");
    value(
        Command::new(&other_tool)
            .args(["read", "src/b.txt"])
            .output()
            .unwrap(),
    );
    let mut child = Command::new(&other_tool)
        .args(["edit", "src/b.txt"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"second work")
        .unwrap();
    refused(child.wait_with_output().unwrap(), "mutation permit");
    assert!(!f.root.join("src/b.txt").exists());
    let config = f.root.join("exitbind.json");
    let mut bytes = fs::read(&config).unwrap();
    bytes.push(b' ');
    fs::write(config, bytes).unwrap();
    refused(
        f.edit("edit", "src/a.txt", b"first work"),
        "binding changed",
    );
    assert_eq!(
        fs::read_to_string(f.root.join("src/a.txt")).unwrap(),
        "first work"
    );
}

#[test]
fn exact_executable_and_config_survive_hostile_path_and_cwd() {
    let f = Fixture::new(false, true);
    f.permit();
    f.edit_ok("read", "src/a.txt", b"");
    f.edit_ok("edit", "src/a.txt", b"pinned");
    let replacement = f.root.join("replacement executable");
    fs::copy(&f.executable, &replacement).unwrap();
    let mut bytes = fs::read(&replacement).unwrap();
    bytes.extend_from_slice(b"changed digest");
    fs::write(&replacement, bytes).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(replacement, &f.executable).unwrap();
    refused(f.edit("read", "src/a.txt", b""), "binding changed");
}

#[test]
fn managed_reads_enforce_observe_scope_utf8_bounds_and_control_aliases() {
    let f = Fixture::new(true, false);
    f.permit();
    for file in [
        "../escape",
        "/absolute",
        "src//alias",
        "src/../alias",
        "src/.codex/config.toml",
        "src/AGENTS.md",
        "src/.exitbind/log",
        "src/CLAUDE.md",
    ] {
        refused(
            f.edit("read", file, b""),
            if file.starts_with("src/.codex")
                || file.ends_with("AGENTS.md")
                || file.contains(".exitbind")
                || file.ends_with("CLAUDE.md")
            {
                "cannot replace"
            } else {
                "normalized"
            },
        );
    }
    fs::hard_link(
        f.root.join("exitbind.json"),
        f.root.join("src/control-alias"),
    )
    .unwrap();
    refused(f.edit("read", "src/control-alias", b""), "cannot replace");
    std::os::unix::fs::symlink(f.root.join("exitbind.json"), f.root.join("src/link")).unwrap();
    refused(f.edit("read", "src/link", b""), "unsafe");
    fs::write(f.root.join("src/binary"), [0xff]).unwrap();
    refused(f.edit("read", "src/binary", b""), "UTF-8");
    fs::write(f.root.join("src/large"), vec![b'a'; 256 * 1024 + 1]).unwrap();
    refused(f.edit("read", "src/large", b""), "unreadable");
    let narrow = Fixture::new(false, false);
    refused(narrow.edit("read", "other.txt", b""), "read boundary");
    narrow.edit_ok("read", "src/a.txt", b"");
    refused(narrow.edit("edit", "src/a.txt", &[0xff]), "UTF-8");
    refused(
        narrow.edit("edit", "src/a.txt", &vec![b'a'; 256 * 1024 + 1]),
        "256 KiB",
    );
}

#[test]
fn empty_observe_stays_empty_for_read_refresh_and_inspect() {
    let f = Fixture::with_observe(false, false, json!([]));
    f.permit();
    fs::write(f.root.join("src/private.txt"), "must stay private").unwrap();
    let reads = fs::read(f.reads_path()).unwrap();
    for action in ["read", "refresh", "inspect"] {
        let output = f.edit(action, "src/private.txt", b"");
        refused(output, "read boundary");
        assert_eq!(fs::read(f.reads_path()).unwrap(), reads);
        assert_eq!(
            fs::read_to_string(f.root.join("src/private.txt")).unwrap(),
            "must stay private"
        );
    }
}

#[test]
fn permit_rebinds_current_detail_and_fresh_managed_action_remains_usable() {
    let f = Fixture::new(false, false);
    let before = f.ok(&["work", "detail", &f.work], b"");
    assert_eq!(before["recipientContext"]["role"], "worker");
    assert_eq!(before["recipient"]["assignment"], f.assignment);
    assert_eq!(
        before["recipientContext"]["declaredBoundary"]["observe"],
        json!(["src/**"])
    );

    let mut permit_argv = before["actionForms"]["beforeEditing"]["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let operation = permit_argv
        .iter_mut()
        .find(|arg| arg.as_str() == "<OPERATION>")
        .unwrap();
    *operation = "managed-handoff-regression".into();
    let permit = Command::new(&permit_argv[0])
        .args(&permit_argv[1..])
        .output()
        .unwrap();
    assert!(permit.status.success(), "{permit:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&permit.stdout).unwrap()["allowed"],
        true
    );

    let second = f.ok(
        &[
            "work",
            "permit",
            &f.work,
            &f.assignment,
            "--operation",
            "managed-handoff-regression-second",
            "--request-id",
            "managed-handoff-second",
        ],
        b"",
    );
    assert_eq!(second["allowed"], true);
    assert_eq!(second["governor"]["state"], "replan_required");
    assert_eq!(second["nextRequest"]["requiredAction"], "work replan");

    let after = f.ok(&["work", "detail", &f.work], b"");
    assert_ne!(before["binding"], after["binding"]);
    assert_eq!(
        after["recipientContext"]["role"],
        before["recipientContext"]["role"]
    );
    assert_eq!(
        after["recipient"]["assignment"],
        before["recipient"]["assignment"]
    );
    assert_eq!(
        after["recipientContext"]["profile"],
        before["recipientContext"]["profile"]
    );
    assert_eq!(
        after["recipientContext"]["declaredBoundary"],
        before["recipientContext"]["declaredBoundary"]
    );
    assert_eq!(
        after["recipientContext"]["rules"],
        before["recipientContext"]["rules"]
    );
    assert!(after["actionForms"].get("beforeEditing").is_none());
    assert_eq!(
        after["actionForms"]["currentGrant"]["governorEventSha256"],
        second["currentGrant"]["governorEventSha256"]
    );
    assert_eq!(
        after["effectiveAction"]["currentGrant"]["governorEventSha256"],
        second["currentGrant"]["governorEventSha256"]
    );
    assert_eq!(
        after["actionForms"]["nextRequest"]["requiredAction"],
        "work replan"
    );
    let choices = after["actionForms"]["choices"].as_array().unwrap();
    assert!(!choices.is_empty());
    let assignment = after["recipient"]["assignment"].as_str().unwrap();
    let binding = after["binding"].as_str().unwrap();
    for choice in choices {
        let argv = choice["command"]["argv"].as_array().unwrap();
        if choice["label"] == "replan" {
            assert_eq!(
                argv[0].as_str(),
                Some(f.executable.to_string_lossy().as_ref())
            );
            assert_eq!(
                &argv[1..5],
                &[
                    json!("work"),
                    json!("replan"),
                    json!(f.work.as_str()),
                    json!(assignment),
                ]
            );
            assert_eq!(argv[argv.len() - 2], "--config");
            assert_eq!(
                argv[argv.len() - 1].as_str(),
                Some(f.root.join("exitbind.json").to_string_lossy().as_ref())
            );
            for flag in ["--hypothesis", "--scope-decision", "--evidence-request"] {
                assert!(argv.iter().any(|arg| arg == flag));
            }
        } else {
            assert!(argv.iter().any(|arg| arg == assignment));
            assert!(argv
                .windows(2)
                .any(|pair| { pair[0] == "--current-binding" && pair[1] == binding }));
        }
    }

    // A newly prepared managed tool uses the current grant while re-plan is
    // required for any next mutation request.
    let prepared = f.ok(&["work", "file", "prepare", &f.work, &f.assignment], b"");
    let tool = PathBuf::from(prepared["tool"].as_str().unwrap());
    let output = Command::new(tool)
        .args(["read", "src/fresh.txt"])
        .current_dir(std::env::temp_dir())
        .env("PATH", "/missing-native-tools")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["exists"],
        false
    );
    f.edit_ok("read", "src/granted.txt", b"");
    assert_eq!(
        f.edit_ok("edit", "src/granted.txt", b"current grant used")["status"],
        "completed"
    );
}

#[test]
fn evidence_required_keeps_the_fresh_present_grant_for_one_managed_effect() {
    let f = Fixture::new(false, false);
    f.permit();
    let second = f.ok(
        &[
            "work",
            "permit",
            &f.work,
            &f.assignment,
            "--operation",
            "evidence-phase-second",
            "--request-id",
            "evidence-phase-second",
        ],
        b"",
    );
    assert_eq!(second["allowed"], true);
    assert_eq!(second["governor"]["state"], "replan_required");

    f.ok(
        &[
            "work",
            "replan",
            &f.work,
            &f.assignment,
            "--hypothesis",
            "The repeated mutation needs a new exact basis.",
            "--scope-decision",
            "Keep the repair within src/**.",
            "--evidence-request",
            "Require a fresh exact check before another request.",
        ],
        b"",
    );
    let third = f.ok(
        &[
            "work",
            "permit",
            &f.work,
            &f.assignment,
            "--operation",
            "evidence-phase-present-grant",
            "--request-id",
            "evidence-phase-present-grant",
        ],
        b"",
    );
    assert_eq!(third["allowed"], true);
    assert_eq!(third["governor"]["state"], "evidence_required");
    assert_eq!(third["nextRequest"]["requiredAction"], "work evidence");

    f.edit_ok("read", "src/a.txt", b"");
    assert_eq!(
        f.edit_ok("edit", "src/a.txt", b"present grant effect")["effect"],
        "file-replaced"
    );
    let current = f.ok(&["work", "detail", &f.work], b"");
    assert!(current["actionForms"].get("beforeEditing").is_none());
    assert_eq!(
        current["actionForms"]["currentGrant"]["governorEventSha256"],
        third["currentGrant"]["governorEventSha256"]
    );
    assert_eq!(
        current["effectiveAction"]["currentGrant"]["governorEventSha256"],
        third["currentGrant"]["governorEventSha256"]
    );
    assert_eq!(
        current["actionForms"]["nextRequest"]["requiredAction"],
        "work evidence"
    );
    let denied = f.call(
        &[
            "work",
            "permit",
            &f.work,
            &f.assignment,
            "--operation",
            "evidence-phase-next-request",
        ],
        b"",
    );
    assert!(!denied.status.success(), "{denied:?}");
    let blocked = f.ok(&["work", "detail", &f.work], b"");
    assert_eq!(blocked["actionForms"]["recovery"]["state"], "blocked");
}

#[test]
fn held_completion_blocks_new_managed_write_and_remains_recoverable() {
    let f = Fixture::new(false, false);
    f.permit();
    f.edit_ok("read", "src/a.txt", b"");
    let second = f.ok(
        &[
            "work",
            "permit",
            &f.work,
            &f.assignment,
            "--operation",
            "held-result-guard",
            "--request-id",
            "held-result-guard",
        ],
        b"",
    );
    assert_eq!(second["allowed"], true);
    assert_eq!(second["governor"]["state"], "replan_required");

    let work_hash = f.work.strip_prefix("smw_").unwrap();
    let ledger = f
        .root
        .join(".exitbind/runs")
        .join(format!("work-{work_hash}.jsonl"));
    let before_hold = fs::read(&ledger).unwrap();
    let held_bytes = b"retained completion result";
    let held_reply = f.call(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
        ],
        held_bytes,
    );
    assert!(held_reply.status.success(), "{held_reply:?}");
    let held_reply: Value = serde_json::from_slice(&held_reply.stdout).unwrap();
    assert_eq!(held_reply["effect"], "held");
    assert_eq!(fs::read(&ledger).unwrap(), before_hold);

    let held_detail = f.ok(&["work", "detail", &f.work], b"");
    assert!(held_detail["actionForms"].get("beforeEditing").is_none());
    assert!(held_detail["actionForms"].get("currentGrant").is_none());
    assert!(held_detail["effectiveAction"]["currentGrant"].is_null());
    assert_eq!(
        held_detail["actionForms"]["recovery"]["state"],
        "replan_required"
    );
    assert_eq!(
        held_detail["actionForms"]["recovery"]["heldResults"][0]["sha256"],
        held_reply["held"]["sha256"]
    );
    assert_eq!(
        held_detail["actionForms"]["recovery"]["heldResults"][0]["bytes"],
        held_bytes.len()
    );

    let provider = f.root.join("held-must-not-launch");
    fs::write(
        &provider,
        "#!/bin/sh\nif [ \"$1\" = '--version' ]; then printf 'codex-cli 0.160.0\\n'; exit 0; fi\ntouch provider-started\nexit 99\n",
    )
    .unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
    let launch = f.call(
        &[
            "work",
            "act",
            &f.work,
            "--controlled-effects",
            "--codex-bin",
            provider.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!launch.status.success(), "{launch:?}");
    assert!(String::from_utf8_lossy(&launch.stderr).contains("worker result is retained"));
    assert!(!f.root.join("provider-started").exists());
    assert_eq!(fs::read(&ledger).unwrap(), before_hold);

    let rejected = f.edit("edit", "src/a.txt", b"must remain absent");
    assert!(!rejected.status.success(), "{rejected:?}");
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("worker result is retained"));
    assert!(!f.root.join("src/a.txt").exists());
    assert_eq!(fs::read(&ledger).unwrap(), before_hold);
    let still_held = f.ok(&["work", "detail", &f.work], b"");
    assert_eq!(
        still_held["actionForms"]["recovery"]["heldResults"][0]["sha256"],
        held_reply["held"]["sha256"]
    );

    let replan_reply = f.ok(
        &[
            "work",
            "replan",
            &f.work,
            &f.assignment,
            "--hypothesis",
            "Return the retained exact result after refreshing the current plan.",
            "--scope-decision",
            "Keep the retained result within the current source boundary.",
            "--evidence-request",
            "Use the existing exact result before any next mutation request.",
        ],
        b"",
    );
    assert_eq!(replan_reply["event"]["action"], "govern");
    assert_eq!(replan_reply["event"]["governorEvent"]["action"], "replan");
    let ready = f.ok(&["work", "detail", &f.work], b"");
    assert_eq!(ready["actionForms"]["recovery"]["state"], "retained_result");
    assert!(ready["actionForms"].get("beforeEditing").is_none());
    assert!(ready["actionForms"].get("currentGrant").is_none());
    assert!(ready["effectiveAction"]["currentGrant"].is_null());
    let reference = ready["actionForms"]["recovery"]["heldResults"][0]["reference"]
        .as_str()
        .unwrap();
    let recovered = f.ok(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            reference,
            "--reason",
            "Return the retained exact result.",
        ],
        b"",
    );
    assert_eq!(recovered["effect"], "recorded");
}
