//! R55 F2a (owner default D13): every call of the one configuration writer
//! keeps the stamped bytes it replaces when a memory ledger carries their
//! hash. The calls are read from the source, not listed by hand: each one
//! must reach `settings::atomic_write`, the only definition, and that writer
//! keeps the replaced bytes before anything is replaced. The class is the
//! mechanism, not the spelling: any other production file operation whose
//! destination is the configuration path fails the test.
use super::root_inputs::sources;

const WRITER: &str = "src/host/settings.rs";
const KEEP: &str = "crate::memory::stamped_config::keep_replaced(target, replaced)?";

/// File operations that create or replace a file, with the index of the
/// argument that names the destination.
const FILE_WRITES: &[(&str, usize)] = &[
    ("fs::rename(", 1),
    ("fs::copy(", 1),
    ("fs::write(", 0),
    ("fs::hard_link(", 1),
    ("write_exclusive(", 0),
    ("write_temp(", 0),
    ("File::create(", 0),
    (".open(", 0),
];
/// The names production code gives the configuration path.
const CONFIGURATION_PATHS: &[&str] = &[
    "loaded.path",
    "l.path",
    "config_path",
    "config",
    "configuration",
    "configuration_path",
];
/// A configuration written where none existed: no stamped bytes to keep.
const CREATIONS: &[(&str, &str)] = &[(
    "src/project/onboarding.rs",
    "write_exclusive(&config", // `init` creates the configuration
)];

struct Call {
    file: String,
    line: usize,
    callee: String,
    arguments: Vec<String>,
}

/// Top-level comma-separated arguments of the call whose `(` is at `open`.
fn arguments(text: &str, open: usize) -> Vec<String> {
    let mut depth = 0;
    let mut current = String::new();
    let mut out = Vec::new();
    for c in text[open..].chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => {
                out.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

/// Every occurrence of `atomic_write` in production source: definitions
/// (callee `fn`) and calls with their qualified path and arguments.
fn calls() -> Vec<Call> {
    let mut found = Vec::new();
    for (file, text) in sources() {
        for (index, _) in text.match_indices("atomic_write") {
            let before = &text[..index];
            let start = before
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
                .map_or(0, |at| at + 1);
            let after = &text[index + "atomic_write".len()..];
            let line = before.matches('\n').count() + 1;
            let line_start = before.rfind('\n').map_or(0, |at| at + 1);
            if text[line_start..].trim_start().starts_with("//") {
                continue; // a comment naming the writer
            }
            if before.ends_with("fn ") {
                found.push(Call {
                    file: file.clone(),
                    line,
                    callee: "fn".into(),
                    arguments: Vec::new(),
                });
            } else if after.starts_with('(') {
                found.push(Call {
                    file: file.clone(),
                    line,
                    callee: text[start..index + "atomic_write".len()].to_owned(),
                    arguments: arguments(&text, index + "atomic_write".len()),
                });
            } else {
                panic!("{file}:{line}: `atomic_write` used other than as a call (alias or pointer): the one writer must be called by name");
            }
        }
    }
    found
}

/// Every production file write outside the one writer whose destination is
/// the configuration path, as `file: operation destination`.
fn configuration_writes_outside_the_writer() -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in sources() {
        if file == WRITER {
            continue;
        }
        for (operation, destination) in FILE_WRITES {
            for (index, _) in text.match_indices(operation) {
                let line_start = text[..index].rfind('\n').map_or(0, |at| at + 1);
                if text[line_start..].trim_start().starts_with("//") {
                    continue;
                }
                // An `OpenOptions` chain writes only with a write flag.
                if *operation == ".open(" {
                    let chain = &text[text[..index].rfind("OpenOptions").unwrap_or(index)..index];
                    let writes = [
                        ".write(true)",
                        ".append(true)",
                        ".create(true)",
                        ".create_new(true)",
                    ]
                    .iter()
                    .any(|flag| chain.contains(flag));
                    if !writes {
                        continue;
                    }
                }
                let open = index + operation.len() - 1;
                let Some(target) = arguments(&text, open).get(*destination).cloned() else {
                    continue;
                };
                let target = target.trim_start_matches('&').trim();
                let named = CONFIGURATION_PATHS
                    .iter()
                    .any(|name| target == *name || target.ends_with(&format!(".{name}")));
                let created = CREATIONS
                    .iter()
                    .any(|(owner, call)| file == *owner && text[index..].starts_with(call));
                if named && !created {
                    found.push(format!("{file}: {operation}{target}"));
                }
            }
        }
    }
    found
}

#[test]
fn the_configuration_file_is_replaced_only_by_the_one_writer() {
    let outside = configuration_writes_outside_the_writer();
    assert!(
        outside.is_empty(),
        "replace the configuration through settings::atomic_write, which keeps the stamped bytes: {outside:#?}"
    );
}

#[test]
fn every_configuration_writer_call_keeps_the_stamped_bytes_it_replaces() {
    let all = calls();
    let definitions = all
        .iter()
        .filter(|call| call.callee == "fn")
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 1, "one configuration writer");
    assert_eq!(definitions[0].file, WRITER);
    let calls = all
        .iter()
        .filter(|call| call.callee != "fn")
        .collect::<Vec<_>>();
    let mut table = String::new();
    let mut configuration = 0;
    for call in &calls {
        let qualified = matches!(
            call.callee.as_str(),
            "settings::atomic_write" | "crate::host::settings::atomic_write"
        ) || (call.file == WRITER && call.callee == "atomic_write");
        assert!(
            qualified,
            "{}:{} calls {} — not the one writer",
            call.file, call.line, call.callee
        );
        assert_eq!(
            call.arguments.len(),
            5,
            "{}:{} {:?}",
            call.file,
            call.line,
            call.arguments
        );
        // A call that replaces this project's configuration hands the writer
        // the bytes it replaces, which is what the keep reads.
        let replaces_configuration = call.arguments[0] == "&loaded.path";
        if replaces_configuration {
            configuration += 1;
            assert_eq!(
                call.arguments[3], "Some(&loaded.source)",
                "{}:{} replaces the configuration without naming the replaced bytes",
                call.file, call.line
            );
        }
        table.push_str(&format!(
            "{}:{} {} target={} replaced={}{}\n",
            call.file,
            call.line,
            call.callee,
            call.arguments[0],
            call.arguments[3],
            if replaces_configuration {
                " [configuration]"
            } else {
                ""
            }
        ));
    }
    eprintln!("configuration writer calls: {}\n{table}", calls.len());
    assert!(!calls.is_empty() && configuration > 0, "{table}");

    // The writer keeps before it creates the temporary or replaces anything,
    // and after it has checked that the target holds exactly those bytes.
    let (_, writer) = sources()
        .into_iter()
        .find(|(file, _)| file == WRITER)
        .unwrap();
    let body_start = writer.find("fn atomic_write(").unwrap();
    let body = &writer[body_start..];
    let body = &body[..body.find("\n}\n").unwrap()];
    let checked = body
        .find("assert_unchanged(target, expected_source)?")
        .unwrap();
    let keep = body
        .find(KEEP)
        .unwrap_or_else(|| panic!("the one writer keeps the replaced bytes: {KEEP}"));
    let temporary = body.find("create_new(true)").unwrap();
    let rename = body.find("fs::rename(").unwrap();
    assert!(
        checked < keep && keep < temporary && keep < rename,
        "{body}"
    );
    assert!(
        body[..keep].contains("if let Some(replaced) = expected_source"),
        "every replacement with known bytes reaches the keep: {body}"
    );
}
