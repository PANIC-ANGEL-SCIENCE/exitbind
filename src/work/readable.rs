// Builds current bound grouped detail and the diagnostic action view.
// Refuses stale grouped references; see `src/work/action_forms.rs`.
//! Product-owned, exact grouped consumption for the current Work.
//!
//! The legacy section reader remains hex encoded for compatibility. This
//! versioned reader resolves and validates the same current binding, then
//! presents bounded UTF-8 artifacts directly so consumers do not implement
//! paging, decoding, or integrity checks themselves.

use super::action_forms::GrantOffer;
use crate::{config::Loaded, run::RunSnapshot};
use serde_json::{json, Map, Value};

#[path = "action_guidance.rs"]
mod action_guidance;
#[path = "recipient_context.rs"]
pub(super) mod recipient_context;

pub(crate) const PREFIX: &str = "ref:work-detail:v1:";
pub(crate) const MAX_GROUPED_BYTES: usize = 256 * 1024;
/// Every `work detail --view` value; the CLI accepts exactly these.
pub(crate) const DETAIL_VIEWS: &[&str] = &["action"];

pub(crate) fn is_grouped(reference: &str) -> bool {
    reference.starts_with(PREFIX)
}

fn token(binding: &str) -> String {
    format!("{PREFIX}{binding}")
}

fn route(loaded: &Loaded, work: &str, binding: &str) -> Value {
    let command = crate::emitted::Route::new(["work", "expand", work, &token(binding), "--json"])
        .config(&loaded.path)
        .within(1024);
    json!({
        "version": 1,
        "kind": "work_detail",
        "reference": token(binding),
        "command": command.argv_value(),
        "sameConfigRequired": command.needs_config(),
        "sameExecutableRequired": command.needs_executable(),
        "readOnly": true,
    })
}

pub(crate) fn attach(loaded: &Loaded, work: &str, binding: &str, current: &mut Value) {
    if let Some(details) = current.get_mut("details").and_then(Value::as_object_mut) {
        details.insert("grouped".into(), route(loaded, work, binding));
    }
}

