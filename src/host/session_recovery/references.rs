use super::hint::{
    bounded_text, canonical_path, current_executable_sha256, read_hint, source_root, valid_sha,
    PendingOperation, Reference, WorkRef, WorkSelection, MAX_ITEMS,
};
use crate::{config, evidence::hash, session_goal};
use serde_json::Value;
use std::{collections::BTreeSet, path::PathBuf};

/// Read-only target projection for callers that need the validated project
/// roots of a retained Goal member. A Work locator here is not a current-work
/// selection or action binding.
pub(crate) struct ValidatedTaskTarget {
    pub(crate) loaded: config::Loaded,
    /// Stable storage representative only; never a current Work selection.
    pub(crate) storage_work: Option<String>,
}

pub(crate) fn task_target(
    explicit_config: Option<&str>,
) -> Result<Option<ValidatedTaskTarget>, String> {
    let Some(session) = session_goal::native::current()? else {
        let Some(path) = explicit_config else {
            return Ok(None);
        };
        let loaded = config::load(Some(path))?;
        return Ok(Some(ValidatedTaskTarget {
            loaded,
            storage_work: None,
        }));
    };
    let owner = session
        .owner()?
        .ok_or("native session Goal owner is unavailable")?;
    let record = session_goal::read_for_recovery(&session.root)?
        .ok_or("native session has no canonical Goal to recover")?;
    let hint = read_hint(&session.root)?.ok_or(
        "no validated recovery hint identifies a retained Work configuration for this session",
    )?;
    if hint.host != session.host
        || hint.session_key != session.key
        || hint.owner_identity_sha256 != owner.identity_sha256
        || hint.owner_project_id != owner.project_id
        || hint.goal_id != record["goalId"]
    {
        return Err("saved recovery hint belongs to another session or Goal".into());
    }
    let owner_loaded = config::load(Some(&hint.owner_config_path))?;
    if canonical_path(&owner_loaded.path, "native owner configuration")? != hint.owner_config_path
        || hash::text(&owner_loaded.source) != hint.owner_config_sha256
    {
        return Err("saved native owner configuration changed".into());
    }
    let scope = session_goal::native::scope(&owner_loaded, false)?;
    if !scope.owner_matches
        || scope
            .owner
            .as_ref()
            .map(|value| value.identity_sha256.as_str())
            != Some(owner.identity_sha256.as_str())
    {
        return Err("saved configuration no longer matches the fixed native Goal owner".into());
    }
    let mut validated_configs = validated_work_configs(&record, &session, &owner, &hint.work_refs)?;
    if validated_configs.is_empty() {
        return Err("the native Goal has no retained validated Work configuration target".into());
    }
    let target_key =
        |loaded: &config::Loaded| -> Result<(String, String, String, Option<String>), String> {
            Ok((
                canonical_path(&loaded.product_root, "Work product root")?,
                canonical_path(&loaded.control_root, "Work control root")?,
                canonical_path(&loaded.state_root, "Work state root")?,
                loaded.project_id.clone(),
            ))
        };
    let first_key = target_key(&validated_configs[0].1)?;
    for (_, loaded) in &validated_configs[1..] {
        if target_key(loaded)? != first_key {
            return Err("retained Goal members resolve to conflicting project targets".into());
        }
    }
    validated_configs.sort_by(|(left_work, left), (right_work, right)| {
        left.path
            .as_os_str()
            .cmp(right.path.as_os_str())
            .then_with(|| left_work.cmp(right_work))
    });
    let storage_work = validated_configs.first().map(|(work, _)| work.clone());
    let loaded = if let Some(path) = explicit_config {
        let explicit = config::load(Some(path))?;
        let explicit_path = canonical_path(&explicit.path, "explicit Work configuration")?;
        if target_key(&explicit)? != first_key {
            return Err(
                "explicit configuration conflicts with the validated Goal-member target".into(),
            );
        }
        if let Some(index) = validated_configs.iter().position(|(_, member)| {
            canonical_path(&member.path, "Work configuration")
                .is_ok_and(|member_path| member_path == explicit_path)
        }) {
            validated_configs.remove(index).1
        } else {
            explicit
        }
    } else {
        validated_configs.remove(0).1
    };
    Ok(Some(ValidatedTaskTarget {
        loaded,
        storage_work,
    }))
}

