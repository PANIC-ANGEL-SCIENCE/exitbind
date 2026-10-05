//! Closed, versioned native launch cell. The tool transport is host-owned;
//! this module grants no file authority and does not implement an effect owner.
use super::{Request, RunError};
use std::{path::Path, process::Command};

#[derive(Clone, Debug)]
pub(crate) struct Tool {
    pub(crate) executable: String,
    pub(crate) session: String,
    pub(crate) config: String,
    pub(crate) protected_roots: Vec<std::path::PathBuf>,
}

pub(super) fn validate(request: &Request) -> Result<(), RunError> {
    if request.controlled_tool.is_none() {
        return Ok(());
    }
    if !cfg!(target_os = "linux")
        || request.sandbox.as_deref() != Some("read-only")
        || request.resume_thread_id.is_some()
        || request.persist_session
    {
        return Err(RunError::InvalidRequest(
            "controlled effects require a fresh ephemeral Linux read-only worker",
        ));
    }
    // Project configuration is another tool-authority layer. Do not guess that
    // an empty MCP map replaces it, or silently delete an operator's settings.
    let ignored_user_config = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".codex"))
        })
        .filter(|home| home.is_absolute())
        .map(|home| home.join("config.toml"));
    for ancestor in request.cwd.ancestors() {
        let config = ancestor.join(".codex/config.toml");
        // exec --ignore-user-config ignores this exact location, including
        // when HOME is an ancestor of the workspace. Other ancestor configs
        // remain project authority and must be refused.
        if ignored_user_config.as_ref() != Some(&config) && config.symlink_metadata().is_ok() {
            return Err(RunError::InvalidRequest("controlled effects refuse project Codex configuration; use an isolated project without .codex/config.toml"));
        }
    }
    for name in [
        "/etc/codex/config.toml",
        "/etc/codex/managed_config.toml",
        "/etc/codex/requirements.toml",
    ] {
        if Path::new(name).symlink_metadata().is_ok() {
            return Err(RunError::InvalidRequest(
                "controlled effects do not support system or managed Codex configuration",
            ));
        }
    }
    let output = Command::new(&request.executable)
        .arg("--version")
        .output()
        .map_err(RunError::Launch)?;
    if !output.status.success() || output.stdout != b"codex-cli 0.160.0\n" {
        return Err(RunError::InvalidRequest(
            "controlled effects currently support Codex 0.160.0 on Linux only",
        ));
    }
    let tool = request.controlled_tool.as_ref().expect("validated tool");
    let probe = Command::new(&request.executable)
        .args(["sandbox", "-P", ":read-only", "-C"])
        .arg(&request.cwd)
        .args([
            "--",
            "/bin/sh",
            "-c",
            "for path do test -e \"$path\" && test ! -w \"$path\" || exit 1; done",
            "exitbind-capability-probe",
        ])
        .args(&tool.protected_roots)
        .output()
        .map_err(RunError::Launch)?;
    if !probe.status.success() {
        return Err(RunError::InvalidRequest("controlled effects unsupported: native read-only capability does not protect every selected source/control/state/tool root"));
    }
    Ok(())
}

pub(super) fn configure(command: &mut Command, tool: &Tool) {
    protect_descriptors(command);
    // CLI values are structured TOML strings, never shell fragments. Ignore
    // inherited user configuration; feature gates close global hooks/plugins
    // and app tools which empty configuration maps do not reliably remove.
    command.args([
        "--ignore-user-config",
        "--ignore-rules",
        "--disable",
        "apps",
        "--disable",
        "plugins",
        "--disable",
        "hooks",
    ]);
    let quote = |value: &str| serde_json::to_string(value).expect("string serialization");
    for setting in [
        "approval_policy=\"never\"".into(),
        format!(
            "mcp_servers.exitbind-file.command={}",
            quote(&tool.executable)
        ),
        format!(
            "mcp_servers.exitbind-file.args={}",
            serde_json::json!(["work", "file-serve", tool.session, "--config", tool.config])
        ),
        "mcp_servers.exitbind-file.tools.file.approval_mode=\"approve\"".into(),
    ] {
        command.args(["-c", &setting]);
    }
}

fn protect_descriptors(command: &mut Command) {
    #[cfg(not(target_os = "linux"))]
    let _ = command;
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // A read-only mount does not revoke a previously opened writable FD.
        // Mark every non-stdio inherited descriptor close-on-exec in the child
        // only. Preserve Rust's spawn error pipe until exec and fail closed on
        // kernels without close_range(CLOEXEC), rather than leaking authority.
        unsafe {
            command.pre_exec(|| {
                if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::protect_descriptors;
    use std::{fs::File, os::unix::io::AsRawFd, process::Command};

    #[test]
    fn controlled_child_does_not_inherit_an_open_authority_descriptor() {
        let file = File::open("Cargo.toml").unwrap();
        let fd = file.as_raw_fd();
        assert!(fd > 2);
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        let mut child = Command::new("/bin/sh");
        child.args(["-c", &format!("test ! -e /proc/self/fd/{fd}")]);
        protect_descriptors(&mut child);
        assert!(child.status().unwrap().success());
        // The parent retains its descriptor and flags.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, 0);
    }
}
