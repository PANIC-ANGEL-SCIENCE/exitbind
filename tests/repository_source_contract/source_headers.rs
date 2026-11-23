//! Opening navigation contracts for the existing complete source inventory.
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn source_inventory_names_a_real_navigation_target_in_the_first_eight_lines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = super::rust_source::sources();
    assert!(!sources.is_empty());
    for (path, text) in &sources {
        let header = text.lines().take(8).collect::<Vec<_>>().join("\n");
        let comments = header
            .lines()
            .filter(|line| line.trim_start().starts_with("//"));
        assert!(
            comments.count() >= 2,
            "{path}: missing responsibility and limit header"
        );
        let mut found = false;
        for part in header.split('`').skip(1).step_by(2) {
            if part.starts_with("src/") && part.ends_with(".rs") {
                assert!(
                    root.join(part).is_file(),
                    "{path}: absent navigation target {part}"
                );
                found = true;
            }
        }
        assert!(
            found,
            "{path}: no source navigation target in first eight lines"
        );
    }
    println!(
        "source inventory: {} Rust files, including test-gated files",
        sources.len()
    );
}

#[test]
fn permit_response_wording_trace_opens_two_relevant_source_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut opened = BTreeSet::new();
    let mut read = |path: &str| {
        opened.insert(path.to_owned());
        std::fs::read_to_string(root.join(path)).unwrap()
    };
    let cli = read("src/cli/work.rs");
    let header = cli.lines().take(8).collect::<Vec<_>>().join("\n");
    let links = header
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|v| v.starts_with("src/"))
        .collect::<Vec<_>>();
    assert_eq!(links, ["src/work/permit_response.rs"]);
    let owner = read(links[0]);
    let owner_header = owner.lines().take(8).collect::<Vec<_>>().join("\n");
    assert!(owner_header.contains("permit result") && owner_header.contains("no host permission"));
    assert_eq!(opened.len(), 2);
    println!("S0 permit response wording source trace: {opened:?}; limit 3");
}

#[test]
fn source_inventory_observes_changed_fixture_contents_between_scans() {
    let root = super::support::temp("source-inventory-changing-fixture");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "mod feature;\n").unwrap();
    std::fs::write(root.join("src/feature.rs"), "fn first() {}\n").unwrap();

    let first = super::rust_source::production_in(&root);
    assert!(first
        .iter()
        .any(|(path, text)| path == "src/feature.rs" && text.contains("first")));

    std::fs::write(root.join("src/feature.rs"), "fn other() {}\n").unwrap();
    let changed = super::rust_source::production_in(&root);
    assert!(changed
        .iter()
        .any(|(path, text)| path == "src/feature.rs" && text.contains("other")));

    std::fs::write(root.join("src/main.rs"), "#[cfg(test)]\nmod feature;\n").unwrap();
    let test_only = super::rust_source::production_in(&root);
    assert!(!test_only.iter().any(|(path, _)| path == "src/feature.rs"));
}
