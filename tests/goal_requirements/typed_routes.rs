//! R55 F8: a printed route is a typed value. The route-carrying fields and
//! their renderers are read from `src/`, not listed by hand: every JSON key
//! that carries a command (`command`, `argv`, `commandSuffix`, `route`, or a
//! name ending in `Route`, `Argv` or `Suffix`) is written from a value built
//! by `crate::emitted::Route`, copied from another such field, or belongs to
//! a registered non-route class; and no command argv is spelled as a literal
//! list anywhere outside the builder. The P1 scans stay as the backstop.
use super::emitted_routes::sources;
use super::follow_up_routes::dispatched;
use std::collections::{BTreeMap, BTreeSet};

/// Field writes that carry no product route, per file with count and class.
const NOT_ROUTES: &[(&str, usize, &str)] = &[
    ("src/host/hooks.rs", 4, "installed host hook command"),
    (
        "src/project/setup_statusline.rs",
        1,
        "installed status line command",
    ),
    (
        "src/run/gate/value.rs",
        3,
        "the project's own check command",
    ),
    (
        "src/project/setup.rs",
        1,
        "the project's own pre-return command",
    ),
    (
        "src/work/early_evidence.rs",
        1,
        "the reported probe command",
    ),
    (
        "src/run/view/successor_protocol.rs",
        1,
        "label of the full route printed beside it",
    ),
    (
        "src/session_goal/display.rs",
        1,
        "sentence on how the Goal closes; its routes are printed lines",
    ),
];

/// Where an argv literal may be spelled: the builder's constructors, and the
/// argument list of a process this product spawns (never printed).
const BUILDERS: &[&str] = &[
    "Route::new(",
    "Emitted::new(",
    ".words(",
    "command_line(",
    ".args(",
    "args(",
    "args_with(",
];

/// A string literal: byte span and raw content.
struct Literal {
    start: usize,
    end: usize,
    text: String,
}

/// String literals of Rust source, skipping comments, char literals and
/// lifetimes; raw strings are literals too.
fn literals(text: &str) -> Vec<Literal> {
    let bytes = text.as_bytes();
    let ident =
        |at: usize| at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
        } else if rest.starts_with("/*") {
            i += rest.find("*/").map_or(rest.len(), |end| end + 2);
        } else if bytes[i] == b'r'
            && !ident(i)
            && (rest.starts_with("r\"") || rest.starts_with("r#"))
        {
            let hashes = rest[1..].bytes().take_while(|byte| *byte == b'#').count();
            if rest.as_bytes().get(1 + hashes) != Some(&b'"') {
                i += 1;
                continue;
            }
            let close = format!("\"{}", "#".repeat(hashes));
            let body = i + 2 + hashes;
            let end = text[body..].find(&close).map_or(text.len(), |at| body + at);
            out.push(Literal {
                start: i,
                end: end + close.len(),
                text: text[body..end].to_owned(),
            });
            i = end + close.len();
        } else if bytes[i] == b'"' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            out.push(Literal {
                start: i,
                end: j + 1,
                text: text[i + 1..j.min(text.len())].to_owned(),
            });
            i = j + 1;
        } else if bytes[i] == b'\'' {
            let next = rest[1..].chars().next().map_or(0, char::len_utf8);
            i += if rest[1..].starts_with('\\') {
                rest[2..].find('\'').map_or(1, |at| at + 3)
            } else if rest.as_bytes().get(1 + next) == Some(&b'\'') {
                next + 2
            } else {
                1
            };
        } else {
            i += 1;
        }
    }
    out
}

/// Production source: no `*tests.rs` file, no trailing `#[cfg(test)]`
/// module, and not the builder itself.
fn production() -> Vec<(String, String)> {
    sources()
        .into_iter()
        .filter(|(path, _)| !path.ends_with("tests.rs") && !path.starts_with("src/emitted"))
        .map(|(path, text)| (path, strip_test_module(&text)))
        .collect()
}

