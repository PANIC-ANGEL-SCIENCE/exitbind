//! The CLI's own tables, read from `src/` with the shared scanner (R55 F11):
//! the top-level commands the dispatcher matches, each family's subcommands
//! from the handler it dispatches to, option-bearing command forms and the
//! values each `--view` accepts beside the values its refusal names.
#![allow(dead_code)]
use super::rust_source::{
    blank, block, call_arguments, code_words, function, literals, matches, production, string_list,
};
use std::collections::BTreeSet;

/// The file whose `run` and `configured_command` dispatch every command.
pub const DISPATCHER: &str = "src/cli/mod.rs";
const ENTRY_FUNCTIONS: &[&str] = &["run", "configured_command"];

fn text_of<'a>(sources: &'a [(String, String)], path: &str) -> Option<&'a str> {
    sources
        .iter()
        .find(|(file, _)| file == path)
        .map(|(_, text)| text.as_str())
}

/// The `Some("name")` values a dispatch condition compares the first
/// positional with: `"project" if first == Some("graph") =>` makes `graph`
/// a subcommand of `project`.
fn guarded_subcommands(condition: &str) -> Vec<String> {
    if !condition.contains("positional.first()") {
        return Vec::new();
    }
    literals(condition)
        .into_iter()
        .filter(|lit| condition[..lit.start].ends_with("Some("))
        .map(|lit| lit.text)
        .collect()
}

/// One top-level command: its name, the code that handles it and the
/// subcommands its dispatch conditions name.
pub struct TopLevel {
    pub name: String,
    pub handler: String,
    pub subcommands: Vec<String>,
}

/// Every top-level command: the string arms of the dispatcher's entry
/// functions and every `command == "name"` test there (with the block it
/// guards). `--help` and `--version` alias `help` and `version`, which are
/// tested by name.
pub fn top_level(sources: &[(String, String)]) -> Vec<TopLevel> {
    let text = text_of(sources, DISPATCHER).expect("CLI dispatcher");
    let mut commands: Vec<TopLevel> = Vec::new();
    let mut add = |name: String, handler: &str, condition: &str| {
        if name.starts_with('-') {
            return;
        }
        let index = match commands.iter().position(|known| known.name == name) {
            Some(index) => index,
            None => {
                commands.push(TopLevel {
                    name,
                    handler: String::new(),
                    subcommands: Vec::new(),
                });
                commands.len() - 1
            }
        };
        commands[index].handler.push_str(handler);
        for sub in guarded_subcommands(condition) {
            if !commands[index].subcommands.contains(&sub) {
                commands[index].subcommands.push(sub);
            }
        }
    };
    for entry in ENTRY_FUNCTIONS {
        let body = function(text, entry).unwrap_or_else(|| panic!("{DISPATCHER}: fn {entry}"));
        for (scrutinee, arms) in matches(body) {
            if scrutinee != "command" {
                continue;
            }
            for arm in arms {
                for name in &arm.strings {
                    add(name.clone(), &arm.body, &arm.pattern);
                }
            }
        }
        let code = blank(body);
        let lits = literals(body);
        for at in code_words(body, "command") {
            let rest = &code[at + "command".len()..];
            let Some(compared) = rest.trim_start().strip_prefix("==") else {
                continue;
            };
            let start = code.len() - compared.trim_start().len();
            let Some(lit) = lits.iter().find(|lit| lit.start == start) else {
                continue;
            };
            // Only an `if` condition: its block follows before any arm or
            // statement ends (`Err(e) if command == "x" =>` is an arm guard).
            let Some(open) = code[lit.end..].find('{').map(|open| lit.end + open) else {
                continue;
            };
            if code[lit.end..open].contains("=>") || code[lit.end..open].contains(';') {
                continue;
            }
            add(lit.text.clone(), block(body, open), &body[lit.end..open]);
        }
    }
    commands
}

/// Every top-level command that reads the configuration: an arm of
/// `configured_command`'s dispatch (reached after the configuration loads)
/// or an arm of `run` whose handler loads it.
pub fn configured(sources: &[(String, String)]) -> BTreeSet<String> {
    let text = text_of(sources, DISPATCHER).expect("CLI dispatcher");
    let mut found = BTreeSet::new();
    for entry in ENTRY_FUNCTIONS {
        let body = function(text, entry).unwrap_or_else(|| panic!("{DISPATCHER}: fn {entry}"));
        for (scrutinee, arms) in matches(body) {
            if scrutinee != "command" {
                continue;
            }
            for arm in arms {
                if *entry == "configured_command" || arm.body.contains("config::load(") {
                    found.extend(arm.strings);
                }
            }
        }
    }
    found
}

