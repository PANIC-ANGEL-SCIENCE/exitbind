use super::*;
use serde_json::json;

/// Resolve an optional native-session context without making malformed or
/// foreign context a failure of ordinary Work detail and recovery routes.
pub(crate) fn context_for_work(loaded: &crate::config::Loaded, work: &str) -> WorkContext {
    context_for_work_with_root(loaded, work).context
}

pub(crate) fn context_for_work_with_root(
    loaded: &crate::config::Loaded,
    work: &str,
) -> super::WorkContextResolution {
    let value = match read_private_companion(
        loaded,
        work,
        ".session-goal.json",
        "Work Goal context companion",
        COMPANION_KIND,
        PROTOCOL,
        &[
            "host",
            "sessionKey",
            "parentGoalId",
            "parentObservedGoalEventSha256",
            "ownerProjectId",
            "ownerIdentitySha256",
        ],
    ) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return match super::super::native::current() {
                Ok(None) => match super::super::read(&loaded.state_root) {
                    Ok(None) => resolution(WorkContext::NotRequested, None),
                    Ok(Some(_)) => resolution(WorkContext::Local, None),
                    Err(_) => resolution(WorkContext::Invalid, None),
                },
                Ok(Some(session)) => {
                    let legacy_bound = match legacy_goal_is_bound(loaded, work, &session) {
                        Ok(bound) => bound,
                        Err(_) => return resolution(WorkContext::Invalid, None),
                    };
                    if legacy_bound {
                        resolution(WorkContext::Local, None)
                    } else {
                        match (session.owner(), super::super::read(&session.root)) {
                            (Ok(None), Ok(None)) => resolution(WorkContext::NotRequested, None),
                            (Err(_), _) | (_, Err(_)) => resolution(WorkContext::Invalid, None),
                            (Ok(Some(_)), Ok(Some(_))) => {
                                match native_successor_context(loaded, work, &session) {
                                    Ok(Some(context)) => context,
                                    Ok(None) => resolution(WorkContext::Unavailable, None),
                                    Err(_) => resolution(WorkContext::Invalid, None),
                                }
                            }
                            _ => resolution(WorkContext::Unavailable, None),
                        }
                    }
                }
                Err(_) => resolution(WorkContext::Invalid, None),
            };
        }
        Err(_) => return resolution(WorkContext::Invalid, None),
    };
    match resolve_companion(loaded, work, &value) {
        Ok((record, owner_matches, root)) if owner_matches => {
            resolution(WorkContext::Owner(record), Some(root))
        }
        Ok((record, _, _)) => resolution(WorkContext::Session(record), None),
        Err(_) => resolution(WorkContext::Invalid, None),
    }
}

fn resolution(
    context: WorkContext,
    owner_root: Option<std::path::PathBuf>,
) -> super::WorkContextResolution {
    super::WorkContextResolution {
        context,
        owner_root,
    }
}

/// Verified successors reuse their original native member without writing a
/// companion, creating an admission or attaching an ordinary unbound Work.
fn native_successor_context(
    loaded: &crate::config::Loaded,
    work: &str,
    session: &super::super::native::Session,
) -> Result<Option<super::WorkContextResolution>, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    if events
        .first()
        .and_then(|event| event.get("supersedes"))
        .is_none()
    {
        return Ok(None);
    }
    let owner = session
        .owner()?
        .ok_or("native session owner is unavailable")?;
    let history = super::super::records(&session.root)?;
    let mut found = None;
    for record in history.iter().rev() {
        if let Some(member) =
            super::super::journey::member_for_work_with_lineage(loaded, record, work)?
        {
            found = Some(member);
            break;
        }
    }
    let Some(member) = found else {
        return Ok(None);
    };
    super::super::journey::validate_member_for_session(loaded, &member, session, &owner)?;
    let observed = history
        .iter()
        .position(|record| {
            record["goalId"] == member["parentGoalId"]
                && record["eventSha256"] == member["parentObservedGoalEventSha256"]
        })
        .ok_or("original native Goal observation is absent")?;
    let mut parent = history[observed..]
        .iter()
        .take_while(|record| record["goalId"] == member["parentGoalId"])
        .last()
        .cloned()
        .ok_or("native parent Goal is unavailable")?;
    let scope = super::super::native::scope(loaded, false)?;
    Ok(Some(if scope.owner_matches {
        resolution(WorkContext::Owner(parent), Some(scope.root))
    } else {
        let start = events.first().ok_or("successor start is unavailable")?;
        if start["action"] != "start" {
            return Err("successor start is malformed".into());
        }
        parent[VERIFIED_CONTEXT_FIELD] = json!({
            "kind":"context_only",
            "work":work,
            "startEventSha256":start["eventSha256"],
        });
        resolution(WorkContext::Session(parent), None)
    }))
}

