//! Durable project facts reuse the opted-in memory lifecycle, never Work authority.
use crate::{config::Loaded, evidence::hash, project::path};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) const SCOPE: &str = "project-lessons.v1";
const MAX_ENTRY: usize = 2048;
const MAX_ITEMS: usize = 4;
const MAX_DELIVERY: usize = 6 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Lesson {
    version: u32,
    pub(crate) id: String,
    fact: String,
    project_identity: String,
    owner: String,
    provenance: Vec<String>,
    created_revision: String,
    revalidated_revision: String,
    applies_to: Applicability,
    guards: Vec<Guard>,
    #[serde(default)]
    supersedes: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Applicability {
    agents: Vec<String>,
    task_terms: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    path: String,
    sha256: String,
}

fn text(value: &str, bound: usize) -> bool {
    !value.trim().is_empty() && value.len() <= bound && !value.chars().any(char::is_control)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn revision(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub(crate) fn mutation_lock(loaded: &Loaded) -> Result<Option<std::fs::File>, String> {
    if !super::policy::get(&loaded.config)
        .is_some_and(|policy| policy.protocol_scopes.contains(SCOPE))
    {
        return Ok(None);
    }
    #[cfg(not(unix))]
    {
        Err("project lesson mutations require a POSIX memory host".into())
    }
    #[cfg(unix)]
    {
        use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
        let root = loaded
            .state_root
            .join(crate::project::layout_types::state_namespace())
            .join("locks");
        crate::project::managed_files::ensure_state_directory(&loaded.state_root, &root)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join("project-lessons.lock"))
            .map_err(|error| error.to_string())?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(
                "project lessons are being changed; reacquire after the current mutation ends"
                    .into(),
            );
        }
        Ok(Some(file))
    }
}

pub(crate) fn parse(loaded: &Loaded, bytes: &[u8]) -> Result<Lesson, String> {
    if bytes.len() > MAX_ENTRY {
        return Err("project lesson exceeds the 2048-byte entry bound".into());
    }
    let lesson: Lesson = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid project lesson: {error}"))?;
    let applies = &lesson.applies_to;
    if lesson.version != 1
        || !text(&lesson.id, 64)
        || !lesson
            .id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        || !text(&lesson.fact, 512)
        || lesson.project_identity != crate::host::assignment_context::project_identity(loaded)?
        || Some(lesson.owner.as_str()) != loaded.lead()
        || lesson.provenance.is_empty()
        || lesson.provenance.len() > 4
        || !lesson.provenance.iter().all(|value| text(value, 160))
        || !revision(&lesson.created_revision)
        || !revision(&lesson.revalidated_revision)
        || applies.agents.is_empty()
        || applies.agents.len() > 8
        || applies
            .agents
            .iter()
            .any(|name| loaded.agent(name).is_none())
        || applies.agents.iter().collect::<BTreeSet<_>>().len() != applies.agents.len()
        || applies.task_terms.len() > 4
        || !applies.task_terms.iter().all(|value| text(value, 64))
        || lesson.guards.is_empty()
        || lesson.guards.len() > 4
        || lesson.guards.iter().any(|guard| !digest(&guard.sha256))
        || lesson
            .supersedes
            .as_deref()
            .is_some_and(|value| !digest(value))
    {
        return Err("invalid project lesson identity, provenance, applicability or guard".into());
    }
    // Validate confinement even when a guard is absent or stale. No machine paths
    // or transient Work identifiers are needed in the project fact itself.
    for guard in &lesson.guards {
        if !text(&guard.path, 256)
            || std::path::Path::new(&guard.path).is_absolute()
            || !std::path::Path::new(&guard.path)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err("lesson guard must be a confined relative project path".into());
        }
    }
    Ok(lesson)
}

pub(crate) fn applicable(
    loaded: &Loaded,
    lesson: &Lesson,
    agent: &str,
    task: Option<&str>,
) -> Result<bool, String> {
    if !lesson.applies_to.agents.iter().any(|name| name == agent) {
        return Ok(false);
    }
    let task_matches = task.is_some_and(|task| {
        let task = task.to_lowercase();
        lesson
            .applies_to
            .task_terms
            .iter()
            .any(|term| task.contains(&term.to_lowercase()))
    });
    if !lesson.applies_to.task_terms.is_empty() && !task_matches {
        return Ok(false);
    }
    for guard in &lesson.guards {
        match path::secure_bytes_observation(&loaded.product_root, &guard.path, "lesson guard") {
            path::SecureBytesResult::Bytes(bytes) if hash::bytes(&bytes) == guard.sha256 => {}
            path::SecureBytesResult::Bytes(_) | path::SecureBytesResult::Absent(_) => {
                return Ok(false)
            }
            path::SecureBytesResult::Unsafe(reason)
            | path::SecureBytesResult::Unreadable(reason) => return Err(reason),
            #[cfg(not(unix))]
            path::SecureBytesResult::Unsupported(reason) => return Err(reason),
        }
    }
    Ok(true)
}

