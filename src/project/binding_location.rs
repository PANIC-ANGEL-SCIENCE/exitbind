//! Select writable storage while preserving every existing project binding.
use std::{fs, path::PathBuf};

pub(super) fn candidates() -> Result<Vec<PathBuf>, String> {
    let exitbind = super::layout_types::exitbind_surface();
    let explicit = if exitbind {
        std::env::var_os("EXITBIND_BINDINGS_DIR")
            .or_else(|| std::env::var_os("SOULMATE_BINDINGS_DIR"))
    } else {
        std::env::var_os("SOULMATE_BINDINGS_DIR")
    };
    let mut candidates = Vec::new();
    if let Some(path) = explicit {
        candidates.push(path.into());
    } else {
        let caller = if exitbind { "exitbind" } else { "soulmate" };
        if let Some(base) = std::env::var_os("XDG_STATE_HOME") {
            candidates.push(PathBuf::from(base).join(caller).join("bindings"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let fallback = PathBuf::from(home)
                .join(".local/state")
                .join(caller)
                .join("bindings");
            if !candidates.contains(&fallback) {
                candidates.push(fallback);
            }
        }
    }
    if candidates.is_empty() || candidates.iter().any(|path| !path.is_absolute()) {
        return Err("machine-local binding directory must be absolute".into());
    }
    Ok(candidates)
}

pub(super) fn directories() -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for candidate in candidates()? {
        match super::layout_types::binding_directory_at(&candidate, false) {
            Ok(path) => found.push(path),
            Err(error) if error.starts_with("binding directory:") => {
                match fs::symlink_metadata(&candidate) {
                    Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(found)
}

pub(super) fn locate(id: &str) -> Result<Option<PathBuf>, String> {
    let mut found = None;
    for directory in directories()? {
        let path = directory.join(format!("{id}.json"));
        match fs::symlink_metadata(&path) {
            Ok(_) if found.replace(path).is_some() => return Err("multiple machine-local bindings claim this project ID; select the authoritative bindings-directory override".into()),
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(format!("project binding: {error}")),
        }
    }
    Ok(found)
}

pub(super) fn select() -> Result<PathBuf, String> {
    let candidates = candidates()?;
    // Reuse a writable existing registry first. A read-only registry remains
    // discoverable; it must not prevent a new unrelated project using writable
    // storage. locate() prevents duplicating its existing project identities.
    for candidate in directories()? {
        if super::portability::writable(&candidate) {
            return Ok(candidate);
        }
    }
    for candidate in candidates {
        let mut ancestor = candidate.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or("binding directory has no parent")?;
        }
        if super::portability::writable(ancestor) {
            return Ok(candidate);
        }
    }
    Err("unsupported storage: no writable machine-local binding registry; provide an owned bindings-directory override".into())
}
