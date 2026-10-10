//! Expose the host-reported native session ID to later shell commands.
//!
//! Claude Code passes `session_id` to SessionStart hooks and lets them persist
//! environment variables through `CLAUDE_ENV_FILE`. Recording that value as
//! `EXITBIND_NATIVE_SESSION_ID` lets a receiving agent bind its own session
//! without inventing an identifier. The value stays host-reported; it is not
//! authenticated. Any unusable input leaves the environment unchanged.

use serde_json::{Map, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

pub(crate) const VARIABLE: &str = "EXITBIND_NATIVE_SESSION_ID";
const MAX_ID: usize = 128;

pub(crate) fn export(payload: &Map<String, Value>, env_file: Option<&std::ffi::OsStr>) {
    let Some(session) = payload.get("session_id").and_then(Value::as_str) else {
        return;
    };
    let Some(env_file) = env_file.map(Path::new) else {
        return;
    };
    if !safe_id(session) || !env_file.is_absolute() {
        return;
    }
    let line = format!("export {VARIABLE}={session}\n");
    // The host names a file it expects the hook to create or extend. Refuse a
    // symlink, and never block on a FIFO, so a hook cannot hang or redirect.
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let Ok(mut file) = options.open(env_file) else {
        return;
    };
    let Ok(metadata) = file.metadata() else {
        return;
    };
    if !metadata.is_file() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // This is a host-managed environment file. Never chmod it; decline to
        // add private session state when its ownership or permissions differ.
        if !same_owner(&metadata, unsafe { libc::geteuid() }) || metadata.mode() & 0o077 != 0 {
            return;
        }
    }
    let _ = file.write_all(line.as_bytes());
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(unix)]
fn same_owner(metadata: &std::fs::Metadata, expected_uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == expected_uid
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(unix)]
    #[test]
    fn descriptor_from_a_different_owner_is_not_a_private_session_target() {
        use std::os::unix::fs::MetadataExt;
        let descriptor = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let metadata = descriptor.metadata().unwrap();
        assert!(same_owner(&metadata, metadata.uid()));
        assert!(!same_owner(&metadata, metadata.uid().wrapping_add(1)));
        let system = std::fs::File::open("/etc/passwd")
            .unwrap()
            .metadata()
            .unwrap();
        let current = unsafe { libc::geteuid() };
        if system.uid() != current {
            assert!(!same_owner(&system, current));
        }
    }

    fn payload(session: &str) -> Map<String, Value> {
        json!({"hook_event_name":"SessionStart","session_id":session})
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn exports_only_safe_ids_to_an_absolute_env_file() {
        let root =
            std::env::temp_dir().join(format!("exitbind-native-session-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("env");
        std::fs::write(&file, "export KEPT=1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        export(&payload("0f1e2d3c-demo-4a5b"), Some(file.as_os_str()));
        export(&payload("x; rm -rf /"), Some(file.as_os_str()));
        export(&payload("$(id)"), Some(file.as_os_str()));
        export(&payload(&"a".repeat(MAX_ID + 1)), Some(file.as_os_str()));
        export(
            &payload("relative"),
            Some(std::ffi::OsStr::new("relative-env")),
        );
        export(&payload("absent"), None);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "export KEPT=1\nexport EXITBIND_NATIVE_SESSION_ID=0f1e2d3c-demo-4a5b\n"
        );
        let fresh = root.join("fresh");
        export(&payload("ok"), Some(fresh.as_os_str()));
        assert_eq!(
            std::fs::read_to_string(&fresh).unwrap(),
            "export EXITBIND_NATIVE_SESSION_ID=ok\n"
        );
        #[cfg(unix)]
        {
            let target = root.join("target");
            let link = root.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            export(&payload("ok"), Some(link.as_os_str()));
            assert!(!target.exists());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
