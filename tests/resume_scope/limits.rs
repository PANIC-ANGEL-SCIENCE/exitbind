use super::*;

#[test]
fn resume_caps_all_directory_entries_including_nonmatching_names() {
    let root = support::temp("resume-scope-entry-limit");
    init(&root);
    let runs = root.join(".exitbind/runs");
    for index in 0..1025usize {
        fs::write(
            runs.join(format!("unrelated-{index:04}.txt")),
            b"ignored by ledger selection",
        )
        .unwrap();
    }
    let config = root.join("exitbind.json");
    let output = invoke(&[
        "work",
        "resume",
        "--json",
        "--config",
        config.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "unresolved");
    assert_eq!(value["reason"]["code"], "discovery_limited");
    assert_eq!(value["reason"]["detail"], "directory_entry_limit_exceeded");
    assert_eq!(value["discovery"]["complete"], false);
    assert_eq!(value["discovery"]["maxDirectoryEntries"], 1024);
    assert_eq!(value["discovery"]["allDirectoryEntries"], 1025);
    fs::remove_dir_all(root).unwrap();
}
