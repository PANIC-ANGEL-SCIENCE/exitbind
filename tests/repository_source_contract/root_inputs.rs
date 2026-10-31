//! Source contracts for path-input admission and private writes.
use super::rust_source::{production, string_list, strip_test_modules};
use std::{collections::BTreeMap, fs, path::Path};

const REGISTRY: &str = "src/project/path/inputs.rs";
const WRITER: &str = "src/project/private_write.rs";

/// Root comparisons (`strip_prefix`/`starts_with` whose argument names a
/// root, top, state, product, project, control, base or private path) that
/// are not a path input meeting its root: each compares a path the product
/// made itself, or relates roots to each other. Count per file.
const INTERNAL_ROOT_CHECKS: &[(&str, usize, &str)] = &[
    (
        "src/project/path.rs",
        2,
        "project::path, the read family the helper belongs to",
    ),
    (REGISTRY, 1, "the helper's one prefix decision"),
    (
        "src/cli/activity.rs",
        2,
        "activity records the product writes under the state root",
    ),
    (
        "src/config/profile.rs",
        1,
        "profile directories setup creates",
    ),
    (
        "src/host/codegraph/provider.rs",
        2,
        "CodeGraph's generated state directory",
    ),
    (
        "src/host/runtime/context.rs",
        1,
        "a configured profile shown relative to the project in hook text",
    ),
    (
        "src/host/runtime/project.rs",
        3,
        "a host hook payload's cwd routed to a configured project",
    ),
    (
        "src/host/settings.rs",
        1,
        "host settings files setup writes",
    ),
    (
        "src/memory/choices.rs",
        3,
        "task memory choice files under the state root",
    ),
    (
        "src/project/continuation/admission.rs",
        1,
        "a boundary pattern matched as text",
    ),
    (
        "src/project/git_preflight.rs",
        4,
        "a write target relative to its Git worktree",
    ),
    (
        "src/project/layout.rs",
        1,
        "legacy layout migration directories",
    ),
    (
        "src/project/layout_types.rs",
        6,
        "control, product and state roots must not nest",
    ),
    (
        "src/project/managed_files.rs",
        2,
        "directories the product creates",
    ),
    (
        "src/project/native_profiles.rs",
        1,
        "native profile projections setup writes",
    ),
    (
        "src/project/onboarding.rs",
        2,
        "init targets grouped by control and state root",
    ),
    (
        "src/project/portability.rs",
        4,
        "source and binding roots must not nest",
    ),
    (
        "src/project/setup_projection.rs",
        1,
        "projection directories setup writes",
    ),
    ("src/project/skills.rs", 2, "managed skill directories"),
    (
        "src/run/capture_error.rs",
        1,
        "capture records under the state root",
    ),
    (
        "src/run/carry.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/run/gate/value.rs",
        1,
        "a check script under the managed state is not a checker source",
    ),
    (
        "src/run/governor.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/run/governor_recovery.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/run/guidance_continuity/advisory_paths.rs",
        1,
        "Git-listed product paths made relative",
    ),
    (
        "src/run/inputs.rs",
        1,
        "tested inputs exclude the state root",
    ),
    (
        "src/run/pre_return.rs",
        3,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/run/pre_return/ownership.rs",
        1,
        "the canonical ledger made relative to bind its persisted admission",
    ),
    (
        "src/session_goal/journey/integration/lineage.rs",
        1,
        "a ledger the ledger judge confined, made relative",
    ),
    (
        "src/session_goal/usage.rs",
        1,
        "usage files the product wrote under the state root",
    ),
    (
        "src/value_benchmark/artifacts.rs",
        2,
        "the benchmark's own fixture tree",
    ),
    (
        "src/work/managed_edit/state.rs",
        1,
        "edit state the product wrote under the state root",
    ),
    (
        "src/work/native_action.rs",
        1,
        "native action files under the state root",
    ),
    (
        "src/work/native_action/continuation.rs",
        1,
        "native action files under the state root",
    ),
];

