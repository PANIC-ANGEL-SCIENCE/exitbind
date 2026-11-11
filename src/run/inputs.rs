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
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::rc::Rc;
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

/// The tested-input walk limits `work resume` enforces. Readiness states the
/// same numbers before a record exists instead of surfacing them after.
pub(crate) const WALK_DIRECTORY_LIMIT: usize = 1024;
pub(crate) const WALK_PATH_LIMIT: usize = 4096;

/// Limits and successful fingerprint memoization owned by one read-only
/// `work resume` request. Normal callers use `fingerprint` and stay unbounded.
#[derive(Clone)]
pub(crate) struct ResumeInputBudget(Rc<RefCell<ResumeInputState>>);

struct ResumeInputState {
    deadline: Instant,
    directory_entries: usize,
    work_ledgers: usize,
    cached: BTreeMap<(PathBuf, PathBuf, BTreeSet<String>), String>,
    limited: Option<&'static str>,
    detail: Option<String>,
}

// Resume visits existing read-only Goal and Work evaluators, some of which
// reload a historical configuration before hashing its inputs. Scope their
// existing fingerprint calls to this request's budget without changing any
// mutation-time validation. A stack restores the outer request on nesting and
// the guard removes it during normal return, errors, and panic unwinding.
thread_local! {
    static ACTIVE_RESUME_BUDGETS: RefCell<Vec<ResumeInputBudget>> = const { RefCell::new(Vec::new()) };
}

struct ResumeBudgetScope;

impl Drop for ResumeBudgetScope {
    fn drop(&mut self) {
        ACTIVE_RESUME_BUDGETS.with(|budgets| {
            budgets.borrow_mut().pop();
        });
    }
}

pub(crate) fn with_resume_budget<T>(
    budget: &mut ResumeInputBudget,
    operation: impl FnOnce(&mut ResumeInputBudget) -> T,
) -> T {
    ACTIVE_RESUME_BUDGETS.with(|budgets| budgets.borrow_mut().push(budget.clone()));
    let _scope = ResumeBudgetScope;
    operation(budget)
}

fn active_resume_budget() -> Option<ResumeInputBudget> {
    ACTIVE_RESUME_BUDGETS.with(|budgets| budgets.borrow().last().cloned())
}

impl ResumeInputBudget {
    pub(crate) fn new() -> Self {
        Self::with_deadline(Instant::now() + Duration::from_millis(3000))
    }

    fn with_deadline(deadline: Instant) -> Self {
        Self(Rc::new(RefCell::new(ResumeInputState {
            deadline,
            directory_entries: 0,
            work_ledgers: 0,
            cached: BTreeMap::new(),
            limited: None,
            detail: None,
        })))
    }

    pub(crate) fn check(&mut self) -> Result<(), &'static str> {
        let mut state = self.0.borrow_mut();
        if state.limited.is_none() && Instant::now() >= state.deadline {
            state.limited = Some("resume_deadline_exceeded");
        }
        state.limited.map_or(Ok(()), Err)
    }

    pub(crate) fn count_directory_entry(&mut self) -> Result<(), &'static str> {
        self.check()?;
        let mut state = self.0.borrow_mut();
        state.directory_entries = state.directory_entries.saturating_add(1);
        if state.directory_entries > WALK_DIRECTORY_LIMIT {
            state.limited = Some("directory_entry_limit_exceeded");
            return Err("directory_entry_limit_exceeded");
        }
        Ok(())
    }

    pub(crate) fn count_work_ledger(&mut self) -> usize {
        let mut state = self.0.borrow_mut();
        state.work_ledgers = state.work_ledgers.saturating_add(1);
        state.work_ledgers
    }

    pub(crate) fn directory_entries(&self) -> usize {
        self.0.borrow().directory_entries
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.0.borrow().deadline
    }

    pub(crate) fn work_ledgers(&self) -> usize {
        self.0.borrow().work_ledgers
    }

    pub(crate) fn limited_reason(&mut self) -> Option<&'static str> {
        let _ = self.check();
        self.0.borrow().limited
    }

    pub(crate) fn mark_limited(&mut self, reason: &'static str) {
        self.0.borrow_mut().limited = Some(reason);
    }

    pub(crate) fn mark_unavailable(&mut self, detail: String) {
        let mut state = self.0.borrow_mut();
        state.limited = Some("tested_inputs_unavailable");
        state.detail = Some(detail);
    }

    pub(crate) fn detail(&self) -> Option<String> {
        self.0.borrow().detail.clone()
    }
}

