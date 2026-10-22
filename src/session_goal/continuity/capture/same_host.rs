//! The receiving binding of a child started by the native session that holds
//! the Work, and the `work child prepare` route `work next` prints for it.
//!
//! A Work begun or linked in a native session already names that session and
//! its Goal owner; a same-host child needs no cross-host `--context` token.
//! The binding is derived and re-verified from the Work's native Goal context
//! (owner identity hash and project ID) on every prepare, claim, context read
//! and stop. A Work held by another session or host is refused here and keeps
//! the cross-host route.

use crate::config::Loaded;
use serde_json::{json, Value};
use std::fs;

/// The intent marker for a binding derived from the native session.
pub(super) const RECEIVING: &str = "same-host";
/// The receiving binding revision of a same-host child; it never changes.
pub(super) const REVISION: u64 = 0;
/// Approved lens files (docs/user-owned-agents.md), under the ControlRoot.
const LENSES: &str = "your.soulmate/lenses";
const MAX_LENSES: usize = 3;

fn binding_value(host: &str, identity: &str) -> Value {
    json!({"session": identity, "host": host, "revision": REVISION})
}

fn verify(loaded: &Loaded, work: &str, host: &str, identity: &str) -> Result<(), String> {
    let session = crate::session_goal::native::host_session(host, identity)?
        .ok_or("native session identity is unavailable")?;
    crate::session_goal::association::verify_owner_session(loaded, work, session)
}

/// The binding of the current native session, which must hold `work`.
pub(super) fn current(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let (host, identity) = crate::session_goal::native::current_identity()?.ok_or(
        "work child prepare without --context needs the native session that holds this Work; a Work received from another host uses the --context token from work continuation",
    )?;
    verify(loaded, work, host, &identity).map_err(|error| {
        format!("this native session does not hold the Work ({error}); a Work received from another host uses the --context token from work continuation")
    })?;
    Ok(binding_value(host, &identity))
}

/// The binding a same-host intent was prepared under, re-verified now.
pub(super) fn of_intent(loaded: &Loaded, work: &str, intent: &Value) -> Result<Value, String> {
    let (Some(host), Some(identity)) = (intent["host"].as_str(), intent["session"].as_str()) else {
        return Err("same-host child intent is malformed".into());
    };
    verify(loaded, work, host, identity)?;
    Ok(binding_value(host, identity))
}

/// Lens files offered by default: regular `.md` files whose stem is a valid
/// perspective id, in name order.
fn lenses(loaded: &Loaded) -> Vec<(String, String)> {
    let directory = loaded.control_root.join(LENSES);
    if !fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir()) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut found = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            let id = name.strip_suffix(".md")?.to_owned();
            let valid = !id.is_empty()
                && id.len() <= 64
                && id.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte)
                });
            valid.then(|| (id, format!("{LENSES}/{name}")))
        })
        .collect::<Vec<_>>();
    found.sort();
    found
}

/// The `work child prepare` route for the pending named spawn of `next`
/// (a `crate::work::next` response), when the current native session holds
/// the Work; otherwise `None`, and the cross-host route stays the way.
pub(crate) fn route(loaded: &Loaded, work: &str, next: &Value) -> Option<Value> {
    let next = &next["next"];
    let assignment = next["assignment"].as_str()?;
    let native = next["packet"]["nativeTaskName"].as_str()?;
    if next["action"] != "spawn" || current(loaded, work).is_err() {
        return None;
    }
    let mut suffix: Vec<String> = vec![
        "work".into(),
        "child".into(),
        "prepare".into(),
        work.into(),
        assignment.into(),
        "--agent-type".into(),
        native.into(),
    ];
    let lenses = lenses(loaded);
    let ids = lenses.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let selected = (1..=MAX_LENSES).contains(&lenses.len());
    if selected {
        let sources = lenses
            .iter()
            .map(|(id, path)| json!({"id": id, "path": path}))
            .collect::<Vec<_>>();
        suffix.push("--perspectives".into());
        suffix.push(Value::Array(sources).to_string());
    }
    let route = crate::work::compact::continuation_route(&loaded.path, suffix);
    let mut result = json!({
        "command": route["command"],
        "sameConfigRequired": route["sameConfigRequired"],
        "sameExecutableRequired": route["sameExecutableRequired"],
        "perspectives": if selected { json!(ids) } else { json!([]) },
        "meaning": format!("run before spawning `{native}` from this session: its first turn then receives its profile and these lenses (files under {LENSES}); remove a lens from --perspectives to narrow the choice"),
    });
    if !selected && !lenses.is_empty() {
        result["perspectivesOmitted"] = json!(format!(
            "{} lens files under {LENSES}; a child takes at most {MAX_LENSES}: add --perspectives with the chosen ones",
            lenses.len()
        ));
    }
    Some(result)
}
