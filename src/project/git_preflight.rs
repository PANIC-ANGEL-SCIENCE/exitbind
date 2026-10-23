//! Read-only Git classification before initialization or private state writes.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn worktree_root(path: &Path) -> Result<Option<PathBuf>, String> {
    let path = path
        .to_str()
        .ok_or("Git preflight requires a UTF-8 worktree path")?;
    let output = Command::new("git")
        .args(["-C", path, "rev-parse", "--show-toplevel"])
        .output()
        .map_err(command_error)?;
    if !output.status.success() {
        if has_git_marker(Path::new(path)) {
            return Err(format!(
                "Git topology preflight failed: a .git marker exists at or above {path}, but Git cannot resolve a worktree; inspect the marker and run git rev-parse --show-toplevel from the requested source directory before retrying bootstrap; suspicious markers are not a no-Git fallback"
            ));
        }
        return Ok(None);
    }
    let value = String::from_utf8(output.stdout)
        .map_err(|_| "Git reported a non-UTF-8 worktree path".to_owned())?;
    std::fs::canonicalize(value.trim())
        .map(Some)
        .map_err(|error| format!("Git worktree root cannot be resolved: {error}"))
}

/// Bounded read-only counterpart used only inside a request-owned discovery
/// budget. Other Git preflight callers retain `worktree_root` semantics.
pub(crate) fn worktree_root_bounded(
    path: &Path,
    deadline: std::time::Instant,
) -> Result<Option<PathBuf>, String> {
    let path_text = path
        .to_str()
        .ok_or("Git preflight requires a UTF-8 worktree path")?;
    let mut command = Command::new("git");
    command.args(["-C", path_text, "rev-parse", "--show-toplevel"]);
    let output = match crate::kernel::bounded_process::run(&mut command, 4096, 65536, deadline) {
        Ok(output) => output,
        Err(error)
            if error.reason == "launch_failed"
                && error.kind == Some(std::io::ErrorKind::NotFound)
                && !has_git_marker(path) =>
        {
            // With no .git marker anywhere above the root, walking it directly
            // has the same documented non-Git coverage. A marker keeps the
            // existing actionable missing-Git refusal.
            return Ok(None);
        }
        Err(error)
            if error.reason == "launch_failed"
                && error.kind == Some(std::io::ErrorKind::NotFound) =>
        {
            return Err(command_error(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
        }
        Err(error) => {
            return Err(format!(
                "git_classification_{}: {}",
                error.reason, error.detail
            ));
        }
    };
    if !output.status.success() {
        if has_git_marker(path) {
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Git topology preflight failed: a .git marker exists at or above {path_text}, but Git cannot resolve a worktree; inspect the marker and run git rev-parse --show-toplevel from the requested source directory before retrying bootstrap; suspicious markers are not a no-Git fallback{}",
                if diagnostic.is_empty() { String::new() } else { format!("; Git reported: {}", diagnostic.trim()) }
            ));
        }
        return Ok(None);
    }
    let value = String::from_utf8(output.stdout)
        .map_err(|_| "Git reported a non-UTF-8 worktree path".to_owned())?;
    std::fs::canonicalize(value.trim())
        .map(Some)
        .map_err(|error| format!("Git worktree root cannot be resolved: {error}"))
}

pub(crate) fn has_git_marker(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        std::fs::symlink_metadata(ancestor.join(".git"))
            .map(|_| true)
            .unwrap_or_else(|error| error.kind() != std::io::ErrorKind::NotFound)
    })
}

