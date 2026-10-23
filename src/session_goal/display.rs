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
    let presentation =
        super::presentation_for_selected(loaded, Some(&record), root, journal == Journal::Project)?;
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

/// Claude Code SessionStart (startup, resume, clear, compact): rebuild the
/// display file of the hook payload's session so the status line shows the
/// Goal before the next ledger command. Display only; every failure is silent.
pub(crate) fn refresh_claude_session(loaded: &Loaded, identity: &str) {
    if let Ok(Some(session)) = super::native::claude_session(identity) {
        let _ = refresh(loaded, &session, None);
    }
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
