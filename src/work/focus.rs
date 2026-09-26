//! Project-local current-work focus: a navigation pointer only.
//!
//! `work begin` points it at the new work so `work resume` can tell the current
//! work from older running ledgers without guessing by age. The pointer never
//! closes, accepts, authorizes, checks, reviews, or budgets any work, and an
//! explicit work locator always overrides it. Projects without it keep the
//! legacy resume behavior.

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
                return Err(invalid(&error))
            }
            #[cfg(not(unix))]
            SecureBytesResult::Unsupported(error) => return Err(invalid(&error)),
        };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid("focus is not JSON"))?;
    match (value["version"].as_u64(), value["work"].as_str()) {
        (Some(1), Some(work)) if super::valid_work_handle(work) => Ok(Focus::Work(work.to_owned())),
        _ => Err(invalid("focus has no valid work handle")),
    }
}

fn invalid(detail: &str) -> String {
    format!(
        "current-work focus is unusable ({detail}); run 'work focus WORK' with an explicit work locator to replace it"
    )
}

/// Replace the focus atomically: a failed write leaves the previous pointer
/// and every work ledger untouched.
pub(crate) fn write(loaded: &Loaded, work: &str) -> Result<(), String> {
    let target = path(loaded);
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

/// Recovery route when `work begin` committed a work but could not point the
/// focus at it.
pub(crate) fn recovery(loaded: &Loaded, work: &str, error: &str) -> Value {
    let command = loaded.path.to_str().map(|config| {
        json!([
            crate::compatibility::profile().caller,
            "work",
            "focus",
            work,
            "--config",
            config
        ])
    });
    json!({"updated": false, "error": error, "command": command})
}
