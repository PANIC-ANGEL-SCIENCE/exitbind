//! Carry a previous project's accepted, unexpired lessons into a new project.
//!
//! The previous project is only read. Each carried lesson becomes a new
//! immutable source bound to the new project identity, with provenance naming
//! the previous project, item and source digest, and is recorded through the
//! ordinary Lead lifecycle (propose, review, promote). No store is added.
use super::*;
use crate::project::path::SecureBytesResult;
use std::path::Path;

const CARRY_DIRECTORY: &str = "lessons";

/// A guard and what carrying it needs in the new project.
struct GuardPlan {
    path: String,
    sha256: String,
    copy: bool,
}

struct Candidate {
    id: String,
    item_id: String,
    source_sha256: String,
    expires_at: String,
    lesson: Value,
    owner: String,
    agents: Vec<String>,
    guards: Vec<GuardPlan>,
}

pub(crate) struct Plan {
    previous_root: std::path::PathBuf,
    previous_identity: String,
    candidates: Vec<Candidate>,
    excluded: Vec<Value>,
}

impl Plan {
    /// Configured agents that read at least one carried lesson.
    pub(crate) fn recipients(&self) -> BTreeSet<String> {
        self.candidates
            .iter()
            .flat_map(|item| item.agents.iter().cloned())
            .collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// The read-only preview: what would be carried and what is not, with reasons.
    pub(crate) fn preview(&self) -> Value {
        json!({
            "previousProject": self.previous_identity,
            "carried": self.candidates.iter().map(|item| json!({
                "id": item.id,
                "previousItemId": item.item_id,
                "previousSourceSha256": item.source_sha256,
                "expiresAt": item.expires_at,
                "guards": guard_view(&item.guards),
            })).collect::<Vec<_>>(),
            "notCarried": self.excluded,
        })
    }
}

fn guard_view(guards: &[GuardPlan]) -> Vec<Value> {
    guards
        .iter()
        .map(
            |guard| json!({"path": guard.path, "action": if guard.copy { "copy" } else { "kept" }}),
        )
        .collect()
}

fn excluded(id: Option<&str>, item_id: &str, reason: &str, path: Option<&str>) -> Value {
    let mut value = json!({"id": id, "previousItemId": item_id, "reason": reason});
    if let Some(path) = path {
        value["path"] = json!(path);
    }
    value
}

/// Read-only classification of every project lesson in the previous project.
/// `new_root` is only observed for guard currentness.
pub(crate) fn plan(previous: &Loaded, new_root: &Path) -> Result<Plan, String> {
    let mut plan = Plan {
        previous_root: previous.product_root.clone(),
        previous_identity: crate::host::assignment_context::project_identity(previous)?,
        candidates: Vec::new(),
        excluded: Vec::new(),
    };
    let now = chrono::Utc::now().fixed_offset();
    for ledger in super::super::discovery::discover(previous)?.unwrap_or_default() {
        for item in ledger.snapshot.items.values() {
            if item["scope"] != SCOPE {
                continue;
            }
            let item_id = item["itemId"].as_str().unwrap_or_default();
            let source_path = item["source"]["path"].as_str().unwrap_or_default();
            let bytes = match path::secure_bytes_observation_bounded(
                &previous.product_root,
                source_path,
                "previous lesson source",
                MAX_ENTRY as u64,
            ) {
                SecureBytesResult::Bytes(bytes)
                    if hash::bytes(&bytes) == item["source"]["sha256"] =>
                {
                    Some(bytes)
                }
                _ => None,
            };
            let lesson = bytes
                .as_deref()
                .and_then(|bytes| parse(previous, bytes).ok());
            let id = lesson.as_ref().map(|lesson| lesson.id.clone());
            let id = id.as_deref();
            let state = item["state"].as_str().unwrap_or("unknown");
            let expiry = item["expiresAt"].as_str().unwrap_or_default();
            let expired = chrono::DateTime::parse_from_rfc3339(expiry).map_or(true, |at| at <= now);
            let reason = match state {
                "accepted" if expired => Some("expired"),
                "accepted" => None,
                "expired" => Some("expired"),
                "revoked" => Some("revoked"),
                "rejected" => Some("rejected"),
                _ => Some("not_accepted"),
            };
            if let Some(reason) = reason {
                plan.excluded.push(excluded(id, item_id, reason, None));
                continue;
            }
            let (Some(bytes), Some(lesson)) = (bytes, lesson) else {
                plan.excluded.push(excluded(
                    id,
                    item_id,
                    "source_changed_in_previous_project",
                    Some(source_path),
                ));
                continue;
            };
            let mut guards = Vec::new();
            let mut stale = None;
            for guard in &lesson.guards {
                let here = path::secure_digest_observation(new_root, &guard.path, "lesson guard")?;
                match here {
                    Some(digest) if digest == guard.sha256 => guards.push(GuardPlan {
                        path: guard.path.clone(),
                        sha256: guard.sha256.clone(),
                        copy: false,
                    }),
                    Some(_) => {
                        stale = Some(("guard_changed_needs_revalidation", guard.path.as_str()));
                        break;
                    }
                    None => {
                        let there = path::secure_digest_observation(
                            &previous.product_root,
                            &guard.path,
                            "previous lesson guard",
                        )?;
                        if there.as_deref() != Some(guard.sha256.as_str()) {
                            stale =
                                Some(("guard_changed_in_previous_project", guard.path.as_str()));
                            break;
                        }
                        guards.push(GuardPlan {
                            path: guard.path.clone(),
                            sha256: guard.sha256.clone(),
                            copy: true,
                        });
                    }
                }
            }
            if let Some((reason, guard)) = stale {
                plan.excluded
                    .push(excluded(Some(&lesson.id), item_id, reason, Some(guard)));
                continue;
            }
            plan.candidates.push(Candidate {
                id: lesson.id.clone(),
                item_id: item_id.to_owned(),
                source_sha256: hash::bytes(&bytes),
                expires_at: expiry.to_owned(),
                lesson: serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
                owner: lesson.owner.clone(),
                agents: lesson.applies_to.agents.clone(),
                guards,
            });
        }
    }
    Ok(plan)
}

/// The new immutable source: same lesson fields and expiry, the new project
/// identity, and provenance naming the previous project, item and source.
fn carried_source(loaded: &Loaded, plan: &Plan, item: &Candidate) -> Result<Vec<u8>, String> {
    let identity = crate::host::assignment_context::project_identity(loaded)?;
    let mut from = format!("Carried by setup from project {}", plan.previous_identity);
    if from.len() > 160 {
        from = "Carried by setup from a previous project (identity exceeds the provenance bound)"
            .into();
    }
    let carried = [
        from,
        format!("Previous item {}", item.item_id),
        format!("Previous source sha256 {}", item.source_sha256),
    ];
    let original = item.lesson["provenance"][0].as_str().map(str::to_owned);
    for keep_original in [true, false] {
        let mut lesson = item.lesson.clone();
        let object = lesson
            .as_object_mut()
            .ok_or("lesson source is not an object")?;
        object.remove("supersedes");
        object.insert("projectIdentity".into(), json!(identity));
        let provenance = original
            .iter()
            .filter(|_| keep_original)
            .cloned()
            .chain(carried.iter().cloned())
            .collect::<Vec<_>>();
        object.insert("provenance".into(), json!(provenance));
        let mut bytes = serde_json::to_vec_pretty(&lesson).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        if bytes.len() <= MAX_ENTRY {
            return Ok(bytes);
        }
    }
    Err("carried lesson exceeds the source bound".into())
}

/// Write `bytes` once at a confined project path; an identical existing file is kept.
fn write_once(root: &Path, relative: &str, bytes: &[u8], label: &str) -> Result<(), String> {
    match path::secure_bytes_observation(root, relative, label) {
        SecureBytesResult::Bytes(existing) if existing == bytes => return Ok(()),
        SecureBytesResult::Absent(_) => {}
        SecureBytesResult::Bytes(_) => {
            return Err(format!(
                "{label} already exists with different bytes: {relative}"
            ))
        }
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => {
            return Err(error)
        }
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => return Err(error),
    }
    if let Some(parent) = Path::new(relative).parent() {
        crate::project::managed_files::ensure_managed_directory(root, &root.join(parent))?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        let mut file = path::secure_create_new(root, relative, label)?;
        file.write_all(bytes)
            .map_err(|error| format!("{label}: {error}"))?;
        file.sync_all().map_err(|error| format!("{label}: {error}"))
    }
    #[cfg(not(unix))]
    {
        Err(format!("{label} carry-over requires a POSIX host"))
    }
}

/// Accepted project lessons already present in the new project, by lesson id.
fn accepted_ids(loaded: &Loaded) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut ids = std::collections::BTreeMap::new();
    for ledger in super::super::discovery::discover(loaded)?.unwrap_or_default() {
        for item in ledger.snapshot.items.values() {
            if item["scope"] != SCOPE || item["state"] != "accepted" {
                continue;
            }
            let source = item["source"]["path"].as_str().unwrap_or_default();
            if let SecureBytesResult::Bytes(bytes) =
                path::secure_bytes_observation(&loaded.product_root, source, "lesson source")
            {
                if let Ok(lesson) = parse(loaded, &bytes) {
                    ids.insert(lesson.id, hash::bytes(&bytes));
                }
            }
        }
    }
    Ok(ids)
}

