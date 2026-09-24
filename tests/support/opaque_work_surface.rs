//! Read-only work packets stay opaque. A bounded mutation result exposes only
//! the exact recorded-event ledger selector in its inspection route.

use serde_json::Value;

pub(super) fn assert_opaque_reference(reference: &Value, expected: Value) {
    assert_eq!(reference, &expected);
    let serialized = reference.to_string();
    assert!(!serialized.contains("ledger"));
    assert!(!serialized.contains(".exitbind"));
    assert!(!serialized.contains("runs/"));
}

pub(super) fn assert_opaque_envelope(value: &Value) {
    let mutation = value["compact"] == true && value["effect"].is_string();
    let ledger = if mutation {
        value["reference"]["ledger"].as_str()
    } else {
        None
    };
    if let Some(ledger) = ledger {
        assert!(ledger.starts_with(".exitbind/runs/work-") && ledger.ends_with(".jsonl"));
        assert_eq!(value["nextAction"]["safe"], true);
        assert!(matches!(
            value["nextAction"]["type"].as_str(),
            Some("inspect" | "inspect_held")
        ));
    }
    visit(value, "", ledger);
}

fn visit(value: &Value, path: &str, ledger: Option<&str>) {
    match value {
        Value::String(text) => {
            let allowed_selector = ledger.is_some_and(|ledger| {
                text == ledger
                    && (path == "/reference/ledger" || path.starts_with("/nextAction/command/"))
            });
            if !allowed_selector {
                for marker in [".exitbind", "runs/", "artifacts/", "state/"] {
                    assert!(
                        !text.contains(marker),
                        "opaque envelope leaked {marker} at {path}"
                    );
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                visit(item, &format!("{path}/{index}"), ledger);
            }
        }
        Value::Object(object) => {
            for key in [
                "path",
                "root",
                "ledgerPath",
                "profilePath",
                "artifactPathHint",
                "sourcePath",
            ] {
                assert!(
                    !object.contains_key(key),
                    "opaque envelope leaked {key} at {path}"
                );
            }
            if object.get("exact") == Some(&Value::Bool(true)) {
                assert!(matches!(
                    object.get("kind").and_then(Value::as_str),
                    Some(
                        "ledger_history"
                            | "ledger_event"
                            | "check_log"
                            | "evidence"
                            | "stdout"
                            | "stderr"
                    )
                ));
            }
            for (key, child) in object {
                visit(child, &format!("{path}/{key}"), ledger);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