/// Digest of the current tested inputs, or an error when they cannot be
/// established (which callers must treat as "not reusable").
pub(crate) fn fingerprint(loaded: &crate::config::Loaded) -> Result<String, String> {
    if let Some(mut budget) = active_resume_budget() {
        return fingerprint_for_resume(loaded, &mut budget);
    }
    fingerprint_impl(loaded, None, &BTreeSet::new())
}

pub(crate) fn fingerprint_for_state(
    loaded: &crate::config::Loaded,
    state: &Value,
) -> Result<String, String> {
    let excluded = crate::run::guidance_continuity::unambiguous_paths(loaded, state);
    if let Some(mut budget) = active_resume_budget() {
        return fingerprint_with_exclusions_for_resume(loaded, &excluded, &mut budget);
    }
    fingerprint_impl(loaded, None, &excluded)
}

pub(crate) fn fingerprint_for_resume(
    loaded: &crate::config::Loaded,
    budget: &mut ResumeInputBudget,
) -> Result<String, String> {
    fingerprint_with_exclusions_for_resume(loaded, &BTreeSet::new(), budget)
}

fn fingerprint_with_exclusions_for_resume(
    loaded: &crate::config::Loaded,
    excluded: &BTreeSet<String>,
    budget: &mut ResumeInputBudget,
) -> Result<String, String> {
    budget.check().map_err(str::to_owned)?;
    let key = (
        loaded.product_root.clone(),
        loaded.state_root.clone(),
        excluded.clone(),
    );
    if let Some(digest) = budget.0.borrow().cached.get(&key).cloned() {
        return Ok(digest);
    }
    let digest = match fingerprint_impl(loaded, Some(budget), excluded) {
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
            if budget.limited_reason().is_none() {
                if let Some(reason) = reason {
                    budget.mark_limited(reason);
                } else {
                    budget.mark_unavailable(error.clone());
                }
            }
            return Err(error);
        }
    };
    // Caching is strictly request-local and capped; a new command always
    // fingerprints again. Distinct continuity exclusions cannot share a hash.
    let mut state = budget.0.borrow_mut();
    if state.cached.len() < 32 {
        state.cached.insert(key, digest.clone());
    }
    Ok(digest)
}

pub(crate) fn fingerprint_for_state_resume(
    loaded: &crate::config::Loaded,
    state: &Value,
    budget: &mut ResumeInputBudget,
) -> Result<String, String> {
    let excluded = crate::run::guidance_continuity::unambiguous_paths(loaded, state);
    fingerprint_with_exclusions_for_resume(loaded, &excluded, budget)
}

