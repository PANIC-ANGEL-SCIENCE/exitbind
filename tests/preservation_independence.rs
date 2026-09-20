//! Preservation without a second installation.
//!
//! These fixtures answer one question mechanically: with no standalone
//! preservation package reachable, does the installed Exitbind distribution
//! carry the guidance, resolve the route and quality assignment, and still
//! catch a broken invariant while the functional check passes?
//!
//! They prove packaging and lifecycle behavior. They do not claim that a live
//! host selected the skill or that any model reasoned correctly.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::{fs, io::Write};

const PRESERVATION: &str = include_str!("../skills/exitbind/references/preservation.md");
const LEGACY: &str = "holytail";

struct Fixture {
    home: PathBuf,
    root: PathBuf,
    path: String,
}

impl Fixture {
    /// An isolated home and project: nothing here can inherit a standalone
    /// preservation installation from the developer's machine.
    fn new(label: &str) -> Self {
        let base = support::temp(label);
        let home = base.join("home");
        let root = base.join("project");
        let bin = base.join("bin");
        for path in [&home, &root, &bin] {
            fs::create_dir(path).unwrap();
        }
        // Installing the session hook asks the CLI on PATH for its protocol
        // token, so the fixture must supply the binary under test rather than
        // depending on whatever the machine has installed.
        support::place_executable(
            Path::new(env!("CARGO_BIN_EXE_exitbind")),
            &bin.join("exitbind"),
        );
        let fixture = Self {
            home,
            root,
            path: format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        };
        let init = fixture.call(&["init", "--root", "."], None);
        assert!(init.status.success(), "{init:?}");
        fixture
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .env("HOME", &self.home)
            .env("PATH", &self.path)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !args.starts_with(&["init"]) && !args.starts_with(&["host"]) {
            command.arg("--config").arg(self.root.join("exitbind.json"));
        }
        if let Some(bytes) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            return child.wait_with_output().unwrap();
        }
        support::run(&mut command)
    }

    fn value(&self, args: &[&str], input: Option<&[u8]>) -> Value {
        let output = self.call(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Start a governed run whose functional check passes and whose accepted
    /// requirement is checked separately.
    fn begin(&self, preservation_check: &str) -> String {
        fs::write(self.root.join("source.txt"), b"env-first\n").unwrap();
        write_script(
            &self.root.join("check.sh"),
            "#!/bin/sh\ngrep -q env-first source.txt\n",
        );
        self.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "keep the accepted precedence",
                "--check-command",
                "sh check.sh",
                "--preserve-requirement",
                "precedence:environment settings win over file settings",
                "--preservation-check-command",
                preservation_check,
            ],
            None,
        )["work"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Record the acceptance the run is waiting for, then read the result.
    fn accept(&self, work: &str, pending: &Value) -> Value {
        let assignment = pending["next"]["assignment"].as_str().unwrap().to_owned();
        self.value(
            &["work", "return", work, &assignment, "--outcome", "accepted"],
            Some(b"accept"),
        );
        self.value(&["work", "next", work], None)
    }

    /// Drive the façade until the acceptance decision is pending.
    fn drive(&self, work: &str) -> Value {
        for _ in 0..16 {
            let seen = self.value(&["work", "next", work], None);
            let next = seen["next"].clone();
            let scope_stage = next["outcomes"][0] == "scoped";
            match next["action"].as_str().unwrap() {
                "lead_decision" if !scope_stage => return seen,
                "check" => {
                    self.value(&["work", "check", work], None);
                }
                "spawn" => {
                    let outcome = if next["role"] == "reviewer" {
                        "approved"
                    } else {
                        "completed"
                    };
                    self.value(
                        &[
                            "work",
                            "return",
                            work,
                            next["assignment"].as_str().unwrap(),
                            "--outcome",
                            outcome,
                        ],
                        Some(b"scripted"),
                    );
                }
                "lead_decision" => {
                    self.value(
                        &[
                            "work",
                            "return",
                            work,
                            next["assignment"].as_str().unwrap(),
                            "--outcome",
                            "scoped",
                        ],
                        Some(b"scope"),
                    );
                }
                other => panic!("unexpected action {other}"),
            }
        }
        panic!("the run never reached an acceptance decision")
    }
}

fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body.as_bytes()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Every path under `root`, relative, for an identity scan.
fn paths(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_owned()];
    while let Some(at) = stack.pop() {
        let Ok(entries) = fs::read_dir(&at) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                stack.push(path.clone());
            }
            found.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }
    found
}

