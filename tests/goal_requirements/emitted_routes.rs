//! R55 F1 (pattern P1): an argv the product prints for a role runs unedited.
//! Every placeholder literal and every executable lookup in `src/` goes
//! through the one emitted-route module, every printed placeholder is
//! registered with the option that fills it, and the fill route runs a
//! current form with its placeholders filled; a refused fill writes nothing.
use super::*;

const REGISTRY: &str = "src/emitted.rs";

/// Files that print `<NAME>` text which is not an emitted route.
const STATIC_TEXT: &[(&str, &str)] = &[
    (
        "src/cli/help.rs",
        "static help text stays outside the registry",
    ),
    (
        "src/host/codex_exec/controlled.rs",
        "a Codex CLI help transcript used as a parser fixture",
    ),
];

/// Executable lookups that print no route: identity hashing, self-spawned
/// children, the self-update and unit tests. Count per file.
const NOT_EMITTERS: &[(&str, usize, &str)] = &[
    (
        "src/producer.rs",
        2,
        "build identity hashes the running executable",
    ),
    ("src/main.rs", 1, "invoked-name compatibility check"),
    ("src/host/away.rs", 1, "spawns its own detached runner"),
    ("src/host/native_session.rs", 1, "unit test fixture"),
    (
        "src/run/capture_supervisor/linux.rs",
        1,
        "spawns its capture supervisor",
    ),
    ("src/run/capture_supervisor/tests.rs", 1, "unit test"),
    (
        "src/run/cleanup_fixture.rs",
        1,
        "spawns its cleanup fixture",
    ),
    (
        "src/distribution/update.rs",
        1,
        "replaces the running executable",
    ),
    (
        "src/work/file_effect.rs",
        1,
        "owner binding hashes the executable",
    ),
    (
        "src/work/managed_edit/state.rs",
        1,
        "edit binding records the executable identity",
    ),
    (
        "src/work/native_action/request.rs",
        2,
        "effect adapter protected roots",
    ),
    (
        "src/value_benchmark/invocation.rs",
        1,
        "spawns benchmark children",
    ),
    ("src/work/recovery.rs", 1, "unit test"),
    ("src/work/mutation_response.rs", 3, "unit tests"),
    ("src/work/compact_tests.rs", 4, "unit tests"),
];

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

/// Every `<NAME>` token (upper case, digits, underscore; two or more bytes).
fn placeholder_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for (start, _) in text.match_indices('<') {
        let name: String = bytes[start + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || **byte == b'_')
            .map(|byte| *byte as char)
            .collect();
        let end = start + 1 + name.len();
        if name.len() >= 2
            && name.as_bytes()[0].is_ascii_uppercase()
            && bytes.get(end) == Some(&b'>')
        {
            found.push(name);
        }
    }
    found
}

/// The quoted first argument of every `needle("...")` call.
fn quoted_calls(text: &str, needle: &str) -> Vec<String> {
    text.match_indices(needle)
        .filter_map(|(at, _)| {
            let rest = &text[at + needle.len()..];
            rest.split_once('"').map(|(name, _)| name.to_owned())
        })
        .collect()
}

fn registered(registry: &str) -> Vec<String> {
    quoted_calls(registry, "name: \"")
}

