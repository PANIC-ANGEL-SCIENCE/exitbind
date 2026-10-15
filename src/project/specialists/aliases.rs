//! Recognize complete reserved sources in bounded conventional content envelopes.
use super::*;

fn reserved_json(bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    reserved_value(value)
}

fn reserved_value(value: Value) -> bool {
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
                } else if reserved_text(&text) {
                    return true;
                }
            }
            _ => (),
        }
    }
    false
}

pub(crate) fn reserved_source(bytes: &[u8]) -> bool {
    if bytes.len() > 4 * 1024 * 1024 {
        return false;
    }
    if reserved_json(bytes) {
        return true;
    }
    std::str::from_utf8(bytes).is_ok_and(reserved_text)
}

fn container_content(mut line: &str) -> &str {
    loop {
        line = line.trim_start_matches([' ', '\t']);
        if let Some(content) = line.strip_prefix('>') {
            line = content;
            continue;
        }
        let bytes = line.as_bytes();
        let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        let marker = if matches!(bytes.first(), Some(b'-' | b'+' | b'*')) {
            1
        } else if (1..=9).contains(&digits) && matches!(bytes.get(digits), Some(b'.' | b')')) {
            digits + 1
        } else {
            0
        };
        if marker > 0 && matches!(bytes.get(marker), Some(b' ' | b'\t')) {
            line = &line[marker + 1..];
            continue;
        }
        return line;
    }
}

fn reserved_text(text: &str) -> bool {
    // Remove only conventional line-container syntax. Inspect complete JSON
    // values wherever explanatory/code text places them, independent of fence
    // labels or closure (including EOF). Never classify loose words as a source.
    let mut normalized = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        normalized.push_str(container_content(line));
    }
    let mut offset = 0;
    while let Some(relative) = normalized.as_bytes()[offset..]
        .iter()
        .position(|b| matches!(b, b'{' | b'[' | b'"'))
    {
        let start = offset + relative;
        let mut values =
            serde_json::Deserializer::from_str(&normalized[start..]).into_iter::<Value>();
        if let Some(Ok(value)) = values.next() {
            if reserved_value(value) {
                return true;
            }
        }
        // An unsuccessful or ordinary quoted fragment must not hide a complete
        // embedded object later in the same explanatory line.
        offset = start + 1;
    }
    false
}
