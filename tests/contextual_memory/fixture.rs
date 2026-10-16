use super::*;
use std::io::Write;
use std::process::{Command, Output, Stdio};

pub const LEDGER: &str = ".exitbind/memory/lesson.jsonl";
pub const TASK: &str = "Check coupled documentation for this edit";
pub const FACT: &str = "Review coupled surfaces before returning the change.";

pub struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new(contextual: bool) -> Self {
        let root = support::temp("contextual-memory");
        support::git_topology::repository(&root);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--skip-skills", "--root"])
            .arg(&root)
            .env("EXITBIND_BINDINGS_DIR", root.join("fixture-bindings"))
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let f = Self { root };
        let mut c = f.config();
        c["memory"] = json!({"root":".exitbind/memory","maxItems":8,"maxBytes":32768,
            "protocolScopes":["project-lessons.v1"],"syntheticScopes":[]});
        for right in [
            "memoryWrite",
            "memoryReview",
            "memoryPromote",
            "memoryRevoke",
        ] {
            c["agents"]["lead"][right] = json!(["project-lessons.v1"]);
        }
        c["agents"]["worker"]["memoryRead"] = json!(["project-lessons.v1"]);
        c["agents"]["worker"]["crossContext"] = json!("protocol-only");
        if contextual {
            c["memory"]["contextual"] = json!({"version":1,"sourcePath":"current-task.json"});
        }
        f.save(&c);
        fs::write(f.root.join("guard"), b"coupled").unwrap();
        f.context(TASK, "r1", vec![]);
        f
    }

    pub fn config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("exitbind.json")).unwrap()).unwrap()
    }

    pub fn save(&self, c: &Value) {
        self.write("exitbind.json", c);
    }

    pub fn write(&self, path: &str, v: &Value) {
        fs::write(self.root.join(path), serde_json::to_vec(v).unwrap()).unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .env("EXITBIND_BINDINGS_DIR", self.root.join("fixture-bindings"));
        command
    }

    pub fn call(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    pub fn ok(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub fn input(&self, args: &[&str], input: &str) -> Value {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub fn lesson(&self) -> Value {
        let mut lesson = self.ok(&["memory", "form", "--scope", "project-lessons.v1", "--json"])
            ["template"]
            .clone();
        lesson["fact"] = json!(FACT);
        lesson["createdRevision"] = json!("a".repeat(40));
        lesson["revalidatedRevision"] = json!("b".repeat(40));
        lesson["appliesTo"] = json!({"agents":["worker"],"taskTerms":["release"]});
        lesson["guards"] = json!([{"path":"guard","sha256":digest(b"coupled")}]);
        lesson
    }

    pub fn accept_lesson(&self) -> Value {
        self.write("lesson.json", &self.lesson());
        self.accept("lead", "lesson.json", "project-lessons.v1", LEDGER, "lead");
        self.ok(&["memory", "inspect", LEDGER, "--json"])["items"][0].clone()
    }

    pub fn accept(&self, author: &str, source: &str, scope: &str, ledger: &str, reviewer: &str) {
        self.ok(&[
            "memory",
            "propose",
            author,
            source,
            "--scope",
            scope,
            "--ledger",
            ledger,
            "--expires-at",
            "2100-01-01T00:00:00Z",
        ]);
        self.ok(&["memory", "review", reviewer, ledger]);
        self.ok(&["memory", "promote", "lead", ledger]);
    }

    pub fn candidates(&self, agent: &str, task: &str) -> Value {
        self.ok(&[
            "memory",
            "candidates",
            agent,
            "--task",
            task,
            "--host",
            "codex",
            "--json",
        ])
    }

    pub fn resolve(&self, task: &str) -> Value {
        self.ok(&["memory", "resolve", "worker", "--task", task, "--json"])["references"].clone()
    }

    pub fn context(&self, task: &str, revision: &str, selections: Vec<Value>) {
        self.write(
            "current-task.json",
            &json!({"version":1,"task":task,"revision":revision,
            "approvedBy":"lead","selections":selections}),
        );
    }

    pub fn choice(&self, agent: &str, task: &str) -> Value {
        let candidates = self.candidates(agent, task);
        let mut choice = candidates["choiceTemplate"].clone();
        let r = &candidates["candidates"][0]["reference"];
        assert!(r["itemId"].is_string(), "{candidates}");
        choice["resources"] = json!([{"itemId":r["itemId"],"sourceSha256":r["sourceSha256"],
            "ledgerHeadSha256":r["ledgerHeadSha256"],"reason":"Lead reviewed this exact candidate for the current task."}]);
        choice
    }

    pub fn select(&self, task: &str) -> Value {
        let choice = self.choice("worker", task);
        self.context(task, "r1", vec![choice.clone()]);
        choice
    }

    pub fn worker(&self, task: &str) -> String {
        let begun = self.ok(&[
            "work",
            "begin",
            "change",
            "--goal",
            task,
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "facts:Keep reviewed facts scoped.",
            "--preservation-check-command",
            "true",
            "--json",
        ]);
        let work = begun["work"].as_str().unwrap();
        self.input(
            &[
                "work",
                "return",
                work,
                begun["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
                "--json",
            ],
            "Fixture bounded implementation scope.",
        );
        work.to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