fn fingerprint_impl(
    loaded: &crate::config::Loaded,
    mut budget: Option<&mut ResumeInputBudget>,
    excluded_paths: &BTreeSet<String>,
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
    paths.retain(|path| {
        !excluded(root, &loaded.state_root, path)
            && path
                .to_str()
                .map_or(true, |relative| !excluded_paths.contains(relative))
    });
    paths.sort();
    paths.dedup();
    let mut entries = Vec::with_capacity(paths.len());
    let mut aggregate_bytes = 0u64;
    for (index, relative) in paths.into_iter().enumerate() {
        if let Some(budget) = budget.as_deref_mut() {
            budget.check().map_err(str::to_owned)?;
            if index >= WALK_PATH_LIMIT {
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

/// The tested-input exclusion every walk uses: an excluded first component
/// or a path under the state root.
pub(crate) fn excluded(root: &Path, state_root: &Path, relative: &Path) -> bool {
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
                if directories > WALK_DIRECTORY_LIMIT {
                    return Err("fingerprint_directory_limit_exceeded".into());
                }
                pending.push(path);
            } else {
                if found.len() >= WALK_PATH_LIMIT {
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

    fn loaded(root: &Path, state_root: &Path) -> crate::config::Loaded {
        assert!(
            !crate::project::git_preflight::has_git_marker(root),
            "input-budget test requires a marker-free temporary root"
        );
        crate::config::Loaded {
            config: json!({}),
            agents: BTreeMap::new(),
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            workflows: BTreeMap::new(),
            verification_preparation: None,
            path: root.join("exitbind.json"),
            control_root: root.to_owned(),
            product_root: root.to_owned(),
            state_root: state_root.to_owned(),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: "{}".into(),
        }
    }

    #[test]
    fn resume_scope_restores_nested_budget_after_error_and_panic() {
        let root = crate::test_root::temp("resume-budget-scope");
        fs::write(root.join("input"), b"first").unwrap();
        let loaded = loaded(&root, &root.join("state"));
        let mut outer = ResumeInputBudget::new();
        let mut expired = ResumeInputBudget::with_deadline(Instant::now() - Duration::from_secs(1));
        let initial = fingerprint(&loaded).unwrap();

        with_resume_budget(&mut outer, |_| {
            assert_eq!(fingerprint(&loaded).unwrap(), initial);
            let error = with_resume_budget(&mut expired, |_| fingerprint(&loaded));
            assert_eq!(error.unwrap_err(), "resume_deadline_exceeded");
            assert_eq!(fingerprint(&loaded).unwrap(), initial);
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut nested = ResumeInputBudget::new();
                with_resume_budget(&mut nested, |_| panic!("test resume scope unwind"));
            }));
            assert!(unwind.is_err());
            assert_eq!(fingerprint(&loaded).unwrap(), initial);
        });
        assert!(active_resume_budget().is_none());

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut request = ResumeInputBudget::new();
            with_resume_budget(&mut request, |_| panic!("test top-level resume unwind"));
        }));
        assert!(unwind.is_err());
        assert!(active_resume_budget().is_none());

        // Outside the read-only scope a mutation consumer must hash new bytes.
        fs::write(root.join("input"), b"changed").unwrap();
        assert_ne!(fingerprint(&loaded).unwrap(), initial);
        assert!(active_resume_budget().is_none());
    }

    #[test]
    fn resume_limit_does_not_apply_to_later_mutation_fingerprints() {
        let root = crate::test_root::temp("resume-budget-mutation");
        File::create(root.join("large"))
            .unwrap()
            .set_len(16 * 1024 * 1024 + 1)
            .unwrap();
        let loaded = loaded(&root, &root.join("state"));
        let mut budget = ResumeInputBudget::new();
        let error = with_resume_budget(&mut budget, |_| fingerprint(&loaded)).unwrap_err();
        assert_eq!(error, "fingerprint_byte_limit_exceeded");
        assert_eq!(
            budget.limited_reason(),
            Some("fingerprint_byte_limit_exceeded")
        );
        assert!(fingerprint(&loaded).is_ok());
    }

    #[test]
    fn resume_cache_keys_product_state_and_exact_exclusions() {
        let root = crate::test_root::temp("resume-budget-cache");
        fs::create_dir(root.join("state-one")).unwrap();
        fs::write(root.join("state-one/kept"), b"state data").unwrap();
        fs::write(root.join("source"), b"source data").unwrap();
        let first = loaded(&root, &root.join("state-one"));
        let other_state = loaded(&root, &root.join("state-two"));
        let mut budget = ResumeInputBudget::new();
        let included = fingerprint_for_resume(&first, &mut budget).unwrap();
        let other = fingerprint_for_resume(&other_state, &mut budget).unwrap();
        assert_ne!(included, other);
        let excluded = BTreeSet::from(["source".to_owned()]);
        let without_source =
            fingerprint_with_exclusions_for_resume(&first, &excluded, &mut budget).unwrap();
        assert_ne!(included, without_source);
        assert_eq!(
            fingerprint_for_resume(&first, &mut budget).unwrap(),
            included
        );

        let another = crate::test_root::temp("resume-budget-other-root");
        fs::write(another.join("different"), b"different").unwrap();
        let other_root = loaded(&another, &another.join("state"));
        assert_ne!(
            fingerprint_for_resume(&other_root, &mut budget).unwrap(),
            included
        );
        assert_eq!(budget.0.borrow().cached.len(), 4);
    }

    #[test]
    fn bounded_stream_hash_matches_existing_full_file_digest() {
        let root = crate::test_root::temp("bounded-fingerprint");
        let path = root.join("fixture");
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
