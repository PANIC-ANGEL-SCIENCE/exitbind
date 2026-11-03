//! Native command construction and read-only Git launch classification.

use super::{controlled, Request, RunError};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Only a confirmed marker-free directory may opt out of Codex's repository
/// requirement. A Git error without a marker is not itself proof of no Git.
pub(crate) fn preflight(cwd: &Path) -> Result<bool, RunError> {
    let cwd = super::checked_directory(cwd)?;
    match crate::project::git_preflight::worktree_root(&cwd) {
        Ok(Some(_)) => return Ok(false),
        Err(error) => return Err(RunError::InvalidRequestDetail(error)),
        Ok(None) => {}
    }
    if crate::project::git_preflight::has_git_marker(&cwd) {
        return Err(RunError::InvalidRequest(
            "Git marker is unresolved before Codex launch",
        ));
    }
    // An explicit Git location can be incomplete even when Git emits its
    // ordinary no-repository diagnostic. Only a resolved worktree above is
    // enough to establish that such a configuration is safe to launch.
    if ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        return Err(RunError::InvalidRequest(
            "Git launch preflight could not confirm a marker-free directory; inspect Git and retry",
        ));
    }
    // Ask Git again under a stable locale and across mount boundaries. The
    // first shared preflight deliberately treats any nonzero exit without a
    // marker as None; this confirmation rejects untrusted configuration,
    // malformed environment overrides and other unknown failures.
    let mut git = Command::new("git");
    git.arg("-C")
        .arg(&cwd)
        .args(["rev-parse", "--show-toplevel"])
        .env("LC_ALL", "C")
        .env("GIT_DISCOVERY_ACROSS_FILESYSTEM", "1");
    let output = crate::kernel::bounded_process::run(
        &mut git,
        4096,
        4096,
        Instant::now() + Duration::from_secs(2),
    )
    .map_err(|_| {
        RunError::InvalidRequest("Git launch preflight could not confirm repository state")
    })?;
    if output.status.code() != Some(128)
        || !output.stdout.is_empty()
        || output.stderr
            != b"fatal: not a git repository (or any of the parent directories): .git\n"
        || crate::project::git_preflight::has_git_marker(&cwd)
    {
        return Err(RunError::InvalidRequest(
            "Git launch preflight could not confirm a marker-free directory; inspect Git and retry",
        ));
    }
    Ok(true)
}

pub(super) fn line(
    request: &Request,
    executable: &Path,
    cwd: &Path,
) -> Result<(Command, bool), RunError> {
    let skip_git = preflight(cwd)?;
    let mut command = Command::new(executable);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.current_dir(cwd).arg("exec");
    if let Some(thread_id) = request.resume_thread_id.as_deref() {
        command.args(["resume", thread_id]);
    }
    if skip_git {
        command.arg("--skip-git-repo-check");
    }
    command.arg("--json");
    if let Some(tool) = &request.controlled_tool {
        controlled::configure(&mut command, tool);
    }
    if !request.persist_session {
        command.arg("--ephemeral");
    }
    if let Some(model) = request.model.as_deref() {
        command.args(["--model", model]);
    }
    if let Some(effort) = request.effort.as_deref() {
        command.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
    }
    if let Some(sandbox) = request.sandbox.as_deref() {
        command.args(["--sandbox", sandbox]);
    }
    if let Some(schema) = request.output_schema.as_deref() {
        command.args([
            "--output-schema",
            schema.to_str().ok_or(RunError::InvalidRequest(
                "output schema path is not valid UTF-8",
            ))?,
        ]);
    }
    command
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok((command, skip_git))
}
