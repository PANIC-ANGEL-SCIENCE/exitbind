use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn bytes(input: &[u8]) -> String {
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
    match input {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON object key serializes"),
                        canonical(&map[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => {
            let values = values.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{values}]")
        }
        _ => serde_json::to_string(input).expect("JSON value serializes"),
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
