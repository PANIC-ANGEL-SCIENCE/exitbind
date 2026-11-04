//! Adding the host-specific Stop handler preserves existing hook rows.

use super::{exitbind, host_home, json};
use serde_json::json;
use std::fs;

#[test]
fn stop_addition_preserves_existing_handlers_and_foreign_indices() {
    let home = host_home("host-bridge-stop-additive");
    for host in ["codex", "claude"] {
        let path = home.join(if host == "codex" {
            ".codex/hooks.json"
        } else {
            ".claude/settings.json"
        });
        let existing = if host == "codex" {
            json!({"type":"command", "command":"command -v exitbind >/dev/null 2>&1 && exitbind hook-run || true", "timeout":5, "additionalContextLimit":4096})
        } else {
            json!({"type":"command", "command":"command -v exitbind >/dev/null 2>&1 && exitbind hook-run || true", "timeout":5})
        };
        let before = json!({"hooks":{
            "SessionStart":[{"hooks":[existing]},{"hooks":[{"type":"command","command":"foreign-start"}]}],
            "Stop":[{"hooks":[{"type":"command","command":"foreign-stop"}]}]
        }});
        fs::write(&path, before.to_string()).unwrap();
        let root = home.to_str().unwrap();
        let applied = json(exitbind(
            &home,
            &["hooks", "apply", "--hosts", host, "--root", root, "--json"],
        ));
        assert_eq!(applied["hosts"][0]["state"], "installed");
        let after: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            after["hooks"]["SessionStart"],
            before["hooks"]["SessionStart"]
        );
        assert_eq!(after["hooks"]["Stop"][0], before["hooks"]["Stop"][0]);
        assert_eq!(
            after["hooks"]["Stop"][1]["hooks"][0]["command"],
            format!(
                "command -v exitbind >/dev/null 2>&1 && exitbind hook-run --host {host} || true"
            )
        );
        let again = json(exitbind(
            &home,
            &["hooks", "apply", "--hosts", host, "--root", root, "--json"],
        ));
        assert_eq!(again["hosts"][0]["changed"], false);
    }
}

#[test]
fn claude_start_upgrades_to_distinct_slots_and_preserves_foreign_handlers() {
    let home = host_home("host-bridge-claude-multipart");
    let path = home.join(".claude/settings.json");
    let old = json!({"type":"command", "command":"command -v exitbind >/dev/null 2>&1 && exitbind hook-run || true", "timeout":5});
    let foreign = json!({"type":"command", "command":"foreign-start"});
    fs::write(
        &path,
        json!({"hooks":{"SubagentStart":[{"hooks":[old,foreign]}]}}).to_string(),
    )
    .unwrap();
    let root = home.to_str().unwrap();
    let before = json(exitbind(
        &home,
        &[
            "hooks", "status", "--hosts", "claude", "--root", root, "--json",
        ],
    ));
    assert_eq!(before["hosts"][0]["state"], "partial");
    let applied = json(exitbind(
        &home,
        &[
            "hooks", "apply", "--hosts", "claude", "--root", root, "--json",
        ],
    ));
    assert_eq!(applied["hosts"][0]["state"], "installed");
    let after: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let handlers = after["hooks"]["SubagentStart"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert!(handlers.contains(&&foreign));
    assert!(!handlers.contains(&&old));
    for index in 1..=4 {
        let command = format!("command -v exitbind >/dev/null 2>&1 && exitbind hook-run --host claude --index {index} || true");
        assert_eq!(
            handlers
                .iter()
                .filter(|handler| handler["command"] == command)
                .count(),
            1
        );
    }
    // Even when every new slot is already installed, the superseded handler
    // must be removed and the retirement persisted.
    let mut duplicate = after.clone();
    duplicate["hooks"]["SubagentStart"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(old.clone());
    fs::write(&path, duplicate.to_string()).unwrap();
    let status = json(exitbind(
        &home,
        &[
            "hooks", "status", "--hosts", "claude", "--root", root, "--json",
        ],
    ));
    assert_eq!(status["hosts"][0]["state"], "partial");
    let upgraded = json(exitbind(
        &home,
        &[
            "hooks", "apply", "--hosts", "claude", "--root", root, "--json",
        ],
    ));
    assert_eq!(upgraded["hosts"][0]["changed"], true);
    let upgraded: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(!upgraded["hooks"]["SubagentStart"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap())
        .any(|handler| handler == &old));
    let again = json(exitbind(
        &home,
        &[
            "hooks", "apply", "--hosts", "claude", "--root", root, "--json",
        ],
    ));
    assert_eq!(again["hosts"][0]["changed"], false);
    let removed = json(exitbind(
        &home,
        &[
            "hooks", "remove", "--hosts", "claude", "--root", root, "--json",
        ],
    ));
    assert!(removed["hosts"][0]["changed"].as_bool().unwrap());
    let after: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        after["hooks"]["SubagentStart"][0]["hooks"],
        json!([foreign])
    );
}

#[test]
fn filtered_old_claude_start_is_a_conflict_and_is_not_rewritten() {
    let home = host_home("host-bridge-claude-filtered");
    let path = home.join(".claude/settings.json");
    let old = json!({"type":"command", "command":"command -v exitbind >/dev/null 2>&1 && exitbind hook-run || true", "timeout":5});
    let before =
        json!({"hooks":{"SubagentStart":[{"matcher":"specific-agent","hooks":[old]}]}}).to_string();
    fs::write(&path, &before).unwrap();
    let root = home.to_str().unwrap();
    let applied = json(exitbind(
        &home,
        &[
            "hooks", "apply", "--hosts", "claude", "--root", root, "--json",
        ],
    ));
    assert_eq!(applied["hosts"][0]["state"], "conflict");
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn old_path_binary_cannot_install_multipart_handlers() {
    use std::os::unix::fs::PermissionsExt;
    let home = host_home("host-bridge-claude-old-binary");
    let bin = home.join("old-bin");
    fs::create_dir(&bin).unwrap();
    let old = bin.join("exitbind");
    fs::write(&old, "#!/bin/sh\nif [ \"$1\" = hook-protocol ]; then echo soulmate-hook-v1; exit 0; fi\nexit 2\n").unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o755)).unwrap();
    let path = home.join(".claude/settings.json");
    let before = "{\"existing\":true}\n";
    fs::write(&path, before).unwrap();
    let output = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args([
            "hooks",
            "apply",
            "--hosts",
            "claude",
            "--root",
            home.to_str().unwrap(),
        ])
        .env("HOME", &home)
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("does not support Claude multipart hooks")
    );
    assert_eq!(fs::read_to_string(path).unwrap(), before);
}
