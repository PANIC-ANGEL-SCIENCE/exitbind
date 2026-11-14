// Reads verified clean Git blobs for the existing tested-input SHA-256 format.
// Falls back to worktree bytes when Git observations are uncertain; see `src/run/inputs.rs`.
//! Request-local Git acceleration; this module creates no persistent identity.

use super::ResumeInputBudget;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const MAX_CONTENT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

#[cfg(unix)]
#[derive(Clone, PartialEq, Eq)]
struct Stat {
    dev: u64,
    ino: u64,
    mode: u32,
    len: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

#[cfg(unix)]
impl Stat {
    fn new(meta: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
            mode: meta.mode(),
            len: meta.len(),
            mtime: meta.mtime(),
            mtime_nsec: meta.mtime_nsec(),
            ctime: meta.ctime(),
            ctime_nsec: meta.ctime_nsec(),
        }
    }
}

pub(super) struct CleanDigest {
    pub(super) sha256: String,
    #[cfg(unix)]
    stat: Stat,
}

impl CleanDigest {
    pub(super) fn matches(&self, meta: &std::fs::Metadata) -> bool {
        #[cfg(unix)]
        {
            meta.is_file() && !meta.file_type().is_symlink() && self.stat == Stat::new(meta)
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            false
        }
    }
}

#[cfg(not(unix))]
pub(super) fn clean_hashes(
    _root: &Path,
    _git_root: &Path,
    _paths: &[PathBuf],
    _budget: &mut ResumeInputBudget,
) -> Result<Option<BTreeMap<PathBuf, CleanDigest>>, String> {
    Ok(None)
}

