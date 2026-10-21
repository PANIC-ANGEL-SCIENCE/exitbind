//! The project's Git worktree for the private writer: the local exclude
//! line that hides a product directory and the `git status` check that proves it.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub(super) struct Git {
    root: PathBuf,
    /// Worktree-relative prefix of the project root, `""` or ending in `/`.
    prefix: String,
    deadline: Instant,
}

impl Git {
    pub(super) fn open(root: &Path, deadline: Instant) -> Option<Self> {
        let mut git = Self {
            root: root.to_path_buf(),
            prefix: String::new(),
            deadline,
        };
        git.prefix = git.text(&["rev-parse", "--show-prefix"])?;
        Some(git)
    }

    fn text(&self, args: &[&str]) -> Option<String> {
        let mut command = Command::new("git");
        crate::host::codegraph::clear_git_environment(&mut command);
        command.arg("-C").arg(&self.root).args(args);
        let output =
            crate::kernel::bounded_process::run(&mut command, 64 * 1024, 4096, self.deadline)
                .ok()?;
        output.status.success().then_some(())?;
        String::from_utf8(output.stdout)
            .ok()
            .map(|text| text.trim_end_matches('\n').to_owned())
    }

    /// Add `/<prefix><directory>/` to the local exclude file once. Only an
    /// absent file reads as empty; an unreadable, non-UTF-8, symlinked or
    /// non-regular file is left untouched and no card is saved.
    pub(super) fn exclude(&self, directory: &str) -> Option<()> {
        let file = self.text(&["rev-parse", "--git-path", "info/exclude"])?;
        let file = self.root.join(file);
        let pattern = format!("/{}{directory}/", self.prefix);
        let (existing, mode) = match fs::symlink_metadata(&file) {
            Ok(info) if info.is_file() => {
                use std::os::unix::fs::PermissionsExt;
                let text = fs::read_to_string(&file).ok()?;
                (text, info.permissions().mode() & 0o7777)
            }
            Ok(_) => return None,
            Err(error) if error.kind() == ErrorKind::NotFound => (String::new(), 0o644),
            Err(_) => return None,
        };
        if existing.lines().any(|line| line.trim() == pattern) {
            return Some(());
        }
        let parent = file.parent()?;
        fs::create_dir_all(parent).ok()?;
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        replace(&file, &format!("{existing}{separator}{pattern}\n"), mode).ok()
    }

    /// Whether `git status` lists nothing under `directory`.
    pub(super) fn unlisted(&self, directory: &str) -> bool {
        self.text(&[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            directory,
        ])
        .is_some_and(|text| text.is_empty())
    }
}

/// Atomically replace `file`: a new temporary file in the same directory with
/// the previous permissions, flushed, then renamed over it.
fn replace(file: &Path, text: &str, mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let parent = file
        .parent()
        .ok_or_else(|| std::io::Error::from(ErrorKind::InvalidInput))?;
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::from(ErrorKind::InvalidInput))?;
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .and_then(|mut handle| {
            handle.write_all(text.as_bytes())?;
            handle.set_permissions(fs::Permissions::from_mode(mode))?;
            handle.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, file));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