pub(crate) fn read(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Result<Value, String> {
    read_with_grant_offer(loaded, work, snapshot, next).map(|(detail, _)| detail)
}

/// Retain the offer from the same post-held canonical assembly as the detail.
/// The public JSON and its fingerprint do not acquire another authority field.
pub(super) fn read_with_grant_offer(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Result<(Value, Option<GrantOffer>), String> {
    read_after_reads(loaded, work, snapshot, next, || Ok(()))
}

fn read_after_reads(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    after_reads: impl FnOnce() -> Result<(), String>,
) -> Result<(Value, Option<GrantOffer>), String> {
    super::details::ensure_current_config(loaded)?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    if let Some(assignment) = canonical["assignment"].as_str().map(str::to_owned) {
        super::attach_held(loaded, work, &assignment, &mut canonical)?;
    }
    let binding = super::details::binding(loaded, work, snapshot, &canonical)?;
    let recipient = recipient_context::read(loaded, &canonical)?;
    let sections = grouped_sections(loaded, work, snapshot, &canonical, &binding)?;
    after_reads()?;
    super::details::ensure_current_config(loaded)?;
    let fresh_snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let fresh = super::next_from(loaded, work, &fresh_snapshot)?;
    if super::details::binding(loaded, work, &fresh_snapshot, &fresh)? != binding {
        let route = crate::emitted::refresh(work, &loaded.path);
        return Err(format!("work detail changed during expansion; {route}"));
    }
    if recipient_context::read(loaded, &fresh)? != recipient {
        return Err(
            "recipient instructions changed during work detail; refresh the current assignment"
                .into(),
        );
    }
    response(
        loaded, work, &binding, &canonical, sections, recipient, snapshot,
    )
}

pub(crate) fn current(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = super::resolve(loaded, work)?;
    let snapshot = RunSnapshot::capture(loaded, &ledger)?;
    let next = super::next_from(loaded, work, &snapshot)?;
    read(loaded, work, &snapshot, &next)
}

/// Comparable identities for current instructions and evidence. Ledger
/// transport and governor bookkeeping are displayed separately. These are
/// observations of content, not read receipts or permission.
pub(crate) fn public_context_identities(detail: &Value) -> Option<Value> {
    if detail["kind"] != "work_detail"
        || detail["complete"] != true
        || !detail["recipientContext"].is_object()
        || !detail["sections"].is_object()
    {
        return None;
    }
    let sections = &detail["sections"];
    for name in ["assignment", "evidence", "tasks"] {
        if !sections[name].is_object() {
            return None;
        }
    }
    let mut assignment = sections["assignment"].clone();
    content_context(assignment.pointer_mut("/assignment/context")?);
    if let Some(context) = assignment.pointer_mut("/residual/context") {
        content_context(context);
    }
    if let Some(snapshot) = assignment
        .pointer_mut("/residual/snapshot")
        .and_then(Value::as_object_mut)
    {
        snapshot.remove("eventCount");
        snapshot.remove("headEventSha256");
        snapshot.remove("inputsSha256");
    }
    let mut selected_evidence = sections["evidence"].clone();
    for item in selected_evidence["items"].as_array_mut()? {
        let object = item.as_object_mut()?;
        let transport_reference = match object.get("reference") {
            None | Some(Value::Null) => true,
            Some(Value::String(reference)) => reference.starts_with("ref:"),
            _ => false,
        };
        if transport_reference {
            object.remove("reference");
        }
    }
    let declared_evidence = content_evidence(
        detail.pointer("/sections/assignment/assignment/context/evidence")?,
        &detail["work"],
    )?;
    let residual_evidence =
        if let Some(items) = detail.pointer("/sections/assignment/residual/context/evidence") {
            Some(content_evidence(items, &detail["work"])?)
        } else {
            None
        };
    Some(
        json!({"recipientSha256":crate::evidence::hash::value(&detail["recipientContext"]),
        "assignmentContentSha256":crate::evidence::hash::value(&assignment),
        "evidenceContentSha256":crate::evidence::hash::value(&json!({
            "declared":declared_evidence,"residual":residual_evidence,"selected":selected_evidence})),
        "tasksSha256":crate::evidence::hash::value(&sections["tasks"])}),
    )
}

fn content_evidence(value: &Value, work: &Value) -> Option<Value> {
    let mut value = value.clone();
    for item in value.as_array_mut()? {
        let selector = item.get("evidenceId").cloned();
        let Some(reference) = item.get_mut("rawEventRef") else {
            continue;
        };
        if reference["kind"] == "ledger_event"
            && reference["selector"].is_string()
            && Some(&reference["selector"]) == selector.as_ref()
            && &reference["work"] == work
            && reference["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("ref:"))
            && reference["eventCount"].is_u64()
            && reference["headEventSha256"].is_string()
            && reference["sha256"].is_string()
        {
            if let Some(reference) = reference.as_object_mut() {
                for field in ["eventCount", "headEventSha256", "id", "sha256"] {
                    reference.remove(field);
                }
            }
        }
    }
    Some(value)
}

/// Separate semantic assignment content from transport and the explicit
/// current governor state. Unknown fields remain part of the identity.
fn content_context(value: &mut Value) {
    let Some(context) = value.as_object_mut() else {
        return;
    };
    for field in ["digest", "evidence"] {
        context.remove(field);
    }
    if let Some(expansions) = context.get_mut("expansions").and_then(Value::as_array_mut) {
        for item in expansions {
            if matches!(
                item["kind"].as_str(),
                Some("ledger_event" | "ledger_history")
            ) && item["id"].as_str().is_some_and(|id| id.starts_with("ref:"))
                && item["eventCount"].is_u64()
                && item["headEventSha256"].is_string()
                && item["sha256"].is_string()
            {
                if let Some(reference) = item.as_object_mut() {
                    for field in ["eventCount", "headEventSha256", "id", "sha256"] {
                        reference.remove(field);
                    }
                }
            }
        }
    }
    if let Some(loop_state) = context.get_mut("loop").and_then(Value::as_object_mut) {
        if let Some(mutation) = loop_state.get_mut("currentMutation") {
            if mutation.is_null() {
                loop_state.remove("currentMutation");
            } else if let Some(fields) = mutation.as_object_mut() {
                for field in [
                    "attempt",
                    "carryLineage",
                    "checkpoint",
                    "eventSha256",
                    "inputSha256",
                    "operation",
                    "runId",
                    "subjectSha256",
                    "unit",
                ] {
                    fields.remove(field);
                }
                if fields.is_empty() {
                    loop_state.remove("currentMutation");
                }
            }
        }
        for field in [
            "attempt",
            "consumedGrants",
            "currentGrantEventSha256",
            "headSha256",
            "lineageSha256",
            "noInformationStreak",
            "postReplanSpent",
            "runId",
            "seenMutations",
            "spent",
            "subjectSha256",
        ] {
            loop_state.remove(field);
        }
    }
}

/// An adjacent permitted transition may carry the already delivered content
/// identity forward. If anything required changed, keep the full delivery.
pub(crate) fn public_context_continuity(before: &Value, after: &Value) -> Option<Value> {
    let base = public_context_identities(before)?;
    let current = public_context_identities(after)?;
    if before["work"] != after["work"]
        || before["recipient"] != after["recipient"]
        || before["recipientContext"] != after["recipientContext"]
        || base != current
        || !before["binding"].is_string()
        || !after["binding"].is_string()
    {
        return None;
    }
    let state = after.pointer("/sections/assignment/assignment/context/loop")?;
    if !state.is_object() {
        return None;
    }
    Some(json!({
        "kind":"work_context_continuity", "complete":false,
        "reason":"compare these content identities with the complete context already read; otherwise use current full detail",
        "work":after["work"], "binding":after["binding"], "baseBinding":before["binding"],
        "recipient":after["recipient"], "contextIdentity":current,
        "inputsSha256":after["sections"]["assignment"]["residual"]["snapshot"]["inputsSha256"],
        "governor":{"enabled":state["enabled"],"state":state["state"],"budget":state["budget"],
            "spent":state["spent"],"noInformationStreak":state["noInformationStreak"],
            "afterReplan":state["afterReplan"],"replanCount":state["replanCount"],
            "postReplanSpent":state["postReplanSpent"]},
    }))
}

/// One copy of the required content for a newly selected actor. This is a
/// public step projection; the original complete detail stays available.
pub(crate) fn public_actor_context(detail: &Value, reuse_recipient: bool) -> Option<Value> {
    let identity = detail
        .get("contextIdentity")
        .cloned()
        .or_else(|| public_context_identities(detail))?;
    let recipient = &detail["recipientContext"];
    if recipient["available"] != true || recipient["complete"] != true {
        return None;
    }
    for field in ["agent", "role", "nativeName"] {
        if recipient[field] != detail["recipient"][field] {
            return None;
        }
    }
    // Reuse the existing reference-only sanitization. Never recursively
    // remove fields from the delivered instruction or evidence body.
    let mut public_recipient = json!({"currentDelivery":{"recipientContext":recipient}});
    public_embedded_delivery(&mut public_recipient);
    let recipient = &public_recipient["currentDelivery"]["recipientContext"];
    let profile = instruction_body(&recipient["profile"])?;
    let rules = recipient["rules"]
        .as_array()?
        .iter()
        .map(instruction_body)
        .collect::<Option<Vec<_>>>()?;
    let mut instructions = json!({"profile":profile,"rules":rules,
        "displayName":recipient["displayName"],"authority":recipient["authority"]});
    if recipient["purpose"] != detail["sections"]["assignment"]["assignment"]["purpose"] {
        instructions["purpose"] = recipient["purpose"].clone();
    }
    if recipient["declaredBoundary"]
        != detail["sections"]["assignment"]["assignment"]["declaredBoundary"]
    {
        instructions["declaredBoundary"] = recipient["declaredBoundary"].clone();
    }
    for (field, value) in recipient.as_object()? {
        if !matches!(
            field.as_str(),
            "available"
                | "complete"
                | "agent"
                | "role"
                | "nativeName"
                | "displayName"
                | "purpose"
                | "configurationSha256"
                | "profile"
                | "rules"
                | "declaredBoundary"
                | "authority"
                | "guidance"
                | "architectureContract"
                | "capabilities"
                | "projectLessons"
                | "specialistExperience"
        ) {
            instructions[field] = value.clone();
        }
    }
    for field in [
        "guidance",
        "architectureContract",
        "capabilities",
        "projectLessons",
        "specialistExperience",
    ] {
        if let Some(original) = recipient.get(field).filter(|value| !value.is_null()) {
            if incomplete_instruction_material(original) {
                return None;
            }
            if field == "guidance"
                && original["selectedCount"] == 0
                && original["overflowCount"] == 0
                && original["unavailableCount"] == 0
                && ["items", "overflow", "unavailable"]
                    .iter()
                    .all(|key| original[*key].as_array().is_some_and(Vec::is_empty))
                && original.as_object().is_some_and(|object| {
                    object.keys().all(|key| {
                        matches!(
                            key.as_str(),
                            "budgetBytes"
                                | "items"
                                | "meaning"
                                | "overflow"
                                | "overflowCount"
                                | "selectedCount"
                                | "unavailable"
                                | "unavailableCount"
                                | "usedBytes"
                        )
                    })
                })
            {
                continue;
            }
            instructions[field] = original.clone();
        }
    }
    if reuse_recipient {
        instructions = json!({"recipientSha256":identity["recipientSha256"],
            "reuseIfPreviouslyRead":true,
            "meaning":"compare this recipient identity with the complete instructions already read; otherwise use full detail"});
    }
    let section = &detail["sections"]["assignment"];
    let mut packet = section["assignment"].clone();
    let original_context = packet.get("context")?.clone();
    let selected = detail["sections"]["evidence"]["items"].as_array()?;
    let context = public_actor_packet_context(&original_context, selected, &detail["work"])?;
    packet.as_object_mut()?.remove("context");

    let mut residual = section["residual"].clone();
    let mut aliases = Vec::new();
    if let Some(context) = residual.get_mut("context").and_then(Value::as_object_mut) {
        for (key, value) in context.clone() {
            if original_context.get(&key) == Some(&value) {
                context.remove(&key);
                aliases.push(key);
            }
        }
        if context.is_empty() {
            residual.as_object_mut()?.remove("context");
        }
    }
    if let Some(snapshot) = residual.get_mut("snapshot").and_then(Value::as_object_mut) {
        snapshot.remove("eventCount");
        snapshot.remove("headEventSha256");
    }
    let mut preparation = section["checkPreparation"].clone();
    if let Some(checker) = preparation.as_object_mut() {
        checker.remove("path");
        checker.remove("workingDirectory");
        if let Some(input) = checker.get_mut("input").and_then(Value::as_object_mut) {
            input.remove("path");
        }
    }
    let evidence = public_actor_evidence(&detail["sections"]["evidence"])?;
    let result = json!({"kind":"work_actor_context","complete":false,
        "requiredContentComplete":!reuse_recipient,
        "reason":if reuse_recipient { "assignment and evidence are included; compare the recipient identity or read full detail" }
            else { "current actor instructions and evidence are included; original full detail remains available" },
        "work":detail["work"],"binding":detail["binding"],"recipient":detail["recipient"],
        "contextIdentity":identity,"instructions":instructions,"assignment":packet,
        "context":context,"residual":residual,"equalContextAliases":aliases,
        "checkPreparation":preparation,"memoryQuery":section["memoryQuery"],
        "outcomes":section["outcomes"],"evidence":evidence,"tasks":detail["sections"]["tasks"]});
    if public_context_has_transport(&result) {
        return None;
    }
    Some(result)
}

fn incomplete_instruction_material(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.get("complete") == Some(&json!(false))
                || object.get("readable") == Some(&json!(false))
                || (object.get("content") == Some(&Value::Null)
                    && (object.contains_key("reference") || object.contains_key("sha256")))
                || ["overflow", "unavailable"].iter().any(|key| {
                    object
                        .get(*key)
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty())
                })
                || object.values().any(incomplete_instruction_material)
        }
        Value::Array(items) => items.iter().any(incomplete_instruction_material),
        _ => false,
    }
}