/// Source without its inline test module: in `src/` a `#[cfg(test)] mod
/// name {` block is always the file's last item.
fn strip_test_module(text: &str) -> String {
    for (start, marker) in text.match_indices("\n#[cfg(test)]\n") {
        let item = text[start + marker.len()..]
            .lines()
            .find(|line| !line.starts_with("#["))
            .unwrap_or("");
        if item.starts_with("mod ") && item.ends_with('{') {
            return text[..start].to_owned();
        }
    }
    text.to_owned()
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// Every command argv spelled as a literal list (`"work", "detail"` or an
/// option-only command and its option) outside the builder's constructors.
fn argv_literals(text: &str, families: &[(&str, Vec<String>)]) -> Vec<String> {
    let lits = literals(text);
    let mut found = Vec::new();
    for pair in lits.windows(2) {
        let between = text[pair[0].end..pair[1].start]
            .trim_start_matches(".into()")
            .trim_start_matches(".to_owned()")
            .trim_start_matches(".to_string()");
        if between.trim() != "," {
            continue;
        }
        let Some((_, subcommands)) = families.iter().find(|(name, _)| *name == pair[0].text) else {
            continue;
        };
        let next = &pair[1].text;
        let argv = if subcommands.is_empty() {
            next.starts_with("--")
        } else {
            subcommands.contains(next)
        };
        if !argv {
            continue;
        }
        let open = opening_bracket(text, &lits, pair[0].start);
        let context = text[..open].trim_end().trim_end_matches('&').trim_end();
        if !BUILDERS.iter().any(|builder| context.ends_with(builder)) {
            found.push(format!(
                "{}: [\"{}\", \"{next}\", ...]",
                line_of(text, pair[0].start),
                pair[0].text
            ));
        }
    }
    found
}

/// The literal spanning byte `at`, if any (literals are sorted by start).
fn containing(lits: &[Literal], at: usize) -> Option<&Literal> {
    let after = lits.partition_point(|lit| lit.start <= at);
    after
        .checked_sub(1)
        .map(|index| &lits[index])
        .filter(|lit| at < lit.end)
}

/// The unmatched `[` that opens the list holding byte `at`.
fn opening_bracket(text: &str, lits: &[Literal], at: usize) -> usize {
    let mut depth = 0;
    let mut i = at;
    while i > 0 {
        i -= 1;
        if let Some(lit) = containing(lits, i) {
            i = lit.start;
            continue;
        }
        match text.as_bytes()[i] {
            b']' => depth += 1,
            b'[' if depth == 0 => return i,
            b'[' => depth -= 1,
            _ => {}
        }
    }
    0
}

fn route_key(key: &str) -> bool {
    matches!(key, "command" | "argv" | "commandSuffix" | "route")
        || (key.starts_with(|c: char| c.is_ascii_lowercase())
            && key.bytes().all(|byte| byte.is_ascii_alphanumeric())
            && ["Route", "Argv", "Suffix"]
                .iter()
                .any(|end| key.ends_with(end)))
}

/// One write of a route-carrying field: its key and the value expression.
struct Write {
    line: usize,
    key: String,
    at: usize,
    value: String,
}

/// The byte where the expression starting at `at` ends: the first of
/// `stops` or an unmatched closing bracket at depth 0, skipping literals.
fn end_of(text: &str, lits: &[Literal], at: usize, stops: &[u8]) -> usize {
    let mut depth = 0i32;
    let mut i = at;
    while i < text.len() {
        if let Ok(found) = lits.binary_search_by_key(&i, |lit| lit.start) {
            i = lits[found].end;
            continue;
        }
        match text.as_bytes()[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => break,
            b')' | b']' | b'}' => depth -= 1,
            byte if depth == 0 && stops.contains(&byte) => break,
            _ => {}
        }
        i += 1;
    }
    i
}

/// The value written after `at`: up to `,` or `;` or the closing bracket.
fn expression(text: &str, lits: &[Literal], at: usize) -> String {
    text[at..end_of(text, lits, at, b",;")].trim().to_owned()
}

/// Every write of a route-carrying key: `"key": value` in a JSON literal,
/// `x["key"] = value`, and `.insert("key".into(), value)`.
fn writes(text: &str) -> Vec<Write> {
    let lits = literals(text);
    let mut out = Vec::new();
    for lit in &lits {
        if !route_key(&lit.text) {
            continue;
        }
        let after = &text[lit.end..];
        let trimmed = after.trim_start();
        let skip = after.len() - trimmed.len();
        let value_at = if trimmed.starts_with(':') && !trimmed.starts_with("::") {
            Some(lit.end + skip + 1)
        } else if let Some(rest) = trimmed.strip_prefix(']') {
            let rest_trim = rest.trim_start();
            (rest_trim.starts_with('=')
                && !rest_trim.starts_with("==")
                && !rest_trim.starts_with("=>"))
            .then_some(lit.end + skip + 1 + (rest.len() - rest_trim.len()) + 1)
        } else if text[..lit.start].ends_with("insert(") {
            trimmed
                .strip_prefix(".into(),")
                .map(|_| lit.end + skip + ".into(),".len())
        } else {
            None
        };
        if let Some(at) = value_at {
            out.push(Write {
                line: line_of(text, lit.start),
                key: lit.text.clone(),
                at,
                value: expression(text, &lits, at),
            });
        }
    }
    out
}

/// Whether an expression renders a `crate::emitted::Route`.
fn rendered(expression: &str) -> bool {
    let argv_field = expression.match_indices(".argv").any(|(at, _)| {
        !expression[at + 5..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
    });
    argv_field
        || [
            "Route::",
            "Emitted::",
            ".bounded(",
            ".suffix()",
            ".display_argv()",
            ".exec_line()",
            ".command_json()",
            "emitted::command_line(",
            "lead_route(",
        ]
        .iter()
        .any(|marker| expression.contains(marker))
}

/// `fn name` definitions with their full text, by name.
fn functions(sources: &[(String, String)]) -> BTreeMap<String, Vec<(String, String)>> {
    let mut index: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (path, text) in sources {
        let lits = literals(text);
        for (at, _) in text.match_indices("fn ") {
            if at > 0 && text.as_bytes()[at - 1].is_ascii_alphanumeric() {
                continue;
            }
            let name: String = text[at + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let Some(open) = text[at..].find('{').map(|open| at + open) else {
                continue;
            };
            let end = end_of(text, &lits, open + 1, b"") + 1;
            index
                .entry(name)
                .or_default()
                .push((path.clone(), text[at..end.min(text.len())].to_owned()));
        }
    }
    index
}

/// How a value expression is typed, or why it is not.
enum Typed {
    Rendered,
    Renderer(String),
    Copied,
    Label,
}

struct Scope<'a> {
    path: &'a str,
    text: &'a str,
    lits: &'a [Literal],
    functions: &'a BTreeMap<String, Vec<(String, String)>>,
    families: &'a [(&'a str, Vec<String>)],
}

