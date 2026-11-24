// Corrects the configured owner of a bounded native named Goal.
// Refuses unselected Work or unresolved admissions; see `src/session_goal.rs`.
//! Explicit correction of a native named Goal's configured owner.
//!
//! The owner record is a reader barrier. The sealed Goal append is the only
//! authority transition; a pending barrier keeps the old owner effective.

mod marker;
mod model;
mod retained;
mod source;

pub(super) use self::marker::{
    recovery_snapshot, resolve_owner, validate_legacy_history, validate_marker_envelope,
};
pub(crate) use self::model::same_owner_binding;
use self::model::{decisions, OwnerCorrectionDecision, SavedRequest};
pub(super) use self::model::{preserve_same_goal, validate_transition};
use crate::config::Loaded;
use serde_json::{json, Value};

#[derive(Clone)]
pub(crate) struct OwnerCorrectionRequest<'a> {
    pub(crate) target_config: &'a str,
    pub(crate) relocations: &'a str,
    pub(crate) request_id: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) old_writers_quiesced: bool,
    pub(crate) uncredited_members: Option<&'a str>,
    pub(crate) source_writers_quiesced: bool,
    pub(crate) current_binding: Option<&'a str>,
    pub(crate) apply: bool,
    pub(crate) abort: bool,
}

fn saved_request(
    loaded: &Loaded,
    request: &OwnerCorrectionRequest<'_>,
) -> Result<SavedRequest, String> {
    let path = loaded
        .path
        .to_str()
        .ok_or("owner configuration path is not valid UTF-8")?;
    if request.request_id.trim().is_empty()
        || request.request_id.len() > 128
        || request.request_id.chars().any(char::is_control)
        || request.reason.trim().is_empty()
        || request.reason.len() > 1024
        || request.reason.chars().any(char::is_control)
        || !request.old_writers_quiesced
    {
        return Err("goal correct-owner requires a bounded request ID and reason and an explicit old-writer quiescence attestation".into());
    }
    if !std::path::Path::new(request.target_config).is_absolute() {
        return Err("goal correct-owner requires an absolute target configuration path".into());
    }
    match (request.uncredited_members, request.source_writers_quiesced) {
        (Some(raw), true) => {
            retained::parse_selection(raw)?;
        }
        (None, false) => {}
        _ => {
            return Err(
                "uncredited members require an explicit source-writer quiescence attestation"
                    .into(),
            )
        }
    }
    Ok(SavedRequest {
        owner_config: path.to_owned(),
        target_config: request.target_config.to_owned(),
        relocations: request.relocations.to_owned(),
        request_id: request.request_id.to_owned(),
        reason: request.reason.to_owned(),
        old_writers_quiesced: request.old_writers_quiesced,
        uncredited_members: request.uncredited_members.map(str::to_owned),
        source_writers_quiesced: request
            .uncredited_members
            .map(|_| request.source_writers_quiesced),
    })
}

fn target_config(path: &str) -> Result<Loaded, String> {
    let selected = std::path::Path::new(path);
    let canonical = selected
        .canonicalize()
        .map_err(|error| format!("target configuration is unavailable: {error}"))?;
    if canonical != selected {
        return Err("target configuration must use its canonical path without symlinks".into());
    }
    crate::config::load(Some(path))
}

fn fresh(loaded: &Loaded) -> Result<Loaded, String> {
    let path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let current = crate::config::load(Some(path))?;
    if super::recovery::config_binding_for_current(&current)?
        != super::recovery::config_binding_for_current(loaded)?
    {
        return Err("configuration differs from the loaded owner selection; preview again".into());
    }
    Ok(current)
}

/// Ensure no historical or pending Work authority can be carried by this
/// narrow correction, even when a mapping was later retired or removed.
fn empty_goal(history: &[Value], goal_id: &str) -> Result<(), String> {
    let mut found = false;
    for record in history.iter().filter(|record| record["goalId"] == goal_id) {
        found = true;
        empty_record(record)?;
    }
    if !found {
        return Err("the named Goal history is unavailable".into());
    }
    Ok(())
}