#[cfg(unix)]
pub(super) fn clean_hashes(
    root: &Path,
    git_root: &Path,
    paths: &[PathBuf],
    budget: &mut ResumeInputBudget,
) -> Result<Option<BTreeMap<PathBuf, CleanDigest>>, String> {
    budget.check().map_err(str::to_owned)?;
    let deadline = budget.deadline();
    // Older versions do not provide the known no-lazy-fetch control used by
    // this optimization. The existing actual-byte path remains available.
    let Some(version) = output(root, &["--version"], 4096, deadline, budget)? else {
        return Ok(None);
    };
    if !version.status.success() || !supported_version(&version.stdout) {
        return Ok(None);
    }
    if !safe_config(root, deadline, budget)? {
        return Ok(None);
    }

    // Attribute evaluation is read-only. Any attribute or unrecognized result
    // means conversions may affect Git's blob and requires actual bytes.
    let mut names = Vec::new();
    for path in paths {
        let Some(text) = path.to_str() else {
            return Ok(None);
        };
        names.extend_from_slice(text.as_bytes());
        names.push(0);
    }
    let mut attributes_seen = false;
    let mut attr = command(root);
    attr.args(["check-attr", "--all", "-z", "--stdin"]);
    let attr_result = input_stream(
        attr,
        names,
        2 * 1024 * 1024,
        deadline,
        &mut |chunk| {
            attributes_seen |= !chunk.is_empty();
            Ok(())
        },
        budget,
    )?;
    if attr_result
        .as_ref()
        .map_or(true, |result| !result.status.success())
        || attributes_seen
    {
        return Ok(None);
    }

    let Some(index) = output(
        root,
        &["ls-files", "-v", "--stage", "-z", "--cached", "--", "."],
        MAX_METADATA_BYTES,
        deadline,
        budget,
    )?
    else {
        return Ok(None);
    };
    if !index.status.success() {
        return Ok(None);
    }
    let index_bytes = index.stdout;
    let Some(index) = parse_index(&index_bytes) else {
        return Ok(None);
    };
    // Detect ordinary file changes while Git obtains status. A clean status
    // alone is not evidence for bytes changed after Git inspected them.
    let mut before_status = BTreeMap::new();
    for path in paths {
        budget.check().map_err(str::to_owned)?;
        if !index.contains_key(path) {
            continue;
        }
        if let Ok(meta) = std::fs::symlink_metadata(root.join(path)) {
            if meta.is_file() && !meta.file_type().is_symlink() {
                before_status.insert(path.clone(), Stat::new(&meta));
            }
        }
    }
    let Some(status) = output(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
            "--ignore-submodules=all",
            "--",
            ".",
        ],
        MAX_METADATA_BYTES,
        deadline,
        budget,
    )?
    else {
        return Ok(None);
    };
    if !status.status.success() {
        return Ok(None);
    }
    let Some(changed) = parse_status(&status.stdout, root, git_root) else {
        return Ok(None);
    };
    if !index_unchanged(root, &index_bytes, deadline, budget)? {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    let mut selected_bytes = 0u64;
    for path in paths {
        budget.check().map_err(str::to_owned)?;
        let Some(oid) = index.get(path).filter(|_| !changed.contains(path)) else {
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(root.join(path)) else {
            continue;
        };
        if !meta.is_file() || meta.file_type().is_symlink() {
            continue;
        }
        let stat = Stat::new(&meta);
        if before_status.get(path) != Some(&stat) {
            continue;
        }
        selected_bytes = selected_bytes.saturating_add(meta.len());
        if selected_bytes > MAX_CONTENT_BYTES {
            return Ok(None);
        }
        candidates.push(Candidate {
            path: path.clone(),
            oid: oid.clone(),
            stat,
        });
    }
    if candidates.is_empty() {
        return Ok(Some(BTreeMap::new()));
    }

    let mut requests = Vec::new();
    for item in &candidates {
        requests.extend_from_slice(item.oid.as_bytes());
        requests.push(b'\n');
    }
    let cap = (MAX_CONTENT_BYTES as usize).saturating_add(candidates.len().saturating_mul(128));
    let mut parser = BatchParser::new(candidates);
    let mut cat = command(root);
    cat.args(["cat-file", "--batch"]);
    let result = input_stream(
        cat,
        requests,
        cap,
        deadline,
        &mut |bytes| parser.consume(bytes),
        budget,
    )?;
    if result
        .as_ref()
        .map_or(true, |output| !output.status.success())
        || !parser.finished()
    {
        return Ok(None);
    }
    if !index_unchanged(root, &index_bytes, deadline, budget)? {
        return Ok(None);
    }
    Ok(Some(parser.digests))
}

#[cfg(unix)]
fn index_unchanged(
    root: &Path,
    previous: &[u8],
    deadline: Instant,
    budget: &mut ResumeInputBudget,
) -> Result<bool, String> {
    let Some(current) = output(
        root,
        &["ls-files", "-v", "--stage", "-z", "--cached", "--", "."],
        MAX_METADATA_BYTES,
        deadline,
        budget,
    )?
    else {
        return Ok(false);
    };
    Ok(current.status.success() && current.stdout == previous)
}

#[cfg(unix)]
fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(["-c", "core.fsmonitor=", "-c", "protocol.allow=never"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

#[cfg(unix)]
fn process_error(
    error: crate::kernel::bounded_process::BoundedProcessError,
    budget: &mut ResumeInputBudget,
) -> Result<(), String> {
    match error.reason {
        "cleanup_incomplete" => Err(format!(
            "Git observation cleanup incomplete: {}",
            error.detail
        )),
        "cancelled" => {
            budget.mark_limited("resume_cancelled");
            Err("resume_cancelled".into())
        }
        "deadline_exceeded" => {
            budget.check().map_err(str::to_owned)?;
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn output(
    root: &Path,
    args: &[&str],
    limit: usize,
    deadline: Instant,
    budget: &mut ResumeInputBudget,
) -> Result<Option<crate::kernel::bounded_process::BoundedOutput>, String> {
    let mut command = command(root);
    command.args(args);
    match crate::kernel::bounded_process::run(&mut command, limit, 65536, deadline) {
        Ok(result) => Ok(Some(result)),
        Err(error) => {
            process_error(error, budget)?;
            Ok(None)
        }
    }
}

#[cfg(unix)]
fn safe_config(
    root: &Path,
    deadline: Instant,
    budget: &mut ResumeInputBudget,
) -> Result<bool, String> {
    let Some(filters) = output(
        root,
        &["config", "--get-regexp", r"^filter\..*\.(clean|process)$"],
        65536,
        deadline,
        budget,
    )?
    else {
        return Ok(false);
    };
    if filters.status.code() != Some(1) {
        return Ok(false);
    }
    for (key, expected) in [
        ("core.autocrlf", "false"),
        ("core.ignorestat", "false"),
        ("core.trustctime", "true"),
    ] {
        let Some(result) = output(
            root,
            &["config", "--bool", "--get", key],
            4096,
            deadline,
            budget,
        )?
        else {
            return Ok(false);
        };
        match result.status.code() {
            Some(1) => {}
            Some(0)
                if std::str::from_utf8(&result.stdout).ok().map(str::trim) == Some(expected) => {}
            _ => return Ok(false),
        }
    }
    for (key, allowed) in [("core.checkstat", Some("default")), ("core.eol", None)] {
        let Some(result) = output(root, &["config", "--get", key], 4096, deadline, budget)? else {
            return Ok(false);
        };
        match result.status.code() {
            Some(1) => {}
            Some(0)
                if allowed.is_some()
                    && std::str::from_utf8(&result.stdout).ok().map(str::trim) == allowed => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

#[cfg(unix)]
fn supported_version(bytes: &[u8]) -> bool {
    let Some(version) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|v| v.strip_prefix("git version "))
    else {
        return false;
    };
    let mut parts = version.split('.');
    let major = parts.next().and_then(|n| n.parse::<u32>().ok());
    let minor = parts.next().and_then(|n| n.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 2 || (major == 2 && minor >= 43))
}

#[cfg(unix)]
fn parse_index(bytes: &[u8]) -> Option<BTreeMap<PathBuf, String>> {
    let mut result = BTreeMap::new();
    for row in bytes.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        let tab = row.iter().position(|byte| *byte == b'\t')?;
        let head = std::str::from_utf8(&row[..tab]).ok()?;
        let path = PathBuf::from(std::str::from_utf8(&row[tab + 1..]).ok()?);
        let mut fields = head.split_ascii_whitespace();
        let tag = fields.next()?;
        let mode = fields.next()?;
        let oid = fields.next()?;
        let stage = fields.next()?;
        if fields.next().is_some() {
            return None;
        }
        if tag != "H" || !matches!(mode, "100644" | "100755") || stage != "0" {
            continue;
        }
        if !matches!(oid.len(), 40 | 64)
            || !oid
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return None;
        }
        if result.insert(path, oid.to_owned()).is_some() {
            return None;
        }
    }
    Some(result)
}

#[cfg(unix)]
fn parse_status(bytes: &[u8], root: &Path, git_root: &Path) -> Option<BTreeSet<PathBuf>> {
    let prefix = crate::project::path::inputs::within(git_root, root)?;
    let mut changed = BTreeSet::new();
    for row in bytes.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        if row.len() < 4
            || row[2] != b' '
            || matches!(row[0], b'R' | b'C')
            || matches!(row[1], b'R' | b'C')
        {
            return None;
        }
        let path = Path::new(std::str::from_utf8(&row[3..]).ok()?);
        // The pathspec selects this ProductRoot. An unexpected path is an
        // untrusted observation, never evidence that all covered paths are clean.
        let relative = path.strip_prefix(prefix).ok()?;
        changed.insert(relative.to_path_buf());
    }
    Some(changed)
}

#[cfg(unix)]
fn input_stream(
    mut command: Command,
    input: Vec<u8>,
    stdout_limit: usize,
    deadline: Instant,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), ()>,
    budget: &mut ResumeInputBudget,
) -> Result<Option<crate::kernel::bounded_process::BoundedStreamOutput>, String> {
    use std::io::{ErrorKind, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;
    use std::time::Duration;
    let (mut writer, reader) = match UnixStream::pair() {
        Ok(pair) => pair,
        Err(_) => return Ok(None),
    };
    if writer.set_nonblocking(true).is_err() {
        return Ok(None);
    }
    command.stdin(Stdio::from(OwnedFd::from(reader)));
    let feeder = std::thread::spawn(move || {
        let mut at = 0usize;
        while at < input.len() {
            if Instant::now() >= deadline {
                return false;
            }
            match writer.write(&input[at..]) {
                Ok(0) => return false,
                Ok(count) => at += count,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return false,
            }
        }
        true
    });
    let result = crate::kernel::bounded_process::run_stream(
        &mut command,
        stdout_limit,
        65536,
        deadline,
        sink,
    );
    drop(command);
    let fed = feeder.join().unwrap_or(false);
    match result {
        Ok(result) if fed => Ok(Some(result)),
        Ok(_) => Ok(None),
        Err(error) => {
            process_error(error, budget)?;
            Ok(None)
        }
    }
}

#[cfg(unix)]
struct Candidate {
    path: PathBuf,
    oid: String,
    stat: Stat,
}

#[cfg(unix)]
enum BatchState {
    Header(Vec<u8>),
    Data {
        left: usize,
        hash: Sha256,
        matches_size: bool,
    },
    End {
        hash: Sha256,
        matches_size: bool,
    },
}

#[cfg(unix)]
struct BatchParser {
    candidates: Vec<Candidate>,
    at: usize,
    state: BatchState,
    digests: BTreeMap<PathBuf, CleanDigest>,
}

#[cfg(unix)]
impl BatchParser {
    fn new(candidates: Vec<Candidate>) -> Self {
        Self {
            candidates,
            at: 0,
            state: BatchState::Header(Vec::new()),
            digests: BTreeMap::new(),
        }
    }
    fn consume(&mut self, mut bytes: &[u8]) -> Result<(), ()> {
        while !bytes.is_empty() {
            if self.at >= self.candidates.len() {
                return Err(());
            }
            match &mut self.state {
                BatchState::Header(header) => {
                    let end = bytes.iter().position(|b| *b == b'\n');
                    let count = end.unwrap_or(bytes.len());
                    if header.len().saturating_add(count) > 128 {
                        return Err(());
                    }
                    header.extend_from_slice(&bytes[..count]);
                    bytes = &bytes[count..];
                    if end.is_some() {
                        bytes = &bytes[1..];
                        let line = std::str::from_utf8(header).map_err(|_| ())?;
                        let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
                        let candidate = &self.candidates[self.at];
                        if parts.first().copied() != Some(candidate.oid.as_str()) {
                            return Err(());
                        }
                        if parts.len() == 2 && parts[1] == "missing" {
                            self.at += 1;
                            self.state = BatchState::Header(Vec::new());
                        } else if parts.len() == 3 && parts[1] == "blob" {
                            let len = parts[2].parse::<usize>().map_err(|_| ())?;
                            if len as u64 > MAX_CONTENT_BYTES {
                                return Err(());
                            }
                            self.state = BatchState::Data {
                                left: len,
                                hash: Sha256::new(),
                                matches_size: len as u64 == candidate.stat.len,
                            };
                        } else {
                            return Err(());
                        }
                    }
                }
                BatchState::Data {
                    left,
                    hash,
                    matches_size,
                } => {
                    let count = (*left).min(bytes.len());
                    hash.update(&bytes[..count]);
                    bytes = &bytes[count..];
                    *left -= count;
                    if *left == 0 {
                        self.state = BatchState::End {
                            hash: std::mem::take(hash),
                            matches_size: *matches_size,
                        };
                    }
                }
                BatchState::End { hash, matches_size } => {
                    if bytes[0] != b'\n' {
                        return Err(());
                    }
                    bytes = &bytes[1..];
                    if *matches_size {
                        let candidate = &self.candidates[self.at];
                        self.digests.insert(
                            candidate.path.clone(),
                            CleanDigest {
                                sha256: format!("{:x}", std::mem::take(hash).finalize()),
                                stat: candidate.stat.clone(),
                            },
                        );
                    }
                    self.at += 1;
                    self.state = BatchState::Header(Vec::new());
                }
            }
        }
        Ok(())
    }
    fn finished(&self) -> bool {
        self.at == self.candidates.len()
            && matches!(&self.state, BatchState::Header(header) if header.is_empty())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn candidate(path: &str, oid: &str, len: u64) -> Candidate {
        Candidate {
            path: PathBuf::from(path),
            oid: oid.into(),
            stat: Stat {
                dev: 1,
                ino: 2,
                mode: 0o100644,
                len,
                mtime: 3,
                mtime_nsec: 4,
                ctime: 5,
                ctime_nsec: 6,
            },
        }
    }

    #[test]
    fn batch_stream_hashes_blob_bytes_and_leaves_missing_to_raw_fallback() {
        let first = "a".repeat(40);
        let missing = "b".repeat(40);
        let empty = "c".repeat(40);
        let mut parser = BatchParser::new(vec![
            candidate("normal", &first, 3),
            candidate("missing", &missing, 4),
            candidate("empty", &empty, 0),
        ]);
        let stream = format!("{first} blob 3\na\0b\n{missing} missing\n{empty} blob 0\n\n");
        for bytes in stream.as_bytes().chunks(2) {
            parser.consume(bytes).unwrap();
        }
        assert!(parser.finished());
        assert_eq!(
            parser.digests[Path::new("normal")].sha256,
            crate::evidence::hash::bytes(b"a\0b")
        );
        assert_eq!(
            parser.digests[Path::new("empty")].sha256,
            crate::evidence::hash::bytes(b"")
        );
        assert!(!parser.digests.contains_key(Path::new("missing")));
        assert!(parser.consume(b"unexpected").is_err());
    }

    #[test]
    fn index_flags_and_root_relative_status_never_treat_uncertain_files_as_clean() {
        let oid = "1".repeat(40);
        let input = format!("H 100644 {oid} 0\tclean\0h 100644 {oid} 0\tassumed\0S 100644 {oid} 0\tskipped\0H 120000 {oid} 0\tlink\0");
        let index = parse_index(input.as_bytes()).unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[Path::new("clean")], oid);
        let changed = parse_status(
            b" M nested/a\0?? nested/new\nline\0",
            Path::new("/repo/nested"),
            Path::new("/repo"),
        )
        .unwrap();
        assert!(changed.contains(Path::new("a")));
        assert!(changed.contains(Path::new("new\nline")));
        assert!(parse_status(
            b" M outside/file\0",
            Path::new("/repo/nested"),
            Path::new("/repo")
        )
        .is_none());
        assert!(parse_status(
            b" M outside/file\0",
            Path::new("/outside"),
            Path::new("/repo")
        )
        .is_none());
        assert!(parse_status(b"R  a\0b\0", Path::new("/repo"), Path::new("/repo")).is_none());
    }
}
