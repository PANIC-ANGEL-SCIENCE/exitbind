//! Explicit adoption of this project's open Goal journal by a new native
//! session of the same owner (`goal adopt`), and the one route every
//! UNAVAILABLE Goal renderer prints when such a journal is reachable.
//!
//! The project already points back at its journal: each Work's Goal context
//! sidecar names the session key of the journal it joined. A journal is
//! adoptable only when its owner record has this project's owner identity,
//! its Goal is open, and it is the only such journal those sidecars name.

use super::native::{self, Owner, Session};
use crate::config::Loaded;
use serde_json::{json, Value};
use std::collections::BTreeSet;

mod record;
pub(crate) use record::{preserve_same_goal, validate_record};

/// The native-session reasons a Goal renderer reports as UNAVAILABLE. Every
/// renderer of these reasons goes through [`unavailable`], which carries the
/// adoption route when one exists.
#[derive(Clone, Copy)]
pub(crate) enum Reason {
    Unavailable,
    ContextInvalid,
}

impl Reason {
    fn code(self) -> &'static str {
        match self {
            Reason::Unavailable => "native_session_goal_unavailable",
            Reason::ContextInvalid => "native_session_goal_context_invalid",
        }
    }
}

/// An UNAVAILABLE Goal progress view for a native-session reason, with the
/// adoption route in `adoptRoute` and on the system text line when this
/// project has exactly one open Goal journal in an earlier session.
pub(crate) fn unavailable(
    loaded: &Loaded,
    result: Option<&Value>,
    reason: Reason,
    detail: &str,
) -> Value {
    let mut view = super::progress::unavailable(result, reason.code(), detail, None, "unavailable");
    if let Some(route) = route(loaded, false) {
        if let Some(text) = view["systemText"].as_str() {
            view["systemText"] = json!(format!("{text} | {}", route.line));
        }
        view["adoptRoute"] = adopt_route(loaded, &route);
    }
    view
}

/// The SessionStart text when the current session has no owner record: the
/// existing sentence, then the adoption route when the launch project has
/// one adoptable journal.
pub(crate) fn hook_text(cwd_config_path: Option<&str>) -> String {
    let base = "Exitbind session recovery is unresolved: the current native Goal owner record is absent or unavailable; no Work was selected.";
    let route = cwd_config_path
        .and_then(|path| crate::config::load(Some(path)).ok())
        .and_then(|loaded| route(&loaded, true));
    match route {
        Some(route) => format!("{base}\n{}", route.line),
        None => base.to_owned(),
    }
}

pub(crate) struct Route {
    pub(crate) line: String,
    route: crate::emitted::Route,
    goal_id: String,
    revision: u64,
}

/// The `adoptRoute` value: the bounded `goal adopt` route and the Goal it
/// adopts.
fn adopt_route(loaded: &Loaded, adoption: &Route) -> Value {
    let mut value = crate::work::compact::continuation_route(&loaded.path, adoption.route.clone());
    value["goalId"] = json!(adoption.goal_id);
    value["revision"] = json!(adoption.revision);
    value
}

/// The full `goal adopt` route for the current session, or `None` when the
/// session already has an owner record, has no native identity, or this
/// project does not name exactly one open journal of the same owner.
pub(crate) fn route(loaded: &Loaded, bounded: bool) -> Option<Route> {
    let session = native::current().ok()??;
    if session.adopted() || session.owner().ok()?.is_some() {
        return None;
    }
    let reach = reach(loaded, &session, bounded).ok()?;
    let [journal] = reach.open.as_slice() else {
        return None;
    };
    let route =
        crate::emitted::Route::new(["goal", "adopt", "--session-key", journal.key.as_str()]);
    let command = route.clone().config(&loaded.path).shell_text();
    let goal = crate::run_presentation::inert(&journal.goal_id);
    Some(Route {
        line: format!("Adopt Goal {goal}, open in an earlier session of this project: {command}"),
        route,
        goal_id: journal.goal_id.clone(),
        revision: journal.revision,
    })
}

struct Journal {
    key: String,
    goal_id: String,
    revision: u64,
}

/// What this project's Work sidecars reach: the session keys they name for
/// this owner and host, and which of those journals hold an open Goal.
struct Reach {
    named: BTreeSet<String>,
    open: Vec<Journal>,
}

fn reach(loaded: &Loaded, session: &Session, bounded: bool) -> Result<Reach, String> {
    let local = native::owner_for(loaded)?;
    let named = named_keys(loaded, session.host, &local)?;
    let mut open = Vec::new();
    for key in &named {
        if *key == session.selector_key {
            continue;
        }
        let candidate = Session::for_key(session.host, key)?;
        if !candidate
            .owner()
            .ok()
            .flatten()
            .is_some_and(|owner| same(&owner, &local))
        {
            continue;
        }
        let latest = if bounded {
            super::read_for_recovery(&candidate.root)
        } else {
            super::read(&candidate.root)
        };
        let Ok(Some(latest)) = latest else {
            continue;
        };
        if super::goal_summary::closed_revision(&latest).is_none() {
            open.push(Journal {
                key: key.clone(),
                goal_id: latest["goalId"].as_str().unwrap_or_default().to_owned(),
                revision: latest["revision"].as_u64().unwrap_or(0),
            });
        }
    }
    Ok(Reach { named, open })
}