/// One historical record's eligibility. Ordinary Goals may be ineligible;
/// the correction fold records this fact and refuses only a correction claim.
fn empty_record(record: &Value) -> Result<(), String> {
    for field in [
        "journeyMembers",
        "journeyIntegrations",
        "journeyAdmissions",
        "workAssociations",
        "sessionAdoptions",
    ] {
        if record
            .get(field)
            .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
        {
            return Err("owner correction requires a Goal with no Work, member, integration, admission or adoption history".into());
        }
    }
    if record.get("nativeGoalRecovery").is_some() || record.get("quarantineResiduals").is_some() {
        return Err("owner correction does not accept recovery or quarantine state".into());
    }
    if !record["continuation"].is_null() {
        if !super::requirements::named(record) {
            return Err("owner correction does not accept an incompatible continuation".into());
        }
        super::requirements::recorded_sources(record)?;
        if !record["continuation"]["mappings"]
            .as_array()
            .is_some_and(Vec::is_empty)
        {
            return Err("owner correction requires no active or retired Work mappings".into());
        }
    }
    for category in super::CATEGORIES {
        let items = record[category]
            .as_array()
            .ok_or("Goal category is malformed")?;
        if items.iter().any(|item| {
            item["disposition"] == "direct"
                || !item["resultRefs"].as_array().is_some_and(Vec::is_empty)
        }) {
            return Err(
                "owner correction cannot carry a prior completion or accepted result".into(),
            );
        }
    }
    Ok(())
}

fn validate_recorded_selection(
    record: &Value,
    decision: &OwnerCorrectionDecision,
) -> Result<(), String> {
    if !super::requirements::named(record) || record["closure"]["closed"] != false {
        return Err("owner correction selection requires an open named Goal".into());
    }
    let prior = decisions(record)?;
    let project_identity = prior.last().map_or_else(
        || {
            record["continuation"]["projectIdentity"]
                .as_str()
                .unwrap_or_default()
        },
        |previous| previous.target_config.project_identity_sha256.as_str(),
    );
    if project_identity != decision.old_config.project_identity_sha256 {
        return Err("owner correction source project identity is inconsistent".into());
    }
    if let Some(selected) = &decision.uncredited_members {
        super::journey::state(record)?;
        let members = record["journeyMembers"]
            .as_array()
            .ok_or("historical members are unavailable")?;
        if members.len() != selected.len()
            || selected.iter().any(|item| {
                !members.iter().any(|member| {
                    member["work"] == item.work
                        && crate::evidence::hash::value(member) == item.member_sha256
                        && member["startEventSha256"] == item.start_event_sha256
                        && member["host"] == decision.host
                        && member["sessionKey"] == decision.session_key
                        && member["ownerIdentitySha256"]
                            == decision.old_config.local_identity_sha256
                        && member["ownerProjectId"] == json!(decision.old_config.project_id)
                })
            })
        {
            return Err(
                "owner correction retained Work does not match the exact selection and prior owner"
                    .into(),
            );
        }
        let all_target = members
            .iter()
            .all(|member| retained::matches_binding(member, &decision.target_config));
        if (decision.version == 2 && !all_target) || (decision.version == 3 && all_target) {
            return Err("owner correction version does not match its retained Work sources".into());
        }
    }
    let sources = super::requirements::recorded_sources(record)?;
    if sources.len() != decision.relocations.len()
        || sources.iter().any(|(reference, sha, _)| {
            !decision
                .relocations
                .iter()
                .any(|item| item.original_ref == *reference && item.recorded_sha256 == *sha)
        })
    {
        return Err(
            "owner correction selection does not cover the current recorded sources".into(),
        );
    }
    Ok(())
}

fn replay<'a>(
    history: &'a [Value],
    request_id: &str,
    request_sha: &str,
    binding: Option<&str>,
) -> Result<Option<&'a Value>, String> {
    for record in history {
        for item in decisions(record)? {
            if item.request_id == request_id {
                if item.request_sha256 != request_sha
                    || binding != Some(item.preview_sha256.as_str())
                {
                    return Err(
                        "owner correction request ID belongs to a different request or preview"
                            .into(),
                    );
                }
                return Ok(Some(record));
            }
        }
    }
    Ok(None)
}

