//! Persisted same-goal governor accounting carried between fresh run identities.

use super::ledger::{ledger_path, load_at_unchecked, LedgerPath};
use super::*;
use std::collections::BTreeSet;

pub(crate) const PROTOCOL: u64 = 1;
const MAX_LINEAGE: usize = 64;

/// Initialize the run governor from its validated start marker and optional
/// successor carry seed; ordinary legacy starts keep their old behavior.
pub(crate) fn initialize_governor(start: &Value, state: &mut Value) -> Result<bool, String> {
    let enabled = start
        .get("governor")
        .is_some_and(crate::context::validate_marker);
    if start.get("governor").is_some() && !enabled {
        return Err("invalid run start: malformed governor marker".into());
    }
    if !enabled {
        return Ok(false);
    }
    let seed = start.get("governorCarry").map(|carry| &carry["accounting"]);
    state["governor"] = crate::context::reduce_governor_seeded(&[], seed)?;
    state["governor"]["enabled"] = json!(true);
    state["governor"]["defaults"] = start["governor"].clone();
    if let Some(protocol) = start["governor"].get("grantProtocol") {
        state["governor"]["grantProtocol"] = protocol.clone();
    }
    if let Some(carry) = start.get("governorCarry") {
        state["governorCarry"] = carry.clone();
    }
    Ok(true)
}

fn valid_sha(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

pub(crate) fn accounting(state: &Value, defaults: &Value) -> Result<Value, String> {
    let governor = state
        .get("governor")
        .filter(|value| value["enabled"] == true)
        .ok_or("same-goal supersession requires an enabled governor")?;
    for field in [
        "budget",
        "spent",
        "noInformationStreak",
        "postReplanSpent",
        "replanCount",
    ] {
        if governor[field].as_u64().is_none() {
            return Err(format!("governor carry has invalid {field}"));
        }
    }
    if governor["afterReplan"].as_bool().is_none()
        || governor["state"].as_str().is_none()
        || defaults.as_object().is_none()
    {
        return Err("governor carry has invalid phase or defaults".into());
    }
    let observation_keys = governor
        .get("observations")
        .and_then(Value::as_array)
        .ok_or("governor carry has invalid observation keys")?
        .iter()
        .map(|observation| {
            observation
                .get("key")
                .cloned()
                .ok_or("governor carry has invalid observation keys")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let accounting = json!({
        "budget": governor["budget"],
        "defaults": defaults,
        "spent": governor["spent"],
        "noInformationStreak": governor["noInformationStreak"],
        "postReplanSpent": governor["postReplanSpent"],
        "replanCount": governor["replanCount"],
        "afterReplan": governor["afterReplan"],
        "state": governor["state"],
        "seenEvidenceSha256": governor["seenEvidenceSha256"],
        "observationKeys": observation_keys,
    });
    crate::context::reduce_governor_seeded(&[], Some(&accounting))?;
    Ok(accounting)
}

pub(crate) fn make(
    old_state: &Value,
    old_ledger_path: &str,
    old_source: &str,
    old_events: &[Value],
    goal: &str,
) -> Result<Value, String> {
    if crate::run::state::check_observation::unresolved(old_state) {
        return Err("supersession refused unresolved check effects; no successor may retry uncertain execution".into());
    }
    let governor = old_state
        .get("governor")
        .filter(|value| value["enabled"] == true)
        .ok_or("governor carry requires an enabled predecessor governor")?;
    if governor["state"] != "ready" {
        return Err("supersession refused an exhausted or unresolved governor phase".into());
    }
    if let Some(request) = governor
        .get("currentSensorRequest")
        .filter(|v| v.is_object())
    {
        let key = crate::evidence::hash::value(&json!({
            "runId": request["runId"],
            "subjectSha256": request["subjectSha256"],
            "attempt": request["attempt"],
            "inputSha256": request["inputSha256"],
            "requestDigest": request["requestDigest"],
        }));
        let resolved = governor["seenSensors"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["key"] == key));
        if !resolved {
            return Err("supersession refused unresolved governor sensor request".into());
        }
    }
    let prior_goal = old_events
        .first()
        .and_then(|event| event["goal"].as_str())
        .ok_or("governor carry predecessor has no goal")?;
    if crate::evidence::hash::text(prior_goal) != crate::evidence::hash::text(goal) {
        return Err("governor carry requires the same goal".into());
    }
    let last = old_events
        .last()
        .ok_or("governor carry predecessor is empty")?;
    Ok(json!({
        "protocol": PROTOCOL,
        "goalSha256": crate::evidence::hash::text(goal),
        "predecessor": {
            "ledgerPath": old_ledger_path,
            "ledgerSha256": crate::evidence::hash::text(old_source),
            "runId": old_state["runId"],
            "headEventSha256": last["eventSha256"],
            "configSha256": old_state["configSha256"],
        },
        "accounting": accounting(
            old_state,
            &old_events[0]["governor"],
        )?,
    }))
}

pub(crate) fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    let has_protocol = event.get("carryProtocol").is_some();
    let has_carry = event.get("governorCarry").is_some();
    let has_hash = event.get("governorCarrySha256").is_some();
    if !(has_protocol || has_carry || has_hash) {
        return Ok(());
    }
    let invalid = || format!("invalid run ledger line {line}: invalid governor carry");
    if event["version"] != 8
        || event["carryProtocol"].as_u64() != Some(PROTOCOL)
        || event["governor"].as_object().is_none()
        || event["supersedes"].as_object().is_none()
        || event["governorCarry"].as_object().is_none()
        || event["governorCarrySha256"].as_str()
            != Some(crate::evidence::hash::value(&event["governorCarry"]).as_str())
    {
        return Err(invalid());
    }
    let carry = &event["governorCarry"];
    let Some(object) = carry.as_object() else {
        return Err(invalid());
    };
    if object.len() != 4
        || carry["protocol"].as_u64() != Some(PROTOCOL)
        || !valid_sha(carry["goalSha256"].as_str())
        || carry["goalSha256"] != crate::evidence::hash::text(event["goal"].as_str().unwrap_or(""))
        || carry["predecessor"] != event["supersedes"]
        || carry["accounting"].as_object().is_none()
    {
        return Err(invalid());
    }
    let accounting = &carry["accounting"];
    if accounting.as_object().map_or(true, |object| {
        object.len() != 10
            || !object.contains_key("observationKeys")
            || [
                "budget",
                "spent",
                "noInformationStreak",
                "postReplanSpent",
                "replanCount",
            ]
            .iter()
            .any(|field| accounting[field].as_u64().is_none())
            || accounting["afterReplan"].as_bool().is_none()
            || accounting["state"].as_str().is_none()
            || accounting["defaults"].as_object().is_none()
            || accounting["seenEvidenceSha256"].as_array().is_none()
            || accounting["seenEvidenceSha256"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| !valid_sha(item.as_str())))
            || crate::context::reduce_governor_seeded(&[], Some(accounting)).is_err()
    }) {
        return Err(invalid());
    }
    if accounting["budget"] != event["governor"]["budget"]
        || accounting["defaults"] != event["governor"]
        || !matches!(
            accounting["state"].as_str(),
            Some("ready" | "replan_required" | "evidence_required" | "blocked")
        )
    {
        return Err(invalid());
    }
    Ok(())
}

