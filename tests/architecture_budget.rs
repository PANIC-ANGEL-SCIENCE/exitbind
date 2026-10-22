//! Structural tripwire: every file under the covered roots stays under
//! 32 KiB, production sources, test modules, integration tests and fixtures
//! alike. A file over the limit before this rule covered it keeps a byte
//! ceiling in `tests/fixtures/architecture-budget.txt` that equals its current
//! size, so the list only falls: growth fails, a shrink lowers the ceiling,
//! and a file back under the limit leaves the list. A feature that needs room
//! in a listed file extracts the responsibility it changes first.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const LIMIT: u64 = 32 * 1024;

/// The covered roots. Every file under each counts, whatever its extension.
const ROOTS: &[&str] = &["src", "tests", "tests/fixtures"];

const CEILINGS: &str = "tests/fixtures/architecture-budget.txt";

fn walk(root: &Path, dir: &Path, found: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(root, &path, found);
        } else {
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
            found.insert(relative.replace('\\', "/"));
        }
    }
}

/// Every file under the covered roots of `root`, relative to it.
fn covered(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for covered_root in ROOTS {
        let dir = root.join(covered_root);
        if dir.is_dir() {
            walk(root, &dir, &mut found);
        }
    }
    found
}

fn ceilings(text: &str) -> BTreeMap<String, u64> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (bytes, path) = line.split_once(' ').unwrap();
            (path.to_owned(), bytes.parse::<u64>().unwrap())
        })
        .collect()
}

/// Every budget failure of the tree at `root` against `ceilings`.
fn failures(root: &Path, ceilings: &BTreeMap<String, u64>) -> Vec<String> {
    let found = covered(root);
    let mut failures = Vec::new();
    for path in &found {
        let size = fs::metadata(root.join(path)).unwrap().len();
        match ceilings.get(path) {
            Some(ceiling) if size > *ceiling => failures.push(format!(
                "{path} grew to {size} bytes over its ceiling {ceiling}; extract the changed responsibility"
            )),
            Some(_) if size <= LIMIT => {}
            Some(ceiling) if size < *ceiling => failures.push(format!(
                "{path} fell to {size} bytes; lower its ceiling from {ceiling} to {size} in {CEILINGS}"
            )),
            None if size > LIMIT => failures.push(format!(
                "{path} is {size} bytes, over {LIMIT}; divide it by responsibility"
            )),
            _ => {}
        }
    }
    for path in ceilings.keys() {
        if !found.contains(path) {
            failures.push(format!("{path} is listed but no longer exists"));
        } else if fs::metadata(root.join(path)).unwrap().len() <= LIMIT {
            failures.push(format!("{path} is under {LIMIT} bytes; remove its ceiling"));
        }
    }
    failures
}

#[test]
fn every_covered_file_stays_within_its_ceiling() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let listed = ceilings(&fs::read_to_string(root.join(CEILINGS)).unwrap());
    let failures = failures(root, &listed);
    assert!(
        failures.is_empty(),
        "covered roots {ROOTS:?}:\n{}",
        failures.join("\n")
    );
}

/// The budget fails on a 33 KiB file seeded under each test root, on a
/// listed file that grew, and on a ceiling left above a file that fell.
#[test]
fn a_seeded_33_kib_test_file_fails_the_budget() {
    let root = std::env::temp_dir().join(format!(
        "exitbind-architecture-budget-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests/fixtures/nested")).unwrap();
    let seeded = vec![b'x'; 33 * 1024];
    fs::write(root.join("src/small.rs"), b"fn main() {}\n").unwrap();
    fs::write(root.join("src/large_tests.rs"), &seeded).unwrap();
    fs::write(root.join("tests/seeded.rs"), &seeded).unwrap();
    fs::write(root.join("tests/fixtures/nested/seeded.tsv"), &seeded).unwrap();
    let found = failures(&root, &BTreeMap::new());
    for path in [
        "src/large_tests.rs",
        "tests/seeded.rs",
        "tests/fixtures/nested/seeded.tsv",
    ] {
        assert!(
            found
                .iter()
                .any(|failure| failure.starts_with(&format!("{path} is 33792 bytes"))),
            "{path}: {found:?}"
        );
    }
    assert_eq!(found.len(), 3, "{found:?}");

    let size = seeded.len() as u64;
    let listed = |ceiling: u64| {
        [
            ("src/large_tests.rs".to_owned(), size),
            ("tests/seeded.rs".to_owned(), ceiling),
            ("tests/fixtures/nested/seeded.tsv".to_owned(), size),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>()
    };
    assert!(failures(&root, &listed(size)).is_empty());
    let grown = failures(&root, &listed(size - 1));
    assert!(grown[0].starts_with("tests/seeded.rs grew"), "{grown:?}");
    let raised = failures(&root, &listed(size + 1));
    assert!(raised[0].starts_with("tests/seeded.rs fell"), "{raised:?}");
    fs::remove_dir_all(&root).unwrap();
}

/// Enumerated another way: every Git-tracked file over the limit under a
/// covered root is on the ceiling list, and the budget's walk visits every
/// tracked file there.
#[test]
fn git_tracked_files_agree_with_the_covered_walk_and_ceiling_list() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // raw-spawn: a read-only `git ls-files` listing; no exitbind process or session state.
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--"])
        .args(ROOTS)
        .output()
        .expect("git runs in the checkout");
    assert!(output.status.success(), "{output:?}");
    let tracked: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8(part.to_vec()).unwrap())
        .filter(|path| root.join(path).is_file())
        .collect();
    assert!(tracked
        .iter()
        .any(|path| path.starts_with("tests/fixtures/")));
    let walked = covered(root);
    let listed = ceilings(&fs::read_to_string(root.join(CEILINGS)).unwrap());
    for path in &tracked {
        assert!(walked.contains(path), "the budget walk misses {path}");
        if fs::metadata(root.join(path)).unwrap().len() > LIMIT {
            assert!(
                listed.contains_key(path),
                "{path} is over the limit and unlisted"
            );
        }
    }
}
