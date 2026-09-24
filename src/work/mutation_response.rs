//! Default `work check` and `work return` JSON contract. `effect` records
//! whether a ledger event was committed or a worker result was held. `result`
//! is the checker's observed outcome; `outcome` is a role return or refusal.
//! `eventSha256` names this call's event. `next` is only a short hint and always
//! requires a fresh `work next --full` before another mutation. The read-only
//! `nextAction.command` inspects the named event even after the head advances.
//! An argv suffix/prefix marked `sameExecutableRequired` or
//! `sameConfigRequired` needs the exact current executable or config argument
//! before execution; neither is guessed silently.

use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

use super::recovery::inspect_command_for_config;

/// Small result envelope shared by check and return.  The event selector comes
/// from the append result, never from a later ledger head.  `next` is only a
/// hint: its packet must be read afresh before another mutation.
pub(super) fn bounded(
    response: &Value,
    work: &str,
    assignment: Option<&str>,
    ledger: &str,
    config_path: &str,
) -> Value {
    let event = &response["event"];
    let event_sha = event["eventSha256"].as_str();
    let held = response["effect"] == "held";
    let refused = event["action"] == "protect";
    let effect = if held { "held" } else { "recorded" };
    let detail_suffix = if let Some(sha) = event_sha {
        vec![
            "run".to_owned(),
            "inspect".to_owned(),
            ledger.to_owned(),
            "--event".to_owned(),
            sha.to_owned(),
            "--json".to_owned(),
            "--config".to_owned(),
        ]
    } else {
        vec![
            "work".to_owned(),
            "next".to_owned(),
            work.to_owned(),
            "--full".to_owned(),
            "--config".to_owned(),
        ]
    };
    let detail = super::response_recovery::bounded_argv(
        detail_suffix.clone(),
        Some(config_path),
        crate::work::compact::MAX_RESPONSE_BYTES - 512,
    );
    let mut value = json!({
        "compact": true,
        "work": work,
        "assignment": assignment,
        "status": if refused { "refused" } else { effect },
        "effect": effect,
        "eventSha256": event_sha,
        "event": {"action": event["action"], "eventSha256": event_sha,
            "result": event["result"], "outcome": event["outcome"],
            "targetEventSha256": event["targetEventSha256"]},
        "targetEventSha256": event["targetEventSha256"],
        "result": event["result"],
        "outcome": if refused { json!("refused") } else if held { json!("completed") } else { event["outcome"].clone() },
        "requestedOutcome": response["requestedOutcome"],
        "reference": if response["reference"].is_object() {
            response["reference"].clone()
        } else {
            json!({"ledger": ledger, "eventSha256": event_sha})
        },
        "nextAction": {"type": "inspect", "safe": true, "command": detail.argv},
        "next": {
            "action": response["next"]["action"],
            "assignment": response["next"]["assignment"],
            "role": response["next"]["role"],
            "resolvedActor": response["next"]["resolvedActor"],
            "check": {"kind": response["next"]["check"]["kind"], "requirementId": response["next"]["check"]["requirementId"]},
            "progress": {"state": response["next"]["progress"]["state"], "reason": {"code": response["next"]["progress"]["reason"]["code"]}},
            "requiresExpansion": true,
        },
        "continuation": {"readOnly": true, "commandSuffix": ["work", "next", work, "--full"], "sameConfig": true},
    });
    if detail.same_config {
        value["nextAction"]["sameConfigRequired"] = json!(true);
        value["nextAction"]["configArgument"] =
            json!("reuse the exact --config value from this invocation");
    }
    if detail.same_executable {
        value["nextAction"]["sameExecutableRequired"] = json!(true);
        value["nextAction"]["executableArgument"] =
            json!("prepend the exact executable used for this invocation");
    }
    if detail.same_config || detail.same_executable {
        value["nextAction"]["commandKind"] =
            json!(match (detail.same_executable, detail.same_config) {
                (true, true) => "argv_suffix_requires_executable_and_config",
                (true, false) => "argv_suffix_requires_executable",
                (false, true) => "argv_prefix_requires_config_value",
                (false, false) => unreachable!(),
            });
    }
    if held {
        value["nextAction"]["type"] = json!("inspect_held");
        value["held"] = json!({
            "reference": response["held"]["reference"],
            "sha256": response["held"]["sha256"],
            "bytes": response["held"]["bytes"],
            "action": response["held"]["action"],
        });
        value["reason"] = json!({"code": "governor_replan_or_evidence_required"});
    } else if refused
        || response["projectionError"].is_string()
        || response["cleanupError"].is_string()
        || response["heldCleanupWarning"].is_string()
    {
        value["reason"] = json!({"code": "recorded_then_failed"});
    }
    let mut phases = Vec::new();
    if refused {
        phases.push("refusal");
    }
    if response["projectionError"].is_string() {
        phases.push("projection");
    }
    if response["cleanupError"].is_string() || response["heldCleanupWarning"].is_string() {
        phases.push("cleanup");
    }
    if !phases.is_empty() {
        value["diagnostic"] = json!({"phase": phases[0], "phases": phases});
    }
    if let Some(presentation) = response.get("presentation") {
        value["presentation"] = json!({
            "neuro": presentation["neuro"],
            "phrase": presentation["phrase"],
            "terminal": presentation["terminal"],
        });
    }
    if serialized_len(&value) > crate::work::compact::MAX_RESPONSE_BYTES {
        value.as_object_mut().unwrap().remove("presentation");
        value["next"]["progress"] = Value::Null;
    }
    if serialized_len(&value) > crate::work::compact::MAX_RESPONSE_BYTES {
        value.as_object_mut().unwrap().remove("continuation");
        value["next"]["check"] = Value::Null;
        value["next"]["role"] = Value::Null;
        value["event"] = Value::Null;
    }
    if serialized_len(&value) > crate::work::compact::MAX_RESPONSE_BYTES {
        // The exact executable and config were supplied by this invocation.
        // Require both explicitly rather than guessing either at the limit.
        value["nextAction"]["command"] = json!(detail_suffix);
        value["nextAction"]["sameConfigRequired"] = json!(true);
        value["nextAction"]["sameExecutableRequired"] = json!(true);
        value["nextAction"]["commandKind"] = json!("argv_suffix_requires_executable_and_config");
        value["nextAction"]["configArgument"] =
            json!("reuse the exact --config value from this invocation");
        value["nextAction"]["executableArgument"] =
            json!("prepend the exact executable used for this invocation");
    }
    value
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value)
        .expect("mutation result is serializable")
        .len()
        + 1
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
    use super::{bounded, recorded_projection_failure, serialized_len};
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

    #[test]
    fn oversized_diagnostic_and_config_keep_exact_bounded_result() {
        let sha = "a".repeat(64);
        let work = format!("smw_{}", "b".repeat(64));
        let ledger = format!(".exitbind/runs/work-{}.jsonl", "b".repeat(64));
        let config = format!("/project/{}/exitbind.json", "p".repeat(7_000));
        let response = json!({
            "event": {"action": "check", "eventSha256": sha,
                "targetEventSha256": "target", "result": {"kind": "exit", "code": 7}},
            "projectionError": "diagnostic".repeat(8_000),
            "next": {"action": "check", "assignment": "assignment", "heldResults": ["held".repeat(8_000)]},
        });
        let compact = bounded(&response, &work, None, &ledger, &config);
        assert!(serialized_len(&compact) <= 8 * 1024);
        assert_eq!(compact["eventSha256"], sha);
        assert_eq!(compact["result"]["code"], 7);
        assert_eq!(compact["effect"], "recorded");
        assert_eq!(compact["diagnostic"]["phase"], "projection");
        assert_eq!(compact["next"]["requiresExpansion"], true);
        assert!(compact["nextAction"]["command"].is_array());
        assert_eq!(
            compact["nextAction"]["command"]
                .as_array()
                .unwrap()
                .last()
                .unwrap(),
            &json!(config)
        );
    }

    #[test]
    fn path_exceeding_budget_keeps_read_only_same_config_argv() {
        let sha = "a".repeat(64);
        let response =
            json!({"event": {"action": "submit", "eventSha256": sha, "outcome": "completed"}});
        let compact = bounded(
            &response,
            "smw_work",
            Some("assignment"),
            "ledger.jsonl",
            &format!("/project/{}/exitbind.json", "p".repeat(10_000)),
        );
        assert!(serialized_len(&compact) <= 8 * 1024);
        assert_eq!(compact["nextAction"]["sameConfigRequired"], true);
        let command = compact["nextAction"]["command"].as_array().unwrap();
        assert_eq!(command[1], "run");
        assert_eq!(command[2], "inspect");
        assert_eq!(command[5], sha);
        assert_eq!(command.last().unwrap(), "--config");
        assert_eq!(
            compact["nextAction"]["commandKind"],
            "argv_prefix_requires_config_value"
        );
        assert_eq!(compact["outcome"], "completed");
    }

    #[test]
    fn simultaneous_projection_and_cleanup_faults_keep_both_phases() {
        let sha = "a".repeat(64);
        let response = json!({
            "event": {"action": "submit", "eventSha256": sha, "outcome": "completed"},
            "projectionError": "projection failed".repeat(10_000),
            "heldCleanupWarning": "held cleanup failed".repeat(10_000),
        });
        let compact = bounded(
            &response,
            "smw_work",
            Some("sma_assignment"),
            "ledger.jsonl",
            "/project/exitbind.json",
        );
        assert!(serialized_len(&compact) <= 8 * 1024);
        assert_eq!(compact["eventSha256"], sha);
        assert_eq!(compact["effect"], "recorded");
        assert_eq!(compact["outcome"], "completed");
        assert_eq!(
            compact["diagnostic"]["phases"],
            json!(["projection", "cleanup"])
        );
        assert_eq!(compact["nextAction"]["command"][2], "inspect");
    }
}
