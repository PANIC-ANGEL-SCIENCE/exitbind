//! R55 F2 (pattern P2): "what was bound differs from what is current" has
//! one typed refusal. The registry of agent-reachable members is
//! `src/drift.rs` (`MEMBERS`, each with classification, message and route);
//! every other source literal with the class signature is listed here per
//! file with its count and why it stays plain. A new signature literal
//! anywhere fails until it is built through `crate::drift` or listed with
//! its reason; a registered member that no source constructs fails too.
use super::*;

const REGISTRY: &str = "src/drift.rs";

/// The refusal wording of the class. A literal containing one of these is a
/// member of the class until classified.
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
];

/// Why a listed literal stays a plain string.
#[derive(Clone, Copy, Debug)]
enum Plain {
    /// The run drift family (`DriftError`, input and artifact drift) already
    /// carries classification, both hashes and `nextAction` in its machine
    /// form; these are its renderings.
    TypedRunDrift,
    /// Something changed while one command ran (a race between its own read
    /// and recheck); running the command again reads the new state.
    Transient,
    /// A persisted record differs from its own immutable claim: an internal
    /// self-consistency check no agent can act on.
    Integrity,
    /// A request, host, recipient or Goal identity differs from the one it
    /// was bound to; the refusal is about who or what, not a changed input.
    Identity,
    /// A message, a warning line or a unit test, not a refusal.
    NotARefusal,
    /// The refusal names its own command (managed skills, the fill route,
    /// the continuation hold form with its diagnostic command).
    RoutedInText,
}

const LISTED: &[(&str, usize, Plain)] = &[
    ("src/cli/run_cli.rs", 5, Plain::TypedRunDrift),
    ("src/run/error.rs", 10, Plain::TypedRunDrift),
    ("src/work/mod.rs", 7, Plain::TypedRunDrift),
    ("src/evidence/receipt.rs", 1, Plain::TypedRunDrift),
    ("src/run/artifact.rs", 2, Plain::TypedRunDrift),
    ("src/run/reducer.rs", 2, Plain::TypedRunDrift),
    ("src/config/profile.rs", 3, Plain::Transient),
    ("src/host/codegraph.rs", 8, Plain::Transient),
    ("src/host/codegraph/preparation.rs", 2, Plain::Transient),
    (
        "src/host/session_recovery/references.rs",
        1,
        Plain::Transient,
    ),
    ("src/memory/lessons/input.rs", 1, Plain::Transient),
    ("src/memory/state.rs", 1, Plain::Transient),
    (
        "src/project/agent_context/phase_guidance.rs",
        1,
        Plain::Transient,
    ),
    ("src/project/architecture/adoption.rs", 1, Plain::Transient),
    ("src/project/capabilities/adoption.rs", 1, Plain::Transient),
    ("src/project/context.rs", 2, Plain::Transient),
    ("src/project/continuation/sources.rs", 5, Plain::Transient),
    ("src/project/layout.rs", 1, Plain::Transient),
    ("src/run/observed_check.rs", 1, Plain::Transient),
    ("src/run/pre_return/execution.rs", 2, Plain::Transient),
    ("src/run/view.rs", 4, Plain::Transient),
    ("src/session_goal/recovery.rs", 1, Plain::Transient),
    (
        "src/session_goal/continuity/capture.rs",
        1,
        Plain::Transient,
    ),
    (
        "src/session_goal/continuity/perspective.rs",
        1,
        Plain::Transient,
    ),
    ("src/memory/lessons.rs", 1, Plain::Integrity),
    ("src/project/capabilities.rs", 1, Plain::Integrity),
    ("src/project/continuation/admission.rs", 1, Plain::Integrity),
    (
        "src/project/continuation/association.rs",
        1,
        Plain::Integrity,
    ),
    ("src/project/continuation/recovery.rs", 1, Plain::Integrity),
    (
        "src/project/continuation/runtime/goal_linkage.rs",
        1,
        Plain::Integrity,
    ),
    (
        "src/project/continuation/runtime/successor.rs",
        1,
        Plain::Integrity,
    ),
    ("src/run/carry.rs", 8, Plain::Integrity),
    ("src/run/check_recovery.rs", 1, Plain::Integrity),
    ("src/run/check_replacement.rs", 1, Plain::Integrity),
    ("src/run/governor_recovery.rs", 2, Plain::Integrity),
    ("src/run/quarantine/admission.rs", 7, Plain::Integrity),
    (
        "src/run/quarantine/admission/followup.rs",
        1,
        Plain::Integrity,
    ),
    ("src/run/quarantine/verification.rs", 1, Plain::Integrity),
    ("src/session_goal/quarantine.rs", 1, Plain::Integrity),
    (
        "src/session_goal/quarantine/evidence.rs",
        2,
        Plain::Integrity,
    ),
    (
        "src/session_goal/requirements/successor.rs",
        1,
        Plain::Integrity,
    ),
    ("src/value_benchmark/result.rs", 1, Plain::Integrity),
    ("src/work/check_recovery.rs", 1, Plain::Integrity),
    ("src/work/file_effect.rs", 1, Plain::Integrity),
    ("src/host/assignment_context.rs", 4, Plain::Identity),
    ("src/memory/choices.rs", 2, Plain::Identity),
    ("src/project/specialists/delivery.rs", 1, Plain::Identity),
    (
        "src/session_goal/association/admission.rs",
        1,
        Plain::Identity,
    ),
    (
        "src/session_goal/continuity/capture/adviser.rs",
        1,
        Plain::Identity,
    ),
    ("src/work/quarantine.rs", 3, Plain::Identity),
    ("src/presentation/mod.rs", 1, Plain::NotARefusal),
    ("src/project/agent_context.rs", 1, Plain::NotARefusal),
    ("src/project/commands.rs", 1, Plain::NotARefusal),
    ("src/session_goal/closure.rs", 2, Plain::NotARefusal),
    (
        "src/session_goal/continuity/capture/tests.rs",
        1,
        Plain::NotARefusal,
    ),
    ("src/work/packet.rs", 1, Plain::NotARefusal),
    ("src/project/skills.rs", 1, Plain::RoutedInText),
    (
        "src/project/continuation/amendment.rs",
        3,
        Plain::RoutedInText,
    ),
    ("src/work/action_forms/fill.rs", 1, Plain::RoutedInText),
];