#[test]
fn preservation_guidance_ships_with_exitbind_and_needs_no_second_installation() {
    let fixture = Fixture::new("preservation-packaging");
    let installed = fixture.call(&["host", "install", "--all"], None);
    assert!(installed.status.success(), "{installed:?}");

    // The delayed reference resolves beside the skill that links to it, in
    // both host locations, as the exact bytes the binary carries.
    for base in [".agents/skills", ".claude/skills"] {
        let skill = fixture.root.join(base).join("exitbind/SKILL.md");
        let reference = fixture
            .root
            .join(base)
            .join("exitbind/references/preservation.md");
        let skill_text = fs::read_to_string(&skill).unwrap();
        assert!(
            skill_text.contains("references/preservation.md"),
            "{base}: the skill does not point at its preservation detail"
        );
        assert_eq!(
            fs::read_to_string(&reference).unwrap(),
            PRESERVATION,
            "{base}: projected preservation detail differs from the packaged bytes"
        );
    }
    assert!(PRESERVATION.contains("second instruction source"));
    assert!(PRESERVATION.contains("every `FORMAL` task is assigned `FULL`"));

    // Nothing in the isolated home or project carries a second preservation
    // installation, and nothing created one.
    for root in [&fixture.home, &fixture.root] {
        let leaked: Vec<String> = paths(root)
            .into_iter()
            .filter(|path| path.to_lowercase().contains(LEGACY))
            .collect();
        assert!(
            leaked.is_empty(),
            "a standalone preservation path exists in the fixture: {leaked:?}"
        );
    }
}

#[test]
fn a_governed_run_resolves_its_own_route_and_quality() {
    let fixture = Fixture::new("preservation-assignment");
    let work = fixture.begin("sh check.sh");
    let seen = fixture.drive(&work);

    let assignment = &seen["residual"]["humanHelp"]["preservationAssignment"];
    assert_eq!(assignment["route"], "FORMAL");
    assert_eq!(assignment["quality"], "FULL");
    assert_eq!(
        assignment["resolvedBy"],
        "accepted_preservation_requirements"
    );
    // Exitbind records the assignment; it never claims to enforce a host's mode.
    assert_eq!(assignment["enforcement"], "recorded_not_enforced");

    assert!(seen["presentation"]["holytail"].is_null());

    // Ungoverned reading keeps the line off entirely.
    let quiet = Fixture::new("preservation-quiet");
    let plain = quiet.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "no accepted requirement",
            "--check-command",
            "true",
        ],
        None,
    )["work"]
        .as_str()
        .unwrap()
        .to_owned();
    let presentation = quiet.value(&["work", "next", &plain], None)["presentation"].clone();
    assert!(presentation["holytail"].is_null());
}