#[test]
fn every_placeholder_and_executable_lookup_goes_through_the_emitted_route_registry() {
    let sources = sources();
    let registry = &sources
        .iter()
        .find(|(path, _)| path == REGISTRY)
        .expect("emitted route registry")
        .1;
    let names = registered(registry);
    assert!(names.len() >= 30, "{names:?}");
    let mut used = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for (path, text) in &sources {
        if path == REGISTRY {
            continue;
        }
        let tokens = placeholder_tokens(text);
        if !tokens.is_empty() && !STATIC_TEXT.iter().any(|(file, _)| file == path) {
            failures.push(format!(
                "{path}: placeholder literal(s) {tokens:?} outside {REGISTRY}; use crate::emitted::slot"
            ));
        }
        for name in quoted_calls(text, "emitted::slot(\"") {
            if !names.contains(&name) {
                failures.push(format!("{path}: slot {name} has no fill in {REGISTRY}"));
            }
            used.insert(name);
        }
        let lookups = text.matches("current_exe()").count();
        let allowed = NOT_EMITTERS
            .iter()
            .find(|(file, _, _)| file == path)
            .map_or(0, |(_, count, _)| *count);
        if lookups != allowed {
            failures.push(format!(
                "{path}: {lookups} executable lookup(s), {allowed} registered as non-emitters; build printed routes with crate::emitted"
            ));
        }
    }
    for name in &names {
        if !used.contains(name) {
            failures.push(format!(
                "{REGISTRY}: {name} is registered but never printed"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The scanner itself sees the class: a literal, an unregistered slot and an
/// unregistered executable lookup each count.
#[test]
fn emitted_route_scanner_detects_each_member_of_the_class() {
    assert_eq!(
        placeholder_tokens("x \"<REASON>\".into() <a> <B> <RFC3339_EXPIRY>"),
        ["REASON", "RFC3339_EXPIRY"]
    );
    assert_eq!(
        quoted_calls("crate::emitted::slot(\"QUESTION\")", "emitted::slot(\""),
        ["QUESTION"]
    );
    assert_eq!(
        registered("Placeholder {\n name: \"REASON\",\n"),
        ["REASON"]
    );
}

fn fill_argv(
    project: &Project,
    view: &Value,
    choice: &str,
    options: &[(&str, &str)],
) -> Vec<String> {
    let route = &view["effectiveAction"]["fillRoute"];
    let mut argv: Vec<String> = route["argv"]
        .as_array()
        .unwrap_or_else(|| panic!("no fill route: {view}"))
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect();
    if route["sameExecutableRequired"] == true {
        argv.insert(0, env!("CARGO_BIN_EXE_exitbind").to_owned());
    }
    if route["sameConfigRequired"] == true {
        argv.push(
            project
                .root
                .join("exitbind.json")
                .to_str()
                .unwrap()
                .to_owned(),
        );
    }
    argv.extend(["--choice".to_owned(), choice.to_owned()]);
    for (option, value) in options {
        argv.extend([(*option).to_owned(), (*value).to_owned()]);
    }
    argv
}

fn action_view(project: &Project, work: &str) -> Value {
    project.value(
        &["work", "detail", work, "--view", "action", "--json"],
        None,
    )
}

/// Every placeholder of every current choice names the option that fills it,
/// so the filled argv carries no placeholder.
fn assert_every_choice_fillable(view: &Value) {
    let action = &view["effectiveAction"];
    let forms = action["choices"].as_array().unwrap().iter().chain(
        action["beforeEditing"]
            .as_object()
            .map(|_| &action["beforeEditing"]),
    );
    for form in forms {
        let Some(placeholders) = form["placeholders"].as_array() else {
            continue;
        };
        for name in placeholders {
            let option = form["fill"]["options"][name.as_str().unwrap()].as_str();
            assert!(
                option.is_some_and(|option| option.starts_with("--")),
                "{form}"
            );
        }
    }
}

#[test]
fn lead_and_worker_forms_run_through_the_fill_route_and_refusals_write_nothing() {
    let project = Project::new("emitted-fill-route");
    project.setup();
    let work = project.begin("R1", "sh check.sh R1");
    let ledger = project.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let artifact = project.root.join("scope.md");
    fs::write(
        &artifact,
        "Scope: R1 only; the import check is the evidence.\n",
    )
    .unwrap();
    let artifact = artifact.to_str().unwrap();

    let view = action_view(&project, &work);
    assert_eq!(
        view["effectiveAction"]["leadChoiceRequired"], true,
        "{view}"
    );
    assert_eq!(view["effectiveAction"]["choiceActor"]["role"], "lead");
    assert_every_choice_fillable(&view);
    let text = project.call(&["work", "detail", &work, "--view", "action"], None);
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(
        text.contains("Lead choice: decided by lead agent 'lead'"),
        "{text}"
    );
    assert!(text.contains("  fill route: "), "{text}");
    assert!(!text.contains("<same config>"), "{text}");

    // Refusals happen before the bound command runs and leave the ledger.
    let before = fs::read(&ledger).unwrap();
    let reason = ("--reason", "R1 is the first Work; the others follow.");
    for (options, expected) in [
        (
            vec![("--artifact-file", artifact)],
            "fill REASON (the actor's reason for this decision) with --reason",
        ),
        (vec![reason], "pass --artifact-file PATH"),
        (
            vec![reason, ("--artifact-file", artifact), ("--question", "why")],
            "--question fills no placeholder",
        ),
    ] {
        let output = project.call_exact_argv(&fill_argv(&project, &view, "scoped", &options));
        assert!(!output.status.success(), "{options:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{output:?}"
        );
        assert_eq!(fs::read(&ledger).unwrap(), before);
    }
    let mut stale = fill_argv(
        &project,
        &view,
        "scoped",
        &[reason, ("--artifact-file", artifact)],
    );
    let at = stale
        .iter()
        .position(|item| item == "--current-binding")
        .unwrap();
    stale[at + 1] = "0".repeat(64);
    let output = project.call_exact_argv(&stale);
    assert!(String::from_utf8_lossy(&output.stderr).contains("the action changed since binding"));
    assert_eq!(fs::read(&ledger).unwrap(), before);

    // The scoped decision through the fill route, with no inline filler.
    let output = project.call_exact_argv(&fill_argv(
        &project,
        &view,
        "scoped",
        &[reason, ("--artifact-file", artifact)],
    ));
    assert!(output.status.success(), "{output:?}");
    let returned: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(returned["next"]["role"], "worker", "{returned}");

    // The worker's permit form through the same route.
    let view = action_view(&project, &work);
    assert_every_choice_fillable(&view);
    let output = project.call_exact_argv(&fill_argv(
        &project,
        &view,
        "permit",
        &[("--operation", "Return fixture evidence")],
    ));
    assert!(output.status.success(), "{output:?}");
    let permit: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(permit["allowed"], true, "{permit}");
}
