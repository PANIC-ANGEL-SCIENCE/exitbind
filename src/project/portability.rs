//! Canonical workspace discovery and owned placement for automatic bootstrap.
//! Placement never copies source or changes source permissions.
use crate::{
    evidence::hash,
    project::{git_preflight, layout_types, path},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) struct Roots {
    pub product: PathBuf,
    pub control: PathBuf,
    pub state: PathBuf,
    pub id: String,
}

pub(crate) fn source(requested: &Path) -> Result<PathBuf, String> {
    let requested = path::absolute(requested)?;
    let canonical =
        fs::canonicalize(requested).map_err(|e| format!("source root unavailable: {e}"))?;
    match git_preflight::worktree_root(&canonical) {
        Ok(Some(root)) => Ok(root),
        Ok(None) if git_preflight::has_git_marker(&canonical) => {
            Err("Git workspace identity unavailable; inspect Git topology before bootstrap".into())
        }
        Err(error) if git_preflight::has_git_marker(&canonical) => Err(error),
        _ => Ok(canonical),
    }
}

pub(crate) fn bootstrap_writable(root: &Path) -> bool {
    writable(root) && {
        let state = root.join(layout_types::state_namespace());
        !state.exists() || writable(&state)
    }
}

pub(crate) fn writable(root: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let Ok(value) = CString::new(root.as_os_str().as_bytes()) else {
            return false;
        };
        // access checks the actual read-only mount and ACL as well as mode;
        // observation does not create a probe file in the user's source tree.
        unsafe { libc::access(value.as_ptr(), libc::W_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        fs::metadata(root).is_ok_and(|m| !m.permissions().readonly())
    }
}

pub(crate) fn automatic(
    product: &Path,
    control: Option<&str>,
    state: Option<&str>,
    id: Option<&str>,
) -> Result<Roots, String> {
    if layout_types::config_for_product(product)?.is_some()
        || product
            .join(crate::compatibility::profile().config)
            .symlink_metadata()
            .is_ok()
    {
        return Err("project already configured; inspect its existing context and roots, do not fork its authority with automatic bootstrap".into());
    }
    let text = product.to_str().ok_or("source identity requires UTF-8")?;
    let id = id
        .map(str::to_owned)
        .unwrap_or_else(|| format!("workspace-{}", &hash::text(text)[..32]));
    layout_types::validate_id(&id)?;
    // Validate explicit placement before creating any directories. A refused
    // source-contained root must not leave bootstrap directories in source.
    for explicit in [control, state].into_iter().flatten() {
        let root = Path::new(explicit);
        normalized_storage(root)?;
        if root.starts_with(product) || product.starts_with(root) {
            return Err("storage preflight: explicit control/state roots are incompatible with source; choose owned private external --control-root and --state-root outside source, then retry without relocating source".into());
        }
    }
    let (control, state) = if let (Some(control), Some(state)) = (control, state) {
        (PathBuf::from(control), PathBuf::from(state))
    } else {
        let candidates = [
            std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")),
        ];
        let mut chosen = None;
        for base in candidates.into_iter().flatten() {
            if !base.is_absolute() {
                continue;
            }
            let base = base
                .join(crate::compatibility::profile().caller)
                .join("projects")
                .join(&id);
            if base.starts_with(product) || product.starts_with(&base) {
                continue;
            }
            if owned_directory(&base).is_ok() {
                chosen = Some(base);
                break;
            }
        }
        let base = chosen.ok_or("unsupported storage: no safe writable owned state location; provide an existing owned --control-root and --state-root outside source")?;
        (
            control
                .map(PathBuf::from)
                .unwrap_or_else(|| base.join("control")),
            state
                .map(PathBuf::from)
                .unwrap_or_else(|| base.join("state")),
        )
    };
    for root in [&control, &state] {
        owned_directory(root).map_err(|error| format!("storage preflight: {error}; choose an owned private writable --control-root and --state-root outside source"))?;
    }
    Ok(Roots {
        product: product.into(),
        control,
        state,
        id,
    })
}

fn normalized_storage(root: &Path) -> Result<(), String> {
    if !root.is_absolute()
        || root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("storage preflight: automatic storage requires normalized absolute paths; provide absolute --control-root and --state-root outside source".into());
    }
    Ok(())
}

