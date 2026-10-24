//! Every version mention in tracked documentation is inventoried, and one tool
//! moves all `current` mentions with the package version while `pinned`
//! mentions stay as they are.

use super::*;

const INVENTORY: &str = "tooling/repository/release/doc-versions.json";
const TOOL: &str = "tooling/repository/release/sync-doc-versions.py";
const BUMPED: &str = "9.8.7";

fn inventory() -> Value {
    serde_json::from_str(include_str!(
        "../../tooling/repository/release/doc-versions.json"
    ))
    .unwrap()
}

/// (file, rule) pairs for every inventoried document and source.
fn rules(inventory: &Value) -> Vec<(String, Value)> {
    ["documents", "sources"]
        .into_iter()
        .flat_map(|group| inventory[group].as_object().unwrap().clone())
        .flat_map(|(file, entries)| {
            entries
                .as_array()
                .unwrap()
                .iter()
                .map(|rule| (file.clone(), rule.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn count(rule: &Value) -> usize {
    rule.get("count")
        .map_or(1, |count| count.as_u64().unwrap() as usize)
}

fn sync(root: &Path, mode: &str) -> Output {
    super::support::git_topology::command("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join(TOOL))
        .args([mode, "--root"])
        .arg(root)
        .output()
        .unwrap()
}

/// A release fixture plus every inventoried file, tracked in a fresh Git index.
fn inventoried_fixture() -> Fixture {
    let fixture = Fixture::release();
    let inventory = inventory();
    let mut files = vec![INVENTORY.to_owned()];
    for group in ["archives", "documents", "sources"] {
        files.extend(inventory[group].as_object().unwrap().keys().cloned());
    }
    for name in files {
        let path = fixture.0.join(&name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join(&name), path).unwrap();
    }
    expect_success(git(&fixture.0, &["init", "-q"]));
    expect_success(git(&fixture.0, &["add", "--all"]));
    fixture
}

#[test]
fn repository_version_mentions_are_inventoried_and_current() {
    let output = sync(Path::new(env!("CARGO_MANIFEST_DIR")), "--check");
    expect_success(output.clone());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(&format!("all current mentions read {VERSION}")),
        "{stdout}"
    );
}

#[test]
fn bumping_the_package_version_moves_every_current_mention_and_no_pinned_one() {
    let fixture = inventoried_fixture();
    let inventory = inventory();
    let rules = rules(&inventory);
    let before = rules
        .iter()
        .map(|(file, _)| {
            (
                file.clone(),
                fs::read_to_string(fixture.0.join(file)).unwrap(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    // The release commit changes only Cargo.toml and adds its CHANGELOG entry.
    fixture.replace(
        "Cargo.toml",
        &format!("version = \"{VERSION}\""),
        &format!("version = \"{BUMPED}\""),
    );
    fixture.replace(
        "CHANGELOG.md",
        current_changelog_heading(),
        &format!(
            "## {BUMPED}\n\n{}\n\nFixture release.\n\n{}",
            release_line(BUMPED),
            current_changelog_heading()
        ),
    );
    let stale = sync(&fixture.0, "--check");
    expect_failure(stale.clone(), "current mentions behind the package version");
    assert!(String::from_utf8_lossy(&stale.stderr).contains("--apply"));
    expect_success(sync(&fixture.0, "--apply"));

    let mut expected_changed = 0;
    for (file, rule) in &rules {
        let after = fs::read_to_string(fixture.0.join(file)).unwrap();
        let lines = after.lines().collect::<Vec<_>>();
        if let Some(template) = rule.get("current").and_then(Value::as_str) {
            let moved = template.replace("{version}", BUMPED);
            let old = template.replace("{version}", VERSION);
            assert_eq!(
                lines.iter().filter(|line| **line == moved).count(),
                count(rule),
                "{file}: current mention did not follow: {template}"
            );
            assert!(!lines.contains(&old.as_str()), "{file}: left behind: {old}");
            expected_changed += count(rule);
        } else {
            let pinned = rule["pinned"].as_str().unwrap();
            assert_eq!(
                lines.iter().filter(|line| **line == pinned).count(),
                count(rule),
                "{file}: pinned mention moved: {pinned}"
            );
        }
    }
    // Nothing but current lines changed, in any inventoried file.
    let mut changed = 0;
    for (file, old) in &before {
        let new = fs::read_to_string(fixture.0.join(file)).unwrap();
        assert_eq!(old.lines().count(), new.lines().count(), "{file}");
        for (old_line, new_line) in old.lines().zip(new.lines()) {
            if old_line != new_line {
                changed += 1;
                assert_eq!(old_line.replace(VERSION, BUMPED), new_line, "{file}");
            }
        }
    }
    assert_eq!(changed, expected_changed);
    assert!(
        expected_changed >= 30,
        "inventory lost its current mentions"
    );

    // The next release commit needs no other version edit.
    expect_success(sync(&fixture.0, "--check"));
    expect_success(gate("check-release-refs.sh", &fixture.0));
}

#[test]
fn unlisted_stale_or_unexplained_mentions_fail_the_inventory_check() {
    let fixture = inventoried_fixture();
    expect_success(sync(&fixture.0, "--check"));

    // A new mention in an inventoried document.
    let path = fixture.0.join("docs/onboarding.md");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{original}\nInstall v1.2.3 for this.\n")).unwrap();
    let output = sync(&fixture.0, "--check");
    expect_failure(output.clone(), "unlisted mention");
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("unlisted version mention: docs/onboarding.md:"));
    fs::write(&path, &original).unwrap();

    // A new tracked document; an untracked one is not part of the docs.
    fs::write(fixture.0.join("docs/new.md"), "Since 0.30.1 this works.\n").unwrap();
    expect_success(sync(&fixture.0, "--check"));
    expect_success(git(&fixture.0, &["add", "docs/new.md"]));
    let output = sync(&fixture.0, "--check");
    expect_failure(output.clone(), "unlisted mention in a new document");
    assert!(String::from_utf8_lossy(&output.stderr).contains("docs/new.md:1"));
    expect_success(git(&fixture.0, &["rm", "-q", "--cached", "docs/new.md"]));

    // A current line edited by hand to another version is stale, not pinned.
    fixture.replace("SECURITY.md", &format!("v{VERSION}"), "v0.10.0");
    expect_failure(sync(&fixture.0, "--check"), "hand-edited current mention");
    expect_success(sync(&fixture.0, "--apply"));
    expect_success(sync(&fixture.0, "--check"));

    // A pinned mention needs a reason, and a vanished pinned line is reported.
    let inventory_path = fixture.0.join(INVENTORY);
    let text = fs::read_to_string(&inventory_path).unwrap();
    let mut changed: Value = serde_json::from_str(&text).unwrap();
    changed["documents"]["CONTRIBUTING.md"][0]["reason"] = Value::from(" ");
    fs::write(&inventory_path, changed.to_string()).unwrap();
    expect_failure(
        sync(&fixture.0, "--check"),
        "pinned mention without a reason",
    );
    fs::write(&inventory_path, &text).unwrap();
    fixture.replace("CONTRIBUTING.md", "actionlint 1.7.12", "actionlint 1.7.13");
    expect_failure(
        sync(&fixture.0, "--check"),
        "pinned line changed without the inventory",
    );
}
