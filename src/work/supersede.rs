// Coordinates native Work supersession with Run and Goal owners.
// Requires a bounded non-empty reason; see `src/work/mod.rs`.
//! Explicit native Work supersession, using the existing Run and Goal owners.

use super::*;
use crate::session_goal::{self, association};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UncreditedRetirementSnapshot {
    pub(crate) work: String,
    pub(crate) start_event_sha256: String,
    pub(crate) head_event_sha256: String,
    pub(crate) config_sha256: String,
    pub(crate) raw_ledger_sha256: String,
    pub(crate) governor_sha256: String,
    pub(crate) governor_spent: Option<u64>,
}

/// Observe an explicitly selected, uncredited blocked Work while its existing
/// Run lock is held. This is not acceptance or authority to change the Work.
/// All source writers must also satisfy the owner's quiescence attestation:
/// an older writer can create a retained result without taking this lock.
pub(crate) fn uncredited_retirement_snapshot(
    loaded: &Loaded,
    work: &str,
) -> Result<UncreditedRetirementSnapshot, String> {
    let relative = resolve(loaded, work)?;
    let (ledger, events, source) = run::ledger::load(loaded, &relative)?;
    let state = run::state::reduce(&events)?;
    let start = events.first().ok_or("retired Work has no start")?;
    let head = events.last().ok_or("retired Work has no current head")?;
    if state["status"] != "blocked" {
        return Err("only a terminal blocked Work can be retired without credit".into());
    }
    let bound_config = start["configSha256"]
        .as_str()
        .ok_or("retired Work configuration identity is unavailable")?;
    let current_config = hash::text(&loaded.source);
    if bound_config != current_config.as_str() {
        return Err(crate::drift::refusal(
            crate::drift::Class::JourneyMemberConfiguration,
            bound_config,
            Some(&current_config),
            None,
            crate::drift::Route::WorkDetail {
                work,
                config: &loaded.path,
            },
        ));
    }
    if run::ledger::valid_claim(&run::ledger::claim_path(&ledger))?.is_some() {
        return Err("a claimed successor prevents uncredited retirement".into());
    }
    // Unmarked legacy checks had no durable admission. If a worker ever
    // completed, their absence or termination cannot be established here.
    if state["checkObservationProtocol"] != run::check_observation::PROTOCOL
        && events.iter().any(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
        })
    {
        return Err("legacy check execution is unverifiable; retirement refused".into());
    }
    if run::state::check_observation::unresolved(&state)
        || action::continuation_native_facts(loaded, work, &state)?.retirement_blocked
    {
        return Err("an active, unknown or retained Work operation prevents retirement".into());
    }
    if run::pre_return::retirement_has_unresolved(loaded, &state)? {
        return Err("an unresolved pre-return attempt prevents retirement".into());
    }
    if let Some(child) = session_goal::blocking_child(loaded, work)? {
        return Err(format!(
            "a live, prepared or retained child prevents retirement: {child}"
        ));
    }
    if held::has_for_work(loaded, work)? {
        return Err("a retained result prevents retirement".into());
    }
    let governor_spent = state["governor"]["spent"].as_u64();
    if state["governor"]["enabled"] == true && governor_spent.is_none() {
        return Err("retired Work governor accounting is unavailable".into());
    }
    Ok(UncreditedRetirementSnapshot {
        work: work.to_owned(),
        start_event_sha256: start["eventSha256"]
            .as_str()
            .ok_or("retired Work start identity is unavailable")?
            .to_owned(),
        head_event_sha256: head["eventSha256"]
            .as_str()
            .ok_or("retired Work terminal identity is unavailable")?
            .to_owned(),
        config_sha256: start["configSha256"]
            .as_str()
            .ok_or("retired Work configuration identity is unavailable")?
            .to_owned(),
        raw_ledger_sha256: hash::bytes(source.as_bytes()),
        governor_sha256: hash::value(&state["governor"]),
        governor_spent,
    })
}

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