fn instruction_body(value: &Value) -> Option<Value> {
    let content = value["content"].as_str()?;
    let sha256 = value["sha256"].as_str()?;
    if value["complete"] != true
        || value["bytes"].as_u64()? != content.len() as u64
        || crate::evidence::hash::bytes(content.as_bytes()) != sha256
    {
        return None;
    }
    let mut body = value.clone();
    let object = body.as_object_mut()?;
    object.remove("path");
    for field in ["access", "reason"] {
        if object.get(field).is_some_and(Value::is_null) {
            object.remove(field);
        }
    }
    Some(body)
}

fn public_actor_packet_context(context: &Value, selected: &[Value], work: &Value) -> Option<Value> {
    let mut result = context.clone();
    let object = result.as_object_mut()?;
    object.remove("digest");
    if let Some(expansions) = object.remove("expansions") {
        if !expansions.as_array()?.iter().all(|item| {
            matches!(
                item["kind"].as_str(),
                Some("ledger_history" | "ledger_event")
            ) && &item["work"] == work
                && item.as_object().is_some_and(|object| {
                    object.keys().all(|key| {
                        matches!(
                            key.as_str(),
                            "eventCount"
                                | "exact"
                                | "headEventSha256"
                                | "id"
                                | "kind"
                                | "selector"
                                | "sha256"
                                | "work"
                        )
                    })
                })
        }) {
            return None;
        }
    }
    if let Some(items) = object.get_mut("evidence") {
        for item in items.as_array_mut()? {
            for field in ["rawEventRef", "checkEventRef", "checkLogRef"] {
                if let Some(reference) = item.get(field).filter(|value| !value.is_null()) {
                    let selector = if field == "checkLogRef" {
                        &reference["checkEventSha256"]
                    } else {
                        &reference["selector"]
                    };
                    let event = selected
                        .iter()
                        .find(|event| event["eventSha256"] == *selector)?;
                    if &reference["work"] != work
                        || !selector.is_string()
                        || reference["exact"] != true
                        || reference["kind"]
                            != if field == "checkLogRef" {
                                "check_log"
                            } else {
                                "ledger_event"
                            }
                        || !reference["id"]
                            .as_str()
                            .is_some_and(|id| id.starts_with("ref:"))
                        || !reference.as_object().is_some_and(|object| {
                            object.keys().all(|key| {
                                matches!(
                                    key.as_str(),
                                    "eventCount"
                                        | "exact"
                                        | "headEventSha256"
                                        | "id"
                                        | "kind"
                                        | "selector"
                                        | "sha256"
                                        | "work"
                                        | "checkEventSha256"
                                        | "stdout"
                                        | "stderr"
                                )
                            })
                        })
                    {
                        return None;
                    }
                    if field == "checkLogRef" {
                        for stream in ["stdout", "stderr"] {
                            if reference[stream].is_object()
                                && (reference[stream]["sha256"] != event[stream]["sha256"]
                                    || reference[stream]["bytes"] != event[stream]["bytes"]
                                    || reference[stream]["exact"] != true
                                    || event[stream]["complete"] != true)
                            {
                                return None;
                            }
                        }
                    }
                }
                item.as_object_mut()?.remove(field);
            }
            rename_evidence_id(item, "targetEventSha256", "targetEvidenceId")?;
            rename_evidence_id(item, "checkEventSha256", "checkEvidenceId")?;
        }
    }
    if let Some(obligations) = object.get_mut("obligations") {
        for item in obligations.as_array_mut()? {
            rename_evidence_id(item, "targetEventSha256", "targetEvidenceId")?;
            rename_evidence_id(item, "checkEventSha256", "checkEvidenceId")?;
        }
    }
    if let Some(recovery) = object.get("recovery") {
        if recovery["stale"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
            || recovery["conflicts"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        {
            return None;
        }
    }
    if let Some(loop_state) = object.get_mut("loop") {
        *loop_state = public_governor_state(loop_state)?;
    }
    Some(result)
}

fn public_governor_state(value: &Value) -> Option<Value> {
    let source = value.as_object()?;
    const KNOWN: &[&str] = &[
        "version",
        "enabled",
        "budget",
        "defaults",
        "grantProtocol",
        "state",
        "afterReplan",
        "replanCount",
        "postReplanSpent",
        "spent",
        "noInformationStreak",
        "currentReplan",
        "currentSensorRequest",
        "evidence",
        "observations",
        "consumedGrants",
        "currentGrantEventSha256",
        "currentMutation",
        "headSha256",
        "lineageSha256",
        "runId",
        "seenEvidenceSha256",
        "seenMutations",
        "seenSensors",
        "subjectSha256",
        "attempt",
    ];
    if source.keys().any(|key| !KNOWN.contains(&key.as_str())) {
        return None;
    }
    if source
        .get("currentReplan")
        .is_some_and(|value| !value.is_null())
        || source
            .get("currentSensorRequest")
            .is_some_and(|value| !value.is_null())
        || source
            .get("evidence")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
    {
        return None;
    }
    let mut state = Map::new();
    for field in [
        "enabled",
        "state",
        "budget",
        "defaults",
        "grantProtocol",
        "spent",
        "noInformationStreak",
        "afterReplan",
        "replanCount",
        "postReplanSpent",
    ] {
        if let Some(value) = source.get(field) {
            state.insert(field.to_owned(), value.clone());
        }
    }
    Some(Value::Object(state))
}

fn rename_evidence_id(value: &mut Value, old: &str, new: &str) -> Option<()> {
    let object = value.as_object_mut()?;
    if let Some(id) = object.remove(old) {
        if object.insert(new.to_owned(), id).is_some() {
            return None;
        }
    }
    Some(())
}

fn public_actor_evidence(value: &Value) -> Option<Value> {
    let mut evidence = value.clone();
    for item in evidence["items"].as_array_mut()? {
        rename_evidence_id(item, "eventSha256", "evidenceId")?;
        let reference = item.get("reference");
        if reference.is_some_and(|value| {
            !value.is_null() && !value.as_str().is_some_and(|text| text.starts_with("ref:"))
        }) {
            return None;
        }
        item.as_object_mut()?.remove("reference");
        for field in ["artifact", "stdout", "stderr"] {
            if item[field].is_object() {
                let kind = if field == "artifact" {
                    "evidence"
                } else {
                    field
                };
                if item[field]
                    .get("kind")
                    .is_some_and(|value| value.as_str() != Some(kind))
                {
                    return None;
                }
                item[field]["kind"] = json!(kind);
            }
        }
    }
    Some(evidence)
}

fn public_context_has_transport(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            matches!(
                key.as_str(),
                "path"
                    | "root"
                    | "ledger"
                    | "ledgerPath"
                    | "profilePath"
                    | "artifactRootHint"
                    | "artifactPathHint"
                    | "sourcePath"
                    | "eventSha256"
                    | "targetEventSha256"
                    | "checkCommand"
                    | "exitCode"
            ) || public_context_has_transport(child)
        }),
        Value::Array(items) => items.iter().any(public_context_has_transport),
        _ => false,
    }
}

