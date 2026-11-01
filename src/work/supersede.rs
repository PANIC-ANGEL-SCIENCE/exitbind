//! Explicit native Work supersession, using the existing Run and Goal owners.

use super::*;
use crate::session_goal::{self, association};

pub(crate) fn supersede(loaded: &Loaded, work: &str, reason: &str) -> Result<Value, String> {
    if reason.trim().is_empty() || reason.len() > 1024 || reason.contains('\0') {
        return Err("--reason must be non-empty and at most 1024 bytes".into());
    }
    details::ensure_current_config(loaded)?;
    let ledger = resolve(loaded, work)?;
    let (_, events, _) = run::ledger::load(loaded, &ledger)?;
    let state = run::state::reduce(&events)?;
    let start = events.first().ok_or("predecessor Work has no start")?;
    let head = events
        .last()
        .and_then(|event| event["eventSha256"].as_str())
        .ok_or("predecessor Work has no current head")?;
    if start["configSha256"] != hash::text(&loaded.source) {
        return Err(
            "native Work supersession requires the unchanged predecessor configuration".into(),
        );
    }
    run::assert_no_drift(loaded, &state)?;
    let scope = session_goal::native::scope(loaded, false)?;
    scope.require_owner()?;
    let session = scope
        .session
        .ok_or("native Work supersession requires its owner session")?;
    association::verify_owner_session(loaded, work, session)?;
    let next = next_for(loaded, work, &ledger)?;
    let assignment = next["assignment"].as_str().map(str::to_owned);
    refuse_pending(loaded, work, &state, assignment.as_deref())?;
    let workflow = start["workflow"]
        .as_str()
        .ok_or("predecessor workflow is unavailable")?;
    let goal = start["goal"]
        .as_str()
        .ok_or("predecessor Goal is unavailable")?;
    let token = hash::value(&json!({"domain":"exitbind.native-work-supersession.v1",
        "predecessor":work,"headEventSha256":head,"configSha256":start["configSha256"],
        "reason":reason}));
    let successor = format!("smw_{token}");
    let new_ledger = format!("{}/work-{token}.jsonl", runs_dir());
    let reason_option = format!("--reason={reason}");
    let retry = crate::emitted::Route::new(["work", "supersede", work, &reason_option, "--json"])
        .config(&loaded.path)
        .within(4096)
        .argv_json();
    let ((link, detail), focus) = focus::successor(loaded, work, &successor, || {
        let mut admitted_here = false;
        let mut admit = |event: &Value, claim: &Value, claimed: bool, started: bool| {
            if claim["oldHeadEventSha256"] != head || claim["oldLedgerPath"] != ledger {
                return Err("predecessor changed after the native supersession request; no admission was recorded".into());
            }
            details::ensure_current_config(loaded)?;
            run::assert_no_drift(loaded, &state)?;
            refuse_pending(loaded, work, &state, assignment.as_deref())?;
            let (intent, created) =
                association::admit_successor(loaded, work, &successor, event, claimed, started)?;
            admitted_here = created;
            Ok(intent)
        };
        run::supersede_with_native_admission(
            loaded, &ledger, workflow, goal, &new_ledger, &mut admit,
        ).map_err(|error| format!("{error}; if a native admission was recorded, retain and repeat the exact original work supersede request, including its reason"))?;
        let link = association::link_session(loaded, &successor);
        let detail = action_forms::current_detail_command(loaded, &successor);
        let state = if link.is_err() {
            focus::SuccessorState::LinkPending
        } else if admitted_here {
            focus::SuccessorState::FreshLinked
        } else {
            focus::SuccessorState::Recovery
        };
        Ok(((link, detail), state))
    })?;
    Ok(match link {
        Ok(link) => json!({"work":successor,"predecessor":work,
            "sessionLink":link,"effect":if focus["current"] == true {"started"} else {"started-focus-pending"},
            "focus":focus,"currentDetail":detail,
            "recovery":{"command":retry,"requiresExactOriginalReason":true}}),
        Err(error) => json!({"work":successor,"predecessor":work,
            "sessionLink":{"error":error,"recovery":crate::emitted::Route::new(
                ["work", "link-session", &successor, "--json"]).config(&loaded.path).within(4096).argv_json()},
            "effect":"started-link-pending","focus":focus,"currentDetail":detail,
            "recovery":{"command":retry,"requiresExactOriginalReason":true}}),
    })
}

fn refuse_pending(
    loaded: &Loaded,
    work: &str,
    state: &Value,
    assignment: Option<&str>,
) -> Result<(), String> {
    if action::continuation_native_facts(loaded, work, state)?.retirement_blocked {
        return Err("native Work supersession is blocked by an active, unknown or retained managed operation".into());
    }
    if let Some(child) = session_goal::blocking_child(loaded, work)? {
        return Err(format!(
            "native Work supersession is blocked by an unresolved child: {child}"
        ));
    }
    if let Some(assignment) = assignment {
        if action_forms::has_held_result(loaded, work, assignment)? {
            return Err("native Work supersession is blocked by a retained result".into());
        }
    }
    Ok(())
}
