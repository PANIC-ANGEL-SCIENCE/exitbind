// Resolves the context of a Goal-associated Work successor.
// Refuses malformed successor start events; see `src/session_goal/association.rs`.
use super::super::native::Access;
use super::*;
use serde_json::json;

/// A validated current caller and Goal observation, without creating a Work
/// association or treating a host-reported selector as authentication.
pub(crate) struct StandaloneObservation {
    pub(crate) caller: Value,
    goal_event_sha256: String,
}

pub(crate) enum ReplacementScope {
    Bound,
    ValidatedCurrentCallerUnbound(StandaloneObservation),
    Unavailable(String),
    Invalid,
}

pub(crate) fn standalone_observation(
    loaded: &crate::config::Loaded,
    work: &str,
) -> Result<StandaloneObservation, String> {
    let session = super::super::native::current_with_access(Access::RequireWritable)?
        .ok_or("a current native caller is required for standalone replacement")?;
    let history = super::super::records(&session.root)?;
    standalone_observation_for(loaded, work, session, Access::RequireWritable, &history)
}

fn standalone_observation_for(
    loaded: &crate::config::Loaded,
    work: &str,
    session: super::super::native::Session,
    access: Access,
    history: &[Value],
) -> Result<StandaloneObservation, String> {
    if companion(loaded, work)?.is_some() || legacy_goal_is_bound(loaded, work, &session, access)? {
        return Err("the Work already has a native or legacy Goal relation".into());
    }
    let owner = session
        .owner_with_access(access)?
        .ok_or("the current native Goal owner is unavailable")?;
    let current = history
        .last()
        .ok_or("the current native Goal is unavailable")?;
    super::super::journey::require_unbound_replacement_coverage(loaded, current, work)?;
    for record in history {
        if super::super::progress::record_bound_to_work(record, work) {
            return Err("the Work already has a native Goal relation".into());
        }
        if let Some(member) =
            super::super::journey::member_for_work_with_lineage(loaded, record, work)?
        {
            super::super::journey::validate_member_for_session(loaded, &member, &session, &owner)?;
            return Err("the Work already has a native Goal member or successor relation".into());
        }
    }
    let event = current["eventSha256"]
        .as_str()
        .filter(|value| valid_sha(value))
        .ok_or("the current native Goal observation is invalid")?;
    let root = session
        .root
        .to_str()
        .ok_or("the current native Goal root is not valid UTF-8")?;
    Ok(StandaloneObservation {
        caller: json!({"host":session.host,"sessionKey":session.key,
            "selectorKey":session.selector_key,"rootSha256":crate::evidence::hash::text(root),
            "ownerIdentitySha256":owner.identity_sha256,
            "ownerProjectSha256":crate::evidence::hash::value(&json!(owner.project_id))}),
        goal_event_sha256: event.to_owned(),
    })
}

pub(crate) fn classify_replacement_scope(
    loaded: &crate::config::Loaded,
    work: &str,
) -> ReplacementScope {
    match context_for_work(loaded, work) {
        WorkContext::Owner(_) | WorkContext::Session(_) | WorkContext::Local => {
            ReplacementScope::Bound
        }
        WorkContext::Invalid => ReplacementScope::Invalid,
        WorkContext::Unavailable | WorkContext::NotRequested => {
            match standalone_observation(loaded, work) {
                Ok(observation) => ReplacementScope::ValidatedCurrentCallerUnbound(observation),
                Err(error) => ReplacementScope::Unavailable(error),
            }
        }
    }
}

