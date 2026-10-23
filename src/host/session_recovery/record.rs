use super::{
    hint::{
        bounded_text, canonical_path, checkpoint_is_ambiguous_older, current_executable_sha256,
        read_hint, valid_sha, validate_hint, write_hint_locked, Checkpoint, Hint, PendingOperation,
        Reference, WorkRef, WorkSelection, HOOK_BUDGET, MAX_ITEMS, MAX_TEXT,
    },
    lock::with_record_lock,
    references::{known_work_locator, verify_pending, verify_source_refs, verify_work_refs},
};
use crate::{config, evidence::hash, host::codegraph, session_goal};
use serde_json::{json, Value};
use std::time::Instant;

/// Persist an explicitly supplied, current owner-validated navigation hint.
/// The payload is not authority and every referenced Work still needs detail.
pub(crate) fn record(owner_loaded: &config::Loaded, bytes: &[u8]) -> Result<Value, String> {
    let session = session_goal::native::current()?
        .ok_or("session recovery record requires a host-reported native session")?;
    with_record_lock(&session.root, || record_locked(owner_loaded, bytes))
}

/// Store one generated graph artifact reference on the existing validated hint.
/// This is evidence navigation only; it cannot select Work or change corrections
/// or pending action, and it never enters guidance_refs.
pub(crate) fn attach_graph_result(
    work: &str,
    summary: &str,
    path: &str,
    sha256: &str,
) -> Result<bool, String> {
    let Some(session) = session_goal::native::current()? else {
        return Ok(false);
    };
    with_record_lock(&session.root, || {
        let owner = session.owner()?.ok_or("native Goal owner is unavailable")?;
        let record =
            session_goal::read_for_recovery(&session.root)?.ok_or("native Goal is unavailable")?;
        let Some(mut hint) = read_hint(&session.root)? else {
            return Ok(false);
        };
        if hint.host != session.host
            || hint.session_key != session.key
            || hint.owner_identity_sha256 != owner.identity_sha256
            || hint.owner_project_id != owner.project_id
            || hint.goal_id != record["goalId"]
            || hint.goal_revision != record["revision"].as_u64().unwrap_or(0)
            || hint.goal_event_sha256 != record["eventSha256"].as_str().unwrap_or_default()
            || !hint
                .work_refs
                .iter()
                .any(|reference| reference.work == work)
        {
            return Ok(false);
        }
        let owner_loaded = config::load(Some(&hint.owner_config_path))?;
        if canonical_path(&owner_loaded.path, "native owner configuration")?
            != hint.owner_config_path
            || hash::text(&owner_loaded.source) != hint.owner_config_sha256
        {
            return Ok(false);
        }
        let scope = session_goal::native::scope(&owner_loaded, false)?;
        if !scope.owner_matches
            || scope
                .owner
                .as_ref()
                .map(|value| value.identity_sha256.as_str())
                != Some(owner.identity_sha256.as_str())
        {
            return Ok(false);
        }
        verify_work_refs(&record, &session, &owner, &hint.work_refs)?;
        bounded_text(work, "graph result Work locator", 128)?;
        bounded_text(summary, "graph result summary", 128)?;
        bounded_text(path, "graph result path", 512)?;
        if !codegraph::valid_artifact_path(path) || !valid_sha(sha256) {
            return Err("generated graph result reference is malformed".into());
        }
        let reference = Reference {
            label: format!("CodeGraph: {summary}"),
            root: "state".into(),
            path: path.into(),
            work: Some(work.into()),
            sha256: sha256.into(),
            producer_executable_sha256: current_executable_sha256()?,
            consumption: "generated_query_result".into(),
        };
        hint.evidence_refs
            .retain(|item| item.consumption != "generated_query_result");
        if hint.evidence_refs.len() >= MAX_ITEMS {
            return Ok(false);
        }
        hint.evidence_refs.push(reference);
        validate_hint(&hint)?;
        hint.sha256.clear();
        hint.sha256 = hash::value(&serde_json::to_value(&hint).map_err(|error| error.to_string())?);
        write_hint_locked(&session, &hint)?;
        Ok(true)
    })
}

