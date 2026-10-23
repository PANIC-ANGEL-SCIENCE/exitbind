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
        "src/test_root.rs",
        1,
        "unit test re-runs its own seeded failing test",
    ),
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
];

/// The product's top-level commands; a literal `exitbind <command>`,
/// `soulmate <command>` or `{product} <command>` in printed text names the
/// command instead of the executable the route must run with.
const COMMANDS: &[&str] = &[
    "work",
    "goal",
    "memory",
    "project",
    "setup",
    "session",
    "host",
    "hook-run",
    "hooks",
    "statusline",
    "check",
    "init",
    "update",
    "run",
    "brief",
    "profile",
    "away",
    "receipt",
    "doctor",
    "version",
];

/// Printed text that names a command by its literal product name, per file
/// with its count and class. A new occurrence anywhere fails until it is
/// built through `crate::emitted` or listed here with its class.
const NAMED_COMMANDS: &[(&str, usize, &str)] = &[
    // Installed into host settings on purpose: resolves the installed
    // executable through PATH after upgrades.
    ("src/host/hooks.rs", 4, "installed host hook command"),
    (
        "src/project/setup_statusline.rs",
        1,
        "installed status line command",
    ),
    (
        "src/value_benchmark.rs",
        2,
        "benchmark command identity, not printed",
    ),
    (
        "src/host/session_recovery/presentation.rs",
        1,
        "unit test of a refused hint",
    ),
];

/// Every printed `<product> <command>` occurrence outside comments.
fn named_commands(text: &str) -> usize {
    let mut found = 0;
    for line in text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
    {
        for needle in ["exitbind ", "soulmate ", "{product} "] {
            for (at, _) in line.match_indices(needle) {
                let before = line[..at].chars().next_back().unwrap_or(' ');
                let rest = &line[at + needle.len()..];
                let word: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                let after = rest[word.len()..].chars().next().unwrap_or(' ');
                if "\"`( :'".contains(before)
                    && COMMANDS.contains(&word.as_str())
                    && " `\"'".contains(after)
                {
                    found += 1;
                }
            }
        }
    }
    found
}