fn legacy_goal_is_bound(
    loaded: &crate::config::Loaded,
    work: &str,
    session: &super::super::native::Session,
) -> Result<bool, String> {
    let Some(record) = super::super::read(&loaded.state_root)? else {
        return Ok(false);
    };
    if super::super::progress::record_bound_to_work(&record, work)
        || record["continuation"]["work"].as_str() == Some(work)
    {
        return Ok(true);
    }
    let Some(member) = super::super::journey::member_for_work_with_lineage(loaded, &record, work)?
    else {
        return Ok(false);
    };
    let owner = session
        .owner()?
        .ok_or("native session Goal owner is unavailable")?;
    super::super::journey::validate_member_for_session(loaded, &member, session, &owner)?;
    let history = super::super::records(&session.root)?;
    if !history.iter().any(|item| {
        item["goalId"] == member["parentGoalId"]
            && item["eventSha256"] == member["parentObservedGoalEventSha256"]
    }) {
        return Err(
            "original Goal observation for the successor Work is absent from this native session"
                .into(),
        );
    }
    Ok(true)
}

fn resolve_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    value: &Value,
) -> Result<(Value, bool, std::path::PathBuf), String> {
    validate_companion(value)?;
    if value["work"].as_str() != Some(work) {
        return Err("Work Goal context companion names another Work".into());
    }
    let session =
        super::super::native::current()?.ok_or("native session identity is unavailable")?;
    if value["host"] != session.host || value["sessionKey"] != session.key {
        return Err("Work Goal context belongs to another native session".into());
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work start event is missing")?;
    if start["action"] != "start" || start["eventSha256"] != value["startEventSha256"] {
        return Err("Work start identity does not match its Goal context".into());
    }
    let owner = session
        .owner()?
        .ok_or("native session Goal owner is unavailable")?;
    if owner.identity_sha256 != value["ownerIdentitySha256"]
        || owner.project_id.as_deref() != value["ownerProjectId"].as_str()
    {
        return Err("Work Goal context owner identity does not match".into());
    }
    let history = super::super::records(&session.root)?;
    if !history.iter().any(|record| {
        record["goalId"] == value["parentGoalId"]
            && record["eventSha256"] == value["parentObservedGoalEventSha256"]
    }) {
        return Err("parent Goal observation is absent from the session journal".into());
    }
    let mut linked_member = None;
    for record in history.iter().rev() {
        if let Some(member) = super::super::journey::member_for_work(record, work)? {
            linked_member = Some(member);
            break;
        }
    }
    let member = linked_member.ok_or("parent Goal has no exact Work/start membership")?;
    if member["startEventSha256"] != value["startEventSha256"]
        || member["parentGoalId"] != value["parentGoalId"]
        || member["parentObservedGoalEventSha256"] != value["parentObservedGoalEventSha256"]
        || member["host"] != value["host"]
        || member["sessionKey"] != value["sessionKey"]
        || member["ownerProjectId"] != value["ownerProjectId"]
        || member["ownerIdentitySha256"] != value["ownerIdentitySha256"]
        || crate::evidence::hash::text(start["goal"].as_str().unwrap_or_default())
            != member["workGoalSha256"]
    {
        return Err(
            "Work Goal companion does not match the exact journey member and parent observation"
                .into(),
        );
    }
    super::super::journey::validate_member_for_session(loaded, &member, &session, &owner)?;
    let parent_observation = history
        .iter()
        .position(|record| {
            record["goalId"] == value["parentGoalId"]
                && record["eventSha256"] == value["parentObservedGoalEventSha256"]
        })
        .ok_or("parent Goal observation is absent from the session journal")?;
    // Same-Goal-ID revisions may add explicit named mappings or current
    // context, and remain part of the Work's parent. Stop at the first
    // different Goal ID so an old Work never follows a successor or a later
    // reuse of the same textual ID.
    let parent = history[parent_observation..]
        .iter()
        .take_while(|record| record["goalId"] == value["parentGoalId"])
        .last()
        .ok_or("parent Goal observation is absent from the session journal")?;
    let mut parent = parent.clone();
    parent[VERIFIED_CONTEXT_FIELD] = json!({
        "kind":"context_only",
        "work":work,
        "startEventSha256":value["startEventSha256"],
    });
    let local = super::super::native::owner_for(loaded)?;
    let owner_matches =
        owner.identity_sha256 == local.identity_sha256 && owner.project_id == local.project_id;
    Ok((parent, owner_matches, session.root))
}
