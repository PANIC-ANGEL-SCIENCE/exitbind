//! Current Works derived from the native Goal's own journey.
//!
//! After a host resume the Goal itself says which Works are open: its exact
//! journey members (and any active named mapping in the verified owner
//! project) whose run is still running. This is read-only navigation; it
//! selects nothing for an action and writes no record.

use super::hint::canonical_path;
use crate::{config, evidence::hash, session_goal};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf, time::Instant};

/// Works named in the hook text; `session recovery show` lists more.
pub(super) const HOOK_WORK_LIMIT: usize = 4;
pub(super) const SHOW_WORK_LIMIT: usize = 16;
/// Journey members are bounded at 32; mappings add at most as many again.
const MAX_CANDIDATES: usize = 64;
const MAX_ASSIGNMENTS: usize = 8;

pub(super) struct Assignment {
    pub(super) id: String,
    pub(super) agent: String,
    pub(super) role: String,
}

pub(super) struct CurrentWork {
    pub(super) work: String,
    pub(super) config_path: String,
    pub(super) product_root: PathBuf,
    pub(super) source: &'static str,
    pub(super) next_action: Option<String>,
    pub(super) assignments: Vec<Assignment>,
    /// The Work's admitted check observation without a check result:
    /// its state (`running` or `failed`) and admission event.
    pub(super) observation: Option<(String, String)>,
}

pub(super) struct Derived {
    pub(super) goal_id: String,
    pub(super) works: Vec<CurrentWork>,
    /// Open Works beyond the requested limit.
    pub(super) more: usize,
    /// Members whose project or run could not be verified now.
    pub(super) unverified: usize,
    /// The deadline passed before every candidate was read.
    pub(super) incomplete: bool,
}

pub(super) fn derive(
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    owner_loaded: Option<&config::Loaded>,
    limit: usize,
    deadline: Instant,
) -> Derived {
    let mut derived = Derived {
        goal_id: record["goalId"].as_str().unwrap_or("unknown").to_owned(),
        works: Vec::new(),
        more: 0,
        unverified: 0,
        incomplete: false,
    };
    for (work, source) in candidates(record) {
        if Instant::now() >= deadline {
            derived.incomplete = true;
            break;
        }
        let loaded = match source {
            "journey_member" => member_project(record, &work, session, owner),
            _ => owner_loaded
                .ok_or(())
                .and_then(|loaded| config::load(loaded.path.to_str()).map_err(|_| ())),
        };
        let Ok(loaded) = loaded else {
            derived.unverified += 1;
            continue;
        };
        let (assignments, observation) = match open_run(&loaded, &work) {
            Ok(Some(open)) => open,
            Ok(None) => continue,
            Err(_) => {
                derived.unverified += 1;
                continue;
            }
        };
        if derived.works.len() >= limit {
            derived.more += 1;
            continue;
        }
        let (next_action, in_time) = bounded_next_action(&loaded, &work, deadline);
        derived.incomplete |= !in_time;
        let Ok(config_path) = canonical_path(&loaded.path, "Work configuration") else {
            derived.unverified += 1;
            continue;
        };
        derived.works.push(CurrentWork {
            work,
            config_path,
            product_root: loaded.product_root.clone(),
            source,
            next_action,
            assignments,
            observation,
        });
    }
    derived
}

/// Exact journey members first (Goal order), then active mapped Works that
/// are not members.
fn candidates(record: &Value) -> Vec<(String, &'static str)> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let members = record["journeyMembers"].as_array().into_iter().flatten();
    for work in members.filter_map(|member| member["work"].as_str()) {
        if seen.insert(work.to_owned()) {
            out.push((work.to_owned(), "journey_member"));
        }
    }
    let mappings = record["continuation"]["mappings"]
        .as_array()
        .into_iter()
        .flatten();
    for mapping in mappings.filter(|mapping| mapping["active"] == true) {
        if let Some(work) = mapping["work"].as_str() {
            if seen.insert(work.to_owned()) {
                out.push((work.to_owned(), "requirement_mapping"));
            }
        }
    }
    out.truncate(MAX_CANDIDATES);
    out
}