/// The session, its fixed Goal owner and canonical Goal a record is for,
/// checked against the owner configuration the caller supplied.
pub(super) struct Owned {
    pub(super) session: session_goal::native::Session,
    pub(super) owner: session_goal::native::Owner,
    pub(super) record: Value,
}

pub(super) fn owned(owner_loaded: &config::Loaded) -> Result<Owned, String> {
    let session = session_goal::native::current()?
        .ok_or("session recovery record requires a host-reported native session")?;
    let scope = session_goal::native::scope(owner_loaded, false)?;
    scope.require_owner()?;
    if scope
        .session
        .as_ref()
        .map_or(true, |current| current.key != session.key)
    {
        return Err("session recovery owner config belongs to another native session".into());
    }
    let owner = scope
        .owner
        .ok_or("native session Goal owner is unavailable")?;
    let record = session_goal::read_for_recovery(&session.root)?
        .ok_or("native session has no canonical Goal to recover")?;
    Ok(Owned {
        session,
        owner,
        record,
    })
}

/// The navigation one record carries.
pub(super) struct Draft {
    pub(super) current_step: String,
    pub(super) current_work: WorkSelection,
    pub(super) latest_corrections: Vec<String>,
    /// One entry per decision: the Work it was bound to when recorded.
    pub(super) latest_correction_works: Vec<Option<String>>,
    pub(super) evidence_refs: Vec<Reference>,
    pub(super) work_refs: Vec<WorkRef>,
    pub(super) pending_operation: Option<PendingOperation>,
    pub(super) guidance_refs: Vec<Reference>,
}

impl Draft {
    pub(super) fn of(hint: &Hint) -> Draft {
        Draft {
            current_step: hint.current_step.clone(),
            current_work: hint.current_work.clone(),
            latest_corrections: hint.latest_corrections.clone(),
            latest_correction_works: if hint.latest_correction_works.is_empty() {
                vec![None; hint.latest_corrections.len()]
            } else {
                hint.latest_correction_works.clone()
            },
            evidence_refs: hint.evidence_refs.clone(),
            work_refs: hint.work_refs.clone(),
            pending_operation: hint.pending_operation.clone(),
            guidance_refs: hint.guidance_refs.clone(),
        }
    }
}