pub(crate) fn replacement_scope_projection(loaded: &crate::config::Loaded, work: &str) -> Value {
    match classify_replacement_scope(loaded, work) {
        ReplacementScope::Bound => json!({
            "state":"bound", "standaloneAvailable":false,
            "reason":"an existing native or local Goal relation must be preserved"}),
        ReplacementScope::Invalid => json!({"state":"invalid", "standaloneAvailable":false,
            "reason":"the current Goal association is malformed or foreign"}),
        ReplacementScope::ValidatedCurrentCallerUnbound(_) => json!({
            "state":"validated_current_caller_unbound", "standaloneAvailable":true,
            "reason":"the current caller and Goal owner are valid and no applicable Work relation was found"}),
        ReplacementScope::Unavailable(reason) => {
            json!({"state":"unavailable", "standaloneAvailable":false,
            "reason":reason})
        }
    }
}

/// Hash the preview's exact decision inputs; this is not a permanent Goal
/// revision requirement on the committed successor.
pub(crate) fn scope_decision_sha256(
    loaded: &crate::config::Loaded,
    work: &str,
    relation: &Value,
    observation: &StandaloneObservation,
) -> String {
    let mut unsigned = relation.clone();
    if let Some(object) = unsigned.as_object_mut() {
        object.remove("scopeDecisionSha256");
    }
    crate::evidence::hash::value(&json!({"version":1,"work":work,
        "configPath":loaded.path,"configSha256":crate::evidence::hash::text(&loaded.source),
        "productRoot":loaded.product_root,"controlRoot":loaded.control_root,
        "stateRoot":loaded.state_root,"projectId":loaded.project_id,
        "goalEventSha256":observation.goal_event_sha256,"decision":unsigned}))
}

/// Recheck the exact preview while holding the Goal journal lock through the
/// predecessor claim. The caller already holds the predecessor Work lock.
pub(crate) fn with_standalone_claim<T>(
    loaded: &crate::config::Loaded,
    work: &str,
    relation: &Value,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let session = super::super::native::current_with_access(Access::RequireWritable)?
        .ok_or("a current native caller is required for standalone replacement")?;
    let root = session.root.clone();
    super::super::with_history(&root, |history| {
        if std::fs::read_to_string(&loaded.path).map_err(|error| error.to_string())?
            != loaded.source
        {
            return Err("project configuration changed after preview; no claim was created".into());
        }
        let binding = relation["scope"]["currentBinding"]
            .as_str()
            .ok_or("standalone scope has no current Work binding")?;
        crate::work::details::ensure_action_binding(loaded, work, Some(binding))?;
        let observation =
            standalone_observation_for(loaded, work, session, Access::RequireWritable, history)?;
        if relation["scope"]["caller"] != observation.caller
            || relation["scopeDecisionSha256"].as_str()
                != Some(scope_decision_sha256(loaded, work, relation, &observation).as_str())
        {
            return Err(
                "standalone scope decision changed after preview; no claim was created".into(),
            );
        }
        action()
    })
}

pub(crate) fn durable_standalone_scope(
    loaded: &crate::config::Loaded,
    work: &str,
    access: Access,
) -> Result<bool, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let Some(marker) = events
        .first()
        .and_then(|start| start.get("checkReplacement"))
    else {
        return Ok(false);
    };
    if marker["protocol"].as_u64() != Some(2) {
        return Ok(false);
    }
    // RunSnapshot validates the claimed predecessor/start pair, not just the
    // marker's shape, before the marker affects association resolution.
    crate::run::RunSnapshot::capture(loaded, &ledger)?;
    let session = super::super::native::current_with_access(access)?
        .ok_or("the standalone successor's native caller is unavailable")?;
    let history = super::super::records(&session.root)?;
    let observation = standalone_observation_for(loaded, work, session, access, &history)?;
    // A verified adopted child may have its own selector while reading the
    // same owner journal. The selector was bound at preview/apply; subsequent
    // lifecycle reads require the same validated journal and owner, not that
    // every participant reuse the Lead's selector.
    let caller = &marker["scope"]["caller"];
    if [
        "host",
        "sessionKey",
        "rootSha256",
        "ownerIdentitySha256",
        "ownerProjectSha256",
    ]
    .iter()
    .any(|field| caller[field] != observation.caller[field])
    {
        return Err("the standalone successor's caller or Goal owner changed".into());
    }
    Ok(true)
}

