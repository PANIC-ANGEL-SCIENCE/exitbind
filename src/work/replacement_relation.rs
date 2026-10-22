//! The check-replacement relation shown on a Work's detail: a successor
//! verified against its committed start marker (or the one submit being
//! previewed), or a predecessor verified through its supersession claim and
//! successor start. The relation conveys no authority or credit.
use crate::config::Loaded;
use serde_json::{json, Value};

fn replacement_policy_projection(marker: &Value, mut relation: Value) -> Value {
    if marker["target"] == "pre_return" {
        relation["target"] = json!("pre_return");
        for side in ["predecessor", "successor"] {
            if let Some(object) = relation[side].as_object_mut() {
                if let Some(policy) = object.remove("checkPolicy") {
                    object.insert("preReturnPolicy".into(), policy);
                }
            }
        }
        relation["finalCheckPolicyPreserved"] = json!(true);
        relation["meaning"] = json!("The predecessor remains immutable. Only the explicitly defective pre-return policy changed; the successor gets no old grant, diagnostic pass, final check or review credit.");
    }
    relation
}

pub(super) fn relation(
    loaded: &Loaded,
    work: &str,
    events: &[Value],
) -> Result<Option<Value>, String> {
    let proposed_start = events.first().ok_or("Work ledger has no start")?;
    if proposed_start.get("checkReplacement").is_some() {
        let ledger = super::resolve(loaded, work)?;
        let (_, current_events, _) = crate::run::ledger::load(loaded, &ledger)?;
        if !events_match_committed_or_submit_preview(events, &current_events) {
            return Err(
                "check replacement changed during Work detail; refresh the current Work view"
                    .into(),
            );
        }
        let start = current_events.first().ok_or("Work ledger has no start")?;
        let marker = start
            .get("checkReplacement")
            .ok_or("check replacement changed during Work detail; refresh the current Work view")?;
        if proposed_start.get("checkReplacement") != Some(marker) {
            return Err(
                "check replacement changed during Work detail; refresh the current Work view"
                    .into(),
            );
        }
        let predecessor = start["supersedes"]["ledgerPath"]
            .as_str()
            .unwrap_or_default();
        let previous_work = crate::work::locator_for_ledger(predecessor);
        return Ok(Some(replacement_policy_projection(
            marker,
            json!({"state":"verified","direction":"successor",
            "predecessor":{"work":previous_work,"ledger":predecessor,
                "runId":start["supersedes"]["runId"],"headEventSha256":start["supersedes"]["headEventSha256"],
                "checkPolicy":marker["oldPolicy"]},
            "successor":{"work":work,"runId":start["runId"],"checkPolicy":marker["newPolicy"]},
            "source":marker["source"],"reason":marker["reason"],"classification":marker["classification"],"leadOwner":marker["leadOwner"],
            "meaning":"The predecessor is immutable; the successor has a new check contract and receives no prior check, review or grant credit."}),
        )));
    }
    let ledger = super::resolve(loaded, work)?;
    let ledger_path = crate::run::ledger::load(loaded, &ledger)?.0;
    let Some(claim) =
        crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&ledger_path))?
    else {
        return Ok(None);
    };
    let Some(marker) = claim.get("checkReplacement") else {
        return Ok(None);
    };
    let successor_ledger = claim["newLedgerPath"].as_str().unwrap_or_default();
    let verified = crate::run::ledger::load(loaded, successor_ledger)
        .ok()
        .is_some_and(|(_, successor_events, _)| {
            successor_events.first().is_some_and(|successor| {
                successor["checkReplacement"] == *marker
                    && successor["supersedes"]["ledgerPath"] == claim["oldLedgerPath"]
                    && successor["supersedes"]["headEventSha256"] == claim["oldHeadEventSha256"]
            })
        });
    if !verified {
        return Ok(Some(json!({"state":"unverified","direction":"predecessor",
            "predecessor":{"work":work,"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"]},
            "successor":{"work":crate::work::locator_for_ledger(successor_ledger),"ledger":successor_ledger,"runId":claim["newRunId"]},
            "reason":"successor ledger or provenance could not be verified; no continuation is authorized"})));
    }
    Ok(Some(replacement_policy_projection(
        marker,
        json!({"state":"verified","direction":"predecessor",
        "predecessor":{"work":work,"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"],"checkPolicy":marker["oldPolicy"]},
        "successor":{"work":crate::work::locator_for_ledger(successor_ledger),"ledger":successor_ledger,"runId":claim["newRunId"],"checkPolicy":marker["newPolicy"]},
        "source":marker["source"],"reason":marker["reason"],"classification":marker["classification"],"leadOwner":marker["leadOwner"],
        "meaning":"This predecessor remains immutable; its successor has a new check contract and receives no prior check, review or grant credit."}),
    )))
}

