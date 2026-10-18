//! Tested-input identity for v6 checked runs.
//!
//! Coverage: every regular file and symlink under the product root that Git
//! would consider (tracked plus untracked, non-ignored files) or, outside a Git
//! worktree, every file found by walking the product root. Exitbind state
//! directories and `.git` are excluded. Ignored files, files outside the
//! product root, the environment, remote services, and time are not covered;
//! evidence reuse never claims them.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

pub(crate) const COVERAGE: &str = "product-root-files-v1";

/// Confirm that a declared checker file actually participates in the existing
/// product fingerprint; an ignored or external file cannot establish reuse.
pub(crate) fn covered_file(loaded: &crate::config::Loaded, path: &str) -> Result<bool, String> {
    let root = &loaded.product_root;
    let wanted = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    let paths = match crate::project::git_preflight::worktree_root(root)? {
        Some(_) => git_paths(root)?,
        None => walk_paths(root)?,
    };
    Ok(paths
        .into_iter()
        .filter(|relative| !excluded(root, &loaded.state_root, relative))
        .any(|relative| {
            root.join(relative)
                .canonicalize()
                .is_ok_and(|candidate| candidate == wanted)
        }))
}

const EXCLUDED_DIRS: [&str; 3] = [".git", ".exitbind", ".soulmate"];

/// Limits and successful fingerprint memoization owned by one read-only
/// `work resume` request. Normal callers use `fingerprint` and stay unbounded.
pub(crate) struct ResumeInputBudget {
    deadline: Instant,
    directory_entries: usize,
    work_ledgers: usize,
    cached: Option<(PathBuf, PathBuf, &'static str, String)>,
    limited: Option<&'static str>,
    detail: Option<String>,
}

impl ResumeInputBudget {
    pub(crate) fn new() -> Self {
        Self::with_deadline(Instant::now() + Duration::from_millis(3000))
    }

    fn with_deadline(deadline: Instant) -> Self {
        Self {
            deadline,
            directory_entries: 0,
            work_ledgers: 0,
            cached: None,
            limited: None,
            detail: None,
        }
    }

    pub(crate) fn check(&mut self) -> Result<(), &'static str> {
        if self.limited.is_none() && Instant::now() >= self.deadline {
            self.limited = Some("resume_deadline_exceeded");
        }
        self.limited.map_or(Ok(()), Err)
    }

    pub(crate) fn count_directory_entry(&mut self) -> Result<(), &'static str> {
        self.check()?;
        self.directory_entries = self.directory_entries.saturating_add(1);
        if self.directory_entries > 1024 {
            self.limited = Some("directory_entry_limit_exceeded");
            return Err("directory_entry_limit_exceeded");
        }
        Ok(())
    }

    pub(crate) fn count_work_ledger(&mut self) -> usize {
        self.work_ledgers = self.work_ledgers.saturating_add(1);
        self.work_ledgers
    }

    pub(crate) fn directory_entries(&self) -> usize {
        self.directory_entries
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(crate) fn work_ledgers(&self) -> usize {
        self.work_ledgers
    }

    pub(crate) fn limited_reason(&mut self) -> Option<&'static str> {
        let _ = self.check();
        self.limited
    }

    pub(crate) fn mark_limited(&mut self, reason: &'static str) {
        self.limited = Some(reason);
    }

    pub(crate) fn mark_unavailable(&mut self, detail: String) {
        self.limited = Some("tested_inputs_unavailable");
        self.detail = Some(detail);
    }

    pub(crate) fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

/// Digest of the current tested inputs, or an error when they cannot be
/// established (which callers must treat as "not reusable").
pub(crate) fn fingerprint(loaded: &crate::config::Loaded) -> Result<String, String> {
    fingerprint_impl(loaded, None)
}

pub(crate) fn fingerprint_for_resume(
    loaded: &crate::config::Loaded,
    budget: &mut ResumeInputBudget,
) -> Result<String, String> {
    budget.check().map_err(str::to_owned)?;
    if let Some((root, state_root, coverage, digest)) = &budget.cached {
        if root == &loaded.product_root && state_root == &loaded.state_root && *coverage == COVERAGE
        {
            return Ok(digest.clone());
        }
    }
    let digest = match fingerprint_impl(loaded, Some(budget)) {
        Ok(digest) => digest,
        Err(error) => {
            let reason = match error.as_str() {
                "git_listing_deadline_exceeded" => Some("git_listing_deadline_exceeded"),
                "git_listing_output_limit_exceeded" => Some("git_listing_output_limit_exceeded"),
                "fingerprint_path_limit_exceeded" => Some("fingerprint_path_limit_exceeded"),
                "fingerprint_directory_limit_exceeded" => {
                    Some("fingerprint_directory_limit_exceeded")
                }
                "fingerprint_byte_limit_exceeded" => Some("fingerprint_byte_limit_exceeded"),
                "resume_deadline_exceeded" => Some("resume_deadline_exceeded"),
                _ => None,
            };
            if let Some(reason) = reason {
                budget.mark_limited(reason);
            }
            return Err(error);
        }
    };
    budget.cached = Some((
        loaded.product_root.clone(),
        loaded.state_root.clone(),
        COVERAGE,
        digest.clone(),
    ));
    Ok(digest)
}

