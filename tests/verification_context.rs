//! Supported focused verification must isolate real fixture subprocesses.
mod support;

#[test]
fn owned_focused_verification_preserves_outer_goal_and_child_outcomes() {
    let root = support::temp("verification-context");
    let output = support::git_topology::command("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/test-verification-context.py"
        ))
        .arg(env!("CARGO_BIN_EXE_exitbind"))
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(root).unwrap();
}
