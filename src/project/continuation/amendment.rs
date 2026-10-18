//! Explicit time-only authorization for an existing frozen Work association.
//! Selection alone never changes a claim; this separate receipt is required.
use super::{control, journal, sources};
use crate::{config::Loaded, evidence::hash, run::ledger};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u64,
    project_identity: String,
    configuration_sha256: String,
    mandate_id: String,
    work: String,
    task_id: String,
    start_intent_sha256: String,
    original_association_sha256: String,
    original_specification_sha256: String,
    selected_specification_sha256: String,
    original_revision: u64,
    selected_revision: u64,
    original_expires_at: String,
    selected_expires_at: String,
    original_max_elapsed_seconds: u64,
    selected_max_elapsed_seconds: u64,
    selection_event_sha256: String,
    preview_head_sha256: String,
    owner: String,
    request_id: String,
    reason: String,
    decision_sha256: String,
}

fn receipt_path(_loaded: &Loaded, claim: &journal::Claim) -> Result<String, String> {
    Ok(format!(
        "{}/artifacts/project-continuation-amendments/{}.json",
        crate::project::layout_types::state_namespace(),
        hash::value(
            &json!({"project":claim.association.project,"claimRoot":claim.association.claim_root,
            "task":claim.task_id,"startIntent":claim.start_intent_sha256})
        )
    ))
}

fn read_receipt(loaded: &Loaded, claim: &journal::Claim) -> Result<Option<Receipt>, String> {
    let relative = receipt_path(loaded, claim)?;
    match crate::project::path::secure_bytes_observation_bounded(
        &loaded.state_root,
        &relative,
        "continuation time amendment",
        64 * 1024,
    ) {
        crate::project::path::SecureBytesResult::Absent(_) => Ok(None),
        crate::project::path::SecureBytesResult::Bytes(bytes) => {
            let receipt: Receipt = serde_json::from_slice(&bytes)
                .map_err(|_| "time amendment receipt is malformed")?;
            validate_receipt(&receipt)?;
            Ok(Some(receipt))
        }
        _ => Err("time amendment receipt is unavailable or unsafe".into()),
    }
}

fn validate_receipt(receipt: &Receipt) -> Result<(), String> {
    let mut body = serde_json::to_value(receipt).map_err(|e| e.to_string())?;
    body.as_object_mut()
        .ok_or("invalid amendment receipt")?
        .remove("decisionSha256");
    if receipt.version != 1
        || receipt.project_identity.trim().is_empty()
        || receipt.configuration_sha256.len() != 64
        || receipt.mandate_id.trim().is_empty()
        || !crate::work::valid_work_handle(&receipt.work)
        || receipt.task_id.trim().is_empty()
        || receipt.start_intent_sha256.len() != 64
        || receipt.original_association_sha256.len() != 64
        || receipt.original_specification_sha256.len() != 64
        || receipt.selected_specification_sha256.len() != 64
        || receipt.selected_revision <= receipt.original_revision
        || receipt.selected_max_elapsed_seconds <= receipt.original_max_elapsed_seconds
        || receipt.selection_event_sha256.len() != 64
        || receipt.preview_head_sha256.len() != 64
        || receipt.owner.trim().is_empty()
        || receipt.request_id.trim().is_empty()
        || receipt.reason.trim().is_empty()
        || receipt.decision_sha256 != hash::value(&body)
    {
        return Err("time amendment receipt failed strict validation".into());
    }
    let original = chrono::DateTime::parse_from_rfc3339(&receipt.original_expires_at)
        .map_err(|_| "invalid original time amendment expiry")?;
    let selected = chrono::DateTime::parse_from_rfc3339(&receipt.selected_expires_at)
        .map_err(|_| "invalid selected time amendment expiry")?;
    if selected <= original {
        return Err("time amendment must extend the existing expiry".into());
    }
    Ok(())
}

fn historic_spec(raw: &str, digest: &str) -> Result<journal::Record, String> {
    raw.lines()
        .map(serde_json::from_str::<journal::Record>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|record| record.specification_sha256 == digest)
        .ok_or_else(|| {
            "original associated specification is absent from retained journal history".into()
        })
}

fn compare_time_only(original: &journal::Record, selected: &journal::Record) -> Result<(), String> {
    let old = &original.specification;
    let new = &selected.specification;
    let mut normalized = new.clone();
    normalized.mandate.revision = old.mandate.revision;
    normalized.mandate.expires_at = old.mandate.expires_at.clone();
    normalized.mandate.max_elapsed_seconds = old.mandate.max_elapsed_seconds;
    let old_expiry = DateTime::parse_from_rfc3339(&old.mandate.expires_at)
        .map_err(|_| "invalid original mandate expiry")?;
    let new_expiry = DateTime::parse_from_rfc3339(&new.mandate.expires_at)
        .map_err(|_| "invalid selected mandate expiry")?;
    if normalized != *old
        || new.mandate.revision <= old.mandate.revision
        || new_expiry <= old_expiry
        || new.mandate.max_elapsed_seconds <= old.mandate.max_elapsed_seconds
    {
        return Err("selected revision is not a strictly time-only extension of this Work's original mandate".into());
    }
    Ok(())
}

