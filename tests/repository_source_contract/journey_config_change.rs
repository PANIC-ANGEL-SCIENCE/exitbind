//! Source contract for member configuration admission.

/// Where a member's configuration is loaded and admitted.
const ADMIT_FILE: &str = "src/session_goal/journey/integration.rs";
/// A member configuration load that is no member admission: recovery reads
/// the current configuration as the selected one, then reaches its members
/// through `current_integration_for_project` and so `project_loaded`.
const SELECTED_LOADS: &[&str] = &["src/session_goal/journey.rs"];

/// The production function enclosing byte `at` of `code`.
fn enclosing(code: &str, at: usize) -> &str {
    let start = code[..at].rfind("fn ").expect("enclosing fn");
    code[start + 3..]
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()
        .unwrap()
}

/// Every member consumer reaches a member's configuration through the one
/// admit rule, read from the source: the only load of a member's
/// configuration path is `member_loaded`, the only call of
/// `config_stamp::admitted` is there, every `validate_member_loaded` call
/// reads a configuration that rule returned, and every production call of
/// `project_loaded` / `member_loaded` is listed with its consumer.
#[test]
fn every_member_consumer_reads_the_configuration_through_the_one_admit_rule() {
    let sources = super::rust_source::production();
    let mut consumers = Vec::new();
    let mut failures = Vec::new();
    for (file, text) in &sources {
        let code = super::rust_source::blank(text);
        for (at, _) in code.match_indices("config::load(Some(") {
            let line = &code[at..code[at..].find('\n').map_or(code.len(), |end| at + end)];
            if line.contains("member.config_path")
                && !(file == ADMIT_FILE && enclosing(&code, at) == "member_loaded")
                && !SELECTED_LOADS.contains(&file.as_str())
            {
                failures.push(format!(
                    "{file}: {} loads a member configuration outside member_loaded",
                    enclosing(&code, at)
                ));
            }
        }
        for (at, _) in code.match_indices("config_stamp::admitted(") {
            if !(file == ADMIT_FILE && enclosing(&code, at) == "member_loaded") {
                failures.push(format!(
                    "{file}: {} calls the admit rule directly",
                    enclosing(&code, at)
                ));
            }
        }
        for (at, _) in code.match_indices("validate_member_loaded(") {
            if code[..at].ends_with("fn ") {
                continue;
            }
            let function = enclosing(&code, at);
            let body = &code[code[..at].rfind("fn ").unwrap()..at];
            if ![
                "project_loaded(",
                "member_loaded(",
                "config_stamp::admitted(",
            ]
            .iter()
            .any(|reader| body.contains(reader))
            {
                failures.push(format!(
                    "{file}: {function} validates a member without the admit rule"
                ));
            }
        }
        for reader in ["project_loaded(", "member_loaded("] {
            for (at, _) in code.match_indices(reader) {
                let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
                if code[..at].ends_with("fn ") || code[..at].ends_with(ident) {
                    continue;
                }
                consumers.push(format!("{file}\t{}\t{reader}", enclosing(&code, at)));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    println!("{}", consumers.join("\n"));
    let functions = consumers
        .iter()
        .map(|line| line.split('\t').take(2).collect::<Vec<_>>().join("::"))
        .collect::<std::collections::BTreeSet<_>>();
    for consumer in [
        "src/session_goal/journey/integration.rs::covered_snapshot",
        "src/session_goal/journey/integration.rs::current_integration_for_project",
        "src/session_goal/journey/readiness.rs::member_currentness",
        "src/session_goal/journey.rs::validate_member_for_session",
        "src/session_goal/journey/integration/lineage.rs::accepted_endpoint",
    ] {
        assert!(functions.contains(consumer), "{consumer}: {functions:#?}");
    }
}
