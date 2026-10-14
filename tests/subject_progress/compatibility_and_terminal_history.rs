use super::*;

#[test]
fn terminal_history_never_authorizes_current_continuation() {
    let fixture = Fixture::new_single();
    let work = prepared_for_acceptance(&fixture);
    let running_packet = save_packet(&fixture, &work, "running.json");
    let decision = fixture.value(&["work", "next", &work], None);
    let accepted = fixture.value(
        &[
            "work",
            "return",
            &work,
            decision["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    let accepted_packet = save_packet(&fixture, &work, "accepted.json");

    // Covered source edit after acceptance: ledger and delivery are unchanged.
    fs::write(fixture.root.join("src/config.txt"), b"file-first\n").unwrap();
    let history = fixture.call(&["run", "inspect", &fixture.ledger(&work), "--json"], None);
    assert!(history.status.success(), "{history:?}");
    let now = fixture.value(&["work", "next", &work], None);
    let residual = &now["residual"];
    assert_eq!(residual["historical"], true);
    assert!(residual["snapshot"]["inputsSha256"].is_null());
    assert_eq!(residual["doNotRepeat"], serde_json::json!([]));
    assert_eq!(residual["stillValid"], serde_json::json!([]));
    assert!(residual["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap()
        .contains("history"));
    for packet in [&running_packet, &accepted_packet] {
        let verdict = fixture.value(
            &[
                "work",
                "validate",
                &work,
                "--packet",
                packet.to_str().unwrap(),
            ],
            None,
        );
        assert_eq!(verdict["result"], "not_continuable");
        assert_eq!(verdict["reuse"], serde_json::json!([]));
        assert_eq!(verdict["packet"]["historical"], true);
    }
    assert_eq!(fixture.count("functional.count"), "1\n");

    // A blocked terminal run is not presented as a completed outcome.
    let blocked = Fixture::new_single();
    let begin = blocked.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "g",
            "--check-command",
            "true",
        ],
        None,
    );
    let blocked_work = begin["work"].as_str().unwrap().to_owned();
    blocked.value(
        &[
            "work",
            "return",
            &blocked_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some(b"cannot proceed"),
    );
    let ended = blocked.value(&["work", "next", &blocked_work], None);
    let help = &ended["residual"]["humanHelp"];
    assert_eq!(ended["residual"]["historical"], true);
    assert_eq!(help["ownerDecision"], "required");
    assert!(help["whatHappened"]
        .as_str()
        .unwrap()
        .contains("not accepted"));
}

/// Both installed guidance copies must carry the same terminal rule. The
/// bootstrap is what a fresh host reads first, so a looser instruction there
/// is what a lead actually follows.
#[test]
fn every_distributed_guidance_copy_states_the_terminal_block_rule() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "skills/exitbind/SKILL.md",
        "plugins/exitbind/skills/exitbind/SKILL.md",
        "skills/exitbind-bootstrap/SKILL.md",
    ] {
        let text = std::fs::read_to_string(root.join(relative)).unwrap();
        assert!(
            text.contains("print it on a line of its own, exactly as given"),
            "{relative} does not state the terminal block rule"
        );
        assert!(
            text.contains("not inside a sentence, not wrapped in emphasis"),
            "{relative} does not forbid decorating the block"
        );
        // Nothing may invite the host to assemble the line itself.
        assert!(
            !text.contains("Report the exit state the CLI gives you (`EXIT READY`"),
            "{relative} still invites paraphrasing the exit state"
        );
    }
}

#[test]
fn distributed_guidance_limits_unchanged_status_polling() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "skills/exitbind/SKILL.md",
        "plugins/exitbind/skills/exitbind/SKILL.md",
        "skills/exitbind-bootstrap/SKILL.md",
    ] {
        let text = std::fs::read_to_string(root.join(relative)).unwrap();
        let lower = text.to_lowercase();
        assert!(
            lower.contains("do not start a fresh watch process"),
            "{relative}"
        );
        assert!(
            lower.contains("poll status at short intervals")
                || (lower.contains("poll unchanged status") && lower.contains("short intervals")),
            "{relative}"
        );
        assert!(lower.contains("a timeout is not a"), "{relative}");
        assert!(
            lower.contains("explicit human status request"),
            "{relative}"
        );
    }
}
