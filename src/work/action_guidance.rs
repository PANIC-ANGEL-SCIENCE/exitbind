//! The current recipient's delivered guidance on `work detail --view action`.
//!
//! One source of truth: this projects the selection and budget that the
//! complete detail delivers as `recipientContext.guidance`; it never resolves
//! guidance again. Every item names its exact read route, and an item the
//! budget left out is counted with its route instead of being clipped.
use serde_json::{json, Value};

const PHASE_KIND: &str = "phase_guidance";

/// `recipient` is the complete detail's `recipientContext` read; `routes` are
/// the current section routes. `None` when the current recipient has no
/// guidance. A recipient read that fails is reported with its route, so the
/// navigation stays usable and the gap stays visible.
pub(super) fn project(recipient: &Result<Value, String>, routes: &Value) -> Option<Value> {
    let section = ["recipient", "grouped"]
        .into_iter()
        .map(|name| &routes[name])
        .find(|route| route["command"].is_array())
        .map(read_route)
        .unwrap_or(Value::Null);
    let recipient = match recipient {
        Ok(recipient) => recipient,
        Err(reason) => {
            return Some(
                json!({"available":false,"reason":reason,"readRoute":section,
                "itemCount":0,"items":[],"omittedCount":0,"omitted":[]}),
            )
        }
    };
    if recipient["available"] != true {
        return None;
    }
    let guidance = &recipient["guidance"];
    let delivered = guidance["items"].as_array()?;
    let lessons = match &recipient["projectLessons"]["detail"] {
        detail if detail["command"].is_array() => read_route(detail),
        _ => section.clone(),
    };
    let items: Vec<Value> = delivered
        .iter()
        .map(|item| {
            let reference = &item["reference"];
            let (kind, route) = if reference["kind"] == PHASE_KIND {
                (PHASE_KIND, &section)
            } else if reference["scope"] == crate::memory::lessons::SCOPE {
                ("project_lesson", &lessons)
            } else {
                ("memory", &section)
            };
            let content = &item["content"];
            let mut entry = json!({"itemId":reference["itemId"],"kind":kind,
                "id":content["id"].as_str().or(reference["id"].as_str()),
                "sourceSha256":reference["sourceSha256"],"byteLength":reference["byteLength"],
                "content":if kind == "project_lesson" { content["fact"].clone() } else { content.clone() },
                "readRoute":route});
            if reference["phase"].is_string() {
                entry["phase"] = reference["phase"].clone();
            }
            entry
        })
        .collect();
    let mut omitted: Vec<Value> = guidance["overflow"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            let route = if entry["readRoute"]["command"].is_array() {
                entry["readRoute"].clone()
            } else {
                section.clone()
            };
            json!({"itemId":entry["reference"]["itemId"],"id":entry["reference"]["id"],
                "reason":entry["reason"],"bytes":entry["bytes"],"readRoute":route})
        })
        .collect();
    omitted.extend(
        guidance["unavailable"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|entry| {
                json!({"itemId":entry["itemId"],"id":Value::Null,"reason":entry["reason"],
                "bytes":Value::Null,"readRoute":section.clone()})
            }),
    );
    let mut result = json!({"available":true,"agent":recipient["agent"],"role":recipient["role"],
        "itemCount":items.len(),"items":items,"omittedCount":omitted.len(),"omitted":omitted,
        "budgetBytes":guidance["budgetBytes"],"usedBytes":guidance["usedBytes"],
        "meaning":"the guidance bodies and lessons delivered to the current recipient for its current phase, from the same selection as the complete detail; read them before dependent work; no permission, check, review or acceptance authority"});
    if let Some(phases) = guidance["phaseGuidance"]["phases"].as_array() {
        result["phases"] = Value::Array(phases.clone());
    }
    fit(&mut result);
    Some(result)
}

