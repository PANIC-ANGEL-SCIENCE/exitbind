// Computes SHA-256 for bytes, files and canonical JSON.
// Canonical object keys are sorted before hashing; see `src/evidence/mod.rs`.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{ErrorKind, Read};
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
    file_with_observer(path, |_| Ok(()))
}

/// Hash the raw file bytes while allowing a caller to check its own limits.
/// The observer receives `None` before each read and `Some(bytes)` after each
/// nonempty read. Ordinary file hashing supplies no limits.
pub(crate) fn file_with_observer(
    path: &Path,
    mut observe: impl FnMut(Option<usize>) -> Result<(), String>,
) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        observe(None)?;
        let count = match file.read(&mut chunk) {
            Ok(count) => count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.to_string()),
        };
        if count == 0 {
            break;
        }
        observe(Some(count))?;
        hasher.update(&chunk[..count]);
    }
    Ok(hex(&hasher.finalize()))
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

#[cfg(test)]
mod streaming_tests {
    use super::*;
    use std::fs;

    #[test]
    fn streaming_file_hash_matches_former_raw_byte_hash_and_symlinks() {
        let root = crate::test_root::temp("streaming-file-sha");
        let path = root.join("data");
        for len in [0, 1, 16_383, 16_384, 16_385, 70_001] {
            let contents = (0..len)
                .map(|index| (index % 251) as u8)
                .collect::<Vec<_>>();
            fs::write(&path, &contents).unwrap();
            let former = bytes(&fs::read(&path).unwrap());
            assert_eq!(file(&path).unwrap(), former, "length {len}");
        }
        #[cfg(unix)]
        {
            let link = root.join("link");
            std::os::unix::fs::symlink("data", &link).unwrap();
            assert_eq!(file(&link).unwrap(), bytes(&fs::read(&link).unwrap()));
        }
        let missing = root.join("missing");
        assert_eq!(
            file(&missing).unwrap_err(),
            fs::read(&missing).unwrap_err().to_string()
        );
    }
}
