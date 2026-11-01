//! Bounded, current-reference pages of a verified observed check log.

use crate::{config::Loaded, emitted::Route, run::artifact};
use serde_json::{json, Value};
use std::path::Path;

// Reuse the existing compact response budget; serialization below also
// accounts for escaped text, metadata, and the continuation commands.
const PAGE_BYTES: usize = super::super::compact::MAX_RESPONSE_BYTES / 2;

pub(super) struct Page<'a> {
    pub(super) base: &'a str,
    stream: &'a str,
    cursor: Cursor,
}

#[derive(Clone, Copy)]
enum Cursor {
    Offset(u64),
    Tail(u64),
}

pub(super) fn parse(reference: &str) -> Result<Option<Page<'_>>, String> {
    let Some((base, suffix)) = reference.split_once(":log:") else {
        return Ok(None);
    };
    let valid_base = base.strip_prefix("ref:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    let mut fields = suffix.split(':');
    let stream = fields.next().ok_or("check-log page has no stream")?;
    let kind = fields.next().ok_or("check-log page has no cursor")?;
    let position = fields
        .next()
        .ok_or("check-log page has no position")?
        .parse::<u64>()
        .map_err(|_| "check-log page position is invalid")?;
    if !valid_base || !matches!(stream, "stdout" | "stderr") || fields.next().is_some() {
        return Err("check-log page reference is invalid".into());
    }
    let cursor = match kind {
        "offset" => Cursor::Offset(position),
        "tail" => Cursor::Tail(position),
        _ => return Err("check-log page cursor is invalid".into()),
    };
    Ok(Some(Page {
        base,
        stream,
        cursor,
    }))
}

fn route(config: &Path, work: &str, base: &str, stream: &str, cursor: Cursor) -> Value {
    let (kind, position) = match cursor {
        Cursor::Offset(offset) => ("offset", offset),
        Cursor::Tail(total) => ("tail", total),
    };
    let reference = format!("{base}:log:{stream}:{kind}:{position}");
    let command = Route::new(["work", "expand", work, &reference, "--json"]);
    let mut route = super::super::compact::continuation_route(config, command);
    route["readOnly"] = json!(true);
    route
}

pub(super) fn attach_routes(
    loaded: &Loaded,
    work: &str,
    reference: &Value,
    stream: &str,
    artifact: &artifact::VerifiedArtifact,
    value: &mut Value,
) -> Result<(), String> {
    if artifact.bytes <= artifact.preview.len() as u64 {
        return Ok(());
    }
    let base = reference["id"]
        .as_str()
        .ok_or("check-log reference is unavailable")?;
    value["continuation"] = json!({
        "next": route(&loaded.path, work, base, stream, Cursor::Offset(artifact.preview.len() as u64)),
        "tail": route(&loaded.path, work, base, stream, Cursor::Tail(artifact.bytes)),
    });
    Ok(())
}

/// Add small direct tail commands to the compact Root reference; no log bytes
/// need to enter the parent context before choosing the relevant stream.
pub(super) fn attach_compact_routes(config: &Path, work: &str, reference: &mut Value) {
    if reference["kind"] != "check_log" {
        return;
    }
    let Some(base) = reference["id"].as_str().map(str::to_owned) else {
        return;
    };
    let mut tails = serde_json::Map::new();
    for stream in ["stdout", "stderr"] {
        if let Some(total) = reference[stream]["bytes"]
            .as_u64()
            .filter(|bytes| *bytes > 0)
        {
            tails.insert(
                stream.to_owned(),
                route(config, work, &base, stream, Cursor::Tail(total)),
            );
        }
    }
    if !tails.is_empty() {
        reference["boundedTail"] = Value::Object(tails);
    }
}

fn continuation_byte(byte: u8) -> bool {
    byte & 0xc0 == 0x80
}

/// The selected window and its complete stream are read together. The other
/// stream is also revalidated as part of the existing check-log contract.
pub(super) fn read(
    loaded: &Loaded,
    work: &str,
    reference: &Value,
    event: &Value,
    page: &Page<'_>,
) -> Result<Value, String> {
    let selected = &event[page.stream];
    let other = if page.stream == "stdout" {
        "stderr"
    } else {
        "stdout"
    };
    artifact::read(loaded, &event[other], other)?;
    let requested = match page.cursor {
        Cursor::Offset(offset) => offset,
        Cursor::Tail(total) => total.saturating_sub(PAGE_BYTES as u64),
    };
    let window_offset = requested.saturating_sub(3);
    let (verified, window) =
        artifact::read_window(loaded, selected, page.stream, window_offset, PAGE_BYTES + 6)?;
    if let Cursor::Tail(total) = page.cursor {
        if verified.bytes != total {
            return Err("check-log tail reference is stale".into());
        }
    }
    if requested > verified.bytes || (requested == verified.bytes && verified.bytes != 0) {
        return Err("check-log page offset is unavailable".into());
    }
    let anchor = usize::try_from(requested - window_offset)
        .map_err(|_| "check-log page offset is unavailable")?;
    if anchor > window.len() {
        return Err("check-log page bytes are unavailable".into());
    }

    let mut limit = PAGE_BYTES;
    loop {
        let (mut start, mut end) = match page.cursor {
            Cursor::Offset(_) => {
                let mut start = anchor;
                let mut moved = 0;
                while start > 0
                    && start < window.len()
                    && continuation_byte(window[start])
                    && moved < 3
                {
                    start -= 1;
                    moved += 1;
                }
                (start, start.saturating_add(limit).min(window.len()))
            }
            Cursor::Tail(_) => {
                let end = window.len();
                let mut start = end.saturating_sub(limit);
                let mut moved = 0;
                while start < end && continuation_byte(window[start]) && moved < 3 {
                    start += 1;
                    moved += 1;
                }
                (start, end)
            }
        };
        if matches!(page.cursor, Cursor::Offset(_)) && end < window.len() {
            let original = end;
            while end > start && continuation_byte(window[end]) {
                end -= 1;
            }
            if end == start {
                end = original;
            }
        }
        if start == end && !window.is_empty() {
            start = end.saturating_sub(1);
        }
        let offset = window_offset + start as u64;
        let next = window_offset + end as u64;
        if matches!(page.cursor, Cursor::Offset(_))
            && next <= requested
            && requested < verified.bytes
        {
            return Err("check-log page cannot advance within the response budget".into());
        }
        let bytes = &window[start..end];
        let mut value = json!({"valid":true,"kind":"check_log_page","stream":page.stream,
            "reference":reference["id"],"sha256":verified.sha256,"totalBytes":verified.bytes,
            "offset":offset,"bytes":bytes.len(),"readOnly":true,
            "next":if next < verified.bytes { route(&loaded.path, work, page.base, page.stream, Cursor::Offset(next)) } else { Value::Null }});
        if let Ok(text) = std::str::from_utf8(bytes) {
            value["encoding"] = json!("utf-8");
            value["contentText"] = json!(text);
        } else {
            value["encoding"] = json!("hex");
            value["contentHex"] = json!(super::hex(bytes));
        }
        let rendered = super::super::effective_action::attach(&value);
        if serde_json::to_vec(&rendered)
            .map_err(|error| error.to_string())?
            .len()
            + 1
            <= super::super::compact::MAX_RESPONSE_BYTES
        {
            return Ok(value);
        }
        if limit <= 1 {
            return Err("check-log page metadata exceeds the existing response budget".into());
        }
        limit /= 2;
    }
}