/// Every string literal outside comments that carries the class signature.
fn signature_literals(text: &str) -> usize {
    let mut found = 0;
    for line in text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
    {
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
            let literal = &after[..end];
            if SIGNATURE.iter().any(|phrase| literal.contains(phrase)) {
                found += 1;
            }
            rest = &after[end + 1..];
        }
    }
    found
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

#[test]
fn every_changed_after_binding_refusal_is_typed_or_listed_with_its_reason() {
    let sources = sources();
    let registry = &sources
        .iter()
        .find(|(path, _)| path == REGISTRY)
        .expect("changed-after-binding registry")
        .1;
    let classes = classes(registry);
    assert!(classes.len() >= 16, "{classes:?}");
    let mut failures = Vec::new();
    for (path, text) in &sources {
        if path == REGISTRY {
            continue;
        }
        let found = signature_literals(text);
        let listed = LISTED
            .iter()
            .find(|(file, _, _)| file == path)
            .map_or(0, |(_, count, _)| *count);
        if found != listed {
            failures.push(format!(
                "{path}: {found} changed-after-binding literal(s), {listed} listed; build an agent-reachable refusal with crate::drift::refusal (classification, both hashes, nextAction) or list it here with why it stays plain"
            ));
        }
    }
    for (file, _, reason) in LISTED {
        if !sources.iter().any(|(path, _)| path == file) {
            failures.push(format!("{file} ({reason:?}) is listed but missing"));
        }
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

/// The scanner sees the class: each signature phrase counts once per
/// literal, escaped quotes stay inside a literal, and comments do not count.
#[test]
fn the_changed_after_binding_scanner_detects_each_member_of_the_class() {
    assert_eq!(
        signature_literals(
            "Err(\"configuration changed since memory event\".into()); \"checker source changed\""
        ),
        2
    );
    assert_eq!(
        signature_literals("format!(\"a \\\"quoted\\\" path differs from {x}\")"),
        1
    );
    assert_eq!(
        signature_literals("// \"configuration changed since memory event\"\n\"plain\""),
        0
    );
    assert_eq!(
        classes(
            "Member {\n class: Class::MemorySource,\n}\nMember { class: Class::LessonSource, }"
        ),
        ["MemorySource", "LessonSource"]
    );
}
