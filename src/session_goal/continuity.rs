//! One-work continuation facts on the existing canonical session-goal history.
//! Native session handles are observations; the work ID and the goal revision
//! remain the portable identity. This surface assumes one authorized local
//! principal and does not authenticate a caller-supplied host name.

use super::{mutate, read, sealed};
use crate::{config::Loaded, evidence::hash, run, work};
use serde_json::{json, Value};

const MAX_INPUT: u64 = 16 * 1024;
const MAX_SOURCE: usize = 8 * 1024;
const MAX_ITEMS: usize = 32;

fn child_key(origin: &Value, native_child: &str) -> String {
    hash::value(
        &json!({"host":origin["host"],"session":origin["session"],"nativeChild":native_child}),
    )
}

fn evidence_verified(inspected: &Value) -> bool {
    inspected["evidence"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| item["status"] == "verified"))
}

fn field<'a>(value: &'a Value, key: &str, max: usize) -> Result<&'a str, String> {
    let Some(value) = value[key].as_str() else {
        return Err(format!("continuation requires bounded {key}"));
    };
    if value.len() > max {
        return Err(format!("continuation {key} exceeds {max} bytes"));
    }
    if value.contains('\0') {
        return Err(format!("continuation {key} contains NUL"));
    }
    if value.trim().is_empty() {
        return Err(format!("continuation requires bounded {key}"));
    }
    Ok(value)
}

fn revision(value: &Value, key: &str) -> Result<u64, String> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("continuation requires {key}"))
}

fn shape(value: &Value, allowed: &[&str]) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("continuation input must be an object")?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("continuation input has unknown field {key}"));
    }
    Ok(())
}

fn entries(value: &Value, key: &str) -> Result<Vec<Value>, String> {
    let values = value[key]
        .as_array()
        .ok_or_else(|| format!("continuation requires {key}"))?;
    if values.is_empty() || values.len() > MAX_ITEMS {
        return Err(format!(
            "continuation {key} must contain 1..={MAX_ITEMS} entries"
        ));
    }
    Ok(values.clone())
}

fn verify_work(loaded: &Loaded, work_id: &str) -> Result<String, String> {
    work::resolve(loaded, work_id)
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn current_conditions(loaded: &Loaded) -> Result<(String, String), String> {
    let inputs = run::inputs::fingerprint(loaded)?;
    let conditions = hash::value(&json!({
        "configSha256": hash::text(&loaded.source),
        "inputsSha256": inputs,
    }));
    Ok((inputs, conditions))
}

fn acquired_config_current(loaded: &Loaded, event: &Value) -> bool {
    event["configSha256"] == hash::text(&loaded.source)
}

fn same_intake(existing: &Value, proposed: &Value) -> bool {
    if existing["work"] != proposed["work"]
        || existing["source"]["ref"] != proposed["source"]["ref"]
        || existing["source"]["sha256"] != proposed["source"]["sha256"]
        || existing["source"]["text"] != proposed["source"]["text"]
    {
        return false;
    }
    let (Some(current), Some(initial)) = (
        existing["requirements"].as_array(),
        proposed["requirements"].as_array(),
    ) else {
        return false;
    };
    current.len() == initial.len()
        && current.iter().zip(initial).all(|(current, initial)| {
            current["id"] == initial["id"]
                && current["sourceSha256"] == initial["sourceSha256"]
                && current["history"]
                    .as_array()
                    .and_then(|history| history.first())
                    .map_or(&current["text"], |first| &first["text"])
                    == &initial["text"]
        })
}

fn base_record(work_id: &str, goal: &str, continuation: Value) -> Value {
    let obligations = continuation["requirements"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|requirement| {
            json!({"id":requirement["id"],"text":requirement["text"],"disposition":"open","resultRefs":[]})
        })
        .collect::<Vec<_>>();
    json!({
        "kind":"lead_session_goal", "version":3, "goalId":work_id,
        "revision":1, "goal":goal,
        "source":{"owner":"lead","kind":"explicit_continuation_intake"},
        "predecessor":null, "successorOf":null,
        "obligations":obligations, "findings":[], "blockers":[],
        "decisions":[], "externalActions":[],
        "categories":{"obligations":"considered","findings":"considered","blockers":"considered","decisions":"considered","externalActions":"considered"},
        "closure":{"closed":false,"revision":null,"resultRefs":[]},
        "continuation":continuation,
    })
}

fn init(loaded: &Loaded, work_id: &str, input: &Value) -> Result<(Value, bool), String> {
    shape(
        input,
        &[
            "action",
            "sourceText",
            "sourceRef",
            "requirements",
            "expectedRevision",
        ],
    )?;
    let ledger = verify_work(loaded, work_id)?;
    let source = field(input, "sourceText", MAX_SOURCE)?;
    let source_ref = field(input, "sourceRef", 256)?;
    let requirements = entries(input, "requirements")?;
    let mut seen = std::collections::BTreeSet::new();
    let mut normalized = Vec::new();
    for item in requirements {
        shape(&item, &["id", "text"])?;
        let id = field(&item, "id", 64)?;
        let text = field(&item, "text", 1024)?;
        if !seen.insert(id.to_owned()) || !source.contains(text) {
            return Err("requirements must be unique exact excerpts of sourceText".into());
        }
        normalized
            .push(json!({"id":id,"text":text,"revision":1,"sourceSha256":hash::text(source)}));
    }
    let (_, events, _) = run::ledger::load(loaded, &ledger)?;
    let goal = events[0]["goal"].as_str().ok_or("work start has no goal")?;
    let continuation = json!({
        "version":1,"work":work_id,
        "source":{"ref":source_ref,"text":source,"sha256":hash::text(source),"coverageConfirmed":false},
        "requirements":normalized,"supports":[],"corrections":[],"operations":[],"diagnoses":[],"children":[],
        "binding":null,
    });
    let mut appended = true;
    let record = mutate(&loaded.state_root, |previous| {
        let Some(previous) = previous else {
            if input.get("expectedRevision").is_some() && revision(input, "expectedRevision")? != 0
            {
                return Err("stale canonical goal revision".into());
            }
            return Ok(sealed(base_record(work_id, goal, continuation), None));
        };
        if previous["goalId"] != work_id {
            return Err(
                "current canonical goal belongs to another work; incorporate the successor first"
                    .into(),
            );
        }
        if previous["closure"]["closed"] == true {
            return Err("closed canonical goal cannot be reopened".into());
        }
        if !previous["continuation"].is_null() {
            if same_intake(&previous["continuation"], &continuation) {
                appended = false;
                return Ok(previous.clone());
            }
            return Err("continuation already initialized with a different source or work".into());
        }
        if revision(input, "expectedRevision")? != previous["revision"].as_u64().unwrap_or(0) {
            return Err("stale canonical goal revision".into());
        }
        let mut record = previous.clone();
        record
            .as_object_mut()
            .ok_or("goal record malformed")?
            .remove("eventSha256");
        record["revision"] = json!(revision(previous, "revision")? + 1);
        record["predecessor"] = json!({"goalId":work_id,"revision":previous["revision"]});
        record["successorOf"] = Value::Null;
        record["continuation"] = continuation;
        Ok(sealed(record, Some(previous)))
    })?;
    Ok((record, appended))
}

mod record;
mod view;
pub(crate) use record::{continuation_bind, continuation_child, continuation_record};
pub(crate) use view::{continuation_section, continuation_view, has_unresolved, support_current};
