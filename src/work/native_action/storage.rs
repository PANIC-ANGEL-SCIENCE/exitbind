//! Native journal locations, locking, and saved-operation selection.

use super::*;

pub(super) fn journal_paths(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    substituted: bool,
) -> Result<Paths, String> {
    if !crate::work::valid_work_handle(work) {
        return Err("native action work handle is invalid".into());
    }
    let root = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("native-actions")
        .join(work);
    managed_files::ensure_state_directory(&loaded.state_root, &root)?;
    let key = journal_key(assignment, substituted);
    Ok(Paths {
        journal: root.join(format!("{key}.json")),
        schema: root.join(format!("{key}.schema.json")),
        lock: root.join(format!("{key}.lock")),
    })
}

/// Locate an existing journal without creating the state directory or lock.
pub(super) fn journal_paths_readonly(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    substituted: bool,
) -> Result<Option<Paths>, String> {
    if !crate::work::valid_work_handle(work) {
        return Err("native action work handle is invalid".into());
    }
    let root = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("native-actions")
        .join(work);
    let relative = root
        .strip_prefix(&loaded.state_root)
        .map_err(|_| "native journal path escapes project root")?;
    let mut cursor = loaded.state_root.clone();
    for component in relative.components() {
        cursor.push(component);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err("native journal directory is not a regular directory".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        }
    }
    let key = journal_key(assignment, substituted);
    Ok(Some(Paths {
        journal: root.join(format!("{key}.json")),
        schema: root.join(format!("{key}.schema.json")),
        lock: root.join(format!("{key}.lock")),
    }))
}

pub(super) fn journal_key(assignment: &str, substituted: bool) -> String {
    if substituted {
        format!("{assignment}-fallback")
    } else {
        assignment.to_owned()
    }
}

pub(super) struct ResumeSource {
    pub(super) assignment: String,
    pub(super) thread_id: String,
    _lock: JournalLock,
}

pub(super) fn find_resume_source(
    paths: &Paths,
    role: &str,
    agent: &str,
    current_attempt: Option<u64>,
) -> Result<Option<ResumeSource>, String> {
    if role == "reviewer" {
        return Err(
            "reviewer resume is refused; a reviewer must use a fresh native session".into(),
        );
    }
    let current_attempt =
        current_attempt.ok_or("current native assignment has no attempt for safe resume")?;
    let directory = paths
        .journal
        .parent()
        .ok_or("native assignment journal has no parent")?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path == paths.journal
            || path.extension().and_then(|value| value.to_str()) != Some("json")
            || path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.ends_with(".schema.json"))
        {
            continue;
        }
        let prior_lock = acquire_lock(&path.with_extension("lock"))?;
        let Some(journal) = read_journal(&path)? else {
            continue;
        };
        if journal["role"] != role
            || journal["agent"] != agent
            || !matches!(journal["status"].as_str(), Some("started" | "completed"))
        {
            continue;
        }
        let Some(attempt) = journal["attempt"].as_u64() else {
            continue;
        };
        let Some(thread_id) = journal["threadId"].as_str() else {
            continue;
        };
        let Some(assignment) = journal["assignment"].as_str() else {
            continue;
        };
        if attempt >= current_attempt || thread_id.is_empty() || assignment.is_empty() {
            continue;
        }
        candidates.push((
            attempt,
            assignment.to_owned(),
            thread_id.to_owned(),
            prior_lock,
        ));
    }
    candidates.sort_by_key(|candidate| candidate.0);
    if let Some(newest) = candidates.last().map(|candidate| candidate.0) {
        if candidates
            .iter()
            .filter(|candidate| candidate.0 == newest)
            .count()
            > 1
        {
            return Err("safe native resume is ambiguous across prior assignments".into());
        }
    }
    Ok(candidates
        .pop()
        .map(|(_, assignment, thread_id, lock)| ResumeSource {
            assignment,
            thread_id,
            _lock: lock,
        }))
}

pub(super) struct JournalLock(File);

pub(super) fn acquire_lock(path: &Path) -> Result<JournalLock, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            return Err("native assignment is already claimed by another process".into());
        }
    }
    Ok(JournalLock(file))
}