fn owner_binding(
    loaded: &Loaded,
    record: &Value,
    owner: &super::native::Owner,
) -> Result<super::recovery::ConfigBinding, String> {
    let old_config = super::recovery::config_binding_for_current(loaded)?;
    if old_config.local_identity_sha256 != owner.identity_sha256
        || old_config.project_id != owner.project_id
    {
        return Err("owner correction requires the current native Goal owner configuration".into());
    }
    let existing = decisions(record)?;
    if existing.len() >= 16 {
        return Err("native Goal owner correction history bound reached".into());
    }
    if let Some(last) = existing.last() {
        if !same_owner_binding(&last.target_config, &old_config) {
            return Err("current owner configuration differs from the saved correction".into());
        }
        source::current(loaded, record)?;
    } else if record["continuation"]["projectIdentity"] != old_config.project_identity_sha256 {
        return Err("named Goal source identity does not match the current owner".into());
    }
    Ok(old_config)
}

struct DecisionContext<'a, 'b> {
    loaded: &'a Loaded,
    target: &'a Loaded,
    session: &'a super::native::Session,
    history: &'a [Value],
    owner_value: &'a Value,
    owner: &'a super::native::Owner,
    request: &'a OwnerCorrectionRequest<'b>,
    request_sha256: &'a str,
    prior_digest: Option<&'a str>,
    member_locks: Option<&'a [retained::MemberLock]>,
}

fn exact_decision(context: DecisionContext<'_, '_>) -> Result<OwnerCorrectionDecision, String> {
    let DecisionContext {
        loaded,
        target,
        session,
        history,
        owner_value,
        owner,
        request,
        request_sha256,
        prior_digest,
        member_locks,
    } = context;
    if session.adopted() {
        return Err(
            "owner correction requires the original native session, not an adoption".into(),
        );
    }
    let record = history.last().ok_or("no native Goal is incorporated")?;
    if !super::requirements::named(record) || record["closure"]["closed"] != false {
        return Err("owner correction requires an open named Goal".into());
    }
    let goal_id = record["goalId"]
        .as_str()
        .ok_or("Goal identity is unavailable")?;
    let old_config = owner_binding(loaded, record, owner)?;
    let target_config = super::recovery::config_binding_for_current(target)?;
    if old_config.local_identity_sha256 == target_config.local_identity_sha256 {
        return Err("owner correction requires a different configured owner".into());
    }
    let (uncredited_members, version) = if let Some(raw) = request.uncredited_members {
        let works = retained::parse_selection(raw)?;
        let selected = retained::select_history(history, goal_id, session, owner, &works)?;
        let (members, same_target) = retained::select(
            &selected,
            target,
            session,
            owner,
            member_locks.ok_or("historical Work locks are unavailable")?,
        )?;
        (Some(members), if same_target { 2 } else { 3 })
    } else {
        empty_goal(history, goal_id)?;
        (None, 1)
    };
    let relocations = source::relocations(target, record, request.relocations)?;
    let mut decision = OwnerCorrectionDecision {
        version,
        goal_id: goal_id.to_owned(),
        host: session.host.into(),
        session_key: session.key.clone(),
        old_owner_record_sha256: prior_digest
            .map(str::to_owned)
            .unwrap_or(marker::legacy_digest(owner_value)?),
        previous_event_sha256: record["eventSha256"]
            .as_str()
            .ok_or("Goal event identity is unavailable")?
            .to_owned(),
        previous_revision: record["revision"]
            .as_u64()
            .ok_or("Goal revision is unavailable")?,
        old_config,
        target_config,
        reason: request.reason.to_owned(),
        relocations,
        request_id: request.request_id.to_owned(),
        request_sha256: request_sha256.to_owned(),
        preview_sha256: String::new(),
        old_writers_quiesced: request.old_writers_quiesced,
        source_writers_quiesced: uncredited_members
            .as_ref()
            .map(|_| request.source_writers_quiesced),
        uncredited_members,
        decision_sha256: String::new(),
    };
    decision.preview_sha256 = decision.preview_digest()?;
    decision.decision_sha256 = decision.digest()?;
    decision.validate()?;
    validate_recorded_selection(record, &decision)?;
    Ok(decision)
}

