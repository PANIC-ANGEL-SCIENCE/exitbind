//! Packaged CLI scenario with real checks. Role returns below are deterministic
//! fixtures, not claims of independent semantic review, model use or adoption.
#![cfg(unix)]
mod support;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

const REQUIREMENTS: [(&str, &str); 3] = [
    ("R1", "Reject invalid input before persistence."),
    ("R2", "Persist each valid name only once."),
    ("R3", "Show the number of distinct imported names."),
];

struct Project {
    root: PathBuf,
}
impl Project {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let mut init = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        init.args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .arg("--skip-skills");
        let output = support::run(&mut init);
        assert!(output.status.success(), "{output:?}");
        Self { root }
    }
    fn call(&self, args: &[&str], input: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.stdin(Stdio::null());
            support::run(&mut command)
        }
    }
    fn value(&self, args: &[&str], input: Option<&str>) -> Value {
        let output = self.call(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn reject(&self, args: &[&str], reason: &str) {
        let output = self.call(args, None);
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason)
                || String::from_utf8_lossy(&output.stdout).contains(reason),
            "{output:?}"
        );
    }
    fn file(&self, name: &str, contents: &str) {
        fs::write(self.root.join(name), contents).unwrap();
    }
    fn setup(&self) {
        self.file(
            "request.txt",
            &REQUIREMENTS
                .iter()
                .map(|(_, text)| *text)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        self.file("format.txt", ",");
        self.file("data.txt", "alpha,x\nalpha,y\nbeta,x\n");
        self.file(
            "validate.sh",
            "set -eu\nawk -F \"$(cat format.txt)\" '{if ($1 == \"bad\") exit 2; print $1}'\n",
        );
        self.file("persist.sh", "set -eu\nsort -u\n");
        self.file("render.sh", "set -eu\nawk 'END {print NR \" items\"}'\n");
        self.file(
            "check.sh",
            r#"set -eu
case "$1" in
R1) if printf 'bad\n' | sh validate.sh > /dev/null; then exit 3; fi
    test "$(printf 'alpha\n' | sh validate.sh)" = alpha ;;
R2) test "$(printf 'alpha\nalpha\n' | sh persist.sh)" = alpha ;;
R3) test "$(printf 'alpha\nbeta\n' | sh render.sh)" = '2 items' ;;
all) sh check.sh R1; sh check.sh R2; sh check.sh R3
     test "$(sh validate.sh < data.txt | sh persist.sh | sh render.sh)" = '2 items' ;;