/// The newly embedded step delivery is an agent-facing projection. Remove
/// transport locations only from its guidance references; the complete
/// bodies, identities and existing read routes remain intact. Explicit detail
/// and the native recipient value retain their established representation.
pub(crate) fn public_embedded_delivery(value: &mut Value) {
    for pointer in [
        "/currentDelivery/recipientContext/guidance/items",
        "/currentDelivery/recipientContext/guidance/overflow",
        "/currentDelivery/recipientContext/projectLessons/items",
        "/currentDelivery/recipientContext/specialistExperience/experience",
    ] {
        let Some(items) = value.pointer_mut(pointer).and_then(Value::as_array_mut) else {
            continue;
        };
        for item in items {
            if let Some(reference) = item.get_mut("reference") {
                super::packet::sanitize_assignment(reference);
                omit_reference_roots(reference);
            }
        }
    }
}

fn omit_reference_roots(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("root");
            object.values_mut().for_each(omit_reference_roots);
        }
        Value::Array(items) => items.iter_mut().for_each(omit_reference_roots),
        _ => {}
    }
}

/// Present grouped detail without repeating equal recovery facts in its
/// residual context. This is applied only at the CLI output boundary: native
/// delivery proofs continue to fingerprint the original grouped sections.
pub(crate) fn public_delivery(value: &mut Value) {
    if value["kind"] != "work_detail" {
        return;
    }
    let Some(section) = value.pointer("/sections/assignment") else {
        return;
    };
    if !section["projection"].is_object()
        || section["projection"]
            .get("residualRecoveryAliases")
            .is_some()
    {
        return;
    }
    let (residual, aliases) = super::action::delivery_packet_with_aliases(&section["residual"]);
    if aliases.is_empty() {
        return;
    }
    let mapping: Map<String, Value> = aliases
        .into_iter()
        .map(|(recovery, context)| (recovery.to_owned(), json!(context)))
        .collect();
    let mut projected = section.clone();
    projected["residual"] = residual;
    projected["projection"]["residualRecoveryAliases"] = Value::Object(mapping);
    // Keep the original when the alias map would cost as much as the facts.
    if serde_json::to_vec(&projected).map_or(true, |bytes| {
        serde_json::to_vec(section).map_or(true, |original| bytes.len() >= original.len())
    }) {
        return;
    }
    value["sections"]["assignment"] = projected;
}