pub(super) fn validated_work_configs(
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    refs: &[WorkRef],
) -> Result<Vec<(String, config::Loaded)>, String> {
    if refs.len() > MAX_ITEMS {
        return Err("session recovery accepts at most eight Work references".into());
    }
    let mut seen = BTreeSet::new();
    let mut loaded_members = Vec::with_capacity(refs.len());
    for item in refs {
        bounded_text(&item.work, "Work locator", 128)?;
        if !seen.insert(item.work.clone())
            || !valid_sha(&item.config_sha256)
            || !valid_sha(&item.start_event_sha256)
            || item.relation != "member"
        {
            return Err("session recovery Work reference is duplicated or malformed".into());
        }
        let member = session_goal::journey::member_for_work(record, &item.work)?
            .ok_or("session recovery Work locator is not an exact native Goal member")?;
        if member["configPath"].as_str() != Some(item.config_path.as_str())
            || member["configSha256"].as_str() != Some(item.config_sha256.as_str())
            || member["startEventSha256"].as_str() != Some(item.start_event_sha256.as_str())
        {
            return Err(
                "session recovery Work reference does not match its exact Goal membership".into(),
            );
        }
        let loaded = config::load(Some(&item.config_path))?;
        if canonical_path(&loaded.path, "Work configuration")? != item.config_path
            || hash::text(&loaded.source) != item.config_sha256
        {
            return Err("session recovery Work configuration changed".into());
        }
        session_goal::journey::validate_member_for_session(&loaded, &member, session, owner)?;
        loaded_members.push((item.work.clone(), loaded));
    }
    Ok(loaded_members)
}

pub(super) fn verify_work_refs(
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    refs: &[WorkRef],
) -> Result<(), String> {
    validated_work_configs(record, session, owner, refs).map(|_| ())
}

pub(super) fn reference_root(
    loaded: &config::Loaded,
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    work_refs: &[WorkRef],
    root_name: &str,
    work: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(work) = work {
        let reference = work_refs
            .iter()
            .find(|reference| reference.work == work)
            .ok_or("reference Work is not an exact retained Goal member")?;
        let member = session_goal::journey::member_for_work(record, work)?
            .ok_or("reference Work is not an exact native Goal member")?;
        if member["configPath"].as_str() != Some(reference.config_path.as_str())
            || member["configSha256"].as_str() != Some(reference.config_sha256.as_str())
            || member["startEventSha256"].as_str() != Some(reference.start_event_sha256.as_str())
        {
            return Err("reference Work configuration does not match exact Goal membership".into());
        }
        let member_loaded = config::load(Some(&reference.config_path))?;
        if canonical_path(&member_loaded.path, "Work configuration")? != reference.config_path
            || hash::text(&member_loaded.source) != reference.config_sha256
        {
            return Err("reference Work configuration changed".into());
        }
        session_goal::journey::validate_member_for_session(
            &member_loaded,
            &member,
            session,
            owner,
        )?;
        Ok(source_root(&member_loaded, root_name)?.to_path_buf())
    } else {
        Ok(source_root(loaded, root_name)?.to_path_buf())
    }
}

pub(super) fn verify_source_refs(
    loaded: &config::Loaded,
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    work_refs: &[WorkRef],
    refs: &[Reference],
) -> Result<(), String> {
    if refs.len() > MAX_ITEMS {
        return Err("session recovery accepts at most eight source references".into());
    }
    if refs.is_empty() {
        return Ok(());
    }
    let executable = current_executable_sha256()?;
    for item in refs {
        // Generated graph artifacts are prior observations, with their own
        // bounded target/source validation and query route. They are never
        // caller-reported reads and never qualify as guidance consumption.
        if item.consumption == "generated_query_result" {
            continue;
        }
        if !source_ref_current(loaded, record, session, owner, work_refs, item, &executable)? {
            return Err(format!(
                "session recovery source changed or is unavailable: {}/{}",
                item.root, item.path
            ));
        }
    }
    Ok(())
}