pub(crate) fn validate_transition(
    loaded: &Loaded,
    actor: &str,
    action: &str,
    source: &Value,
    expiry: Option<&str>,
) -> Result<(), String> {
    if !super::policy::get(&loaded.config)
        .is_some_and(|policy| policy.protocol_scopes.contains(SCOPE))
    {
        return Err("project lessons require the opted-in protocol scope".into());
    }
    if Some(actor) != loaded.lead() {
        return Err("durable project lessons are owned by the configured Lead".into());
    }
    if expiry.is_none() {
        return Err("project lessons require an explicit expires-at boundary".into());
    }
    let bytes = path::secure_bytes(
        &loaded.product_root,
        source["path"].as_str().ok_or("lesson source missing")?,
        "lesson source",
    )?;
    let lesson = parse(loaded, &bytes)?;
    if matches!(action, "propose" | "review" | "promote")
        && !applicable(
            loaded,
            &lesson,
            &lesson.applies_to.agents[0],
            Some(&lesson.applies_to.task_terms.join(" ")),
        )?
    {
        return Err("project lesson guards are stale; revalidate before promotion".into());
    }
    if action == "promote" {
        for ledger in super::discovery::discover(loaded)?.unwrap_or_default() {
            for item in ledger.snapshot.items.values() {
                if item["scope"] != SCOPE || item["state"] != "accepted" {
                    continue;
                }
                let other_bytes = path::secure_bytes(
                    &loaded.product_root,
                    item["source"]["path"]
                        .as_str()
                        .ok_or("lesson source missing")?,
                    "lesson source",
                )?;
                let other = parse(loaded, &other_bytes)?;
                if other.id == lesson.id && item["state"] == "accepted" {
                    return Err("retire the accepted lesson before promoting its correction".into());
                }
            }
        }
        if let Some(previous) = lesson.supersedes.as_deref() {
            let mut found = false;
            for ledger in super::discovery::discover(loaded)?.unwrap_or_default() {
                if let Some(item) = ledger.snapshot.items.get(previous) {
                    if item["scope"] != SCOPE
                        || !matches!(
                            item["state"].as_str(),
                            Some("revoked" | "rejected" | "expired")
                        )
                    {
                        break;
                    }
                    let prior_bytes = path::secure_bytes(
                        &loaded.product_root,
                        item["source"]["path"]
                            .as_str()
                            .ok_or("retired lesson source missing")?,
                        "retired lesson",
                    )?;
                    if hash::bytes(&prior_bytes) != item["source"]["sha256"] {
                        return Err("retired lesson source changed".into());
                    }
                    let prior = parse(loaded, &prior_bytes)?;
                    found =
                        prior.id == lesson.id && prior.created_revision == lesson.created_revision;
                    break;
                }
            }
            if !found {
                return Err("lesson supersedes must reference its retired predecessor with the same stable identity".into());
            }
        }
    }
    Ok(())
}

/// Complete selected records and an explicit inspection route for each omitted
/// record. The cap includes metadata, rather than only the fact's text.
pub(crate) fn delivery(loaded: &Loaded, agent: &str, task: &str) -> Result<Option<Value>, String> {
    let references = super::selection::resolve_for_task(loaded, agent, Some(task))?;
    let mut items = Vec::new();
    let mut omitted = 0usize;
    for reference in references
        .into_iter()
        .filter(|reference| reference["scope"] == SCOPE)
    {
        let source = reference["sourcePath"]
            .as_str()
            .ok_or("lesson source missing")?;
        let bytes = path::secure_bytes(&loaded.product_root, source, "lesson source")?;
        if hash::bytes(&bytes) != reference["sourceSha256"] {
            return Err("lesson changed during recipient delivery".into());
        }
        let lesson: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let item = json!({"reference": reference, "lesson": lesson});
        let mut attempted = items.clone();
        attempted.push(item.clone());
        if items.len() < MAX_ITEMS
            && serde_json::to_vec(&attempted)
                .map_err(|error| error.to_string())?
                .len()
                <= MAX_DELIVERY
        {
            items.push(item);
        } else {
            omitted += 1;
        }
    }
    if items.is_empty() && omitted == 0 {
        return Ok(None);
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let command = vec![
        executable
            .to_str()
            .ok_or("lesson inspector executable is not UTF-8")?,
        "memory",
        "resolve",
        agent,
        "--task",
        task,
        "--json",
        "--config",
        loaded
            .path
            .to_str()
            .ok_or("lesson configuration is not UTF-8")?,
    ];
    let mut value = json!({"items":items,"omittedCount":omitted,
        "maxItems":MAX_ITEMS,"maxBytes":MAX_DELIVERY,
        "meaning":"reviewed durable project facts; no permission, check, review or acceptance authority",
        "detail":{"command":command,"readOnly":true,"sameConfigRequired":false,"sameExecutableRequired":false}});
    while serde_json::to_vec(&value)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_DELIVERY
    {
        let list = value["items"]
            .as_array_mut()
            .ok_or("lesson delivery items missing")?;
        if list.pop().is_none() {
            return Err("lesson inspection route exceeds delivery bound".into());
        }
        omitted += 1;
        value["omittedCount"] = json!(omitted);
    }
    Ok(Some(value))
}
