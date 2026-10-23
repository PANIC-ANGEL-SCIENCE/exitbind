//! R55 F8: route values found by the mechanism, not by key spelling. The
//! renderers are read from `src/emitted/route.rs` (every `pub(crate) fn
//! name(&self` of `Route` and `Bounded`; one documented as "Incomplete"
//! prints only beside the full argv) and `src/emitted.rs` (each free
//! function that builds a `Route`). In the rest of `src/` every constructed
//! `Route` reaches a complete renderer or a function typed `Route`, an
//! incomplete rendering sits beside the full argv in the same object, no
//! other code joins quoted words into a shell line, and no format string
//! prints an executable variable followed by a command.
use super::follow_up_routes::dispatched;
use super::rust_source::{
    end_of, functions, line_of, literals, opening, production, sources, Literal,
};
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

/// Production source the route scans read: every file but the builder
/// itself (`src/emitted*`), whose renderers are read separately.
pub(super) fn route_sources() -> Vec<(String, String)> {
    production()
        .into_iter()
        .filter(|(path, _)| !path.starts_with("src/emitted"))
        .collect()
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
    families: &[(String, Vec<String>)],
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

/// Whether the expression starting at `at` carries the configuration on its
/// chain.
fn configured_chain(text: &str, lits: &[Literal], at: usize) -> bool {
    let chain = &text[at..end_of(text, lits, at, b",;")];
    chain.contains(".config(") || chain.contains(".config_text(")
}

/// Whether the function a call names (`name`, `module::name`) adds the
/// configuration to the Route it is given: its body calls `.config(` or
/// passes on to a function that does. The definition is looked up in the
/// named module's file, else the caller's file, else every definition.
fn adds_config(
    definitions: &BTreeMap<String, Vec<(String, String)>>,
    qualified: &str,
    caller: &str,
    depth: usize,
) -> bool {
    // `"key":name(` in a JSON literal: the `:` belongs to the key.
    let qualified = qualified.trim_start_matches(':');
    let mut segments: Vec<&str> = qualified.split("::").filter(|s| !s.is_empty()).collect();
    let Some(name) = segments.pop() else {
        return false;
    };
    let Some(bodies) = definitions.get(name) else {
        return false;
    };
    let module = segments.last().copied();
    let pick = |keep: &dyn Fn(&str) -> bool| -> Vec<(&String, &String)> {
        bodies
            .iter()
            .filter_map(|(file, body)| keep(file).then_some((file, body)))
            .collect()
    };
    let mut chosen = match module {
        // `super::name` from `dir/child.rs` is defined in `dir.rs` or `dir/mod.rs`.
        Some("super") => {
            let dir = caller
                .rsplit_once('/')
                .map_or("", |(dir, _)| dir)
                .to_owned();
            pick(&|file: &str| file == format!("{dir}.rs") || file == format!("{dir}/mod.rs"))
        }
        Some(module) => pick(&|file: &str| {
            file.ends_with(&format!("/{module}.rs")) || file.contains(&format!("/{module}/"))
        }),
        None => Vec::new(),
    };
    if chosen.is_empty() {
        chosen = pick(&|file: &str| file == caller);
    }
    if chosen.is_empty() {
        // A child module calls what its parent defines (`use super::*`).
        let dir = caller
            .rsplit_once('/')
            .map_or("", |(dir, _)| dir)
            .to_owned();
        chosen = pick(&|file: &str| file == format!("{dir}.rs") || file == format!("{dir}/mod.rs"));
    }
    if chosen.is_empty() {
        chosen = pick(&|_: &str| true);
    }
    chosen.iter().all(|(file, body)| {
        if body.contains(".config(") || body.contains(".config_text(") {
            return true;
        }
        // Passes its Route parameter on to a function that adds it.
        let signature = &body[..body.find('{').unwrap_or(body.len())];
        let params: Vec<String> = signature
            .split(['(', ','])
            .filter_map(|param| {
                let (name, kind) = param.split_once(':')?;
                kind.contains("Route")
                    .then(|| name.trim().trim_start_matches("mut ").to_owned())
            })
            .collect();
        depth < 3 && !params.is_empty() && {
            let lits = literals(body);
            body.match_indices('(').any(|(open, _)| {
                let callee: String = body[..open]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                !callee.is_empty()
                    && super::rust_source::containing(&lits, open).is_none()
                    && super::rust_source::call_arguments(body, open)
                        .iter()
                        .any(|argument| {
                            params
                                .iter()
                                .any(|param| !occurrences(argument, param, 0).is_empty())
                        })
                    && adds_config(definitions, &callee, file, depth + 1)
            })
        }
    })
}

/// F8 review R2: every Route for a command that reads the configuration
/// carries `--config`: on its own chain, on a later use of the name it is
/// bound to, inside the function it is passed to, or at every call of the
/// function that returns it. `configured` is read from the CLI dispatcher.
pub(super) fn config_less(
    sources: &[(String, String)],
    configured: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let definitions = functions(sources);
    let configured_at = |path: &str, text: &str, lits: &[Literal], at: usize| {
        if configured_chain(text, lits, at) {
            return true;
        }
        let call = opening(text, lits, at, b'(', b')');
        let qualified: String = text[..call]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        call > 0 && adds_config(&definitions, &qualified, path, 0)
    };
    let calls_configure = |name: &str| {
        let mut sites = 0;
        let mut configured_sites = 0;
        for (path, text) in sources {
            let lits = literals(text);
            for at in occurrences(text, name, 0) {
                if !text[at + name.len()..].starts_with('(') || text[..at].ends_with("fn ") {
                    continue;
                }
                sites += 1;
                if configured_at(path, text, &lits, at) {
                    configured_sites += 1;
                }
            }
        }
        sites > 0 && sites == configured_sites
    };
    let mut failures = Vec::new();
    for (path, text) in sources {
        let lits = literals(text);
        for (found, _) in text.match_indices("Route::new(") {
            let open = found + "Route::new(".len();
            let Some(first) = lits.iter().find(|lit| lit.start >= open) else {
                continue;
            };
            if !matches!(text[open..first.start].trim(), "[" | "&[")
                || !configured.contains(&first.text)
            {
                continue;
            }
            let at = text[..found]
                .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == ':')
                .len();
            if configured_at(path, text, &lits, at) {
                continue;
            }
            let bound: String = text[..at]
                .trim_end()
                .strip_suffix('=')
                .map(|left| {
                    left.trim_end()
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                })
                .unwrap_or_default();
            if !bound.is_empty()
                && occurrences(text, &bound, at)
                    .into_iter()
                    .any(|use_at| configured_at(path, text, &lits, use_at))
            {
                continue;
            }
            let function = text[..at].rfind("fn ").unwrap_or(0);
            let name: String = text[function + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let signature =
                &text[function..text[function..].find('{').map_or(at, |end| function + end)];
            let returned = signature
                .split("->")
                .nth(1)
                .is_some_and(|kind| kind.contains("Route"));
            if returned && calls_configure(&name) {
                continue;
            }
            failures.push(format!(
                "{path}:{}: a `{}` route without --config; the command reads the configuration",
                line_of(text, at),
                first.text
            ));
        }
    }
    failures
}

#[test]
fn every_route_for_a_configured_command_carries_its_configuration() {
    let sources = route_sources();
    let configured = super::cli_tables::configured(&sources);
    for command in ["work", "goal", "run", "memory", "project"] {
        assert!(configured.contains(command), "{configured:?}");
    }
    assert!(!configured.contains("init") && !configured.contains("setup"));
    let failures = config_less(&sources, &configured);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Seeded: a configured command's route left without `--config` fails in
/// any position; one configured on its chain, through its binding, inside
/// the typed function it is passed to, or at every call of the function
/// that returns it passes, and an unconfigured command needs none.
#[test]
fn a_configured_route_without_config_is_a_member() {
    let configured: std::collections::BTreeSet<String> =
        ["run".to_owned(), "work".to_owned()].into();
    let seeded = concat!(
        "fn a(v: &Value) -> String { v.as_str().map(str::to_owned).unwrap_or_else(|| crate::emitted::Route::new([\"run\", \"supersede\"]).shell_text()) }\n",
        "fn b(w: &str, l: &Loaded) -> Value { json!(Route::new([\"work\", \"next\", w]).config(&l.path).full_argv()) }\n",
        "fn c(w: &str, l: &Loaded) -> String { let r = Route::new([\"work\", \"detail\", w]); r.config(&l.path).shell_text() }\n",
        "fn d(l: &Loaded) -> String { render(l, Route::new([\"work\", \"resume\"])) }\n",
        "fn render(l: &Loaded, r: Route) -> String { r.config(&l.path).shell_text() }\n",
        "fn made(w: &str) -> Route { Route::new([\"work\", \"act\", w]) }\n",
        "fn e(l: &Loaded) -> String { made(\"x\").config(&l.path).shell_text() }\n",
        "fn f() -> String { Route::new([\"init\", \"--root\"]).shell_text() }\n",
    );
    let found = config_less(
        &[("src/seeded.rs".to_owned(), seeded.to_owned())],
        &configured,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("src/seeded.rs:1: a `run` route"),
        "{found:?}"
    );
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
    let sources = route_sources();
    let mut typed = sources.clone();
    typed.extend(
        super::rust_source::sources()
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
    let families = vec![
        ("work".to_owned(), vec!["next".to_owned()]),
        ("init".to_owned(), Vec::new()),
    ];
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
