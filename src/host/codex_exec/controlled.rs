//! Closed, versioned native launch cell. The tool transport is host-owned;
//! this module grants no file authority and does not implement an effect owner.
use super::{Request, RunError};
use std::{path::Path, process::Command};

const EXEC_REQUIRED_OPTIONS: &[&str] = &[
    "--json",
    "--ephemeral",
    "--ignore-user-config",
    "--ignore-rules",
    "--disable",
    "-c",
    "--sandbox",
    "--model",
    "--output-schema",
];
const SANDBOX_REQUIRED_OPTIONS: &[&str] = &["-P", "-C"];

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
    require_options(
        &request.executable,
        &["exec"],
        EXEC_REQUIRED_OPTIONS,
        "controlled effects unsupported: Codex exec lacks required CLI options",
    )?;
    require_options(
        &request.executable,
        &["sandbox"],
        SANDBOX_REQUIRED_OPTIONS,
        "controlled effects unsupported: Codex sandbox lacks required read-only CLI options",
    )?;
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

fn require_options(
    executable: &Path,
    command: &[&str],
    required: &[&str],
    refusal: &'static str,
) -> Result<(), RunError> {
    let output = Command::new(executable)
        .args(command)
        .arg("--help")
        .output()
        .map_err(RunError::Launch)?;
    if !output.status.success() {
        return Err(RunError::InvalidRequest(refusal));
    }
    let mut help = output.stdout;
    help.extend_from_slice(&output.stderr);
    let help = String::from_utf8_lossy(&help);
    if first_missing_option(&help, required).is_some() {
        return Err(RunError::InvalidRequest(refusal));
    }
    Ok(())
}

fn first_missing_option<'a>(help: &str, required: &'a [&'a str]) -> Option<&'a str> {
    required
        .iter()
        .copied()
        .find(|option| !help_lists_option(help, option))
}

fn help_lists_option(help: &str, option: &str) -> bool {
    help.lines().any(|line| {
        let mut tokens = line.split_whitespace();
        let Some(first) = tokens.next() else {
            return false;
        };
        if first.trim_end_matches(',') == option {
            return true;
        }
        first.ends_with(',')
            && tokens
                .next()
                .is_some_and(|alias| alias == option && alias.starts_with('-'))
    })
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
    let serve = crate::emitted::Route::new(["work", "file-serve", tool.session.as_str()])
        .config_text(&tool.config)
        .run_by(&tool.executable)
        .shown_argv();
    for setting in [
        "approval_policy=\"never\"".into(),
        format!("mcp_servers.exitbind-file.command={}", quote(&serve[0])),
        format!(
            "mcp_servers.exitbind-file.args={}",
            serde_json::json!(serve[1..])
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
    use super::{
        first_missing_option, help_lists_option, protect_descriptors, EXEC_REQUIRED_OPTIONS,
    };
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

    #[test]
    fn capability_detection_uses_options_not_a_version_allowlist() {
        let help = "codex-cli 0.163.0\n  --json\n  --ephemeral\n  --ignore-user-config\n  --ignore-rules\n  --disable <FEATURE>\n  -c, --config <key=value>\n  --sandbox <MODE>\n  --model <MODEL>\n  --output-schema <FILE>\nDescription: --missing-capability is not listed above.\n";
        assert_eq!(first_missing_option(help, EXEC_REQUIRED_OPTIONS), None);
        assert!(help_lists_option(help, "-c"));
        assert!(help_lists_option(help, "--ignore-user-config"));
        assert!(!help_lists_option(help, "--missing-capability"));
        assert_eq!(
            first_missing_option(help, &["-P", "--missing-capability"]),
            Some("-P")
        );
    }
}