fn fingerprint_impl(
    loaded: &crate::config::Loaded,
    mut budget: Option<&mut ResumeInputBudget>,
) -> Result<String, String> {
    let root = &loaded.product_root;
    let worktree = if let Some(budget) = budget.as_deref_mut() {
        match crate::project::git_preflight::worktree_root_bounded(root, budget.deadline()) {
            Ok(worktree) => worktree,
            Err(error) => {
                if error.starts_with("git_classification_deadline_exceeded") {
                    budget.mark_limited("git_classification_deadline_exceeded");
                } else if error.starts_with("git_classification_cancelled") {
                    budget.mark_limited("resume_cancelled");
                } else if error.starts_with("git_classification_stdout_limit_exceeded")
                    || error.starts_with("git_classification_stderr_limit_exceeded")
                {
                    budget.mark_limited("git_classification_output_limit_exceeded");
                } else if error.starts_with("git_classification_cleanup_incomplete") {
                    budget.mark_limited("git_classification_cleanup_incomplete");
                }
                return Err(error);
            }
        }
    } else {
        crate::project::git_preflight::worktree_root(root)?
    };
    let mut paths = match worktree {
        Some(_) => match budget.as_deref_mut() {
            Some(budget) => git_paths_bounded(root, budget)?,
            None => git_paths(root)?,
        },
        None => match budget.as_deref_mut() {
            Some(budget) => walk_paths_bounded(root, budget)?,
            None => walk_paths(root)?,
        },
    };
    paths.retain(|path| !excluded(root, &loaded.state_root, path));
    paths.sort();
    paths.dedup();
    let mut entries = Vec::with_capacity(paths.len());
    let mut aggregate_bytes = 0u64;
    for (index, relative) in paths.into_iter().enumerate() {
        if let Some(budget) = budget.as_deref_mut() {
            budget.check().map_err(str::to_owned)?;
            if index >= 4096 {
                return Err("fingerprint_path_limit_exceeded".into());
            }
        }
        let text = relative
            .to_str()
            .ok_or("tested input paths must be UTF-8")?
            .to_owned();
        let full = root.join(&relative);
        let entry = match fs::symlink_metadata(&full) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                json!([text, "deleted"])
            }
            Err(error) => return Err(format!("tested input cannot be read: {error}")),
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = fs::read_link(&full).map_err(|error| error.to_string())?;
                let target = target
                    .to_str()
                    .ok_or("tested input symlink targets must be UTF-8")?;
                json!([text, "symlink", crate::evidence::hash::text(target)])
            }
            Ok(meta) if meta.is_file() => {
                let digest = if let Some(budget) = budget.as_deref_mut() {
                    let size = meta.len();
                    aggregate_bytes = aggregate_bytes.saturating_add(size);
                    if aggregate_bytes > 16 * 1024 * 1024 {
                        return Err("fingerprint_byte_limit_exceeded".into());
                    }
                    budget.check().map_err(str::to_owned)?;
                    hash_file_bounded(&full, size, budget)?
                } else {
                    crate::evidence::hash::file(&full)?
                };
                json!([text, "file", digest])
            }
            Ok(_) => continue,
        };
        entries.push(entry);
    }
    Ok(crate::evidence::hash::value(
        &json!({"coverage": COVERAGE, "entries": Value::Array(entries)}),
    ))
}

fn hash_file_bounded(
    path: &Path,
    expected_len: u64,
    budget: &mut ResumeInputBudget,
) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut observed = 0u64;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        budget.check().map_err(str::to_owned)?;
        let count = file.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        observed = observed.saturating_add(count as u64);
        if observed > expected_len || observed > 16 * 1024 * 1024 {
            return Err("fingerprint_byte_limit_exceeded".into());
        }
        hasher.update(&chunk[..count]);
    }
    if observed != expected_len {
        return Err("tested input changed while fingerprinting".into());
    }
    let bytes = hasher.finalize();
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("String formatting cannot fail");
    }
    Ok(output)
}

fn excluded(root: &Path, state_root: &Path, relative: &Path) -> bool {
    let first = relative
        .components()
        .next()
        .and_then(|part| part.as_os_str().to_str());
    if first.is_some_and(|name| EXCLUDED_DIRS.contains(&name)) {
        return true;
    }
    state_root != root && root.join(relative).starts_with(state_root)
}

fn git_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            ".",
        ])
        .output()
        .map_err(|error| format!("tested inputs cannot be listed: {error}"))?;
    if !output.status.success() {
        return Err("tested inputs cannot be listed by Git".into());
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| {
            std::str::from_utf8(part)
                .map(PathBuf::from)
                .map_err(|_| "tested input paths must be UTF-8".to_owned())
        })
        .collect()
}

