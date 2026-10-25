//! Validate and project non-check observation facts; never mutate check credit.
use super::*;

/// The admission's observer identity: the process that wrote it and its start
/// marker, so a later check can tell it ended without trusting PID reuse.
pub(super) const OBSERVER_FIELD: &str = "observer";
pub(super) const OBSERVER_PID_FIELD: &str = "pid";
pub(super) const OBSERVER_START_FIELD: &str = "startTimeTicks";

/// The optional failure fact naming the checker's owned process group, when
/// the capsule recorded it before the observation ended without a result.
pub(crate) const OWNED_GROUP_FIELD: &str = "ownedProcessGroup";
/// The optional start marker of that group's leader, recorded with the group
/// so a reused group id is not mistaken for the owned group.
pub(crate) const OWNED_START_FIELD: &str = "ownedProcessStart";

pub(super) fn validate(event: &Value) -> Result<(), String> {
    if event["version"] != 8 || !event["observation"].is_object() {
        return Err("invalid check observation event".into());
    }
    let value = &event["observation"];
    if event["action"] == "check_observation" {
        if !observation_shape(value) || !binding_shape(&value["binding"]) {
            return Err("invalid check observation fields".into());
        }
        if value["state"] != "running"
            || !(value["binding"]["requirementId"].is_null()
                || value["binding"]["requirementId"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && id.len() <= 64))
            || value["timeoutMs"].as_u64().map_or(true, |v| {
                v == 0 || v > crate::run::check_observation::MAX_TIMEOUT_MS
            })
            || value["perStreamBytes"] != crate::run_value::MAX_CAPTURE_BYTES
            || value["combinedBytes"] != 16 * 1_048_576
        {
            return Err("invalid check observation execution limits".into());
        }
        for field in [
            "runId",
            "targetEventSha256",
            "subjectSha256",
            "inputsSha256",
            "configSha256",
            "checkCommandSha256",
            "observerExecutableSha256",
        ] {
            if !is_sha(value["binding"][field].as_str()) {
                return Err(format!("invalid check observation {field}"));
            }
        }
        if let Some(digest) = value["binding"].get("verificationPreparationSha256") {
            if !is_sha(digest.as_str())
                || !is_sha(value["binding"]["verificationOperationId"].as_str())
            {
                return Err("invalid check observation preparation binding".into());
            }
        } else if value["binding"].get("verificationOperationId").is_some() {
            return Err("invalid check observation preparation binding".into());
        }
    } else if !exact(value, &["admissionEventSha256", "facts"])
        || !is_sha(value["admissionEventSha256"].as_str())
        || !valid_facts(&value["facts"])
    {
        return Err("invalid check observation failure facts".into());
    }
    Ok(())
}

/// The admission fields. An observer identity is optional so every ledger
/// written before it existed still validates; a written one must be a real
/// PID with an optional start marker.
fn observation_shape(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    const BASE: &[&str] = &[
        "binding",
        "timeoutMs",
        "perStreamBytes",
        "combinedBytes",
        "state",
    ];
    let has_observer = object.contains_key(OBSERVER_FIELD);
    object.len()
        == if has_observer {
            BASE.len() + 1
        } else {
            BASE.len()
        }
        && BASE.iter().all(|field| object.contains_key(*field))
        && (!has_observer || observer(&value[OBSERVER_FIELD]))
}

fn observer(value: &Value) -> bool {
    exact(value, &[OBSERVER_PID_FIELD, OBSERVER_START_FIELD])
        && value[OBSERVER_PID_FIELD]
            .as_u64()
            .is_some_and(|pid| (1..=u32::MAX as u64).contains(&pid))
        && (value[OBSERVER_START_FIELD].is_u64() || value[OBSERVER_START_FIELD].is_null())
}