/// Whether an expression reads a field of another JSON value (`x["key"]`).
fn reads_field(expression: &str) -> bool {
    expression.contains(".get(")
        || expression.match_indices("[\"").any(|(at, _)| {
            expression[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ']' | ')'))
        })
}

fn classify(scope: &Scope, written: &str, at: usize, depth: usize) -> Result<Typed, String> {
    let mut value = written.trim().trim_start_matches('&');
    if depth > 4 || value.is_empty() {
        return Err("not traceable to crate::emitted::Route".into());
    }
    for wrapper in ["json!(", "Some("] {
        if let Some(rest) = value
            .strip_prefix(wrapper)
            .and_then(|rest| rest.strip_suffix(')'))
        {
            value = rest.trim();
        }
    }
    let value = value
        .trim_end_matches('?')
        .trim_end_matches(".clone()")
        .trim_end_matches('?');
    if matches!(value, "null" | "Value::Null" | "None") {
        return Ok(Typed::Copied);
    }
    if value.starts_with('{') {
        // A nested object: each route-carrying key inside is its own write.
        return Ok(Typed::Copied);
    }
    if value.starts_with("if ") {
        for tail in tails(value) {
            classify(scope, &tail, at, depth + 1)?;
        }
        return Ok(Typed::Rendered);
    }
    let head = value.split('(').next().unwrap_or("");
    let path_call = value.len() > head.len()
        && !head.is_empty()
        && head
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':');
    let name = head.rsplit("::").next().unwrap_or("");
    if path_call && binding(scope, name, at).is_none() {
        if let Some(definitions) = scope.functions.get(name) {
            let local: Vec<_> = definitions
                .iter()
                .filter(|(path, _)| path == scope.path)
                .collect();
            let candidates = if local.is_empty() {
                definitions.iter().collect()
            } else {
                local
            };
            if candidates.iter().any(|(_, body)| {
                body.contains("Route") || body.contains("Bounded") || rendered(body)
            }) {
                return Ok(Typed::Renderer(name.to_owned()));
            }
        }
    }
    if rendered(value) {
        return Ok(Typed::Rendered);
    }
    if reads_field(value) {
        return Ok(Typed::Copied);
    }
    if let Some(literal) = value.strip_prefix('"') {
        let words: Vec<&str> = literal.trim_end_matches('"').split_whitespace().collect();
        let names_command = words.windows(2).any(|pair| {
            scope
                .families
                .iter()
                .any(|(name, subs)| *name == pair[0] && subs.iter().any(|sub| sub == pair[1]))
        });
        return if names_command {
            Err("prints a command as a string".into())
        } else {
            Ok(Typed::Label)
        };
    }
    let name: String = head
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || value[name.len()..].starts_with('.') {
        return Err("not traceable to crate::emitted::Route".into());
    }
    if let Some((initializer, tuple)) = binding(scope, &name, at) {
        if let Some(index) = tuple {
            // One element of a tuple: that element of every branch's tuple.
            for tail in tails(&initializer) {
                let element = tuple_element(&tail, index)
                    .ok_or_else(|| format!("`{name}` is not traceable in its tuple"))?;
                classify(scope, &element, at, depth + 1)?;
            }
            return Ok(Typed::Rendered);
        }
        if initializer.trim() == "Vec::new()" {
            let pushes: Vec<_> = scope
                .text
                .match_indices(&format!("{name}.push("))
                .map(|(push, _)| push + name.len() + 6)
                .collect();
            if pushes.is_empty() {
                return Err(format!("{name} is never filled"));
            }
            for push in pushes {
                classify(
                    scope,
                    &expression(scope.text, scope.lits, push),
                    push,
                    depth + 1,
                )?;
            }
            return Ok(Typed::Rendered);
        }
        return classify(scope, &initializer, at, depth + 1);
    }
    if parameter_typed(scope.text, &name, at) {
        return Ok(Typed::Rendered);
    }
    Err(format!("`{name}` is not built by crate::emitted::Route"))
}

