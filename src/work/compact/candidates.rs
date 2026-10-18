use serde_json::{json, Map, Value};

use super::{copy_if_present, serialized_len, MAX_RESPONSE_BYTES};

pub(super) fn retain_small_candidates(response: &Value, output: &mut Value) -> Result<(), String> {
    if response.get("works").is_none() && response.get("unreadable").is_none() {
        return Ok(());
    }
    let omitted = output["omitted"].clone();
    for key in ["works", "unreadable"] {
        copy_if_present(
            response,
            output.as_object_mut().expect("compact object"),
            key,
        );
    }
    output["omitted"] = json!(["large response detail; use fullCommand"]);
    if serialized_len(output)? < MAX_RESPONSE_BYTES {
        return Ok(());
    }

    // Keep empty arrays distinct from omitted nonempty details. Under pressure,
    // retain executable recovery routes and identities before dropping duplicate
    // ledger/error text or other candidate metadata.
    for key in ["works", "unreadable"] {
        let Some(items) = response.get(key).and_then(Value::as_array) else {
            output.as_object_mut().expect("compact object").remove(key);
            continue;
        };
        if items.is_empty() {
            output[key] = Value::Array(Vec::new());
        } else if key == "unreadable" {
            output[key] = Value::Array(items.iter().map(minimal_unreadable_candidate).collect());
        } else {
            output[key] = Value::Array(items.iter().map(minimal_work_candidate).collect());
        }
    }
    output["omitted"] = json!(["candidate details; use fullCommand"]);
    if serialized_len(output)? + 1 > MAX_RESPONSE_BYTES {
        for key in ["works", "unreadable"] {
            if response
                .get(key)
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
            {
                output.as_object_mut().expect("compact object").remove(key);
            }
        }
        output["omitted"] = omitted;
    }
    Ok(())
}

fn minimal_work_candidate(candidate: &Value) -> Value {
    minimal_candidate_fields(
        candidate,
        &[
            "work",
            "progress",
            "command",
            "sameConfigRequired",
            "sameExecutableRequired",
        ],
    )
}

fn minimal_unreadable_candidate(candidate: &Value) -> Value {
    minimal_candidate_fields(
        candidate,
        &[
            "work",
            "reason",
            "command",
            "sameConfigRequired",
            "sameExecutableRequired",
            "workingDirectory",
        ],
    )
}

fn minimal_candidate_fields(candidate: &Value, fields: &[&str]) -> Value {
    let Some(object) = candidate.as_object() else {
        return candidate.clone();
    };
    let mut compact = Map::new();
    for key in fields {
        if let Some(value) = object.get(*key) {
            compact.insert((*key).to_owned(), value.clone());
        }
    }
    Value::Object(compact)
}
