use super::{canonical, read_bounded, MAX_OUTPUT};
use crate::config;

const MAX_PROVIDER_ENTRY: usize = 16 * 1024 * 1024;
const MAX_DIRTY_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIRTY_PATHS: usize = 4096;
use crate::evidence::hash;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::Instant,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProviderIdentity {
    pub(super) version: String,
    pub(super) entry_path: String,
    pub(super) entry_sha256: String,
}

pub(super) enum IndexState {
    Missing,
    Ready {
        status: Value,
        index_path: String,
        index_identity: String,
        stale_reason: Option<String>,
        pending_changes: bool,
    },
}

pub(super) fn discover_provider() -> Result<PathBuf, String> {
    let path =
        std::env::var_os("PATH").ok_or("provider_unavailable: CodeGraph CLI is not on PATH")?;
    let names: &[&str] = if cfg!(windows) {
        &["codegraph.exe", "codegraph.cmd", "codegraph"]
    } else {
        &["codegraph"]
    };
    for directory in std::env::split_paths(&path) {
        for name in names {
            let candidate = directory.join(name);
            let Ok(resolved) = fs::canonicalize(&candidate) else {
                continue;
            };
            let Ok(metadata) = fs::metadata(&resolved) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            return Ok(resolved);
        }
    }
    Err("provider_unavailable: CodeGraph CLI is not available on PATH; continue ordinary source investigation".into())
}

fn bounded_file(
    path: &Path,
    limit: usize,
    deadline: Instant,
    label: &str,
) -> Result<Vec<u8>, String> {
    if Instant::now() >= deadline {
        return Err(format!("{label} deadline exceeded"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label} has no parent"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{label} path is not UTF-8"))?;
    let bytes = read_bounded(parent, name, label, limit)?;
    if Instant::now() >= deadline {
        return Err(format!("{label} deadline exceeded"));
    }
    Ok(bytes)
}