fn binding_shape(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    const BASE: &[&str] = &[
        "runId",
        "targetEventSha256",
        "subjectSha256",
        "inputsSha256",
        "configSha256",
        "checkCommandSha256",
        "requirementId",
        "observerExecutableSha256",
    ];
    let selected = object.contains_key("verificationPreparationSha256")
        && object.contains_key("verificationOperationId");
    let expected = if selected { BASE.len() + 2 } else { BASE.len() };
    object.len() == expected
        && BASE.iter().all(|field| object.contains_key(*field))
        && (!selected
            || object.contains_key("verificationPreparationSha256")
                && object.contains_key("verificationOperationId"))
        && object.keys().all(|key| {
            BASE.contains(&key.as_str())
                || (selected
                    && ["verificationPreparationSha256", "verificationOperationId"]
                        .contains(&key.as_str()))
        })
}

fn exact(value: &Value, fields: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field))
    })
}

fn process(value: &Value) -> bool {
    value.is_null()
        || match value["kind"].as_str() {
            Some("exit") => {
                exact(value, &["kind", "code"]) && value["code"].as_u64().is_some_and(|v| v <= 255)
            }
            Some("signal") => {
                exact(value, &["kind", "signal"]) && value["signal"].as_u64().is_some_and(|v| v > 0)
            }
            _ => false,
        }
}

fn partial(value: &Value) -> bool {
    exact(
        value,
        &[
            "root",
            "path",
            "sha256",
            "bytes",
            "stream",
            "completeness",
            "perStreamLimit",
        ],
    ) && value["root"] == "state"
        && value["completeness"] == "partial"
        && value["perStreamLimit"] == crate::run_value::MAX_CAPTURE_BYTES
        && value["bytes"]
            .as_u64()
            .is_some_and(|v| v <= crate::run_value::MAX_CAPTURE_BYTES)
        && is_sha(value["sha256"].as_str())
        && matches!(value["stream"].as_str(), Some("stdout" | "stderr"))
        && value["path"].as_str().is_some_and(|p| {
            p.len() <= 1024
                && p.starts_with(".exitbind/artifacts/check-partial-")
                && !p.contains(['\\', '\0'])
                && !p.split('/').any(|c| matches!(c, ".." | "."))
        })
}

pub(crate) fn valid_facts(value: &Value) -> bool {
    const BASE: &[&str] = &[
        "process",
        "processStarted",
        "groupEnded",
        "captureReadersEnded",
        "captureAvailability",
        "durationMs",
        "capture",
        "deadlineExceeded",
        "partialCaptures",
        "storageStage",
        "storage",
        "termination",
        "diagnostic",
        "cleanupErrorCount",
        "remainingOwnedPathCount",
    ];
    let Some(object) = value.as_object() else {
        return false;
    };
    let has_group = object.contains_key(OWNED_GROUP_FIELD);
    let has_start = object.contains_key(OWNED_START_FIELD);
    object.len() == BASE.len() + usize::from(has_group) + usize::from(has_start)
        && BASE.iter().all(|field| object.contains_key(*field))
        && (!has_group || owned_group(&value[OWNED_GROUP_FIELD]))
        && (!has_start || has_group && owned_start(&value[OWNED_START_FIELD]))
        && process(&value["process"])
        && value["processStarted"].is_boolean()
        && value["groupEnded"].is_boolean()
        && value["captureReadersEnded"].is_boolean()
        && (value["termination"] != "ended"
            || (value["groupEnded"] == true
                && value["captureReadersEnded"] == true
                && value["cleanupErrorCount"] == 0))
        && matches!(
            value["captureAvailability"].as_str(),
            Some("complete" | "partial" | "unavailable")
        )
        && value["durationMs"].as_u64().is_some()
        && matches!(
            value["capture"].as_str(),
            Some("complete" | "incomplete" | "disabled")
        )
        && value["deadlineExceeded"].is_boolean()
        && value["partialCaptures"]
            .as_array()
            .is_some_and(|v| v.len() <= 2 && v.iter().all(partial))
        && matches!(value["storageStage"].as_str(), Some("capture" | "commit"))
        && value["storage"] == "not_committed"
        && matches!(value["termination"].as_str(), Some("ended" | "unknown"))
        && value["diagnostic"]
            .as_str()
            .is_some_and(|v| v.chars().count() <= 1024)
        && value["cleanupErrorCount"].as_u64().is_some()
        && value["remainingOwnedPathCount"].as_u64().is_some()
}

