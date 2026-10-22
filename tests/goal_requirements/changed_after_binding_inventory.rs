//! R55 F2 (pattern P2): "what was bound differs from what is current" has
//! one typed refusal. The registry of agent-reachable members is
//! `src/drift.rs` (`MEMBERS`, each with classification, message and route).
//! The scan finds the class two ways: refusal wording, and a refusal that
//! follows a stored value compared (`!=`) with a recomputed hash. Every hit
//! outside `src/drift.rs` must be listed literal by literal under
//! `tests/fixtures/changed-after-binding-plain/` with why it stays plain, in
//! the file named for its source domain (`src/run/...` in `run.tsv`); a new
//! hit, a stale entry, an unknown reason or an entry filed under another
//! domain fails, and so does a registered member that no source constructs.
use super::*;

const REGISTRY: &str = "src/drift.rs";
const PLAIN: &str = "tests/fixtures/changed-after-binding-plain";

/// The refusal wording of the class.
const SIGNATURE: &[&str] = &[
    "changed since",
    "differs from",
    "drift detected",
    "source changed",
    "changed after",
    "changed outside",
    "frozen current configuration",
    "current product inputs",
    "refuses changed configuration",
    "configuration changed",
    "no longer matches",
    "is not current",
    "stale",
];

/// A recomputation on a comparison line: the stored-vs-current half.
const RECOMPUTED: &[&str] = &[
    "hash::text(",
    "hash::bytes(",
    "hash::file(",
    "hash::value(",
    "fingerprint",
    "current_sha",
    "current_hash",
];

/// Why a listed literal stays a plain string.
const REASONS: &[(&str, &str)] = &[
    ("TypedRunDrift", "the run drift family (DriftError, input, artifact and receipt drift) already carries classification, both hashes and nextAction in its machine form; these are its renderings or items"),
    ("Transient", "something changed while one command ran; running it again reads the new state"),
    ("Integrity", "a persisted record differs from its own immutable claim; an internal self-consistency check no agent can act on"),
    ("Identity", "a request, host, recipient, owner or Goal identity differs from the one bound; about who or what, not a changed input"),
    ("NotCurrentState", "the requested action or item is not the current one; a position check, not a changed input"),
    ("LeadPreparation", "refused while the Lead or owner prepares a selection, continuation, quarantine, recovery or replacement; no Work input was bound yet"),
    ("Validation", "a malformed or out-of-bound input, not a binding"),
    ("StrictExport", "exporting an accepted result's receipt refuses on drift; the read views report it accepted at binding"),
    ("ReadAsFact", "its caller reads the error as a boolean fact; it is never shown"),
    ("RoutedInText", "the refusal names its own command"),
    ("NotARefusal", "a message, status word, warning line or unit-test string"),
];

/// Every class literal in `text`: string literals outside comment lines that
/// read as a sentence and carry the wording, or that a refusal returns
/// within eight lines after a stored value is compared with a recomputed one.
fn class_literals(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let refusal = line.contains("Err(")
            || (1..=2)
                .any(|back| index >= back && lines[index - back].trim_end().ends_with("Err("));
        let compared = refusal
            && lines[index.saturating_sub(8)..index].iter().any(|earlier| {
                !earlier.trim_start().starts_with("//")
                    && earlier.contains("!=")
                    && RECOMPUTED.iter().any(|needle| earlier.contains(needle))
            });
        for literal in literals(line) {
            if !literal.trim().contains(' ') {
                continue;
            }
            let worded = SIGNATURE.iter().any(|phrase| literal.contains(phrase));
            if worded || (compared && literal.len() > 12) {
                found.push(literal);
            }
        }
    }
    found
}

fn literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let mut end = None;
        let mut escaped = false;
        for (at, ch) in after.char_indices() {
            match ch {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => {
                    end = Some(at);
                    break;
                }
                _ => escaped = false,
            }
        }
        let Some(end) = end else { break };
        out.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    out
}

fn sources() -> Vec<(String, String)> {
    fn visit(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, root, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy();
                out.push((
                    relative.replace('\\', "/"),
                    fs::read_to_string(&path).unwrap(),
                ));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    visit(&root.join("src"), root, &mut out);
    out.sort();
    out
}

/// The registered classes of `src/drift.rs`.
fn classes(registry: &str) -> Vec<String> {
    registry
        .match_indices("class: Class::")
        .map(|(at, needle)| {
            registry[at + needle.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect()
        })
        .collect()
}

/// Every listed line with the domain its file is named for, from each
/// `*.tsv` file of the plain-literal directory.
fn plain_lines() -> Vec<(String, String)> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(PLAIN);
    let mut lines = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let domain = path.file_stem().unwrap().to_str().unwrap().to_owned();
        assert_eq!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("tsv"),
            "{PLAIN} holds only per-domain .tsv files: {}",
            path.display()
        );
        for line in fs::read_to_string(&path).unwrap().lines() {
            if !line.starts_with('#') && !line.is_empty() {
                lines.push((domain.clone(), line.to_owned()));
            }
        }
    }
    lines
}

