//! Temp-root tripwire: test code takes every temp directory from the one
//! helper of its crate boundary (`tests/support/temp_root.rs` for integration
//! tests, `src/test_root.rs` for `src/` test modules), whose guard removes the
//! directory when the test passes and keeps and names it when the test fails.
//! Any other acquisition in test code is a member and fails the scan.
//!
//! Test code is every `.rs` file under `tests/` (fixtures excluded), every
//! `src/` file declared by a `#[cfg(test)]` module item (and the modules it
//! declares in turn), and the text of a `src/` file from an inline
//! `#[cfg(test)] mod … {` to the end of that file. A member is a line that
//! acquires a temp location by any spelling: `temp_dir` (any path form), an
//! environment read of `TMPDIR`, `TMP` or `TEMP`, `CARGO_TARGET_TMPDIR`, a
//! temp-file crate or helper (`tempfile::`, `tempdir(`, `TempDir::`),
//! `mkdtemp`/`mktemp` (also in embedded shell text), or a `create_dir` of an
//! absolute literal path. Setting `TMPDIR` for a child (`.env("TMPDIR", root)`)
//! points it into the test's own root and is not a member.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The owners allowed to acquire a temp location: the two helpers and this
/// scanner, whose patterns and seeded fixtures spell every member.
const OWNERS: &[&str] = &[
    "tests/support/temp_root.rs",
    "src/test_root.rs",
    "tests/architecture_budget/temp_roots.rs",
];

fn rust_files(root: &Path, dir: &str, found: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return;
    };
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let relative = format!("{dir}/{name}");
        if entry.file_type().unwrap().is_dir() {
            if relative != "tests/fixtures" {
                rust_files(root, &relative, found);
            }
        } else if name.ends_with(".rs") {
            found.insert(relative);
        }
    }
}

/// One `mod` item: its name, whether it is out of line (`mod x;`), whether a
/// `#[cfg(test)]` attribute gates it, its `#[path]`, and its first line.
struct ModItem {
    name: String,
    out_of_line: bool,
    test_only: bool,
    path: Option<String>,
    line: usize,
}

fn mod_items(text: &str) -> Vec<ModItem> {
    let mut items = Vec::new();
    let mut attributes: Vec<(usize, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            attributes.push((index, trimmed.replace(' ', "")));
            continue;
        }
        let item = trimmed
            .strip_prefix("pub(crate) ")
            .or_else(|| trimmed.strip_prefix("pub(super) "))
            .or_else(|| trimmed.strip_prefix("pub "))
            .unwrap_or(trimmed);
        if let Some(rest) = item.strip_prefix("mod ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let after = rest[name.len()..].trim_start();
            if !name.is_empty() && (after.starts_with(';') || after.starts_with('{')) {
                items.push(ModItem {
                    name,
                    out_of_line: after.starts_with(';'),
                    test_only: attributes.iter().any(|(_, attribute)| {
                        attribute.starts_with("#[cfg(test)")
                            || attribute.starts_with("#[cfg(all(test")
                    }),
                    path: attributes.iter().find_map(|(_, attribute)| {
                        let value = attribute.strip_prefix("#[path=\"")?;
                        Some(value[..value.find('"')?].to_owned())
                    }),
                    line: attributes.first().map_or(index, |(first, _)| *first),
                });
            }
        }
        if !trimmed.is_empty() && !trimmed.starts_with("//") {
            attributes.clear();
        }
    }
    items
}

/// The file an out-of-line `mod` item of `file` declares.
fn declared_file(root: &Path, file: &str, item: &ModItem) -> Option<String> {
    let parent = file.rsplit_once('/').map_or("", |(parent, _)| parent);
    if let Some(path) = &item.path {
        return Some(format!("{parent}/{path}"));
    }
    let stem = file.trim_end_matches(".rs");
    let own_dir = match stem.rsplit_once('/') {
        Some((dir, "mod" | "main" | "lib")) => dir.to_owned(),
        _ => stem.to_owned(),
    };
    [
        format!("{own_dir}/{}.rs", item.name),
        format!("{own_dir}/{}/mod.rs", item.name),
    ]
    .into_iter()
    .find(|candidate| root.join(candidate).is_file())
}

/// Every test-code file with the line its test code starts at (0 = whole file).
fn test_code(root: &Path) -> BTreeMap<String, usize> {
    let mut files = BTreeSet::new();
    rust_files(root, "src", &mut files);
    rust_files(root, "tests", &mut files);
    let items: BTreeMap<String, Vec<ModItem>> = files
        .iter()
        .map(|file| {
            let text = fs::read_to_string(root.join(file)).unwrap();
            (file.clone(), mod_items(&text))
        })
        .collect();
    let mut whole: BTreeSet<String> = files
        .iter()
        .filter(|file| file.starts_with("tests/"))
        .cloned()
        .collect();
    loop {
        let mut added = false;
        for (file, declared) in &items {
            for item in declared.iter().filter(|item| item.out_of_line) {
                if item.test_only || whole.contains(file) {
                    if let Some(child) = declared_file(root, file, item) {
                        added |= files.contains(&child) && whole.insert(child);
                    }
                }
            }
        }
        if !added {
            break;
        }
    }
    let mut code: BTreeMap<String, usize> = whole.into_iter().map(|file| (file, 0)).collect();
    for (file, declared) in &items {
        if code.contains_key(file) {
            continue;
        }
        if let Some(item) = declared
            .iter()
            .find(|item| item.test_only && !item.out_of_line)
        {
            code.insert(file.clone(), item.line);
        }
    }
    code
}

/// Whether one line of test code acquires a temp location.
fn acquires_temp(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    let environment_read = ["TMPDIR", "TMP", "TEMP"].iter().any(|name| {
        compact.contains(&format!("var(\"{name}\""))
            || compact.contains(&format!("var_os(\"{name}\""))
    });
    let mktemp = line
        .match_indices("mktemp")
        .any(|(at, _)| !line[at + "mktemp".len()..].starts_with('"'));
    compact.contains("temp_dir")
        || environment_read
        || compact.contains("CARGO_TARGET_TMPDIR")
        || compact.contains("tempfile::")
        || compact.contains("tempdir(")
        || compact.contains("TempDir::")
        || compact.contains("mkdtemp")
        || mktemp
        || compact.contains("create_dir(\"/")
        || compact.contains("create_dir_all(\"/")
}

/// Every member: `path:line: text` of a temp acquisition in test code outside
/// the owners.
pub fn members(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for (file, start) in test_code(root) {
        if OWNERS.contains(&file.as_str()) {
            continue;
        }
        let text = fs::read_to_string(root.join(&file)).unwrap();
        for (index, line) in text.lines().enumerate().skip(start) {
            if !line.trim_start().starts_with("//") && acquires_temp(line) {
                found.push(format!("{file}:{}: {}", index + 1, line.trim()));
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
/// declarations, `#[path]` and nesting.
#[test]
fn a_seeded_temp_acquisition_in_test_code_is_a_member() {
    let root = super::temp_root::temp("temp-root-scan");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
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
    write(
        "tests/fixtures/ignored.rs",
        "fn t() { std::env::temp_dir(); }\n",
    );
    let found = members(&root);
    let expected = [
        "src/main.rs:10:",
        "src/production/tests.rs:1:",
        "src/unit_checks/nested.rs:1:",
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
