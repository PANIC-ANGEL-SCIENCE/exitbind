//! R55 F8: a printed route is a typed value. The route-carrying fields and
//! their renderers are read from `src/`, not listed by hand: every JSON key
//! that carries a command (`command`, `argv`, `commandSuffix`, `route`, or a
//! name ending in `Route`, `Argv` or `Suffix`) is written from a value built
//! by `crate::emitted::Route`, copied from another such field, or belongs to
//! a registered non-route class; and no command argv is spelled as a literal
//! list anywhere outside the builder. The P1 scans stay as the backstop.
use super::follow_up_routes::dispatched;
use super::route_values::{renderers, renderers_in, sinks, Renderers};
use super::rust_source::{
    end_of, expression, functions, line_of, literals, opening_bracket, production, Literal,
};
use std::collections::{BTreeMap, BTreeSet};

/// Field writes that carry no product route, per file and key with count
/// and class.
type NotRoutes = [(&'static str, &'static str, usize, &'static str)];
const NOT_ROUTES: &NotRoutes = &[
    (
        "src/host/hooks.rs",
        "command",
        4,
        "installed host hook command",
    ),
    (
        "src/project/setup_statusline.rs",
        "command",
        1,
        "installed status line command",
    ),
    (
        "src/run/gate/value.rs",
        "command",
        3,
        "the project's own check command",
    ),
    (
        "src/project/setup.rs",
        "command",
        1,
        "the project's own pre-return command",
    ),
    (
        "src/work/early_evidence.rs",
        "command",
        1,
        "the reported probe command",
    ),
    (
        "src/session_goal/display.rs",
        "closingRoute",
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
    renderers: &'a Renderers,
    sinks: &'a [String],
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
            let typed = |body: &str| {
                scope.renderers.rendered(body)
                    || scope.sinks.iter().any(|sink| {
                        body.match_indices(&format!("{sink}(")).any(|(at, _)| {
                            !body[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                        })
                    })
            };
            if candidates.iter().any(|(_, body)| typed(body)) {
                return Ok(Typed::Renderer(name.to_owned()));
            }
        }
    }
    if scope.renderers.rendered(value) {
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
    // A method chain or field of a local binding derives from the binding; a
    // field of anything else (`self.command`) is not traceable.
    let chained = value[name.len()..].starts_with('.');
    if name.is_empty() || (chained && binding(scope, &name, at).is_none()) {
        return Err("not traceable to crate::emitted::Route".into());
    }
    if let Some((initializer, tuple)) = binding(scope, &name, at) {
        if let Some(index) = tuple {
            // One element of a tuple: that element of every branch's tuple.
            for tail in tails(&initializer) {
                // A tuple built from a value (`items.unzip()`): trace the value.
                let element = tuple_element(&tail, index).unwrap_or(tail);
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
    // Only a binding of the enclosing function counts.
    let function = before.rfind("fn ").unwrap_or(0);
    let mut best: Option<(usize, Option<usize>)> = None;
    for (found, _) in before
        .match_indices("let ")
        .filter(|(found, _)| *found > function)
    {
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

fn registry(
    sources: &[(String, String)],
    families: &[(&str, Vec<String>)],
    not_routes: &NotRoutes,
    renderers: &Renderers,
) -> Registry {
    let functions = functions(sources);
    let mut typed = sources.to_vec();
    typed.extend(
        super::emitted_routes::sources()
            .into_iter()
            .filter(|(path, _)| path.starts_with("src/emitted")),
    );
    let sinks = sinks(&typed);
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
            renderers,
            sinks: &sinks,
        };
        let mut refused: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for write in writes(text) {
            *registry.fields.entry(write.key.clone()).or_default() += 1;
            match classify(&scope, &write.value, write.at, 0) {
                Ok(Typed::Renderer(name)) => {
                    registry.renderers.insert(name);
                }
                Ok(_) => {}
                Err(reason) => refused.entry(write.key.clone()).or_default().push(format!(
                    "{path}:{}: \"{}\": {} ({reason})",
                    write.line, write.key, write.value
                )),
            }
        }
        let mut keys: BTreeSet<&str> = refused.keys().map(String::as_str).collect();
        keys.extend(
            not_routes
                .iter()
                .filter(|(file, _, _, _)| file == path)
                .map(|(_, key, _, _)| *key),
        );
        for key in keys {
            let found = refused.get(key).map_or(&[][..], Vec::as_slice);
            let listed = not_routes
                .iter()
                .find(|(file, listed_key, _, _)| file == path && *listed_key == key)
                .map_or(0, |(_, _, count, _)| *count);
            if found.len() != listed {
                registry.failures.push(format!(
                    "{path}: {} \"{key}\" field(s) not built by crate::emitted::Route, {listed} registered as non-routes:\n  {}",
                    found.len(),
                    found.join("\n  ")
                ));
            }
        }
    }
    registry
}

#[test]
fn every_route_carrying_field_is_rendered_from_the_typed_route() {
    let families = dispatched();
    let sources = production();
    assert!(sources.len() > 300, "{}", sources.len());
    let renderers = renderers();
    let registry = registry(&sources, &families, NOT_ROUTES, &renderers);
    eprintln!(
        "route registry: {} field writes over {} keys {:?}; {} renderers {:?}",
        registry.fields.values().sum::<usize>(),
        registry.fields.len(),
        registry.fields,
        registry.renderers.len(),
        registry.renderers
    );
    assert!(registry.fields.values().sum::<usize>() >= 100);
    assert!(registry.renderers.len() >= 3, "{:?}", registry.renderers);
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
    let renderers = renderers_in(
        concat!(
            "    pub(crate) fn full_argv(&self) {}\n    pub(crate) fn within(&self) {}\n",
            "    pub(crate) fn command_json(&self) {}\n",
            "    /// Incomplete on its own.\n    pub(crate) fn partial_suffix(&self) {}\n",
        ),
        "",
    );
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
    let found = registry(&source, &families, &[], &renderers)
        .failures
        .join("\n");
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
    for key in ["command", "readRoute", "argv", "route", "fillRoute"] {
        assert!(
            found.contains(&format!("1 \"{key}\" field(s)")),
            "{key}\n{found}"
        );
    }
    // A non-route entry is bound to its key: the same count under another
    // key of the same file still fails, and so does a stale entry.
    let swapped = registry(
        &source,
        &families,
        &[
            ("src/seeded.rs", "readRoute", 1, "x"),
            ("src/seeded.rs", "propose", 1, "x"),
        ],
        &renderers,
    )
    .failures
    .join("\n");
    assert!(swapped.contains("1 \"command\" field(s)"), "{swapped}");
    assert!(!swapped.contains("1 \"readRoute\" field(s)"), "{swapped}");
    assert!(swapped.contains("0 \"propose\" field(s)"), "{swapped}");
    let passing = concat!(
        "fn route(loaded: &Loaded, route: Route) -> Value {\n",
        "    route.config(&loaded.path).within(1024).command_json()\n",
        "}\n",
        "fn e(w: &str, l: &Loaded, v: &Value) -> Value {\n",
        "    let argv = Route::new([\"work\", \"next\", w]).config(&l.path).full_argv()?;\n",
        "    let mut all = Vec::new();\n",
        "    all.push(Route::new([\"work\", \"detail\", w]).full_argv()?);\n",
        "    json!({\"command\": argv, \"commandSuffix\": all, \"route\": \"current.details.grouped\",\n",
        "        \"readRoute\": v[\"readRoute\"].clone(), \"fillRoute\": route(l, Route::new([\"check\"])),\n",
        "        \"beginRoute\": null, \"char\": '\"', \"lifetime\": x::<'a>()})\n",
        "}\n",
    );
    let source = vec![("src/seeded.rs".to_owned(), passing.to_owned())];
    let checked = registry(&source, &families, &[], &renderers);
    let suffix = "fn f(w: &str) -> Value { json!({\"commandSuffix\": Route::new([\"x\"]).partial_suffix()}) }";
    let partial = registry(
        &[("src/s.rs".to_owned(), suffix.to_owned())],
        &families,
        &[],
        &renderers,
    );
    assert!(
        !partial.failures.is_empty(),
        "an incomplete renderer is not a route"
    );
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
