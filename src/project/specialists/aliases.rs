//! Recognize complete reserved sources in bounded conventional content envelopes.
use super::*;

fn reserved_json(bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(value) => {
                if [
                    "version",
                    "author",
                    "projectIdentity",
                    "taskTerms",
                    "hosts",
                    "procedure",
                    "verification",
                    "requiredCommands",
                    "guards",
                ]
                .iter()
                .all(|key| value.contains_key(*key))
                {
                    return true;
                }
                pending.extend(value.into_values());
            }
            Value::Array(values) => pending.extend(values),
            Value::String(text) => {
                // Decoding removes an enclosing string/escapes, so repeated
                // conventional string envelopes shrink within the input bound.
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    pending.push(value);
                } else if reserved_fences(&text) {
                    return true;
                }
            }
            _ => (),
        }
    }
    false
}

fn fence(line: &str) -> Option<(u8, usize, &str)> {
    let line = line.trim_end_matches(['\r', '\n']);
    let content = line.trim_start_matches(' ');
    if line.len() - content.len() > 3 {
        return None;
    }
    let marker = *content.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let length = content.bytes().take_while(|byte| *byte == marker).count();
    (length >= 3).then_some((marker, length, &content[length..]))
}

pub(crate) fn reserved_source(bytes: &[u8]) -> bool {
    if bytes.len() > 4 * 1024 * 1024 {
        return false;
    }
    if reserved_json(bytes) {
        return true;
    }
    std::str::from_utf8(bytes).is_ok_and(reserved_fences)
}

fn reserved_fences(text: &str) -> bool {
    // Linear scan within the existing maximum generic source envelope. Inspect
    // complete JSON bodies only; loose prose, field names and paraphrases are
    // not classified as specialist experience.
    let mut opening: Option<(u8, usize, usize)> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some((marker, length, suffix)) = fence(line) {
            match opening {
                Some((expected, minimum, start))
                    if marker == expected && length >= minimum && suffix.trim().is_empty() =>
                {
                    let body = text[start..offset].trim();
                    if body.len() <= 4 * 1024 * 1024 && reserved_json(body.as_bytes()) {
                        return true;
                    }
                    opening = None;
                }
                None if marker != b'`' || !suffix.contains('`') => {
                    opening = Some((marker, length, offset + line.len()));
                }
                _ => (),
            }
        }
        offset += line.len();
    }
    false
}