fn routes(request: &SavedRequest, binding: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let mut route = crate::emitted::Route::new([
        "goal",
        "correct-owner",
        "--target-config",
        request.target_config.as_str(),
        "--relocations",
        request.relocations.as_str(),
        "--request-id",
        request.request_id.as_str(),
        "--reason",
        request.reason.as_str(),
        "--old-writers-quiesced",
    ]);
    if let Some(members) = &request.uncredited_members {
        route = route.words(["--uncredited-members", members, "--source-writers-quiesced"]);
    }
    route = route.words(["--current-binding", binding]);
    Ok((
        route
            .clone()
            .words(["--apply", "--json"])
            .config_text(&request.owner_config)
            .full_argv()?,
        route
            .words(["--abort", "--json"])
            .config_text(&request.owner_config)
            .full_argv()?,
    ))
}

fn snapshot(
    session: &super::native::Session,
    access: super::native::Access,
) -> Result<(Value, Vec<Value>, marker::Resolved), String> {
    let value = session
        .owner_value(access)?
        .ok_or("native Goal owner record is unavailable")?;
    let raw = super::raw_history(&session.root)?;
    if raw.as_ref().is_some_and(|source| !source.ends_with('\n')) {
        return Err(
            "native Goal journal needs a terminated final record before owner correction".into(),
        );
    }
    let history = super::records_from_raw(raw.as_deref())?;
    let resolved = if value["protocol"] == 2 {
        marker::resolve_history(session, &value, &history)?
    } else {
        super::native::validate_owner_v1(&value, &session.key, session.host)?;
        if value.get("journalSessionKey").is_some() || session.adopted() {
            return Err("owner correction requires the original native session".into());
        }
        if history
            .iter()
            .any(|record| record.get(model::FIELD).is_some())
        {
            return Err("native Goal correction history has no reader barrier".into());
        }
        marker::Resolved {
            owner: super::native::Owner {
                project_id: value["ownerProjectId"].as_str().map(str::to_owned),
                identity_sha256: value["ownerIdentitySha256"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            },
            pending: false,
            latest: None,
        }
    };
    if session.owner_value(access)?.as_ref() != Some(&value) {
        return Err("native Goal owner and journal snapshot is inconsistent; retry".into());
    }
    if super::raw_history(&session.root)? != raw {
        return Err("native Goal owner and journal snapshot is inconsistent; retry".into());
    }
    Ok((value, history, resolved))
}

pub(crate) fn correct(
    loaded: &Loaded,
    request: OwnerCorrectionRequest<'_>,
) -> Result<Value, String> {
    if request.abort {
        return correct_inner(loaded, request, None);
    }
    if let Some(raw) = request.uncredited_members {
        if !request.source_writers_quiesced {
            return Err(
                "uncredited members require an explicit source-writer quiescence attestation"
                    .into(),
            );
        }
        let saved = saved_request(loaded, &request)?;
        let session = super::native::current()?
            .ok_or("owner correction requires the original native host session")?;
        if session.adopted() {
            return Err("owner correction requires the original native host session".into());
        }
        let (owner_value, history, resolved) =
            snapshot(&session, super::native::Access::RequireWritable)?;
        if request.apply {
            if let Some(event) = replay(
                &history,
                request.request_id,
                &saved.digest(),
                request.current_binding,
            )? {
                return Ok(
                    json!({"kind":"native_goal_owner_correction_replayed","readOnly":true,"event":event}),
                );
            }
        }
        let local = super::native::owner_for(loaded)?;
        if local.identity_sha256 != resolved.owner.identity_sha256
            || local.project_id != resolved.owner.project_id
        {
            return Err("native session Goal is owned by another configured project; this project may read context only".into());
        }
        let record = history.last().ok_or("no native Goal is incorporated")?;
        if !super::requirements::named(record) || record["closure"]["closed"] != false {
            return Err("owner correction requires an open named Goal".into());
        }
        owner_binding(&fresh(loaded)?, record, &resolved.owner)?;
        let goal_id = record["goalId"]
            .as_str()
            .ok_or("Goal identity is unavailable")?;
        let works = retained::parse_selection(raw)?;
        let selected =
            retained::select_history(&history, goal_id, &session, &resolved.owner, &works)?;
        let guarded = LockedMembers {
            owner_value,
            goal_event: record["eventSha256"].clone(),
            locks: retained::member_locks(&selected)?,
        };
        return with_member_locks(&guarded.locks, || {
            correct_inner(loaded, request, Some(&guarded))
        });
    }
    correct_inner(loaded, request, None)
}

struct LockedMembers {
    owner_value: Value,
    goal_event: Value,
    locks: Vec<retained::MemberLock>,
}

impl LockedMembers {
    fn verify(&self, owner_value: &Value, history: &[Value]) -> Result<(), String> {
        if &self.owner_value != owner_value
            || history.last().map(|record| &record["eventSha256"]) != Some(&self.goal_event)
        {
            return Err(
                "native Goal or owner changed while acquiring historical Work locks".into(),
            );
        }
        Ok(())
    }
}

fn with_member_locks<T>(
    members: &[retained::MemberLock],
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    fn take<T>(
        locks: &[crate::run::ledger::LedgerPath],
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        match locks.split_first() {
            Some((lock, rest)) => crate::run::ledger::with_lock(lock, || take(rest, operation)),
            None => operation(),
        }
    }
    let mut locks = members
        .iter()
        .map(|member| member.ledger.clone())
        .collect::<Vec<_>>();
    locks.sort_by(|left, right| left.lock.cmp(&right.lock));
    locks.dedup_by(|left, right| left.lock == right.lock);
    take(&locks, operation)
}

fn correct_inner(
    loaded: &Loaded,
    request: OwnerCorrectionRequest<'_>,
    locked: Option<&LockedMembers>,
) -> Result<Value, String> {
    if request.abort && request.apply {
        return Err("choose either apply or abort".into());
    }
    if !request.apply && !request.abort && request.current_binding.is_some() {
        return Err("--current-binding is accepted only for apply or abort".into());
    }
    let saved = saved_request(loaded, &request)?;
    let request_sha256 = saved.digest();
    let session = super::native::current()?
        .ok_or("owner correction requires the original native host session")?;
    if session.adopted() {
        return Err("owner correction requires the original native host session".into());
    }
    let (owner_value, history, resolved) =
        snapshot(&session, super::native::Access::RequireWritable)?;
    if request.apply {
        if let Some(event) = replay(
            &history,
            request.request_id,
            &request_sha256,
            request.current_binding,
        )? {
            return Ok(
                json!({"kind":"native_goal_owner_correction_replayed","readOnly":true,"event":event}),
            );
        }
    }
    if let Some(locked) = locked {
        locked.verify(&owner_value, &history)?;
    }
    if !request.apply && !request.abort {
        for record in &history {
            if decisions(record)?
                .iter()
                .any(|item| item.request_id == request.request_id)
            {
                return Err("owner correction request ID is already recorded; replay its exact apply binding".into());
            }
        }
    }
    let local = super::native::owner_for(loaded)?;
    if local.identity_sha256 != resolved.owner.identity_sha256
        || local.project_id != resolved.owner.project_id
    {
        return Err("native session Goal is owned by another configured project; this project may read context only".into());
    }
    let pending = if resolved.pending {
        marker::Marker::parse(&session, &owner_value)?.pending
    } else {
        None
    };
    if let Some(pending) = &pending {
        if pending.request_sha256 != request_sha256 || pending.request_id != request.request_id {
            let saved_pending = marker::Marker::parse(&session, &owner_value)?
                .request
                .ok_or("native Goal pending request is unavailable")?;
            let (retry, abort) = routes(&saved_pending, &pending.preview_sha256)?;
            return Err(pending_refusal(
                "another owner correction has a pending reader barrier",
                &retry,
                &abort,
            ));
        }
    }
    if request.abort {
        let pending = pending.ok_or("no uncommitted owner correction is available to abort")?;
        if request.current_binding != Some(pending.preview_sha256.as_str()) {
            return Err("abort requires the pending correction's exact preview binding".into());
        }
        return abort(&session, loaded, &request_sha256, &pending);
    }
    let decision = (|| {
        let current = fresh(loaded)?;
        let target = target_config(request.target_config)?;
        let prior_digest = pending
            .as_ref()
            .map(|value| value.old_owner_record_sha256.as_str());
        exact_decision(DecisionContext {
            loaded: &current,
            target: &target,
            session: &session,
            history: &history,
            owner_value: &owner_value,
            owner: &resolved.owner,
            request: &request,
            request_sha256: &request_sha256,
            prior_digest,
            member_locks: locked.map(|value| value.locks.as_slice()),
        })
    })()
    .map_err(|error: String| {
        if let Some(pending) = &pending {
            match routes(&saved, &pending.preview_sha256) {
                Ok((retry, abort)) => pending_refusal(&crate::drift::plain(&error), &retry, &abort),
                Err(_) => {
                    let status = crate::emitted::command_line(
                        &["goal", "status", "--json"],
                        Some(&loaded.path),
                    );
                    format!(
                        "{}; pending reader barrier: read `{status}` for recovery",
                        crate::drift::plain(&error)
                    )
                }
            }
        } else {
            error
        }
    })?;
    if pending.as_ref().is_some_and(|saved| saved != &decision) {
        let prior = pending.as_ref().expect("pending decision was checked");
        let (retry, abort) = routes(&saved, &prior.preview_sha256)?;
        return Err(pending_refusal(
            "pending owner correction does not match its recorded decision",
            &retry,
            &abort,
        ));
    }
    let (apply_argv, abort_argv) = routes(&saved, &decision.preview_sha256)?;
    if !request.apply {
        return Ok(
            json!({"kind":"native_goal_owner_correction_preview","readOnly":true,
            "currentBinding":decision.preview_sha256,"decision":decision,"request":saved,"pending":resolved.pending,
            "applyArgv":apply_argv,"abortArgv":abort_argv,
            "attestation":"Lead attests old-version writers for this Goal are quiescent; the marker cannot fence already-running old code"}),
        );
    }
    if request.current_binding != Some(decision.preview_sha256.as_str()) {
        return Err("--current-binding does not match the exact owner correction preview".into());
    }
    apply(
        &session,
        loaded,
        &request,
        ApplyEvidence {
            saved_request: &saved,
            request_sha: &request_sha256,
            expected: &decision,
            apply_argv: &apply_argv,
            abort_argv: &abort_argv,
            locked,
        },
    )
}

fn abort(
    session: &super::native::Session,
    loaded: &Loaded,
    request_sha: &str,
    pending: &OwnerCorrectionDecision,
) -> Result<Value, String> {
    let ledger = super::history_ledger(&session.root)?;
    crate::run::ledger::with_lock(&ledger, || {
        let (value, _, resolved) = snapshot(session, super::native::Access::RequireWritable)?;
        let current = marker::Marker::parse(session, &value)?;
        if !resolved.pending
            || current.pending.as_ref() != Some(pending)
            || pending.request_sha256 != request_sha
        {
            return Err(
                "pending owner correction no longer matches; inspect current Goal state".into(),
            );
        }
        let local = super::native::owner_for(loaded)?;
        if local.identity_sha256 != resolved.owner.identity_sha256
            || local.project_id != resolved.owner.project_id
        {
            return Err("only the current owner can abort this correction".into());
        }
        let next = marker::Marker::new(session, &value, current.base_decision_sha256, None, None)?;
        marker::replace(session, &value, &next)?;
        Ok(
            json!({"kind":"native_goal_owner_correction_aborted","readOnly":false,
            "goalEventUnchanged":pending.previous_event_sha256,"ownerUnchanged":true}),
        )
    })
}

struct ApplyEvidence<'a> {
    saved_request: &'a SavedRequest,
    request_sha: &'a str,
    expected: &'a OwnerCorrectionDecision,
    apply_argv: &'a [String],
    abort_argv: &'a [String],
    locked: Option<&'a LockedMembers>,
}