/// The value of each branch of an `if` expression (the tail after its last
/// statement), or the expression itself.
fn tails(expression: &str) -> Vec<String> {
    let Some(branches) = expression.strip_prefix("if ") else {
        return vec![expression.to_owned()];
    };
    let lits = literals(branches);
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(open) = branches[cursor..].find('{').map(|open| cursor + open) {
        let close = end_of(branches, &lits, open + 1, b"");
        let body = &branches[open + 1..close];
        let body_lits = literals(body);
        let mut start = 0;
        loop {
            let end = end_of(body, &body_lits, start, b";");
            if end >= body.len() {
                break;
            }
            start = end + 1;
        }
        out.push(body[start..].trim().to_owned());
        cursor = close + 1;
        if !branches[cursor..].trim_start().starts_with("else") {
            break;
        }
    }
    out
}

/// Element `index` of a tuple expression `(a, b, ...)`.
fn tuple_element(expression: &str, index: usize) -> Option<String> {
    let inner = expression.trim().strip_prefix('(')?.strip_suffix(')')?;
    let lits = literals(inner);
    let mut start = 0;
    for _ in 0..index {
        start = end_of(inner, &lits, start, b",") + 1;
        if start > inner.len() {
            return None;
        }
    }
    Some(
        inner[start..end_of(inner, &lits, start, b",")]
            .trim()
            .to_owned(),
    )
}

