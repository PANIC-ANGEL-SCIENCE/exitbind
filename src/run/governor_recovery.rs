//! Owner-attested, evidence-bound recovery for one duplicate-permit terminal block.
//!
//! This module only creates a successor seed. It never edits or replays the
//! blocked predecessor, and it does not infer absence of effects from ledger
//! inputs. The owner must supply a complete bounded inventory artifact.

use crate::{config::Loaded, run::artifact};
use serde_json::{json, Value};

pub(crate) const PROTOCOL: u64 = 1;
const MAX_DECISION_BYTES: u64 = 32 * 1024;
const MAX_EVIDENCE_BYTES: u64 = 32 * 1024;

/// Exact predecessor and successor inputs used to verify carried evidence.
/// Keeping this context together prevents call sites from swapping identities.
pub(crate) struct RecoveryBinding<'a> {
    pub(super) loaded: &'a Loaded,
    pub(super) state: &'a Value,
    pub(super) events: &'a [Value],
    pub(super) source: &'a str,
    pub(super) old_ledger_path: &'a str,
    pub(super) goal: &'a str,
    pub(super) successor_config_sha256: &'a str,
}

/// A read-only template for the one terminal refusal shape this successor can
/// recover. It deliberately records no approval and no effect conclusion.
pub(crate) fn draft(
    loaded: &Loaded,
    work: &str,
    ledger_name: &str,
) -> Result<Option<Value>, String> {
    let ledger = super::ledger::ledger_path(&loaded.state_root, ledger_name, false)?;
    let (_, events, source) = super::ledger::load_at(loaded, &ledger)?;
    let state = super::state::reduce(&events)?;
    if state["governor"]["enabled"] != true
        || state["governor"]["state"] != "blocked"
        || !matches!(state["status"].as_str(), Some("running" | "blocked"))
    {
        return Ok(None);
    }
    let Some(grant) = events.iter().rev().nth(1) else {
        return Ok(None);
    };
    let Some(blocked) = events.last() else {
        return Ok(None);
    };
    let partial = json!({
        "grantEventSha256": grant["eventSha256"],
        "grantGovernorEventSha256": grant["governorEvent"]["eventSha256"],
        "blockedEventSha256": blocked["eventSha256"],
        "blockedGovernorEventSha256": blocked["governorEvent"]["eventSha256"],
    });
    if validate_adjacent_duplicate(&state, &events, &partial).is_err()
        || crate::run::state::check_observation::unresolved(&state)
        || unresolved_sensor_request(&state, &events)
        || state["held"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        || state["heldResults"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    {
        return Ok(None);
    }
    let old_path = ledger
        .expected
        .strip_prefix(&ledger.root)
        .map_err(|_| "predecessor ledger escapes StateRoot".to_string())?
        .to_str()
        .ok_or("predecessor ledger path is not UTF-8")?;
    if crate::work::locator_for_ledger(old_path).as_deref() != Some(work) {
        return Ok(None);
    }
    let predecessor = predecessor_identity(
        &state,
        &events,
        &source,
        old_path,
        state["goal"].as_str().unwrap_or_default(),
    )?;
    let identity = event_identity(&state, &events, &grant["eventSha256"], old_path)?;
    if crate::work::action_forms::has_held_result(
        loaded,
        identity["work"].as_str().unwrap_or_default(),
        identity["assignment"].as_str().unwrap_or_default(),
    )? {
        return Ok(None);
    }
    let product_snapshot = crate::run::inputs::fingerprint(loaded)?;
    let decision = json!({
        "version": 1,
        "kind": "duplicate_permit_owner_recovery",
        "approved": false,
        "owner": "<OWNER_IDENTITY>",
        "reason": "<OWNER_REASON>",
        "currentConfigSha256": crate::evidence::hash::text(&loaded.source),
        "predecessor": predecessor,
        "grantEventSha256": grant["eventSha256"],
        "grantGovernorEventSha256": grant["governorEvent"]["eventSha256"],
        "blockedEventSha256": blocked["eventSha256"],
        "blockedGovernorEventSha256": blocked["governorEvent"]["eventSha256"],
        "identity": identity,
        "productSnapshotSha256": product_snapshot,
        "invocation": {"sameExactInvocation": false, "evidence": "<OWNER_INVOCATION_EVIDENCE>"},
        "effectsEvidence": {"root":"state", "path":"<EFFECT_EVIDENCE_PATH>", "sha256":"<EFFECT_EVIDENCE_SHA256>", "bytes":0}
    });
    Ok(Some(json!({
        "workflow": state["workflow"],
        "goal": state["goal"],
        "decision": decision,
        "effectsInventory": {"productWrites":"unknown", "processStarts":"unknown", "networkWrites":"unknown", "otherExternal":"unknown"},
        "decisionRequired": true,
        "meaning": "Read-only candidate only. It does not authorize recovery. The owner must attest the exact invocation, bind a complete known-no-effect StateRoot artifact, then refresh the command inputs; unknown effects remain blocked."
    })))
}

/// Check the current sensor request against its validated request/result
/// history. The result key includes that result's checkpoint; the current
/// mutation may have advanced since the result was recorded.
pub(crate) fn unresolved_sensor_request(state: &Value, events: &[Value]) -> bool {
    let request = &state["governor"]["currentSensorRequest"];
    if !request.is_object() {
        return false;
    }
    let fields = [
        "runId",
        "subjectSha256",
        "attempt",
        "inputSha256",
        "requestDigest",
    ];
    let Some((request_index, request_event)) = events
        .iter()
        .enumerate()
        .rev()
        .find(|(_, event)| event["governorEvent"]["action"] == "sensor_request")
    else {
        return true;
    };
    if fields
        .iter()
        .any(|field| request_event["governorEvent"][*field] != request[*field])
    {
        return true;
    }
    let Some(seen) = state["governor"]["seenSensors"].as_array() else {
        return true;
    };
    !events.iter().skip(request_index + 1).any(|event| {
        let result = &event["governorEvent"];
        if result["action"] != "sensor"
            || fields.iter().any(|field| result[*field] != request[*field])
        {
            return false;
        }
        let key = crate::evidence::hash::value(&json!({
            "runId": result["runId"],
            "subjectSha256": result["subjectSha256"],
            "attempt": result["attempt"],
            "checkpoint": result["checkpoint"],
            "inputSha256": result["inputSha256"],
            "requestDigest": result["requestDigest"],
        }));
        seen.iter().any(|item| {
            item["key"] == key
                && item["assessment"] == result["assessment"]
                && item["inputDigest"] == result["inputDigest"]
        })
    })
}

pub(crate) fn build_carry_evidence(
    loaded: &Loaded,
    path: &str,
    state: &Value,
    events: &[Value],
    source: &str,
    old_ledger_path: &str,
    goal: &str,
) -> Result<Value, String> {
    let decision = read_state_json(loaded, path, MAX_DECISION_BYTES, "owner recovery")?;
    let effects_ref = descriptor(&decision.value["effectsEvidence"], "effects evidence")?;
    if effects_ref["bytes"].as_u64().unwrap_or(u64::MAX) > MAX_EVIDENCE_BYTES {
        return Err("owner recovery effects evidence exceeds 32768 bytes".into());
    }
    let effects = read_descriptor(loaded, &effects_ref, "effects evidence")?;
    let successor_config_sha256 = crate::evidence::hash::text(&loaded.source);
    let binding = RecoveryBinding {
        loaded,
        state,
        events,
        source,
        old_ledger_path,
        goal,
        successor_config_sha256: &successor_config_sha256,
    };
    let carry = build_carry_evidence_from_decision(&decision, &effects, &binding)?;
    let current_inputs = crate::run::inputs::fingerprint(loaded)?;
    let product_snapshot = carry["productSnapshotSha256"]
        .as_str()
        .ok_or("owner recovery product snapshot digest is missing")?;
    if !valid_sha(Some(product_snapshot))
        || current_inputs != product_snapshot
        || current_inputs != carry["identity"]["inputsSha256"]
    {
        return Err("owner recovery product-root snapshot does not match current inputs".into());
    }
    if carry["currentConfigSha256"] != crate::evidence::hash::text(&loaded.source) {
        return Err("owner recovery current configuration identity does not match".into());
    }
    Ok(carry)
}

pub(crate) fn validate_carried_evidence(
    carry: &Value,
    binding: &RecoveryBinding<'_>,
) -> Result<(), String> {
    exact_fields(
        carry,
        &[
            "protocol",
            "decision",
            "effectsEvidence",
            "ownerDecisionSha256",
            "predecessor",
            "currentConfigSha256",
            "grantEventSha256",
            "grantGovernorEventSha256",
            "blockedEventSha256",
            "blockedGovernorEventSha256",
            "identity",
            "productSnapshotSha256",
            "effectInventory",
        ],
        "carried owner recovery",
    )?;
    if carry["protocol"] != PROTOCOL || !valid_sha(carry["productSnapshotSha256"].as_str()) {
        return Err("invalid carried owner recovery marker".into());
    }
    if carry["currentConfigSha256"] != binding.successor_config_sha256 {
        return Err("carried owner recovery configuration binding differs from successor".into());
    }
    let decision_descriptor = descriptor(&carry["decision"], "owner recovery")?;
    if decision_descriptor["bytes"].as_u64().unwrap_or(u64::MAX) > MAX_DECISION_BYTES {
        return Err("carried owner recovery exceeds 32768 bytes".into());
    }
    let decision = read_descriptor(binding.loaded, &decision_descriptor, "owner recovery")?;
    let effects_descriptor = descriptor(&carry["effectsEvidence"], "effects evidence")?;
    if effects_descriptor["bytes"].as_u64().unwrap_or(u64::MAX) > MAX_EVIDENCE_BYTES {
        return Err("carried effects evidence exceeds 32768 bytes".into());
    }
    let effects = read_descriptor(binding.loaded, &effects_descriptor, "effects evidence")?;
    validate_effects(&effects.value)?;
    let rebuilt = build_carry_evidence_from_decision(&decision, &effects, binding)?;
    if rebuilt != *carry {
        return Err("carried owner recovery evidence differs from its verified artifacts".into());
    }
    Ok(())
}

struct ReadArtifact {
    descriptor: Value,
    value: Value,
}

fn read_state_json(
    loaded: &Loaded,
    path: &str,
    max_bytes: u64,
    label: &str,
) -> Result<ReadArtifact, String> {
    let descriptor = artifact::evidence(loaded, Some("state"), path)?;
    if descriptor["bytes"].as_u64().unwrap_or(u64::MAX) > max_bytes {
        return Err(format!("{label} exceeds {} bytes", max_bytes));
    }
    read_descriptor(loaded, &descriptor, label)
}

fn read_descriptor(
    loaded: &Loaded,
    descriptor: &Value,
    label: &str,
) -> Result<ReadArtifact, String> {
    let verified = artifact::read(loaded, descriptor, label)?;
    let source = std::str::from_utf8(&verified.preview)
        .map_err(|_| format!("{label} must be UTF-8 JSON"))?;
    let value = serde_json::from_str(source).map_err(|_| format!("{label} is invalid JSON"))?;
    Ok(ReadArtifact {
        descriptor: descriptor.clone(),
        value,
    })
}

fn build_carry_evidence_from_decision(
    decision: &ReadArtifact,
    effects: &ReadArtifact,
    binding: &RecoveryBinding<'_>,
) -> Result<Value, String> {
    let loaded = binding.loaded;
    let value = &decision.value;
    exact_fields(
        value,
        &[
            "version",
            "kind",
            "approved",
            "owner",
            "reason",
            "currentConfigSha256",
            "predecessor",
            "grantEventSha256",
            "grantGovernorEventSha256",
            "blockedEventSha256",
            "blockedGovernorEventSha256",
            "identity",
            "productSnapshotSha256",
            "invocation",
            "effectsEvidence",
        ],
        "owner recovery",
    )?;
    if value["version"] != 1
        || value["kind"] != "duplicate_permit_owner_recovery"
        || value["approved"] != true
    {
        return Err("owner recovery is not an explicitly approved protocol-1 decision".into());
    }
    bounded_text(&value["owner"], "owner identity", 200)?;
    bounded_text(&value["reason"], "owner recovery reason", 1024)?;
    exact_fields(
        &value["invocation"],
        &["sameExactInvocation", "evidence"],
        "invocation attestation",
    )?;
    if value["invocation"]["sameExactInvocation"] != true {
        return Err("owner recovery lacks the explicit exact-invocation attestation".into());
    }
    bounded_text(
        &value["invocation"]["evidence"],
        "invocation evidence",
        4096,
    )?;
    let predecessor = predecessor_identity(
        binding.state,
        binding.events,
        binding.source,
        binding.old_ledger_path,
        binding.goal,
    )?;
    if value["predecessor"] != predecessor {
        return Err("owner recovery predecessor identity does not match the locked ledger".into());
    }
    validate_adjacent_duplicate(binding.state, binding.events, value)?;
    let identity = event_identity(
        binding.state,
        binding.events,
        &value["grantEventSha256"],
        binding.old_ledger_path,
    )?;
    if crate::work::action_forms::has_held_result(
        loaded,
        identity["work"].as_str().unwrap_or_default(),
        identity["assignment"].as_str().unwrap_or_default(),
    )? {
        return Err("owner recovery refused while a result is held for this assignment".into());
    }
    if value["identity"] != identity {
        return Err("owner recovery worker identity does not match both recorded events".into());
    }
    if !valid_sha(value["productSnapshotSha256"].as_str())
        || value["productSnapshotSha256"] != value["identity"]["inputsSha256"]
    {
        return Err("owner recovery product snapshot digest is invalid".into());
    }
    if !valid_sha(value["currentConfigSha256"].as_str())
        || value["currentConfigSha256"] != binding.successor_config_sha256
    {
        return Err(
            "owner recovery current configuration identity does not match successor".into(),
        );
    }
    validate_effects(&effects.value)?;
    let evidence = descriptor(&value["effectsEvidence"], "effects evidence")?;
    if evidence != effects.descriptor {
        return Err("owner recovery effects evidence descriptor changed".into());
    }
    Ok(json!({
        "protocol": PROTOCOL,
        "decision": decision.descriptor,
        "effectsEvidence": evidence,
        "ownerDecisionSha256": crate::evidence::hash::value(&json!({
            "owner": value["owner"],
            "approved": value["approved"],
            "reason": value["reason"],
            "invocation": value["invocation"],
        })),
        "predecessor": predecessor,
        "currentConfigSha256": value["currentConfigSha256"],
        "grantEventSha256": value["grantEventSha256"],
        "grantGovernorEventSha256": value["grantGovernorEventSha256"],
        "blockedEventSha256": value["blockedEventSha256"],
        "blockedGovernorEventSha256": value["blockedGovernorEventSha256"],
        "identity": identity,
        "productSnapshotSha256": value["productSnapshotSha256"],
        "effectInventory": effects.value["inventory"],
    }))
}

fn validate_effects(value: &Value) -> Result<(), String> {
    exact_fields(
        value,
        &["version", "complete", "inventory", "ownerAttestation"],
        "effects evidence",
    )?;
    if value["version"] != 1 || value["complete"] != true {
        return Err("owner recovery effects evidence is incomplete".into());
    }
    exact_fields(
        &value["inventory"],
        &[
            "productWrites",
            "processStarts",
            "networkWrites",
            "otherExternal",
        ],
        "effect inventory",
    )?;
    for key in [
        "productWrites",
        "processStarts",
        "networkWrites",
        "otherExternal",
    ] {
        if value["inventory"][key] != "none" {
            return Err(format!("owner recovery has unknown or observed {key}"));
        }
    }
    bounded_text(
        &value["ownerAttestation"],
        "effects owner attestation",
        4096,
    )
}

fn predecessor_identity(
    state: &Value,
    events: &[Value],
    source: &str,
    old_ledger_path: &str,
    goal: &str,
) -> Result<Value, String> {
    let head = events.last().ok_or("blocked predecessor has no events")?;
    let goal_sha = crate::evidence::hash::text(goal);
    if old_ledger_path.is_empty()
        || !matches!(state["status"].as_str(), Some("running" | "blocked"))
        || state["governor"]["enabled"] != true
        || state["governor"]["state"] != "blocked"
        || state["subject"]["goalSha256"] != goal_sha
        || crate::run::state::check_observation::unresolved(state)
        || state["held"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        || state["heldResults"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    {
        return Err("owner recovery requires the exact blocked same-goal predecessor".into());
    }
    Ok(json!({
        "ledgerPath": old_ledger_path,
        "ledgerSha256": crate::evidence::hash::text(source),
        "runId": state["runId"],
        "headEventSha256": head["eventSha256"],
        "configSha256": state["configSha256"],
    }))
}

fn validate_adjacent_duplicate(
    state: &Value,
    events: &[Value],
    decision: &Value,
) -> Result<(), String> {
    if events.len() < 3
        || !matches!(state["status"].as_str(), Some("running" | "blocked"))
        || state["governor"]["state"] != "blocked"
    {
        return Err("owner recovery requires a terminal blocked predecessor".into());
    }
    let grant_index = events
        .iter()
        .position(|event| event["eventSha256"] == decision["grantEventSha256"])
        .ok_or("owner recovery grant event is absent from the predecessor")?;
    let blocked_index = events
        .iter()
        .position(|event| event["eventSha256"] == decision["blockedEventSha256"])
        .ok_or("owner recovery refusal event is absent from the predecessor")?;
    if blocked_index != grant_index + 1 || blocked_index + 1 != events.len() {
        return Err("owner recovery grant and terminal refusal must be adjacent and last".into());
    }
    let grant = &events[grant_index];
    let blocked = &events[blocked_index];
    let grant_gov = &grant["governorEvent"];
    let blocked_gov = &blocked["governorEvent"];
    let request_identity_matches = ["requestId", "requestDigest"]
        .iter()
        .all(|field| grant.get(*field) == blocked.get(*field))
        && grant_gov.get("requestId") == blocked_gov.get("requestId")
        && grant_gov.get("requestDigest") == blocked_gov.get("requestDigest")
        && grant.get("requestId") == grant_gov.get("requestId")
        && grant.get("requestDigest") == grant_gov.get("requestDigest")
        && blocked.get("requestId") == blocked_gov.get("requestId")
        && blocked.get("requestDigest") == blocked_gov.get("requestDigest");
    let checkpoint_pair_matches = grant_gov["checkpoint"]
        .as_u64()
        .zip(blocked_gov["checkpoint"].as_u64())
        .is_some_and(|(grant, blocked)| grant.checked_add(1) == Some(blocked));
    let blocked_checkpoint_matches_state = state["governor"]["spent"]
        .as_u64()
        .zip(blocked_gov["checkpoint"].as_u64())
        .is_some_and(|(spent, blocked)| spent.checked_add(1) == Some(blocked));
    if grant["action"] != "govern"
        || grant_gov["action"] != "mutation"
        || blocked["action"] != "govern"
        || blocked_gov["action"] != "blocked"
        || grant_gov["eventSha256"] != decision["grantGovernorEventSha256"]
        || blocked_gov["eventSha256"] != decision["blockedGovernorEventSha256"]
        || grant["operation"] != blocked["operation"]
        || grant_gov["operation"] != grant["operation"]
        || grant["runId"] != blocked["runId"]
        || grant["stage"] != blocked["stage"]
        || grant["attempt"] != blocked["attempt"]
        || grant["agent"] != blocked["agent"]
        || grant["role"] != "worker"
        || blocked["role"] != "worker"
        || grant["subjectSha256"] != blocked["subjectSha256"]
        || grant["assignmentSha256"] != blocked["assignmentSha256"]
        || grant["inputsSha256"] != blocked["inputsSha256"]
        || grant_gov["runId"] != blocked_gov["runId"]
        || grant_gov["subjectSha256"] != blocked_gov["subjectSha256"]
        || grant_gov["attempt"] != blocked_gov["attempt"]
        || grant_gov["inputSha256"] != blocked_gov["inputSha256"]
        || !checkpoint_pair_matches
        || !blocked_checkpoint_matches_state
        || blocked_gov["previousSha256"] != grant_gov["eventSha256"]
        || grant_gov["unit"] != "worker-mutation"
        || !request_identity_matches
        || blocked_gov["reason"] != "new exact evidence is required"
        || state["governor"]["headSha256"] != blocked_gov["eventSha256"]
        || state["governor"]["currentMutation"]["eventSha256"] != grant_gov["eventSha256"]
        || state["governor"]["consumedGrants"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == &grant["eventSha256"]))
    {
        return Err(
            "owner recovery events do not form the exact duplicate-permit terminal block".into(),
        );
    }
    if events[grant_index + 1..]
        .iter()
        .any(|event| event["action"] != "govern")
    {
        return Err("owner recovery predecessor contains an unresolved non-governor event".into());
    }
    Ok(())
}

fn event_identity(
    state: &Value,
    events: &[Value],
    grant_sha: &Value,
    old_ledger_path: &str,
) -> Result<Value, String> {
    let grant = events
        .iter()
        .find(|event| &event["eventSha256"] == grant_sha)
        .ok_or("owner recovery identity has no grant event")?;
    let work = crate::work::locator_for_ledger(old_ledger_path)
        .ok_or("owner recovery predecessor has no canonical Work locator")?;
    let assignment = crate::run::assignment::pending(state)
        .into_iter()
        .find(|item| item["agent"] == grant["agent"])
        .ok_or("owner recovery has no current worker assignment")?;
    if assignment["role"] != "worker"
        || assignment["stage"] != grant["stage"]
        || assignment["attempt"] != grant["attempt"]
    {
        return Err("owner recovery event is not the exact current worker assignment".into());
    }
    let assignment_handle = crate::run::assignment::handle(&work, &assignment)?;
    if crate::evidence::hash::text(&assignment_handle) != grant["assignmentSha256"] {
        return Err(
            "owner recovery assignment hash does not match the canonical pending worker".into(),
        );
    }
    Ok(json!({
        "work": work,
        "assignment": assignment_handle,
        "runId": grant["runId"],
        "stage": grant["stage"],
        "attempt": grant["attempt"],
        "agent": grant["agent"],
        "role": grant["role"],
        "subjectSha256": grant["subjectSha256"],
        "assignmentSha256": grant["assignmentSha256"],
        "inputsSha256": grant["inputsSha256"],
        "operation": grant["operation"],
    }))
}

fn descriptor(value: &Value, label: &str) -> Result<Value, String> {
    exact_fields(value, &["root", "path", "sha256", "bytes"], label)?;
    if value["root"] != "state"
        || value["path"].as_str().is_none()
        || !valid_sha(value["sha256"].as_str())
        || value["bytes"].as_u64().is_none()
    {
        return Err(format!("{label} artifact descriptor is invalid"));
    }
    Ok(value.clone())
}

fn exact_fields(value: &Value, fields: &[&str], label: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!("{label} has missing or unknown fields"));
    }
    Ok(())
}

fn bounded_text(value: &Value, label: &str, max: usize) -> Result<(), String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("{label} must be text"))?;
    if text.trim().is_empty() || text.len() > max || text.contains('\0') {
        return Err(format!("{label} is empty or exceeds {max} bytes"));
    }
    Ok(())
}

fn valid_sha(value: Option<&str>) -> bool {
    value.is_some_and(|sha| {
        sha.len() == 64
            && sha
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
