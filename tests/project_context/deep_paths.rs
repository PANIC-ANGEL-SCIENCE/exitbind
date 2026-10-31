//! R55 session-summary budget with deep paths (re-review 9dd1dca).
use super::*;
use sha2::{Digest, Sha256};

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
    let rules = [
        ("AGENTS.md", "Keep the user's correction."),
        ("CLAUDE.md", "Do not publish."),
    ];
    for (name, body) in rules {
        fs::write(root.join(name), body).unwrap();
    }
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
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &exe);
    let context = session_hook_with(&exe, &root);
    let command = context
        .split_once("Current project facts: `")
        .and_then(|(_, rest)| rest.split_once('`'))
        .map(|(command, _)| command)
        .unwrap_or_else(|| panic!("project facts route missing: {context}"));
    let output = support::git_topology::command("/bin/sh")
        .args(["-c", command])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{command}: {output:?}");
    let facts: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut omitted = 0;
    for (name, body) in rules {
        let rule = facts["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["path"] == name)
            .unwrap();
        assert_eq!(rule["state"], "current");
        assert_eq!(
            rule["sha256"],
            format!("{:x}", Sha256::digest(body.as_bytes()))
        );
        if !context.contains(&format!("Project rule: {name} ")) {
            omitted += 1;
        }
    }
    if omitted > 0 {
        assert!(
            context.contains(&format!(
                "{omitted} more references; run the current project facts route above."
            )),
            "{context}"
        );
    }
    if depth == 3 && root.to_str().unwrap().len() > 250 {
        assert!(
            omitted > 0,
            "fixture must exercise reference omission: {context}"
        );
    }
    let lead = &facts["lead"];
    let profile_bytes = fs::read(root.join(lead["profilePath"].as_str().unwrap())).unwrap();
    assert!(!profile_bytes.is_empty());
    assert_eq!(
        lead["profileSha256"],
        format!("{:x}", Sha256::digest(&profile_bytes))
    );
    let profile = format!(
        "Lead profile: {} -> {} (sha256 {}).",
        lead["agentId"].as_str().unwrap(),
        lead["nativeName"].as_str().unwrap(),
        lead["profileSha256"].as_str().unwrap()
    );
    assert!(context.contains(&profile), "{context}");
    let resume = context
        .split('`')
        .find(|part| part.contains(" work resume --json"))
        .unwrap_or_else(|| panic!("resume route missing: {context}"));
    let output = support::git_topology::command("/bin/sh")
        .args(["-c", resume])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{resume}: {output:?}");
    let _: Value = serde_json::from_slice(&output.stdout).unwrap();
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
    // Under pressure the concrete resume route supplies the current Work
    // detail; project facts and the mandatory core remain available.
    let exe = route.split(" project context").next().unwrap();
    assert!(
        deepest.contains(&format!("{exe} work resume --json --config ")),
        "{deepest}"
    );
    assert!(deepest.contains(&route), "{deepest}");
    assert!(
        deepest.contains("Preserve the existing root host conversation"),
        "{deepest}"
    );
    assert!(
        deepest.contains("do not run Exitbind commands, initialize a project"),
        "{deepest}"
    );
    assert!(
        deepest.contains("Classification is the lead's job")
            || deepest.contains("the Lead classifies"),
        "{deepest}"
    );
    assert!(deepest.contains("Lead profile:"), "{deepest}");
}