/// The source domain of `src/<domain>/...` or `src/<domain>.rs`.
fn source_domain(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("src/")?;
    let first = rest.split('/').next()?;
    Some(first.strip_suffix(".rs").unwrap_or(first))
}

#[test]
fn every_changed_after_binding_refusal_is_typed_or_listed_with_its_reason() {
    let sources = sources();
    let registry = &sources
        .iter()
        .find(|(path, _)| path == REGISTRY)
        .expect("changed-after-binding registry")
        .1;
    let classes = classes(registry);
    assert!(classes.len() >= 19, "{classes:?}");
    let mut listed = std::collections::BTreeMap::<(String, String), usize>::new();
    let mut failures = Vec::new();
    for (domain, line) in plain_lines() {
        let fields: Vec<&str> = line.splitn(3, '\t').collect();
        let [reason, path, literal] = fields[..] else {
            failures.push(format!("{PLAIN}/{domain}.tsv: malformed line {line:?}"));
            continue;
        };
        if source_domain(path) != Some(domain.as_str()) {
            failures.push(format!(
                "{PLAIN}/{domain}.tsv: {path} belongs in the file named for its source domain"
            ));
        }
        if !REASONS.iter().any(|(name, _)| *name == reason) {
            failures.push(format!(
                "{PLAIN}: unknown reason {reason} for {path}: {literal}"
            ));
        }
        *listed
            .entry((path.to_owned(), literal.to_owned()))
            .or_default() += 1;
    }
    let mut found = std::collections::BTreeMap::<(String, String), usize>::new();
    for (path, text) in &sources {
        if path == REGISTRY {
            continue;
        }
        for literal in class_literals(text) {
            *found.entry((path.clone(), literal)).or_default() += 1;
        }
    }
    for (key, count) in &found {
        let known = listed.get(key).copied().unwrap_or(0);
        if known != *count {
            failures.push(format!(
                "{}: {count} \"{}\" literal(s), {known} listed; build an agent-reachable refusal with crate::drift::refusal (classification, both hashes, nextAction) or list it in {PLAIN}/{}.tsv with why it stays plain",
                key.0,
                key.1,
                source_domain(&key.0).unwrap_or("its-domain")
            ));
        }
    }
    for (key, _) in listed.iter().filter(|(key, _)| !found.contains_key(*key)) {
        failures.push(format!("{PLAIN}: stale entry {}: {}", key.0, key.1));
    }
    for class in &classes {
        let constructed = sources.iter().any(|(path, text)| {
            path != REGISTRY && text.contains(&format!("drift::Class::{class}"))
        });
        if !constructed {
            failures.push(format!(
                "{REGISTRY}: Class::{class} is registered but never refused"
            ));
        }
    }
    // Every typed refusal reaches the agent through the one renderer.
    let main = &sources
        .iter()
        .find(|(path, _)| path == "src/main.rs")
        .unwrap()
        .1;
    if !main.contains("crate::drift::split(&error)") || !main.contains("crate::drift::human(") {
        failures.push("src/main.rs no longer renders the typed refusal".into());
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The scanner sees the class: wording counts once per literal, escaped
/// quotes stay inside a literal, comments and status words do not count,
/// and a refusal after a stored-vs-recomputed comparison counts whatever its
/// wording.
#[test]
fn the_changed_after_binding_scanner_detects_each_member_of_the_class() {
    // An entry files under the domain of its source path.
    assert_eq!(source_domain("src/context.rs"), Some("context"));
    assert_eq!(
        source_domain("src/run/view/check_replacement.rs"),
        Some("run")
    );
    assert_eq!(source_domain("tests/run/view.rs"), None);
    assert_eq!(
        class_literals(
            "Err(\"configuration changed since memory event\".into()); \"checker source changed\""
        )
        .len(),
        2
    );
    assert_eq!(
        class_literals("format!(\"a \\\"quoted\\\" path differs from {x}\")").len(),
        1
    );
    assert!(
        class_literals("// \"configuration changed since memory event\"\n\"plain\"").is_empty()
    );
    assert!(class_literals("let state = \"stale\";").is_empty());
    let comparison = "    if hash::text(&loaded.source) != hint.owner_config_sha256 {\n        return Err(\"saved native owner moved on\".into());\n    }";
    assert_eq!(class_literals(comparison), ["saved native owner moved on"]);
    let unrelated =
        "    if a != b {\n        return Err(\"value is not allowed here\".into());\n    }";
    assert!(class_literals(unrelated).is_empty());
    assert_eq!(
        classes(
            "Member {\n class: Class::MemorySource,\n}\nMember { class: Class::LessonSource, }"
        ),
        ["MemorySource", "LessonSource"]
    );
}