/// A current executable form plus the guidance delivered to the current
/// recipient for its phase, without repeating the profile, rules and
/// artifacts. Mandatory context stays available through exact section pages.
pub(crate) fn action_view(loaded: &Loaded, work: &str) -> Result<Value, String> {
    super::details::ensure_current_config(loaded)?;
    // This view selects the complete form below, so the compact form normally
    // embedded in each navigation snapshot would only be constructed and lost.
    let next = super::next_for_projection(loaded, work, &super::resolve(loaded, work)?, false)?;
    let binding = next["current"]["binding"]
        .as_str()
        .ok_or("current action binding unavailable")?;
    let mut forms = super::action_forms::full(loaded, work, &next, binding);
    crate::emitted::complete_known(&mut forms, &loaded.path);
    let mut routes = next["current"]["details"].clone();
    crate::emitted::complete_known(&mut routes, &loaded.path);
    let recipient = recipient_context::read(loaded, &next);
    let mut guidance = action_guidance::project(&recipient, &routes);
    if let Some(guidance) = guidance.as_mut() {
        crate::emitted::complete_known(guidance, &loaded.path);
    }
    let grouped_route = routes.get("grouped").cloned();
    if let Some(routes) = routes.as_object_mut() {
        routes.remove("grouped");
    }
    let result = json!({"compact":true,"version":1,"kind":"work_action","work":work,"binding":binding,
        "producer":crate::producer::observation(),
        "actionForms":forms.clone(),"recipient":next["current"]["recipient"],
        "decision": {"goal":next["current"]["goal"], "phase":next["current"]["phase"],
            "result":next["current"]["result"], "readiness":next["current"]["readiness"],
            "obligations":next["packet"]["context"]["obligations"],
            "meaning":"current navigation metadata; inspect complete relevant evidence before deciding"},
        "details":routes,
        "instructions":{"complete":false,"meaning":"action view carries the delivered guidance below but not the profile, rules, assignment or evidence; consume those complete current sections before dependent work"}});
    let fresh = super::next_for_projection(loaded, work, &super::resolve(loaded, work)?, false)?;
    super::details::ensure_current_config(loaded)?;
    if fresh["current"]["binding"] != binding {
        return Err("action changed during read; inspect current work detail before acting".into());
    }
    if recipient_context::read(loaded, &fresh) != recipient {
        return Err(
            "recipient instructions changed during work detail; refresh the current assignment"
                .into(),
        );
    }
    let mut result = result;
    if next["current"]["uncreditedChild"].is_object() {
        result["decision"]["uncreditedChild"] = next["current"]["uncreditedChild"].clone();
    }
    if next["goalProgress"]["workAssociation"].is_object() {
        result["goalAssociation"] = next["goalProgress"]["workAssociation"].clone();
    }
    if next["current"]["checkReplacement"].is_object() {
        result["checkReplacement"] = next["current"]["checkReplacement"].clone();
    }
    project_action_view(&mut result);
    result.as_object_mut().unwrap().remove("actionForms");
    if navigation_bytes(&result)? > super::compact::MAX_RESPONSE_BYTES {
        let mut forms = forms;
        crate::emitted::omit_commands(&mut forms);
        forms["commandsComplete"] = json!(false);
        forms["commandsOmitted"] =
            json!("complete commands omitted to fit the action view; use complete current detail");
        result["actionForms"] = forms;
        project_action_view(&mut result);
        result.as_object_mut().unwrap().remove("actionForms");
        // When choices are unavailable, keep one complete read-only
        // recovery route instead of the larger set of section-only routes.
        if let Some(grouped) = grouped_route.as_ref().filter(|route| route.is_object()) {
            result["details"] = json!({"grouped":grouped});
        }
    }
    if navigation_bytes(&result)? > super::compact::MAX_RESPONSE_BYTES {
        result.as_object_mut().unwrap().remove("actionForms");
        if let Some(grouped_route) = grouped_route.filter(|route| route.is_object()) {
            result["details"] = json!({"grouped": grouped_route});
        }
        if navigation_bytes(&result)? > super::compact::MAX_RESPONSE_BYTES {
            // A route that cannot fit whole is unavailable, never an argv
            // prefix. The reference remains available for read-only recovery.
            crate::emitted::omit_commands(&mut result["details"]["grouped"]);
        }
        if navigation_bytes(&result)? > super::compact::MAX_RESPONSE_BYTES {
            return Err(
                "action view exceeds 8192 bytes; use current work next section routes".into(),
            );
        }
    }
    // The navigation above keeps its own bound; delivered guidance keeps the
    // recipient budget it was selected under (overflow is counted as omitted
    // with its route, never clipped or refused here).
    if let Some(guidance) = guidance {
        result["guidance"] = guidance;
    }
    Ok(result)
}