/// The file a call path names from `caller`: `crate::a::b::f` is
/// `src/a/b.rs` (or `mod.rs`), `a::f` is beside the caller, `f` is the
/// caller itself.
fn callee_file(sources: &[(String, String)], caller: &str, path: &[&str]) -> Option<String> {
    let dir = caller.rsplit_once('/').map_or("src", |(dir, _)| dir);
    let base = match path.split_first() {
        None => return Some(caller.to_owned()),
        Some((&"crate", rest)) => format!("src/{}", rest.join("/")),
        Some((&"super", rest)) => format!("src/{}", rest.join("/")),
        Some(_) => format!("{dir}/{}", path.join("/")),
    };
    [format!("{base}.rs"), format!("{base}/mod.rs")]
        .into_iter()
        .find(|file| text_of(sources, file).is_some())
}

/// Every function `code` calls, as (path segments, name).
fn calls(code: &str) -> Vec<(Vec<String>, String)> {
    let blanked = blank(code);
    let bytes = blanked.as_bytes();
    let mut found = Vec::new();
    for (open, _) in blanked.match_indices('(') {
        let name_start = blanked[..open]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .map_or(0, |at| at + 1);
        let path = &blanked[name_start..open];
        if path.is_empty() || path.starts_with(':') || bytes[name_start].is_ascii_uppercase() {
            continue;
        }
        let mut segments: Vec<String> = path.split("::").map(str::to_owned).collect();
        let name = segments.pop().unwrap();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            found.push((segments, name));
        }
    }
    found
}

/// The subcommands one handler function dispatches on its first positional:
/// the string arms of `match action` once `action` is positional 0, and every
/// declared `*_ACTIONS` list it checks the action against.
fn subcommands(file_text: &str, body: &str) -> Vec<String> {
    let mut names = Vec::new();
    let code = blank(body);
    let first = code.find("let action = ").is_some_and(|at| {
        let statement = &code[at..at + code[at..].find(';').unwrap_or(code.len() - at)];
        statement.contains(".positional.first()")
            || statement.find("positional(").is_some_and(|call| {
                let open = at + call + "positional".len();
                call_arguments(body, open).get(1).map(String::as_str) == Some("0")
            })
    });
    if first {
        for (scrutinee, arms) in matches(body) {
            if scrutinee == "action" {
                names.extend(arms.into_iter().flat_map(|arm| arm.strings));
            }
        }
    }
    for (at, _) in code.match_indices(".contains(") {
        let list: String = code[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_uppercase() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if list.ends_with("_ACTIONS") {
            names.extend(string_list(file_text, &list));
        }
    }
    names.retain(|name| !name.is_empty());
    let mut seen = BTreeSet::new();
    names.retain(|name| seen.insert(name.clone()));
    names
}

/// Every top-level command with the subcommands its handler dispatches; an
/// option-only command has none.
pub fn families_in(sources: &[(String, String)]) -> Vec<(String, Vec<String>)> {
    top_level(sources)
        .into_iter()
        .map(|command| {
            let mut subs = command.subcommands;
            for (path, callee) in calls(&command.handler) {
                let segments: Vec<&str> = path.iter().map(String::as_str).collect();
                let Some(file) = callee_file(sources, DISPATCHER, &segments) else {
                    continue;
                };
                let file_text = text_of(sources, &file).unwrap();
                if let Some(body) = function(file_text, &callee) {
                    for sub in subcommands(file_text, body) {
                        if !subs.contains(&sub) {
                            subs.push(sub);
                        }
                    }
                }
            }
            (command.name, subs)
        })
        .collect()
}

/// The families of the product's own CLI.
pub fn families() -> Vec<(String, Vec<String>)> {
    families_in(&production())
}

/// The value text of the last `let name = VALUE;` before `at` in `text`:
/// a `match` with its whole block, or up to the `;` at depth 0.
fn bound_value<'a>(text: &'a str, at: usize, name: &str) -> Option<(usize, &'a str)> {
    let code = blank(text);
    let function = code[..at].rfind("fn ").unwrap_or(0);
    let plain = format!("let {name} = ");
    let mutable = format!("let mut {name} = ");
    let start = code_words(text, "let")
        .into_iter()
        .filter(|start| *start > function && *start < at && binding_visible(&code, *start, at))
        .filter(|start| code[*start..].starts_with(&plain) || code[*start..].starts_with(&mutable))
        .last()?;
    let value = start + code[start..].find("= ")? + 2;
    if code[value..].starts_with("match ") {
        let open = value + code[value..].find('{')?;
        let end = open + block(text, open).len();
        return Some((start, &text[value..end]));
    }
    let mut depth = 0i32;
    for (offset, byte) in code.as_bytes()[value..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b';' if depth == 0 => return Some((start, &text[value..value + offset])),
            _ => {}
        }
    }
    None
}