fn pending_refusal(error: &str, apply: &[String], abort: &[String]) -> String {
    format!(
        "{error}; pending reader barrier: retry exact argv {} or abort exact argv {}",
        json!(apply),
        json!(abort)
    )
}

fn apply(
    session: &super::native::Session,
    loaded: &Loaded,
    request: &OwnerCorrectionRequest<'_>,
    evidence: ApplyEvidence<'_>,
) -> Result<Value, String> {
    let ApplyEvidence {
        saved_request,
        request_sha,
        expected,
        apply_argv,
        abort_argv,
        locked,
    } = evidence;
    let ledger = super::history_ledger(&session.root)?;
    crate::run::ledger::with_lock(&ledger, || {
        let (value, history, resolved) = snapshot(session, super::native::Access::RequireWritable)?;
        if let Some(event) = replay(
            &history,
            request.request_id,
            request_sha,
            request.current_binding,
        )? {
            return Ok(
                json!({"kind":"native_goal_owner_correction_replayed","readOnly":true,"event":event}),
            );
        }
        if let Some(locked) = locked {
            locked.verify(&value, &history)?;
        }
        let pending = if resolved.pending {
            marker::Marker::parse(session, &value)?.pending
        } else {
            None
        };
        if pending.as_ref().is_some_and(|saved| saved != expected) {
            return Err("a different owner correction is pending".into());
        }
        let decision = (|| {
            let current = fresh(loaded)?;
            let target = target_config(request.target_config)?;
            let prior_digest = pending
                .as_ref()
                .map(|item| item.old_owner_record_sha256.as_str());
            exact_decision(DecisionContext {
                loaded: &current,
                target: &target,
                session,
                history: &history,
                owner_value: &value,
                owner: &resolved.owner,
                request,
                request_sha256: request_sha,
                prior_digest,
                member_locks: locked.map(|value| value.locks.as_slice()),
            })
        })()
        .map_err(|error: String| {
            if pending.is_some() {
                pending_refusal(&crate::drift::plain(&error), apply_argv, abort_argv)
            } else {
                error
            }
        })?;
        if &decision != expected {
            if pending.is_some() {
                return Err(pending_refusal(
                    "pending correction no longer matches its preview",
                    apply_argv,
                    abort_argv,
                ));
            }
            return Err("owner correction preview no longer matches its exact inputs".into());
        }
        let base = resolved
            .latest
            .as_ref()
            .map(|item| item.decision_sha256.clone());
        if pending.is_none() {
            let marker = marker::Marker::new(
                session,
                &value,
                base,
                Some(decision.clone()),
                Some(saved_request.clone()),
            )?;
            marker::replace(session, &value, &marker).map_err(|error| format!(
                "{error}; inspect the reader barrier and retry exact argv {} or abort exact argv {}",
                json!(apply_argv), json!(abort_argv)))?;
        }
        let next = (|| {
            let previous = history.last().ok_or("native Goal disappeared")?;
            let mut next = previous.clone();
            next.as_object_mut().ok_or("Goal is malformed")?.remove("eventSha256");
            next["revision"] = json!(decision.previous_revision.checked_add(1).ok_or("Goal revision exhausted")?);
            next["predecessor"] = json!({"goalId":decision.goal_id,"revision":decision.previous_revision});
            next["successorOf"] = Value::Null;
            let mut corrections = decisions(previous)?;
            corrections.push(decision.clone());
            next[model::FIELD] = serde_json::to_value(corrections).map_err(|error| error.to_string())?;
            let next = super::sealed(next, Some(previous));
            super::validate_record(&next, Some(previous), history.len() + 1)?;
            let raw = super::raw_history(&session.root)?.ok_or("native Goal journal is unavailable")?;
            if super::records_from_raw(Some(&raw))? != history {
                return Err("native Goal journal no longer matches the selected predecessor".into());
            }
            crate::run::ledger::append(&ledger, &super::journal_form::stored(&next, Some(previous)), false, &raw)?;
            Ok(next)
        })().map_err(|error: String| format!("{error}; reader barrier remains with original owner effective. Retry exact argv {} or abort exact argv {}", json!(apply_argv), json!(abort_argv)))?;
        marker::sync_directory(ledger.path.parent().ok_or("Goal journal has no parent")?)
            .map_err(|error| format!("Goal correction journal was committed but directory synchronization failed: {error}; inspect current owner and replay exact argv {}; do not abort or repeat a different request", json!(apply_argv)))?;
        Ok(
            json!({"kind":"native_goal_owner_correction_applied","readOnly":false,"event":next,
            "currentOwnerConfig":decision.target_config.path}),
        )
    })
}