fn record_locked(owner_loaded: &config::Loaded, bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > 8 * 1024 {
        return Err("session recovery record input exceeds 8192 bytes".into());
    }
    let owned = owned(owner_loaded)?;
    let input: Value = serde_json::from_slice(bytes)
        .map_err(|_| "session recovery record input must be one JSON object")?;
    let object = input
        .as_object()
        .ok_or("session recovery record input must be one JSON object")?;
    const INPUT_FIELDS: &[&str] = &[
        "sessionId",
        "currentStep",
        "currentWork",
        "latestCorrections",
        "evidenceRefs",
        "workRefs",
        "pendingOperation",
        "guidanceRefs",
    ];
    if object
        .keys()
        .any(|key| !INPUT_FIELDS.contains(&key.as_str()))
    {
        return Err("session recovery record input has an unknown field".into());
    }
    let session_id = input["sessionId"]
        .as_str()
        .ok_or("session recovery record requires the exact host-reported sessionId")?;
    bounded_text(session_id, "session ID", 512)?;
    if !owned.session.reports(session_id) {
        return Err("session recovery payload ID does not match current host selectors".into());
    }
    let current_step = input["currentStep"]
        .as_str()
        .ok_or("session recovery record requires currentStep")?;
    let current_work: WorkSelection = serde_json::from_value(input["currentWork"].clone())
        .map_err(|_| "session recovery currentWork must explicitly select or mark unresolved")?;
    let list = |field: &str| input.get(field).cloned().unwrap_or_else(|| json!([]));
    let latest_corrections: Vec<String> = serde_json::from_value(list("latestCorrections"))
        .map_err(|_| "session recovery latestCorrections must be a text array")?;
    let evidence_refs: Vec<Reference> = serde_json::from_value(list("evidenceRefs"))
        .map_err(|_| "session recovery evidenceRefs are malformed")?;
    let guidance_refs: Vec<Reference> = serde_json::from_value(list("guidanceRefs"))
        .map_err(|_| "session recovery guidanceRefs are malformed")?;
    let work_refs: Vec<WorkRef> = serde_json::from_value(list("workRefs"))
        .map_err(|_| "session recovery workRefs are malformed")?;
    let pending_operation: Option<PendingOperation> = serde_json::from_value(
        input
            .get("pendingOperation")
            .cloned()
            .unwrap_or(Value::Null),
    )
    .map_err(|_| "session recovery pendingOperation must be null or a read-only reference")?;
    // A decision already saved keeps the Work it was bound to. A correction
    // given here is bound to none: only `--decision` binds one, to the
    // current Work.
    let saved = read_hint(&owned.session.root)
        .ok()
        .flatten()
        .filter(|hint| hint.belongs_to(&owned.session, &owned.owner, &owned.record["goalId"]))
        .map(|hint| Draft::of(&hint));
    let latest_correction_works = latest_corrections
        .iter()
        .map(|text| {
            let draft = saved.as_ref()?;
            let at = draft
                .latest_corrections
                .iter()
                .position(|item| item == text)?;
            draft.latest_correction_works[at].clone()
        })
        .collect();
    let draft = Draft {
        current_step: current_step.into(),
        current_work,
        latest_corrections,
        latest_correction_works,
        evidence_refs,
        work_refs,
        pending_operation,
        guidance_refs,
    };
    for item in draft.evidence_refs.iter().chain(&draft.guidance_refs) {
        if item.consumption != "caller_reported_read" {
            return Err("session recovery reference must bind a caller-reported read to a source and producer".into());
        }
    }
    store_locked(owner_loaded, &owned, draft)
}

/// Check every field and reference of `draft` against the current Goal and
/// owner, as a stored record must be.
pub(super) fn verify_draft(
    owner_loaded: &config::Loaded,
    owned: &Owned,
    draft: &Draft,
) -> Result<(), String> {
    let Owned {
        session,
        owner,
        record,
    } = owned;
    bounded_text(&draft.current_step, "current step", MAX_TEXT)?;
    let current_work = &draft.current_work;
    if !matches!(
        current_work.state.as_str(),
        "unresolved" | "not_selected" | "known"
    ) || (current_work.state == "known") != current_work.work.is_some()
    {
        return Err("session recovery currentWork must be unresolved, not_selected, or an exact known Work locator".into());
    }
    if draft.latest_corrections.len() > MAX_ITEMS {
        return Err("session recovery accepts at most eight corrections".into());
    }
    for item in &draft.latest_corrections {
        bounded_text(item, "correction", super::hint::MAX_DECISION_BYTES)?;
    }
    if draft.latest_correction_works.len() != draft.latest_corrections.len() {
        return Err("session recovery must bind each correction to a Work or to none".into());
    }
    for item in draft.evidence_refs.iter().chain(&draft.guidance_refs) {
        bounded_text(&item.label, "reference label", 128)?;
        bounded_text(&item.path, "reference path", 512)?;
        if let Some(work) = item.work.as_deref() {
            bounded_text(work, "reference Work locator", 128)?;
        }
        if !valid_sha(&item.sha256)
            || !valid_sha(&item.producer_executable_sha256)
            || !matches!(
                item.consumption.as_str(),
                "caller_reported_read" | "generated_query_result"
            )
            || !matches!(item.root.as_str(), "product" | "control" | "state")
        {
            return Err("session recovery reference must bind a caller-reported read to a source and producer".into());
        }
    }
    if draft.evidence_refs.len() > MAX_ITEMS || draft.guidance_refs.len() > MAX_ITEMS {
        return Err(
            "session recovery accepts at most eight evidence and guidance references".into(),
        );
    }
    let work_refs = &draft.work_refs;
    verify_work_refs(record, session, owner, work_refs)?;
    if current_work.state == "known"
        && !work_refs
            .iter()
            .any(|reference| current_work.work.as_deref() == Some(reference.work.as_str()))
    {
        return Err("known currentWork must match an exact workRefs Goal member".into());
    }
    let _known_locator = known_work_locator(current_work, work_refs)?;
    for refs in [&draft.evidence_refs, &draft.guidance_refs] {
        verify_source_refs(owner_loaded, record, session, owner, work_refs, refs)?;
    }
    let pending = draft.pending_operation.as_ref();
    verify_pending(owner_loaded, record, session, owner, work_refs, pending)?;
    if let Some(PendingOperation::WorkDetail { work, .. }) = pending {
        if !work_refs.iter().any(|item| &item.work == work) {
            return Err("pending Work detail must name an exact workRefs Goal member".into());
        }
    }
    Ok(())
}