pub(super) fn sources() -> Vec<(String, String)> {
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
        if path != "src/cli/help.rs" {
            let named = named_commands(text);
            let listed = NAMED_COMMANDS
                .iter()
                .find(|(file, _, _)| file == path)
                .map_or(0, |(_, count, _)| *count);
            if named != listed {
                failures.push(format!(
                    "{path}: {named} printed command(s) name the product literally, {listed} listed; build it with crate::emitted::Route"
                ));
            }
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
    // A literal product-name command, in a format string or plain literal;
    // prose, paths, comments and non-commands are not commands.
    assert_eq!(
        named_commands(
            "format!(\"read: exitbind work detail {w}\"); \"`exitbind session recovery show`\""
        ),
        2
    );
    assert_eq!(named_commands("format!(\"{product} work act {w}\")"), 1);
    assert_eq!(
        named_commands("\"/bin/exitbind work\" \"{product} update: x\" \"Exitbind recovered\"\n// exitbind work"),
        0
    );
}

fn prepared_integration(project: &Project) -> Value {
    project.value(
        &[
            "goal",
            "prepare",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh all",
            "--scope",
            "integration",
            "--json",
        ],
        None,
    )
}

/// The printed begin route with the owner's review choice filled.
fn begin_argv(prepared: &Value) -> Vec<String> {
    prepared["beginRoute"]["command"]
        .as_array()
        .unwrap_or_else(|| panic!("no begin route: {prepared}"))
        .iter()
        .map(|item| match item.as_str().unwrap() {
            "<REVIEW_DECISION>" => "required".to_owned(),
            other => other.to_owned(),
        })
        .collect()
}

/// Project and native-store bytes, without the session display projection.
fn stored(project: &Project) -> Vec<(String, Vec<u8>)> {
    let (_, store) = project.native_session.clone().unwrap();
    let mut bytes = super::setup_first_work::tree(&project.root);
    bytes.extend(
        super::setup_first_work::tree(&store)
            .into_iter()
            .filter(|(path, _)| !path.starts_with("display")),
    );
    bytes
}

/// An integration prepared over a conversation journey whose members are all
/// accepted prints a begin route that freezes them; run as printed, it starts
/// the journey-integration Work. Preparing writes nothing.
#[test]
fn an_integration_begin_route_over_an_accepted_journey_freezes_it_as_printed() {
    let p = Project::new("p1-journey-route").with_native_session("p1-journey-route-session");
    p.setup();
    let first = p.begin("R1", "sh check.sh R1");
    p.drive(&first, true);

    let before = stored(&p);
    let prepared = prepared_integration(&p);
    assert!(
        stored(&p) == before,
        "goal prepare wrote project or Goal state"
    );
    assert!(
        prepared["journeyIntegrationOmitted"].is_null(),
        "{prepared}"
    );
    let argv = begin_argv(&prepared);
    assert!(
        argv.iter().any(|word| word == "--journey-integration"),
        "{argv:?}"
    );

    let started = p.call_exact_argv(&argv);
    assert!(started.status.success(), "{started:?}");
    let work = serde_json::from_slice::<Value>(&started.stdout).unwrap()["work"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = p
        .root
        .join(".exitbind/runs")
        .join(format!("work-{}.jsonl", work.trim_start_matches("smw_")));
    assert!(
        fs::read_to_string(ledger)
            .unwrap()
            .contains("exitbind.session-journey-integration.v1.header:"),
        "the printed route did not start a journey-integration Work"
    );
}

/// With a member that has no accepted result, the route omits the flag and
/// the response states why in one plain field.
#[test]
fn an_integration_begin_route_omits_the_journey_freeze_and_says_why() {
    let p =
        Project::new("p1-journey-unaccepted").with_native_session("p1-journey-unaccepted-session");
    p.setup();
    let _open = p.begin("R1", "sh check.sh R1");

    let before = stored(&p);
    let prepared = prepared_integration(&p);
    assert!(
        stored(&p) == before,
        "goal prepare wrote project or Goal state"
    );
    let reason = prepared["journeyIntegrationOmitted"]
        .as_str()
        .unwrap_or_else(|| panic!("no omission reason: {prepared}"));
    assert!(
        reason.contains("has no terminal accepted result"),
        "{reason}"
    );
    let argv = begin_argv(&prepared);
    assert!(
        !argv.iter().any(|word| word == "--journey-integration"),
        "{argv:?}"
    );
    assert!(argv.iter().any(|word| word == "integration"), "{argv:?}");
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
    let refused = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        refused.contains("the action changed since binding"),
        "{refused}"
    );
    // The refusal's follow-up is a full route that runs as printed.
    let follow_up: Vec<String> = refused
        .split("; read ")
        .nth(1)
        .and_then(|rest| rest.split(" again").next())
        .unwrap_or_else(|| panic!("no follow-up route: {refused}"))
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert_eq!(follow_up[0], env!("CARGO_BIN_EXE_exitbind"), "{refused}");
    assert!(follow_up.iter().any(|word| word == "--config"), "{refused}");
    let reread = project.call_exact_argv(&follow_up);
    assert!(
        reread.status.success(),
        "{}",
        String::from_utf8_lossy(&reread.stderr)
    );
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

/// R55 F8: every form a Work's roles meet, from the Lead's scope to its
/// acceptance, runs from the typed route as printed: each choice and the
/// permit through the fill route, the mechanical check by its own argv.
#[test]
fn every_role_form_of_a_work_runs_as_printed_to_acceptance() {
    let project = Project::new("emitted-typed-forms");
    project.setup();
    let work = project.begin("R1", "sh check.sh R1");
    let artifact = project.root.join("result.md");
    fs::write(
        &artifact,
        "Deterministic fixture result; not a semantic review.\n",
    )
    .unwrap();
    let artifact = artifact.to_str().unwrap();
    let reason = ("--reason", "Deterministic fixture decision.");
    let mut used = Vec::new();
    let mut done = false;
    for _ in 0..12 {
        let view = action_view(&project, &work);
        assert_every_choice_fillable(&view);
        let action = &view["effectiveAction"];
        let labels: Vec<&str> = action["choices"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|choice| choice["label"].as_str())
            .collect();
        let (argv, label) = match action["state"].as_str().unwrap() {
            "done" => {
                done = true;
                break;
            }
            _ if action["kind"] == "check" => {
                let command = &action["command"];
                assert_eq!(command["sameExecutableRequired"], false, "{view}");
                assert_eq!(command["sameConfigRequired"], false, "{view}");
                let argv: Vec<String> = command["argv"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|word| word.as_str().unwrap().to_owned())
                    .collect();
                (argv, "check")
            }
            _ if action["beforeEditing"].is_object() && action["currentGrant"].is_null() => (
                fill_argv(
                    &project,
                    &view,
                    "permit",
                    &[("--operation", "Fixture edit")],
                ),
                "permit",
            ),
            _ => {
                let label = ["scoped", "completed", "approved", "accepted"]
                    .into_iter()
                    .find(|label| labels.contains(label))
                    .unwrap_or_else(|| panic!("no expected choice in {labels:?}"));
                let form = &action["choices"][labels.iter().position(|l| *l == label).unwrap()];
                let mut options = vec![reason];
                if form["fill"]["artifactOption"].is_string() {
                    options.push(("--artifact-file", artifact));
                }
                (fill_argv(&project, &view, label, &options), label)
            }
        };
        let output = project.call_exact_argv(&argv);
        assert!(output.status.success(), "{label}: {output:?}");
        used.push(label);
    }
    assert!(
        done,
        "the Work did not reach done through its forms: {used:?}"
    );
    for label in ["scoped", "permit", "completed", "check", "accepted"] {
        assert!(used.contains(&label), "{label} not run: {used:?}");
    }
}

/// R55 F8 rework: the read routes a role meets outside the action forms run
/// as printed with no executable or configuration added: the humanHelp
/// route of `work next`, the continuation of a recorded return, and the
/// permit reply's next read and audit.
#[test]
fn agent_surface_read_routes_run_as_printed() {
    let project = Project::new("emitted-surface-reads");
    project.setup();
    let work = project.begin("R1", "sh check.sh R1");
    let run = |route: &Value, label: &str| {
        let argv: Vec<String> = route["command"]
            .as_array()
            .unwrap_or_else(|| panic!("{label}: no route in {route}"))
            .iter()
            .map(|word| word.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(argv[0], env!("CARGO_BIN_EXE_exitbind"), "{label}");
        assert_ne!(route["sameConfigRequired"], true, "{label}");
        assert_ne!(route["sameExecutableRequired"], true, "{label}");
        let output = project.call_exact_argv(&argv);
        assert!(output.status.success(), "{label}: {output:?}");
    };
    let full = project.value(&["work", "next", &work, "--json", "--full"], None);
    let help = &full["residual"]["humanHelp"]["nextAction"]["command"];
    assert_eq!(help["args"][0], "work", "{help}");
    run(&json!({"command": help["argv"]}), "humanHelp");

    let view = action_view(&project, &work);
    let reason = ("--reason", "Deterministic fixture decision.");
    let artifact = project.root.join("scope.md");
    fs::write(&artifact, "Scope: R1 only.\n").unwrap();
    let artifact = artifact.to_str().unwrap();
    let scoped = project.call_exact_argv(&fill_argv(
        &project,
        &view,
        "scoped",
        &[reason, ("--artifact-file", artifact)],
    ));
    assert!(scoped.status.success(), "{scoped:?}");
    let scoped: Value = serde_json::from_slice(&scoped.stdout).unwrap();
    assert_eq!(scoped["continuation"]["readOnly"], true, "{scoped}");
    run(&scoped["continuation"], "continuation");

    let view = action_view(&project, &work);
    let permit = project.call_exact_argv(&fill_argv(
        &project,
        &view,
        "permit",
        &[("--operation", "Fixture edit")],
    ));
    assert!(permit.status.success(), "{permit:?}");
    let permit: Value = serde_json::from_slice(&permit.stdout).unwrap();
    run(&permit["nextAction"], "permit nextAction");
    run(&permit["audit"], "permit audit");
}
