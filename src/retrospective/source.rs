//! Bounded, read-only reader for the Codex 0.159.2 JSONL export shape.

use crate::evidence::hash;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const ADAPTER: &str = "codex-jsonl";
pub const VERSION: &str = "0.159.2";
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 64 * 1024;
pub const MAX_RECORDS: usize = 20_000;
pub const MAX_FILES: usize = 256;
pub const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub index: usize,
}

#[derive(Debug, Clone)]
pub struct SourceData {
    pub files: Vec<SourceFile>,
    pub digest: String,
    pub records: usize,
    pub malformed: usize,
    pub oversized: usize,
    pub unsupported_shapes: usize,
    pub unsupported_versions: Vec<String>,
    pub session_meta_seen: usize,
    pub metadata_missing: bool,
    pub files_missing_metadata: usize,
    pub bytes_read: u64,
    pub truncated: bool,
}

pub fn read(path: &Path) -> Result<SourceData, String> {
    if fs::symlink_metadata(path)
        .map_err(|error| format!("retrospective source cannot be inspected: {error}"))?
        .file_type()
        .is_symlink()
    {
        return Err("retrospective source refuses a symlink".into());
    }
    let root = fs::canonicalize(path)
        .map_err(|error| format!("retrospective source cannot be resolved: {error}"))?;
    let metadata = fs::symlink_metadata(&root)
        .map_err(|error| format!("retrospective source metadata: {error}"))?;
    let paths = if metadata.is_file() {
        vec![root]
    } else if metadata.is_dir() {
        let mut paths = Vec::new();
        collect_files(&root, &root, 0, &mut paths)?;
        paths.sort();
        if paths.is_empty() {
            return Err("retrospective source directory contains no JSONL files".into());
        }
        paths
    } else {
        return Err("retrospective source must be a regular file or directory".into());
    };
    let mut files = Vec::with_capacity(paths.len());
    let mut digest_input = Vec::new();
    let mut records = 0;
    let mut malformed = 0;
    let mut oversized = 0;
    let mut unsupported_shapes = 0;
    let mut unsupported_versions = Vec::new();
    let mut session_meta_seen = 0;
    let mut metadata_missing = true;
    let mut files_missing_metadata = 0;
    let mut total_bytes = 0u64;
    let mut truncated = false;
    for (index, path) in paths.into_iter().enumerate() {
        let metadata =
            fs::metadata(&path).map_err(|error| format!("retrospective source: {error}"))?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(format!(
                "retrospective source file exceeds {MAX_FILE_BYTES} bytes"
            ));
        }
        let bytes = read_stable(&path)?;
        total_bytes = total_bytes.saturating_add(bytes.len() as u64);
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(format!(
                "retrospective source exceeds {MAX_TOTAL_BYTES} bytes"
            ));
        }
        digest_input.extend_from_slice(
            path.to_str()
                .ok_or("retrospective source path must be UTF-8")?
                .as_bytes(),
        );
        digest_input.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        digest_input.extend_from_slice(&bytes);
        let mut file_records = 0usize;
        let mut file_supported_meta = false;
        for line in bytes.split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            records += 1;
            file_records += 1;
            if records > MAX_RECORDS {
                truncated = true;
                break;
            }
            if line.len() > MAX_LINE_BYTES {
                oversized += 1;
                continue;
            }
            match serde_json::from_slice::<Value>(line) {
                Ok(value) => {
                    let version_value = value.get("payload").unwrap_or(&value);
                    if let Some(version) = version_value
                        .get("cli_version")
                        .and_then(Value::as_str)
                        .or_else(|| value.get("version").and_then(Value::as_str))
                    {
                        if version != VERSION && !unsupported_versions.iter().any(|v| v == version)
                        {
                            unsupported_versions.push(version.to_owned());
                        }
                    }
                    let typ = value.get("type").and_then(Value::as_str).or_else(|| {
                        value
                            .get("payload")
                            .and_then(|v| v.get("type"))
                            .and_then(Value::as_str)
                    });
                    if typ == Some("session_meta") {
                        session_meta_seen += 1;
                        if version_value.get("cli_version").and_then(Value::as_str) == Some(VERSION)
                        {
                            metadata_missing = false;
                            file_supported_meta = true;
                        }
                    }
                    let payload_type = value
                        .get("payload")
                        .and_then(|v| v.get("type"))
                        .and_then(Value::as_str);
                    if !matches!(typ, Some("session_meta" | "response_item"))
                        && !matches!(payload_type, Some("session_meta" | "response_item"))
                    {
                        unsupported_shapes += 1;
                    }
                }
                Err(_) => malformed += 1,
            }
        }
        files.push(SourceFile { path, bytes, index });
        if file_records > 0 && !file_supported_meta {
            files_missing_metadata += 1;
        }
        if records > MAX_RECORDS {
            break;
        }
    }
    if records > MAX_RECORDS {
        records = MAX_RECORDS;
    }
    Ok(SourceData {
        files,
        digest: hash::bytes(&digest_input),
        records,
        malformed,
        oversized,
        unsupported_shapes,
        unsupported_versions,
        session_meta_seen,
        metadata_missing,
        files_missing_metadata,
        bytes_read: total_bytes,
        truncated,
    })
}