/// Verify every persisted supersession edge, including the accounting seed
/// against replayed predecessor events. A bounded visited set prevents cycles.
pub(crate) fn validate_chain(
    loaded: &Loaded,
    ledger: &LedgerPath,
    events: &[Value],
) -> Result<(), String> {
    let Some(start) = events.first() else {
        return Err("run ledger has no start event".into());
    };
    let mut seen = BTreeSet::new();
    validate_edge(loaded, ledger, start, &mut seen, 0)
}

fn validate_edge(
    loaded: &Loaded,
    ledger: &LedgerPath,
    start: &Value,
    seen: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_LINEAGE {
        return Err("supersession lineage exceeds the supported depth".into());
    }
    let Some(link) = start.get("supersedes") else {
        if has_carry_fields(start) {
            return Err("governor carry start has no predecessor link".into());
        }
        return Ok(());
    };
    let relative = link["ledgerPath"]
        .as_str()
        .ok_or("invalid superseded predecessor path")?;
    let marked = has_carry_fields(start);
    let predecessor = match ledger_path(&loaded.state_root, relative, true) {
        Ok(predecessor) => predecessor,
        Err(_) if !marked => return Ok(()),
        Err(error) => return Err(error),
    };
    let claim_result = super::ledger::valid_claim(&super::ledger::claim_path(&predecessor));
    let claim = if marked {
        claim_result?
    } else {
        // Legacy starts never depended on a claim. Ignore absent or malformed
        // historical claims unless a valid claim identifies a carry edge.
        match claim_result {
            Ok(claim) if claim.as_ref().is_some_and(has_carry_fields) => claim,
            _ => return Ok(()),
        }
    };
    if !marked && claim.is_none() {
        // Preserve historical unmarked replay: old starts did not require
        // their predecessor file or claim to remain available.
        return Ok(());
    }
    if !seen.insert(relative.to_owned()) {
        return Err("supersession lineage contains a cycle".into());
    }
    let claim = claim.ok_or("governor carry is missing its persisted successor claim")?;
    if claim["carryProtocol"].as_u64() != Some(PROTOCOL)
        || !valid_sha(claim["governorCarrySha256"].as_str())
        || claim["oldLedgerPath"] != link["ledgerPath"]
        || claim["oldLedgerSha256"] != link["ledgerSha256"]
        || claim["oldRunId"] != link["runId"]
        || claim["oldHeadEventSha256"] != link["headEventSha256"]
        || claim["oldConfigSha256"] != link["configSha256"]
        || claim["newLedgerPath"] != relative_path(ledger)?
        || claim["newRunId"] != start["runId"]
        || claim["configSha256"] != start["configSha256"]
        || claim["goalSha256"] != crate::evidence::hash::text(start["goal"].as_str().unwrap_or(""))
    {
        return Err("governor carry successor claim provenance mismatch".into());
    }
    if !marked
        || start["carryProtocol"].as_u64() != Some(PROTOCOL)
        || claim["governorCarrySha256"] != start["governorCarrySha256"]
    {
        return Err("governor carry marker is missing or differs from its persisted claim".into());
    }
    validate_start(start, 1)?;
    let (_, predecessor_events, source) = load_at_unchecked(loaded, &predecessor)?;
    let head = predecessor_events
        .last()
        .ok_or("superseded predecessor is empty")?;
    if crate::evidence::hash::text(&source) != link["ledgerSha256"]
        || predecessor_events[0]["runId"] != link["runId"]
        || head["eventSha256"] != link["headEventSha256"]
        || predecessor_events[0]["configSha256"] != link["configSha256"]
    {
        return Err("superseded predecessor provenance mismatch".into());
    }
    if start["governorCarry"]["predecessor"] != *link
        || start["governorCarry"]["goalSha256"]
            != crate::evidence::hash::text(
                predecessor_events[0]["goal"]
                    .as_str()
                    .ok_or("predecessor has no goal")?,
            )
    {
        return Err("governor carry predecessor provenance mismatch".into());
    }
    let predecessor_state = crate::run::state::reduce(&predecessor_events)?;
    if accounting(&predecessor_state, &predecessor_events[0]["governor"])?
        != start["governorCarry"]["accounting"]
    {
        return Err("governor carry accounting differs from predecessor replay".into());
    }
    validate_edge(
        loaded,
        &predecessor,
        &predecessor_events[0],
        seen,
        depth + 1,
    )?;
    Ok(())
}