pub(super) fn effective_identity(
    loaded: &Loaded,
    record: &Value,
    original: &str,
) -> Result<String, String> {
    let Some(decision) = decisions(record)?.last().cloned() else {
        return Ok(original.to_owned());
    };
    let fresh = fresh(loaded)?;
    if !same_owner_binding(
        &super::recovery::config_binding_for_current(&fresh)?,
        &decision.target_config,
    ) {
        return Err("named Goal owner configuration does not match its correction".into());
    }
    source::current(&fresh, record)?;
    Ok(decision.target_config.project_identity_sha256)
}

pub(super) fn recorded_identity(record: &Value, original: &str) -> Result<String, String> {
    Ok(decisions(record)?.last().map_or_else(
        || original.to_owned(),
        |decision| decision.target_config.project_identity_sha256.clone(),
    ))
}

/// The correction seals exclusions rather than retiring or changing a Work.
/// Every later reader requires the exact retained member identities to remain.
pub(crate) fn uncredited_binding(record: &Value) -> Result<Option<String>, String> {
    let Some(decision) = decisions(record)?.last().cloned() else {
        return Ok(None);
    };
    let Some(selected) = &decision.uncredited_members else {
        return Ok(None);
    };
    let members = record["journeyMembers"]
        .as_array()
        .ok_or("retained historical members are unavailable")?;
    for item in selected {
        if members
            .iter()
            .filter(|member| {
                member["work"] == item.work
                    && crate::evidence::hash::value(member) == item.member_sha256
                    && member["startEventSha256"] == item.start_event_sha256
            })
            .count()
            != 1
        {
            return Err("retained historical Work identity changed after owner correction".into());
        }
    }
    Ok(Some(decision.decision_sha256))
}

