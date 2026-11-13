// Frames bounded Claude start-hook delivery and acknowledgement.
// Refuses oversized child envelopes; see `src/host/runtime.rs`.
//! Lossless, bounded Claude start-hook framing and write acknowledgement.

use serde_json::{json, Value};
use std::io::Write;

pub(crate) const SLOTS: usize = 4;
const EVENT: &str = "SubagentStart";

pub(crate) type Facts<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);

pub(crate) enum Delivery<'a> {
    Legacy {
        binding: Option<&'a str>,
        permit: Option<&'a Value>,
        facts: Option<Facts<'a>>,
    },
    Select {
        child: &'a Value,
        bundle: &'a Bundle,
        slot: usize,
    },
    Receipt {
        bundle: &'a str,
        slot: usize,
        piece: &'a str,
    },
    Invalidate,
}

#[derive(Clone, Debug)]
pub(crate) struct Bundle {
    pub(crate) digest: String,
    pub(crate) parts: Vec<String>,
}

pub(crate) struct Piece {
    pub(crate) bundle: String,
    pub(crate) slot: usize,
    pub(crate) digest: String,
    pub(crate) text: String,
}

fn framed(digest: &str, index: usize, total: usize, payload: &str) -> String {
    // Claude rewrites literal system-reminder tags in additionalContext.
    // Escaping these two characters makes that transformation reversible
    // without hiding the selected context behind a separate read route.
    let encoded = payload.replace('&', "&amp;").replace('<', "&lt;");
    format!("Exitbind child context bundle SHA-256 {digest}; part {index} of {total}; decoded payload {} UTF-8 bytes. To reconstruct, order all parts by number. Use identical duplicate parts once; conflicting parts or missing numbers make the context incomplete. Scan the payload after the first blank line of each part once from left to right: &lt; means < and &amp; means &; do not decode a replacement again. Concatenate decoded payloads without separators. A boundary may continue inside a token; interpret the context after joining all parts.\n\n{encoded}", payload.len())
}

fn fits(text: &str) -> bool {
    crate::host::assignment_evidence::append_for_claude(text, EVENT, &[]).is_some()
}

fn partition(text: &str, digest: &str, semantic: bool) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() && parts.len() < SLOTS {
        let mut boundaries = remaining.char_indices().map(|(i, _)| i).collect::<Vec<_>>();
        boundaries.push(remaining.len());
        let (mut low, mut high) = (0, boundaries.len());
        while low + 1 < high {
            let mid = (low + high) / 2;
            let candidate = framed(
                digest,
                parts.len() + 1,
                SLOTS,
                &remaining[..boundaries[mid]],
            );
            if fits(&candidate) {
                low = mid;
            } else {
                high = mid;
            }
        }
        let mut end = boundaries[low];
        if end == 0 {
            return None;
        }
        if semantic && end < remaining.len() {
            let boundary = remaining[..end].rfind('\n').map_or(0, |index| index + 1);
            if boundary > 0 && boundary >= end / 2 {
                end = boundary;
            }
        }
        parts.push(remaining[..end].to_owned());
        remaining = &remaining[end..];
    }
    remaining.is_empty().then_some(parts)
}

pub(crate) fn bundle(text: &str) -> Result<Bundle, String> {
    let digest = crate::evidence::hash::text(text);
    let payloads = partition(text, &digest, true)
        .or_else(|| partition(text, &digest, false))
        .filter(|parts| !parts.is_empty())
        .ok_or("complete child assignment exceeds the bounded Claude multipart capacity")?;
    let total = payloads.len();
    let parts = payloads
        .iter()
        .enumerate()
        .map(|(index, payload)| framed(&digest, index + 1, total, payload))
        .collect::<Vec<_>>();
    if parts.iter().any(|part| !fits(part)) {
        return Err("complete child multipart envelope exceeds a Claude limit".into());
    }
    Ok(Bundle { digest, parts })
}

fn output(text: &str) -> Result<Vec<u8>, String> {
    if !fits(text) {
        return Err("child multipart envelope exceeds a Claude limit".into());
    }
    let mut output = serde_json::to_vec(&json!({"hookSpecificOutput": {
        "hookEventName": EVENT, "additionalContext": text
    }}))
    .map_err(|error| error.to_string())?;
    output.push(b'\n');
    Ok(output)
}

