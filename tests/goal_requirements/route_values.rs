//! R55 F8: route values found by the mechanism, not by key spelling. The
//! renderers are read from `src/emitted/route.rs` (every `pub(crate) fn
//! name(&self` of `Route` and `Bounded`; one documented as "Incomplete"
//! prints only beside the full argv) and `src/emitted.rs` (each free
//! function that builds a `Route`). In the rest of `src/` every constructed
//! `Route` reaches a complete renderer or a function typed `Route`, an
//! incomplete rendering sits beside the full argv in the same object, no
//! other code joins quoted words into a shell line, and no format string
//! prints an executable variable followed by a command.
use super::emitted_routes::sources;
use super::follow_up_routes::dispatched;
use super::rust_source::{end_of, functions, line_of, literals, opening, production, Literal};
use std::collections::BTreeMap;

/// Shell lines joined outside the builder that are run, never printed:
/// per file with count and why.
const SHELL_SPAWNS: &[(&str, usize, &str)] = &[(
    "src/host/away.rs",
    1,
    "the line tmux runs to start the away runner",
)];

/// The renderer names of the typed route.
pub(super) struct Renderers {
    pub(super) complete: Vec<String>,
    pub(super) incomplete: Vec<String>,
    pub(super) free: Vec<String>,
}

/// Every `pub(crate) fn name(&self` in `route.rs`, split by its doc comment,
/// and every free `pub(crate) fn` of `emitted.rs` whose body builds a Route.
pub(super) fn renderers_in(route: &str, emitted: &str) -> Renderers {
    let mut found = Renderers {
        complete: Vec::new(),
        incomplete: Vec::new(),
        free: Vec::new(),
    };
    for (at, marker) in route.match_indices("pub(crate) fn ") {
        let rest = &route[at + marker.len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !rest[name.len()..].starts_with("(&self") {
            continue;
        }
        let docs: Vec<&str> = route[..at]
            .lines()
            .rev()
            .skip(1)
            .take_while(|line| line.trim_start().starts_with("///"))
            .collect();
        if docs.iter().any(|line| line.contains("Incomplete")) {
            found.incomplete.push(name);
        } else {
            found.complete.push(name);
        }
    }
    let definitions = functions(&[("src/emitted.rs".to_owned(), emitted.to_owned())]);
    loop {
        let before = found.free.len();
        for (name, bodies) in &definitions {
            let public = emitted.contains(&format!("pub(crate) fn {name}("));
            let builds = bodies.iter().any(|(_, body)| {
                body.contains("Route::")
                    || found
                        .free
                        .iter()
                        .any(|free| free != name && body.contains(&format!("{free}(")))
            });
            if public && builds && !found.free.contains(name) {
                found.free.push(name.clone());
            }
        }
        if found.free.len() == before {
            return found;
        }
    }
}

pub(super) fn renderers() -> Renderers {
    let all = sources();
    let read = |path: &str| {
        all.iter()
            .find(|(file, _)| file == path)
            .map(|(_, text)| text.clone())
            .unwrap_or_else(|| panic!("{path} missing"))
    };
    renderers_in(&read("src/emitted/route.rs"), &read("src/emitted.rs"))
}

impl Renderers {
    /// Whether `text` calls a complete renderer.
    pub(super) fn rendered(&self, text: &str) -> bool {
        self.complete
            .iter()
            .any(|name| text.contains(&format!(".{name}(")))
            || self
                .free
                .iter()
                .any(|name| text.contains(&format!("emitted::{name}(")))
    }
}

/// Byte offsets of every whole-word occurrence of `word` after `from`.
fn occurrences(text: &str, word: &str, from: usize) -> Vec<usize> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text[from..]
        .match_indices(word)
        .map(|(at, _)| from + at)
        .filter(|at| !text[..*at].ends_with(ident) && !text[at + word.len()..].starts_with(ident))
        .collect()
}

/// Whether the value at `at` reaches a typed sink: a complete renderer on
/// its method chain, an argument of a function with a `Route` parameter,
/// or a function that returns a Route.
fn reaches(
    text: &str,
    lits: &[Literal],
    at: usize,
    sinks: &[String],
    renderers: &Renderers,
) -> bool {
    let end = end_of(text, lits, at, b",;");
    if renderers.rendered(&text[at..end]) {
        return true;
    }
    let open = opening(text, lits, at, b'(', b')');
    let callee: String = text[..open]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if open > 0 && sinks.contains(&callee) {
        return true;
    }
    let function = text[..at].rfind("fn ").unwrap_or(0);
    let signature = &text[function..text[function..].find('{').map_or(at, |end| function + end)];
    signature
        .split("->")
        .nth(1)
        .is_some_and(|returned| returned.contains("Route"))
}

