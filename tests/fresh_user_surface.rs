//! Two guarantees held at once: the surface a new user meets is Exitbind only,
//! and a historical Soulmate artifact stays readable under its own identity.
//!
//! The first test walks everything a fresh install writes and everything the
//! normal commands print. The second proves the compatibility path still works
//! and that it does not leak the old identity into newly written state.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

const LEGACY: &str = "soulmate";

struct Fresh {
    home: PathBuf,
    project: PathBuf,
    bin: PathBuf,
    transcript: String,
}

impl Fresh {
    fn new(label: &str) -> Self {
        let base = support::temp(label);
        let home = base.join("home");
        let project = base.join("project");
        let bin = base.join("bin");
        for path in [&home, &project, &bin] {
            fs::create_dir(path).unwrap();
        }
        let binary = bin.join("exitbind");
        fs::copy(env!("CARGO_BIN_EXE_exitbind"), &binary).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .arg(&project)
            .status()
            .unwrap()
            .success());
        fs::write(project.join("work.txt"), b"unfinished user work\n").unwrap();
        Self {
            home,
            project,
            bin,
            transcript: String::new(),
        }
    }

    /// Run one command the way a new user would and keep what they would see.
    fn run(&mut self, arguments: &[&str]) -> String {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = Command::new(self.bin.join("exitbind"))
            .args(arguments)
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("PATH", path)
            .env("TMPDIR", self.home.join("tmp-unused"))
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .output()
            .unwrap();
        let seen = format!(
            "$ exitbind {}\n{}{}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        self.transcript.push_str(&seen);
        seen
    }
}

/// Every readable file a fresh install produced, as `path -> contents`.
fn written_files(root: &Path, skip: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    collect(root, root, skip, &mut found);
    found
}

fn collect(root: &Path, at: &Path, skip: &Path, found: &mut Vec<(String, String)>) {
    let entries = match fs::read_dir(at) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == skip || path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Ok(info) if info.is_dir() => collect(root, &path, skip, found),
            Ok(info) if info.is_file() => {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                match fs::read_to_string(&path) {
                    Ok(text) => found.push((relative, text)),
                    // A non-UTF-8 file is not part of the reading surface.
                    Err(error) if error.kind() == io::ErrorKind::InvalidData => {}
                    Err(error) => panic!("read {}: {error}", path.display()),
                }
            }
            _ => {}
        }
    }
}

fn legacy_mentions(label: &str, text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.to_lowercase().contains(LEGACY))
        .map(|line| format!("{label}: {}", line.trim()))
        .collect()
}

#[test]
fn a_fresh_install_and_project_never_show_the_previous_product() {
    let mut fresh = Fresh::new("fresh-surface");

    // Everything a new user reasonably runs on day one, including the two
    // shapes of failure they are most likely to hit.
    fresh.run(&["--help"]);
    fresh.run(&["help", "advanced"]);
    fresh.run(&["version"]);
    fresh.run(&["host", "status"]);
    fresh.run(&["doctor"]);
    fresh.run(&["work", "next", "not-a-handle"]);
    let initialized = fresh.run(&["init", "--mode", "portable", "--root", "."]);
    assert!(
        initialized.contains("exitbind.json"),
        "initialization did not report its configuration: {initialized}"
    );
    fresh.run(&["host", "install", "--all"]);
    fresh.run(&["check"]);
    fresh.run(&["host", "status"]);
    let begun = fresh.run(&[
        "work",
        "begin",
        "change",
        "--goal",
        "prove the fresh surface",
        "--check-command",
        "true",
    ]);
    let work = serde_json::from_str::<Value>(&begun[begun.find('{').unwrap()..])
        .ok()
        .and_then(|value| value["work"].as_str().map(str::to_owned))
        .expect("work begin returns a handle");
    fresh.run(&["work", "next", &work]);
    fresh.run(&["brief", "worker", "--task", "describe the change"]);
    fresh.run(&["hooks", "status", "--hosts", "codex,claude"]);

    let mut mentions = legacy_mentions("output", &fresh.transcript);
    for (root, skip) in [
        (&fresh.home, fresh.home.join("tmp-unused")),
        (&fresh.project, fresh.bin.clone()),
    ] {
        for (path, contents) in written_files(root, &skip) {
            assert!(
                !path.to_lowercase().contains(LEGACY),
                "a fresh install wrote {path}"
            );
            mentions.extend(legacy_mentions(&format!("file {path}"), &contents));
        }
    }
    assert!(
        mentions.is_empty(),
        "the fresh user surface still carries the previous product identity:\n{}",
        mentions.join("\n")
    );
    // The scan is only worth its name if it read a real surface.
    assert!(
        fresh.transcript.len() > 2000,
        "the transcript is too small to have exercised the surface"
    );
}

#[test]
fn a_historical_project_stays_readable_under_its_own_identity() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frozen = manifest.join("tests/fixtures/v0.7.x-run-v2.jsonl");
    let before = fs::read_to_string(&frozen).unwrap();

    // The current binary reads a ledger written by the previous product.
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(manifest)
        .args([
            "run",
            "inspect",
            "tests/fixtures/v0.7.x-run-v2.jsonl",
            "--config",
            "examples/soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "current binary cannot read the historical ledger: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inspected: Value = serde_json::from_slice(&output.stdout).unwrap();
    let producers: Vec<&str> = inspected["events"]
        .as_array()
        .expect("inspected events")
        .iter()
        .filter_map(|event| event["producer"]["name"].as_str())
        .collect();
    assert!(
        !producers.is_empty() && producers.iter().all(|name| *name == LEGACY),
        "historical producer identity was not preserved: {producers:?}"
    );
    assert_eq!(
        before,
        fs::read_to_string(&frozen).unwrap(),
        "reading a historical ledger rewrote it"
    );

    // Reading the old world does not seed the new one with its identity.
    let mut fresh = Fresh::new("legacy-isolation");
    fresh.run(&["init", "--mode", "portable", "--root", "."]);
    fresh.run(&[
        "work",
        "begin",
        "change",
        "--goal",
        "prove new state is exitbind-native",
        "--check-command",
        "true",
    ]);
    assert!(!fresh.project.join("soulmate.json").exists());
    assert!(!fresh.project.join(".soulmate").exists());
    let runs = fresh.project.join(".exitbind/runs");
    let ledger = fs::read_dir(runs)
        .unwrap()
        .flatten()
        .map(|entry| fs::read_to_string(entry.path()).unwrap())
        .collect::<String>();
    assert!(!ledger.is_empty(), "no new ledger was written");
    for line in ledger.lines() {
        let event: Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            event["producer"]["name"], "exitbind",
            "new state recorded a historical producer: {line}"
        );
    }
}
