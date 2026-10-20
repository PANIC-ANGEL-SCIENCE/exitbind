//! Readable page content, so a reader never needs a hex decoder.
//!
//! UTF-8 work-section pages carry their exact bytes as `contentText`; evidence
//! artifact reads add `contentText` beside `contentHex`. Either form is the
//! exact bytes, so recorded hashes still verify them.
use serde_json::{json, Value};

/// Lowercase hex of `bytes`, the hash-verified evidence form of a page.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

/// The exact text of `bytes`, or `None` when they are not valid UTF-8 (for
/// example a page boundary that splits a multi-byte character).
pub(crate) fn text(bytes: &[u8]) -> Value {
    std::str::from_utf8(bytes).map_or(Value::Null, |text| json!(text))
}

/// A work-section page whose bytes are UTF-8 carries them as `contentText`
/// with `encoding: "utf-8"` instead of hex; other pages keep `contentHex`.
/// Either way the page bytes are exact, so the section hash still verifies
/// the reassembled section.
pub(crate) fn prefer_text(response: &mut Value, bytes: &[u8]) {
    let content = text(bytes);
    if content.is_null() {
        return;
    }
    if let Some(object) = response.as_object_mut() {
        object.remove("contentHex");
        object.insert("encoding".into(), json!("utf-8"));
        object.insert("contentText".into(), content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_pages_carry_text_and_other_pages_keep_hex() {
        let mut page = json!({"encoding":"hex","contentHex":"c3a9"});
        prefer_text(&mut page, "é".as_bytes());
        assert_eq!(page, json!({"encoding":"utf-8","contentText":"é"}));
        let mut split = json!({"encoding":"hex","contentHex":"c3"});
        prefer_text(&mut split, &[0xc3]);
        assert_eq!(split, json!({"encoding":"hex","contentHex":"c3"}));
        assert_eq!(hex("é".as_bytes()), "c3a9");
        assert_eq!(text(&[0xff]), Value::Null);
    }
}