/// Functions with a parameter typed `Route` (a sink for a route value).
pub(super) fn sinks(sources: &[(String, String)]) -> Vec<String> {
    functions(sources)
        .into_iter()
        .filter(|(_, bodies)| {
            bodies.iter().any(|(_, body)| {
                let signature = &body[..body.find('{').unwrap_or(body.len())];
                let params = signature.split("->").next().unwrap_or("");
                [
                    ": Route",
                    ": &Route",
                    ": crate::emitted::Route",
                    ": &crate::emitted::Route",
                ]
                .iter()
                .any(|kind| params.contains(kind))
            })
        })
        .map(|(name, _)| name)
        .collect()
}

/// Every route value in `sources` that is not rendered whole.
pub(super) fn unrendered(
    sources: &[(String, String)],
    sinks: &[String],
    renderers: &Renderers,
    families: &[(&str, Vec<String>)],
) -> Vec<String> {
    let mut failures = Vec::new();
    let mut spawns: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, text) in sources {
        let lits = literals(text);
        for constructor in [
            "Route::new(",
            "Emitted::new(",
            "Route::printed(",
            "Route::reprinted(",
        ] {
            for (found, _) in text.match_indices(constructor) {
                // The value starts at its path (`crate::emitted::Route::new`).
                let at = text[..found]
                    .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == ':')
                    .len();
                if reaches(text, &lits, at, sinks, renderers) {
                    continue;
                }
                // Bound to a name: some later use must reach a sink.
                let before = text[..at].trim_end();
                // `let name =`, `let Ok(name) =`, `name =`: the last identifier.
                let bound = before
                    .strip_suffix('=')
                    .map(|left| left.trim_end().trim_end_matches(')'))
                    .map(|left| {
                        left.chars()
                            .rev()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<String>()
                    })
                    .filter(|name| !name.is_empty());
                let used = bound.as_deref().is_some_and(|name| {
                    occurrences(text, name, at)
                        .into_iter()
                        .any(|use_at| reaches(text, &lits, use_at, sinks, renderers))
                });
                if !used {
                    failures.push(format!(
                        "{path}:{}: a Route is built and never rendered whole",
                        line_of(text, at)
                    ));
                }
            }
        }
        for name in &renderers.incomplete {
            for (at, _) in text.match_indices(&format!(".{name}(")) {
                let open = opening(text, &lits, at, b'{', b'}');
                let close = end_of(text, &lits, open + 1, b"");
                if !text[open..close].contains(".full_argv(") {
                    failures.push(format!(
                        "{path}:{}: `{name}` printed without the full argv beside it",
                        line_of(text, at)
                    ));
                }
            }
        }
        for quote in ["shell_quote", "emitted::word"] {
            for (at, _) in text.match_indices(quote) {
                let statement = text[at..].split(';').next().unwrap_or("");
                if statement.contains(".join(\" \")") {
                    *spawns.entry(path.as_str()).or_default() += 1;
                }
            }
        }
        for lit in &lits {
            for variable in ["{executable} ", "{exe} ", "{binary} ", "{bin} "] {
                for (at, _) in lit.text.match_indices(variable) {
                    let word: String = lit.text[at + variable.len()..]
                        .chars()
                        .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                        .collect();
                    if families.iter().any(|(name, _)| *name == word) {
                        failures.push(format!(
                            "{path}:{}: format string prints {variable}{word}; build the line with crate::emitted::Route",
                            line_of(text, lit.start)
                        ));
                    }
                }
            }
        }
    }
    for (path, count) in &spawns {
        let listed = SHELL_SPAWNS
            .iter()
            .find(|(file, _, _)| file == path)
            .map_or(0, |(_, count, _)| *count);
        if *count != listed {
            failures.push(format!(
                "{path}: {count} shell line(s) joined from quoted words outside crate::emitted::Route, {listed} registered as spawned"
            ));
        }
    }
    for (path, listed, _) in SHELL_SPAWNS {
        let scanned = sources.iter().any(|(file, _)| file == path);
        if scanned && !spawns.contains_key(path) && *listed > 0 {
            failures.push(format!("{path}: registered shell spawn no longer present"));
        }
    }
    failures
}

