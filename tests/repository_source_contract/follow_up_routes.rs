//! R55 F1 (pattern P1) across every CLI family: a refusal, hint or argv the
//! product prints names its follow-up command as a full route (this
//! executable, `--config`, registered `<NAME>` placeholders), never by bare
//! command words, a product-name variable or an upper-case template.
use super::cli_tables::{families, families_in};
use super::rust_source::{literals, production};

/// Stems of words that ask the reader to run what follows them; inflected
/// forms (`running`, `uses`, `inspecting`, `reran`...) count too.
const INSTRUCTION_STEMS: &[&str] = &[
    "read", "use", "run", "rerun", "ran", "reran", "inspect", "via", "try", "follow", "call",
    "invoke", "see", "then", "execute",
];

/// Variables that print the product's name, not the executable that runs.
const PRODUCT_NAMES: &[&str] = &["{command}", "{skill_name}", "{product}", "{caller}"];

/// Whether `word` is an instruction stem or one of its inflections.
fn instruction_word(word: &str) -> bool {
    INSTRUCTION_STEMS.iter().any(|stem| {
        word == *stem
            || word
                .strip_prefix(stem)
                .is_some_and(|suffix| ["s", "es", "ing", "ed", "ning", "ding"].contains(&suffix))
            || stem
                .strip_suffix('e')
                .is_some_and(|base| word == format!("{base}ing") || word == format!("{base}ed"))
    })
}

/// Whether the text before a command ends with an instruction: an
/// instruction word, or `refresh/recompute/inspect ... with`.
fn instructed(lead: &str) -> bool {
    if !lead.ends_with(|c: char| c.is_ascii_alphabetic()) {
        return false;
    }
    let words: Vec<&str> = lead
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .collect();
    match words.as_slice() {
        [.., before, "with"] => ["refresh", "recompute", "inspect"]
            .iter()
            .any(|stem| before.starts_with(stem)),
        [.., last] => instruction_word(last),
        [] => false,
    }
}

/// Whether `rest` (after the command words) continues like a route: an
/// option, placeholder, quoted or upper-case argument, directly or after one
/// positional word.
fn route_like(rest: &str) -> bool {
    let starts = |token: &str| {
        token.starts_with("--")
            || token.starts_with(['<', '{', '"', '\''])
            || token.len() >= 3 && token.chars().all(|c| c.is_ascii_uppercase() || c == '_')
    };
    let mut tokens = rest.split_whitespace();
    match tokens.next() {
        Some(first) if starts(first) => true,
        Some(first) if first.chars().all(|c| c.is_ascii_lowercase() || c == '-') => {
            tokens.next().is_some_and(starts)
        }
        _ => false,
    }
}

/// Whether the options after an option-only command carry an upper-case
/// template value such as `--root ROOT`.
fn template_options(rest: &str) -> bool {
    rest.starts_with("--")
        && rest
            .split_whitespace()
            .find(|token| !token.starts_with("--"))
            .is_some_and(|token| {
                token.len() >= 3 && token.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            })
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
fn bare_follow_ups(text: &str, commands: &[(String, Vec<String>)]) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//")
            || line.contains("command_line(")
            || line.contains("command_text(")
        {
            continue;
        }
        for literal in located_literals(line).into_iter().map(|(_, text)| text) {
            let lower = literal.to_ascii_lowercase();
            let bare = commands.iter().any(|(command, names)| {
                let needle = format!("{command} ");
                lower.match_indices(&needle).any(|(at, _)| {
                    let before = lower[..at].chars().next_back().unwrap_or(' ');
                    if !" `('\"\n".contains(before) && !lower[..at].ends_with('}') {
                        return false;
                    }
                    let quotes: &[char] = &['`', '\'', '"', ' ', '\n'];
                    let lead = lower[..at].trim_end_matches(quotes);
                    // A product-name variable prints the name, not the executable.
                    let named = PRODUCT_NAMES.iter().find(|name| lead.ends_with(**name));
                    let lead = named.map_or(lead, |name| {
                        lead[..lead.len() - name.len()].trim_end_matches(quotes)
                    });
                    let asked = instructed(lead);
                    let original = &literal[at + needle.len()..];
                    if names.is_empty() {
                        // A command without subcommands: its options follow.
                        return template_options(original)
                            || ((asked || named.is_some()) && route_like(original));
                    }
                    let rest = &lower[at + needle.len()..];
                    let word: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                        .collect();
                    let after = &original[word.len()..];
                    // A product name before a variable subcommand is a route too.
                    (named.is_some() && word.is_empty() && after.starts_with('{'))
                        || names.contains(&word)
                            && (template_arguments(after)
                                || asked
                                || (named.is_some() && route_like(after)))
                })
            });
            if bare {
                found.push(literal);
            }
        }
    }
    found
}

fn upper_word(text: &str) -> bool {
    text.len() >= 3
        && text.starts_with(|c: char| c.is_ascii_uppercase())
        && text.chars().all(|c| c.is_ascii_uppercase() || c == '_')
}