/// Record each planned lesson in the new project as its configured Lead.
/// `loaded` must already carry the project-lessons memory policy and rights.
pub(crate) fn apply(loaded: &Loaded, plan: Plan) -> Result<Value, String> {
    let lead = loaded
        .lead()
        .ok_or("lesson carry-over requires a configured Lead")?;
    let policy = super::super::policy::get(&loaded.config)
        .filter(|policy| policy.protocol_scopes.contains(SCOPE))
        .ok_or("lesson carry-over requires the project-lessons memory scope")?;
    let namespace = crate::project::layout_types::state_namespace();
    let existing = accepted_ids(loaded)?;
    let mut carried = Vec::new();
    let mut not_carried = plan.excluded.clone();
    for item in &plan.candidates {
        let refuse = |reason: &str| excluded(Some(&item.id), &item.item_id, reason, None);
        if Some(item.owner.as_str()) != Some(lead) {
            not_carried.push(refuse("owner_is_not_the_configured_lead"));
            continue;
        }
        if item
            .agents
            .iter()
            .any(|agent| loaded.agent(agent).is_none())
        {
            not_carried.push(refuse("recipient_not_configured"));
            continue;
        }
        let bytes = carried_source(loaded, &plan, item)?;
        parse(loaded, &bytes)?;
        let digest = hash::bytes(&bytes);
        let stem = format!("{}-{}", item.id, &item.source_sha256[..12]);
        let source = format!("{namespace}/{CARRY_DIRECTORY}/{stem}.json");
        let ledger = format!("{}/{stem}.jsonl", policy.root.trim_end_matches('/'));
        match existing.get(&item.id) {
            Some(present) if *present == digest => {
                carried.push(json!({"id": item.id, "previousItemId": item.item_id,
                    "source": source, "ledger": ledger, "state": "already_carried",
                    "guards": guard_view(&item.guards)}));
                continue;
            }
            Some(_) => {
                not_carried.push(refuse("accepted_lesson_with_same_id_exists"));
                continue;
            }
            None => {}
        }
        for guard in item.guards.iter().filter(|guard| guard.copy) {
            let bytes =
                path::secure_bytes(&plan.previous_root, &guard.path, "previous lesson guard")?;
            if hash::bytes(&bytes) != guard.sha256 {
                return Err(format!(
                    "previous lesson guard changed during carry-over: {}",
                    guard.path
                ));
            }
            write_once(
                &loaded.product_root,
                &guard.path,
                &bytes,
                "carried lesson guard",
            )?;
        }
        crate::project::git_preflight::refuse_tracked_targets(
            &loaded.product_root,
            &[loaded.product_root.join(&source).as_path()],
        )?;
        write_once(
            &loaded.product_root,
            &source,
            &bytes,
            "carried lesson source",
        )?;
        crate::project::managed_files::ensure_state_directory(
            &loaded.state_root,
            &loaded.state_root.join(&policy.root),
        )?;
        let snapshot = super::super::read_ledger(loaded, &ledger, true)?;
        let state = snapshot.items.values().next().map(|value| {
            (
                value["state"].as_str().unwrap_or("").to_owned(),
                value["source"]["sha256"] == digest.as_str(),
            )
        });
        let steps: &[&str] = match state.as_ref().map(|(state, same)| (state.as_str(), *same)) {
            None => &["propose", "review", "promote"],
            Some(("proposed", true)) => &["review", "promote"],
            Some(("reviewed", true)) => &["promote"],
            _ => {
                not_carried.push(refuse("ledger_conflict"));
                continue;
            }
        };
        let mut item_id = None;
        for step in steps {
            let response = if *step == "propose" {
                super::super::action(
                    loaded,
                    lead,
                    step,
                    Some(&source),
                    Some(SCOPE),
                    &ledger,
                    Some(&item.expires_at),
                )?
            } else {
                super::super::action(loaded, lead, step, None, None, &ledger, None)?
            };
            item_id = response["event"]["itemId"].as_str().map(str::to_owned);
        }
        carried.push(
            json!({"id": item.id, "previousItemId": item.item_id, "itemId": item_id,
            "source": source, "sourceSha256": digest, "ledger": ledger, "state": "accepted",
            "expiresAt": item.expires_at, "guards": guard_view(&item.guards)}),
        );
    }
    Ok(
        json!({"previousProject": plan.previous_identity, "carried": carried, "notCarried": not_carried}),
    )
}