pub(super) fn run_provider(
    executable: &Path,
    root: &Path,
    args: &[&str],
    deadline: Instant,
    init: bool,
) -> Result<Vec<u8>, String> {
    let mut command = Command::new(executable);
    command.args(args).current_dir(root);
    command.env("CODEGRAPH_NO_DOWNLOAD", "1");
    command.env("CODEGRAPH_NO_DAEMON", "1");
    command.env("CODEGRAPH_TELEMETRY", "0");
    command.env("DO_NOT_TRACK", "1");
    command.env("CODEGRAPH_PARSE_WORKERS", "1");
    command.env("CODEGRAPH_RESOLVE_WORKERS", "1");
    if init {
        command.env_remove("CODEGRAPH_NO_WATCH");
        command.env("CODEGRAPH_FORCE_WATCH", "1");
    } else {
        command.env("CODEGRAPH_NO_WATCH", "1");
        command.env_remove("CODEGRAPH_FORCE_WATCH");
    }
    let output =
        crate::kernel::bounded_process::run(&mut command, MAX_OUTPUT, MAX_OUTPUT, deadline)
            .map_err(|error| format!("provider_process_{}: {}", error.reason, error.detail))?;
    if !output.status.success() {
        return Err(format!(
            "provider_command_failed: CodeGraph {} exited {}; {}",
            args.first().copied().unwrap_or("command"),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

pub(super) fn provider_identity(
    executable: &Path,
    root: &Path,
    deadline: Instant,
) -> Result<ProviderIdentity, String> {
    let path = canonical(executable)?;
    let entry_path = path
        .to_str()
        .ok_or("provider_identity_invalid: CodeGraph executable path is not UTF-8")?
        .to_owned();
    let bytes = bounded_file(
        &path,
        MAX_PROVIDER_ENTRY,
        deadline,
        "CodeGraph entry executable",
    )?;
    let version_bytes = run_provider(&path, root, &["--version"], deadline, false)?;
    let version = String::from_utf8(version_bytes)
        .map_err(|_| "provider_version_invalid: CodeGraph version is not UTF-8")?
        .trim()
        .to_owned();
    if version.is_empty() || version.len() > 128 {
        return Err(
            "provider_version_invalid: CodeGraph returned an invalid version string".into(),
        );
    }
    Ok(ProviderIdentity {
        version,
        entry_path,
        entry_sha256: hash::bytes(&bytes),
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::provider_identity;
    use std::{
        fs,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn provider_identity_refuses_non_utf8_executable_before_invocation() {
        use std::os::unix::ffi::OsStringExt;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codegraph-provider-path-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let executable = root.join(std::ffi::OsString::from_vec(b"provider-\xff".to_vec()));
        fs::write(&executable, b"fixture executable").unwrap();

        let result = provider_identity(&executable, &root, Instant::now() + Duration::from_secs(2));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            result.unwrap_err(),
            "provider_identity_invalid: CodeGraph executable path is not UTF-8"
        );
    }
}

/// Run Git against the named directory only, not an inherited repository.
pub(crate) fn clear_git_environment(command: &mut Command) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_SHALLOW_FILE",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(name);
    }
    command.env("GIT_OPTIONAL_LOCKS", "0");
}

fn git(root: &Path, args: &[&str], cap: usize, deadline: Instant) -> Result<Vec<u8>, String> {
    if Instant::now() >= deadline {
        return Err("source_identity_timeout: Git deadline exceeded".into());
    }
    let mut command = Command::new("git");
    clear_git_environment(&mut command);
    command.args(args).current_dir(root);
    let output = crate::kernel::bounded_process::run(&mut command, cap, MAX_OUTPUT, deadline)
        .map_err(|error| format!("source_identity_{}: {}", error.reason, error.detail))?;
    if !output.status.success() {
        return Err(format!(
            "source_identity_git_failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if Instant::now() >= deadline {
        return Err("source_identity_timeout: Git deadline exceeded".into());
    }
    Ok(output.stdout)
}

fn safe_dirty_path(root: &Path, raw: &[u8]) -> Result<PathBuf, String> {
    let relative = std::str::from_utf8(raw).map_err(|_| "Git reported a non-UTF-8 dirty path")?;
    let path = Path::new(relative);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("source_identity_unsafe: Git reported an unsafe dirty path".into());
    }
    let full = root.join(path);
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part.as_os_str());
        if let Ok(meta) = fs::symlink_metadata(&current) {
            if meta.file_type().is_symlink() {
                return Err("source_identity_unsafe: dirty source contains a symlink".into());
            }
        }
    }
    Ok(full)
}

fn generated_artifact_directory(
    root: &Path,
    state_root: &Path,
    deadline: Instant,
) -> Result<Option<String>, String> {
    if !state_root.starts_with(root) {
        return Ok(None);
    }
    let namespace = state_root.join(crate::project::layout_types::state_namespace());
    let directory = namespace.join("codegraph");
    for path in [&namespace, &directory] {
        if Instant::now() >= deadline {
            return Err("source_identity_timeout: state path validation deadline exceeded".into());
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(
                    "source_identity_unsafe: generated state path is not a safe directory".into(),
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("source_identity_unavailable: {error}"));
            }
        }
    }
    let namespace = canonical(&namespace)?;
    let directory = canonical(&directory)?;
    if directory.parent() != Some(namespace.as_path()) {
        return Err(
            "source_identity_unsafe: generated state directory escaped its namespace".into(),
        );
    }
    let Ok(relative) = directory.strip_prefix(root) else {
        return Ok(None);
    };
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or("source_identity_invalid: generated state path is not UTF-8")?;
    if components.is_empty()
        || components.iter().any(|component| component.is_empty())
        || components.join("/").len() > 512
    {
        return Err("source_identity_invalid: generated state path is malformed".into());
    }
    Ok(Some(components.join("/")))
}

pub(super) fn source_identity(
    loaded: &config::Loaded,
    deadline: Instant,
) -> Result<String, String> {
    let root = canonical(&loaded.product_root)?;
    let state_root = canonical(&loaded.state_root)?;
    let generated_relative = generated_artifact_directory(&root, &state_root, deadline)?;
    let is_generated_path = |path: &[u8]| {
        generated_relative.as_ref().is_some_and(|prefix| {
            path == prefix.as_bytes()
                || path
                    .strip_prefix(prefix.as_bytes())
                    .is_some_and(|tail| tail.starts_with(b"/"))
        })
    };
    let top = git(&root, &["rev-parse", "--show-toplevel"], 4096, deadline)?;
    let top = std::str::from_utf8(&top)
        .map_err(|_| "source_identity_invalid: Git root is not UTF-8")?
        .trim();
    if canonical(Path::new(top))? != root {
        return Err(
            "source_identity_foreign: Git worktree root does not match the validated project"
                .into(),
        );
    }
    let head = git(&root, &["rev-parse", "--verify", "HEAD"], 4096, deadline)?;
    let generated_exclusion = generated_relative
        .as_deref()
        .map(|path| format!(":(exclude){path}/**"));
    let mut staged_args = vec![
        "diff",
        "--cached",
        "--raw",
        "--no-abbrev",
        "--no-renames",
        "-z",
        "--",
        ".",
        ":(exclude).codegraph/**",
    ];
    if let Some(exclusion) = generated_exclusion.as_deref() {
        staged_args.push(exclusion);
    }
    let staged = git(&root, &staged_args, MAX_OUTPUT, deadline)?;
    let status = git(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        MAX_OUTPUT,
        deadline,
    )?;
    let mut dirty = Vec::new();
    let mut filtered_status = Vec::new();
    let mut total = 0usize;
    let mut records = status
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty());
    while let Some(record) = records.next() {
        if Instant::now() >= deadline {
            return Err("source_identity_timeout: dirty-file hashing deadline exceeded".into());
        }
        if record.len() < 4 {
            return Err("source_identity_invalid: malformed Git status record".into());
        }
        let status_code = &record[..2];
        let raw_path = &record[3..];
        let in_index = raw_path == b".codegraph"
            || raw_path.starts_with(b".codegraph/")
            || is_generated_path(raw_path);
        let rename = status_code.contains(&b'R') || status_code.contains(&b'C');
        if in_index {
            if rename {
                let _ = records.next();
            }
            continue;
        }
        if dirty.len() >= MAX_DIRTY_PATHS {
            return Err("source_identity_limit: more than 4096 dirty paths".into());
        }
        filtered_status.extend_from_slice(record);
        filtered_status.push(0);
        let full = safe_dirty_path(&root, raw_path)?;
        if !status_code.contains(&b'D') {
            let metadata = fs::symlink_metadata(&full)
                .map_err(|error| format!("source_identity_unavailable: {error}"))?;
            if !metadata.is_file() {
                return Err("source_identity_unsafe: dirty source is not a regular file".into());
            }
            let remaining = MAX_DIRTY_BYTES.saturating_sub(total);
            if remaining == 0 {
                return Err("source_identity_limit: dirty bytes exceed 16 MiB".into());
            }
            let relative = std::str::from_utf8(raw_path)
                .map_err(|_| "source_identity_invalid: dirty path is not UTF-8")?;
            let bytes = read_bounded(&root, relative, "dirty CodeGraph source", remaining)
                .map_err(|error| {
                    if error == "dirty CodeGraph source exceeds its byte bound" {
                        "source_identity_limit: dirty bytes exceed 16 MiB".to_owned()
                    } else {
                        error
                    }
                })?;
            total = total
                .checked_add(bytes.len())
                .ok_or("source_identity_limit: dirty byte count overflow")?;
            if total > MAX_DIRTY_BYTES {
                return Err("source_identity_limit: dirty bytes exceed 16 MiB".into());
            }
            if Instant::now() >= deadline {
                return Err("source_identity_timeout: dirty-file hashing deadline exceeded".into());
            }
            dirty.push(json!({"path":relative,"sha256":hash::bytes(&bytes)}));
        } else {
            dirty.push(json!({"path":String::from_utf8_lossy(raw_path),"deleted":true}));
        }
        if rename {
            if let Some(original) = records.next() {
                filtered_status.extend_from_slice(original);
                filtered_status.push(0);
            }
        }
    }
    Ok(hash::value(&json!({
        "root":root,
        "head":String::from_utf8_lossy(&head).trim(),
        "staged":hash::bytes(&staged),
        "status":hash::bytes(&filtered_status),
        "dirty":dirty
    })))
}

pub(super) fn status(
    executable: &Path,
    root: &Path,
    deadline: Instant,
) -> Result<IndexState, String> {
    let root_string = root.to_str().ok_or("target path is not UTF-8")?;
    let output = run_provider(
        executable,
        root,
        &["status", "--json", root_string],
        deadline,
        false,
    )?;
    let value: Value = serde_json::from_slice(&output)
        .map_err(|_| "provider_status_invalid: CodeGraph status returned malformed JSON")?;
    let index_path = value
        .get("indexPath")
        .and_then(Value::as_str)
        .ok_or("provider_status_invalid: status omitted indexPath")?
        .to_owned();
    let expected = root.join(".codegraph");
    if canonical(Path::new(&index_path))? != canonical(&expected)? {
        return Err(
            "foreign_index: CodeGraph index is not under the exact validated project".into(),
        );
    }
    let project_path = value
        .get("projectPath")
        .and_then(Value::as_str)
        .ok_or("provider_status_invalid: status omitted projectPath")?;
    if canonical(Path::new(project_path))? != root {
        return Err("foreign_index: CodeGraph status describes another project".into());
    }
    if value.get("initialized").and_then(Value::as_bool) != Some(true) {
        return Ok(IndexState::Missing);
    }
    if value.get("worktreeMismatch") != Some(&Value::Null) {
        return Err("foreign_index: CodeGraph reports a worktree mismatch".into());
    }
    let pending = value
        .get("pendingChanges")
        .and_then(Value::as_object)
        .ok_or("provider_status_invalid: pendingChanges must be an object")?;
    let pending_changes = pending.values().try_fold(false, |any, count| {
        count
            .as_u64()
            .map(|value| any || value > 0)
            .ok_or("provider_status_invalid: pending change count is not numeric")
    })?;
    let state = value
        .pointer("/index/state")
        .and_then(Value::as_str)
        .ok_or("provider_status_invalid: status omitted index state")?;
    let pending_refs = value
        .pointer("/index/pendingRefs")
        .and_then(Value::as_u64)
        .ok_or("provider_status_invalid: status omitted pending reference count")?;
    let reindex = value
        .pointer("/index/reindexRecommended")
        .and_then(Value::as_bool)
        .ok_or("provider_status_invalid: status omitted reindex recommendation")?;
    let stale_reason = if state != "complete" || pending_refs != 0 || reindex {
        Some(format!("partial or extraction-stale index (state {state}, pending references {pending_refs}, reindex recommended {reindex})"))
    } else {
        None
    };
    let index_identity = hash::value(&json!({
        "path":index_path,
        "version":value.get("version"),
        "lastIndexed":value.get("lastIndexed"),
        "fileCount":value.get("fileCount"),
        "nodeCount":value.get("nodeCount"),
        "edgeCount":value.get("edgeCount"),
        "dbSizeBytes":value.get("dbSizeBytes"),
        "backend":value.get("backend"),
        "index":value.get("index")
    }));
    Ok(IndexState::Ready {
        status: value,
        index_path,
        index_identity,
        stale_reason,
        pending_changes,
    })
}
