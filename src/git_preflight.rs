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
            .map(|metadata| metadata.is_dir() || metadata.is_file())
            .unwrap_or(false)
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
            "local ControlRoot and StateRoot must stay outside ProductRoot's Git worktree".into(),
        );
    }
    Ok(())
}

fn command_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "Git executable not found on PATH".into()
    } else {
        format!("Git preflight command could not run: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repository(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "soulmate-git-preflight-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("create test root");
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
        assert_eq!(error, "Git executable not found on PATH");
    }
}
