//! The recipient runtime test exercises every accepted detail view.
use super::cli_tables::views_in;
use super::rust_source::{production, string_list};
use std::{fs, path::Path};

#[test]
fn every_recipient_detail_view_is_covered_by_the_runtime_test() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/agent_context_delivery/action_view.rs"),
    )
    .unwrap();
    let covered = string_list(&source, "COVERED_VIEWS");
    let sources = production();
    let detail: Vec<(String, String)> = sources
        .iter()
        .filter(|(path, _)| path == "src/cli/work/detail.rs")
        .cloned()
        .collect();
    let readers = views_in(&sources, &detail);
    let accepted: Vec<String> = readers
        .iter()
        .flat_map(|reader| reader.accepted.iter().cloned())
        .collect();
    assert!(!accepted.is_empty(), "work detail reads no --view");
    for name in &accepted {
        assert!(
            covered.contains(name),
            "work detail --view {name} is not in the recipient surface inventory"
        );
    }
}
