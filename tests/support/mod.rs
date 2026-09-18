use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn temp(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "exitbind-{label}-{}-{sequence}",
            std::process::id()
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => return std::fs::canonicalize(path).expect("canonicalize test directory"),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("create test directory: {error}"),
        }
    }
}

/// Place an executable at `target` without racing another thread's fork.
///
/// Copying onto a path that a concurrently forked child inherited as an open
/// descriptor makes the following `exec` fail with `ETXTBSY`. Writing beside it
/// and renaming gives the path a fresh inode, so the exec cannot see a
/// half-written or still-held file.
#[allow(dead_code)]
pub fn place_executable(source: &Path, target: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let staging = target.with_file_name(format!(
        ".{}.staging-{}",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("binary"),
        std::process::id()
    ));
    std::fs::copy(source, &staging).expect("stage test binary");
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))
        .expect("mark test binary executable");
    std::fs::rename(&staging, target).expect("place test binary");
}
