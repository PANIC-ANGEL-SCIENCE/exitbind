//! Stored form of one canonical Goal journal line.
//!
//! The hash chain and every record validator cover the logical (complete)
//! Goal record. On disk a revision may instead carry only what changed
//! against the logical record directly before it, named by that record's
//! event hash, so a journal grows by what changed rather than by the whole
//! Goal state. Complete records stay valid at every position: journals
//! written before this form, and any record whose change list would not be
//! smaller, are stored complete. A long text that a revision adds but the
//! previous record already holds elsewhere (a requirement whose source is
//! the same task card) is copied from that record's path, not repeated.

use serde_json::{json, Value};
use std::collections::HashMap;

/// `kind` of a stored change record; complete records keep their own kind.
const CHANGE_KIND: &str = "lead_session_goal_delta";
const CHANGE_VERSION: u64 = 1;
/// The complete key set of a stored change record.
const CHANGE_KEYS: [&str; 5] = ["kind", "version", "base", "eventSha256", "changes"];
/// A string at least this long that the previous record already holds is
/// copied from it rather than written again.
const COPY_MIN_BYTES: usize = 256;

/// Long strings of the previous record, each with its first path.
type Known<'a> = HashMap<&'a str, Vec<Value>>;

/// The line a writer appends for the sealed logical `record` whose logical
/// predecessor is `previous`.
pub(super) fn stored(record: &Value, previous: Option<&Value>) -> Value {
    let Some(previous) = previous else {
        return record.clone();
    };
    let (Some(base), Some(event)) = (
        previous["eventSha256"].as_str(),
        record["eventSha256"].as_str(),
    ) else {
        return record.clone();
    };
    let base_record = without_event(previous);
    let mut known = Known::new();
    index_strings(&base_record, &mut Vec::new(), &mut known);
    let mut changes = Vec::new();
    diff(
        &base_record,
        &without_event(record),
        &known,
        &mut Vec::new(),
        &mut changes,
    );
    let change = json!({
        "kind": CHANGE_KIND,
        "version": CHANGE_VERSION,
        "base": base,
        "eventSha256": event,
        "changes": changes,
    });
    if encoded_len(&change) < encoded_len(record) {
        change
    } else {
        record.clone()
    }
}

/// The logical record of one parsed journal line. A complete record is
/// returned unchanged; a change record must name the logical record directly
/// before it and is applied to it. The caller still validates the result
/// (its event hash covers the reconstructed record).
pub(super) fn resolve(
    line: Value,
    previous: Option<&Value>,
    number: usize,
) -> Result<Value, String> {
    if line.get("kind").and_then(Value::as_str) != Some(CHANGE_KIND) {
        return Ok(line);
    }
    let invalid = |reason: &str| format!("invalid canonical session goal line {number}: {reason}");
    let object = line.as_object().expect("a kind field implies an object");
    if object.len() != CHANGE_KEYS.len() || CHANGE_KEYS.iter().any(|key| !object.contains_key(*key))
    {
        return Err(invalid("change record fields are not exact"));
    }
    if object["version"].as_u64() != Some(CHANGE_VERSION) {
        return Err(invalid("change record version is unsupported"));
    }
    let previous = previous.ok_or_else(|| invalid("change record has no previous record"))?;
    if object["base"].as_str().is_none() || object["base"] != previous["eventSha256"] {
        return Err(invalid("change record base is not the previous record"));
    }
    let event = object["eventSha256"]
        .as_str()
        .ok_or_else(|| invalid("change record event hash is missing"))?;
    let changes = object["changes"]
        .as_array()
        .ok_or_else(|| invalid("change record changes are not a list"))?;
    let mut logical = without_event(previous);
    for change in changes {
        apply(&mut logical, change, previous).map_err(|reason| invalid(&reason))?;
    }
    let target = logical
        .as_object_mut()
        .ok_or_else(|| invalid("change record does not yield an object"))?;
    target.insert("eventSha256".into(), json!(event));
    Ok(logical)
}

fn without_event(record: &Value) -> Value {
    let mut copy = record.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove("eventSha256");
    }
    copy
}

fn encoded_len(value: &Value) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |text| text.len())
}

fn index_strings<'a>(value: &'a Value, path: &mut Vec<Value>, known: &mut Known<'a>) {
    match value {
        Value::String(text) if text.len() >= COPY_MIN_BYTES => {
            known.entry(text.as_str()).or_insert_with(|| path.clone());
        }
        Value::Object(object) => {
            for (key, item) in object {
                path.push(json!(key));
                index_strings(item, path, known);
                path.pop();
            }
        }
        Value::Array(array) => {
            for (index, item) in array.iter().enumerate() {
                path.push(json!(index));
                index_strings(item, path, known);
                path.pop();
            }
        }
        _ => {}
    }
}

/// `["set", path, value]`, with every long string of `value` that the
/// previous record holds replaced by a following `["copy", at, from]`.
fn set(path: &[Value], value: &Value, known: &Known) -> Vec<Value> {
    let mut written = value.clone();
    let mut copies = Vec::new();
    let mut at = path.to_vec();
    take_known(&mut written, &mut at, known, &mut copies);
    if copies
        .first()
        .is_some_and(|copy| copy[1].as_array() == Some(&at))
    {
        return copies;
    }
    let mut changes = vec![json!(["set", path, written])];
    changes.extend(copies);
    changes
}