pub(super) fn uncredited_member(
    record: &Value,
    member: &super::journey::Member,
) -> Result<bool, String> {
    if uncredited_binding(record)?.is_none() {
        return Ok(false);
    }
    Ok(decisions(record)?
        .last()
        .and_then(|decision| decision.uncredited_members.as_ref())
        .is_some_and(|items| items.iter().any(|item| item.work == member.work)))
}

/// After correction a Work category reference must name its Work explicitly.
/// This prevents an anonymous ledger-head hash from bypassing the lineage
/// exclusion while leaving every retained Work in its original state.
pub(super) fn uncredited_reference(
    loaded: &Loaded,
    record: &Value,
    reference: &str,
) -> Result<bool, String> {
    if uncredited_binding(record)?.is_none() {
        return Ok(false);
    }
    let decision = decisions(record)?
        .last()
        .cloned()
        .ok_or("owner correction is unavailable")?;
    let selected = decision
        .uncredited_members
        .ok_or("uncredited Work selection is unavailable")?;
    if selected.iter().any(|item| item.work == reference) {
        return Ok(true);
    }
    if crate::work::valid_work_handle(reference) {
        if let Some(member) =
            super::journey::member_for_work_with_lineage(loaded, record, reference)?
        {
            if selected.iter().any(|item| member["work"] == item.work) {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    Ok(super::native::valid_hash(reference))
}

pub(super) fn current_source_path(
    record: &Value,
    reference: &str,
    sha: &str,
) -> Result<String, String> {
    source::current_path(record, reference, sha)
}

/// Recover the exact interrupted request without requiring the caller to
/// remember its original JSON spelling or target path selection.
pub(crate) fn pending_recovery(
    session: &super::native::Session,
    loaded: &Loaded,
) -> Result<Option<Value>, String> {
    let value = session
        .owner_value(super::native::Access::ReadExisting)?
        .ok_or("native Goal owner record is unavailable")?;
    if value["protocol"] != 2 {
        return Ok(None);
    }
    let (current, _, resolved) = snapshot(session, super::native::Access::ReadExisting)?;
    if current != value {
        return Err("native Goal owner and journal snapshot is inconsistent; retry".into());
    }
    if !resolved.pending {
        return Ok(None);
    }
    let local = super::native::owner_for(loaded)?;
    if local.identity_sha256 != resolved.owner.identity_sha256
        || local.project_id != resolved.owner.project_id
    {
        return Ok(None);
    }
    let marker = marker::Marker::parse(session, &value)?;
    let pending = marker
        .pending
        .ok_or("native Goal pending correction is unavailable")?;
    let request = marker
        .request
        .ok_or("native Goal pending request is unavailable")?;
    let (apply, abort) = routes(&request, &pending.preview_sha256)?;
    Ok(Some(
        json!({"state":"pending","requestId":pending.request_id,
        "currentBinding":pending.preview_sha256,"applyArgv":apply,"abortArgv":abort,
        "owner":"unchanged","meaning":"Retry the exact correction or abort its uncommitted reader barrier."}),
    ))
}

#[cfg(test)]
mod tests;
