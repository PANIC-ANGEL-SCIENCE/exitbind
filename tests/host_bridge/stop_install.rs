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