fn collect_files(
    root: &Path,
    current: &Path,
    depth: usize,
    result: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "retrospective source exceeds recursion depth {MAX_DEPTH}"
        ));
    }
    if result.len() >= MAX_FILES {
        return Err(format!("retrospective source exceeds {MAX_FILES} files"));
    }
    let mut entries = fs::read_dir(current)
        .map_err(|error| format!("retrospective source directory: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "retrospective source refuses symlink: {}",
                safe_name(&path)
            ));
        }
        if metadata.is_dir() {
            collect_files(root, &path, depth + 1, result)?;
        } else if metadata.is_file() && path.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            let canonical = fs::canonicalize(&path).map_err(|error| error.to_string())?;
            if !canonical.starts_with(root) {
                return Err("retrospective source path escapes its root".into());
            }
            result.push(canonical);
            if result.len() >= MAX_FILES {
                return Err(format!("retrospective source exceeds {MAX_FILES} files"));
            }
        }
    }
    Ok(())
}

pub fn lines(data: &SourceData) -> impl Iterator<Item = (usize, usize, &[u8])> {
    data.files
        .iter()
        .flat_map(|file| {
            file.bytes
                .split(|b| *b == b'\n')
                .enumerate()
                .filter(|(_, bytes)| !bytes.is_empty())
                .map(move |(line, bytes)| (file.index, line + 1, bytes))
        })
        .take(MAX_RECORDS)
        .filter(|(_, _, bytes)| bytes.len() <= MAX_LINE_BYTES)
}

fn read_stable(path: &Path) -> Result<Vec<u8>, String> {
    for attempt in 0..2 {
        let before =
            fs::metadata(path).map_err(|error| format!("retrospective source: {error}"))?;
        if before.len() > MAX_FILE_BYTES {
            return Err(format!(
                "retrospective source file exceeds {MAX_FILE_BYTES} bytes"
            ));
        }
        let file =
            fs::File::open(path).map_err(|error| format!("retrospective source: {error}"))?;
        let mut bytes = Vec::with_capacity(before.len() as usize);
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("retrospective source: {error}"))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(format!(
                "retrospective source file exceeds {MAX_FILE_BYTES} bytes"
            ));
        }
        let after = fs::metadata(path).map_err(|error| format!("retrospective source: {error}"))?;
        if before.len() == bytes.len() as u64
            && before.len() == after.len()
            && before.modified().ok() == after.modified().ok()
        {
            return Ok(bytes);
        }
        if attempt == 1 {
            return Err("retrospective source changed during bounded read".into());
        }
    }
    Err("retrospective source could not be read stably".into())
}

pub fn safe_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source")
        .to_owned()
}