fn same(left: &Owner, right: &Owner) -> bool {
    left.identity_sha256 == right.identity_sha256 && left.project_id == right.project_id
}

/// Session keys the project's valid Work Goal context sidecars name for this
/// host and owner identity. An unreadable or invalid sidecar names nothing.
fn named_keys(loaded: &Loaded, host: &str, local: &Owner) -> Result<BTreeSet<String>, String> {
    const MAX_SIDECARS: usize = 4096;
    let namespace = crate::project::layout_types::state_namespace();
    let runs = loaded.state_root.join(namespace).join("runs");
    let entries = match std::fs::read_dir(runs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut keys = BTreeSet::new();
    let mut seen = 0usize;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with("work-") || !name.ends_with(".session-goal.json") {
            continue;
        }
        seen += 1;
        if seen > MAX_SIDECARS {
            break;
        }
        let Some(value) = sidecar(loaded, &format!("{namespace}/runs/{name}")) else {
            continue;
        };
        if value["host"] == host
            && value["ownerIdentitySha256"] == local.identity_sha256.as_str()
            && value["ownerProjectId"].as_str() == local.project_id.as_deref()
        {
            if let Some(key) = value["sessionKey"].as_str() {
                keys.insert(key.to_owned());
            }
        }
    }
    Ok(keys)
}

fn sidecar(loaded: &Loaded, relative: &str) -> Option<Value> {
    use crate::project::path::SecureBytesResult;
    let SecureBytesResult::Bytes(bytes) =
        crate::project::path::secure_bytes_observation_single_link_bounded(
            &loaded.state_root,
            relative,
            "Work Goal context companion",
            4096,
        )
    else {
        return None;
    };
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    super::association::validate_companion(&value).ok()?;
    let mut unsigned = value.clone();
    let digest = unsigned.as_object_mut()?.remove("sha256")?;
    (crate::evidence::hash::value(&unsigned) == digest.as_str()?).then_some(value)
}

/// `goal adopt --session-key KEY`: the current session reads and writes the
/// open Goal journal `KEY` of this project from now on. The journal records
/// the adoption, then this session's own root records the journal it reads.
/// Every refusal names what differed and writes nothing.
pub(crate) fn adopt(loaded: &Loaded, journal_key: &str) -> Result<Value, String> {
    let refuse = |what: String| Err(format!("{what}; nothing was written"));
    if !native::valid_hash(journal_key) {
        return refuse("--session-key must be the 64-character session key the route names".into());
    }
    let session = native::current()?
        .ok_or("goal adopt needs a host-reported native session identity; nothing was written")?;
    if session.adopted() && session.key != journal_key {
        return refuse(format!(
            "this session already adopted the Goal journal {}",
            session.key
        ));
    }
    if session.selector_key == journal_key {
        return refuse("that session key is this session's own".into());
    }
    if !session.adopted() {
        if let Some(owner) = session.owner()? {
            let goal = super::read(&session.root)?;
            let foreign = if same(&owner, &native::owner_for(loaded)?) {
                ""
            } else {
                "native session Goal is owned by another configured project; "
            };
            return refuse(match goal {
                Some(goal) => format!(
                    "{foreign}this session already owns Goal {}",
                    goal["goalId"].as_str().unwrap_or("unknown")
                ),
                None => format!(
                    "this session already has an owner record for project {}",
                    owner.project_id.as_deref().unwrap_or("(no project ID)")
                ),
            });
        }
    }
    let target = Session::for_key(session.host, journal_key)?;
    let owner = target
        .owner()?
        .ok_or("no Goal journal of this host has that session key; nothing was written")?;
    let local = native::owner_for(loaded)?;
    if !same(&owner, &local) {
        return refuse(format!(
            "that Goal journal belongs to another project: its owner identity {} differs from this project's {}",
            owner.identity_sha256, local.identity_sha256
        ));
    }
    let reach = reach(loaded, &session, false)?;
    if !reach.named.contains(journal_key) {
        return refuse("no Work of this project names that session key".into());
    }
    let latest = super::read(&target.root)?
        .ok_or("that session has no Goal journal; nothing was written")?;
    if let Some(revision) = super::goal_summary::closed_revision(&latest) {
        return refuse(format!(
            "Goal {} is closed (revision {revision})",
            latest["goalId"].as_str().unwrap_or("unknown")
        ));
    }
    if reach.open.len() > 1 {
        let goals = reach
            .open
            .iter()
            .map(|journal| journal.goal_id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return refuse(format!(
            "this project reaches {} open Goal journals ({goals}); adopt none by guess",
            reach.open.len()
        ));
    }
    let goal_id = latest["goalId"].clone();
    let recorded = record::append(&target.root, &goal_id, &session, &owner)?;
    if !session.adopted() {
        native::Session::for_key(session.host, &session.selector_key)?
            .write_adoption(&owner, journal_key)?;
    }
    Ok(json!({
        "kind":"native_goal_adopted",
        "goalId":recorded.record["goalId"],
        "revision":recorded.record["revision"],
        "effect":if recorded.appended { "recorded" } else { "unchanged" },
        "next":crate::emitted::goal_status(&loaded.path),
    }))
}
