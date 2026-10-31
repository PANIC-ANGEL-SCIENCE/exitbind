//! Synthetic v7 bytes supplement the frozen v1-v3 historical run fixtures.
//! The v7 start was constructed from a current start, with newer markers
//! removed and a v7 fallback binding added; it is not a historical record.

use super::{
    assert_chain, inspect, json_output, project, read_events, rehash_chain, text, write_events,
};
use serde_json::json;
use std::fs;

const SAVED_V7: &[u8] = include_bytes!("../fixtures/synthetic-v7-run.jsonl");
const LEDGER: &str = ".exitbind/runs/saved-v7.jsonl";

#[test]
fn saved_v7_fallback_is_read_without_rewriting_its_producer_or_bytes() {
    let root = project("saved-v7-reader");
    fs::write(root.join(LEDGER), SAVED_V7).unwrap();
    let output = inspect(&root, LEDGER);
    assert!(output.status.success(), "{}", text(&output));
    let result = json_output(&output);
    assert_eq!(result["valid"], true);
    let event = &result["events"][0];
    assert_eq!(event["version"], 7);
    assert_eq!(event["producer"]["name"], "exitbind");
    assert_eq!(event["producer"]["version"], "synthetic-fixture");
    let reviewer = event["plan"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|stage| stage["agents"].as_array().unwrap())
        .find(|agent| agent["role"] == "reviewer")
        .unwrap();
    assert_eq!(reviewer["fallbackRuntime"]["model"], "alternate-review");
    assert_eq!(fs::read(root.join(LEDGER)).unwrap().as_slice(), SAVED_V7);
}

#[test]
fn malformed_and_incompatible_saved_records_refuse_without_rewriting() {
    let root = project("saved-v7-refusals");
    fs::write(root.join(LEDGER), SAVED_V7).unwrap();
    let original = read_events(&root, LEDGER);

    // Rehash both mutations so the refusal must come from the reader's
    // format checks rather than an incidental hash mismatch.
    for (kind, expected) in [
        (
            "unknown-field",
            "unknown field 'guidanceContinuityProtocol'",
        ),
        ("future-version", "invalid event header"),
    ] {
        let mut records = original.clone();
        if kind == "unknown-field" {
            records[0]["guidanceContinuityProtocol"] = json!(1);
        } else {
            records[0]["version"] = json!(9);
        }
        rehash_chain(&mut records);
        assert_chain(&records);
        write_events(&root, LEDGER, &records);
        let before = fs::read(root.join(LEDGER)).unwrap();
        let refused = inspect(&root, LEDGER);
        assert!(!refused.status.success(), "{kind}: {}", text(&refused));
        assert!(
            text(&refused).contains(expected),
            "{kind}: {}",
            text(&refused)
        );
        assert_eq!(fs::read(root.join(LEDGER)).unwrap(), before, "{kind}");
    }

    let malformed = b"{\"version\":7,\n";
    fs::write(root.join(LEDGER), malformed).unwrap();
    let refused = inspect(&root, LEDGER);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("invalid run ledger line"),
        "{}",
        text(&refused)
    );
    assert_eq!(fs::read(root.join(LEDGER)).unwrap().as_slice(), malformed);
}
