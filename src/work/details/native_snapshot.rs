//! Reconstruct native delivery facts from validated Work snapshots.
use super::{binding, ensure_current_config, native_sections};
use crate::{config::Loaded, evidence::hash, run::RunSnapshot};
use serde_json::{json, Value};

/// Native delivery consumes the same canonical section as the public scoped
/// reader. It requires complete instructions; compact status never grants work.
pub(crate) fn required_assignment(
    loaded: &Loaded,
    work: &str,
    current: &Value,
) -> Result<Value, String> {
    required_assignment_inner(loaded, work, current, false).map(|(assignment, _)| assignment)
}

/// Selected delivery also needs the exact check policy from this same snapshot.
pub(crate) fn required_assignment_with_check(
    loaded: &Loaded,
    work: &str,
    current: &Value,
) -> Result<(Value, Value), String> {
    let (assignment, check) = required_assignment_inner(loaded, work, current, true)?;
    Ok((
        assignment,
        check.ok_or("native check policy observation is unavailable")?,
    ))
}

fn required_assignment_inner(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    include_check: bool,
) -> Result<(Value, Option<Value>), String> {
    let ledger = super::super::resolve(loaded, work)?;
    let snapshot = RunSnapshot::capture(loaded, &ledger)?;
    let fresh = super::super::next_from(loaded, work, &snapshot)?;
    if current["current"]["binding"] != fresh["current"]["binding"] {
        return Err("native current binding changed; refresh before execution".into());
    }
    let detail = super::super::readable::read(loaded, work, &snapshot, &fresh)?;
    // Native request assembly owns its separate, validated profile read.
    // A grouped inline-profile fallback is not missing upstream evidence.
    if !super::super::readable::evidence_complete(&detail["sections"]["evidence"]) {
        let item = detail["sections"]["evidence"]["items"]
            .as_array()
            .and_then(|items| {
                items.iter().find_map(|item| {
                    ["artifact", "stdout", "stderr"].iter().find_map(|key| {
                        (item[*key].is_object() && item[*key]["readable"] != true).then(|| {
                            format!(
                                "{} {} ({})",
                                item["eventSha256"].as_str().unwrap_or("unknown event"),
                                key,
                                item[*key]["reason"].as_str().unwrap_or("unavailable")
                            )
                        })
                    })
                })
            })
            .unwrap_or_else(|| "required evidence".into());
        let route =
            crate::emitted::command_line(&["work", "detail", work, "--json"], Some(&loaded.path));
        return Err(format!("native required evidence {item} is not completely readable; use {route} before provider launch"));
    }
    let recipient = &detail["recipientContext"];
    if recipient["available"] != true || !recipient["rules"].is_array() {
        // Preserve the existing actionable memory-drift diagnostic when it is
        // the reason this recipient cannot be safely delivered.
        crate::memory::contextual::assert_assignment(loaded, &fresh["packet"])?;
        return Err("current recipient instructions are unavailable for native delivery".into());
    }
    let sections = native_sections(
        detail["sections"].clone(),
        &detail["actionForms"],
        &detail["binding"],
    );
    // The readable owner has already rechecked the current binding and
    // recipient after assembling these sections. The command comes from the
    // original validated snapshot, never from the readiness projection.
    let check = if include_check {
        Some(check_command(&snapshot)?)
    } else {
        None
    };
    Ok((
        json!({
            "canonicalAssignment": fresh["packet"],
            "requiredFactsSha256": hash::value(&sections),
            "recipientSha256": hash::value(recipient),
            "recipientRules": recipient["rules"],
            "sections": sections,
            "binding": detail["binding"],
        }),
        check,
    ))
}

/// Read the exact frozen check policy from a validated Work snapshot.
fn check_command(snapshot: &RunSnapshot) -> Result<Value, String> {
    let Some(policy) = snapshot.state().get("checkPolicy") else {
        return Ok(Value::Null);
    };
    // Reuse the same policy owner that validates the ledger, including the
    // exact command digest; do not infer arguments from executable readiness.
    Ok(json!(
        crate::run_value::policy_from_value(policy, 1)?.command
    ))
}

/// Reproduce the required facts from a validated predecessor snapshot. This
/// proves the one governor transition without treating that old view as a
/// current read route or changing the ledger.
pub(in crate::work) fn native_snapshot_facts(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    pre_edit_inputs: &str,
    managed: bool,
) -> Result<Value, String> {
    ensure_current_config(loaded)?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("predecessor action unavailable")?
        .remove("current");
    if let Some(assignment) = canonical["assignment"].as_str().map(str::to_owned) {
        super::super::attach_held(loaded, work, &assignment, &mut canonical)?;
    }
    let binding = binding(loaded, work, snapshot, &canonical)?;
    let recipient = super::super::readable::recipient_context::read(loaded, &canonical)?;
    if recipient["available"] != true || !recipient["rules"].is_array() {
        return Err("predecessor recipient instructions are unavailable".into());
    }
    let mut sections = Value::Object(super::super::readable::grouped_sections(
        loaded, work, snapshot, &canonical, &binding,
    )?);
    if !super::super::readable::evidence_complete(&sections["evidence"]) {
        return Err("predecessor required evidence is unavailable".into());
    }
    let observed = sections
        .pointer_mut("/assignment/residual/snapshot/inputsSha256")
        .filter(|value| value.is_string())
        .ok_or("predecessor input observation is unavailable")?;
    if pre_edit_inputs.len() != 64 || !pre_edit_inputs.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("grant pre-edit input identity is malformed".into());
    }
    // Only this historical observation comes from the exact validated grant.
    // Current result/check inputs remain untouched.
    *observed = json!(pre_edit_inputs);
    let forms = super::super::action_forms::full(loaded, work, &canonical, &binding);
    let mut sections = native_sections(sections, &forms, &json!(binding));
    let continuation = if managed {
        crate::project::continuation::runtime::delivery(loaded, work)?
    } else {
        None
    };
    if let Some(delivery) = continuation {
        sections["projectContinuation"] = delivery;
    }
    let mut facts = json!({"binding":binding,"requiredFactsSha256":hash::value(&sections),
        "recipientSha256":hash::value(&recipient)});
    if !managed {
        // An older presented delivery never carried an actor proof. A failed
        // optional projection cannot grant new credit, but must not break its
        // already-supported exact legacy permit bridge.
        let actor = check_command(snapshot).and_then(|command| {
            crate::host::runtime::selected_actor_projection(
                loaded, work, &binding, &sections, &command,
            )
        });
        if let Ok(actor) = actor {
            facts["actorFactsSha256"] = json!(hash::value(&actor));
        }
    }
    Ok(facts)
}
