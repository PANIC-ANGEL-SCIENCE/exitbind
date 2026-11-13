//! The current-work focus is navigation only: it selects which running work
//! `work resume` returns, and never changes a work ledger.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

mod support;

fn call(root: &Path, args: &[&str]) -> Output {
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .output()
        .unwrap()
}

fn ok(root: &Path, args: &[&str]) -> Value {
    let output = call(root, args);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn project(label: &str) -> support::TempRoot {
    let root = support::temp(label);
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    root
}

fn begin(root: &Path, goal: &str) -> Value {
    ok(
        root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "true",
        ],
    )
}

fn ledgers(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = fs::read_dir(root.join(".exitbind/runs"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn focus_file(root: &Path) -> PathBuf {
    root.join(".exitbind/current-work.json")
}

#[test]
fn full_history_and_candidate_routes_execute_with_long_portable_invocation_paths() {
    const MAC_PATH_LIMIT: usize = 1024;
    const STATE_PATH_HEADROOM: usize = 256;
    let base = support::temp("focus-live-routes");
    let mut root = base.join("project");
    loop {
        let config = root.join("exitbind.json");
        if serde_json::to_vec(config.to_str().unwrap()).unwrap().len() > 1024 {
            break;
        }
        root.push("\"".repeat(180));
    }
    assert!(root.as_os_str().len() + STATE_PATH_HEADROOM < MAC_PATH_LIMIT);
    fs::create_dir_all(&root).unwrap();
    let config = root.join("exitbind.json");
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");

    let mut binary_dir = base.join("binary");
    for _ in 0..3 {
        binary_dir.push("\"".repeat(180));
    }
    fs::create_dir_all(&binary_dir).unwrap();
    let executable = binary_dir.join("exitbind");
    assert!(executable.as_os_str().len() < MAC_PATH_LIMIT);
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &executable);
    let executable = fs::canonicalize(executable).unwrap();
    // POSIX permits these non-NUL bytes in names. JSON escapes each as six
    // bytes, so the executable alone crosses the route budget while its real
    // filesystem path and each component remain below macOS limits.
    let mut escaped_dir = base.join("escaped-binary");
    for _ in 0..3 {
        escaped_dir.push("\u{0001}".repeat(160));
    }
    fs::create_dir_all(&escaped_dir).unwrap();
    let escaped = escaped_dir.join("exitbind");
    assert!(escaped.as_os_str().len() < MAC_PATH_LIMIT);
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &escaped);
    let escaped = fs::canonicalize(escaped).unwrap();
    assert!(serde_json::to_vec(escaped.to_str().unwrap()).unwrap().len() > 2048);
    let config = fs::canonicalize(config).unwrap();
    let serialized_operands =
        serde_json::to_vec(&[executable.to_str().unwrap(), config.to_str().unwrap()]).unwrap();
    assert!(
        serialized_operands.len() > 2048,
        "fixture did not cross the route budget"
    );

    let invoke = |executable: &Path, args: &[&str]| {
        support::git_topology::command(executable)
            .current_dir(&root)
            .args(args)
            .arg("--config")
            .arg(&config)
            .output()
            .unwrap()
    };
    let begin = |goal: &str| {
        let output = invoke(
            &executable,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                goal,
                "--check-command",
                "true",
            ],
        );
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["work"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let old = begin("older live route");
    let current = begin("focused live route");
    let run_route = |executable: &Path, route: &Value| {
        assert_eq!(route["sameExecutableRequired"], false, "{route}");
        assert_eq!(route["sameConfigRequired"], false, "{route}");
        let argv = route["command"].as_array().expect("complete route");
        let words = argv
            .iter()
            .map(|word| word.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(Path::new(words[0]), executable);
        assert_eq!(words[words.len() - 2], "--config");
        assert_eq!(Path::new(words[words.len() - 1]), config);
        support::git_topology::command(words[0])
            .current_dir(&root)
            .args(&words[1..])
            .output()
            .expect("the emitted executable must launch without transport repair")
    };

    for executable in [&executable, &escaped] {
        let full = invoke(executable, &["work", "resume", "--full", "--json"]);
        assert!(full.status.success(), "{full:?}");
        let full: Value = serde_json::from_slice(&full.stdout).unwrap();
        assert_eq!(full["work"], current);
        let history = run_route(executable, &full["history"]);
        assert!(history.status.success(), "{history:?}");
        let history: Value = serde_json::from_slice(&history.stdout).unwrap();
        assert_eq!(history["status"], "history");
        let works = history["works"].as_array().unwrap();
        assert_eq!(works.len(), 2);
        for candidate in works {
            let output = run_route(executable, candidate);
            assert!(output.status.success(), "{output:?}");
            let next: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(next["work"], candidate["work"]);
        }
    }

    // A damaged older ledger keeps its exact live inspection route; failure
    // must come from the damaged ledger, not an incomplete invocation.
    let ledger = root
        .join(".exitbind/runs")
        .join(format!("work-{}.jsonl", old.strip_prefix("smw_").unwrap()));
    fs::write(ledger, b"not-json\n").unwrap();
    for executable in [&executable, &escaped] {
        let full = invoke(executable, &["work", "resume", "--full", "--json"]);
        assert!(full.status.success(), "{full:?}");
        let full: Value = serde_json::from_slice(&full.stdout).unwrap();
        assert_eq!(full["work"], current);
        let unreadable = full["unreadable"].as_array().unwrap();
        assert_eq!(unreadable.len(), 1);
        let inspected = run_route(executable, &unreadable[0]);
        assert!(!inspected.status.success(), "{inspected:?}");
        let error: Value = serde_json::from_slice(&inspected.stdout).unwrap();
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("invalid run ledger"),
            "{inspected:?}"
        );
    }

    let compact = invoke(&executable, &["work", "resume", "--json"]);
    assert!(compact.status.success(), "{compact:?}");
    assert!(compact.stdout.len() <= 8 * 1024);
    let compact: Value = serde_json::from_slice(&compact.stdout).unwrap();
    assert_eq!(compact["work"], current);
    if let Some(items) = compact["unreadable"].as_array() {
        for item in items {
            if item["command"].is_array() {
                let output = run_route(&executable, item);
                assert!(!output.status.success());
            } else {
                assert_eq!(item["available"], false);
            }
        }
    } else {
        assert_eq!(compact["complete"], false, "{compact}");
    }
}

#[test]
fn begin_selects_new_work_and_older_running_work_stays_history() {
    // Force the longer native temporary-path boundary on every host, rather
    // than depending on the runner's TMPDIR to reach compact fallback.
    let root = project(&format!("focus-select-{}", "path".repeat(24)));
    let old = begin(&root, "older task")["work"]
        .as_str()
        .unwrap()
        .to_owned();
    let started = begin(&root, "current task");
    assert_eq!(started["focus"]["updated"], true);
    assert_eq!(started["focus"]["authority"], "none");
    let current = started["work"].as_str().unwrap().to_owned();
    let before = ledgers(&root);

    let output = call(&root, &["work", "resume"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.len() <= 8 * 1024, "{}", output.stdout.len());
    let resumed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], current.as_str());
    assert_eq!(resumed["selection"]["basis"], "current_work_focus");
    assert_eq!(resumed["history"]["running"], 1);
    assert_eq!(
        resumed["history"]["command"][0],
        env!("CARGO_BIN_EXE_exitbind")
    );
    assert_eq!(resumed["history"]["sameExecutableRequired"], false);
    assert_eq!(resumed["history"]["sameConfigRequired"], false);
    assert_eq!(resumed["history"]["command"][3], "--history");
    let grouped = &resumed["current"]["details"]["grouped"];
    assert!(grouped["command"].is_array(), "{resumed}");
    assert_eq!(grouped["sameExecutableRequired"], false);
    assert_eq!(grouped["sameConfigRequired"], false);

    let route = resumed["history"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap())
        .collect::<Vec<_>>();
    let output = support::git_topology::command(route[0])
        .current_dir(&root)
        .args(&route[1..])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let history: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(history["status"], "history");
    assert_eq!(
        history["works"][0]["command"][0],
        env!("CARGO_BIN_EXE_exitbind")
    );
    let mut listed = history["works"]
        .as_array()
        .unwrap()
        .iter()
        .map(|work| work["work"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    listed.sort();
    let mut expected = vec![old.clone(), current];
    expected.sort();
    assert_eq!(listed, expected);

    // An explicit locator still reaches older work, and refocusing changes
    // navigation only.
    assert!(call(&root, &["work", "next", &old]).status.success());
    let refocused = ok(&root, &["work", "focus", &old]);
    assert_eq!(refocused["effect"], "focus-only");
    assert_eq!(ok(&root, &["work", "resume"])["work"], old.as_str());
    assert_eq!(ledgers(&root), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn focus_on_non_running_work_reports_no_current_work() {
    let root = project("focus-stale");
    begin(&root, "abandoned one");
    begin(&root, "abandoned two");
    let gone = format!("smw_{}", "a".repeat(64));
    fs::write(
        focus_file(&root),
        format!(r#"{{"version":1,"work":"{gone}","authority":"none"}}"#),
    )
    .unwrap();
    let before = ledgers(&root);
    let entry = ok(&root, &["work", "resume"]);
    assert_eq!(entry["status"], "none");
    assert_eq!(entry["reason"]["code"], "no_current_work");
    assert_eq!(entry["focus"]["work"], gone.as_str());
    assert_eq!(entry["history"]["running"], 2);
    assert!(entry.get("work").is_none());
    assert!(!call(&root, &["work", "focus", &gone]).status.success());
    assert_eq!(ledgers(&root), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn legacy_project_without_focus_keeps_count_based_resume() {
    let root = project("focus-legacy");
    begin(&root, "one");
    begin(&root, "two");
    fs::remove_file(focus_file(&root)).unwrap();
    let entry = ok(&root, &["work", "resume"]);
    assert_eq!(entry["status"], "ambiguous");
    assert_eq!(entry["reason"]["code"], "ambiguous_candidates");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn unusable_focus_selects_nothing_and_names_the_recovery_route() {
    let root = project("focus-invalid");
    let work = begin(&root, "task")["work"].as_str().unwrap().to_owned();
    fs::write(focus_file(&root), "not json").unwrap();
    let entry = ok(&root, &["work", "resume"]);
    assert_eq!(entry["status"], "none");
    assert_eq!(entry["reason"]["code"], "focus_unusable");
    assert!(entry["reason"]["detail"]
        .as_str()
        .unwrap()
        .contains("work resume --history"));
    assert_eq!(entry["history"]["running"], 1);
    assert!(entry.get("work").is_none());
    ok(&root, &["work", "focus", &work]);
    assert_eq!(ok(&root, &["work", "resume"])["work"], work.as_str());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn default_resume_keeps_focused_result_and_unreadable_history_in_compact_json() {
    let root = project("focus-unreadable-compact");
    let focused = begin(&root, "current work");
    let corrupt = begin(&root, "unreadable older history");
    let focused_work = focused["work"].as_str().unwrap();
    ok(&root, &["work", "focus", focused_work]);
    let corrupt_work = corrupt["work"].as_str().unwrap();
    let ledger = root.join(".exitbind/runs").join(format!(
        "work-{}.jsonl",
        corrupt_work.strip_prefix("smw_").unwrap()
    ));
    fs::write(ledger, b"not-json\n").unwrap();

    // This uses the ordinary CLI path without --full, so compaction is part of
    // the user-visible contract under test.
    let output = call(&root, &["work", "resume"]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "resumed");
    assert_eq!(value["work"], focused_work);
    assert_eq!(value["works"].as_array().unwrap().len(), 0);
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
    assert_eq!(value["unreadable"][0]["work"], corrupt_work);
    assert_eq!(value["unreadable"][0]["reason"], "corrupt_ledger");
    assert_eq!(value["unreadable"][0]["command"][2], "inspect");
    assert_eq!(value["fullCommand"][2], "resume");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn default_resume_unreadable_result_has_exact_read_only_candidate_route() {
    let root = project("unreadable-candidate-route");
    let started = begin(&root, "unreadable current candidate");
    let work = started["work"].as_str().unwrap();
    let ledger = root
        .join(".exitbind/runs")
        .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()));
    fs::write(ledger, b"not-json\n").unwrap();
    fs::remove_file(focus_file(&root)).unwrap();

    let output = call(&root, &["work", "resume"]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "unresolved");
    assert_eq!(value["reason"]["code"], "unreadable_candidate");
    assert!(value["nextAction"]["summary"]
        .as_str()
        .unwrap()
        .contains("read-only command shown for each unreadable candidate"));

    let candidate = &value["unreadable"][0];
    assert_eq!(candidate["workingDirectory"], root.to_str().unwrap());
    assert_eq!(candidate["reason"], "corrupt_ledger");
    let command = candidate["command"].as_array().unwrap();
    assert_eq!(command[0], env!("CARGO_BIN_EXE_exitbind"));
    assert_eq!(candidate["sameExecutableRequired"], false);
    assert_eq!(candidate["sameConfigRequired"], false);
    assert_eq!(command[1], "run");
    assert_eq!(command[2], "inspect");
    assert_eq!(command[4], "--json");
    assert_eq!(command[5], "--config");
    assert_eq!(command[6], root.join("exitbind.json").to_str().unwrap());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn failed_focus_update_reports_the_committed_work_and_recovery() {
    let root = project("focus-write-failure");
    fs::create_dir(focus_file(&root)).unwrap();
    let started = begin(&root, "task");
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["focus"]["updated"], false);
    assert!(started["focus"]["error"].is_string());
    assert_eq!(started["focus"]["command"][2], "focus");
    assert_eq!(started["focus"]["command"][3], work);
    assert!(call(&root, &["work", "next", work]).status.success());
    assert_eq!(ledgers(&root).len(), 1);

    // Resume selects nothing, and the recovery says why until the path is fixed.
    assert_eq!(
        ok(&root, &["work", "resume"])["reason"]["code"],
        "focus_unusable"
    );
    let refused = call(&root, &["work", "focus", work]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not a regular file"));
    fs::remove_dir(focus_file(&root)).unwrap();
    ok(&root, &["work", "focus", work]);
    assert_eq!(ok(&root, &["work", "resume"])["work"], work);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn begin_accepts_redundant_json_and_rejects_unknown_options_before_mutation() {
    let root = project("begin-json-option");
    let started = call(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "JSON-compatible begin",
            "--check-command",
            "true",
            "--json",
        ],
    );
    assert!(started.status.success(), "{started:?}");
    let value: Value = serde_json::from_slice(&started.stdout).unwrap();
    assert!(value["work"].as_str().is_some());
    let before = ledgers(&root);
    assert_eq!(before.len(), 1);
    let focus_before = fs::read(focus_file(&root)).unwrap();

    let rejected = call(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "must not begin",
            "--check-command",
            "true",
            "--unrecognized",
        ],
    );
    assert!(!rejected.status.success(), "{rejected:?}");
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--unrecognized"));
    assert_eq!(ledgers(&root), before);
    assert_eq!(fs::read(focus_file(&root)).unwrap(), focus_before);
    fs::remove_dir_all(&root).unwrap();
}
