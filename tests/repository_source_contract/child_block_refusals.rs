//! R55 F5b follow-up D1: an act a retained or live child blocks names the
//! child and prints its recover line. Read from `src/`: whether a child
//! blocks (its retained result or a non-terminal state) is decided only in
//! the child capture modules, and every refusal that a child blocks calls
//! the one producer, `blocking_child`, which names it and prints the line.
use super::rust_source::{blank, line_of, literals, production};

const OWNER: &str = "src/session_goal/continuity/capture";
const FIELDS: [&str; 2] = ["retainedResultBytes", "pendingResult"];
const PRODUCER: &str = "blocking_child(";

/// `file:line` of every literal naming a retention field outside `OWNER`.
fn readers(files: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files.iter().filter(|(file, _)| !file.starts_with(OWNER)) {
        for literal in literals(text) {
            if FIELDS.contains(&literal.text.as_str()) {
                found.push(format!("{file}:{}", line_of(text, literal.start)));
            }
        }
    }
    found
}

#[test]
fn every_child_blocked_refusal_reads_the_one_producer() {
    let files = production();
    assert_eq!(readers(&files), Vec::<String>::new());
    let mut callers = Vec::new();
    for (file, text) in &files {
        let code = blank(text);
        for (at, _) in code.match_indices(PRODUCER) {
            callers.push(format!("{file}:{}", line_of(&code, at)));
            // The refusal carries the producer's line.
            let rest = &text[at..text.len().min(at + 600)];
            assert!(
                rest.contains("{child}"),
                "{file}: the refusal drops the child"
            );
        }
    }
    // Quarantine, check replacement and follow-up admission, read from the
    // source: at least one, each through the producer.
    assert!(!callers.is_empty(), "no child-blocked refusal");
    eprintln!("child-blocked refusals ({}): {callers:?}", callers.len());
}

#[test]
fn a_seeded_direct_reader_is_found() {
    let seeded = vec![(
        "src/work/seeded.rs".to_owned(),
        concat!(
            "fn f(child: &Value) -> bool {\n",
            "    // \"retainedResultBytes\" in a comment is not a reader\n",
            "    child[\"retainedResultBytes\"].as_u64().is_some()\n",
            "}\n",
        )
        .to_owned(),
    )];
    assert_eq!(readers(&seeded), ["src/work/seeded.rs:3"]);
}