/// Resolve an optional native-session context without making malformed or
/// foreign context a failure of ordinary Work detail and recovery routes.
pub(crate) fn context_for_work(loaded: &crate::config::Loaded, work: &str) -> WorkContext {
    context_for_work_with_root(loaded, work).context
}

pub(crate) fn context_for_work_with_root(
    loaded: &crate::config::Loaded,
    work: &str,
) -> super::WorkContextResolution {
    context_for_work_with_access(loaded, work, Access::RequireWritable)
}

pub(crate) fn context_for_work_readonly(
    loaded: &crate::config::Loaded,
    work: &str,
) -> super::WorkContextResolution {
    context_for_work_with_access(loaded, work, Access::ReadExisting)
}

fn context_for_work_with_access(
    loaded: &crate::config::Loaded,
    work: &str,
    access: Access,
) -> super::WorkContextResolution {
    match durable_standalone_scope(loaded, work, access) {
        Ok(true) => return resolution(WorkContext::NotRequested, None),
        Ok(false) => {}
        Err(_) => return resolution(WorkContext::Invalid, None),
    }
    let value = match companion(loaded, work) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return match super::super::native::current_with_access(access) {
                Ok(None) => match super::super::read(&loaded.state_root) {
                    Ok(None) => resolution(WorkContext::NotRequested, None),
                    Ok(Some(_)) => resolution(WorkContext::Local, None),
                    Err(_) => resolution(WorkContext::Invalid, None),
                },
                Ok(Some(session)) => {
                    let legacy_bound = match legacy_goal_is_bound(loaded, work, &session, access) {
                        Ok(bound) => bound,
                        Err(_) => return resolution(WorkContext::Invalid, None),
                    };
                    if legacy_bound {
                        resolution(WorkContext::Local, None)
                    } else {
                        match (
                            session.owner_with_access(access),
                            super::super::read(&session.root),
                        ) {
                            (Ok(None), Ok(None)) => resolution(WorkContext::NotRequested, None),
                            (Err(_), _) | (_, Err(_)) => resolution(WorkContext::Invalid, None),
                            (Ok(Some(_)), Ok(Some(_))) => {
                                match native_successor_context(loaded, work, &session, access) {
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
    let resolved = super::super::native::current_with_access(access)
        .and_then(|session| session.ok_or_else(|| "native session identity is unavailable".into()))
        .and_then(|session| resolve_companion(loaded, work, &value, session, access));
    match resolved {
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
    access: Access,
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
        .owner_with_access(access)?
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
    let scope = super::super::native::scope_with_access(loaded, access)?;
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
    access: Access,
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
        .owner_with_access(access)?
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

fn companion(loaded: &crate::config::Loaded, work: &str) -> Result<Option<Value>, String> {
    read_private_companion(
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
    )
}

/// `Ok` only when `work`'s native Goal context companion names `session` and
/// this project is that Goal's owner, verified by owner identity hash and
/// project ID through the same reader as the Work's Goal context. The session
/// may come from the environment or from a hook payload.
pub(crate) fn verify_owner_session(
    loaded: &crate::config::Loaded,
    work: &str,
    session: super::super::native::Session,
) -> Result<(), String> {
    let value = companion(loaded, work)?
        .ok_or("this Work has no native session Goal context; it was not begun or linked in a native session")?;
    let (_, owner_matches, _) =
        resolve_companion(loaded, work, &value, session, Access::RequireWritable)?;
    if !owner_matches {
        return Err(
            "this Work's native session Goal is owned by another configured project".into(),
        );
    }
    Ok(())
}

fn resolve_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    value: &Value,
    session: super::super::native::Session,
    access: Access,
) -> Result<(Value, bool, std::path::PathBuf), String> {
    validate_companion(value)?;
    if value["work"].as_str() != Some(work) {
        return Err("Work Goal context companion names another Work".into());
    }
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
        .owner_with_access(access)?
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
