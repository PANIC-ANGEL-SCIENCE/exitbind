//! Read-only lookup of one recorded event across later ledger heads.

use super::*;

/// Read one named recorded event without projecting current progression.
/// A later ledger head cannot change which operation this result describes.
pub fn inspect_event(loaded: &Loaded, ledger: &str, event_sha256: &str) -> Result<Value, String> {
    if event_sha256.len() != 64 || !event_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--event requires a 64-character event SHA-256".into());
    }
    let (_, events, _) = load(loaded, ledger)?;
    let (index, event) = events
        .iter()
        .enumerate()
        .find(|(_, event)| event["eventSha256"].as_str() == Some(event_sha256))
        .ok_or("selected event is not in the recorded ledger")?;
    let evidence = super::event_evidence::inspect(loaded, event)?;
    Ok(json!({
        "ledger": ledger,
        "event": event,
        "eventIndex": index,
        "eventSha256": event_sha256,
        "currentHeadEventSha256": events.last().map(|head| &head["eventSha256"]),
        "historical": index + 1 != events.len(),
        "evidence": evidence,
        "authorizesMutation": false,
    }))
}
