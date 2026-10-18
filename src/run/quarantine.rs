//! Immutable representation for separately authorized verification of an
//! artifact whose earlier check retains unknown effects. Representation
//! validation does not establish live owner authority or runtime isolation.

use serde_json::{json, Value};

pub(crate) mod admission;
mod custody;
mod followup;
mod verification;
pub(crate) use custody::validate_against_first_check as validate_custody_against_first_check;
pub(crate) use followup::validate_first_failed_check;
pub(crate) use verification::{acknowledgement, validate_event};

pub(crate) const PROTOCOL: u64 = 1;
pub(crate) const FOLLOWUP_PROTOCOL: u64 = 2;
pub(crate) const OPERATION_MAX_BYTES: usize = 4096;

/// Validate the command text shared by the producer and persisted marker
/// reader. The limit is measured in UTF-8 bytes because that is what the
/// ledger stores and hashes.
pub(crate) fn validate_operation(operation: &str) -> Result<(), &'static str> {
    if operation.trim().is_empty() {
        return Err("must contain non-whitespace text");
    }
    if operation.len() > OPERATION_MAX_BYTES {
        return Err("exceeds the 4096-byte limit");
    }
    if operation.contains('\0') {
        return Err("must not contain NUL");
    }
    Ok(())
}

pub(crate) fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    let Some(marker) = event.get("quarantineVerification") else {
        return Ok(());
    };
    validate_marker(marker, line)?;
    let owner = &marker["leadOwner"];
    let configured_owner = event["plan"]["stages"].as_array().is_some_and(|stages| {
        stages.iter().any(|stage| {
            stage["agents"].as_array().is_some_and(|agents| {
                agents.iter().any(|agent| {
                    agent["role"] == "lead"
                        && agent["name"] == owner["agent"]
                        && agent["nativeTaskName"] == owner["nativeTaskName"]
                })
            })
        })
    });
    let independent_reviewer = event["plan"]["stages"].as_array().is_some_and(|stages| {
        stages
            .iter()
            .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
            .any(|agent| {
                agent["role"] == "reviewer"
                    && agent["name"] != owner["agent"]
                    && agent["nativeTaskName"] != owner["nativeTaskName"]
            })
    });
    if event["version"] != 8
        || [
            "supersedes",
            "checkReplacement",
            "governor",
            "governorCarry",
            "carryProtocol",
            "governorCarrySha256",
            "recoveryProtocol",
            "preReturnPolicy",
            "preservation",
            "projectContinuation",
        ]
        .iter()
        .any(|field| event.get(*field).is_some())
        || !configured_owner
        || !independent_reviewer
        || event["reviewPolicy"]["decision"] != "required"
        || event["basisProtocol"] != crate::kernel::basis::PROTOCOL_VERSION
        || event["checkObservationProtocol"] != crate::run::check_observation::PROTOCOL
        || event["checkPolicy"]["origin"] != "local_report"
        || event["configSha256"] != marker["verification"]["configSha256"]
        || event["checkPolicy"]["commandSha256"] != marker["verification"]["commandSha256"]
    {
        return Err(invalid(line));
    }
    Ok(())
}

/// Verification authority never supplies ordinary mutation, repair or
/// replacement authority, including after its sole check has failed.
pub(crate) fn refuse_mutation(state: &Value, operation: &str) -> Result<(), String> {
    if state.get("quarantineVerification").is_some() {
        return Err(format!(
            "quarantine verification has no {operation} allowance"
        ));
    }
    Ok(())
}

