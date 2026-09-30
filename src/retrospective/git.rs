//! Minimal Git facts used by retrospection. Every command is explicitly read-only.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Repository {
    pub root: PathBuf,
    pub head: Option<String>,
    pub dirty: Option<bool>,
}

pub fn inspect(path: &Path) -> Result<Repository, String> {
    let requested = std::fs::canonicalize(path)
        .map_err(|error| format!("repository cannot be resolved: {error}"))?;
    if !requested.is_dir() {
        return Err("repository must be a directory".into());
    }
    let root_text = run(&requested, &["rev-parse", "--show-toplevel"])?;
    let root = std::fs::canonicalize(root_text.trim())
        .map_err(|error| format!("Git worktree root cannot be resolved: {error}"))?;
    let head = run_optional(&root, &["rev-parse", "HEAD"])?;
    let status = run_optional(&root, &["status", "--porcelain=v1", "--untracked-files=no"])?;
    Ok(Repository {
        root,
        head: head.map(|v| v.trim().to_owned()),
        dirty: status.map(|v| !v.trim().is_empty()),
    })
}

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = command(root, args)
        .output()
        .map_err(|error| git_error(error.to_string()))?;
    if !output.status.success() {
        return Err(format!(
            "Git read-only inspection failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "Git returned non-UTF-8 metadata".into())
}

fn run_optional(root: &Path, args: &[&str]) -> Result<Option<String>, String> {
    let output = command(root, args)
        .output()
        .map_err(|error| git_error(error.to_string()))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(output.stdout).map_err(|_| {
        "Git returned non-UTF-8 metadata".to_owned()
    })?))
}

fn command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root).args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "diff.external=",
        "-c",
        "protocol.version=0",
    ]);
    command
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn git_error(error: String) -> String {
    if error.contains("No such file") {
        "Git executable not found on PATH".into()
    } else {
        format!("Git could not run: {error}")
    }
}
