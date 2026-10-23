//! Temp-root tripwire: test code takes every temp directory from the one
//! helper of its crate boundary (`tests/support/temp_root.rs` for integration
//! tests, `src/test_root.rs` for `src/` test modules), whose guard removes the
//! directory when the test passes and keeps and names it when the test fails.
//! Any other acquisition in test code is a member and fails the scan.
//!
//! Test code, read with the shared scanner (`tests/inventory/rust_source.rs`):
//! every `.rs` file under `tests/` (fixtures excluded), every `src/` file a
//! `cfg(test)`, `cfg(all(test, ..))` or `cfg(any(test, ..))` module item
//! declares (and the modules it declares in turn), and each such inline item
//! of any other `src/` file by its own extent. A member is a line that
//! acquires a temp location by any spelling, also across lines: the
//! helpers `temp_dir`, `mkdtemp`, `tempfile::`, `tempdir(` or `TempDir::`; a
//! read of `TMPDIR`, `TMP`, `TEMP` or `CARGO_TARGET_TMPDIR` (`var`,
//! `var_os`, `env!`), also through a constant or binding; shell text running
//! `mktemp` or reading `$TMPDIR`, `$TMP` or `$TEMP`; a `/tmp` path created
//! where it is built or through its binding
//! (`create_dir_all(format!("/tmp/{x}"))`, `let p = Path::new("/tmp")` then
//! `create_dir_all(p.join(..))`); and any absolute literal created as a
//! directory. Setting a variable for a child and an inert `/tmp` string are
//! not members.

use super::rust_source::{blank, gated_items, line_of, literals, sources_in, test_files};
use std::path::Path;

/// The owners allowed to acquire a temp location: the two helpers and this
/// scanner, whose patterns and seeded fixtures spell every member.
const OWNERS: &[&str] = &[
    "tests/support/temp_root.rs",
    "src/test_root.rs",
    "tests/architecture_budget/temp_roots.rs",
];

/// Identifiers that acquire a temp location, with what must follow them
/// (`TempDir::new`, not a type alias that happens to be named `TempDir`).
const IDENTIFIERS: &[(&str, &str)] = &[
    ("temp_dir", ""),
    ("mkdtemp", ""),
    ("tempfile", "::"),
    ("tempdir", "("),
    ("TempDir", "::"),
];
/// Environment variables that name the temp directory.
const VARIABLES: &[&str] = &["TMPDIR", "TMP", "TEMP", "CARGO_TARGET_TMPDIR"];
/// Calls that read an environment variable.
const READS: &[&str] = &["var", "var_os", "env!", "option_env!"];
/// Calls that create a file or a directory.
const CREATES: &[&str] = &[
    "create_dir(",
    "create_dir_all(",
    "File::create(",
    "fs::write(",
    ".open(",
    "set_current_dir(",
];

/// One file of test code: path, text and the byte ranges that are test code.
type TestCode = (String, String, Vec<(usize, usize)>);

/// Every test-code file of `root` with the byte ranges of its test code.
fn test_code(root: &Path) -> Vec<TestCode> {
    let test = test_files(root);
    let mut code = Vec::new();
    for (file, text) in sources_in(root, "src")
        .into_iter()
        .chain(sources_in(root, "tests"))
    {
        let ranges = if test.contains_key(&file) {
            vec![(0, text.len())]
        } else {
            gated_items(&text)
                .into_iter()
                .map(|item| (item.start, item.end))
                .collect()
        };
        if !ranges.is_empty() {
            code.push((file, text, ranges));
        }
    }
    code
}

/// The name of the call whose argument list holds byte `at` of blanked
/// `code`, with a macro's `!` kept (`format!`).
fn enclosing_call(code: &str, at: usize) -> String {
    let mut depth = 0;
    let bytes = code.as_bytes();
    let mut i = at;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' | b']' => depth += 1,
            b'[' if depth > 0 => depth -= 1,
            b'(' if depth > 0 => depth -= 1,
            b'(' => {
                return code[..i]
                    .trim_end()
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '!')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
            }
            _ => {}
        }
    }
    String::new()
}

/// The statement of blanked `code` that holds byte `at`.
fn statement(code: &str, at: usize) -> &str {
    let start = code[..at]
        .rfind([';', '{', '}'])
        .map_or(0, |found| found + 1);
    let end = code[at..].find(';').map_or(code.len(), |found| at + found);
    &code[start..end]
}

