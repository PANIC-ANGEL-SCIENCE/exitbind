//! Deterministic admission fixtures, not native-agent or independent-review claims.
#![cfg(unix)]
mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const SPEC: &str = ".planning/spec.json";
const PLAN: &str = ".planning/plan.json";
const PRESERVE: &str = "Keep existing records.";
const OBLIGATION: &str = "Preserve valid records when importing.";

struct Project {
    root: PathBuf,
    spec: Value,
    plan: Value,
}

impl Project {
    fn new(label: &str, kind: &str) -> Self {
        let root = support::temp(label);
        support::git_topology::repository(&root);
        let mut init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        init.args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .arg("--skip-skills")
            .env("EXITBIND_NO_UPDATE_CHECK", "1");
        let output = support::run(&mut init);
        assert!(output.status.success(), "{output:?}");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".planning")).unwrap();
        fs::write(
            root.join("docs/brief.md"),
            format!("# Direction\n{OBLIGATION}\n{PRESERVE}\n"),
        )
        .unwrap();
        fs::write(root.join("src/import.txt"), "original records\n").unwrap();
        fs::write(root.join("checks.sh"), "set -eu\ntest -s src/import.txt\n").unwrap();
        let ignore = root.join(".gitignore");
        let mut contents = match fs::read_to_string(&ignore) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => panic!("read fixture ignore: {error}"),
        };
        contents.push_str("\n.planning/\n");
        fs::write(ignore, contents).unwrap();
        let config_path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        for agent in config["agents"].as_object_mut().unwrap().values_mut() {
            agent["observe"] = json!(["docs", "src", ".planning", "checks.sh"]);
            agent["write"] = json!(["src"]);
            agent["commands"] = json!(["sh checks.sh"]);
        }
        fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let mut project = Self {
            root,
            spec: Value::Null,
            plan: Value::Null,
        };
        project.git(&["add", "."]);
        project.git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "Initial fixture",
        ]);
        let revision = if kind == "git" {
            project.git(&["rev-parse", "HEAD"])
        } else {
            "supplied-v1".into()
        };
        let digest = format!(
            "{:x}",
            Sha256::digest(fs::read(project.root.join("docs/brief.md")).unwrap())
        );
        project.spec = json!({"version":1,"projectId":"record-import","mandate":{
            "id":"advance-records","revision":1,"outcome":"Import records without losing useful data.",
            "mode":"continue","allowedSources":["brief"],"write":["src"],"commands":["sh checks.sh"],
            "roles":["lead","worker","reviewer"],"preserve":[PRESERVE],
            "priorityPolicy":"Use current implementation and accepted Work evidence.",
            "expiresAt":"2099-01-01T00:00:00Z","maxWorks":3,"maxActions":12,"maxParallel":2,
            "maxElapsedSeconds":3600,"externalPublication":false},"sources":[{
            "id":"brief","kind":kind,"repository":if kind=="git" {json!(project.root)} else {Value::Null},
            "revision":revision,"path":"docs/brief.md","section":"# Direction","sha256":digest,
            "acquiredAt":"2026-01-01T00:00:00Z",
            "currentness":if kind=="git" {"checkout_head"} else {"declared_export"},"trust":"ownerDirection"}]});
        project.plan = json!({"version":1,"id":"first-plan","taskId":"preserve-records",
            "title":"Preserve valid records","rationale":"The current importer needs exact preservation evidence.",
            "outcome":"Import records without losing useful data.","acceptance":[OBLIGATION],
            "preservation":[PRESERVE],"sourceIds":["brief"],"dependencies":[],"workflow":"change",
            "write":["src"],"checkCommand":"sh checks.sh","preservationCheckCommand":"sh checks.sh",
            "estimatedActions":4,"overlapReason":"No prior claim owns this task; fixture exercises admission only.",
            "goalId":"record-import","requirements":["records"],"checker":"checks.sh","integration":false});
        project.save_spec();
        project.save_plan();
        project.value(&[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            "record-import",
            "--goal",
            "Import records without losing useful data.",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
        ]);
        project.value(&[
            "goal",
            "require",
            "--project",
            "--goal-id",
            "record-import",
            "--requirement",
            "records",
            "--obligation",
            OBLIGATION,
            "--artifact",
            "docs/brief.md",
        ]);
        project.value(&["goal", "cover", "--project", "--goal-id", "record-import"]);
        project
    }

    fn command(&self) -> Command {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env(
                "EXITBIND_SESSION_GOALS_DIR",
                self.root.join(".planning/native-goals"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn call(&self, args: &[&str]) -> Output {
        let mut command = self.command();
        command.args(args).args(["--config", "exitbind.json"]);
        // Work's native control forms are JSON by contract and do not accept
        // the optional rendering flag used by the other command families.
        if args.first() != Some(&"work") {
            command.arg("--json");
        }
        support::run(&mut command)
    }

    fn value(&self, args: &[&str]) -> Value {
        successful(self.call(args))
    }

    fn reject(&self, args: &[&str], reason: &str) {
        let output = self.call(args);
        assert!(
            !output.status.success(),
            "{args:?} unexpectedly succeeded: {output:?}"
        );
        assert!(
            message(&output).contains(reason),
            "expected {reason:?}: {output:?}"
        );
    }

    fn git(&self, args: &[&str]) -> String {
        let output = support::git_topology::git(&self.root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .into()
    }

    fn save_spec(&self) {
        fs::write(
            self.root.join(SPEC),
            serde_json::to_vec_pretty(&self.spec).unwrap(),
        )
        .unwrap();
    }

    fn refresh_brief_digest(&mut self) {
        let bytes = fs::read(self.root.join("docs/brief.md")).unwrap();
        self.spec["sources"][0]["sha256"] = json!(format!("{:x}", Sha256::digest(bytes)));
        self.save_spec();
    }

    fn save_plan(&self) {
        fs::write(
            self.root.join(PLAN),
            serde_json::to_vec_pretty(&self.plan).unwrap(),
        )
        .unwrap();
    }

    fn apply(&self, preview: &Value) -> Value {
        successful(self.apply_command(preview).output().unwrap())
    }

    fn apply_command(&self, preview: &Value) -> Command {
        let argv = preview["apply"]["command"]
            .as_array()
            .expect("exact apply argv");
        assert_eq!(argv[0], env!("CARGO_BIN_EXE_exitbind"));
        let mut command = self.command();
        command.args(argv.iter().skip(1).map(|v| v.as_str().unwrap()));
        command
    }

    fn execute_form(&self, form: &Value, input: Option<&str>) -> Value {
        successful(self.form_output(form, input))
    }

    fn form_output(&self, form: &Value, input: Option<&str>) -> Output {
        let mut argv = form["command"]["argv"]
            .as_array()
            .expect("exact action argv")
            .iter()
            .map(|value| value.as_str().expect("action argv is text").to_owned())
            .collect::<Vec<_>>();
        let expected_executable = fs::canonicalize(env!("CARGO_BIN_EXE_exitbind")).unwrap();
        let expected_config = fs::canonicalize(self.root.join("exitbind.json")).unwrap();
        if form["command"]["sameExecutableRequired"] == true {
            argv.insert(0, expected_executable.to_string_lossy().into_owned());
        } else {
            assert_eq!(
                fs::canonicalize(&argv[0]).unwrap(),
                expected_executable,
                "action form must name the fixture product executable"
            );
        }
        if form["command"]["sameConfigRequired"] == true {
            assert_eq!(argv.last().map(String::as_str), Some("--config"));
            argv.push(expected_config.to_string_lossy().into_owned());
        } else {
            assert!(argv.len() >= 2);
            assert_eq!(argv[argv.len() - 2], "--config");
            let config_arg = std::path::Path::new(argv.last().unwrap());
            let config_arg = if config_arg.is_absolute() {
                config_arg.to_path_buf()
            } else {
                self.root.join(config_arg)
            };
            assert_eq!(
                fs::canonicalize(config_arg).unwrap(),
                expected_config,
                "action form must retain the fixture config"
            );
        }
        let mut command = self.command();
        command.args(argv.iter().skip(1).map(|value| match value.as_str() {
            "<OPERATION>" => "Return deterministic fixture evidence",
            "<REASON>" => "Deterministic protocol fixture; no native-agent claim",
            other => other,
        }));
        if let Some(input) = input {
            let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    }

    fn call_input(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = self
            .command()
            .args(args)
            .args(["--config", "exitbind.json", "--json"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn accept_fixture_work(&self, work: &str) {
        for _ in 0..16 {
            let detail = self.value(&["work", "detail", work]);
            let current = self.value(&["work", "next", work, "--full"]);
            let next = &current["next"];
            let label = match next["action"].as_str().unwrap() {
                "done" => return,
                "check" => {
                    self.value(&["work", "check", work]);
                    continue;
                }
                "lead_decision" => {
                    if next["outcomes"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("scoped"))
                    {
                        "scoped"
                    } else {
                        "accepted"
                    }
                }
                "spawn" if next["role"] == "worker" => {
                    assert_eq!(detail["recipientContext"]["complete"], true);
                    let before = &detail["actionForms"]["beforeEditing"];
                    if before.is_object() {
                        assert_eq!(self.execute_form(before, None)["allowed"], true);
                    }
                    "completed"
                }
                "spawn" if next["role"] == "reviewer" => "approved",
                other => panic!("unexpected fixture action {other}: {current}"),
            };
            let fresh = self.value(&["work", "detail", work, "--view", "action"]);
            let forms = fresh
                .get("actionForms")
                .unwrap_or(&fresh["effectiveAction"]);
            let choice = forms["choices"]
                .as_array()
                .unwrap()
                .iter()
                .find(|choice| choice["label"] == label)
                .unwrap_or_else(|| panic!("missing {label}: {fresh}"));
            self.execute_form(
                choice,
                Some(
                    "Deterministic lifecycle fixture evidence; not independent semantic review.\n",
                ),
            );
        }
        panic!("fixture Work did not terminate");
    }

    fn select(&self) -> Value {
        let preview = self.value(&[
            "project",
            "advance",
            "select",
            SPEC,
            "--reason",
            "Explicit bounded fixture mandate.",
        ]);
        self.apply(&preview)
    }

    fn preview(&self) -> Value {
        self.value(&["project", "advance", "admit", SPEC, "--proposal", PLAN])
    }

    fn admit(&self) -> Value {
        self.apply(&self.preview())
    }

    fn status(&self) -> Value {
        self.value(&["project", "advance", "status", "advance-records"])
    }

    fn work_path(&self, work: &str) -> PathBuf {
        self.root.join(format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        ))
    }

    fn state(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &entry.path(), files);
                } else {
                    files.insert(
                        entry.path().strip_prefix(root).unwrap().into(),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(&self.root, &self.root.join(".exitbind"), &mut files);
        files
    }

    fn runs(&self) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(self.root.join(".exitbind/runs"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn message(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn successful(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {output:?}"))
}

#[path = "project_continuation/admission.rs"]
mod admission;
#[path = "project_continuation/authority.rs"]
mod authority;
#[path = "project_continuation/control.rs"]
mod control;