pub(crate) fn output_digest(text: &str) -> Result<String, String> {
    output(text).map(|bytes| crate::evidence::hash::bytes(&bytes))
}

/// A receipt is permitted only after the complete JSON and newline are flushed.
pub(crate) fn write_piece(writer: &mut impl Write, text: &str) -> Result<String, String> {
    let output = output(text)?;
    writer
        .write_all(&output)
        .and_then(|()| writer.flush())
        .map_err(|error| error.to_string())?;
    Ok(crate::evidence::hash::bytes(&output))
}

pub(super) fn invalidate(loaded: &crate::config::Loaded, payload: &serde_json::Map<String, Value>) {
    let _ = super::retry_busy(|| {
        crate::session_goal::note_child_delivery(loaded, payload, Delivery::Invalidate)
    });
}

pub(super) fn deliver(
    loaded: &crate::config::Loaded,
    payload: &serde_json::Map<String, Value>,
    child: &Value,
    text: &str,
    slot: usize,
) -> Result<(), String> {
    let bundle = match bundle(text) {
        Ok(bundle) => bundle,
        Err(error) => {
            invalidate(loaded, payload);
            return Err(error);
        }
    };
    let selected = super::retry_busy(|| {
        crate::session_goal::note_child_delivery(
            loaded,
            payload,
            Delivery::Select {
                child,
                bundle: &bundle,
                slot,
            },
        )
    })?;
    if let Some(piece) = selected {
        let emitted = write_piece(&mut std::io::stdout().lock(), &piece.text)?;
        if emitted != piece.digest {
            invalidate(loaded, payload);
            return Err("child multipart output does not match its selected receipt".into());
        }
        super::context::report_delivery_record(super::retry_busy(|| {
            crate::session_goal::note_child_delivery(
                loaded,
                payload,
                Delivery::Receipt {
                    bundle: &piece.bundle,
                    slot: piece.slot,
                    piece: &piece.digest,
                },
            )
            .map(|_| ())
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn payload(part: &str) -> String {
        part.split_once("\n\n")
            .unwrap()
            .1
            .replace("&lt;", "<")
            .replace("&amp;", "&")
    }

    #[test]
    fn pieces_reconstruct_exact_unicode_and_escaped_content_with_both_limits() {
        let original = format!(
            "section one <system-reminder>keep</system-reminder> &lt; &amp;\n{}\n{}",
            "🦊\\\"\n".repeat(2200),
            "한".repeat(6000)
        );
        let bundle = bundle(&original).unwrap();
        assert!(bundle.parts.len() > 1);
        assert_eq!(
            bundle
                .parts
                .iter()
                .map(|part| payload(part))
                .collect::<String>(),
            original
        );
        for (index, part) in bundle.parts.iter().enumerate() {
            assert!(!part.contains("<system-reminder>"));
            assert!(!part.contains("</system-reminder>"));
            let reported: usize = part
                .split("decoded payload ")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(reported, payload(part).len());
            let (bytes, units) =
                crate::host::assignment_evidence::claude_size(EVENT, part).unwrap();
            assert!(bytes <= 16 * 1024 && units <= 10_000);
            assert!(part.contains(&format!("part {} of {}", index + 1, bundle.parts.len())));
            assert!(part.contains(&bundle.digest));
        }
    }

    #[test]
    fn overflow_refuses_and_single_part_remains_bounded() {
        assert!(bundle(&"x".repeat(100_000)).is_err());
        let single = bundle("literal text").unwrap();
        assert_eq!(single.parts.len(), 1);
        assert_eq!(payload(&single.parts[0]), "literal text");
    }

    struct FlushFails;
    impl Write for FlushFails {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
    }

    #[test]
    fn failed_flush_is_not_a_successful_emission() {
        let part = &bundle("text").unwrap().parts[0];
        assert!(write_piece(&mut FlushFails, part).is_err());
        let mut bytes = Vec::new();
        let digest = write_piece(&mut bytes, part).unwrap();
        assert!(bytes.ends_with(b"\n"));
        assert_eq!(digest, crate::evidence::hash::bytes(&bytes));
    }
}
