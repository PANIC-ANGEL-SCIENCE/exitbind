//! An unselected legacy pre-return retains the existing pipe observer.
use super::*;

#[test]
fn unselected_preparation_does_not_create_a_durable_observer() {
    let root = support::temp("pre-return-unselected-observer");
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let begun = successful(command(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "unselected pre-return",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--pre-return-command",
            "printf x >> .exitbind/legacy-count; exit 0",
        ],
        None,
    ));
    let work = begun["work"].as_str().unwrap();
    let lead = begun["next"]["assignment"].as_str().unwrap();
    assert!(command(
        &root,
        &["work", "return", work, lead, "--outcome", "scoped"],
        Some(b"scope")
    )
    .status
    .success());
    let detail = successful(command(&root, &["work", "detail", work, "--json"], None));
    let worker = detail["recipient"]["assignment"].as_str().unwrap();
    let returned = command(
        &root,
        &["work", "return", work, worker, "--outcome", "completed"],
        Some(b"result"),
    );
    assert!(returned.status.success(), "{returned:?}");
    let entries = fs::read_dir(root.join(".exitbind/pre-return"))
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    assert!(!entries
        .iter()
        .any(|name| name.to_string_lossy().ends_with(".observer.raw")));
    let ledger =
        fs::read_to_string(root.join(format!(".exitbind/runs/work-{}.jsonl", &work[4..]))).unwrap();
    let event = ledger
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|event| event["action"] == "pre_return_admitted")
        .unwrap();
    let run_id = event["runId"].as_str().unwrap();
    let id = event["admission"]["attemptId"].as_str().unwrap();
    assert!(root
        .join(".exitbind/pre-return")
        .join(format!("pre-return-{run_id}-{id}.attempt.json"))
        .is_file());
    let saved = successful(command(
        &root,
        &["run", "pre-return", "read", work, id, "--json"],
        None,
    ));
    assert_eq!(saved["result"]["state"], "passed");
    assert_eq!(fs::read(root.join(".exitbind/legacy-count")).unwrap(), b"x");
}