/// A binding inside a closed block cannot supply a later option check.
fn binding_visible(code: &str, start: usize, at: usize) -> bool {
    let mut depth = 0usize;
    for byte in code.as_bytes()[start..at].iter().copied() {
        match byte {
            b'{' => depth += 1,
            b'}' if depth == 0 => return false,
            b'}' => depth -= 1,
            _ => {}
        }
    }
    true
}

/// Literal values assigned to `name` by one component of a tuple-valued match.
/// Other tuple components and match patterns are not command names.
fn tuple_bound_names(text: &str, at: usize, name: &str) -> Option<(usize, Vec<String>)> {
    let code = blank(text);
    let function = code[..at].rfind("fn ").unwrap_or(0);
    let mut found = None;
    for start in code_words(text, "let")
        .into_iter()
        .filter(|start| *start > function && *start < at && binding_visible(&code, *start, at))
    {
        let rest = &code[start + 3..at];
        let open = start + 3 + rest.len() - rest.trim_start().len();
        if code.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let bindings = call_arguments(text, open);
        let Some(index) = bindings.iter().position(|binding| {
            let binding = binding.trim();
            binding.strip_prefix("mut ").unwrap_or(binding) == name
        }) else {
            continue;
        };
        let end = open + block(text, open).len();
        if end >= at {
            continue;
        }
        let rest = code[end..at].trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        if !value.starts_with("match ") {
            continue;
        }
        let value_start = at - value.len();
        let Some((_, arms)) = matches(&text[value_start..at]).into_iter().next() else {
            continue;
        };
        let mut names = Vec::new();
        for arm in arms {
            let body = arm.body.trim();
            if !body.starts_with('(') {
                continue;
            }
            let fields = call_arguments(body, 0);
            let Some(field) = fields.get(index) else {
                continue;
            };
            let values = literals(field);
            if let [literal] = values.as_slice() {
                if literal.start == 0 && literal.end == field.len() {
                    names.push(literal.text.clone());
                }
            }
        }
        found = Some((start, names));
    }
    found
}

/// The literal values of `value`: each literal after `=`, `[`, `,`, `{` or
/// `else` (a value), not an argument of a condition.
fn value_literals(value: &str) -> Vec<String> {
    let code = blank(value);
    literals(value)
        .into_iter()
        .filter(|lit| {
            let before = code[..lit.start].trim_end();
            before.is_empty()
                || (["=", "[", ",", "{", "else"]
                    .iter()
                    .any(|end| before.ends_with(end))
                    && !before.ends_with("=="))
        })
        .map(|lit| lit.text)
        .collect()
}

