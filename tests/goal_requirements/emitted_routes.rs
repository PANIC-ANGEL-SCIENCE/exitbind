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
    // Usage text with uppercase arguments, not a route to run as printed.
    ("src/cli/project.rs", 1, "usage text"),
    ("src/cli/project_graph.rs", 1, "usage text"),
    (
        "src/distribution/update.rs",
        2,
        "human repair hint after an update",
    ),
    ("src/host/bridge.rs", 1, "human status-line action"),
    ("src/work/return_result_impl.rs", 1, "usage text"),
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
        if path != "src/cli/help.rs" {
            let named = named_commands(text);
            let listed = NAMED_COMMANDS
                .iter()
                .find(|(file, _, _)| file == path)
                .map_or(0, |(_, count, _)| *count);
            if named != listed {
                failures.push(format!(
                    "{path}: {named} printed command(s) name the product literally, {listed} listed; print crate::emitted::executable() or crate::emitted::command_text"
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

/// Words that ask the reader to run what follows them.
const INSTRUCTIONS: &[&str] = &[
    "refresh with",
    "recompute with",
    "read",
    "use",
    "run",
    "rerun",
    "inspect",
    "via",
    "try",
    "follow",
    "call",
    "invoke",
    "see",
    "then",
];

/// Commands with subcommands, each read from the CLI file that dispatches it.
const DISPATCHERS: &[(&str, &str)] = &[
    ("work", "src/cli/work.rs"),
    ("goal", "src/cli/goal.rs"),
    ("memory", "src/cli/memory_cli.rs"),
    ("run", "src/cli/run_cli.rs"),
];

/// The names of a dispatcher's match arms: `"name" =>` or `"a" | "b" =>`.
fn arms(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let Some((head, _)) = line.trim_start().split_once(" =>") else {
            continue;
        };
        let parts: Vec<&str> = head.split(" | ").collect();
        let quoted = |part: &&str| {
            part.len() > 2
                && part.starts_with('"')
                && part.ends_with('"')
                && part[1..part.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-')
        };
        if parts.iter().all(quoted) {
            names.extend(parts.iter().map(|part| part[1..part.len() - 1].to_owned()));
        }
    }
    names
}

fn string_literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let mut escaped = false;
        let Some((end, _)) = after.char_indices().find(|(_, ch)| {
            let close = *ch == '"' && !escaped;
            escaped = *ch == '\\' && !escaped;
            close
        }) else {
            break;
        };
        out.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    out
}

/// Words between a subcommand and its arguments that make a usage error, not
/// a command template: `work next requires WORK`.
const USAGE_WORDS: &[&str] = &["requires", "accepts", "supports"];

/// Whether `rest` (the text after `<command> <subcommand> `) continues as a
/// template: an upper-case argument such as `WORK` or `ASSIGNMENT`, directly
/// or after one more subcommand word, instead of a registered `<NAME>` token.
fn template_arguments(rest: &str) -> bool {
    let upper =
        |word: &str| word.len() >= 3 && word.chars().all(|c| c.is_ascii_uppercase() || c == '_');
    let mut words = rest.split_whitespace();
    match words.next() {
        Some(word) if upper(word) => true,
        Some(word)
            if word.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && !USAGE_WORDS.contains(&word) =>
        {
            words.next().is_some_and(upper)
        }
        _ => false,
    }
}

/// Every literal that tells its reader to run `<command> <subcommand>` by
/// its bare words, so whatever PATH names would answer and the configuration
/// is left out, or that prints a command template with upper-case arguments
/// instead of registered placeholders. Lines that build the route through
/// `crate::emitted` pass.
fn bare_follow_ups(text: &str, commands: &[(&str, Vec<String>)]) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//")
            || line.contains("command_line(")
            || line.contains("command_text(")
        {
            continue;
        }
        for literal in string_literals(line) {
            let lower = literal.to_ascii_lowercase();
            let bare = commands.iter().any(|(command, names)| {
                let needle = format!("{command} ");
                lower.match_indices(&needle).any(|(at, _)| {
                    let rest = &lower[at + needle.len()..];
                    let word: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                        .collect();
                    let lead = lower[..at].trim_end_matches('`').trim_end();
                    let original = &literal[at + needle.len() + word.len()..];
                    names.contains(&word)
                        && (template_arguments(original)
                            || INSTRUCTIONS.iter().any(|instruction| {
                                lead.strip_suffix(instruction).is_some_and(|before| {
                                    before
                                        .chars()
                                        .next_back()
                                        .map_or(true, |c| !c.is_ascii_alphanumeric())
                                })
                            }))
                })
            });
            if bare {
                found.push(literal);
            }
        }
    }
    found
}

fn dispatched() -> Vec<(&'static str, Vec<String>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    DISPATCHERS
        .iter()
        .map(|(command, file)| {
            let names = arms(&fs::read_to_string(root.join(file)).unwrap());
            assert!(!names.is_empty(), "{file} dispatches no subcommand");
            (*command, names)
        })
        .collect()
}

/// A refusal or hint that names a follow-up command prints the full route:
/// the executable this process runs as and `--config`, through
/// `crate::emitted`. Static help text describes commands and is exempt.
#[test]
fn every_follow_up_a_refusal_or_hint_names_is_a_full_route() {
    let commands = dispatched();
    assert!(commands[0].1.iter().any(|name| name == "next"));
    let mut failures = Vec::new();
    for (path, text) in sources() {
        if path == "src/cli/help.rs" {
            continue;
        }
        for literal in bare_follow_ups(&text, &commands) {
            failures.push(format!(
                "{path}: \"{literal}\" names a follow-up command by its bare words; build it with crate::emitted::command_line (executable and --config)"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The follow-up scanner sees the class in any case and quoting, and passes
/// prose, usage errors, product-named lines and routes built by the builder.
#[test]
fn follow_up_scanner_detects_each_member_of_the_class() {
    let commands = vec![
        ("work", vec!["next".to_owned(), "detail".to_owned()]),
        ("goal", vec!["status".to_owned()]),
    ];
    assert_eq!(
        arms("        \"next\" => {\n        \"a-b\" | \"c\" => x,\n    \"Not\" => y,\n"),
        ["next", "a-b", "c"]
    );
    let found = bare_follow_ups(
        concat!(
            "Err(\"stale; Refresh With `Work Next` now\".into())\n",
            "json!({\"s\": \"then READ goal status\"})\n",
            "format!(\"x; use work detail {w} --json\")\n",
            "\"record: Work Detail WORK ASSIGNMENT --reason TEXT\"\n",
            "\"recover\": \"work next child WORK INTENT\"\n",
        ),
        &commands,
    );
    assert_eq!(found.len(), 5, "{found:?}");
    let passed = bare_follow_ups(
        concat!(
            "\"configuration changed during work detail consumption\"\n",
            "\"work next requires WORK\"\n",
            "\"work detail <WORK> --reason <REASON>\"\n",
            "\"do not repeat work begin; read exitbind work next\"\n",
            "\"use the network\"\n",
            "// refresh with work next\n",
            "let r = crate::emitted::command_line(&[\"work\", \"next\"], c); \"refresh with work next\"\n",
        ),
        &commands,
    );
    assert!(passed.is_empty(), "{passed:?}");
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
