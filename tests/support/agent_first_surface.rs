//! Facade output contract: exact recorded-result selectors are allowed only in
//! the bounded mutation reply, while raw protocol setup stays internal.

use serde_json::Value;

const FORBIDDEN_FIELDS: &[&str] = &[
    "ledger",
    "eventSha256",
    "targetEventSha256",
    "artifactPathHint",
    "artifactRootHint",
    "checkCommand",
    "exitCode",
];

pub(super) fn assert_no_raw_protocol_fields(value: &Value) {
    let mutation_result = value["compact"] == true && value["effect"].is_string();
    inspect(value, "", mutation_result);
}

fn inspect(value: &Value, path: &str, mutation_result: bool) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let allowed_selector = mutation_result
                    && matches!(
                        (path, key.as_str()),
                        ("", "eventSha256" | "targetEventSha256")
                            | ("/event", "eventSha256" | "targetEventSha256")
                            | ("/reference", "eventSha256" | "ledger")
                    );
                assert!(
                    allowed_selector || !FORBIDDEN_FIELDS.contains(&key.as_str()),
                    "facade leaked forbidden field {key} at {path}"
                );
                inspect(child, &format!("{path}/{key}"), mutation_result);
            }
        }
        Value::Array(values) => {
            for child in values {
                inspect(child, path, mutation_result);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}