/// Verify `draft` and replace the session's record with it under the
/// caller's ledger lock. The last writer wins and is named in the record.
pub(super) fn store_locked(
    owner_loaded: &config::Loaded,
    owned: &Owned,
    draft: Draft,
) -> Result<Value, String> {
    verify_draft(owner_loaded, owned, &draft)?;
    let Owned {
        session,
        owner,
        record,
    } = owned;
    let owner_config_path = canonical_path(&owner_loaded.path, "native owner configuration")?;
    let last_checkpoint = match read_hint(&session.root)? {
        Some(existing)
            if existing.host == session.host
                && existing.session_key == session.key
                && existing.owner_identity_sha256 == owner.identity_sha256
                && existing.owner_project_id == owner.project_id =>
        {
            existing.last_checkpoint
        }
        _ => None,
    };
    let mut hint = Hint {
        kind: "exitbind_session_recovery_hint".into(),
        protocol: 2,
        host: session.host.into(),
        session_key: session.key.clone(),
        owner_config_path,
        owner_config_sha256: hash::text(&owner_loaded.source),
        owner_project_id: owner.project_id.clone(),
        owner_identity_sha256: owner.identity_sha256.clone(),
        goal_id: record["goalId"]
            .as_str()
            .ok_or("native Goal identity is unavailable")?
            .into(),
        goal_revision: record["revision"]
            .as_u64()
            .ok_or("native Goal revision is unavailable")?,
        goal_event_sha256: record["eventSha256"]
            .as_str()
            .ok_or("native Goal event identity is unavailable")?
            .into(),
        current_step: draft.current_step,
        current_work: draft.current_work,
        latest_corrections: draft.latest_corrections,
        latest_correction_works: if draft.latest_correction_works.iter().all(Option::is_none) {
            Vec::new()
        } else {
            draft.latest_correction_works
        },
        evidence_refs: draft.evidence_refs,
        work_refs: draft.work_refs,
        pending_operation: draft.pending_operation,
        guidance_refs: draft.guidance_refs,
        last_checkpoint,
        recorded_by_session_key: session.adopted().then(|| session.selector_key.clone()),
        sha256: String::new(),
    };
    validate_hint(&hint)?;
    hint.sha256 = hash::value(&serde_json::to_value(&hint).map_err(|error| error.to_string())?);
    write_hint_locked(session, &hint)?;
    Ok(json!({
        "recorded":true,
        "readOnlyAuthority":false,
        "hintSha256":hint.sha256,
        "goalId":hint.goal_id,
        "goalRevision":hint.goal_revision,
        "currentWork":hint.current_work,
        "savedStep":hint.current_step,
        "leadDecisions":hint.latest_corrections,
        "recordedBySessionKey":hint.recorded_by(),
        "next":"session recovery show",
    }))
}

