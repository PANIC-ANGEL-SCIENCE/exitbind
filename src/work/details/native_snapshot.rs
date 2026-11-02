//! Reconstruct native delivery facts from validated Work snapshots.
use super::{binding, ensure_current_config, native_sections};
use crate::{config::Loaded, evidence::hash, run::RunSnapshot};
use serde_json::{json, Value};

/// Read the frozen policy from a fresh snapshot of the same bound action.
pub(crate) fn current_check_command(
    loaded: &Loaded,
    work: &str,
    expected_binding: &str,
) -> Result<Value, String> {
    ensure_current_config(loaded)?;
    let snapshot = RunSnapshot::capture(loaded, &super::super::resolve(loaded, work)?)?;
    let current = super::super::next_from(loaded, work, &snapshot)?;
    if current["current"]["binding"] != expected_binding {
        return Err(super::stale_action_binding(loaded, work));
    }
    check_command(&snapshot)
}

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