/// The member's own project, verified against the exact recorded membership,
/// the current native session and the fixed Goal owner.
fn member_project(
    record: &Value,
    work: &str,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
) -> Result<config::Loaded, ()> {
    let member = session_goal::journey::member_for_work(record, work)
        .ok()
        .flatten()
        .ok_or(())?;
    let path = member["configPath"].as_str().ok_or(())?;
    let loaded = config::load(Some(path)).map_err(|_| ())?;
    if canonical_path(&loaded.path, "Work configuration")
        .ok()
        .as_deref()
        != Some(path)
        || member["configSha256"].as_str() != Some(hash::text(&loaded.source).as_str())
    {
        return Err(());
    }
    session_goal::journey::validate_member_for_session(&loaded, &member, session, owner)
        .map_err(|_| ())?;
    Ok(loaded)
}

type OpenRun = (Vec<Assignment>, Option<(String, String)>);

/// `Ok(None)` for a Work whose run has ended; otherwise its open assignments
/// and its admitted check observation, if one has no check result.
fn open_run(loaded: &config::Loaded, work: &str) -> Result<Option<OpenRun>, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let snapshot = crate::run::RunSnapshot::capture(loaded, &ledger)?;
    if snapshot.status() != "running" {
        return Ok(None);
    }
    let assignments = crate::run::assignment::pending(snapshot.state())
        .iter()
        .take(MAX_ASSIGNMENTS)
        .filter_map(|item| {
            Some(Assignment {
                id: crate::run::assignment::handle(work, item).ok()?,
                agent: item["agent"].as_str()?.to_owned(),
                role: item["role"].as_str().unwrap_or("agent").to_owned(),
            })
        })
        .collect();
    let observation = &snapshot.state()["checkObservation"];
    let observation = observation["state"].as_str().map(|state| {
        let event = observation["eventSha256"].as_str().unwrap_or("unknown");
        (state.to_owned(), event.to_owned())
    });
    Ok(Some((assignments, observation)))
}

/// The next action, computed on a helper thread so the caller never waits
/// past `deadline` (W1 review finding 2). A late answer is dropped; the hook
/// then names the Work with its `work detail` command and no action. The
/// helper only reads and ends with the process.
fn bounded_next_action(
    loaded: &config::Loaded,
    work: &str,
    deadline: Instant,
) -> (Option<String>, bool) {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return (None, false);
    };
    let (Some(path), work) = (loaded.path.to_str().map(str::to_owned), work.to_owned()) else {
        return (None, true);
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("exitbind-next-action".into())
        .spawn(move || {
            let action = config::load(Some(&path))
                .ok()
                .and_then(|loaded| next_action(&loaded, &work));
            let _ = sender.send(action);
        });
    if spawned.is_err() {
        return (None, true);
    }
    match receiver.recv_timeout(remaining) {
        Ok(action) => (action, true),
        Err(_) => (None, false),
    }
}

/// The action from the same compact action projection `work detail --view
/// action` prints.
fn next_action(loaded: &config::Loaded, work: &str) -> Option<String> {
    let view = crate::work::readable::action_view(loaded, work).ok()?;
    let effective = &view["effectiveAction"];
    if let Some(text) = effective["delegation"]["text"].as_str() {
        return Some(text.trim_start_matches("next action: ").to_owned());
    }
    match effective["state"].as_str() {
        Some("action") => effective["kind"].as_str().map(str::to_owned),
        Some(state) => Some(state.to_owned()),
        None => None,
    }
}

/// The `session recovery show` field: each Work with its exact read argv.
pub(super) fn show_value(derived: &Derived, executable: &str, trees: Value) -> Value {
    json!({
        "source":"native_goal_journey",
        "goalId":derived.goal_id,
        "works":derived.works.iter().map(|item| json!({
            "work":item.work,
            "relation":item.source,
            "configPath":item.config_path,
            "nextAction":item.next_action,
            "workDetailArgv":[executable,"work","detail",item.work,"--view","action","--config",item.config_path],
            "openAssignments":item.assignments.iter().map(|assignment| json!({
                "assignment":assignment.id,"agent":assignment.agent,"role":assignment.role,"state":"open"
            })).collect::<Vec<_>>(),
            "checkObservation":item.observation.as_ref().map(|(state, event)| json!({
                "state":state,"admissionEventSha256":event
            })),
        })).collect::<Vec<_>>(),
        "moreOpenWorks":derived.more,
        "unverifiedMembers":derived.unverified,
        "complete":!derived.incomplete,
        "uncommittedTrees":trees,
        "authority":"read_only",
    })
}