fn take_known(value: &mut Value, at: &mut Vec<Value>, known: &Known, copies: &mut Vec<Value>) {
    match value {
        Value::String(text) => {
            if let Some(from) = known.get(text.as_str()) {
                copies.push(json!(["copy", at.clone(), from]));
                text.clear();
            }
        }
        Value::Object(object) => {
            for (key, item) in object.iter_mut() {
                at.push(json!(key));
                take_known(item, at, known, copies);
                at.pop();
            }
        }
        Value::Array(array) => {
            for (index, item) in array.iter_mut().enumerate() {
                at.push(json!(index));
                take_known(item, at, known, copies);
                at.pop();
            }
        }
        _ => {}
    }
}

fn changes_len(changes: &[Value]) -> usize {
    changes.iter().map(encoded_len).sum::<usize>() + changes.len() + 1
}

/// Changes that turn `old` into `new`, appended to `out`. Each subtree uses
/// whichever is shorter: its nested changes or one replacement.
fn diff(old: &Value, new: &Value, known: &Known, path: &mut Vec<Value>, out: &mut Vec<Value>) {
    if old == new {
        return;
    }
    let mut nested = Vec::new();
    match (old, new) {
        (Value::Object(before), Value::Object(after)) => {
            for (key, value) in after {
                path.push(json!(key));
                match before.get(key) {
                    Some(prior) => diff(prior, value, known, path, &mut nested),
                    None => nested.extend(set(path, value, known)),
                }
                path.pop();
            }
            for key in before.keys().filter(|key| !after.contains_key(*key)) {
                path.push(json!(key));
                nested.push(json!(["del", path.clone()]));
                path.pop();
            }
        }
        (Value::Array(before), Value::Array(after)) => {
            for (index, value) in after.iter().enumerate() {
                path.push(json!(index));
                match before.get(index) {
                    Some(prior) => diff(prior, value, known, path, &mut nested),
                    None => nested.extend(set(path, value, known)),
                }
                path.pop();
            }
            if after.len() < before.len() {
                nested.push(json!(["len", path.clone(), after.len()]));
            }
        }
        _ => {
            out.extend(set(path, new, known));
            return;
        }
    }
    let replacement = set(path, new, known);
    if changes_len(&nested) < changes_len(&replacement) {
        out.extend(nested);
    } else {
        out.extend(replacement);
    }
}

/// Apply one change: `["set", path, value]` (object key, array index, or the
/// next array index), `["copy", path, from]` (the same, with the value at
/// `from` in the previous record `base`), `["del", path]` (an existing object
/// key), or `["len", path, n]` (shorten an array to `n`).
fn apply(target: &mut Value, change: &Value, base: &Value) -> Result<(), String> {
    let parts = change.as_array().ok_or("change is not a list")?;
    let operation = parts.first().and_then(Value::as_str).unwrap_or_default();
    let path = parts
        .get(1)
        .and_then(Value::as_array)
        .ok_or("change path is not a list")?;
    match (operation, parts.len()) {
        ("set", 3) => place(target, path, parts[2].clone())?,
        ("copy", 3) => {
            let from = parts[2].as_array().ok_or("copy source is not a list")?;
            let value = read_at(base, from)
                .filter(|_| from.first().and_then(Value::as_str) != Some("eventSha256"))
                .ok_or("copy source is not in the previous record")?;
            place(target, path, value.clone())?;
        }
        ("del", 2) => {
            let (last, parents) = path.split_last().ok_or("change path is empty")?;
            match (locate(target, parents)?, last) {
                (Value::Object(object), Value::String(key)) => {
                    object
                        .remove(key)
                        .ok_or("change removes a field the record does not have")?;
                }
                _ => return Err("change path does not match the record".into()),
            }
        }
        ("len", 3) => {
            let length = parts[2]
                .as_u64()
                .and_then(|length| usize::try_from(length).ok())
                .ok_or("change length is invalid")?;
            match locate(target, path)? {
                Value::Array(array) if length <= array.len() => array.truncate(length),
                _ => return Err("change length does not match the record".into()),
            }
        }
        _ => return Err("change operation is unknown".into()),
    }
    Ok(())
}

fn place(target: &mut Value, path: &[Value], value: Value) -> Result<(), String> {
    let Some((last, parents)) = path.split_last() else {
        *target = value;
        return Ok(());
    };
    match (locate(target, parents)?, last) {
        (Value::Object(object), Value::String(key)) => {
            object.insert(key.clone(), value);
        }
        (Value::Array(array), Value::Number(index)) => {
            let index = index_of(index)?;
            match index.cmp(&array.len()) {
                std::cmp::Ordering::Less => array[index] = value,
                std::cmp::Ordering::Equal => array.push(value),
                std::cmp::Ordering::Greater => {
                    return Err("change index is past the end of its list".into())
                }
            }
        }
        _ => return Err("change path does not match the record".into()),
    }
    Ok(())
}

fn read_at<'a>(value: &'a Value, path: &[Value]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, segment| match segment {
            Value::String(key) => current.as_object()?.get(key),
            Value::Number(index) => current
                .as_array()?
                .get(usize::try_from(index.as_u64()?).ok()?),
            _ => None,
        })
}

fn locate<'a>(target: &'a mut Value, path: &[Value]) -> Result<&'a mut Value, String> {
    let mut current = target;
    for segment in path {
        current = match (current, segment) {
            (Value::Object(object), Value::String(key)) => object.get_mut(key),
            (Value::Array(array), Value::Number(index)) => array.get_mut(index_of(index)?),
            _ => None,
        }
        .ok_or("change path does not match the record")?;
    }
    Ok(current)
}

fn index_of(index: &serde_json::Number) -> Result<usize, String> {
    index
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| "change index is invalid".into())
}

#[cfg(test)]
mod tests;
