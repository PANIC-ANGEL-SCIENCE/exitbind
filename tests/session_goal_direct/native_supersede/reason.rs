//! Exact committed reason and structured recovery route.

use super::*;

#[test]
fn recovery_route_keeps_shell_metacharacters_inside_the_exact_reason_argument() {
    let case = Case::new("native-successor-literal-reason");
    let reason_text = "literal>fixture-redirection";
    let reason = "--reason=literal>fixture-redirection";
    let result = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "supersede", &case.predecessor, reason, "--json"],
    );
    let route = result["recovery"]["command"]["argv"].as_array().unwrap();
    let argv = route
        .iter()
        .map(|word| word.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(argv.contains(&reason));
    let successor = result["work"].as_str().unwrap();
    let claim = PathBuf::from(format!(
        "{}.supersede",
        case.ledger(&case.predecessor).display()
    ));
    for path in [case.journal(), claim, case.ledger(successor)] {
        let saved = fs::read_to_string(&path).unwrap();
        assert!(
            !saved.contains(reason_text),
            "reason text must not be presented as recoverable from {}",
            path.display()
        );
    }
    let mut command = crate::support::git_topology::command(argv[0]);
    command
        .current_dir(&case.project.root)
        .args(&argv[1..])
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CODEX_THREAD_ID", ADOPTER)
        .env("EXITBIND_SESSION_GOALS_DIR", &case.project.session_store);
    let replay = command.output().unwrap();
    assert!(
        replay.status.success(),
        "the structured exact request must replay: {replay:?}"
    );
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["work"], result["work"]);
    assert!(!case.project.root.join("fixture-redirection").exists());
}