/// The action view's size accounting, in one place. The navigation part is
/// everything except the delivered `guidance` block and is bounded by the
/// compact response limit (serialized bytes plus the trailing newline). The
/// guidance block is bounded by the recipient budget its items were selected
/// under (`usedBytes <= budgetBytes`), as in the complete detail.
fn navigation_bytes(view: &Value) -> Result<usize, String> {
    let mut navigation = view.clone();
    if let Some(object) = navigation.as_object_mut() {
        object.remove("guidance");
    }
    Ok(serde_json::to_vec(&navigation)
        .map_err(|e| e.to_string())?
        .len()
        + 1)
}

fn project_action_view(result: &mut Value) {
    let mut effective = super::effective_action::project(result);
    let form = result
        .get("actionForms")
        .expect("action view projection requires action forms");
    effective["formState"] = form["state"].clone();
    for (key, value) in form.as_object().expect("action forms are an object") {
        if !effective.as_object().unwrap().contains_key(key) {
            effective[key] = value.clone();
        }
    }
    effective["source"]["location"] = json!("effectiveAction");
    result["effectiveAction"] = effective;
    super::action_forms::public_permit_guidance(result);
    super::action_forms::public_fill_guidance(result);
}

pub(crate) fn expand(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    requested: &str,
) -> Result<Value, String> {
    super::details::ensure_current_config(loaded)?;
    let binding = requested
        .strip_prefix(PREFIX)
        .filter(|value| !value.is_empty() && !value.contains(':'))
        .ok_or_else(|| {
            let route = crate::emitted::refresh(work, &loaded.path);
            format!("malformed grouped work detail reference; {route}")
        })?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    let expected = super::details::binding(loaded, work, snapshot, &canonical)?;
    if binding != expected {
        let route = crate::emitted::refresh(work, &loaded.path);
        return Err(format!("work detail reference is stale, revoked or belongs to another project/Work/recipient; {route}"));
    }
    read(loaded, work, snapshot, next)
}