pub(crate) fn refuse_tracked_targets(root: &Path, targets: &[&Path]) -> Result<(), String> {
    let top = match worktree_root(root) {
        Ok(Some(top)) => top,
        Ok(None) if has_git_marker(root) => {
            return Err("Git reported a problem while resolving the worktree root".into())
        }
        Err(error) if has_git_marker(root) => return Err(error),
        _ => return Ok(()),
    };
    let top_text = top
        .to_str()
        .ok_or("Git preflight requires a UTF-8 worktree path")?;
    let real_root = std::fs::canonicalize(root)
        .map_err(|error| format!("Git mutation root cannot be resolved: {error}"))?;
    for target in targets {
        let beneath_root = target
            .strip_prefix(root)
            .map_err(|_| "Git mutation target escapes the worktree".to_owned())?;
        let normalized = real_root.join(beneath_root);
        let relative = normalized
            .strip_prefix(&top)
            .map_err(|_| "Git mutation target escapes the worktree".to_owned())?;
        let relative = relative
            .to_str()
            .ok_or("Git preflight requires UTF-8 target paths")?;
        let tracked_status = Command::new("git")
            .args([
                "-C",
                top_text,
                "ls-files",
                "--error-unmatch",
                "--",
                relative,
            ])
            .output()
            .map_err(command_error)?
            .status;
        let tracked = match tracked_status.code() {
            Some(0) => true,
            Some(1) => false,
            Some(code) => {
                return Err(format!(
                    "Git tracked-path preflight failed with exit code {code}"
                ))
            }
            None => return Err("Git tracked-path preflight was terminated".into()),
        };
        let staged_status = Command::new("git")
            .args([
                "-C", top_text, "diff", "--cached", "--quiet", "--", relative,
            ])
            .output()
            .map_err(command_error)?
            .status;
        let staged = match staged_status.code() {
            Some(0) => false,
            Some(1) => true,
            Some(code) => {
                return Err(format!(
                    "Git staged-path preflight failed with exit code {code}"
                ))
            }
            None => return Err("Git staged-path preflight was terminated".into()),
        };
        if tracked || staged {
            return Err("refusing mutation of a tracked or staged Git path".into());
        }
    }
    Ok(())
}

pub(crate) fn reject_roots_under_worktree(
    product_root: &Path,
    control_root: &Path,
    state_root: &Path,
) -> Result<(), String> {
    // Callers must pass existing canonical roots; onboarding establishes this
    // precondition before comparing roots across platform path aliases.
    let top = match worktree_root(product_root) {
        Ok(Some(top)) => top,
        Ok(None) if has_git_marker(product_root) => {
            return Err("Git reported a problem during local initialization preflight".into())
        }
        Err(error) if has_git_marker(product_root) => return Err(error),
        _ => return Ok(()),
    };
    if control_root.starts_with(&top) || state_root.starts_with(&top) {
        return Err(
            "storage preflight: local ControlRoot and StateRoot must stay outside ProductRoot's Git worktree; choose existing owned external --control-root and --state-root, then retry without relocating source".into(),
        );
    }
    Ok(())
}

fn command_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "Git executable not found on PATH; Git discovery requires an available Git executable when a .git marker exists; provide Git on PATH and retry bootstrap without changing project files".into()
    } else {
        format!("Git preflight command could not run: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn repository(_label: &str) -> crate::test_root::TempRoot {
        let root = crate::test_root::temp("git-preflight");
        assert!(Command::new("git")
            .args(["-C", root.to_str().expect("UTF-8 test path"), "init", "-q"])
            .status()
            .expect("run git")
            .success());
        root
    }

    #[test]
    fn target_outside_worktree_is_rejected() {
        let root = repository("outside");
        let outside = root.parent().expect("test parent").join("outside.jsonl");
        let error = refuse_tracked_targets(&root, &[outside.as_path()]).expect_err("must refuse");
        assert_eq!(error, "Git mutation target escapes the worktree");
        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_target_is_rejected_without_lossy_conversion() {
        use std::os::unix::ffi::OsStringExt;

        let root = repository("non-utf8");
        let target = root.join(std::ffi::OsString::from_vec(vec![b'x', 0xff]));
        let error = refuse_tracked_targets(&root, &[target.as_path()]).expect_err("must refuse");
        assert_eq!(error, "Git preflight requires UTF-8 target paths");
        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[cfg(unix)]
    #[test]
    fn canonical_worktree_alias_preserves_target_classification() {
        use std::os::unix::fs::symlink;

        let root = repository("alias");
        let alias = root.with_extension("alias");
        symlink(&root, &alias).expect("create test alias");
        let target = alias.join("untracked.jsonl");
        refuse_tracked_targets(&alias, &[target.as_path()]).expect("classify aliased target");
        std::fs::remove_file(alias).expect("remove test alias");
        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn missing_git_error_is_actionable() {
        let error = command_error(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(error, "Git executable not found on PATH; Git discovery requires an available Git executable when a .git marker exists; provide Git on PATH and retry bootstrap without changing project files");
    }
}