/// The initializer of the last `let NAME` (or a closure, or a tuple
/// pattern naming it) before `at`, and whether the pattern is a tuple.
fn binding(scope: &Scope, name: &str, at: usize) -> Option<(String, Option<usize>)> {
    let text = scope.text;
    let before = &text[..at];
    let mut best: Option<(usize, Option<usize>)> = None;
    for (found, _) in before.match_indices("let ") {
        if found > 0 && before.as_bytes()[found - 1].is_ascii_alphanumeric() {
            continue;
        }
        let Some(pattern_end) = before[found..].find('=').map(|end| found + end) else {
            continue;
        };
        let pattern = &before[found + 4..pattern_end];
        let words: Vec<&str> = pattern
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .collect();
        if words.contains(&name) && !pattern.contains(';') {
            let tuple = pattern.trim_start().starts_with('(').then(|| {
                pattern
                    .split(',')
                    .position(|part| {
                        part.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                            .any(|word| word == name)
                    })
                    .unwrap_or(0)
            });
            best = Some((pattern_end + 1, tuple));
        }
    }
    let (start, tuple) = best?;
    let initializer = text[start..end_of(text, scope.lits, start, b";")].trim();
    Some((initializer.to_owned(), tuple))
}

fn parameter_typed(text: &str, name: &str, at: usize) -> bool {
    let Some(start) = text[..at].rfind("fn ") else {
        return false;
    };
    let signature = &text[start..text[start..].find('{').map_or(at, |end| start + end)];
    signature
        .split(&format!("{name}:"))
        .nth(1)
        .and_then(|rest| rest.split([',', ')']).next())
        .is_some_and(|kind| kind.contains("Route") || kind.contains("Bounded"))
}

struct Registry {
    fields: BTreeMap<String, usize>,
    renderers: BTreeSet<String>,
    failures: Vec<String>,
}

fn registry(sources: &[(String, String)], families: &[(&str, Vec<String>)]) -> Registry {
    let functions = functions(sources);
    let mut registry = Registry {
        fields: BTreeMap::new(),
        renderers: BTreeSet::new(),
        failures: Vec::new(),
    };
    for (path, text) in sources {
        if path == "src/cli/help.rs" {
            continue;
        }
        for literal in argv_literals(text, families) {
            registry.failures.push(format!(
                "{path}:{literal}: a command argv spelled outside crate::emitted::Route"
            ));
        }
        let lits = literals(text);
        let scope = Scope {
            path,
            text,
            lits: &lits,
            functions: &functions,
            families,
        };
        let mut refused = Vec::new();
        for write in writes(text) {
            *registry.fields.entry(write.key.clone()).or_default() += 1;
            match classify(&scope, &write.value, write.at, 0) {
                Ok(Typed::Renderer(name)) => {
                    registry.renderers.insert(name);
                }
                Ok(_) => {}
                Err(reason) => refused.push(format!(
                    "{path}:{}: \"{}\": {} ({reason})",
                    write.line, write.key, write.value
                )),
            }
        }
        let listed = NOT_ROUTES
            .iter()
            .find(|(file, _, _)| file == path)
            .map_or(0, |(_, count, _)| *count);
        if refused.len() != listed {
            registry.failures.push(format!(
                "{path}: {} route-carrying field(s) not built by crate::emitted::Route, {listed} registered as non-routes:\n  {}",
                refused.len(),
                refused.join("\n  ")
            ));
        }
    }
    registry
}