/// The name a `let NAME =`, `const NAME: T =` or `static NAME: T =`
/// statement binds the value at `at` to.
fn bound_name(code: &str, at: usize) -> Option<String> {
    let statement = statement(code, at).trim_start();
    let rest = [
        "let mut ",
        "let ",
        "const ",
        "static ",
        "pub const ",
        "pub(crate) const ",
    ]
    .iter()
    .find_map(|keyword| statement.strip_prefix(keyword))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Whole-word uses of `name` in blanked `code` after byte `from`.
fn uses(code: &str, name: &str, from: usize) -> Vec<usize> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code[from..]
        .match_indices(name)
        .map(|(at, _)| from + at)
        .filter(|at| !code[..*at].ends_with(ident) && !code[at + name.len()..].starts_with(ident))
        .collect()
}

/// Byte offsets in `text` where a temp location is acquired.
fn acquisitions(text: &str) -> Vec<usize> {
    let code = blank(text);
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let creates = |at: usize| {
        let statement = statement(&code, at);
        CREATES.iter().any(|call| statement.contains(call))
    };
    let mut found = Vec::new();
    for (name, follows) in IDENTIFIERS {
        for (at, _) in code.match_indices(name) {
            let after = &code[at + name.len()..];
            if !code[..at].ends_with(ident)
                && !after.starts_with(ident)
                && after.trim_start().starts_with(follows)
            {
                found.push(at);
            }
        }
    }
    for lit in literals(text) {
        let call = enclosing_call(&code, lit.start);
        let bound = bound_name(&code, lit.start);
        // A read of the variable, also through a constant or binding.
        let variable = VARIABLES.contains(&lit.text.as_str())
            && (READS.contains(&call.as_str())
                || bound.as_deref().is_some_and(|name| {
                    uses(&code, name, lit.end)
                        .into_iter()
                        .any(|at| READS.contains(&enclosing_call(&code, at).as_str()))
                }));
        let shell = lit.text.match_indices("mktemp").any(|(at, _)| {
            !(lit.text == "mktemp" || lit.text[at + "mktemp".len()..].starts_with('"'))
        }) || VARIABLES.iter().any(|name| {
            [format!("${name}"), format!("${{{name}")]
                .iter()
                .any(|spelled| {
                    lit.text.match_indices(spelled.as_str()).any(|(at, _)| {
                        !lit.text[at + spelled.len()..]
                            .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                    })
                })
        });
        // A /tmp path created where it is built or through its binding;
        // any absolute directory created from a literal.
        let temp_path = (lit.text.starts_with("/tmp") || lit.text.starts_with("/var/tmp"))
            && (creates(lit.start)
                || bound
                    .as_deref()
                    .is_some_and(|name| uses(&code, name, lit.end).into_iter().any(creates)));
        let absolute = lit.text.starts_with('/') && call.starts_with("create_dir");
        if variable || shell || temp_path || absolute {
            found.push(lit.start);
        }
    }
    found.sort_unstable();
    found
}

/// Every member: `path:line: text` of a temp acquisition in test code outside
/// the owners, one per line.
pub fn members(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text, ranges) in test_code(root) {
        if OWNERS.contains(&file.as_str()) {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        let mut seen = Vec::new();
        for at in acquisitions(&text) {
            if !ranges.iter().any(|(start, end)| *start <= at && at < *end) {
                continue;
            }
            let line = line_of(&text, at);
            if !seen.contains(&line) {
                seen.push(line);
                found.push(format!("{file}:{line}: {}", lines[line - 1].trim()));
            }
        }
    }
    found
}

#[test]
fn test_code_takes_every_temp_root_from_its_helper() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let found = members(root);
    eprintln!("temp-root scan members: {}", found.len());
    assert!(
        found.is_empty(),
        "{} temp acquisition(s) in test code outside {OWNERS:?}; take the directory from \
         support::temp (integration tests) or crate::test_root::temp (src test modules):\n{}",
        found.len(),
        found.join("\n")
    );
}

