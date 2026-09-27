//! Assignment-scoped perspective sources. IDs and hashes survive in the child
//! intent; current bytes must still match before any context is presented.

use crate::{config::Loaded, evidence::hash, project::path};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const MAX_ITEMS: usize = 3;
const MAX_SOURCE: usize = 4 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Requested {
    id: String,
    path: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frozen {
    pub id: String,
    pub path: String,
    #[serde(rename = "sourceSha256")]
    pub source_sha256: String,
}

pub(super) fn freeze(loaded: &Loaded, source: Option<&str>) -> Result<Vec<Frozen>, String> {
    let Some(source) = source else {
        return Ok(Vec::new());
    };
    if source.len() > 1024 {
        return Err("perspective selector exceeds 1024 bytes".into());
    }
    let requested: Vec<Requested> = serde_json::from_str(source)
        .map_err(|_| "--perspectives must be a JSON list of {id,path} sources")?;
    if requested.is_empty() || requested.len() > MAX_ITEMS {
        return Err("select one to three perspectives".into());
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut frozen = Vec::new();
    for item in requested {
        if item.id.is_empty()
            || item.id.len() > 64
            || !item.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
            })
        {
            return Err(
                "perspective id must be 1-64 lowercase ASCII letters, digits, '-' or '_'".into(),
            );
        }
        if !ids.insert(item.id.clone()) || !paths.insert(item.path.clone()) {
            return Err("duplicate perspective id or path".into());
        }
        let bytes = path::secure_bytes(&loaded.control_root, &item.path, "perspective source")?;
        if bytes.len() > MAX_SOURCE || std::str::from_utf8(&bytes).is_err() {
            return Err("perspective source must be UTF-8 and at most 4096 bytes".into());
        }
        frozen.push(Frozen {
            id: item.id,
            path: item.path,
            source_sha256: hash::bytes(&bytes),
        });
    }
    Ok(frozen)
}

pub(super) fn current(loaded: &Loaded, sources: &[Frozen]) -> Result<Vec<Value>, String> {
    let mut values = Vec::new();
    for source in sources {
        let bytes = path::secure_bytes(&loaded.control_root, &source.path, "perspective source")?;
        if hash::bytes(&bytes) != source.source_sha256 {
            return Err(format!("perspective '{}' source changed", source.id));
        }
        let content = String::from_utf8(bytes).map_err(|_| "perspective source is not UTF-8")?;
        let presented = crate::host::runtime::redact(&content);
        values.push(json!({"id":source.id,"path":source.path,
            "sourceSha256":source.source_sha256,
            "presentedSha256":hash::text(&presented),"content":presented,
            "transformation":"path-redaction", "coverage":"complete"}));
    }
    Ok(values)
}