fn facts(
    loaded: &Loaded,
    work: &str,
) -> Result<(String, journal::Record, journal::Claim, Value, String), String> {
    crate::work::details::ensure_current_config(loaded)?;
    let work_path = crate::work::resolve(loaded, work)?;
    let (_, events, work_bytes) = ledger::load(loaded, &work_path)?;
    let start = events.first().ok_or("associated Work start is missing")?;
    let work_head = events.last().ok_or("associated Work has no event head")?;
    let frozen: control::Association = serde_json::from_value(start["projectContinuation"].clone())
        .map_err(|_| "Work is not bound to a continuation claim")?;
    let relative = journal::relative(loaded, &frozen.mandate_id)?;
    let (record, raw) = journal::read(loaded, &relative)?;
    let record = record.ok_or("mandate has not been selected")?;
    let (selected_spec, selected_sha) = sources::read(loaded, &record.specification_path)?;
    if record.specification_sha256 != selected_sha
        || selected_spec != record.specification
        || record.stopped
        || record.configuration_sha256 != hash::text(&loaded.source)
    {
        return Err(
            "current selected mandate is stale, stopped, or differs from its source".into(),
        );
    }
    let claim = record
        .claims
        .iter()
        .find(|claim| claim.work == work)
        .cloned()
        .ok_or("Work has no exact selected continuation claim")?;
    let old_record = historic_spec(&raw, &claim.association.specification_sha256)?;
    compare_time_only(&old_record, &record)?;
    let state = crate::run::reduce_live(loaded, &events)?;
    if start["projectContinuation"]
        != serde_json::to_value(&claim.association).map_err(|e| e.to_string())?
        || crate::run::start_event_intent_sha256(start)? != claim.start_intent_sha256
        || !crate::work::action::continuation_native_facts(loaded, work, &state)?.all_ended
        || record.account.operations.iter().any(|operation| {
            operation.association == claim.association
                && operation.intent.work == work
                && matches!(
                    operation.outcome,
                    control::Outcome::Reserved | control::Outcome::Unknown
                )
        })
        || crate::run::assert_no_drift(loaded, &state).is_err()
        || crate::memory::contextual::assert_plan(loaded, &state).is_err()
    {
        return Err("time amendment is held by changed Work identity, live/unknown effects, or source drift".into());
    }
    let owner = loaded
        .lead()
        .ok_or("configured Lead is missing")?
        .to_owned();
    let binding = hash::value(&json!({"work":work,"claim":claim.association,
        "startIntent":claim.start_intent_sha256,"originalSpec":old_record.specification_sha256,
        "selectedSpec":record.specification_sha256,"selectionEvent":record.event_sha256,
        "journalHead":record.event_sha256,"configuration":hash::text(&loaded.source),
        "owner":owner,"usage":record.account,
        "workHeadSha256":work_head["eventSha256"],
        "workLedgerSha256":hash::bytes(work_bytes.as_bytes())}));
    Ok((relative, record, claim, state, binding))
}