#[test]
fn every_route_value_is_rendered_whole_by_the_typed_route() {
    let renderers = renderers();
    for expected in [
        "full_argv",
        "within",
        "shell_text",
        "quoted_line",
        "command_json",
    ] {
        assert!(
            renderers.complete.iter().any(|name| name == expected),
            "{expected}"
        );
    }
    assert_eq!(renderers.incomplete, ["partial_suffix"]);
    assert!(renderers.free.iter().any(|name| name == "command_line"));
    let sources = production();
    let mut typed = sources.clone();
    typed.extend(
        super::emitted_routes::sources()
            .into_iter()
            .filter(|(path, _)| path.starts_with("src/emitted")),
    );
    let mut failures = unrendered(&sources, &sinks(&typed), &renderers, &dispatched());
    // A renderer name is the typed route's own: no other code defines it.
    let names = renderers.complete.iter().chain(&renderers.incomplete);
    for name in names {
        for (path, text) in &sources {
            if text.contains(&format!("fn {name}(")) {
                failures.push(format!("{path}: defines `{name}`, a Route renderer name"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The mechanism scan sees each member: a Route left as a suffix under any
/// key, a Route built and dropped, a second shell renderer and a format
/// string with an executable; a Route rendered, passed to a typed function
/// or returned passes.
#[test]
fn route_value_scanner_detects_each_member_of_the_class() {
    let route = concat!(
        "    /// The full argv.\n    pub(crate) fn full_argv(&self) -> Vec<String> {}\n",
        "    /// Incomplete on its own.\n    pub(crate) fn partial_suffix(&self) -> Vec<String> {}\n",
        "    pub(crate) fn words(mut self) -> Self {}\n",
    );
    let emitted =
        "pub(crate) fn command_line(w: &[&str]) -> String { Route::new(w).x() }\nfn word() {}\n";
    let renderers = renderers_in(route, emitted);
    assert_eq!(renderers.complete, ["full_argv"]);
    assert_eq!(renderers.incomplete, ["partial_suffix"]);
    assert_eq!(renderers.free, ["command_line"]);
    let families = vec![("work", vec!["next".to_owned()]), ("init", Vec::new())];
    let failing = concat!(
        "fn a(w: &str) -> Value {\n",
        "    json!({\"lifecycle\": {\"propose\": Route::new([\"memory\", \"propose\"]).partial_suffix()}})\n",
        "}\n",
        "fn b(w: &str) -> Value {\n",
        "    let dropped = Route::new([\"work\", \"next\", w]);\n",
        "    json!({\"retry\": format!(\"x\")})\n",
        "}\n",
        "fn c(argv: &[String]) -> Value {\n",
        "    json!({\"commands\": argv.iter().map(|w| shell_quote(w)).collect::<Vec<_>>().join(\" \")})\n",
        "}\n",
        "fn d(exe: &str) -> String {\n",
        "    format!(\"run {exe} init --mode portable\")\n",
        "}\n",
    );
    let seeded = [("src/seeded.rs".to_owned(), failing.to_owned())];
    let found = unrendered(&seeded, &sinks(&seeded), &renderers, &families).join("\n");
    for expected in [
        "src/seeded.rs:2: `partial_suffix` printed without the full argv beside it",
        "src/seeded.rs:5: a Route is built and never rendered whole",
        "src/seeded.rs: 1 shell line(s) joined",
        "src/seeded.rs:12: format string prints {exe} init",
    ] {
        assert!(found.contains(expected), "{expected}\n{found}");
    }
    let passing = concat!(
        "fn sink(l: &Loaded, route: Route) -> Value { route.full_argv() }\n",
        "fn made(w: &str) -> crate::emitted::Route { Route::new([\"work\", w]) }\n",
        "fn e(l: &Loaded, w: &str) -> Value {\n",
        "    let route = Route::new([\"work\", \"next\", w]);\n",
        "    let kept = Route::new([\"work\", w]);\n",
        "    let shown = Route::new([\"work\"]);\n",
        "    json!({\"a\": route.clone().words([\"x\"]).full_argv(), \"b\": sink(l, kept),\n",
        "        \"c\": {\"argv\": shown.clone().full_argv(), \"args\": shown.partial_suffix()},\n",
        "        \"d\": crate::emitted::command_line(&[\"work\", \"next\"])})\n",
        "}\n",
    );
    let seeded = [("src/seeded.rs".to_owned(), passing.to_owned())];
    let found = unrendered(&seeded, &sinks(&seeded), &renderers, &families);
    assert!(found.is_empty(), "{}", found.join("\n"));
}