/// The string literals one line opens and closes, with the byte offset of
/// their opening quote, read by the shared scanner (a literal that runs on
/// past the line is not one of its literals).
fn located_literals(line: &str) -> Vec<(usize, String)> {
    literals(line)
        .into_iter()
        .filter(|lit| {
            line.as_bytes()[lit.start] == b'"'
                && lit.end == lit.start + lit.text.len() + 2
                && line.as_bytes()[lit.end - 1] == b'"'
        })
        .map(|lit| (lit.start, lit.text))
        .collect()
}

/// Every bare upper-case word an argv array prints as an argument, such as
/// `"work", "detail", "WORK"` or a `"CURRENT_WORK".into()` element, instead
/// of a registered `crate::emitted::slot`. The statement around it must
/// carry a command word and an option, so environment-variable lists,
/// placeholder-name lists and JSON values do not count.
fn argv_templates(text: &str, commands: &[(String, Vec<String>)]) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") || line.contains("placeholders") {
            continue;
        }
        let literals = located_literals(line);
        // A list of several placeholder names is not an argv.
        if literals.len() > 1 && literals.iter().all(|(_, literal)| upper_word(literal)) {
            continue;
        }
        for (at, literal) in &literals {
            let before = line[..*at].trim_end();
            let after = line[at + literal.len() + 2..].trim_start();
            // Slot arguments, JSON keys and values, one-name lists.
            if !upper_word(literal)
                || before.ends_with("slot(")
                || ["env(", "var(", "var_os(", "env_remove("]
                    .iter()
                    .any(|call| before.ends_with(call))
                || before.ends_with(':')
                || after.starts_with(':')
                || (before.ends_with('[') && after.starts_with(']'))
            {
                continue;
            }
            let mut start = index;
            while start > 0
                && index - start < 12
                && !lines[start - 1].trim_end().ends_with([';', '{', '}'])
            {
                start -= 1;
            }
            let mut end = index;
            while end + 1 < lines.len()
                && end - index < 4
                && !lines[end].trim_end().ends_with([';', '{', '}'])
            {
                end += 1;
            }
            let end = end + 1;
            let window = lines[start..end].join(" ");
            let commanded = commands
                .iter()
                .any(|(command, _)| window.contains(&format!("\"{command}\"")));
            // The value of the option just before it, as in an argv extension.
            let previous = literals[..literals.iter().position(|(start, _)| start == at).unwrap()]
                .last()
                .map(|(_, literal)| literal.clone())
                .or_else(|| {
                    let above = located_literals(lines[index.checked_sub(1)?]);
                    above.last().map(|(_, literal)| literal.clone())
                });
            let option_value = literal.contains('_')
                && previous.is_some_and(|previous| previous.starts_with("--"));
            if (commanded && window.contains("\"--")) || option_value {
                found.push(literal.clone());
            }
        }
    }
    found
}

/// Every command family with the subcommands its dispatcher accepts, read
/// from the CLI's own tables; an option-only command has none.
pub(super) fn dispatched() -> Vec<(String, Vec<String>)> {
    let mut commands = families();
    let goal = commands
        .iter_mut()
        .find(|(name, _)| name == "goal")
        .expect("goal dispatcher");
    goal.1 = super::goal_action::GoalAction::ALL
        .iter()
        .map(|action| action.name().to_owned())
        .collect();
    commands
}