pub(crate) fn amend(
    loaded: &Loaded,
    work: &str,
    reason: &str,
    request_id: &str,
    expected: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    if reason.trim().is_empty() || reason.len() > 4096 || reason.contains('\0') {
        return Err("--reason must be non-empty and at most 4096 bytes".into());
    }
    if request_id.trim().is_empty() || request_id.len() > 128 || request_id.contains('\0') {
        return Err("--request-id must be non-empty and at most 128 bytes".into());
    }
    let (relative, record, claim, _state, base_binding) = facts(loaded, work)?;
    let binding = hash::value(&json!({"base":base_binding,"reason":reason,"requestId":request_id}));
    if let Some(receipt) = read_receipt(loaded, &claim)? {
        if receipt.request_id == request_id && receipt.reason == reason {
            return Ok(json!({"state":"amended","effect":"replayed","work":work,
                "decisionSha256":receipt.decision_sha256,"selectedRevision":receipt.selected_revision,
                "originalClockPreserved":true,"usagePreserved":true}));
        }
        return Err(
            "this Work's time amendment is already reserved; changed requests cannot mint another"
                .into(),
        );
    }
    let apply_argv = argv(loaded, work, reason, request_id, &binding)?;
    let mut response = json!({"version":1,"state":"preview","effect":"no-change","readOnly":true,
        "work":work,"originalRevision":claim.association.mandate_revision,
        "selectedRevision":record.specification.mandate.revision,"currentBinding":binding,
        "originalClock":record.account.started_at,"actionsSpent":record.account.actions_spent(),
        "worksSpent":record.account.works_spent(),"apply":{"command":apply_argv},
        "authority":{"fields":["revision","expiresAt","maxElapsedSeconds"],"sourceAndAllOtherMandateFieldsUnchanged":true}});
    if !apply {
        return Ok(response);
    }
    if expected != Some(binding.as_str()) {
        return Err("time amendment preview is stale; no receipt was written".into());
    }
    let selected = journal::prepare(loaded, &relative)?;
    let work_relative = crate::work::resolve(loaded, work)?;
    let work_lock = ledger::ledger_path(&loaded.state_root, &work_relative, true)?;
    ledger::with_lock(&work_lock, || {
        ledger::with_lock(&selected, || {
            // Serialize revalidation and receipt creation in the established
            // Work-lock then account-lock order used by controlled effects.
            let (_, fresh, _, _, fresh_base_binding) = facts(loaded, work)?;
            let fresh_binding = hash::value(
                &json!({"base":fresh_base_binding,"reason":reason,"requestId":request_id}),
            );
            if fresh.event_sha256 != record.event_sha256 || fresh_binding != binding {
                return Err("time amendment current Work head, ledger, or mandate binding changed; prepare again".into());
            }
            if let Some(existing) = read_receipt(loaded, &claim)? {
                if existing.request_id == request_id && existing.reason == reason {
                    response["state"] = json!("amended");
                    response["effect"] = json!("replayed");
                    response["decisionSha256"] = json!(existing.decision_sha256);
                    return Ok(response);
                }
                return Err("a conflicting time amendment receipt already exists".into());
            }
            let relative_receipt = receipt_path(loaded, &claim)?;
            let full_receipt = loaded.state_root.join(relative_receipt);
            crate::project::managed_files::ensure_state_directory(
                &loaded.state_root,
                full_receipt
                    .parent()
                    .ok_or("amendment receipt has no parent")?,
            )?;
            let owner = loaded
                .lead()
                .ok_or("configured Lead is missing")?
                .to_owned();
            let mut receipt = Receipt {
                version: 1,
                project_identity: fresh.project_identity.clone(),
                configuration_sha256: hash::text(&loaded.source),
                mandate_id: fresh.specification.mandate.id.clone(),
                work: work.into(),
                task_id: claim.task_id.clone(),
                start_intent_sha256: claim.start_intent_sha256.clone(),
                original_association_sha256: hash::value(
                    &serde_json::to_value(&claim.association).map_err(|error| error.to_string())?,
                ),
                original_specification_sha256: claim.association.specification_sha256.clone(),
                selected_specification_sha256: fresh.specification_sha256.clone(),
                original_revision: claim.association.mandate_revision,
                selected_revision: fresh.specification.mandate.revision,
                original_expires_at: historic_spec(
                    &journal::read(loaded, &relative)?.1,
                    &claim.association.specification_sha256,
                )?
                .specification
                .mandate
                .expires_at,
                selected_expires_at: fresh.specification.mandate.expires_at.clone(),
                original_max_elapsed_seconds: historic_spec(
                    &journal::read(loaded, &relative)?.1,
                    &claim.association.specification_sha256,
                )?
                .specification
                .mandate
                .max_elapsed_seconds,
                selected_max_elapsed_seconds: fresh.specification.mandate.max_elapsed_seconds,
                selection_event_sha256: fresh.event_sha256.clone(),
                preview_head_sha256: binding.clone(),
                owner,
                request_id: request_id.into(),
                reason: reason.into(),
                decision_sha256: String::new(),
            };
            let mut body = serde_json::to_value(&receipt).map_err(|e| e.to_string())?;
            body.as_object_mut().unwrap().remove("decisionSha256");
            receipt.decision_sha256 = hash::value(&body);
            let bytes = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
            crate::project::managed_files::write_state_exclusive(&full_receipt, &bytes)?;
            response["state"] = json!("amended");
            response["effect"] = json!("recorded");
            response["decisionSha256"] = json!(receipt.decision_sha256);
            response["originalClockPreserved"] = json!(true);
            response["usagePreserved"] = json!(true);
            Ok(response)
        })
    })
}

fn argv(
    loaded: &Loaded,
    work: &str,
    reason: &str,
    request: &str,
    binding: &str,
) -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(vec![
        exe.to_str().ok_or("executable path is not UTF-8")?.into(),
        "project".into(),
        "advance".into(),
        "amend".into(),
        work.into(),
        "--reason".into(),
        reason.into(),
        "--request-id".into(),
        request.into(),
        "--current-binding".into(),
        binding.into(),
        "--apply".into(),
        "--config".into(),
        loaded
            .path
            .to_str()
            .ok_or("configuration path is not UTF-8")?
            .into(),
        "--json".into(),
    ])
}