fn owned_directory(root: &Path) -> Result<(), String> {
    normalized_storage(root)?;
    let mut current = PathBuf::new();
    for part in root.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(info) if info.is_dir() && !info.file_type().is_symlink() => {}
            Ok(_) => {
                return Err("automatic storage refuses symlinks or non-directory components".into())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    fs::DirBuilder::new()
                        .mode(0o700)
                        .create(&current)
                        .map_err(|e| e.to_string())?;
                }
                #[cfg(not(unix))]
                fs::create_dir(&current).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let info = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
        if info.uid() != unsafe { libc::geteuid() } || info.permissions().mode() & 0o077 != 0 {
            return Err(
                "automatic storage must be owned by the current user and private (0700)".into(),
            );
        }
    }
    if !writable(root) {
        return Err("automatic storage is read-only".into());
    }
    Ok(())
}

pub(crate) fn discover_config() -> Result<Option<PathBuf>, String> {
    discover_from(&std::env::current_dir().map_err(|e| e.to_string())?)
}

pub(crate) fn discover_from(cwd: &Path) -> Result<Option<PathBuf>, String> {
    let top = git_preflight::worktree_root(cwd).ok().flatten();
    let home = std::env::var_os("HOME").and_then(|value| fs::canonicalize(value).ok());
    for ancestor in cwd.ancestors() {
        // HOME may hold a plan-only host context. It is not an implicit
        // portable project for every unrelated no-Git workspace below it.
        // An explicit local binding or --config still selects that project.
        if top.is_none() && ancestor != cwd && home.as_deref() == Some(ancestor) {
            return layout_types::config_for_product(ancestor);
        }
        let candidate = ancestor.join(crate::compatibility::profile().config);
        if let Ok(info) = candidate.symlink_metadata() {
            if info.file_type().is_symlink() || !info.is_file() {
                return Err("discovered configuration must be an ordinary file".into());
            }
            return Ok(Some(candidate));
        }
        if let Some(bound) = layout_types::config_for_product(ancestor)? {
            return Ok(Some(bound));
        }
        if top.as_deref() == Some(ancestor) {
            break;
        }
    }
    Ok(None)
}

pub(crate) fn diagnostic(product: &Path, state: &Path, control: &Path) -> Value {
    let git = match git_preflight::worktree_root(product) {
        Ok(Some(top)) => {
            let fields = ["--absolute-git-dir", "--git-common-dir"];
            let mut paths = Vec::new();
            for field in fields {
                let observed = Command::new("git")
                    .arg("-C")
                    .arg(product)
                    .args(["rev-parse", "--path-format=absolute", field])
                    .output();
                let path = observed
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_owned());
                paths.push(path);
            }
            json!({"state":"available","root":top,"gitDir":paths[0],"commonDir":paths[1],"topology":if paths[0].is_some() && paths[0] != paths[1] {"worktree"} else {"ordinary-or-gitfile"}})
        }
        Ok(None) => {
            json!({"state":"unavailable","topology":"no-git","discovery":"marker-free-fallback","detail":"No .git marker or Git worktree was discovered; canonical source identity is retained."})
        }
        Err(error) if git_preflight::has_git_marker(product) => {
            json!({"state":"unavailable","topology":"git-preflight-failed","detail":error,"nextAction":"Inspect the .git marker and provide Git on PATH; resolve Git discovery before bootstrap."})
        }
        Err(error) => {
            json!({"state":"unavailable","topology":"no-git","discovery":"marker-free-fallback","detail":error})
        }
    };
    json!({"sourceRoot":product,"git":git,"controlRoot":control,"stateRoot":state,"runtimeRoot":state,
        "sourceCapability":if writable(product) {"writable"} else {"read-only"},
        "stateCapability":if writable(state) {"writable"} else {"read-only"}})
}
