// Renders native Goal status for host display surfaces.
// Reports an unavailable display owner; see `src/session_goal.rs`.
//! Human display state for the native host surfaces (status line, task list,
//! native goal). One small JSON file per native session is rewritten from the
//! Goal ledger after each ledger command; display readers open only this file
//! and never read the ledger. Nothing here is shown to agents and nothing here
//! carries authority: it is a projection for the human's screen.

use crate::config::Loaded;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

const DIRECTORY: &str = "display";
const KIND: &str = "exitbind_goal_display";
/// Display readers refuse larger files; the writer stays well under it.
pub(crate) const MAX_BYTES: u64 = 64 * 1024;
const MAX_TEXT_CHARS: usize = 4000;

/// Which Goal journal a display state was projected from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Journal {
    Project,
    Session,
}

impl Journal {
    fn name(self) -> &'static str {
        match self {
            Journal::Project => "project",
            Journal::Session => "session",
        }
    }
}

/// The display file for a host-reported session identity.
pub(crate) fn path_for(host: &str, identity: &str) -> Result<PathBuf, String> {
    path_for_key(&super::native::session_key(host, identity))
}

/// The display file for an already hashed native session key.
pub(crate) fn path_for_key(key: &str) -> Result<PathBuf, String> {
    Ok(super::native::base_directory()?
        .join(DIRECTORY)
        .join(format!("{key}.json")))
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

/// Build the display state from the selected journal's latest record and the
/// same presentation `goal status` uses. A closed record reads CLOSED from the
/// ledger itself, so a later tree change cannot reopen it on the screen.
pub(crate) fn build(
    loaded: &Loaded,
    root: &Path,
    journal: Journal,
    host: &str,
    now: &str,
) -> Result<Value, String> {
    let Some(record) = super::read(root)? else {
        return Ok(
            json!({"kind":KIND,"version":1,"host":host,"journal":journal.name(),
            "state":"none","revision":Value::Null,"writtenAt":now}),
        );
    };
    let limited_resume = || {
        crate::run::inputs::active_resume_budget()
            .and_then(|mut budget| budget.limited_reason())
            .is_some()
    };
    if limited_resume() {
        return Ok(metadata_only(&record, journal, host, now));
    }
    let presentation =
        super::presentation_for_selected(loaded, Some(&record), root, journal == Journal::Project)?;
    if limited_resume() {
        return Ok(metadata_only(&record, journal, host, now));
    }
    let progress = &presentation["goalProgress"];
    let closed = record["closure"]["closed"] == true;
    let requirements = progress["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|task| {
            json!({"id":task["id"],"doneWhen":bounded(task["label"].as_str().unwrap_or_default()),
                "state":if task["state"] == "complete" {"met"} else {task["state"].as_str().unwrap_or("pending")}})
        })
        .collect::<Vec<_>>();
    let met = requirements
        .iter()
        .filter(|row| row["state"] == "met")
        .count();
    let next = if closed {
        Value::Null
    } else {
        requirements
            .iter()
            .find(|row| row["state"] != "met")
            .map_or(Value::Null, |row| {
                json!(format!(
                    "{}: {}",
                    row["id"].as_str().unwrap_or_default(),
                    row["doneWhen"].as_str().unwrap_or_default()
                ))
            })
    };
    let current_work = record["continuation"]["mappings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|mapping| mapping["active"] == true)
        .filter_map(|mapping| mapping["work"].as_str())
        .last()
        .map_or(Value::Null, |work| json!(work));
    let goal = record["goal"].as_str().unwrap_or_default();
    let lessons = crate::memory::lessons::deliverable(loaded, Some(goal).filter(|g| !g.is_empty()))
        .and_then(Result::ok);
    Ok(json!({
        "kind":KIND,"version":1,"host":host,"journal":journal.name(),
        "state":if closed {"closed"} else {"open"},
        "goalId":record["goalId"],"goal":bounded(goal),
        "closingRoute":closing_route(&record, progress),
        "requirements":requirements,"met":met,"total":progress["tasks"].as_array().map_or(0, Vec::len),
        "currentWork":current_work,"nextAction":next,"lessonCount":lessons,
        "revision":record["revision"],"eventSha256":record["eventSha256"],"writtenAt":now,
    }))
}

/// The same metadata-only shape used by native recovery when current task
/// evidence is unavailable. Preserve the selected journal and Goal identity
/// without presenting historical task counts as a fresh observation.
fn metadata_only(record: &Value, journal: Journal, host: &str, now: &str) -> Value {
    json!({"kind":KIND,"version":1,"host":host,"journal":journal.name(),
        "state":if record["closure"]["closed"] == true {"closed"} else {"open"},
        "goalId":record["goalId"],"goal":bounded(record["goal"].as_str().unwrap_or_default()),
        "revision":record["revision"],"eventSha256":record["eventSha256"],"writtenAt":now})
}

fn closing_route(record: &Value, progress: &Value) -> Value {
    if let Some(text) = super::goal_summary::journey(
        record,
        progress.get("journeyCompletion"),
        false,
        super::goal_summary::task_counts(progress),
    ) {
        return json!(text);
    }
    if super::requirements::named(record) {
        return json!("closes when every requirement is met and the Lead closes the Goal");
    }
    json!("closes when every incorporated item is resolved and the Lead closes the Goal")
}

/// Rebuild the display file of `session` from the selected journal. Without
/// an explicit journal the one the file was last projected from is kept, else
/// the session journal. Writes nothing when the session journal belongs to
/// another owner.
pub(crate) fn refresh(
    loaded: &Loaded,
    session: &super::native::Session,
    journal: Option<Journal>,
) -> Result<(), String> {
    let path = path_for_key(&session.selector_key)?;
    let journal = journal.unwrap_or(match read(&path) {
        Some(previous) if previous["journal"] == "project" => Journal::Project,
        _ => Journal::Session,
    });
    let root = match journal {
        Journal::Project => loaded.state_root.clone(),
        Journal::Session => {
            let scope = super::native::scope_for(loaded, session.clone(), false)?;
            if !scope.owner_matches {
                return Ok(());
            }
            scope.root
        }
    };
    let state = build(
        loaded,
        &root,
        journal,
        session.host,
        &crate::memory::state::now(),
    )?;
    write(&path, &state)
}

/// SessionStart uses only an owner-verified, bounded Goal snapshot. It never
/// runs the ordinary full display projection or evaluates Work and lessons.
/// Failure is silent and must not interfere with the recovery prompt.
pub(crate) fn refresh_claude_session(
    loaded: &Loaded,
    identity: &str,
    recovery: Option<&(super::native::Session, super::native::RecoverySnapshot)>,
) {
    if let Some((session, snapshot)) =
        recovery.filter(|(session, _)| session.host == "claude" && session.reports(identity))
    {
        let _ = refresh_recovery_metadata(loaded, session, snapshot);
        return;
    }
    let Ok(Some(session)) = super::native::host_session_readonly("claude", identity) else {
        return;
    };
    // An explicitly selected project display is independent of this session.
    let Ok(path) = path_for_key(&session.selector_key) else {
        return;
    };
    if read(&path).is_some_and(|previous| previous["journal"] == "project") {
        return;
    }
    let Ok(Some(snapshot)) = session.recovery_snapshot(super::native::Access::ReadExisting) else {
        return;
    };
    let _ = refresh_recovery_metadata(loaded, &session, &snapshot);
}

fn refresh_recovery_metadata(
    loaded: &Loaded,
    session: &super::native::Session,
    snapshot: &super::native::RecoverySnapshot,
) -> Result<(), String> {
    let path = path_for_key(&session.selector_key)?;
    let previous = read(&path);
    if previous
        .as_ref()
        .is_some_and(|state| state["journal"] == "project")
    {
        return Ok(());
    }
    let local = super::native::owner_for(loaded)?;
    if local.identity_sha256 != snapshot.owner.identity_sha256
        || local.project_id != snapshot.owner.project_id
    {
        return Err("native Goal display owner is unavailable".into());
    }
    let state = recovery_metadata(
        session,
        snapshot.latest.as_ref(),
        previous.as_ref(),
        &crate::memory::state::now(),
    )?;
    write(&path, &state)
}

/// Reuse progress only when this display names the same session journal and
/// exact Goal event. An adopted session's display does not record the target
/// journal key, so its older progress cannot be proven to name that journal.
pub(super) fn recovery_metadata(
    session: &super::native::Session,
    record: Option<&Value>,
    previous: Option<&Value>,
    now: &str,
) -> Result<Value, String> {
    let Some(record) = record else {
        return Ok(
            json!({"kind":KIND,"version":1,"host":session.host,"journal":"session",
            "state":"none","revision":Value::Null,"writtenAt":now}),
        );
    };
    let goal = record["goal"]
        .as_str()
        .filter(|goal| !goal.trim().is_empty())
        .ok_or("native Goal display text is unavailable")?;
    let closed = record["closure"]["closed"]
        .as_bool()
        .ok_or("native Goal display closure is unavailable")?;
    let goal_id = record["goalId"]
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .ok_or("native Goal display identity is unavailable")?;
    let revision = record["revision"]
        .as_u64()
        .filter(|revision| *revision > 0)
        .ok_or("native Goal display revision is unavailable")?;
    let event = record["eventSha256"]
        .as_str()
        .filter(|event| super::native::valid_hash(event))
        .ok_or("native Goal display event is unavailable")?;
    let mut state = json!({"kind":KIND,"version":1,"host":session.host,"journal":"session",
        "state":if closed {"closed"} else {"open"},
        "goalId":goal_id,"goal":bounded(goal),
        "revision":revision,"eventSha256":event,"writtenAt":now});
    let same_event = previous.filter(|old| {
        !session.adopted()
            && old["journal"] == "session"
            && old["host"] == session.host
            && old["goalId"] == record["goalId"]
            && old["revision"] == record["revision"]
            && record["eventSha256"].as_str().is_some()
            && old["eventSha256"] == record["eventSha256"]
    });
    if let Some(previous) = same_event {
        for field in [
            "closingRoute",
            "requirements",
            "met",
            "total",
            "currentWork",
            "nextAction",
            "lessonCount",
        ] {
            if let Some(value) = previous.get(field) {
                state[field] = value.clone();
            }
        }
    }
    Ok(state)
}

/// Atomically replace the display file: write a private temporary file in the
/// same directory, flush it, then rename it over the previous state.
pub(crate) fn write(path: &Path, state: &Value) -> Result<(), String> {
    let directory = path.parent().ok_or("display state path has no parent")?;
    crate::project::portability::create_private_directory(
        directory
            .parent()
            .ok_or("display directory has no parent")?,
    )?;
    crate::project::portability::create_private_directory(directory)?;
    let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("display state exceeds its size bound".into());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("display state file name is not valid UTF-8")?;
    let temporary = directory.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = options
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

/// Read a display file for a display surface. Absent, oversized, symlinked or
/// malformed files read as `None`: a display never reports an error.
pub(crate) fn read(path: &Path) -> Option<Value> {
    let info = std::fs::symlink_metadata(path).ok()?;
    if !info.is_file() || info.len() > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    (value["kind"] == KIND && value["version"] == 1).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_start_keeps_only_progress_bound_to_its_current_journal_event() {
        let mut session = super::super::native::Session {
            host: "claude",
            key: "a".repeat(64),
            selector_key: "a".repeat(64),
            root: PathBuf::from("/fixture"),
        };
        let mut record = json!({"goalId":"g", "goal":"A Goal", "revision":3,
            "eventSha256":"b".repeat(64), "closure":{"closed":false}});
        let previous = json!({"kind":KIND,"version":1,"journal":"session","host":"claude",
            "goalId":"g","revision":3,"eventSha256":"b".repeat(64),
            "met":1,"total":2,"requirements":[{"id":"R1"}],"nextAction":"next",
            "lessonCount":3,"currentWork":"smw_test","closingRoute":"close"});
        let same = recovery_metadata(&session, Some(&record), Some(&previous), "now").unwrap();
        for field in [
            "met",
            "total",
            "requirements",
            "nextAction",
            "lessonCount",
            "currentWork",
            "closingRoute",
        ] {
            assert_eq!(same[field], previous[field], "{field}");
        }
        record["eventSha256"] = json!("c".repeat(64));
        record["closure"]["closed"] = json!(true);
        let changed = recovery_metadata(&session, Some(&record), Some(&previous), "now").unwrap();
        assert_eq!(changed["state"], "closed");
        assert_eq!(changed["eventSha256"], record["eventSha256"]);
        for field in [
            "met",
            "total",
            "requirements",
            "nextAction",
            "lessonCount",
            "currentWork",
            "closingRoute",
        ] {
            assert!(changed.get(field).is_none(), "unexpected retained {field}");
        }
        record["eventSha256"] = previous["eventSha256"].clone();
        session.key = "d".repeat(64);
        let adopted = recovery_metadata(&session, Some(&record), Some(&previous), "now").unwrap();
        assert!(adopted.get("requirements").is_none());
        let absent = recovery_metadata(&session, None, Some(&previous), "now").unwrap();
        assert_eq!(absent["state"], "none");
        assert!(absent.get("requirements").is_none());
        record["closure"]["closed"] = Value::Null;
        assert!(recovery_metadata(&session, Some(&record), Some(&previous), "now").is_err());
    }

    #[test]
    fn atomic_write_replaces_the_state_and_reads_back_within_the_time_bound() {
        let root = crate::test_root::temp("display");
        let directory = root.join("store");
        let path = directory.join(DIRECTORY).join("key.json");
        let state = json!({"kind":KIND,"version":1,"state":"open","revision":3,
            "goal":"g".repeat(MAX_TEXT_CHARS),"requirements":(0..40).map(|i| json!({"id":format!("S{i}"),
            "doneWhen":"d".repeat(400),"state":"pending"})).collect::<Vec<_>>()});
        write(&path, &state).expect("first write");
        let started = std::time::Instant::now();
        let mut next = state.clone();
        next["revision"] = json!(4);
        write(&path, &next).expect("replacement");
        let elapsed = started.elapsed();
        assert!(elapsed.as_millis() < 50, "display write took {elapsed:?}");
        assert_eq!(read(&path).expect("state")["revision"], 4);
        let leftovers = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".tmp"))
            })
            .count();
        assert_eq!(leftovers, 0);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn foreign_or_malformed_files_read_as_absent() {
        let directory = crate::test_root::temp("display");
        let path = directory.join("x.json");
        assert!(read(&path).is_none());
        std::fs::write(&path, b"{\"kind\":\"other\",\"version\":1}").unwrap();
        assert!(read(&path).is_none());
        std::fs::write(&path, b"not json").unwrap();
        assert!(read(&path).is_none());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