esac
"#,
        );
        self.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                "Import names safely and show a count",
                "--none-applicable",
                "findings,blockers,decisions,externalActions",
            ],
            None,
        );
        for (id, text) in REQUIREMENTS {
            self.value(
                &[
                    "goal",
                    "require",
                    "--goal-id",
                    "import",
                    "--requirement",
                    id,
                    "--obligation",
                    text,
                    "--artifact",
                    "request.txt",
                ],
                None,
            );
        }
        self.value(&["goal", "cover", "--goal-id", "import"], None);
    }
    fn status(&self) -> Value {
        self.value(&["goal", "status", "--json"], None)
    }
    fn begin(&self, ids: &str, command: &str) -> String {
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            "Check the declared import requirements",
            "--goal-id",
            "import",
            "--requirement",
            ids,
            "--artifact",
            "check.sh",
            "--check-command",
            command,
            "--review-policy",
            "required",
        ];
        if command == "sh check.sh all" {
            args.extend(["--scope", "integration"]);
        }
        let started = self.value(&args, None);
        let work = started["work"].as_str().unwrap().to_owned();
        assert_eq!(started["next"]["goalProgress"]["decomposition"]["total"], 3);
        let current = self.value(&["work", "next", &work, "--json", "--full"], None);
        assert_eq!(
            started["next"]["current"]["binding"],
            current["next"]["current"]["binding"]
        );
        work
    }
    fn return_stage(&self, work: &str, next: &Value, outcome: &str) {
        self.value(
            &[
                "work",
                "return",
                work,
                next["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            Some("Deterministic process fixture decision; not a semantic review.\n"),
        );
    }
    fn drive(&self, work: &str, passing: bool) {
        for _ in 0..12 {
            let detail = self.value(&["work", "next", work, "--json", "--full"], None);
            let next = &detail["next"];
            match next["action"].as_str().unwrap() {
                "lead_decision" => {
                    let scoped = next["outcomes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item == "scoped");
                    self.return_stage(work, next, if scoped { "scoped" } else { "accepted" });
                }
                "spawn" if next["role"] == "worker" => {
                    self.value(
                        &[
                            "work",
                            "permit",
                            work,
                            next["assignment"].as_str().unwrap(),
                            "--operation",
                            "Return fixture evidence",
                        ],
                        None,
                    );
                    self.return_stage(work, next, "completed");
                }
                "spawn" => self.return_stage(work, next, "approved"),
                "check" => {
                    let output = self.call(&["work", "check", work], None);
                    assert_eq!(output.status.success(), passing, "{output:?}");
                    let checked: Value = serde_json::from_slice(&output.stdout).unwrap();
                    assert_eq!(checked["effect"], "recorded");
                    if !passing {
                        return;
                    }
                }
                "done" => return,
                action => panic!("unexpected {action}: {detail}"),
            }
        }
        panic!("fixture did not finish");
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn row(status: &Value, id: &str) -> Value {
    status["requirements"]["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap()
        .clone()
}

#[test]
fn three_requirements_multiple_works_interruption_drift_composition_and_closure() {
    let p = Project::new("goal-import-scenario");
    p.setup();
    assert_eq!(row(&p.status(), "R3")["state"], "unmapped");
    let a = p.begin("R1,R2", "sh check.sh R1 && sh check.sh R2");
    p.drive(&a, true);
    let partial = p.status();
    assert_eq!(row(&partial, "R1")["state"], "current");
    assert_eq!(row(&partial, "R2")["state"], "current");
    assert_eq!(row(&partial, "R3")["state"], "unmapped");
    assert_eq!(partial["requirements"]["coverageCurrent"], false);
    p.reject(
        &["goal", "close", "--goal-id", "import", "--result-ref", &a],
        "coverage",
    );
    // Every call is a new CLI process. Recovery uses only persisted supported views.
    let recovered = p.status();
    assert_eq!(recovered["revision"], partial["revision"]);
    let detail = p.value(&["work", "next", &a, "--json", "--full"], None);
    assert_eq!(detail["next"]["goalProgress"]["decomposition"]["total"], 3);
    let b = p.begin("R3", "sh check.sh R3");
    p.drive(&b, true);
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    p.reject(
        &["goal", "close", "--goal-id", "import", "--result-ref", &b],
        "integration Work",
    );
    // The producer now uses another format. Unit checks still pass, composition fails.
    p.file("format.txt", "|");
    assert_eq!(row(&p.status(), "R1")["state"], "stale");
    let c = p.begin("R1,R2", "sh check.sh R1 && sh check.sh R2");
    p.drive(&c, true);
    let d = p.begin("R3", "sh check.sh R3");
    p.drive(&d, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2",
            "--result-ref",
            &c,
            "--disposition",
            "replace",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R3",
            "--result-ref",
            &d,
            "--disposition",
            "replace",
        ],
        None,
    );
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    let failed = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&failed, false);
    assert_eq!(row(&p.status(), "R1")["state"], "failed");
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &failed,
        ],
        "coverage",
    );
    // Repair shared data, create a legitimate validation Work, keep old Works immutable.
    p.file("data.txt", "alpha|x\nalpha|y\nbeta|x\n");
    let integrated = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&integrated, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integrated,
            "--disposition",
            "replace",
        ],
        None,
    );
    let before_repeat = p.status()["revision"].clone();
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integrated,
            "--disposition",
            "replace",
        ],
        None,
    );
    assert_eq!(p.status()["revision"], before_repeat);
    p.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &integrated,
        ],
        None,
    );
    let closed = p.status();
    assert_eq!(closed["currentReadiness"]["state"], "current");
    for (id, _) in REQUIREMENTS {
        let evidence = &row(&closed, id)["support"][0]["evidence"];
        assert_eq!(evidence["check"]["acquisition"], "observed");
        assert_eq!(evidence["accepted"], true);
    }
    println!("Process-only scenario: 3 requirements; multi-Work; partial; process recovery; shared-input drift; failed composition; new checked integration; fixture review/Lead; explicit current closure.");
    p.file("format.txt", ",");
    let stale = p.status();
    assert_eq!(stale["closure"], closed["closure"]);
    assert_eq!(stale["currentReadiness"]["state"], "stale");
}