/// Writes outside the private writer in production code: each writes under
/// the state root, the control root, a host store or a destination the
/// operator named, never a private product file under the project root.
/// Count per file of `WRITE_CALLS`.
const OTHER_WRITES: &[(&str, usize, &str)] = &[
    (WRITER, 1, "the one private writer"),
    (
        "src/project/private_write/git.rs",
        2,
        "the writer's local exclude, replaced atomically",
    ),
    (
        "src/project/path.rs",
        1,
        "the secure creation primitive the writer uses",
    ),
    (
        "src/memory/lessons/carry.rs",
        1,
        "a guard: a product file the lesson names, copied where the previous project kept it",
    ),
    ("src/cli/mod.rs", 1, "receipt --output under the state root"),
    (
        "src/config/profile.rs",
        2,
        "agent profiles under the control root; the configuration goes through the one writer",
    ),
    (
        "src/distribution/update.rs",
        7,
        "the self-update replaces the executable",
    ),
    (
        "src/evidence/receipt.rs",
        1,
        "brief --receipt under the state root",
    ),
    (
        "src/host/away.rs",
        2,
        "away runner state under the state root",
    ),
    (
        "src/host/codegraph.rs",
        3,
        "CodeGraph state under the state root",
    ),
    (
        "src/host/native_session.rs",
        1,
        "the host's session environment file it names",
    ),
    (
        "src/host/session_recovery/hint.rs",
        2,
        "the native session store",
    ),
    (
        "src/host/settings.rs",
        2,
        "host settings files setup writes",
    ),
    (
        "src/memory/choices.rs",
        2,
        "task memory choices and their lock under the state root",
    ),
    (
        "src/memory/forgetting.rs",
        1,
        "forgetting receipts under the state root",
    ),
    (
        "src/memory/ledger.rs",
        1,
        "memory ledgers under the state root",
    ),
    (
        "src/memory/lessons.rs",
        1,
        "the lesson mutation lock under the state root",
    ),
    (
        "src/project/layout_types.rs",
        3,
        "project bindings in the bindings directory",
    ),
    (
        "src/project/managed_files.rs",
        2,
        "state and managed files init creates",
    ),
    (
        "src/project/skills.rs",
        2,
        "managed skills under the control root",
    ),
    (
        "src/run/capture.rs",
        1,
        "captured logs under the state root",
    ),
    (
        "src/run/ledger.rs",
        3,
        "run ledgers and locks under the state root",
    ),
    (
        "src/run/quarantine/admission.rs",
        1,
        "quarantine records under the state root",
    ),
    (
        "src/session_goal/association/companion.rs",
        1,
        "Goal association under the state root",
    ),
    (
        "src/session_goal/continuity/capture.rs",
        2,
        "continuity capture under the state root",
    ),
    (
        "src/session_goal/display.rs",
        2,
        "the native session display store",
    ),
    (
        "src/session_goal/native.rs",
        1,
        "the native session Goal owner record",
    ),
    (
        "src/value_benchmark/artifacts.rs",
        1,
        "the benchmark export the operator named",
    ),
    (
        "src/work/closeout.rs",
        1,
        "Work receipts under the state root",
    ),
    ("src/work/focus.rs", 2, "focus records under the state root"),
    ("src/work/held.rs", 1, "held results under the state root"),
    (
        "src/work/managed_edit/state.rs",
        1,
        "managed edit state under the state root",
    ),
    (
        "src/work/native_action.rs",
        1,
        "the native action journal lock under the state root",
    ),
    (
        "src/work/native_action/journal.rs",
        2,
        "native action journal under the state root",
    ),
    (
        "src/work/native_action/recovery.rs",
        1,
        "native action recovery under the state root",
    ),
];

/// Every way production code creates, replaces or copies a file.
const WRITE_CALLS: &[&str] = &[
    "secure_create_new(",
    "create_new(true)",
    ".create(true)",
    "File::create(",
    "fs::write(",
    "fs::copy(",
    "fs::rename(",
];
const ROOT_WORDS: &[&str] = &[
    "root", "top", "state", "product", "project", "control", "base", "private",
];

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Occurrences per file of a call whose argument names a root.
fn root_checks(text: &str) -> usize {
    let mut count = 0;
    for call in ["strip_prefix(", "starts_with("] {
        for (index, _) in text.match_indices(call) {
            let argument: String = text[index + call.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "_.&()".contains(*c))
                .collect();
            let name = argument.trim_start_matches(['&', '(']);
            if name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
                && ROOT_WORDS.iter().any(|word| name.contains(word))
            {
                count += 1;
            }
        }
    }
    count
}

fn writes(text: &str) -> usize {
    WRITE_CALLS
        .iter()
        .map(|call| text.matches(call).count())
        .sum()
}

fn counted(table: &[(&str, usize, &str)]) -> BTreeMap<String, usize> {
    table
        .iter()
        .map(|(file, count, _)| ((*file).to_owned(), *count))
        .collect()
}

/// `(label, options, outcome, judge)` of every registered input.
fn registry() -> Vec<(String, String, String, String)> {
    let text = fs::read_to_string(manifest().join(REGISTRY)).unwrap();
    let field = |entry: &str, name: &str| -> String {
        let start = entry.find(&format!("{name}: ")).unwrap() + name.len() + 2;
        let rest = &entry[start..];
        let end = rest.find(",\n").unwrap();
        rest[..end].trim_matches('"').to_owned()
    };
    let body = &text[text.find("pub(crate) const INPUTS").unwrap()..];
    let body = &body[..body.find("\n];").unwrap()];
    body.split("    Input {")
        .skip(1)
        .map(|entry| {
            (
                field(entry, "label"),
                field(entry, "options"),
                field(entry, "outcome"),
                field(entry, "judge"),
            )
        })
        .collect()
}