pub(super) fn grouped_sections(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    binding: &str,
) -> Result<Map<String, Value>, String> {
    let mut sections = Map::new();
    let mut assignment = super::details::section_value(loaded, work, snapshot, next, "assignment")?;
    // This is a delivery projection, not a replacement canonical packet.
    // Only byte-equal aliases are removed; every unequal fact is retained.
    let mut aliases = Vec::new();
    for (residual_key, context_key) in [
        ("context", None),
        ("goal", Some("goal")),
        ("currentSubject", Some("subject")),
        ("remaining", Some("obligations")),
    ] {
        let original = &assignment["residual"][residual_key];
        let context = &assignment["assignment"]["context"];
        let replacement = context_key.map_or(context, |key| &context[key]);
        if !original.is_null() && original == replacement {
            assignment["residual"]
                .as_object_mut()
                .unwrap()
                .remove(residual_key);
            aliases.push(json!({"removed": format!("residual.{residual_key}"),
                "source": context_key.map_or("assignment.context".to_owned(), |key| format!("assignment.context.{key}"))}));
        }
    }
    if assignment["assignment"].is_object() {
        assignment["assignment"] = super::action::delivery_packet(&assignment["assignment"]);
    }
    assignment["projection"] = json!({"equalAliases": aliases,
        "recoveryAliases": "equal recovery goal/scope/subject/current/missing/loop/next refer to context goal/scope/subject/evidence/obligations/loop/next"});
    if next["role"] == "lead" {
        let question = crate::emitted::slot("QUESTION");
        let command = crate::emitted::Route::new([
            "work",
            "challenge",
            work,
            "--question",
            &question,
            "--json",
        ])
        .config(&loaded.path)
        .within(1024);
        assignment["optionalChallenge"] = json!({"required":false,"readOnly":true,
            "when":"Only for a consequential unresolved assumption in this approved task.",
            "command":command.argv_value(),"sameConfigRequired":command.needs_config(),
            "sameExecutableRequired":command.needs_executable(),
            "meaning":"Prepare a bounded native-host evidence packet; no agent or probe is launched."});
    }
    sections.insert("assignment".into(), assignment);
    let mut evidence = super::details::readable_evidence(loaded, snapshot, next)?;
    evidence["earlyEvidence"] = super::early_evidence::projection(loaded, work, snapshot)?;
    evidence["preReturn"] = crate::run::pre_return::projection(loaded, work)?;
    for item in evidence["items"].as_array_mut().unwrap() {
        for field in ["artifact", "stdout", "stderr"] {
            if item[field].is_object() && item[field]["readable"] == false {
                item[field]["access"] = json!({"currentMetadata": route(loaded, work, binding),
                    "legacySection": super::details::legacy_evidence_route(loaded, work, binding),
                    "limitation": "non-UTF-8 material remains exact bytes in the legacy section; oversized material exposes verified metadata only"});
            }
        }
    }
    sections.insert("evidence".into(), evidence);
    sections.insert(
        "tasks".into(),
        super::details::section_value(loaded, work, snapshot, next, "tasks")?,
    );
    Ok(sections)
}