/// A preflight projection may contain the one submit transaction being
/// considered, while the ledger on disk is still the checked committed base.
/// Every other consumer must present that exact base; no arbitrary suffix or
/// changed committed history is a valid preview.
fn events_match_committed_or_submit_preview(candidate: &[Value], committed: &[Value]) -> bool {
    if candidate == committed {
        return true;
    }
    if candidate.len() != committed.len().saturating_add(1)
        || candidate.get(..committed.len()) != Some(committed)
    {
        return false;
    }
    let Some(previous) = committed.last() else {
        return false;
    };
    let Some(submission) = candidate.last() else {
        return false;
    };
    submission["kind"] == "run"
        && submission["action"] == "submit"
        && submission["version"] == previous["version"]
        && submission["runId"] == previous["runId"]
        && submission["previousEventSha256"] == previous["eventSha256"]
        && crate::run::state::validate_event(submission, Some(previous), committed.len() + 1)
            .is_ok()
}

#[cfg(test)]
mod replacement_relation_preview_tests {
    use super::*;

    fn committed() -> Vec<Value> {
        vec![json!({
            "version": 8,
            "runId": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "eventSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "timestamp": "2026-10-14T00:00:00Z"
        })]
    }

    fn submit(previous: &Value) -> Value {
        crate::run::state::make_event(json!({
            "version": 8,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(8),
            "action": "submit",
            "runId": previous["runId"],
            "stage": 1,
            "attempt": 1,
            "agent": "lead",
            "role": "lead",
            "outcome": "scoped",
            "artifact": {
                "root": "state",
                "path": "result.json",
                "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
            },
            "previousEventSha256": previous["eventSha256"],
            "timestamp": "2026-10-14T00:00:01Z"
        }))
    }

    #[test]
    fn replacement_relation_accepts_only_exact_current_or_one_valid_linked_submit() {
        let committed = committed();
        assert!(events_match_committed_or_submit_preview(
            &committed, &committed
        ));

        let transaction = submit(&committed[0]);
        let mut preview = committed.clone();
        preview.push(transaction.clone());
        assert!(events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut changed_prefix = preview.clone();
        changed_prefix[0]["timestamp"] = json!("2026-10-14T00:00:00Z");
        changed_prefix[0]["eventSha256"] =
            json!("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd");
        assert!(!events_match_committed_or_submit_preview(
            &changed_prefix,
            &committed
        ));

        let mut wrong_previous = transaction.clone();
        wrong_previous["previousEventSha256"] =
            json!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
        wrong_previous
            .as_object_mut()
            .unwrap()
            .remove("eventSha256");
        let wrong_previous = crate::run::state::make_event(wrong_previous);
        let mut preview = committed.clone();
        preview.push(wrong_previous);
        assert!(!events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut wrong_run = transaction.clone();
        wrong_run["runId"] =
            json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
        wrong_run.as_object_mut().unwrap().remove("eventSha256");
        let wrong_run = crate::run::state::make_event(wrong_run);
        let mut preview = committed.clone();
        preview.push(wrong_run);
        assert!(!events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut non_submit = transaction.clone();
        non_submit["action"] = json!("check");
        non_submit.as_object_mut().unwrap().remove("eventSha256");
        let non_submit = crate::run::state::make_event(non_submit);
        let mut preview = committed.clone();
        preview.push(non_submit);
        assert!(!events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut multiple = committed.clone();
        multiple.push(transaction.clone());
        multiple.push(transaction.clone());
        assert!(!events_match_committed_or_submit_preview(
            &multiple, &committed
        ));

        let mut invalid_hash = committed.clone();
        invalid_hash.push(submit(&invalid_hash[0]));
        invalid_hash[1]["eventSha256"] =
            json!("0000000000000000000000000000000000000000000000000000000000000000");
        assert!(!events_match_committed_or_submit_preview(
            &invalid_hash,
            &committed
        ));
    }
}