#[test]
fn every_route_carrying_field_is_rendered_from_the_typed_route() {
    let families = dispatched();
    let sources = production();
    assert!(sources.len() > 300, "{}", sources.len());
    let registry = registry(&sources, &families);
    eprintln!(
        "route registry: {} field writes over {} keys {:?}; {} renderers {:?}",
        registry.fields.values().sum::<usize>(),
        registry.fields.len(),
        registry.fields,
        registry.renderers.len(),
        registry.renderers
    );
    assert!(registry.fields.values().sum::<usize>() >= 100);
    assert!(registry.renderers.len() >= 6, "{:?}", registry.renderers);
    assert!(
        registry.failures.is_empty(),
        "{}",
        registry.failures.join("\n")
    );
}

/// The scanner sees the class: a field written from a format string, an
/// argv literal, a vector filled by hand, and a string naming a command
/// fail; fields rendered from a Route, copied or labelled pass.
#[test]
fn typed_route_scanner_detects_each_member_of_the_class() {
    let families = vec![
        ("work", vec!["next".to_owned(), "detail".to_owned()]),
        ("check", Vec::new()),
    ];
    let failing = concat!(
        "fn a(w: &str) -> Value {\n",
        "    json!({\"nextAction\": {\"command\": format!(\"exitbind work next {w}\")}})\n",
        "}\n",
        "fn b(exe: String) -> Value {\n",
        "    json!({\"readRoute\": [exe, \"work\", \"detail\", \"--json\"]})\n",
        "}\n",
        "fn c(w: &str) -> Value {\n",
        "    let argv = vec![\"work\".into(), \"next\".into(), w.into()];\n",
        "    json!({\"argv\": argv})\n",
        "}\n",
        "fn d(mut v: Value) {\n",
        "    v[\"route\"] = json!(\"work detail\");\n",
        "    v.insert(\"fillRoute\".into(), json!([\"check\", \"--config\"]));\n",
        "}\n",
    );
    let source = vec![("src/seeded.rs".to_owned(), failing.to_owned())];
    let found = registry(&source, &families).failures.join("\n");
    for expected in [
        "\"command\": format!(\"exitbind work next {w}\")",
        "[\"work\", \"detail\", ...]",
        "[\"work\", \"next\", ...]",
        "\"argv\": argv",
        "prints a command as a string",
        "[\"check\", \"--config\", ...]",
    ] {
        assert!(found.contains(expected), "{expected}\n{found}");
    }
    assert!(found.contains("5 route-carrying field(s)"), "{found}");
    let passing = concat!(
        "fn route(loaded: &Loaded, route: Route) -> Value {\n",
        "    route.config(&loaded.path).bounded(1024).command_json()\n",
        "}\n",
        "fn e(w: &str, l: &Loaded, v: &Value) -> Value {\n",
        "    let argv = Route::new([\"work\", \"next\", w]).config(&l.path).argv()?;\n",
        "    let mut all = Vec::new();\n",
        "    all.push(Route::new([\"work\", \"detail\", w]).suffix());\n",
        "    json!({\"command\": argv, \"commandSuffix\": all, \"route\": \"current.details.grouped\",\n",
        "        \"readRoute\": v[\"readRoute\"].clone(), \"fillRoute\": route(l, Route::new([\"check\"])),\n",
        "        \"beginRoute\": null, \"char\": '\"', \"lifetime\": x::<'a>()})\n",
        "}\n",
    );
    let source = vec![("src/seeded.rs".to_owned(), passing.to_owned())];
    let checked = registry(&source, &families);
    assert!(
        checked.failures.is_empty(),
        "{}",
        checked.failures.join("\n")
    );
    assert_eq!(
        checked.fields.values().sum::<usize>(),
        6,
        "{:?}",
        checked.fields
    );
    assert!(
        checked.renderers.contains("route"),
        "{:?}",
        checked.renderers
    );
    assert_eq!(
        literals("let c = '\"'; let r = r#\"a \"b\"\"#; // \"no\"\n\"yes\"")
            .iter()
            .map(|lit| lit.text.as_str())
            .collect::<Vec<_>>(),
        ["a \"b\"", "yes"]
    );
}