#[test]
fn correction_retains_meaning_history_and_refuses_unrelated_or_old_work() {
    let p = Project::new("goal-correction");
    p.setup();
    let a = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&a, true);
    let before = p.status()["revision"].clone();
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            REQUIREMENTS[0].1,
            "--artifact",
            "request.txt",
        ],
        None,
    );
    assert_eq!(p.status()["revision"], before);
    let corrected = "Reject malformed input before persistence.";
    p.file(
        "request.txt",
        &format!(
            "{corrected}\n{}\n{}\n",
            REQUIREMENTS[1].1, REQUIREMENTS[2].1
        ),
    );
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            corrected,
            "--artifact",
            "request.txt",
        ],
        None,
    );
    let changed = p.status();
    assert_eq!(row(&changed, "R1")["revision"], 2);
    assert_eq!(row(&changed, "R1")["history"][0]["text"], REQUIREMENTS[0].1);
    assert_eq!(changed["goalProgress"]["decomposition"]["total"], 3);
    assert_eq!(changed["requirements"]["coverageConfirmed"], false);
    p.reject(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--result-ref",
            &a,
        ],
        "exact requirement revision",
    );
    let unrelated = p.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "unrelated pass",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
        ],
        None,
    )["work"]
        .as_str()
        .unwrap()
        .to_owned();
    p.drive(&unrelated, true);
    p.reject(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R2",
            "--result-ref",
            &unrelated,
        ],
        "frozen named requirement",
    );
    p.reject(
        &["goal", "close", "--goal-id", "import", "--direct"],
        "named requirements",
    );
}

#[test]
fn checker_coverage_and_intake_are_guarded_before_start() {
    let p = Project::new("goal-checker-coverage");
    p.reject(
        &[
            "goal",
            "require",
            "--goal-id",
            "missing",
            "--requirement",
            "R1",
            "--obligation",
            "x",
            "--artifact",
            "missing.txt",
        ],
        "source",
    );
    p.setup();
    p.reject(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R4",
            "--obligation",
            "invented requirement",
            "--artifact",
            "request.txt",
        ],
        "exact excerpt",
    );
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "wrong checker",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
        "name its declared",
    );
    let mut git = Command::new("git");
    git.current_dir(&p.root).args(["init", "-q"]);
    assert!(support::run(&mut git).status.success());
    p.file(".gitignore", "check.sh\n");
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "ignored checker",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh R1",
            "--review-policy",
            "required",
        ],
        "outside tested",
    );
    assert_eq!(row(&p.status(), "R1")["state"], "unmapped");
    for child in ["require", "cover", "assign", "status"] {
        let help = p.call(&["goal", child, "--help"], None);
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains(child));
    }
}

#[test]
fn relocated_project_cannot_reuse_a_named_goal_identity() {
    let mut p = Project::new("goal-project-identity");
    p.setup();
    let relocated = support::temp("goal-other-root");
    fs::remove_dir(&relocated).unwrap();
    fs::rename(&p.root, &relocated).unwrap();
    p.root = relocated;
    p.reject(&["goal", "status", "--json"], "another project");
}

#[test]
fn full_mapping_of_individual_passes_is_not_an_integration_declaration() {
    let p = Project::new("goal-integration-declaration");
    p.setup();
    let work = p.begin(
        "R1,R2,R3",
        "sh check.sh R1 && sh check.sh R2 && sh check.sh R3",
    );
    p.drive(&work, true);
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &work,
        ],
        "integration Work",
    );
    let next = p.status()["requirements"]["nextAction"]["command"].clone();
    assert!(next
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "integration"));
}
