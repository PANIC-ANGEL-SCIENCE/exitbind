//! Two guarantees held at once: the surface a new user meets is Exitbind only,
//! and a historical Soulmate artifact stays readable under its own identity.
//!
//! The first test walks everything a fresh install writes and everything the
//! normal commands print. The second proves the compatibility path still works
//! and that it does not leak the old identity into newly written state.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
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
        support::place_executable(
            Path::new(env!("CARGO_BIN_EXE_exitbind")),
            &bin.join("exitbind"),
        );
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
        let output = support::run(
            Command::new(self.bin.join("exitbind"))
                .args(arguments)
                .current_dir(&self.project)
                .env("HOME", &self.home)
                .env("PATH", path)
                .env("TMPDIR", self.home.join("tmp-unused"))
                .env("EXITBIND_NO_UPDATE_CHECK", "1"),
        );
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
    assert_eq!(
        inspected["warnings"][0]["classification"], "harness_receipt_drift",
        "a missing sidecar on a v2 historical ledger should be disclosed as drift"
    );
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

#[test]
fn a_fresh_exitbind_config_accepts_the_fallback_shape_in_its_declared_schema() {
    let mut fresh = Fresh::new("fresh-schema");
    fresh.run(&["init", "--mode", "portable", "--root", "."]);

    let config_path = fresh.project.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let schema_uri = config["$schema"]
        .as_str()
        .expect("fresh config declares a schema URI");
    let schema: Value = serde_json::from_str(include_str!("../schema/exitbind.schema.json"))
        .expect("current schema is valid JSON");
    assert_eq!(
        schema_uri, schema["$id"],
        "fresh config names the current schema"
    );

    let fallback = json!({
        "host": "claude",
        "model": "alternate-review",
        "reasoningEffort": "high",
    });
    config["agents"]["reviewer"]["runtime"]["fallback"] = fallback.clone();
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    let checked = fresh.run(&["check", "--json"]);
    assert!(
        checked.contains("\"valid\":true"),
        "runtime validation rejected the schema regression fixture: {checked}"
    );
    assert!(schema_accepts_fallback(&schema, &fallback));
    assert!(schema_accepts_fallback(&schema, &json!("none")));
    for rejected in [
        json!("alternate"),
        json!({}),
        json!({"host": "claude", "unexpected": "field"}),
    ] {
        assert!(
            !schema_accepts_fallback(&schema, &rejected),
            "schema accepted malformed fallback {rejected}"
        );
    }
    let whitespace_only = json!({"host": " "});
    assert!(!schema_accepts_fallback(&schema, &whitespace_only));
    config["agents"]["reviewer"]["runtime"]["fallback"] = whitespace_only;
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let rejected = fresh.run(&["check", "--json"]);
    assert!(
        rejected.contains("invalid configuration")
            && rejected.contains("fallback.host must be a non-empty string"),
        "runtime validation accepted whitespace-only fallback: {rejected}"
    );
}

/// Exercise the fallback subschema from the declared schema without adding a
/// schema-engine dependency just for this public-boundary regression.
fn schema_accepts_fallback(schema: &Value, fallback: &Value) -> bool {
    let alternatives = &schema["$defs"]["agent"]["properties"]["runtime"]["$ref"];
    let runtime_name = alternatives.as_str().and_then(|reference| {
        reference
            .strip_prefix("#/$defs/")
            .map(|name| schema["$defs"][name].clone())
    });
    let Some(runtime) = runtime_name else {
        return false;
    };
    let Some(options) = runtime["properties"]["fallback"]["oneOf"].as_array() else {
        return false;
    };
    if options
        .iter()
        .any(|option| option.get("const") == Some(fallback))
    {
        return true;
    }
    let Some(binding_name) = options.iter().find_map(|option| {
        option["$ref"]
            .as_str()
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
    }) else {
        return false;
    };
    let Some(binding) = schema["$defs"][binding_name].as_object() else {
        return false;
    };
    let Some(object) = fallback.as_object() else {
        return false;
    };
    object.len() >= binding["minProperties"].as_u64().unwrap_or(0) as usize
        && object.iter().all(|(name, value)| {
            binding["properties"][name]["pattern"] == json!(".*\\S.*")
                && value.as_str().is_some_and(|text| {
                    text.len()
                        >= binding["properties"][name]["minLength"]
                            .as_u64()
                            .unwrap_or(0) as usize
                        && text.chars().any(|character| !character.is_whitespace())
                })
        })
        && binding["additionalProperties"] == json!(false)
}
