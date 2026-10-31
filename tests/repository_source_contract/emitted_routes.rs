//! Source contracts for emitted routes and placeholders.
use super::cli_tables::top_level;
use super::rust_source::{code_words, line_of, literals, production};

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

/// Executable lookups in production code that print no route: identity
/// hashing, self-spawned children and the self-update. Count per file; test
/// code prints nothing and is not scanned.
const NOT_EMITTERS: &[(&str, usize, &str)] = &[
    (
        "src/producer.rs",
        2,
        "build identity hashes the running executable",
    ),
    ("src/main.rs", 1, "invoked-name compatibility check"),
    ("src/host/away.rs", 1, "spawns its own detached runner"),
    (
        "src/run/capture_supervisor/linux.rs",
        1,
        "spawns its capture supervisor",
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
];

/// Every printed `<product> <command>` in a string literal, for the
/// product's top-level `commands` (read from the CLI dispatcher).
fn named_commands(text: &str, commands: &[String]) -> usize {
    let mut found = 0;
    for lit in literals(text) {
        let line = lit.text.as_str();
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
                    && commands.iter().any(|command| *command == word)
                    && " `\"'".contains(after)
                {
                    found += 1;
                }
            }
        }
    }
    found
}

/// The line of every executable lookup in code, by any spelling:
/// `std::env::current_exe()`, `env::current_exe ()`, an imported
/// `current_exe` called or the function passed as a value by path; not in a
/// literal or comment, and not a variable that happens to share the name.
fn executable_lookups(text: &str) -> Vec<usize> {
    let code = super::rust_source::blank(text);
    code_words(text, "current_exe")
        .into_iter()
        .filter(|at| {
            code[..*at].trim_end().ends_with("::")
                || code[at + "current_exe".len()..]
                    .trim_start()
                    .starts_with('(')
        })
        .map(|at| line_of(text, at))
        .collect()
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
    let sources = production();
    let commands: Vec<String> = top_level(&sources).into_iter().map(|c| c.name).collect();
    assert!(commands.len() >= 20, "{commands:?}");
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
            let named = named_commands(text, &commands);
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
        let lookups = executable_lookups(text).len();
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
    // The commands come from the dispatcher: a command added there is one.
    let dispatcher = concat!(
        "pub fn run(argv: Vec<String>) -> Result<(), String> {\n",
        "    if command == \"update\" { return update(); }\n",
        "    match command {\n",
        "        \"session\" => session(&parsed),\n",
        "        \"new-thing\" | \"work\" => configured_command(command, &parsed),\n",
        "        command => configured_command(command, &parsed),\n",
        "    }\n",
        "}\n",
        "fn configured_command(command: &str, a: &Arguments) -> Result<(), String> {\n",
        "    match command { \"init\" => init(a), _ => Err(\"x\".into()) }\n",
        "}\n",
    );
    let seeded = vec![("src/cli/mod.rs".to_owned(), dispatcher.to_owned())];
    let commands: Vec<String> = top_level(&seeded).into_iter().map(|c| c.name).collect();
    assert_eq!(commands, ["session", "new-thing", "work", "update", "init"]);
    // A literal product-name command, in a format string or plain literal;
    // prose, paths, comments and non-commands are not commands.
    assert_eq!(
        named_commands(
            "format!(\"read: exitbind work detail {w}\"); \"`exitbind session recovery show`\"",
            &commands
        ),
        2
    );
    assert_eq!(
        named_commands("format!(\"{product} new-thing {w}\")", &commands),
        1
    );
    assert_eq!(
        named_commands("\"/bin/exitbind work\" \"{product} update: x\" \"Exitbind recovered\"\n// \"exitbind work\"", &commands),
        0
    );
    // An executable lookup in any spelling is one; a comment, a literal and
    // a test module are not code that prints.
    let lookups = concat!(
        "fn a() { let _ = std::env::current_exe(); }\n",
        "fn b() { let _ = env::current_exe (); }\n",
        "use std::env::current_exe;\n",
        "fn c() { current_exe().ok(); }\n",
        "fn d() -> Lookup { std::env::current_exe }\n",
        "// current_exe() in a comment\n",
        "const S: &str = \"current_exe()\";\n",
        "fn not_current_exe_named() {}\n",
        "fn e(current_exe: Option<&Path>) { let current_exe = current_exe.clone(); }\n",
    );
    assert_eq!(executable_lookups(lookups), [1, 2, 3, 4, 5]);
    let tested = format!(
        "#[cfg(test)]\nmod tests {{\n    const C: char = '}}';\n    fn t() {{ std::env::current_exe(); }}\n}}\n{lookups}"
    );
    let production = super::rust_source::strip_test_modules(&tested);
    assert_eq!(executable_lookups(&production), [6, 7, 8, 9, 10]);
    // The base spelling missed the spaced and path-value forms.
    assert_eq!(lookups.matches("current_exe()").count(), 4);
}
