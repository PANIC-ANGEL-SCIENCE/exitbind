//! Recognize complete reserved sources in bounded conventional content envelopes.
use super::*;

pub(crate) fn reserved_source(bytes: &[u8]) -> bool {
    Inspector::default().source(bytes)
}

#[derive(Default)]
struct Inspector {
    #[cfg(test)]
    parser_input_bytes: usize,
}

impl Inspector {
    fn parse(&mut self, bytes: &[u8]) -> Option<(Value, usize)> {
        #[cfg(test)]
        {
            self.parser_input_bytes += bytes.len();
        }
        let mut values = serde_json::Deserializer::from_slice(bytes).into_iter::<Value>();
        let value = values.next()?.ok()?;
        Some((value, values.byte_offset()))
    }

    fn source(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > 4 * 1024 * 1024 {
            return false;
        }
        if let Some((value, end)) = self.parse(bytes) {
            if self.value(value) {
                return true;
            }
            if bytes[end..].iter().all(u8::is_ascii_whitespace) {
                return false;
            }
        }
        std::str::from_utf8(bytes).is_ok_and(|text| self.text(text))
    }

    fn value(&mut self, value: Value) -> bool {
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
                    if self.source(text.as_bytes()) {
                        return true;
                    }
                }
                _ => (),
            }
        }
        false
    }

    fn text(&mut self, text: &str) -> bool {
        // Remove only conventional line-container syntax. Inspect complete JSON
        // values wherever explanatory/code text places them, independent of fence
        // labels or closure (including EOF). Never classify loose words as a source.
        let mut normalized = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            normalized.push_str(container_content(line));
        }
        // Complete containers are inspected once, including all decoded values.
        // Object scanning is independent of prose quotes, so an ordinary quoted
        // fragment cannot hide a raw reserved object spanning its boundary.
        let mut covered = Vec::new();
        let mut offset = 0;
        while let Some(relative) = normalized.as_bytes()[offset..]
            .iter()
            .position(|b| matches!(b, b'{' | b'['))
        {
            let start = offset + relative;
            if let Some((value, length)) = self.parse(&normalized.as_bytes()[start..]) {
                if self.value(value) {
                    return true;
                }
                offset = start + length;
                covered.push((start, offset));
            } else {
                offset = start + 1;
            }
        }

        let (mut offset, mut container) = (0, 0);
        while let Some(relative) = normalized.as_bytes()[offset..]
            .iter()
            .position(|b| *b == b'"')
        {
            let start = offset + relative;
            while container < covered.len() && covered[container].1 <= start {
                container += 1;
            }
            if container < covered.len() && covered[container].0 <= start {
                offset = covered[container].1;
                continue;
            }
            let Some(end) = quote_end(normalized.as_bytes(), start) else {
                break;
            };
            if let Some((value, _)) = self.parse(&normalized.as_bytes()[start..end]) {
                if self.value(value) {
                    return true;
                }
            }
            // Only the closing delimiter overlaps: it may also open the next
            // explanatory fragment. Escaped interior quotes never start new parses.
            offset = end - 1;
        }
        false
    }
}

fn quote_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut offset = start + 1;
    while offset < bytes.len() {
        match bytes[offset] {
            b'\\' => offset += 2,
            b'"' => return Some(offset + 1),
            _ => offset += 1,
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_and_malformed_quotes_have_bounded_parser_work_and_stay_ordinary() {
        for count in [256, 4096] {
            for suffix in ["\"\n", ""] {
                let source = format!(
                    "Ordinary escaped quote example: \"{}{suffix}",
                    "\\\"".repeat(count)
                );
                let mut inspector = Inspector::default();
                assert!(!inspector.source(source.as_bytes()));
                assert!(
                    inspector.parser_input_bytes <= source.len() * 4,
                    "{} parser-input bytes for {} ordinary source bytes",
                    inspector.parser_input_bytes,
                    source.len()
                );
            }
        }
    }
}
