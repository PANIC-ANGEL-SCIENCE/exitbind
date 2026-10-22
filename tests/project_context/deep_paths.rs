//! R55 session-summary budget with deep paths (re-review 9dd1dca).
use super::*;

/// R55: with a deep checkout and a deep executable path, the session context
/// still fits 3072 bytes, keeps the mandatory core (the full sentence, or its
/// compact form when the full summary cannot fit), and prints the full
/// project-context route; the complete-lists route is what gives way first.
/// The SessionStart context for a project and an executable copy, each
/// `depth` long directory segments below the temporary base.
fn deep_context(label: &str, depth: usize) -> (String, String) {
    let base = support::temp(label);
    let deep = |name: &str| {
        (0..depth).fold(base.join(name), |path, index| {
            path.join(format!("a-deep-checkout-directory-segment-{index:02}"))
        })
    };
    let root = deep("project");
    fs::create_dir_all(&root).unwrap();
    support::git_topology::repository(&root);
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let worker = config["agents"]["worker"].clone();
    for index in 0..100 {
        config["agents"][format!("extra_agent_{index:03}")] = worker.clone();
    }
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let bin = deep("bin");
    fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("exitbind");
    fs::copy(env!("CARGO_BIN_EXE_exitbind"), &exe).unwrap();
    let context = session_hook_with(&exe, &root);
    let route = format!("{} project context --json --config ", exe.display());
    fs::remove_dir_all(base).unwrap();
    (context, route)
}

/// R55: with deep checkout and executable paths the session context still
/// fits 3072 bytes and prints full routes (the project-context route at a
/// moderate depth, the Work read route at any depth). The
/// complete-lists route gives way first, then the full mandatory core gives
/// way to its compact form; the core itself is never cut.
#[test]
fn deep_paths_keep_the_session_context_bounded_with_its_core() {
    let (moderate, route) = deep_context("project-context-deep-1", 1);
    assert!(moderate.len() <= 3072, "{} bytes", moderate.len());
    assert!(moderate.contains(&route), "{moderate}");
    assert!(
        moderate.contains("Preserve the existing root"),
        "{moderate}"
    );

    let (deepest, route) = deep_context("project-context-deep-3", 3);
    assert!(route.len() > 150, "{route}");
    assert!(deepest.len() <= 3072, "{} bytes", deepest.len());
    // At this depth the receiving routes alone fill most of the budget; the
    // Work read route stays, and the core is never cut.
    let exe = route.split(" project context").next().unwrap();
    assert!(
        deepest.contains(&format!("{exe} work detail <WORK> --json --config ")),
        "{deepest}"
    );
    assert!(deepest.contains("Preserve the existing root"), "{deepest}");
}
