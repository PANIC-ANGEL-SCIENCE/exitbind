//! Exact, bounded Python source context for credential-assignment occurrences.
//! See `tests/privacy_gate/README.md` for the standalone test dependency.

#[path = "process.rs"]
mod process;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use super::patch_context::{self, Hunk};
use super::CREDENTIAL_KEYS;

const PYTHON_AST: &str = include_str!("python_ast.py");
const MAX_SOURCE_BYTES: usize = 512 * 1024;
const MAX_SOURCE_LINES: usize = 16_384;
const MAX_OBJECTS: usize = 16;
const MAX_CHILDREN: usize = 64;
const MAX_PARSER_OUTPUT: usize = 8 * 1024;
const MAX_OCCURRENCES: usize = 4096;

#[derive(Clone)]
pub(super) struct Options {
    pub(super) python: OsString,
    pub(super) child_timeout: Duration,
    pub(super) total_timeout: Duration,
    pub(super) max_source_bytes: usize,
    pub(super) max_objects: usize,
    pub(super) max_children: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            python: "python3".into(),
            child_timeout: Duration::from_secs(3),
            total_timeout: Duration::from_secs(30),
            max_source_bytes: MAX_SOURCE_BYTES,
            max_objects: MAX_OBJECTS,
            max_children: MAX_CHILDREN,
        }
    }
}

struct Blob {
    text: String,
    starts: Vec<usize>,
    eligible: Option<Option<BTreeSet<usize>>>,
}

impl Blob {
    fn new(bytes: Vec<u8>, limit: usize) -> Option<Self> {
        if bytes.len() > limit || bytes.contains(&b'\r') {
            return None;
        }
        let text = String::from_utf8(bytes).ok()?;
        let mut starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index + 1);
                if starts.len() > MAX_SOURCE_LINES {
                    return None;
                }
            }
        }
        Some(Self {
            text,
            starts,
            eligible: None,
        })
    }

    fn line(&self, number: usize) -> Option<(&str, bool)> {
        let start = *self.starts.get(number.checked_sub(1)?)?;
        let end = self.starts.get(number).copied().unwrap_or(self.text.len());
        let physical = &self.text[start..end];
        Some((
            physical.strip_suffix('\n').unwrap_or(physical),
            physical.ends_with('\n'),
        ))
    }
}

pub(super) struct Engine<'a> {
    root: &'a Path,
    options: Options,
    deadline: Option<Instant>,
    children: usize,
    blobs: BTreeMap<String, Option<Blob>>,
}

impl<'a> Engine<'a> {
    pub(super) fn new(root: &'a Path, options: Options) -> Self {
        Self {
            root,
            options,
            deadline: None,
            children: 0,
            blobs: BTreeMap::new(),
        }
    }

    fn run(
        &mut self,
        command: std::process::Command,
        input: &[u8],
        limit: usize,
    ) -> Option<Vec<u8>> {
        if self.children >= self.options.max_children {
            return None;
        }
        let now = Instant::now();
        let deadline = *self
            .deadline
            .get_or_insert(now.checked_add(self.options.total_timeout)?);
        let remaining = deadline.checked_duration_since(now)?;
        self.children += 1;
        process::run(
            command,
            input,
            limit,
            remaining.min(self.options.child_timeout),
        )
    }

    fn parse(&mut self, source: &[u8]) -> Option<BTreeSet<usize>> {
        if source.len() > self.options.max_source_bytes || source.len() > MAX_SOURCE_BYTES {
            return None;
        }
        let mut command = super::support::git_topology::command(&self.options.python);
        command.args(["-I", "-S", "-B", "-c", PYTHON_AST]);
        let output = self.run(command, source, MAX_PARSER_OUTPUT)?;
        let output = std::str::from_utf8(&output).ok()?;
        let mut lines = output.lines();
        if lines.next()? != "OK" {
            return None;
        }
        let mut eligible = BTreeSet::new();
        for line in lines {
            if line.is_empty() || !line.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let number: usize = line.parse().ok()?;
            if number == 0 || number > MAX_SOURCE_LINES || !eligible.insert(number) {
                return None;
            }
        }
        Some(eligible)
    }

    pub(super) fn direct_offsets(&mut self, source: &str) -> BTreeSet<usize> {
        if source.len() > self.options.max_source_bytes || source.len() > MAX_SOURCE_BYTES {
            return BTreeSet::new();
        }
        let mut possible = Vec::new();
        let mut offset = 0;
        for (index, part) in source.split_inclusive('\n').enumerate() {
            if index >= MAX_SOURCE_LINES || possible.len() >= MAX_OCCURRENCES {
                return BTreeSet::new();
            }
            let line = part.strip_suffix('\n').unwrap_or(part);
            if let Some(key_offset) = lexical_key_offset(line) {
                possible.push((offset + key_offset, index + 1));
            }
            offset += part.len();
        }
        if possible.is_empty() {
            return BTreeSet::new();
        }
        let Some(eligible) = self.parse(source.as_bytes()) else {
            return BTreeSet::new();
        };
        possible
            .into_iter()
            .filter_map(|(offset, line)| eligible.contains(&line).then_some(offset))
            .collect()
    }