/// Refresh only the last observed lifecycle marker and Goal event. The saved
/// step binding is not advanced, so any Goal change still invalidates it.
pub(crate) fn checkpoint(payload: &serde_json::Map<String, Value>, event: &str) {
    if !matches!(event, "PreCompact" | "PostCompact") {
        return;
    }
    let result = (|| -> Result<(), String> {
        let session =
            session_goal::native::current()?.ok_or("native session selector is unavailable")?;
        with_record_lock(&session.root, || checkpoint_locked(payload, event))
    })();
    let _ = result;
}

fn checkpoint_locked(payload: &serde_json::Map<String, Value>, event: &str) -> Result<(), String> {
    (|| -> Result<(), String> {
        let started = Instant::now();
        let session =
            session_goal::native::current()?.ok_or("native session selector is unavailable")?;
        if session.host != "codex" {
            return Err("compaction checkpoints are supported only for Codex sessions".into());
        }
        let id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or("host event session ID is unavailable")?;
        if !session.reports(id) {
            return Err("host event session ID disagrees with selectors".into());
        }
        let owner = session.owner()?.ok_or("native Goal owner is unavailable")?;
        let Some(record) = session_goal::read_for_recovery(&session.root)? else {
            return Err("native Goal is unavailable".into());
        };
        if started.elapsed() > HOOK_BUDGET {
            return Err("native recovery checkpoint exceeded its four-second hook budget".into());
        }
        let Some(mut hint) = read_hint(&session.root)? else {
            return Ok(());
        };
        if hint.host != session.host
            || hint.session_key != session.key
            || hint.owner_identity_sha256 != owner.identity_sha256
            || hint.owner_project_id != owner.project_id
        {
            return Err("recovery hint is foreign".into());
        }
        let owner_config = config::load(Some(&hint.owner_config_path))?;
        if canonical_path(&owner_config.path, "native owner configuration")?
            != hint.owner_config_path
            || hash::text(&owner_config.source) != hint.owner_config_sha256
        {
            return Err(crate::drift::refusal(
                crate::drift::Class::RecoveryOwnerConfiguration,
                &hint.owner_config_sha256,
                Some(&hash::text(&owner_config.source)),
                None,
                crate::drift::Route::RecoveryShow,
            ));
        }
        let owner_scope = session_goal::native::scope(&owner_config, false)?;
        if !owner_scope.owner_matches
            || owner_scope
                .owner
                .as_ref()
                .map(|actual| actual.identity_sha256.as_str())
                != Some(owner.identity_sha256.as_str())
        {
            return Err(
                "recovery hint owner configuration no longer matches the fixed Goal owner".into(),
            );
        }
        let next = Checkpoint {
            event: event.to_owned(),
            goal_revision: record["revision"]
                .as_u64()
                .ok_or("Goal revision is missing")?,
            goal_event_sha256: record["eventSha256"]
                .as_str()
                .ok_or("Goal event hash is missing")?
                .to_owned(),
        };
        if hint.last_checkpoint.as_ref() == Some(&next) {
            return Ok(());
        }
        if hint
            .last_checkpoint
            .as_ref()
            .is_some_and(|previous| checkpoint_is_ambiguous_older(previous, &next))
        {
            return Err(
                "ambiguous older PreCompact event was ignored; recovery hint was not changed"
                    .into(),
            );
        }
        hint.last_checkpoint = Some(next);
        hint.sha256.clear();
        hint.sha256 = hash::value(&serde_json::to_value(&hint).map_err(|error| error.to_string())?);
        write_hint_locked(&session, &hint)?;
        if started.elapsed() > HOOK_BUDGET {
            return Err("native recovery checkpoint exceeded its four-second hook budget".into());
        }
        Ok(())
    })()
}
