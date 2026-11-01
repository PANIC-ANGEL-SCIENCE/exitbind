//! Exact inspection of a captured but uncredited host observation.

use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

fn observations(loaded: &Loaded, work: &str) -> Result<Vec<Value>, String> {
    let summary = super::summary::summary(loaded, work);
    let items = summary
        .as_array()
        .ok_or("child capture state is unreadable")?;
    Ok(items
        .iter()
        .filter(|item| item["rawObservation"].is_object())
        .cloned()
        .collect())
}

fn list_route(loaded: &Loaded, work: &str, index: usize) -> Value {
    let route = crate::emitted::Route::new([
        "work",
        "child",
        "context",
        work,
        "--view",
        "results",
        "--index",
        &index.to_string(),
    ]);
    let mut route = crate::work::compact::continuation_route(&loaded.path, route);
    route["readOnly"] = json!(true);
    route
}

/// A bounded current decision fact; exact raw bytes stay behind a read route.
pub(crate) fn latest(loaded: &Loaded, work: &str) -> Option<Value> {
    match observations(loaded, work) {
        Ok(items) if items.is_empty() => None,
        Ok(items) => Some(json!({"state":"uncredited","count":items.len(),
            "latest":items.last().map(|item| item["rawObservation"].clone()),
            "all":list_route(loaded, work, 0)})),
        Err(_) => Some(
            json!({"state":"unavailable","reason":"child capture state is unreadable",
            "all":list_route(loaded, work, 0)}),
        ),
    }
}

/// Page complete metadata using the existing compact response budget.
pub(crate) fn list(loaded: &Loaded, work: &str, index: usize) -> Result<Value, String> {
    let items = observations(loaded, work)?;
    if index > items.len() {
        return Err("raw observation index is unavailable".into());
    }
    let mut value = json!({"work":work,"kind":"uncredited_child_observations","index":index,
        "count":items.len(),"items":[],"next":Value::Null,"readOnly":true});
    let mut next = index;
    while next < items.len() {
        value["items"]
            .as_array_mut()
            .expect("items array")
            .push(items[next].clone());
        value["next"] = if next + 1 < items.len() {
            list_route(loaded, work, next + 1)
        } else {
            Value::Null
        };
        if !crate::work::compact::response_fits(&value)? {
            value["items"].as_array_mut().expect("items array").pop();
            value["next"] = list_route(loaded, work, next);
            break;
        }
        next += 1;
    }
    if (next == index && index < items.len()) || !crate::work::compact::response_fits(&value)? {
        return Err("raw observation metadata exceeds the existing response budget".into());
    }
    Ok(value)
}

pub(crate) fn read(loaded: &Loaded, work: &str, id: &str) -> Result<Value, String> {
    let intent = super::load(loaded)?
        .into_iter()
        .find(|intent| intent["work"] == work && intent["id"] == id)
        .ok_or("prepared child intent is unknown")?;
    if intent["receiving"] == super::same_host::RECEIVING {
        super::same_host::attributable(loaded, work, &intent)?;
    } else {
        let (_, binding) = super::binding_of(loaded, work, &intent)?;
        if binding["session"] != intent["session"]
            || binding["revision"] != intent["bindingRevision"]
        {
            return Err("captured observation belongs to a different receiving binding".into());
        }
    }
    let raw = &intent["rawObservation"];
    let bytes = if raw["artifact"].is_object() {
        crate::run::artifact::read_exact(
            loaded,
            &raw["artifact"],
            "captured host observation",
            super::MAX_RESULT as u64,
        )?
    } else {
        raw["text"]
            .as_str()
            .ok_or("this child has no uncredited raw observation")?
            .as_bytes()
            .to_vec()
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| "captured host observation is not UTF-8")?;
    if raw["sha256"] != hash::text(text)
        || intent["resultSha256"] != raw["sha256"]
        || raw["bytes"].as_u64() != Some(bytes.len() as u64)
    {
        return Err("captured raw observation changed".into());
    }
    let mut result = json!({"work":work,"intent":id,"resultText":text,"resultSha256":raw["sha256"],
        "bytes":text.len(),"credit":"none","reason":raw["reason"],
        "sourceClass":"host_reported_native_child"});
    if raw["diagnostic"].is_object() {
        result["diagnostic"] = raw["diagnostic"].clone();
    }
    Ok(result)
}