fn response(
    loaded: &Loaded,
    work: &str,
    binding: &str,
    next: &Value,
    sections: Map<String, Value>,
    recipient: Value,
    snapshot: &RunSnapshot,
) -> Result<(Value, Option<GrantOffer>), String> {
    let readable_complete =
        evidence_complete(&sections["evidence"]) && recipient["complete"] == true;
    let (forms, offer) = super::action_forms::full_with_grant_offer(loaded, work, next, binding);
    let mut value = json!({
        "compact": true,
        "valid": true,
        "version": 1,
        "kind": "work_detail",
        "reference": token(binding),
        "work": work,
        "binding": binding,
        "recipient": recipient_context::identity(loaded, next),
        "complete": readable_complete,
        "sections": sections,
        "recipientContext": recipient,
        "usageAccount": crate::session_goal::usage::details_for_work(loaded, work),
        "actionForms": forms,
        "transport": {"encoding": "utf-8", "exact": readable_complete, "modelPaging": false},
    });
    if next["goalProgress"]["workAssociation"].is_object() {
        value["goalAssociation"] = next["goalProgress"]["workAssociation"].clone();
    }
    let view = snapshot.inspect_view();
    if let Some(relation) = super::replacement_relation::relation(
        loaded,
        work,
        view["events"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default(),
    )? {
        value["checkReplacement"] = relation;
    }
    if next["projectGoalProgress"].is_object() {
        value["projectGoalProgress"] = next["projectGoalProgress"].clone();
    }
    if next["action"] == "done" {
        value["completion"] = super::closeout::projection(loaded, work, snapshot, next);
    }
    if let Some(accepted) = snapshot.accepted_at_binding() {
        value["acceptedAtBinding"] = accepted;
    }
    let canonical = json!({
        "current":{"action":next["action"], "binding":binding, "readiness":next["progress"]["state"],
            "recipient":value["recipient"]},
        "actionForms":value["actionForms"], "warnings":next["warnings"],
    });
    super::effective_action::insert(&mut value, &canonical);
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    if bytes.len() + 1 > MAX_GROUPED_BYTES {
        return Err("grouped work detail exceeds the bounded readable channel; use the current section routes".into());
    }
    Ok((value, offer))
}

pub(super) fn evidence_complete(value: &Value) -> bool {
    value["items"].as_array().is_some_and(|items| {
        items.iter().all(|item| {
            ["artifact", "stdout", "stderr"].iter().all(|key| {
                !item[*key].is_object()
                    || (item[*key]["readable"] == true && item[*key]["complete"] == true)
            })
        })
    })
}

#[cfg(test)]
#[path = "readable_tests.rs"]
mod tests;
