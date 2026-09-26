//! How a run reports its review outcome in `run status` and the Exit Path
//! receipt.  A review cycle the Lead resolved by `defer` or `reject` is
//! reported as that resolution, never as an omission or an approval.

use serde_json::{json, Value};

/// The Lead's resolution of the current review cycle, with the decision and
/// disposition identity that produced it.
fn resolution(state: &Value) -> Option<Value> {
    crate::run::disposition::current_resolution(state).map(|resolution| {
        json!({
            "status": "resolved_by_lead_disposition",
            "decision": resolution["decision"],
            "dispositionSha256": resolution["dispositionSha256"],
            "findingSha256s": resolution["findingSha256s"],
            "decisionSha256": resolution["reviewDecisionSha256"],
        })
    })
}

/// The `review` value of `run status`, given the latest reviewer submission of
/// the current attempt.
pub(crate) fn status(state: &Value, review: Option<&Value>) -> Value {
    if state["reviewPolicy"]["decision"] == "omitted" {
        return json!({
            "status": "omitted",
            "decisionSha256": state["reviewPolicy"]["sha256"],
            "source": state["reviewPolicy"]["source"],
            "reason": state["reviewPolicy"]["reason"],
        });
    }
    let approved = review.filter(|submission| submission["outcome"] == "approved");
    if approved.is_none() {
        if let Some(resolved) = resolution(state) {
            return resolved;
        }
    }
    review.map_or_else(
        || json!({"status":"absent"}),
        |submission| {
            json!({
                "status": submission["outcome"],
                "eventSha256": submission["eventSha256"],
                "artifactSha256": submission["artifact"]["sha256"],
            })
        },
    )
}

/// The `review` block of the Exit Path receipt.  `reviewer` is the current
/// approval, if any; a required review needs an approval or a Lead resolution.
pub(crate) fn receipt(
    state: &Value,
    review_required: bool,
    review_decision_sha256: Option<&str>,
    reviewer: Option<&Value>,
) -> Result<Value, String> {
    let marked_policy = state.get("basisProtocol").is_some();
    let Some(reviewer) = reviewer else {
        if review_required {
            return resolution(state)
                .ok_or_else(|| "Exit Path receipt requires reviewer approval".to_owned());
        }
        return Ok(if marked_policy {
            json!({
                "status": "omitted",
                "decisionSha256": review_decision_sha256,
                "source": "owner-reported",
            })
        } else {
            json!({})
        });
    };
    Ok(if marked_policy {
        json!({
            "status": "approved",
            "eventSha256": reviewer["eventSha256"],
            "artifactSha256": reviewer["artifact"]["sha256"],
        })
    } else {
        json!({
            "eventSha256": reviewer["eventSha256"],
            "artifactSha256": reviewer["artifact"]["sha256"],
        })
    })
}