fn git_paths_bounded(root: &Path, budget: &mut ResumeInputBudget) -> Result<Vec<PathBuf>, String> {
    const OUTPUT_LIMIT: usize = 1024 * 1024;
    let started = Instant::now();
    let listing_deadline = (started + Duration::from_millis(1000)).min(budget.deadline());
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args([
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
        ".",
    ]);
    let output = match crate::kernel::bounded_process::run(
        &mut command,
        OUTPUT_LIMIT,
        65536,
        listing_deadline,
    ) {
        Ok(output) => output,
        Err(error) => {
            let reason = match error.reason {
                "deadline_exceeded"
                    if budget.deadline() <= started + Duration::from_millis(1000) =>
                {
                    "resume_deadline_exceeded"
                }
                "deadline_exceeded" => "git_listing_deadline_exceeded",
                "cancelled" => "resume_cancelled",
                "stdout_limit_exceeded" | "stderr_limit_exceeded" => {
                    "git_listing_output_limit_exceeded"
                }
                "cleanup_incomplete" => "git_listing_cleanup_incomplete",
                _ => return Err(format!("tested inputs cannot be listed: {}", error.detail)),
            };
            budget.mark_limited(reason);
            return Err(format!("{reason}: {}", error.detail));
        }
    };
    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        return Err(if diagnostic.is_empty() {
            "tested inputs cannot be listed by Git".into()
        } else {
            format!(
                "tested inputs cannot be listed by Git: {}",
                diagnostic.trim()
            )
        });
    }
    parse_git_paths(output.status, &output.stdout)
}

fn parse_git_paths(status: ExitStatus, output: &[u8]) -> Result<Vec<PathBuf>, String> {
    if !status.success() {
        return Err("tested inputs cannot be listed by Git".into());
    }
    output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| {
            std::str::from_utf8(part)
                .map(PathBuf::from)
                .map_err(|_| "tested input paths must be UTF-8".to_owned())
        })
        .collect()
}

fn walk_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let entries = fs::read_dir(root.join(&relative))
            .map_err(|error| format!("tested inputs cannot be listed: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = relative.join(entry.file_name());
            if relative.as_os_str().is_empty()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| EXCLUDED_DIRS.contains(&name))
            {
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    Ok(found)
}

fn walk_paths_bounded(root: &Path, budget: &mut ResumeInputBudget) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    let mut directories = 1usize;
    while let Some(relative) = pending.pop() {
        budget.check().map_err(str::to_owned)?;
        let entries = fs::read_dir(root.join(&relative))
            .map_err(|error| format!("tested inputs cannot be listed: {error}"))?;
        for entry in entries {
            budget.check().map_err(str::to_owned)?;
            let entry = entry.map_err(|error| error.to_string())?;
            let path = relative.join(entry.file_name());
            if relative.as_os_str().is_empty()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| EXCLUDED_DIRS.contains(&name))
            {
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                directories += 1;
                if directories > 1024 {
                    return Err("fingerprint_directory_limit_exceeded".into());
                }
                pending.push(path);
            } else {
                if found.len() >= 4096 {
                    return Err("fingerprint_path_limit_exceeded".into());
                }
                found.push(path);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_stream_hash_matches_existing_full_file_digest() {
        let path = std::env::temp_dir().join(format!(
            "exitbind-bounded-fingerprint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos()
        ));
        let contents = (0..50_000)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        fs::write(&path, &contents).expect("write fixture");
        let mut budget = ResumeInputBudget::new();
        let bounded = hash_file_bounded(&path, contents.len() as u64, &mut budget)
            .expect("bounded full file hash");
        assert_eq!(
            bounded,
            crate::evidence::hash::file(&path).expect("existing hash")
        );
        fs::remove_file(path).expect("remove fixture");
    }

    #[test]
    fn deadline_and_directory_entry_limits_keep_distinct_reasons() {
        let mut expired =
            ResumeInputBudget::with_deadline(Instant::now() - Duration::from_millis(1));
        assert_eq!(
            expired.count_directory_entry(),
            Err("resume_deadline_exceeded")
        );

        let mut full = ResumeInputBudget::new();
        for _ in 0..1024 {
            assert_eq!(full.count_directory_entry(), Ok(()));
        }
        assert_eq!(
            full.count_directory_entry(),
            Err("directory_entry_limit_exceeded")
        );
    }

    #[test]
    fn excluded_state_directories_do_not_hide_sources() {
        let root = Path::new("/p");
        assert!(excluded(root, root, Path::new(".exitbind/runs/a.jsonl")));
        assert!(excluded(root, root, Path::new(".git/HEAD")));
        assert!(!excluded(root, root, Path::new("src/.exitbind.rs")));
        assert!(!excluded(root, root, Path::new("checks/preserve.sh")));
    }
}