pub(crate) fn validate_marker(value: &Value, line: usize) -> Result<(), String> {
    let fail = || invalid(line);
    let protocol = value["protocol"].as_u64();
    let base_keys = [
        "protocol",
        "requestId",
        "reason",
        "leadOwner",
        "predecessor",
        "verification",
        "allowance",
        "oldCredited",
        "oldReset",
        "decisionSha256",
    ];
    let mut marker_keys = base_keys.to_vec();
    if protocol == Some(FOLLOWUP_PROTOCOL) {
        marker_keys.push("followUp");
    }
    if !exact(value, &marker_keys)
        || !matches!(protocol, Some(PROTOCOL | FOLLOWUP_PROTOCOL))
        || !text(&value["requestId"], 128)
        || !text(&value["reason"], 1024)
        || value["oldCredited"] != false
        || value["oldReset"] != false
        || !sha(&value["decisionSha256"])
        || !exact(&value["leadOwner"], &["agent", "nativeTaskName"])
        || !text(&value["leadOwner"]["agent"], 128)
        || !text(&value["leadOwner"]["nativeTaskName"], 128)
    {
        return Err(fail());
    }
    followup::validate_marker(value, line)?;
    let predecessor = &value["predecessor"];
    if !exact(
        predecessor,
        &[
            "work",
            "ledger",
            "runId",
            "configSha256",
            "startEventSha256",
            "headEventSha256",
            "ledgerSha256",
            "observationEventSha256",
            "admissionEventSha256",
            "accounting",
        ],
    ) || !work(&predecessor["work"])
        || ![
            "runId",
            "configSha256",
            "startEventSha256",
            "headEventSha256",
            "ledgerSha256",
            "observationEventSha256",
            "admissionEventSha256",
        ]
        .iter()
        .all(|key| sha(&predecessor[key]))
    {
        return Err(fail());
    }
    let handle = predecessor["work"].as_str().ok_or_else(fail)?;
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &handle[4..]);
    if predecessor["ledger"].as_str() != Some(ledger.as_str()) {
        return Err(fail());
    }
    let accounting = &predecessor["accounting"];
    if !exact(accounting, &["governor", "usageAccount"])
        || !accounting["governor"].is_object()
        || !accounting["usageAccount"].is_object()
        || !["spent", "budget", "replanCount", "postReplanSpent"]
            .iter()
            .all(|key| accounting["governor"][key].as_u64().is_some())
    {
        return Err(fail());
    }
    let verification = &value["verification"];
    if !exact(
        verification,
        &[
            "artifactSha256",
            "inputsSha256",
            "commandSha256",
            "configSha256",
            "environmentSha256",
            "operation",
            "isolationReview",
        ],
    ) || ![
        "artifactSha256",
        "inputsSha256",
        "commandSha256",
        "configSha256",
        "environmentSha256",
    ]
    .iter()
    .all(|key| sha(&verification[key]))
        || !verification["operation"]
            .as_str()
            .is_some_and(|operation| validate_operation(operation).is_ok())
    {
        return Err(fail());
    }
    let review = &verification["isolationReview"];
    let review_hashes = [
        "startEventSha256",
        "headEventSha256",
        "subjectSha256",
        "checkEventSha256",
        "reviewEventSha256",
        "acceptanceEventSha256",
        "planSha256",
        "effectInventorySha256",
        "environmentSha256",
    ];
    let mut review_keys = review_hashes.to_vec();
    review_keys.push("work");
    if !exact(review, &review_keys)
        || !work(&review["work"])
        || review["work"] == predecessor["work"]
        || !review_hashes.iter().all(|key| sha(&review[key]))
        || review["environmentSha256"] != verification["environmentSha256"]
        || !exact(
            &value["allowance"],
            &["checks", "workerMutations", "replans", "replacements"],
        )
        || value["allowance"]["checks"].as_u64() != Some(1)
        || !["workerMutations", "replans", "replacements"]
            .iter()
            .all(|key| value["allowance"][key].as_u64() == Some(0))
    {
        return Err(fail());
    }
    let mut body = value.clone();
    body.as_object_mut()
        .ok_or_else(fail)?
        .remove("decisionSha256");
    if value["decisionSha256"] != crate::evidence::hash::value(&body) {
        return Err(fail());
    }
    Ok(())
}

/// A new request, artifact or decision never supplies a second allowance for
/// the same original check admission.
pub(crate) fn one_shot_identity(marker: &Value) -> Result<String, String> {
    validate_marker(marker, 0)?;
    if marker["protocol"] == FOLLOWUP_PROTOCOL {
        followup::identity(marker)
    } else {
        Ok(crate::evidence::hash::value(&json!({
            "protocol": PROTOCOL,
            "oldWork": marker["predecessor"]["work"],
            "startEventSha256": marker["predecessor"]["startEventSha256"],
            "admissionEventSha256": marker["predecessor"]["admissionEventSha256"],
        })))
    }
}

fn invalid(line: usize) -> String {
    format!("invalid run ledger line {line}: invalid quarantine verification")
}