#[test]
fn a_broken_invariant_is_caught_while_the_functional_check_passes() {
    // The control: the same shape of run, with the requirement genuinely held.
    let preserved = Fixture::new("preservation-control");
    let held = preserved.begin("sh check.sh");
    let pending = preserved.drive(&held);
    assert_eq!(
        pending["presentation"]["state"]["preservation"],
        "satisfied"
    );
    let ready = preserved.accept(&held, &pending);
    assert_eq!(ready["presentation"]["exitState"], "READY");

    // The loss: a functional check that still passes over a requirement that
    // no longer holds. Rejecting everything would fail the control above.
    let broken = Fixture::new("preservation-loss");
    let work = broken.begin("sh preservation.sh");
    write_script(
        &broken.root.join("preservation.sh"),
        "#!/bin/sh\ngrep -q 'environment wins' precedence.md\n",
    );
    let seen = broken.drive(&work);
    let presentation = &seen["presentation"];
    assert_ne!(presentation["exitState"], "READY");
    assert_eq!(presentation["state"]["preservation"], "failed");
    assert_eq!(presentation["state"]["check"], "current");

    let help = &seen["residual"]["humanHelp"];
    let what = help["whatHappened"].as_str().unwrap();
    assert!(
        what.contains("the functional check passed"),
        "the report must separate the passing functional check from the failure: {what}"
    );
    assert!(
        what.contains("broken checker or environment"),
        "a failing checker is not proof of a broken product: {what}"
    );

    // Acceptance cannot be recorded over the failure.
    let assignment = seen["next"]["assignment"].as_str().unwrap().to_owned();
    let refused = broken.call(
        &[
            "work",
            "return",
            &work,
            &assignment,
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(!refused.status.success());

    // No parallel authority file appeared for either run.
    for fixture in [&preserved, &broken] {
        assert!(!fixture.root.join(".holytail").exists());
        assert!(!fixture.root.join(".holytail/accepted.md").exists());
    }
}

#[test]
fn an_unrelated_preservation_installation_is_neither_used_nor_disturbed() {
    let fixture = Fixture::new("preservation-migration");
    // A standalone installation as it would be registered on a host: its own
    // skill and its own session-hook record.
    let foreign_skill = fixture.home.join(".claude/skills/holytail/SKILL.md");
    fs::create_dir_all(foreign_skill.parent().unwrap()).unwrap();
    fs::write(&foreign_skill, b"# someone else's skill\n").unwrap();
    let settings = fixture.home.join(".claude/settings.json");
    fs::write(
        &settings,
        br#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"holytail hook-run","timeout":5}]}]}}"#,
    )
    .unwrap();

    fixture.call(&["host", "install", "--all"], None);

    // Exitbind installs its own bridge and leaves the other installation alone:
    // it is not a second authority to absorb, and not Exitbind's to remove.
    assert!(fixture
        .home
        .join(".claude/skills/exitbind/SKILL.md")
        .is_file());
    assert_eq!(
        fs::read_to_string(&foreign_skill).unwrap(),
        "# someone else's skill\n"
    );
    let after: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    let records = after["hooks"]["SessionStart"].as_array().unwrap();
    let text = serde_json::to_string(records).unwrap();
    assert!(text.contains("holytail hook-run"), "{text}");
    assert!(text.contains("exitbind hook-run"), "{text}");

    // Retiring the standalone installation is the operator's exact removal of
    // its own registrations. Exitbind must not bring them back.
    fs::remove_dir_all(fixture.home.join(".claude/skills/holytail")).unwrap();
    let mut settings_value: Value =
        serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    let retained: Vec<Value> = settings_value["hooks"]["SessionStart"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| !serde_json::to_string(entry).unwrap().contains("holytail"))
        .cloned()
        .collect();
    settings_value["hooks"]["SessionStart"] = Value::Array(retained);
    fs::write(&settings, format!("{settings_value}\n")).unwrap();

    fixture.call(&["host", "install", "--all"], None);
    let restored = fs::read_to_string(&settings).unwrap();
    assert!(
        !restored.to_lowercase().contains(LEGACY),
        "installing Exitbind reintroduced a retired registration: {restored}"
    );
    assert!(!fixture.home.join(".claude/skills/holytail").exists());
    assert!(fixture
        .home
        .join(".claude/skills/exitbind/SKILL.md")
        .is_file());

    // The replacement still works with the old installation gone.
    let work = fixture.begin("sh check.sh");
    let pending = fixture.drive(&work);
    let ready = fixture.accept(&work, &pending);
    assert_eq!(ready["presentation"]["exitState"], "READY");
}
