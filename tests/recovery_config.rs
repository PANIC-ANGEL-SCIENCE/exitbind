#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("recovery-config");
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        fs::rename(root.join("exitbind.json"), root.join("alternate.json")).unwrap();
        Self { root }
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("alternate.json"))
            .output()
            .unwrap()
    }

    fn value(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ledger(&self, work: &str) -> String {
        format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_emitted(root: &Path, command: &Value) -> Output {
    let mut args = command
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let program = args.remove(0);
    let executable = if program == "exitbind" {
        env!("CARGO_BIN_EXE_exitbind").to_owned()
    } else {
        program
    };
    Command::new(executable)
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn emitted_recovery_commands_preserve_an_alternate_config() {
    let fixture = Fixture::new();
    let config = fixture.root.join("alternate.json").canonicalize().unwrap();
    let config = config.to_str().unwrap();
    let first = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "alternate healthy one",
        "--check-command",
        "true",
    ]);
    let second = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "alternate healthy two",
        "--check-command",
        "true",
    ]);
    let first_work = first["work"].as_str().unwrap();
    let second_work = second["work"].as_str().unwrap();
    let second_ledger = fixture.ledger(second_work);

    // The second begin holds the current-work focus; the history listing
    // emits a command for every running work.
    let resume = fixture.call(&["work", "resume", "--history", "--json"]);
    assert!(resume.status.success(), "{resume:?}");
    let resume: Value = serde_json::from_slice(&resume.stdout).unwrap();
    assert_eq!(resume["status"], "history");
    let candidate = &resume["works"][0];
    let candidate_work = candidate["work"].as_str().unwrap();
    assert!(candidate_work == first_work || candidate_work == second_work);
    let command = candidate["command"].as_array().unwrap();
    assert_eq!(command[command.len() - 2], "--config");
    assert_eq!(command.last().unwrap(), config);
    let next = run_emitted(&fixture.root, &candidate["command"]);
    assert!(next.status.success(), "{next:?}");
    let next: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(next["work"], candidate_work);

    fs::write(fixture.root.join(&second_ledger), b"not-json\n").unwrap();
    let mixed = fixture.call(&["work", "resume", "--json"]);
    assert!(mixed.status.success(), "{mixed:?}");
    let mixed: Value = serde_json::from_slice(&mixed.stdout).unwrap();
    let unreadable = mixed["unreadable"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["work"] == second_work)
        .unwrap();
    let inspect = unreadable["command"].as_array().unwrap();
    assert_eq!(inspect[3], second_ledger);
    assert_eq!(inspect[inspect.len() - 2], "--config");
    assert_eq!(inspect.last().unwrap(), config);
    let inspect_output = run_emitted(&fixture.root, &unreadable["command"]);
    assert!(!inspect_output.status.success(), "{inspect_output:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&inspect_output.stdout),
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    assert!(diagnostic.contains("invalid") && diagnostic.contains("ledger"));
    assert!(!fixture.root.join("exitbind.json").exists());
}