/// A refusal or hint that names a follow-up command prints the full route:
/// the executable this process runs as and `--config`, through
/// `crate::emitted`. Static help text describes commands and is exempt.
#[test]
fn every_follow_up_a_refusal_or_hint_names_is_a_full_route() {
    let commands = dispatched();
    let work = commands.iter().find(|(name, _)| name == "work").unwrap();
    assert!(work.1.iter().any(|name| name == "next"), "{commands:?}");
    let mut failures = Vec::new();
    for (path, text) in production() {
        if path == "src/cli/help.rs" {
            continue;
        }
        for literal in argv_templates(&text, &commands) {
            failures.push(format!(
                "{path}: argv argument \"{literal}\" is a bare upper-case template; print it with crate::emitted::slot"
            ));
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
    // Families come from the dispatcher and the handler it calls: a
    // subcommand arm or a declared action list, in any spelling, is one; an
    // arm-like line in a literal or comment, or a nested match, is not.
    let seeded = vec![
        (
            "src/cli/mod.rs".to_owned(),
            concat!(
                "fn run(argv: Vec<String>) -> Result<(), String> {\n",
                "    match command {\n",
                "        \"init\" => init(&parsed),\n",
                "        \"project\" if parsed.positional.first().map(String::as_str) == Some(\"graph\") => {\n",
                "            project_graph::command(&parsed)\n",
                "        }\n",
                "        command => configured_command(command, &parsed),\n",
                "    }\n",
                "}\n",
                "fn configured_command(command: &str, a: &Arguments) -> Result<(), String> {\n",
                "    match command {\n",
                "        \"work\" => work::work_command(&loaded, a),\n",
                "        \"project\" => project::command(&loaded, a),\n",
                "        _ => Err(\"unknown\".into()),\n",
                "    }\n",
                "}\n",
            )
            .to_owned(),
        ),
        (
            "src/cli/work.rs".to_owned(),
            concat!(
                "pub(super) fn work_command(l: &Loaded, a: &Arguments) -> Result<(), String> {\n",
                "    let action = positional(\n        a,\n        0,\n        \"work requires next\",\n    )?;\n",
                "    // \"commented\" => old,\n",
                "    match action {\n",
                "        \"next\" => next(),\n",
                "        \"a-b\" | \"c\" => {\n            match mode { \"nested\" => 1, _ => 2 }\n        }\n",
                "        \"new-arm\" => Err(\"x => y\".into()),\n",
                "        _ => Err(\"\\\"text\\\" => no\".into()),\n",
                "    }\n",
                "}\n",
            )
            .to_owned(),
        ),
        (
            "src/cli/project.rs".to_owned(),
            concat!(
                "pub(crate) const PROJECT_ACTIONS: &[&str] = &[\n    \"agents\",\n    \"advance\",\n];\n",
                "pub(super) fn command(l: &Loaded, arguments: &Arguments) -> Result<(), String> {\n",
                "    let action = arguments.positional.first().map(String::as_str);\n",
                "    if !action.is_some_and(|action| PROJECT_ACTIONS.contains(&action)) { return Err(x); }\n",
                "    Ok(())\n",
                "}\n",
            )
            .to_owned(),
        ),
    ];
    let commands = families_in(&seeded);
    assert_eq!(
        commands,
        [
            ("init".to_owned(), Vec::new()),
            (
                "project".to_owned(),
                vec![
                    "graph".to_owned(),
                    "agents".to_owned(),
                    "advance".to_owned()
                ]
            ),
            (
                "work".to_owned(),
                ["next", "a-b", "c", "new-arm"].map(str::to_owned).to_vec()
            ),
        ]
    );
    let mut commands = commands;
    commands[2].1.push("detail".to_owned());
    commands.push(("goal".to_owned(), vec!["status".to_owned()]));
    let found = bare_follow_ups(
        concat!(
            "Err(\"stale; Refresh With `Work Next` now\".into())\n",
            "json!({\"s\": \"then READ goal status\"})\n",
            "format!(\"x; use work detail {w} --json\")\n",
            "\"record: Work Detail WORK ASSIGNMENT --reason TEXT\"\n",
            "\"recover\": \"work next child WORK INTENT\"\n",
            "\"x (See Project Agents --json)\"\n",
            "\"explicitly RUN '{command} Init --refresh-skills --root {}'\"\n",
            "\"resolve ownership before Running init --x --root ROOT to refresh\"\n",
            "\"CLI reference:\\n  {skill_name} Work Next change --goal \\\"x\\\"\"\n",
            "\"then inspecting `goal status`\"\n",
            "format!(\"{command} work {action} --config={}\", c)\n",
        ),
        &commands,
    );
    assert_eq!(found.len(), 11, "{found:?}");
    let passed = bare_follow_ups(
        concat!(
            "\"configuration changed during work detail consumption\"\n",
            "\"work next requires WORK\"\n",
            "\"work detail <WORK> --reason <REASON>\"\n",
            "\"do not repeat work begin; read exitbind work next\"\n",
            "\"use the network\"\n",
            "\"run init for the project\"\n",
            "\"only a successful {skill_name} work next result counts\"\n",
            "\"'{skill_name} init' validates configuration\"\n",
            "\"use the project advance form\"\n",
            "// refresh with work next\n",
            "let r = crate::emitted::command_line(&[\"work\", \"next\"], c); \"refresh with work next\"\n",
        ),
        &commands,
    );
    assert!(passed.is_empty(), "{passed:?}");
    let argv = argv_templates(
        concat!(
            "let args = vec![\n",
            "    \"goal\".into(),\n",
            "    \"--result-ref\".into(),\n",
            "    \"Current_Work\".into(),\n",
            "    \"CURRENT_WORK\".into(),\n",
            "];\n",
            "route(&[\"work\", \"detail\", \"WORK\", \"--json\"]);\n",
            "let env = [\n    \"GIT_DIR\",\n    \"HOME\",\n];\n",
            "json!({\"work\": w, \"readiness\": \"IN_PROGRESS\", \"x\": \"--a\"});\n",
            "route(&[\"work\", \"next\", &crate::emitted::slot(\"WORK\"), \"--json\"]);\n",
            "choice(\n    &[\"work\", \"--reason\"],\n    &[\"REASON\", \"NEW_LEDGER\"],\n);\n",
            "args.extend([\n    \"--result-ref\".into(),\n    \"ACCEPTED_WORK\".into(),\n]);\n",
            "git(&[\"rev-parse\", \"--verify\", \"HEAD\"]);\n",
            "cmd.args([\"--exact\", \"run\"]).env(\"EXITBIND_TEST_MODE\", m);\n",
        ),
        &commands,
    );
    assert_eq!(argv, ["CURRENT_WORK", "WORK", "ACCEPTED_WORK"], "{argv:?}");
}
