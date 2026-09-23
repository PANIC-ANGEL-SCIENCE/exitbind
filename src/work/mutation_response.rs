use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

use super::recovery::inspect_command_for_config;

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

pub(super) fn recorded_protection_reference(
    ledger: &str,
    event: &Value,
    ledger_sha256: &str,
) -> Value {
    json!({
        "ledger": ledger,
        "ledgerSha256": ledger_sha256,
        "headEventSha256": event["eventSha256"],
        "eventSha256": event["eventSha256"],
    })
}

pub(super) fn recorded_projection_failure(
    work: &str,
    ledger: &str,
    submitted: &Value,
    projection_error: &str,
    reference: Value,
    config_path: &str,
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
            "command": inspect_command_for_config(config_path, ledger),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::recorded_projection_failure;
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
            "/project/exitbind.json",
        );
        assert_eq!(response["effect"], "recorded");
        assert_eq!(response["reason"]["code"], "recorded_then_failed");
        assert_eq!(response["event"]["eventSha256"], "event-head");
        assert_eq!(response["reference"]["headEventSha256"], "event-head");
        assert_eq!(response["projectionError"], "projection failed");
        assert_eq!(response["nextAction"]["type"], "inspect");
        assert_eq!(response["nextAction"]["safe"], true);
        assert!(response.get("next").is_none());
        assert_eq!(
            response["nextAction"]["command"],
            json!([
                crate::compatibility::profile().caller,
                "run",
                "inspect",
                ".exitbind/runs/work-work.jsonl",
                "--json",
                "--config",
                "/project/exitbind.json",
            ])
        );
    }
}