#[test]
fn path_inputs_inventory_declares_one_outcome_for_every_path_option() {
    let args = fs::read_to_string(manifest().join("src/cli/args.rs")).unwrap();
    let options = string_list(&args, "VALUE_OPTIONS");
    assert!(options.len() > 100, "{options:?}");
    let registry_text = fs::read_to_string(manifest().join(REGISTRY)).unwrap();
    let not_paths = string_list(&registry_text, "NOT_PATHS");
    let inputs = registry();
    assert!(inputs.len() >= 15, "{inputs:?}");
    let registered = |option: &str| {
        inputs.iter().any(|(_, options, _, _)| {
            options
                .split(", ")
                .any(|item| item.split(' ').next() == Some(&format!("--{option}")))
        })
    };
    for option in &options {
        let path = registered(option);
        let plain = not_paths.contains(option);
        assert!(
            path ^ plain,
            "--{option} must be either a registered path input with an outcome in {REGISTRY} or listed in NOT_PATHS (registered: {path}, not a path: {plain})"
        );
    }
    for listed in &not_paths {
        assert!(
            options.contains(listed),
            "NOT_PATHS lists unknown --{listed}"
        );
    }
    let sources: BTreeMap<_, _> = production().into_iter().collect();
    for (label, _, outcome, judge) in &inputs {
        if !(outcome.starts_with("Outcome::Confined") || outcome == "Outcome::Snapshotted") {
            continue;
        }
        for file in judge.split(", ") {
            let text = sources
                .get(file)
                .unwrap_or_else(|| panic!("{label}: judge {file} does not exist"));
            assert!(
                text.contains("inputs::"),
                "{label}: judge {file} must place the input through the root helper"
            );
        }
    }
}

#[test]
fn root_checks_and_private_writes_live_in_the_helper_and_the_writer() {
    let roots = counted(INTERNAL_ROOT_CHECKS);
    let creates = counted(OTHER_WRITES);
    let mut found_roots = BTreeMap::new();
    let mut found_creates = BTreeMap::new();
    for (file, text) in production() {
        let checks = root_checks(&text);
        if checks > 0 {
            found_roots.insert(file.clone(), checks);
        }
        let created = writes(&text);
        if created > 0 {
            found_creates.insert(file, created);
        }
    }
    assert_eq!(
        found_roots, roots,
        "a root comparison outside the root helper must place its input through src/project/path/inputs.rs, or be registered here as internal with its reason"
    );
    assert_eq!(
        found_creates, creates,
        "a file created under a root must go through src/project/private_write.rs, or be registered here with the root it writes under"
    );
    let writer = fs::read_to_string(manifest().join(WRITER)).unwrap();
    assert!(writer.contains("git.exclude(directory)") && writer.contains("unlisted(directory)"));
    for (file, owner) in [
        ("src/host/plan_card.rs", "plan card"),
        ("src/memory/lessons/carry.rs", "carried lesson source"),
        ("src/memory/stamped_config.rs", "stamped configuration copy"),
        ("src/project/setup_card.rs", "task card snapshot"),
    ] {
        let text = fs::read_to_string(manifest().join(file)).unwrap();
        assert!(
            text.contains("private_write::write_") && text.contains(owner),
            "{file} writes its {owner} through the private writer"
        );
    }
}

/// The P3 scan reads production code by extent: a write or root comparison
/// after a test module that holds brace literals is counted, the ones inside
/// it are not; every write spelling counts.
#[test]
fn root_and_write_scan_detects_each_member_after_a_test_module() {
    let seeded = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    const OPEN: char = '{';\n",
        "    const CLOSE: char = '}';\n",
        "    fn t() { std::fs::write(\"x\", \"}\").unwrap(); }\n",
        "}\n",
        "fn production(root: &Path, p: &Path) {\n",
        "    let _ = p.strip_prefix(root);\n",
        "    std::fs::write(p, \"{\").unwrap();\n",
        "    std::fs::OpenOptions::new().create(true).open(p).unwrap();\n",
        "}\n",
        "#[cfg(all(test, unix))]\n",
        "mod unix_tests { fn t(root: &Path, p: &Path) { let _ = p.starts_with(root); } }\n",
    );
    let code = strip_test_modules(seeded);
    assert_eq!(root_checks(&code), 1, "{code}");
    assert_eq!(writes(&code), 2, "{code}");
    assert_eq!(code.lines().count(), seeded.lines().count());
}