pub(super) fn source_ref_current(
    loaded: &config::Loaded,
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    work_refs: &[WorkRef],
    item: &Reference,
    executable: &str,
) -> Result<bool, String> {
    if item.consumption == "generated_query_result" {
        let restored = generated_result_info(loaded, record, session, owner, work_refs, item)?;
        return Ok(restored["sourceCurrent"].as_bool() == Some(true));
    }
    let root = reference_root(
        loaded,
        record,
        session,
        owner,
        work_refs,
        &item.root,
        item.work.as_deref(),
    )?;
    use crate::project::path::SecureBytesResult;
    match crate::project::path::secure_bytes_observation_single_link_bounded(
        &root,
        &item.path,
        "session recovery referenced source",
        256 * 1024,
    ) {
        SecureBytesResult::Bytes(bytes) => Ok(hash::bytes(&bytes) == item.sha256
            && item.producer_executable_sha256 == executable
            && item.consumption == "caller_reported_read"),
        SecureBytesResult::Absent(_) => Ok(false),
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => Err(error),
    }
}

pub(super) fn generated_result_info(
    _owner_loaded: &config::Loaded,
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    work_refs: &[WorkRef],
    item: &Reference,
) -> Result<Value, String> {
    if item.consumption != "generated_query_result" || item.root != "state" {
        return Err("reference is not a generated graph result".into());
    }
    let work = item
        .work
        .as_deref()
        .ok_or("generated graph result has no exact Work locator")?;
    let configs = validated_work_configs(record, session, owner, work_refs)?;
    let member = configs
        .iter()
        .find(|(locator, _)| locator == work)
        .map(|(_, loaded)| loaded)
        .ok_or("generated graph result Work is not an exact validated Goal member")?;
    crate::host::codegraph::validate_saved_result(
        member,
        &item.path,
        &item.sha256,
        work,
        &item.producer_executable_sha256,
    )
}

pub(super) fn verify_pending(
    loaded: &config::Loaded,
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    work_refs: &[WorkRef],
    pending: Option<&PendingOperation>,
) -> Result<(), String> {
    let Some(PendingOperation::EvidenceFile {
        root: root_name,
        path,
        work,
        sha256,
        bytes: expected_bytes,
        ..
    }) = pending
    else {
        return Ok(());
    };
    let root = reference_root(
        loaded,
        record,
        session,
        owner,
        work_refs,
        root_name,
        work.as_deref(),
    )?;
    use crate::project::path::SecureBytesResult;
    match crate::project::path::secure_bytes_observation_single_link_bounded(
        &root,
        path,
        "pending session recovery evidence",
        *expected_bytes,
    ) {
        SecureBytesResult::Bytes(bytes)
            if bytes.len() as u64 == *expected_bytes && hash::bytes(&bytes) == *sha256 =>
        {
            Ok(())
        }
        SecureBytesResult::Bytes(_) => Err("pending session recovery evidence changed".into()),
        SecureBytesResult::Absent(_) => {
            Err("pending session recovery evidence is unavailable".into())
        }
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => Err(error),
    }
}

pub(super) fn known_work_locator<'a>(
    selection: &WorkSelection,
    refs: &'a [WorkRef],
) -> Result<Option<&'a WorkRef>, String> {
    if selection.state != "known" {
        return Ok(None);
    }
    let work = selection
        .work
        .as_deref()
        .ok_or("known Work locator is missing")?;
    let item = refs
        .iter()
        .find(|item| item.work == work)
        .ok_or("known Work locator must be an exact retained Goal member reference")?;
    Ok(Some(item))
}
