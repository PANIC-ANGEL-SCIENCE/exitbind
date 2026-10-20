//! The README-only CI path runs a fixed list of existing tests with `--exact`.
//! A renamed or removed test would make that path run nothing and fail only on
//! the next README-only change, so every listed test must exist where it is named.

use super::*;

#[test]
fn readme_only_gate_list_names_existing_tests_and_matches_its_count() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script =
        fs::read_to_string(root.join("tooling/repository/ci/readme/check-existing.sh")).unwrap();
    let list = script
        .split_once("<<'TESTS'\n")
        .and_then(|(_, rest)| rest.split_once("\nTESTS\n"))
        .unwrap()
        .0;
    let mut gates = 0;
    for line in list.lines() {
        let (target, name) = line.split_once(' ').unwrap();
        let mut path = name.split("::").collect::<Vec<_>>();
        let test = path.pop().unwrap();
        // The test lives in the module file named by its last module segment,
        // or in the target root for a top-level test.
        let file = match path.last() {
            Some(module) => format!("{module}.rs"),
            None => format!("{target}.rs"),
        };
        let source = rust_files(&root.join("tests"))
            .into_iter()
            .find(|candidate| {
                candidate.file_name().unwrap().to_str() == Some(file.as_str())
                    && fs::read_to_string(candidate)
                        .unwrap()
                        .contains(&format!("fn {test}()"))
            })
            .unwrap_or_else(|| panic!("{line}: no test {test} in any tests/**/{file}"));
        assert!(source.is_file());
        gates += 1;
    }
    let readme = fs::read_to_string(root.join("tooling/repository/ci/readme/README.md")).unwrap();
    assert!(
        readme.contains(&format!("existing {gates} README-related Rust gates")),
        "tooling/repository/ci/readme/README.md must state {gates} gates"
    );
}

fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}