/// A recorded owned process group: a positive Linux group id. It is the only
/// identity a failure may carry, and it is observed again, never trusted.
fn owned_group(value: &Value) -> bool {
    value.is_null()
        || value
            .as_i64()
            .is_some_and(|group| group > 0 && group <= i32::MAX as i64)
}

/// A recorded leader start marker: a positive host clock tick count.
fn owned_start(value: &Value) -> bool {
    value.as_u64().is_some_and(|ticks| ticks > 0)
}

pub(super) fn apply(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["checkObservationProtocol"] != crate::run::check_observation::PROTOCOL
        || state["status"] != "running"
    {
        return Err("check observation requires a marked running run".into());
    }
    if event["action"] == "check_observation" {
        if state["checkObservation"].is_object() || state["pendingDisposition"].is_object() {
            return Err("another observation or Lead decision is pending".into());
        }
        let binding = &event["observation"]["binding"];
        crate::run_value::validate_check_target(
            state,
            binding["targetEventSha256"].as_str().unwrap_or(""),
        )?;
        let policy = crate::run::active_check_policy(state, binding["requirementId"].as_str())?;
        if binding["runId"] != state["runId"]
            || binding["subjectSha256"] != state["subject"]["sha256"]
            || binding["configSha256"] != state["configSha256"]
            || binding["checkCommandSha256"] != policy.command_sha256
            || (state.get("verificationPreparationProtocol").is_some()
                != binding.get("verificationPreparationSha256").is_some())
            || (binding.get("verificationPreparationSha256").is_some()
                != binding.get("verificationOperationId").is_some())
        {
            return Err(
                "check observation does not match frozen configuration, current policy or subject"
                    .into(),
            );
        }
        state["checkObservation"] = event["observation"].clone();
        state["checkObservation"]["eventSha256"] = event["eventSha256"].clone();
        state["checkObservation"]["producer"] = event["producer"].clone();
        state["checkObservation"]["resumeStage"] = state["currentStage"].clone();
        state["currentStage"] = json!(lead_stage(state)?);
    } else {
        if state["checkObservation"]["state"] != "running"
            || event["producer"] != state["checkObservation"]["producer"]
            || event["observation"]["admissionEventSha256"]
                != state["checkObservation"]["eventSha256"]
        {
            return Err("check observation failure has no matching running admission".into());
        }
        state["checkObservation"]["state"] = json!("failed");
        state["checkObservation"]["facts"] = event["observation"]["facts"].clone();
        state["pendingDisposition"] = json!({"owner": "lead", "triggerEventSha256": event["eventSha256"],
            "basisSha256": state["basis"]["sha256"], "kind": "check_observation_failed", "findingSha256s": [event["eventSha256"]]});
    }
    Ok(())
}

pub(super) fn checked(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["checkObservation"].is_object() {
        let current = &state["checkObservation"];
        let binding = &current["binding"];
        if current["state"] != "running"
            || event["acquisition"] != "observed"
            || event["observationEventSha256"] != current["eventSha256"]
            || event["inputsSha256"] != binding["inputsSha256"]
            || event["configSha256"] != binding["configSha256"]
            || event["subjectSha256"] != binding["subjectSha256"]
            || event["targetEventSha256"] != binding["targetEventSha256"]
            || event["requirementId"] != binding["requirementId"]
            || event["checkCommandSha256"] != binding["checkCommandSha256"]
            || event["producer"] != current["producer"]
        {
            return Err("check does not complete its exact current observation".into());
        }
        state["currentStage"] = current["resumeStage"].clone();
        state["checkObservation"] = Value::Null;
    } else if event.get("observationEventSha256").is_some() {
        return Err("check has no admitted observation".into());
    }
    Ok(())
}

pub(crate) fn unresolved(state: &Value) -> bool {
    state["checkObservation"].is_object()
        && (state["checkObservation"]["state"] == "running"
            || state["checkObservation"]["facts"]["termination"] != "ended")
}