/// Seeded tree: each spelling in test code is a member, production code and
/// the helpers are not, and test-only files are found through `#[cfg(test)]`
/// declarations, `#[path]` and nesting; a test module ends at its own brace
/// even when a literal holds one.
#[test]
fn a_seeded_temp_acquisition_in_test_code_is_a_member() {
    let root = super::temp_root::temp("temp-root-scan");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "src/main.rs",
        "mod production;\n#[cfg(test)]\nmod test_root;\n#[cfg(test)]\n#[path = \"unit_checks.rs\"]\nmod unit;\nfn main() { let _ = std::env::temp_dir(); }\n#[cfg(test)]\nmod tests {\n    fn t() { let _ = std::env::temp_dir(); }\n}\n",
    );
    write(
        "src/production.rs",
        "pub fn p() -> std::path::PathBuf { std::env::temp_dir() }\n#[cfg(test)]\nmod tests;\n",
    );
    write(
        "src/production/tests.rs",
        "fn t() { let _ = std::env::var_os(\"TMPDIR\"); }\n",
    );
    write(
        "src/unit_checks.rs",
        "mod nested;\n// std::env::temp_dir() in a comment is not a member\n",
    );
    write("src/unit_checks/nested.rs", "use std::env::temp_dir;\n");
    write("src/test_root.rs", "fn t() { std::env::temp_dir(); }\n");
    write(
        "tests/support/temp_root.rs",
        "fn t() { std::env::temp_dir(); }\n",
    );
    write(
        "tests/seeded.rs",
        concat!(
            "fn a() { let _ = tempfile::tempdir(); }\n",
            "fn b() { std::fs::create_dir_all(\"/seeded\").unwrap(); }\n",
            "const SCRIPT: &str = \"scratch=$(mktemp -d)\";\n",
            "fn c() { let _ = env!(\"CARGO_TARGET_TMPDIR\"); }\n",
            "fn d() { let _ = std::env::var(\"TEMP\"); }\n",
            "fn e(c: &mut std::process::Command, r: &std::path::Path) { c.env(\"TMPDIR\", r); }\n",
            "const TOOLS: &[&str] = &[\"mktemp\", \"tar\"];\n",
        ),
    );
    // F9 review D3: a read through a constant, shell text without mktemp, a
    // formatted or bound /tmp path, a call split across lines, and a
    // `cfg(any(test, ..))` module after production code with brace literals.
    write(
        "tests/blind_spots.rs",
        concat!(
            "const TMP_NAME: &str = \"TMPDIR\";\n",
            "fn a() { let _ = std::env::var(TMP_NAME); }\n",
            "const SH: &str = \"cd ${TMPDIR:-/tmp} && ls $TMP/x\";\n",
            "fn b(x: &str) { std::fs::create_dir_all(format!(\"/tmp/{x}\")).unwrap(); }\n",
            "fn c() { let p = std::path::Path::new(\"/tmp\"); std::fs::create_dir_all(p.join(\"x\")).unwrap(); }\n",
            "fn d() {\n    let _ = std::env::\n        temp_dir();\n}\n",
            "fn e() {\n    let _ = std::env::var(\n        \"TMP\",\n    );\n}\n",
            "fn f(c: &mut std::process::Command) { c.env_remove(\"TMPDIR\").args([\"--root\", \"/tmp/x\"]); }\n",
            "fn g() { let inert = std::path::Path::new(\"/tmp/inert\"); let _ = inert.display(); }\n",
            "fn h(name: &str) { run(\"TMPDIR\", name); for n in [\"TMPDIR\", \"HOME\"] { let _ = n; } }\n",
            "type TempDir = Root;\n",
        ),
    );
    write(
        "src/helpers.rs",
        concat!(
            "#[cfg(any(test, feature = \"fixtures\"))]\n",
            "pub mod fixtures {\n",
            "    const BRACE: char = '}';\n",
            "    pub fn dir() -> std::path::PathBuf { std::env::temp_dir() }\n",
            "}\n",
            "pub fn production() -> std::path::PathBuf { std::env::temp_dir() }\n",
        ),
    );
    write(
        "tests/fixtures/ignored.rs",
        "fn t() { std::env::temp_dir(); }\n",
    );
    let found = members(&root);
    let expected = [
        "src/helpers.rs:4:",
        "src/main.rs:10:",
        "src/production/tests.rs:1:",
        "src/unit_checks/nested.rs:1:",
        "tests/blind_spots.rs:1:",
        "tests/blind_spots.rs:3:",
        "tests/blind_spots.rs:4:",
        "tests/blind_spots.rs:5:",
        "tests/blind_spots.rs:8:",
        "tests/blind_spots.rs:12:",
        "tests/seeded.rs:1:",
        "tests/seeded.rs:2:",
        "tests/seeded.rs:3:",
        "tests/seeded.rs:4:",
        "tests/seeded.rs:5:",
    ];
    assert_eq!(found.len(), expected.len(), "{found:#?}");
    for (member, prefix) in found.iter().zip(expected) {
        assert!(member.starts_with(prefix), "{prefix}: {found:#?}");
    }
}