pub(super) fn held_action(
    loaded: &Loaded,
    work: &str,
    state: &Value,
    binding: &str,
) -> Result<Option<Value>, String> {
    let Some(frozen) = state.get("projectContinuation") else {
        return Ok(None);
    };
    let marker: control::Association = serde_json::from_value(frozen.clone())
        .map_err(|_| "invalid frozen continuation association")?;
    let relative = journal::relative(loaded, &marker.mandate_id)?;
    let (record, _) = journal::read(loaded, &relative)?;
    let Some(record) = record else {
        return Ok(None);
    };
    if marker.mandate_revision == record.specification.mandate.revision
        && marker.specification_sha256 == record.specification_sha256
    {
        return Ok(None);
    }
    let claim = record.claims.iter().find(|claim| claim.work == work);
    let Some(claim) = claim else { return Ok(None) };
    if read_receipt(loaded, claim)?.is_some() {
        return Ok(None);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let config = loaded
        .path
        .to_str()
        .ok_or("configuration path is not UTF-8")?;
    Ok(Some(json!({
        "version":1,"state":"held_time_amendment_required","binding":binding,
        "leadChoiceRequired":true,"choices":[],"mechanicalAction":Value::Null,
        "meaning":"The selected mandate revision differs from this frozen Work association. Selection alone does not rebind it. A configured Lead may inspect a time-only amendment preview; no Work action is currently available.",
        "diagnostic":{"readOnly":true,"command":[exe.to_str().ok_or("executable path is not UTF-8")?,
            "project","advance","amend",work,"--reason","<REASON>","--request-id","<REQUEST_ID>",
            "--config",config,"--json"],"placeholders":["REASON","REQUEST_ID"]}
    })))
}

pub(super) fn effective_association(
    loaded: &Loaded,
    record: &journal::Record,
    claim: &journal::Claim,
    work: &str,
    frozen: &control::VerifiedAssociation,
) -> Result<control::VerifiedAssociation, String> {
    if claim.association.mandate_revision == record.specification.mandate.revision
        && claim.association.specification_sha256 == record.specification_sha256
    {
        return Ok(frozen.clone());
    }
    let relative = journal::relative(loaded, &record.specification.mandate.id)?;
    let (latest, raw) = journal::read(loaded, &relative)?;
    let latest = latest.ok_or("selected mandate journal disappeared")?;
    let historical_selection = raw.lines().any(|line| {
        serde_json::from_str::<journal::Record>(line).is_ok_and(|entry| {
            entry.event_sha256 == record.event_sha256
                && entry.specification_sha256 == record.specification_sha256
        })
    });
    if latest.event_sha256 != record.event_sha256 || !historical_selection {
        return Err("selected mandate changed while checking its amendment".into());
    }
    let receipt = read_receipt(loaded, claim)?.ok_or(
        "project continuation held: selected mandate changed; inspect project advance amend",
    )?;
    let path = crate::work::resolve(loaded, work)?;
    let (_, events, _) = ledger::load(loaded, &path)?;
    let start = events.first().ok_or("associated Work start is missing")?;
    if receipt.work != work
        || receipt.project_identity != record.project_identity
        || receipt.mandate_id != record.specification.mandate.id
        || receipt.configuration_sha256 != hash::text(&loaded.source)
        || receipt.original_association_sha256
            != hash::value(
                &serde_json::to_value(&claim.association).map_err(|error| error.to_string())?,
            )
        || receipt.original_specification_sha256 != claim.association.specification_sha256
        || receipt.selected_specification_sha256 != record.specification_sha256
        || receipt.selected_revision != record.specification.mandate.revision
        || !raw.lines().any(|line| {
            serde_json::from_str::<journal::Record>(line).is_ok_and(|entry| {
                entry.event_sha256 == receipt.selection_event_sha256
                    && entry.specification_sha256 == receipt.selected_specification_sha256
            })
        })
        || receipt.start_intent_sha256 != claim.start_intent_sha256
        || start["runId"].as_str().is_none()
        || crate::run::start_event_intent_sha256(start)? != claim.start_intent_sha256
    {
        return Err(
            "project continuation time amendment is stale or does not match this frozen claim"
                .into(),
        );
    }
    let original = historic_spec(&raw, &claim.association.specification_sha256)?;
    compare_time_only(&original, record)?;
    let (selected_spec, digest) = sources::read(loaded, &record.specification_path)?;
    if digest != receipt.selected_specification_sha256 || selected_spec != record.specification {
        return Err("time amendment source changed after explicit owner selection".into());
    }
    frozen
        .with_selected_time_authority(
            receipt.selected_revision,
            receipt.selected_specification_sha256,
        )
        .map_err(|reason| format!("invalid amended association: {reason:?}"))
}