/// Every command whose option check accepts `--option`: the command name of
/// each `assert_options(NAME, arguments, OPTIONS)` call whose options (a
/// literal list, a `let` list, or a `let` table matched on the action, one
/// command per arm) include it; a NAME bound by `let` contributes each
/// command literal of that binding. Returned as (file, command), sorted.
pub fn option_commands(sources: &[(String, String)], option: &str) -> Vec<(String, String)> {
    let mut found = BTreeSet::new();
    for (file, text) in sources {
        let code = blank(text);
        for (at, _) in code.match_indices("assert_options(") {
            let open = at + "assert_options".len();
            let arguments = call_arguments(text, open);
            let [name, _, options] = arguments.as_slice() else {
                continue;
            };
            let names: Vec<String> = if name.starts_with('"') {
                literals(name).into_iter().map(|lit| lit.text).collect()
            } else {
                let scalar = bound_value(text, at, name)
                    .map(|(start, value)| (start, value_literals(value)));
                let tuple = tuple_bound_names(text, at, name);
                match (scalar, tuple) {
                    (Some(scalar), Some(tuple)) if scalar.0 > tuple.0 => scalar.1,
                    (_, Some(tuple)) => tuple.1,
                    (Some(scalar), None) => scalar.1,
                    (None, None) => Vec::new(),
                }
            };
            let options = options.trim_start_matches('&');
            // (suffix, options) pairs: one per arm of a table, else one.
            let mut lists: Vec<(Option<String>, Vec<String>)> = Vec::new();
            if options.starts_with('[') {
                lists.push((
                    None,
                    literals(options).into_iter().map(|lit| lit.text).collect(),
                ));
            } else if let Some((_, value)) = bound_value(text, at, options) {
                if value.starts_with("match ") {
                    for (_, arms) in matches(value).into_iter().take(1) {
                        for arm in arms {
                            let listed: Vec<String> = literals(&arm.body)
                                .into_iter()
                                .map(|lit| lit.text)
                                .collect();
                            for sub in arm.strings {
                                lists.push((Some(sub), listed.clone()));
                            }
                        }
                    }
                } else {
                    lists.push((
                        None,
                        literals(value).into_iter().map(|lit| lit.text).collect(),
                    ));
                }
            }
            for (sub, listed) in lists {
                if !listed.iter().any(|item| item == option) {
                    continue;
                }
                for name in &names {
                    let command = match &sub {
                        Some(sub) => format!("{name} {sub}"),
                        None => name.clone(),
                    };
                    found.insert((file.clone(), command));
                }
            }
        }
    }
    found.into_iter().collect()
}

/// One function that reads `--view`: the values it accepts and the values
/// its refusal text names.
pub struct Views {
    pub file: String,
    pub function: String,
    pub accepted: BTreeSet<String>,
    pub refused: BTreeSet<String>,
}

/// Every declared `const NAME: &[&str]` list named `name` in `sources`.
fn declared(sources: &[(String, String)], name: &str) -> Vec<String> {
    sources
        .iter()
        .flat_map(|(_, text)| string_list(text, name))
        .collect()
}

/// The upper-case list `code` checks with `NAME.contains(` or `NAME.join(`.
fn lists_in(code: &str, call: &str) -> Vec<String> {
    code.match_indices(call)
        .map(|(at, _)| {
            code[..at]
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// The `--view` readers among `files` (a subset of `sources`).
pub fn views_in(sources: &[(String, String)], files: &[(String, String)]) -> Vec<Views> {
    let mut found = Vec::new();
    for (file, text) in files {
        let index = super::rust_source::functions(&[(file.clone(), text.clone())]);
        for (name, bodies) in index {
            for (_, body) in bodies {
                let code = blank(&body);
                if !body.contains("get(\"view\")") {
                    continue;
                }
                let mut accepted = BTreeSet::new();
                for (scrutinee, arms) in matches(&body) {
                    if !scrutinee.contains("get(\"view\")") {
                        continue;
                    }
                    for arm in arms {
                        let pattern = arm.pattern.trim();
                        if let Some(value) = pattern
                            .strip_prefix("Some(\"")
                            .and_then(|rest| rest.split_once("\")"))
                        {
                            accepted.insert(value.0.to_owned());
                        }
                        for list in lists_in(&blank(pattern), ".contains(") {
                            accepted.extend(declared(sources, &list));
                        }
                    }
                }
                let lits = literals(&body);
                for compare in ["view != ", "view == "] {
                    for (at, _) in code.match_indices(compare) {
                        let start = at + compare.len();
                        if let Some(lit) = lits.iter().find(|lit| lit.start == start) {
                            accepted.insert(lit.text.clone());
                        }
                    }
                }
                let mut refused = BTreeSet::new();
                for lit in &lits {
                    let Some((_, named)) = lit.text.split_once("--view accepts ") else {
                        continue;
                    };
                    let named = named.split(';').next().unwrap_or(named);
                    let named = named.strip_prefix("only ").unwrap_or(named);
                    if named.starts_with("{}") {
                        let after = &code[lit.end..];
                        let call = &after[..after.find(';').unwrap_or(after.len())];
                        for list in lists_in(call, ".join(") {
                            refused.extend(declared(sources, &list));
                        }
                    } else {
                        refused.extend(
                            named
                                .split([',', ' '])
                                .filter(|word| !word.is_empty() && *word != "or")
                                .map(str::to_owned),
                        );
                    }
                }
                found.push(Views {
                    file: file.clone(),
                    function: name.clone(),
                    accepted,
                    refused,
                });
            }
        }
    }
    found
}
