use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

use super::recovery::inspect_command;

pub(super) struct RecordedFailureContext<'a> {
    pub(super) previous_head: Option<&'a str>,
    pub(super) stage: u64,
    pub(super) attempt: u64,
    pub(super) agent: &'a str,
    pub(super) role: &'a str,
    pub(super) outcome: &'a str,
    pub(super) submission_error: &'a str,
}

pub(super) fn recorded_failure_event(
    loaded: &Loaded,
    ledger: &str,
    context: RecordedFailureContext<'_>,
) -> Option<Value> {
    if !is_protection_append_error(context.submission_error) {
        return None;
    }
    let (_, events, _) = crate::run::ledger::load(loaded, ledger).ok()?;
    let event = events.last()?.clone();
    if context.previous_head == event["eventSha256"].as_str() {
        return None;
    }
    if event["action"] == "protect"
        && context.outcome == "accepted"
        && event["stage"].as_u64() == Some(context.stage)
        && event["attempt"].as_u64() == Some(context.attempt)
        && event["actor"].as_str() == Some(context.agent)
        && event["role"].as_str() == Some(context.role)
        && event["attemptedOutcome"] == "accepted"
    {
        Some(event)
    } else {
        None
    }
}

fn is_protection_append_error(error: &str) -> bool {
    error.starts_with("acceptance refused: configured check evidence is ")
}

pub(super) fn recorded_reference(loaded: &Loaded, ledger: &str, submitted: &Value) -> Value {
    let event_sha256 = submitted["event"]["eventSha256"].clone();
    match crate::run::ledger::load(loaded, ledger) {
        Ok((_, events, source)) => {
            let head_event_sha256 = events.last().map_or_else(
                || event_sha256.clone(),
                |event| event["eventSha256"].clone(),
            );
            json!({
                "ledger": ledger,
                "ledgerSha256": hash::bytes(source.as_bytes()),
                "headEventSha256": head_event_sha256,
                "eventSha256": event_sha256,
            })
        }
        Err(_) => json!({
            "ledger": ledger,
            "headEventSha256": event_sha256,
            "eventSha256": event_sha256,
        }),
    }
}

pub(super) fn recorded_projection_failure(
    work: &str,
    ledger: &str,
    submitted: &Value,
    projection_error: &str,
    reference: Value,
) -> Value {
    json!({
        "work": work,
        "effect": "recorded",
        "status": "recorded",
        "event": submitted["event"],
        "reference": reference,
        "reason": {"code": "recorded_then_failed"},
        "projectionError": projection_error,
        "nextAction": {
            "type": "inspect",
            "safe": true,
            "command": inspect_command(ledger),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{is_protection_append_error, recorded_projection_failure};
    use serde_json::json;

    #[test]
    fn recorded_projection_failure_is_successful_and_inspectable() {
        let submitted = json!({
            "event": {
                "eventSha256": "event-head",
                "action": "submit",
            }
        });
        let response = recorded_projection_failure(
            "smw_work",
            ".exitbind/runs/work-work.jsonl",
            &submitted,
            "projection failed",
            json!({
                "ledger": ".exitbind/runs/work-work.jsonl",
                "ledgerSha256": "ledger-hash",
                "headEventSha256": "event-head",
                "eventSha256": "event-head",
            }),
        );
        assert_eq!(response["effect"], "recorded");
        assert_eq!(response["reason"]["code"], "recorded_then_failed");
        assert_eq!(response["event"]["eventSha256"], "event-head");
        assert_eq!(response["reference"]["headEventSha256"], "event-head");
        assert_eq!(response["projectionError"], "projection failed");
        assert_eq!(response["nextAction"]["type"], "inspect");
        assert_eq!(response["nextAction"]["safe"], true);
        assert!(response.get("next").is_none());
    }

    #[test]
    fn only_the_protection_append_error_can_be_recorded() {
        assert!(is_protection_append_error(
            "acceptance refused: configured check evidence is check_failed"
        ));
        assert!(!is_protection_append_error(
            "assignment changed; no mutation was made"
        ));
    }
}