    fn load_blob(&mut self, oid: &str) -> bool {
        if self.blobs.contains_key(oid) {
            return self.blobs.get(oid).is_some_and(Option::is_some);
        }
        if self.blobs.len() >= self.options.max_objects {
            return false;
        }
        let mut size_command = super::support::git_topology::git(self.root);
        size_command
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(["cat-file", "-s", oid]);
        let loaded = self.run(size_command, b"", 32).and_then(|size| {
            let size = std::str::from_utf8(&size).ok()?.strip_suffix('\n')?;
            if size.is_empty() || !size.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let size: usize = size.parse().ok()?;
            if size > self.options.max_source_bytes || size > MAX_SOURCE_BYTES {
                return None;
            }
            let mut content_command = super::support::git_topology::git(self.root);
            content_command
                .env("GIT_NO_REPLACE_OBJECTS", "1")
                .args(["cat-file", "blob", oid]);
            let bytes = self.run(content_command, b"", size)?;
            if bytes.len() != size {
                return None;
            }
            Blob::new(bytes, self.options.max_source_bytes)
        });
        let valid = loaded.is_some();
        self.blobs.insert(oid.to_owned(), loaded);
        valid
    }

    fn eligible_line(&mut self, oid: &str, line: usize) -> bool {
        let needs_parse = self
            .blobs
            .get(oid)
            .and_then(Option::as_ref)
            .is_some_and(|blob| blob.eligible.is_none());
        if needs_parse {
            let source = self.blobs[oid].as_ref().unwrap().text.as_bytes().to_vec();
            let parsed = self.parse(&source);
            self.blobs.get_mut(oid).unwrap().as_mut().unwrap().eligible = Some(parsed);
        }
        self.blobs
            .get(oid)
            .and_then(Option::as_ref)
            .and_then(|blob| blob.eligible.as_ref())
            .and_then(Option::as_ref)
            .is_some_and(|eligible| eligible.contains(&line))
    }

    fn check_hunk(&mut self, hunk: &Hunk<'_>, offsets: &mut BTreeSet<usize>) {
        if !(hunk.old_python || hunk.new_python)
            || !hunk
                .lines
                .iter()
                .any(|line| line.marker != b' ' && lexical_key_offset(line.text).is_some())
        {
            return;
        }
        for oid in [hunk.old_oid, hunk.new_oid].into_iter().flatten() {
            if !self.load_blob(oid) {
                return;
            }
        }
        for line in &hunk.lines {
            for (oid, number) in [(hunk.old_oid, line.old), (hunk.new_oid, line.new)] {
                if let Some(number) = number {
                    let Some(blob) = oid
                        .and_then(|oid| self.blobs.get(oid))
                        .and_then(Option::as_ref)
                    else {
                        return;
                    };
                    if blob.line(number) != Some((line.text, !line.no_newline)) {
                        return;
                    }
                }
            }
        }
        for line in &hunk.lines {
            let Some(key_offset) = lexical_key_offset(line.text) else {
                continue;
            };
            let (oid, number, python_path) = match line.marker {
                b'-' => (hunk.old_oid, line.old, hunk.old_python),
                b'+' => (hunk.new_oid, line.new, hunk.new_python),
                _ => continue,
            };
            if let (Some(oid), Some(number), true) = (oid, number, python_path) {
                if self.eligible_line(oid, number) {
                    offsets.insert(line.patch_offset + 1 + key_offset);
                }
            }
        }
    }

    pub(super) fn patch_offsets(&mut self, patch: &str) -> BTreeSet<usize> {
        let mut offsets = BTreeSet::new();
        let mut exhausted = false;
        patch_context::visit_hunks(patch, |hunk| {
            if !exhausted {
                self.check_hunk(hunk, &mut offsets);
                if offsets.len() > MAX_OCCURRENCES {
                    offsets.clear();
                    exhausted = true;
                }
            }
        });
        offsets
    }
}

// This is only a cheap prefilter. The complete source AST must confirm it.
pub(super) fn lexical_key_offset(line: &str) -> Option<usize> {
    let (left, right) = line.split_once('=')?;
    let without_indent = left.trim_start_matches([' ', '\t']);
    let indent = left.len() - without_indent.len();
    let key = without_indent.trim_end_matches([' ', '\t']);
    if key.len() == without_indent.len() || !CREDENTIAL_KEYS.contains(&key) {
        return None;
    }
    let trimmed = right.trim_start_matches([' ', '\t']);
    if trimmed.len() == right.len() {
        return None;
    }
    let value = trimmed.trim_end_matches([' ', '\t']);
    let (receiver, argument) = value.split_once(".group(")?;
    let mut chars = receiver.bytes();
    if receiver.len() < 6
        || !chars
            .next()
            .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        || !chars.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
    {
        return None;
    }
    let argument = argument.strip_suffix(')')?.trim_matches([' ', '\t']);
    if argument.len() != 1 || !argument.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    Some(indent)
}