/// Keep the delivered block within its recipient budget without refusing the
/// view: items past the budget move, last first, to `omitted` with the
/// `recipient_budget` reason and their read route.
fn fit(result: &mut Value) {
    let Some(budget) = result["budgetBytes"].as_u64() else {
        return;
    };
    let mut used = result["usedBytes"].as_u64().unwrap_or(0);
    while used > budget {
        let Some(item) = result["items"].as_array_mut().and_then(Vec::pop) else {
            break;
        };
        used = used.saturating_sub(item["byteLength"].as_u64().unwrap_or(used));
        let entry = json!({"itemId":item["itemId"],"id":item["id"],"reason":"recipient_budget",
            "bytes":item["byteLength"],"readRoute":item["readRoute"]});
        if let Some(omitted) = result["omitted"].as_array_mut() {
            omitted.push(entry);
        }
    }
    result["usedBytes"] = json!(used);
    result["itemCount"] = json!(result["items"].as_array().map_or(0, Vec::len));
    result["omittedCount"] = json!(result["omitted"].as_array().map_or(0, Vec::len));
}

fn read_route(route: &Value) -> Value {
    json!({"command":route["command"],"readOnly":true,
        "sameConfigRequired":route["sameConfigRequired"] == true,
        "sameExecutableRequired":route["sameExecutableRequired"] == true})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_every_delivered_item_with_a_route_and_counts_omissions() {
        let recipient = json!({"available":true,"agent":"worker","role":"worker",
            "guidance":{"budgetBytes":10240,"usedBytes":10,"phaseGuidance":{"phases":["test"]},
                "items":[
                    {"reference":{"kind":"phase_guidance","itemId":"phase:triage","id":"triage",
                        "phase":"test","sourceSha256":"a","byteLength":4},"content":"body"},
                    {"reference":{"scope":"project-lessons.v1","itemId":"l1","sourceSha256":"b",
                        "byteLength":9},"content":{"id":"lesson-1","fact":"a fact","owner":"lead"}}],
                "overflow":[{"reference":{"itemId":"big"},"reason":"recipient_budget","bytes":20000,
                    "readRoute":{"command":["exitbind","project","context","guidance","big"]}}],
                "unavailable":[{"itemId":"gone","reason":"source_unavailable_or_changed"}]},
            "projectLessons":{"detail":{"command":["exitbind","memory","resolve","worker"]}}});
        let routes = json!({"recipient":{"command":["exitbind","work","expand","W","ref:r"]}});
        let view = project(&Ok(recipient.clone()), &routes).unwrap();
        assert_eq!(view["itemCount"], 2);
        assert_eq!(view["items"][0]["content"], "body");
        assert_eq!(view["items"][0]["phase"], "test");
        assert_eq!(
            view["items"][0]["readRoute"]["command"],
            routes["recipient"]["command"]
        );
        assert_eq!(view["items"][1]["kind"], "project_lesson");
        assert_eq!(view["items"][1]["content"], "a fact");
        assert_eq!(view["items"][1]["readRoute"]["command"][2], "resolve");
        assert_eq!(view["omittedCount"], 2);
        assert_eq!(view["omitted"][0]["readRoute"]["command"][4], "big");
        assert_eq!(
            view["omitted"][1]["readRoute"]["command"],
            routes["recipient"]["command"]
        );
        assert_eq!(view["phases"], json!(["test"]));
        assert_eq!(view["usedBytes"], 10);
        assert!(project(&Ok(json!({"available":false})), &routes).is_none());
        // W1 review F3: a block over its budget is not a refusal; the overflow
        // is counted as omitted with its route.
        let mut over = recipient;
        over["guidance"]["usedBytes"] = json!(10_245);
        let view = project(&Ok(over), &routes).unwrap();
        assert_eq!(view["itemCount"], 1);
        assert_eq!(view["usedBytes"], 10_236);
        assert_eq!(view["omittedCount"], 3);
        assert_eq!(view["omitted"][2]["id"], "lesson-1");
        assert_eq!(view["omitted"][2]["reason"], "recipient_budget");
        assert_eq!(view["omitted"][2]["readRoute"]["command"][2], "resolve");
        let failed = project(&Err("profile changed".into()), &routes).unwrap();
        assert_eq!(failed["available"], false);
        assert_eq!(
            failed["readRoute"]["command"],
            routes["recipient"]["command"]
        );
    }
}
