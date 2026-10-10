//! Explicit owner decisions for one project's otherwise incompatible recall policy.
//! This route never treats administrative authorization as agent compatibility.
use crate::{
    config::{self, Loaded},
    evidence::hash,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ledger::*;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerDecision {
    version: u64,
    approved: bool,
    owner: String,
    project_identity: String,
    item_id: String,
    previous_config_sha256: String,
    current_config_sha256: String,
    prior_ledger_head_sha256: String,
    differences: Vec<Value>,
    reason: String,
}

pub(crate) fn run(
    loaded: &Loaded,
    ledger: &str,
    from: &str,
    reason: &str,
    decision_path: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    if reason.trim().is_empty() || reason.len() > 1024 {
        return Err(
            "policy correction requires a nonempty owner reason of at most 1024 bytes".into(),
        );
    }
    if apply && decision_path.is_none() {
        return Err("policy correction requires --owner-decision with the explicit approved exact-item decision; no change was made".into());
    }
    super::ensure_config_current(loaded)?;
    let _lock = super::lessons::mutation_lock(loaded)?;
    let previous = config::load(Some(from))?;
    let snapshot = read_ledger(loaded, ledger, false)?;
    let supplied = decision_path
        .map(|path| {
            if std::fs::symlink_metadata(path)
                .map_err(|e| e.to_string())?
                .len()
                > 16 * 1024
            {
                return Err("owner decision exceeds 16384 bytes".into());
            }
            let bytes = stable_bytes(std::path::Path::new(path), "owner decision")?;
            if bytes.len() > 16 * 1024 {
                return Err("owner decision exceeds 16384 bytes".into());
            }
            serde_json::from_slice::<OwnerDecision>(&bytes)
                .map_err(|e| format!("invalid owner decision: {e}"))
        })
        .transpose()?;

    // Recover the recorded response after a lost reply, before testing old
    // currentness. A different head, decision, reason or configuration refuses.
    if let Some(last) = snapshot.events.last() {
        if last["action"] == "correct-policy" {
            if let Some(decision) = &supplied {
                let proof = serde_json::to_value(decision).map_err(|e| e.to_string())?;
                if apply
                    && last["policyCorrection"] == proof
                    && decision.reason == reason
                    && decision.previous_config_sha256 == hash::text(&previous.source)
                    && decision.current_config_sha256 == hash::text(&loaded.source)
                {
                    super::validate_history_current(loaded, &snapshot)?;
                    super::validate_source_current(loaded, &snapshot)?;
                    differences(&previous, loaded)?;
                    super::ensure_config_current(&previous)?;
                    return response(loaded, ledger, last, "existing_verified");
                }
            }
        }
    }
    super::validate_history_current(&previous, &snapshot)?;
    super::validate_source_current(&previous, &snapshot)?;
    let item = snapshot
        .items
        .values()
        .next()
        .ok_or("lesson ledger is empty")?;
    if item["scope"] != super::lessons::SCOPE || item["state"] != "accepted" {
        return Err("policy correction is limited to one accepted project lesson".into());
    }
    let owner = loaded.lead().ok_or("configured Lead missing")?;
    let expected = OwnerDecision {
        version: 1,
        approved: true,
        owner: owner.to_owned(),
        project_identity: crate::host::assignment_context::project_identity(loaded)?,
        item_id: item["itemId"].as_str().ok_or("item ID missing")?.into(),
        previous_config_sha256: hash::text(&previous.source),
        current_config_sha256: hash::text(&loaded.source),
        prior_ledger_head_sha256: snapshot
            .last_event_sha256
            .clone()
            .ok_or("lesson head missing")?,
        differences: differences(&previous, loaded)?,
        reason: reason.into(),
    };
    let mut template = serde_json::to_value(&expected).map_err(|e| e.to_string())?;
    template["approved"] = json!(false);
    if !apply {
        let command = vec![
            std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("executable is not UTF-8")?
                .to_owned(),
            "memory".into(),
            "correct-policy".into(),
            ledger.into(),
            "--from-config".into(),
            previous
                .path
                .to_str()
                .ok_or("prior configuration is not UTF-8")?
                .into(),
            "--reason".into(),
            reason.into(),
            "--owner-decision".into(),
            "<OWNER_DECISION>".into(),
            "--apply".into(),
            "--config".into(),
            loaded
                .path
                .to_str()
                .ok_or("configuration is not UTF-8")?
                .into(),
        ];
        return Ok(
            json!({"effect":"no-change","status":"preview","ownerDecision":template,
            "meaning":"The project owner must explicitly approve these exact policy differences. This is not compatible revalidation or a grant from the old agent.",
                "nextAction":{"command":command,"placeholders":["OWNER_DECISION"],"readOnly":false,
                    "summary":"Save the exact decision, record the owner's approval, then supply its path. Keep the prior configuration and Work readers."}}),
        );
    }
    if supplied.as_ref() != Some(&expected) {
        return Err("owner decision is unapproved, stale or does not match the exact project, item, configurations and predecessor head; no change was made".into());
    }
    let lead = loaded.agent(owner).ok_or("configured Lead missing")?;
    let profile = project_file(&loaded.control_root, &lead.profile, "owner profile")?;
    let mut event = json!({"version":3,"kind":"memory","producer":crate::producer::evidence(),
        "action":"correct-policy","itemId":item["itemId"],"scope":item["scope"],"actor":owner,
        "source":item["source"],"configSha256":hash::text(&loaded.source),
        "actorProfile":{"path":relative_project_path(&loaded.control_root,&profile)?,
            "sha256":hash::text(&stable_text(&profile,"owner profile")?)},
        "previousEventSha256":snapshot.last_event_sha256,"timestamp":super::now(),
        "policyCorrection":expected});
    if let Some(expiry) = item.get("expiresAt") {
        event["expiresAt"] = expiry.clone();
    }
    event["eventSha256"] = json!(hash::value(&event));
    super::state::validate_event(
        &event,
        &loaded.product_root,
        &loaded.control_root,
        snapshot.events.len() + 1,
        snapshot.last_event_sha256.as_deref(),
        snapshot.last_timestamp.as_deref(),
        &snapshot.items,
    )?;
    super::ensure_config_current(&previous)?;
    super::ensure_config_current(loaded)?;
    super::validate_source_current(loaded, &snapshot)?;
    crate::project::git_preflight::refuse_tracked_targets(
        &loaded.state_root,
        &[snapshot.path.as_path()],
    )?;
    append_event(&snapshot, &event)?;
    response(loaded, ledger, &event, "recorded")
}

fn differences(previous: &Loaded, current: &Loaded) -> Result<Vec<Value>, String> {
    if previous.product_root != current.product_root
        || previous.control_root != current.control_root
        || previous.state_root != current.state_root
        || previous.project_id != current.project_id
        || previous.lead() != current.lead()
    {
        return Err("policy correction cannot change project roots, identity or ownership".into());
    }
    let owner = current.lead().ok_or("configured Lead missing")?;
    let old_lead = previous.agent(owner).ok_or("prior Lead missing")?;
    let new_lead = current.agent(owner).ok_or("Lead missing")?;
    for right in ["memoryWrite", "memoryReview", "memoryPromote"] {
        if !super::authorized(&old_lead.boundary_value(), right, super::lessons::SCOPE) {
            return Err(format!("prior Lead lacks project lesson right {right}"));
        }
    }
    if !super::authorized(
        &new_lead.boundary_value(),
        "memoryRevoke",
        super::lessons::SCOPE,
    ) {
        return Err("corrected Lead must be able to retire this lesson".into());
    }
    let mut expected: Value = serde_json::from_str(&previous.source).map_err(|e| e.to_string())?;
    let actual: Value = serde_json::from_str(&current.source).map_err(|e| e.to_string())?;
    let mut changes = Vec::new();
    for (name, old) in &previous.agents {
        let new = current.agent(name).ok_or("recipient removed")?;
        for (field, before, after) in [
            ("memoryRead", json!(old.memory_read), json!(new.memory_read)),
            (
                "memoryRevoke",
                json!(old.memory_revoke),
                json!(new.memory_revoke),
            ),
            (
                "crossContext",
                json!(old.cross_context),
                json!(new.cross_context),
            ),
        ] {
            if before == after {
                continue;
            }
            let allowed = match field {
                "memoryRead" => additive_scope(&old.memory_read, &new.memory_read),
                "memoryRevoke" => {
                    name == owner && additive_scope(&old.memory_revoke, &new.memory_revoke)
                }
                "crossContext" => {
                    old.cross_context == "none"
                        && new.cross_context == "protocol-only"
                        && new.memory_read == [super::lessons::SCOPE]
                }
                _ => false,
            };
            if !allowed {
                return Err("policy correction permits only project-lesson read access, none-to-protocol-only selection and the owner's lesson revoke right".into());
            }
            expected["agents"][name][field] = after.clone();
            changes.push(json!({"agent":name,"field":field,"before":before,"after":after}));
        }
    }
    if expected != actual || changes.is_empty() {
        return Err("policy correction contains an unrelated or unchanged configuration; preserve all other configuration bytes and meanings".into());
    }
    Ok(changes)
}

fn additive_scope(before: &[String], after: &[String]) -> bool {
    let mut expected = before.to_vec();
    if expected.iter().any(|s| s == super::lessons::SCOPE) {
        return false;
    }
    expected.push(super::lessons::SCOPE.into());
    let mut actual = after.to_vec();
    expected.sort();
    actual.sort();
    expected == actual
}

pub(crate) fn validate_evidence(event: &Value, current: &Value) -> Result<(), String> {
    let decision: OwnerDecision = serde_json::from_value(event["policyCorrection"].clone())
        .map_err(|_| "invalid owner-directed policy correction")?;
    if decision.version != 1
        || !decision.approved
        || decision.owner != event["actor"]
        || decision.item_id != event["itemId"]
        || decision.previous_config_sha256 != current["configSha256"]
        || decision.current_config_sha256 != event["configSha256"]
        || decision.prior_ledger_head_sha256 != event["previousEventSha256"]
        || decision.project_identity.is_empty()
        || decision.differences.is_empty()
        || decision.reason.trim().is_empty()
        || decision.reason.len() > 1024
        || event["scope"] != super::lessons::SCOPE
        || current["state"] != "accepted"
    {
        return Err("invalid exact-item owner policy correction".into());
    }
    Ok(())
}

fn response(loaded: &Loaded, ledger: &str, event: &Value, status: &str) -> Result<Value, String> {
    Ok(
        json!({"effect":if status=="recorded" {"recorded"} else {"no-change"},
        "status":status,"itemId":event["itemId"],"event":event,
        "nextAction":super::revalidation::next_action(loaded, loaded.lead().ok_or("Lead missing")?, ledger, "accepted")?,
        "activation":{"status":"requires_recipient_delivery_check",
            "meaning":"A recorded correction is not completed activation. Use this corrected configuration for the intended next task and verify the original item in each intended recipient's relevant context. Empty required selection leaves activation unfinished."},
        "meaning":"Only this immutable project lesson's policy lineage changed. Source, expiry and predecessor history are preserved; no Work, grant, check, review or acceptance is transferred."}),
    )
}