fn exact(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

fn text(value: &Value, limit: usize) -> bool {
    value
        .as_str()
        .is_some_and(|text| !text.trim().is_empty() && text.len() <= limit && !text.contains('\0'))
}

fn sha(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn work(value: &Value) -> bool {
    value.as_str().is_some_and(|work| {
        work.strip_prefix("smw_").is_some_and(|hash| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seal(value: &mut Value) {
        value.as_object_mut().unwrap().remove("decisionSha256");
        value["decisionSha256"] = json!(crate::evidence::hash::value(value));
    }

    pub(super) fn marker() -> Value {
        let hash = "a".repeat(64);
        let mut value = json!({
            "protocol":1,"requestId":"independent-check-1","reason":"Retain unknown effects",
            "leadOwner":{"agent":"lead","nativeTaskName":"lead"},
            "predecessor":{
                "work":format!("smw_{hash}"),"ledger":format!(".exitbind/runs/work-{hash}.jsonl"),
                "runId":hash,"configSha256":hash,"startEventSha256":hash,
                "headEventSha256":hash,"ledgerSha256":hash,"observationEventSha256":hash,
                "admissionEventSha256":hash,
                "accounting":{"governor":{"spent":9,"budget":3,"replanCount":2,
                    "postReplanSpent":1,"retainedEvidence":["prior failure"]},
                    "usageAccount":{"status":"unknown","totals":null}}
            },
            "verification":{
                "artifactSha256":hash,"inputsSha256":hash,"commandSha256":hash,
                "configSha256":hash,"environmentSha256":hash,"operation":"Verify frozen artifact",
                "isolationReview":{"work":format!("smw_{}","b".repeat(64)),
                    "startEventSha256":hash,"headEventSha256":hash,"subjectSha256":hash,
                    "checkEventSha256":hash,"reviewEventSha256":hash,"acceptanceEventSha256":hash,
                    "planSha256":hash,"effectInventorySha256":hash,"environmentSha256":hash}
            },
            "allowance":{"checks":1,"workerMutations":0,"replans":0,"replacements":0},
            "oldCredited":false,"oldReset":false
        });
        seal(&mut value);
        value
    }

    fn start(marker: Value) -> Value {
        json!({"version":8,"configSha256":marker["verification"]["configSha256"],
            "checkPolicy":{"commandSha256":marker["verification"]["commandSha256"],"origin":"local_report"},
            "reviewPolicy":{"decision":"required"},
            "basisProtocol":crate::kernel::basis::PROTOCOL_VERSION,
            "checkObservationProtocol":crate::run::check_observation::PROTOCOL,
            "plan":{"stages":[{"agents":[{"role":"lead","name":"lead","nativeTaskName":"lead"}]},
                {"agents":[{"role":"reviewer","name":"reviewer","nativeTaskName":"reviewer"}]}]},
            "quarantineVerification":marker})
    }

    #[test]
    fn quarantine_preserves_overspent_accounting_and_unknown_usage() {
        let value = marker();
        let before = value["predecessor"]["accounting"].to_string();
        validate_marker(&value, 1).unwrap();
        validate_start(&start(value.clone()), 1).unwrap();
        assert_eq!(value["predecessor"]["accounting"].to_string(), before);
        assert_eq!(value["predecessor"]["accounting"]["governor"]["spent"], 9);
        assert!(value["predecessor"]["accounting"]["usageAccount"]["totals"].is_null());
    }

    #[test]
    fn quarantine_operation_uses_shared_utf8_byte_bound_for_producer_and_marker() {
        let mut values = vec![
            "x".repeat(255),
            "x".repeat(256),
            "x".repeat(257),
            "x".repeat(328),
            "x".repeat(OPERATION_MAX_BYTES),
        ];
        let unicode_at_limit = format!("{}é", "x".repeat(OPERATION_MAX_BYTES - 2));
        assert_eq!(unicode_at_limit.len(), OPERATION_MAX_BYTES);
        values.push(unicode_at_limit);
        for operation in values {
            assert!(validate_operation(&operation).is_ok());
            let mut value = marker();
            value["verification"]["operation"] = json!(operation);
            seal(&mut value);
            validate_marker(&value, 1).unwrap();
        }

        let over_limit = "x".repeat(OPERATION_MAX_BYTES + 1);
        let unicode_over_limit = format!("{}é", "x".repeat(OPERATION_MAX_BYTES - 1));
        assert_eq!(unicode_over_limit.len(), OPERATION_MAX_BYTES + 1);
        for operation in [
            over_limit,
            unicode_over_limit,
            String::new(),
            " \t\n".to_owned(),
            "exec sh checker.sh\0".to_owned(),
        ] {
            assert!(validate_operation(&operation).is_err());
            let mut value = marker();
            value["verification"]["operation"] = json!(operation);
            seal(&mut value);
            assert!(validate_marker(&value, 1).is_err());
        }
    }

    #[test]
    fn quarantine_refuses_semantically_unsafe_resealed_markers() {
        let cases = [
            ("/protocol", json!(2)),
            ("/oldCredited", json!(true)),
            ("/oldReset", json!(true)),
            ("/allowance/checks", json!(2)),
            ("/allowance/workerMutations", json!(1)),
            ("/allowance/replans", json!(1)),
            ("/allowance/replacements", json!(1)),
            ("/reason", json!("\u{0}")),
            ("/requestId", json!(" ")),
            ("/predecessor/runId", json!("short")),
            ("/predecessor/accounting/governor/spent", json!(-1)),
            ("/predecessor/accounting/usageAccount", Value::Null),
            (
                "/verification/isolationReview/work",
                json!(format!("smw_{}", "a".repeat(64))),
            ),
            (
                "/verification/isolationReview/environmentSha256",
                json!("b".repeat(64)),
            ),
        ];
        for (path, changed) in cases {
            let mut value = marker();
            *value.pointer_mut(path).unwrap() = changed;
            seal(&mut value);
            assert!(
                validate_marker(&value, 7).unwrap_err().contains("line 7"),
                "{path}"
            );
        }
        for ledger in [
            "/absolute",
            "../escape",
            ".exitbind/runs/../other.jsonl",
            ".exitbind\\runs\\file.jsonl",
            ".exitbind/runs/work-other.jsonl",
        ] {
            let mut value = marker();
            value["predecessor"]["ledger"] = json!(ledger);
            seal(&mut value);
            assert!(validate_marker(&value, 1).is_err(), "{ledger}");
        }
    }

    #[test]
    fn quarantine_refuses_unknown_structural_fields_and_digest_drift() {
        for path in [
            "",
            "/leadOwner",
            "/predecessor",
            "/predecessor/accounting",
            "/verification",
            "/verification/isolationReview",
            "/allowance",
        ] {
            let mut value = marker();
            value.pointer_mut(path).unwrap()["unexpected"] = json!(true);
            seal(&mut value);
            assert!(validate_marker(&value, 1).is_err(), "{path}");
        }
        let mut value = marker();
        value["reason"] = json!("unsealed change");
        assert!(validate_marker(&value, 1).is_err());
    }

    #[test]
    fn quarantine_one_shot_identity_survives_request_and_artifact_changes() {
        let original = marker();
        let identity = one_shot_identity(&original).unwrap();
        let mut changed = original.clone();
        changed["requestId"] = json!("a-new-request");
        changed["reason"] = json!("a changed reason");
        changed["verification"]["artifactSha256"] = json!("c".repeat(64));
        seal(&mut changed);
        assert_ne!(changed["decisionSha256"], original["decisionSha256"]);
        assert_eq!(one_shot_identity(&changed).unwrap(), identity);
        for field in ["startEventSha256", "admissionEventSha256"] {
            let mut changed = original.clone();
            changed["predecessor"][field] = json!("c".repeat(64));
            seal(&mut changed);
            assert_ne!(one_shot_identity(&changed).unwrap(), identity);
        }
    }

    #[test]
    fn quarantine_start_requires_exact_owner_policy_and_separate_lineage() {
        for (path, changed) in [
            ("/version", json!(9)),
            ("/configSha256", json!("c".repeat(64))),
            ("/checkPolicy/commandSha256", json!("c".repeat(64))),
            ("/checkPolicy/origin", json!("ci")),
            ("/reviewPolicy/decision", json!("omitted")),
            ("/checkObservationProtocol", json!(0)),
            ("/plan/stages/0/agents/0/name", json!("another-owner")),
            ("/plan/stages/1/agents/0/nativeTaskName", json!("lead")),
        ] {
            let mut value = start(marker());
            *value.pointer_mut(path).unwrap() = changed;
            assert!(validate_start(&value, 1).is_err(), "{path}");
        }
        for field in [
            "supersedes",
            "checkReplacement",
            "governor",
            "governorCarry",
            "recoveryProtocol",
            "preReturnPolicy",
            "preservation",
            "projectContinuation",
        ] {
            let mut value = start(marker());
            value[field] = Value::Null;
            assert!(validate_start(&value, 1).is_err(), "{field}");
        }
        validate_start(&json!({"version":8}), 1).unwrap();
    }
}
