//! R55 F11 (F7a review): the skip set of an environmental attestation, read
//! from the source. Every production read of a run's raw check list
//! (`state["checks"]`, `.get("checks")`; not the assessed
//! `status["checks"]["targets"]`, which already skips) is listed per file:
//! a file that must not count an attested check calls
//! `environmental_check::attested`; every other file says why counting it is
//! right. A new consumer, or a changed count, fails until it is classified.
use super::rust_source::{blank, literals, production};
use std::collections::BTreeMap;

/// How one file treats a check the Lead attested environmental.
enum Treats {
    /// Skips it: the file calls `environmental_check::attested`.
    Skips,
    /// Counts it, and why that is right.
    Counts(&'static str),
}

const CONSUMERS: &[(&str, usize, Treats)] = &[
    ("src/run/gate/exit.rs", 1, Treats::Skips),
    ("src/run/reducer.rs", 1, Treats::Skips),
    (
        "src/host/session_recovery/milestone.rs",
        1,
        Treats::Counts("milestone references name recorded check events; the last is the re-run"),
    ),
    (
        "src/run/artifact.rs",
        1,
        Treats::Counts("artifact drift reads every kept check log, the attested one included"),
    ),
    (
        "src/run/gate/value/human.rs",
        2,
        Treats::Counts("an acquisition lookup by event hash and the event counters"),
    ),
    (
        "src/run/gate/value/report.rs",
        1,
        Treats::Counts("report counters count every observed check"),
    ),
    (
        "src/presentation/run_report.rs",
        3,
        Treats::Counts("report group counters"),
    ),
    (
        "src/run/state/check_stage.rs",
        1,
        Treats::Counts("the reducer appends every check event to the list"),
    ),
    (
        "src/run/state/historical_review.rs",
        1,
        Treats::Counts(
            "replays reviewer events of producers that predate environmental attestation",
        ),
    ),
    (
        "src/run/quarantine.rs",
        1,
        Treats::Counts("an allowance count field, not the check list"),
    ),
    (
        "src/run/quarantine/followup.rs",
        2,
        Treats::Counts("a quarantine verification Work, which refuses every Lead disposition"),
    ),
    (
        "src/run/quarantine/verification.rs",
        1,
        Treats::Counts("a quarantine verification Work, which refuses every Lead disposition"),
    ),
    (
        "src/work/quarantine.rs",
        3,
        Treats::Counts("a quarantine verification Work, which refuses every Lead disposition"),
    ),
    (
        "src/run/observed_check.rs",
        2,
        Treats::Counts("copies the assessed status view's section, which already skips"),
    ),
    (
        "src/run/mod.rs",
        1,
        Treats::Counts("copies the assessed status view, which already skips"),
    ),
];

/// Reads of a raw check list per file: `x["checks"]` not followed by
/// `["targets"]`, and `.get("checks")`, in code in any spacing.
fn consumers(sources: &[(String, String)]) -> BTreeMap<String, usize> {
    let mut found = BTreeMap::new();
    for (file, text) in sources {
        let code = blank(text);
        let lits = literals(text);
        for (index, lit) in lits.iter().enumerate() {
            if lit.text != "checks" {
                continue;
            }
            let before = code[..lit.start].trim_end();
            let after = code[lit.end..].trim_start();
            let indexed = before.ends_with('[') && after.starts_with(']');
            let got = before.ends_with(".get(") && after.starts_with(')');
            let assessed = indexed
                && lits.get(index + 1).is_some_and(|next| {
                    next.text == "targets" && code[lit.end..next.start].trim() == "]["
                });
            if (indexed || got) && !assessed {
                *found.entry(file.clone()).or_default() += 1;
            }
        }
    }
    found
}

fn unclassified(sources: &[(String, String)], listed: &[(&str, usize, Treats)]) -> Vec<String> {
    let found = consumers(sources);
    let mut failures = Vec::new();
    for (file, count) in &found {
        match listed.iter().find(|(name, _, _)| name == file) {
            None => failures.push(format!(
                "{file}: {count} read(s) of the raw check list; classify whether an attested check is skipped"
            )),
            Some((_, listed, _)) if listed != count => failures.push(format!(
                "{file}: {count} read(s) of the raw check list, {listed} classified"
            )),
            Some(_) => {}
        }
    }
    for (file, _, treats) in listed {
        if !found.contains_key(*file) {
            failures.push(format!("{file}: classified but reads no raw check list"));
        }
        let text = sources
            .iter()
            .find(|(name, _)| name == file)
            .map(|(_, text)| text);
        if matches!(treats, Treats::Skips)
            && !text.is_some_and(|text| text.contains("environmental_check::attested("))
        {
            failures.push(format!(
                "{file}: classified as skipping but never reads the attested set"
            ));
        }
    }
    failures
}

#[test]
fn every_raw_check_list_consumer_says_whether_it_skips_an_attested_check() {
    let failures = unclassified(&production(), CONSUMERS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Seeded: a new consumer in either spelling is a member; the assessed
/// targets, a literal and a comment are not; a skipping file must read the
/// attested set.
#[test]
fn a_new_raw_check_list_consumer_is_a_member() {
    let seeded = concat!(
        "fn a(state: &Value) -> usize { state[\"checks\"].as_array().map_or(0, Vec::len) }\n",
        "fn b(state: &Value) -> bool { state.get( \"checks\" ).is_some() }\n",
        "fn c(status: &Value) -> &Value { &status[\"checks\"][\"targets\"] }\n",
        "// state[\"checks\"] in a comment\n",
        "const S: &str = \"state[\\\"checks\\\"]\";\n",
    );
    let sources = vec![("src/seeded.rs".to_owned(), seeded.to_owned())];
    assert_eq!(consumers(&sources).get("src/seeded.rs"), Some(&2));
    let failures = unclassified(&sources, &[]);
    assert_eq!(failures.len(), 1, "{failures:?}");
    let skipping = unclassified(&sources, &[("src/seeded.rs", 2, Treats::Skips)]);
    assert_eq!(
        skipping,
        ["src/seeded.rs: classified as skipping but never reads the attested set"]
    );
}
