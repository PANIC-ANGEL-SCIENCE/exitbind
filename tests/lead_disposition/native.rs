//! Drive and independently reconstruct the native multipart CLI surface.
use super::{text, Fixture};
use serde_json::Value;

pub(super) fn context(fixture: &Fixture, payload: &Value) -> String {
    // Execute every installed Claude slot; reconstruct before interpreting
    // the complete assignment, independently checking framing and bounds.
    let mut context = String::new();
    let mut digest = String::new();
    let (mut total, mut received) = (0, 0);
    for index in 1..=4 {
        let slot = index.to_string();
        let output = fixture.invoke(
            &["hook-run", "--host", "claude", "--index", &slot],
            payload.to_string().as_bytes(),
        );
        assert!(
            output.status.success(),
            "hook-run {index}: {}",
            text(&output)
        );
        if output.stdout.is_empty() {
            continue;
        }
        assert!(output.stdout.len() <= 16 * 1024);
        let hook: Value = serde_json::from_slice(&output.stdout).unwrap();
        let part = hook["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(part.encode_utf16().count() <= 10_000);
        let (header, body) = part.split_once("\n\n").unwrap();
        let hash = header
            .split("SHA-256 ")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        if digest.is_empty() {
            digest = hash.to_owned();
        } else {
            assert_eq!(digest, hash);
        }
        let position = header
            .split("; part ")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let (number, count) = position.split_once(" of ").unwrap();
        assert_eq!(number.parse::<usize>().unwrap(), index);
        let count = count.parse::<usize>().unwrap();
        if total == 0 {
            total = count;
        } else {
            assert_eq!(total, count);
        }
        received += 1;
        context.push_str(&body.replace("&lt;", "<").replace("&amp;", "&"));
    }
    assert_eq!(received, total);
    assert_eq!(
        digest,
        format!(
            "{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(context.as_bytes())
        )
    );
    context
}
