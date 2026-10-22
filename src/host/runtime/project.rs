//! Which project configuration a hook event uses.
//!
//! Every event the dispatcher handles resolves here. The project that owns
//! the session's native Goal comes first, verified by its owner identity
//! (`session_recovery::goal_owner`); the payload `cwd` is used only when no
//! owner configuration is found. The event's text names the configuration it
//! used with `fact_line`.

use crate::config::Loaded;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    GoalOwner,
    WorkingDirectory,
}

pub(super) enum Resolution {
    Project {
        loaded: Box<Loaded>,
        config_path: PathBuf,
        source: Source,
    },
    CleanAbsence,
    /// The configuration that was found but could not be used, if any.
    Unresolved(Option<String>),
}

pub(super) fn resolve(object: &serde_json::Map<String, Value>) -> Resolution {
    let goal_owner = crate::host::session_recovery::goal_owner(object);
    let owner = match goal_owner {
        Some(crate::host::session_recovery::GoalOwner {
            config: Some((path, loaded)),
            ..
        }) => {
            if !usable_control_root(&loaded) {
                return Resolution::Unresolved(Some(path));
            }
            return Resolution::Project {
                loaded: Box::new(loaded),
                config_path: PathBuf::from(path),
                source: Source::GoalOwner,
            };
        }
        Some(found) => Some(found.owner),
        None => None,
    };
    let cwd = object
        .get("cwd")
        .or_else(|| object.get("current_directory"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Ok(project) = absolute_directory(cwd) else {
        return Resolution::Unresolved(None);
    };
    let portable = if crate::producer::exitbind_surface() {
        project.join("exitbind.json")
    } else {
        project.join("soulmate.json")
    };
    let portable_present = match fs::symlink_metadata(&portable) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Resolution::Unresolved(None),
    };
    let config_path = if portable_present {
        let safe = fs::symlink_metadata(&portable)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            && portable.starts_with(&project)
            && contained_existing(&project, &portable);
        if !safe {
            return Resolution::Unresolved(None);
        }
        portable
    } else {
        match crate::project::portability::discover_from(&project) {
            Ok(Some(path)) => path,
            Ok(None) => return Resolution::CleanAbsence,
            Err(_) => return Resolution::Unresolved(None),
        }
    };
    let loaded = match config_path
        .to_str()
        .ok_or(())
        .and_then(|path| crate::config::load(Some(path)).map_err(|_| ()))
    {
        Ok(value) => value,
        Err(_) => return Resolution::Unresolved(config_path.to_str().map(str::to_owned)),
    };
    if !project.starts_with(&loaded.product_root) || !usable_control_root(&loaded) {
        return Resolution::Unresolved(config_path.to_str().map(str::to_owned));
    }
    // The working-directory project can itself be the owner (no hint or
    // journey member named its location yet).
    let source = if owner
        .as_ref()
        .is_some_and(|owner| crate::host::session_recovery::is_owner(&loaded, owner))
    {
        Source::GoalOwner
    } else {
        Source::WorkingDirectory
    };
    Resolution::Project {
        loaded: Box::new(loaded),
        config_path,
        source,
    }
}

/// The plain fact the event's text carries: which configuration it used.
pub(super) fn fact_line(config_path: &Path, source: Source) -> String {
    format!(
        "Configuration used: {} ({}).",
        config_path.display(),
        from(source)
    )
}

/// The same fact for a text whose routes already print the exact
/// `--config` path (SessionStart, which spends a fixed byte budget).
pub(super) fn routed_fact_line(source: Source) -> String {
    format!(
        "Configuration used: the --config path above ({}).",
        from(source)
    )
}

fn from(source: Source) -> &'static str {
    match source {
        Source::GoalOwner => "native Goal owner",
        Source::WorkingDirectory => "working directory",
    }
}

fn usable_control_root(loaded: &Loaded) -> bool {
    loaded.mode != crate::project::layout_types::Mode::Portable
        || contained_existing(&loaded.product_root, &loaded.control_root)
}

fn absolute_directory(value: &str) -> Result<PathBuf, ()> {
    if value.is_empty() || value.contains('\0') || !Path::new(value).is_absolute() {
        return Err(());
    }
    let path = PathBuf::from(value);
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|_| ())?;
        if metadata.file_type().is_symlink() {
            return Err(());
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
    if !metadata.is_dir() {
        return Err(());
    }
    fs::canonicalize(path).map_err(|_| ())
}

pub(super) fn contained_existing(root: &Path, target: &Path) -> bool {
    fs::canonicalize(target)
        .ok()
        .is_some_and(|path| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::{fact_line, routed_fact_line, Source};
    use std::path::Path;

    #[test]
    fn the_fact_line_names_the_path_and_where_it_came_from() {
        let path = Path::new("/srv/control/exitbind.json");
        assert_eq!(
            fact_line(path, Source::GoalOwner),
            "Configuration used: /srv/control/exitbind.json (native Goal owner)."
        );
        assert_eq!(
            fact_line(path, Source::WorkingDirectory),
            "Configuration used: /srv/control/exitbind.json (working directory)."
        );
        assert_eq!(
            routed_fact_line(Source::GoalOwner),
            "Configuration used: the --config path above (native Goal owner)."
        );
    }
}
