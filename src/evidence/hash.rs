use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn bytes(input: &[u8]) -> String {
    #[cfg(test)]
    let _timer = crate::session_goal::recovery_diagnostic::timer(
        crate::session_goal::recovery_diagnostic::Phase::Digest,
    );
    hex(&Sha256::digest(input))
}

pub fn text(input: &str) -> String {
    bytes(input.as_bytes())
}

pub fn file(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|content| bytes(&content))
        .map_err(|error| error.to_string())
}

pub fn value(input: &Value) -> String {
    text(&canonical(input))
}

pub fn canonical(input: &Value) -> String {
    #[cfg(test)]
    let _timer = crate::session_goal::recovery_diagnostic::timer(
        crate::session_goal::recovery_diagnostic::Phase::Canonical,
    );
    let mut output = Vec::new();
    append_canonical(input, &mut output);
    String::from_utf8(output).expect("canonical JSON is valid UTF-8")
}

fn append_canonical(input: &Value, output: &mut Vec<u8>) {
    match input {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            output.push(b'{');
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key).expect("JSON object key serializes");
                output.push(b':');
                append_canonical(&map[key], output);
            }
            output.push(b'}');
        }
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                append_canonical(value, output);
            }
            output.push(b']');
        }
        _ => {
            serde_json::to_writer(&mut *output, input).expect("JSON value serializes");
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests;
