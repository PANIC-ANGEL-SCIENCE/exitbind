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
    root: support::TempRoot,
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
            "id":"brief","kind":kind,"repository":if kind=="git" {json!(project.root.path())} else {Value::Null},
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
        self.command_for(env!("CARGO_BIN_EXE_exitbind"))
    }

    fn command_for(&self, executable: &str) -> Command {
        let mut command = support::git_topology::command(executable);
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
        let mut value = successful(self.call(args));
        if args.len() == 6
            && args[0] == "work"
            && args[1] == "detail"
            && args[3..] == ["--view", "action", "--json"]
        {
            self.capture_omitted_choices(args[2], &mut value);
        }
        value
    }

    fn exact_fixture_argv(&self, argv: &Value) -> Vec<String> {
        let argv = argv
            .as_array()
            .expect("complete fixture argv")
            .iter()
            .map(|part| part.as_str().expect("fixture argv is text").to_owned())
            .collect::<Vec<_>>();
        assert!(argv.len() >= 3, "incomplete fixture argv");
        assert_eq!(
            fs::canonicalize(&argv[0]).unwrap(),
            fs::canonicalize(env!("CARGO_BIN_EXE_exitbind")).unwrap(),
            "route must name the fixture executable"
        );
        assert_eq!(
            argv.iter()
                .filter(|part| part.as_str() == "--config")
                .count(),
            1
        );
        assert_eq!(argv[argv.len() - 2], "--config");
        let config = Path::new(argv.last().unwrap());
        let config = if config.is_absolute() {
            config.to_path_buf()
        } else {
            self.root.join(config)
        };
        assert_eq!(
            fs::canonicalize(config).unwrap(),
            fs::canonicalize(self.root.join("exitbind.json")).unwrap(),
            "route must name the fixture config"
        );
        argv
    }

    fn assert_current_choice(&self, choice: &Value, work: &str, binding: &str, assignment: &str) {
        let label = choice["label"].as_str().expect("selected outcome label");
        assert_eq!(choice["fill"]["choice"], label);
        assert_eq!(choice["placeholders"], json!(["REASON"]));
        assert_eq!(choice["command"]["sameExecutableRequired"], false);
        assert_eq!(choice["command"]["sameConfigRequired"], false);
        let argv = self.exact_fixture_argv(&choice["command"]["argv"]);
        assert_eq!(argv.len(), 13);
        assert_eq!(argv[1], "work");
        assert_eq!(argv[2], "return");
        assert_eq!(argv[3], work);
        assert_eq!(argv[4], assignment);
        assert_eq!(argv[5], "--current-binding");
        assert_eq!(argv[6], binding);
        assert_eq!(argv[7], "--outcome");
        assert_eq!(argv[8], label);
        assert_eq!(argv[9], "--reason");
        assert_eq!(argv[10], "<REASON>");
    }

    fn capture_omitted_choices(&self, work: &str, diagnostic: &mut Value) {
        assert_eq!(diagnostic["kind"], "work_action");
        assert_eq!(diagnostic["work"], work);
        let action = &diagnostic["effectiveAction"];
        let Some(choices) = action["choices"].as_array() else {
            return;
        };
        for choice in choices {
            assert!(choice.get("_fixtureBoundChoice").is_none());
        }
        if choices.is_empty() {
            return;
        }
        let binding = diagnostic["binding"].as_str().expect("current binding");
        assert!(!binding.is_empty());
        assert_eq!(action["work"], work);
        assert_eq!(action["binding"], binding);
        assert_eq!(diagnostic["recipient"]["actor"], action["actor"]);
        assert_eq!(diagnostic["recipient"]["role"], action["actor"]);
        assert_eq!(diagnostic["recipient"]["assignment"], action["assignment"]);
        assert!(matches!(
            action["actor"].as_str(),
            Some("lead" | "worker" | "reviewer")
        ));
        let assignment = action["assignment"].as_str().expect("current assignment");
        assert!(!assignment.is_empty());
        for choice in choices {
            if choice["command"]["argv"].is_array() {
                self.assert_current_choice(choice, work, binding, assignment);
            }
        }
        if choices
            .iter()
            .all(|choice| choice["command"]["argv"].is_array())
        {
            return;
        }
        assert_eq!(action["commandsComplete"], false);
        assert_eq!(
            action["commandsOmitted"],
            "complete commands omitted to fit the action view; use complete current detail"
        );
        for choice in choices {
            if choice["command"]["argv"].is_array() {
                continue;
            }
            assert!(choice["command"].get("argv").is_none());
            assert_eq!(choice["command"]["available"], false);
            assert_eq!(choice["command"]["sameExecutableRequired"], false);
            assert_eq!(choice["command"]["sameConfigRequired"], false);
            assert_eq!(
                choice["command"]["unavailableReason"],
                "complete route omitted at the response bound"
            );
        }
        let route = &diagnostic["details"]["grouped"];
        assert_eq!(route["kind"], "work_detail");
        assert_eq!(route["readOnly"], true);
        assert_eq!(route["sameExecutableRequired"], false);
        assert_eq!(route["sameConfigRequired"], false);
        let reference = format!("ref:work-detail:v1:{binding}");
        assert_eq!(route["reference"], reference);
        let argv = self.exact_fixture_argv(&route["command"]);
        assert_eq!(argv.len(), 8);
        assert_eq!(argv[1], "work");
        assert_eq!(argv[2], "expand");
        assert_eq!(argv[3], work);
        assert_eq!(argv[4], reference);
        assert_eq!(argv[5], "--json");
        let mut command = self.command_for(&argv[0]);
        command.args(&argv[1..]);
        let packet = successful(support::run(&mut command));
        assert_eq!(packet["kind"], "work_detail");
        assert_eq!(packet["complete"], true);
        assert_eq!(packet["valid"], true);
        assert_eq!(packet["work"], work);
        assert_eq!(packet["binding"], binding);
        assert_eq!(packet["reference"], reference);
        assert_eq!(packet["recipient"], diagnostic["recipient"]);
        assert_eq!(packet["recipientContext"]["complete"], true);
        assert_eq!(
            packet["recipientContext"]["role"],
            diagnostic["recipient"]["role"]
        );
        assert_eq!(
            packet["recipientContext"]["agent"],
            diagnostic["recipient"]["agent"]
        );
        let forms = &packet["actionForms"];
        assert_eq!(forms["work"], work);
        assert_eq!(forms["binding"], binding);
        assert_eq!(forms["actor"], action["actor"]);
        assert_eq!(forms["assignment"], assignment);
        assert_eq!(forms["state"], action["formState"]);
        let full_choices = forms["choices"].as_array().expect("current choices");

        // A caller may retain a choice and stop its mandate before executing it.
        // Capture the bound form now, without changing the diagnostic's omission.
        let mut recovered = Vec::new();
        for (index, choice) in choices.iter().enumerate() {
            if choice["command"]["argv"].is_array() {
                continue;
            }
            let label = choice["label"].as_str().expect("selected outcome label");
            let matches = full_choices
                .iter()
                .filter(|full| full["label"] == label)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1, "current outcome must be unique");
            let full = matches[0];
            let mut displayed = choice.clone();
            let mut actual = full.clone();
            displayed.as_object_mut().unwrap().remove("command");
            actual.as_object_mut().unwrap().remove("command");
            assert_eq!(displayed, actual, "current outcome facts must match");
            self.assert_current_choice(full, work, binding, assignment);
            recovered.push((index, full.clone()));
        }
        let choices = diagnostic["effectiveAction"]["choices"]
            .as_array_mut()
            .unwrap();
        for (index, full) in recovered {
            choices[index]["_fixtureBoundChoice"] = full;
        }
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
        let selected = if let Some(recovered) = form.get("_fixtureBoundChoice") {
            assert!(form["command"].get("argv").is_none());
            assert_eq!(form["command"]["available"], false);
            assert_eq!(form["command"]["sameExecutableRequired"], false);
            assert_eq!(form["command"]["sameConfigRequired"], false);
            assert_eq!(
                form["command"]["unavailableReason"],
                "complete route omitted at the response bound"
            );
            let mut displayed = form.clone();
            let mut actual = recovered.clone();
            displayed.as_object_mut().unwrap().remove("command");
            displayed
                .as_object_mut()
                .unwrap()
                .remove("_fixtureBoundChoice");
            actual.as_object_mut().unwrap().remove("command");
            assert_eq!(displayed, actual, "selected outcome must still match");
            recovered
        } else {
            form
        };
        assert_eq!(selected["command"]["sameExecutableRequired"], false);
        assert_eq!(selected["command"]["sameConfigRequired"], false);
        let argv = self.exact_fixture_argv(&selected["command"]["argv"]);
        let mut command = self.command_for(&argv[0]);
        command.args(argv.iter().skip(1).map(|value| match value.as_str() {
            "<OPERATION>" => "Return deterministic fixture evidence",
            "<REASON>" => "Deterministic protocol fixture; no native-agent claim",
            other => {
                assert!(
                    !(other.starts_with('<') && other.ends_with('>')),
                    "unknown action placeholder: {other}"
                );
                other
            }
        }));
        if let Some(input) = input {
            let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
            // A command may refuse and exit before reading stdin; its output is still asserted.
            if let Err(error) = child.stdin.take().unwrap().write_all(input.as_bytes()) {
                assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
            }
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
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
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
            let fresh = self.value(&["work", "detail", work, "--view", "action", "--json"]);
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
