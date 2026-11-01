//! Project-local current-work focus: a navigation pointer only.
//!
//! `work begin` points it at the new work so `work resume` can tell the current
//! work from older running ledgers without guessing by age. The pointer never
//! closes, accepts, authorizes, checks, reviews, or budgets any work, and an
//! explicit work locator always overrides it. Projects without it keep the
//! legacy resume behavior.

pub(crate) fn valid_work_handle(work: &str) -> bool {
    work.strip_prefix(super::WORK_PREFIX)
        .is_some_and(super::valid_token)
}

use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::config::Loaded;
use crate::project::path::{secure_bytes_observation, SecureBytesResult};

const FILE: &str = "current-work.json";

pub(crate) enum Focus {
    Absent,
    Work(String),
    /// Present but unusable; navigation then selects nothing.
    Unusable(String),
}

fn relative() -> String {
    format!("{}/{FILE}", crate::project::layout_types::state_namespace())
}

fn path(loaded: &Loaded) -> PathBuf {
    loaded.state_root.join(relative())
}

pub(crate) fn read(loaded: &Loaded) -> Result<Focus, String> {
    let bytes =
        match secure_bytes_observation(&loaded.state_root, &relative(), "current-work focus") {
            SecureBytesResult::Bytes(bytes) => bytes,
            SecureBytesResult::Absent(_) => return Ok(Focus::Absent),
            SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => {
                return Ok(Focus::Unusable(invalid(loaded, &error)))
            }
            #[cfg(not(unix))]
            SecureBytesResult::Unsupported(error) => {
                return Ok(Focus::Unusable(invalid(loaded, &error)))
            }
        };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(Focus::Unusable(invalid(loaded, "focus is not JSON")));
    };
    match (value["version"].as_u64(), value["work"].as_str()) {
        (Some(1), Some(work)) if super::valid_work_handle(work) => Ok(Focus::Work(work.to_owned())),
        _ => Ok(Focus::Unusable(invalid(
            loaded,
            "focus has no valid work handle",
        ))),
    }
}

fn invalid(loaded: &Loaded, detail: &str) -> String {
    let history =
        crate::emitted::command_line(&["work", "resume", "--history"], Some(&loaded.path));
    let focus = crate::emitted::command_line(
        &["work", "focus", &crate::emitted::slot("WORK")],
        Some(&loaded.path),
    );
    format!(
        "current-work focus is unusable ({detail}); list running work with '{history}', then run '{focus}' with an explicit work locator"
    )
}

/// Serialize all product focus writers with the existing ephemeral lock owner.
/// The anchor names a lock only; it is not a new focus or ledger record.
fn with_focus_lock<T>(
    loaded: &Loaded,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let anchor = format!(
        "{}/current-work-focus",
        crate::project::layout_types::state_namespace()
    );
    let lock = crate::run::ledger::ledger_path(&loaded.state_root, &anchor, true)?;
    crate::run::ledger::with_lock(&lock, action)
}

/// Replace the focus atomically: a failed write leaves the previous pointer
/// and every work ledger untouched.
pub(crate) fn write(loaded: &Loaded, work: &str) -> Result<(), String> {
    with_focus_lock(loaded, || write_locked(loaded, work))
}

fn write_locked(loaded: &Loaded, work: &str) -> Result<(), String> {
    let target = path(loaded);
    if fs::symlink_metadata(&target).is_ok_and(|metadata| !metadata.is_file()) {
        let focus = crate::emitted::command_line(&["work", "focus", work], Some(&loaded.path));
        return Err(format!(
            "{} is not a regular file; remove it by hand, then run '{focus}'",
            relative()
        ));
    }
    let parent = target.parent().ok_or("focus path has no parent")?;
    let staged = parent.join(format!(".{FILE}.{}.tmp", std::process::id()));
    let body = json!({"version": 1, "work": work, "authority": "none"}).to_string();
    let result = (|| -> Result<(), String> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&staged).map_err(|error| error.to_string())?;
        file.write_all(body.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        fs::rename(&staged, &target).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

/// Whether the start and native session link were completed by this call.
pub(crate) enum SuccessorState {
    FreshLinked,
    Recovery,
    LinkPending,
}

/// Hold the focus lock from observation through native start and link. If the
/// lock cannot be taken, refuse before attempting a native start.
pub(crate) fn successor<T>(
    loaded: &Loaded,
    predecessor: &str,
    work: &str,
    action: impl FnOnce() -> Result<(T, SuccessorState), String>,
) -> Result<(T, Value), String> {
    let mut action = Some(action);
    let locked = with_focus_lock(loaded, || {
        let observed = read(loaded);
        let run = action
            .take()
            .ok_or("focus transaction was already executed")?;
        let (result, state) = run()?;
        Ok((
            result,
            successor_locked(loaded, predecessor, work, observed, state),
        ))
    });
    match locked {
        Err(error) if action.is_some() => {
            let detail = crate::emitted::command_line(
                &["work", "detail", work, "--json"],
                Some(&loaded.path),
            );
            Err(format!("focus transaction unavailable ({error}); no native supersession was started by this attempt. If the successor already exists, inspect `{detail}` before recovery"))
        }
        other => other,
    }
}

fn successor_locked(
    loaded: &Loaded,
    predecessor: &str,
    work: &str,
    observed: Result<Focus, String>,
    state: SuccessorState,
) -> Value {
    let moved = match observed {
        Ok(Focus::Work(current)) if current == work => Ok(false),
        Ok(Focus::Work(current)) if current == predecessor => match state {
            SuccessorState::FreshLinked => write_locked(loaded, work).map(|()| true),
            SuccessorState::Recovery => Err(
                "successor admission already exists; select its Work explicitly before continuing"
                    .into(),
            ),
            SuccessorState::LinkPending => {
                Err("native session link is pending; successor focus was not selected".into())
            }
        },
        Ok(Focus::Work(_)) => {
            Err("current-work focus names another Work; successor focus was not selected".into())
        }
        Ok(Focus::Absent) => {
            Err("current-work focus is absent; successor focus was not selected".into())
        }
        Ok(Focus::Unusable(error)) | Err(error) => Err(error),
    };
    match moved {
        Ok(updated) => json!({"updated": updated, "current": true, "authority": "none"}),
        Err(error) => pending(loaded, work, &error),
    }
}

fn pending(loaded: &Loaded, work: &str, error: &str) -> Value {
    let mut result = recovery(loaded, work, error);
    result["current"] = json!(false);
    result
}

/// Recovery route when `work begin` committed a work but could not point the
/// focus at it.
pub(crate) fn recovery(loaded: &Loaded, work: &str, error: &str) -> Value {
    let route = crate::emitted::Route::new(["work", "focus", work])
        .config(&loaded.path)
        .within(2048);
    json!({"updated": false, "error": error, "command": route.argv_value(),
        "sameConfigRequired": route.needs_config(),
        "sameExecutableRequired": route.needs_executable()})
}

#[cfg(test)]
#[path = "focus/tests.rs"]
mod tests;