fn has_carry_fields(value: &Value) -> bool {
    value.get("carryProtocol").is_some()
        || value.get("governorCarry").is_some()
        || value.get("governorCarrySha256").is_some()
}

fn relative_path(ledger: &LedgerPath) -> Result<&str, String> {
    ledger
        .expected
        .strip_prefix(&ledger.root)
        .map_err(|_| "ledger path escapes project root".to_string())?
        .to_str()
        .ok_or_else(|| "ledger path is not valid UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_state_at_hard_budget_carries_telemetry() {
        let state = json!({
            "runId": "run",
            "configSha256": "config",
            "governor": {
                "enabled": true,
                "state": "ready",
                "spent": 3,
                "budget": 3,
                "noInformationStreak": 0,
                "postReplanSpent": 0,
                "replanCount": 1,
                "afterReplan": false,
                "seenEvidenceSha256": [],
                "observations": [],
            },
        });
        let events = [
            json!({"goal": "same goal", "governor": {"budget": 3}}),
            json!({"eventSha256": "a".repeat(64)}),
        ];
        let carry = make(&state, "old.jsonl", "source", &events, "same goal").unwrap();
        assert_eq!(carry["accounting"]["spent"], 3);
        assert_eq!(carry["accounting"]["budget"], 3);
        assert_eq!(carry["accounting"]["state"], "ready");
        assert_eq!(carry["accounting"]["observationKeys"], json!([]));
    }

    #[test]
    fn marked_carry_requires_valid_observation_keys() {
        fn start(include_observation_keys: bool, observation_keys: Value) -> Value {
            let mut accounting = json!({
                "budget": 3,
                "defaults": {"budget": 3},
                "spent": 3,
                "noInformationStreak": 0,
                "postReplanSpent": 0,
                "replanCount": 1,
                "afterReplan": false,
                "state": "ready",
                "seenEvidenceSha256": [],
            });
            if include_observation_keys {
                accounting["observationKeys"] = observation_keys;
            }
            let supersedes = json!({
                "ledgerPath": "old.jsonl",
                "ledgerSha256": "b".repeat(64),
                "runId": "old-run",
                "headEventSha256": "c".repeat(64),
                "configSha256": "d".repeat(64),
            });
            let carry = json!({
                "protocol": PROTOCOL,
                "goalSha256": crate::evidence::hash::text("goal"),
                "predecessor": supersedes,
                "accounting": accounting,
            });
            json!({
                "version": 8,
                "carryProtocol": PROTOCOL,
                "governor": {"budget": 3},
                "supersedes": supersedes,
                "goal": "goal",
                "governorCarrySha256": crate::evidence::hash::value(&carry),
                "governorCarry": carry,
            })
        }

        let valid = start(true, json!(["a".repeat(64)]));
        assert!(validate_start(&valid, 1).is_ok());

        let malformed = start(true, json!(["A".repeat(64)]));
        assert!(validate_start(&malformed, 1).is_err());

        let missing = start(false, Value::Null);
        let mut carry = missing["governorCarry"].clone();
        carry["accounting"].as_object_mut().unwrap().remove("observationKeys");
        let mut rehashed = missing;
        rehashed["governorCarrySha256"] = json!(crate::evidence::hash::value(&carry));
        rehashed["governorCarry"] = carry;
        assert!(validate_start(&rehashed, 1).is_err());
    }
}
