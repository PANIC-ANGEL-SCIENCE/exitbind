//! Structural tripwire: a grandfathered large production source may not grow,
//! and any other production source stays under 32 KiB.  A feature that needs
//! room in a listed file extracts the responsibility it changes first.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const LIMIT: u64 = 32 * 1024;

fn sources(dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        // Test modules kept beside their production module are not
        // production responsibilities.
        if name.ends_with(".rs") && !name.ends_with("_tests.rs") && name != "tests.rs" {
            found.push(path.to_str().unwrap().to_owned());
        }
    }
}

#[test]
fn production_sources_stay_within_their_ceilings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = fs::read_to_string(root.join("tests/fixtures/architecture-budget.txt")).unwrap();
    let ceilings = baseline
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (bytes, path) = line.split_once(' ').unwrap();
            (path.to_owned(), bytes.parse::<u64>().unwrap())
        })
        .collect::<BTreeMap<_, _>>();
    let mut found = Vec::new();
    std::env::set_current_dir(root).unwrap();
    sources(Path::new("src"), &mut found);
    let mut failures = Vec::new();
    for path in &found {
        let size = fs::metadata(path).unwrap().len();
        match ceilings.get(path) {
            Some(ceiling) if size > *ceiling => failures.push(format!(
                "{path} grew to {size} bytes over its ceiling {ceiling}; extract the changed responsibility"
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
        } else if fs::metadata(path).unwrap().len() <= LIMIT {
            failures.push(format!("{path} is under {LIMIT} bytes; remove its ceiling"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
