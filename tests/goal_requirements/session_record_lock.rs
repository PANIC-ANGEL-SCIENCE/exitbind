//! R55 F15 review R1: a refusal raised while the session recovery record's
//! lock is busy names that record, never "run ledger is busy". The class is
//! read from `src/`: under `src/host/session_recovery/` the run ledger's lock
//! (`with_lock`, on a `ledger_path`) is taken only by the one helper in
//! `lock.rs`, which remaps an error only when the lock was never entered
//! (no error-text comparison); every other site calls that helper.
use super::rust_source::{blank, line_of, production};

const DIR: &str = "src/host/session_recovery/";
const HELPER_FILE: &str = "src/host/session_recovery/lock.rs";
const HELPER: &str = "with_record_lock(";
const PRIMITIVES: [&str; 2] = ["with_lock(", "ledger_path("];

/// `file:line` of every occurrence of `needle` in the code of `files`
/// (comments and literals blanked), skipping `fn` definitions.
fn sites(files: &[(String, String)], needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        let code = blank(text);
        for (at, _) in code.match_indices(needle) {
            if code[..at].trim_end().ends_with("fn") {
                continue;
            }
            found.push(format!("{file}:{}", line_of(&code, at)));
        }
    }
    found
}

/// The run-ledger lock primitives taken outside the helper.
fn direct(files: &[(String, String)]) -> Vec<String> {
    let outside: Vec<_> = files
        .iter()
        .filter(|(file, _)| file != HELPER_FILE)
        .cloned()
        .collect();
    PRIMITIVES
        .iter()
        .flat_map(|needle| sites(&outside, needle))
        .collect()
}

fn session_recovery() -> Vec<(String, String)> {
    production()
        .into_iter()
        .filter(|(file, _)| file.starts_with(DIR))
        .collect()
}

#[test]
fn every_session_record_lock_goes_through_the_one_entered_helper() {
    let files = session_recovery();
    assert!(
        files.iter().any(|(file, _)| file == HELPER_FILE),
        "{HELPER_FILE} is missing"
    );
    assert_eq!(direct(&files), Vec::<String>::new());
    let callers = sites(&files, HELPER);
    // The record, graph attach, checkpoint, pause record and saved-decision
    // spend writers, read from the source: at least one, each through it.
    assert!(!callers.is_empty(), "no session record lock site");
    eprintln!("session record lock sites ({}): {callers:?}", callers.len());

    let helper = &files
        .iter()
        .find(|(file, _)| file == HELPER_FILE)
        .unwrap()
        .1;
    let code = blank(helper);
    for needle in PRIMITIVES {
        assert_eq!(code.matches(needle).count(), 1, "{needle} in {HELPER_FILE}");
    }
    assert!(code.contains("entered = true"), "decided by lock entry");
    assert!(code.contains("if entered"), "decided by lock entry");
    for text_test in [".contains(", "starts_with(", "ends_with(", "==", "!="] {
        assert!(
            !code.contains(text_test),
            "{HELPER_FILE} decides by lock entry, not by comparing error text ({text_test})"
        );
    }
}

#[test]
fn a_seeded_direct_lock_is_found() {
    let seeded = vec![(
        "src/host/session_recovery/seeded.rs".to_owned(),
        concat!(
            "fn f() {\n",
            "    // with_lock( in a comment is not a site\n",
            "    let ledger = crate::run::ledger::ledger_path(&root, FILE, true)?;\n",
            "    crate::run::ledger::with_lock(&ledger, || Ok(()))\n",
            "}\n",
        )
        .to_owned(),
    )];
    assert_eq!(
        direct(&seeded),
        [
            "src/host/session_recovery/seeded.rs:4",
            "src/host/session_recovery/seeded.rs:3"
        ]
    );
}
