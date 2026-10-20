//! Real CLI and launcher boundary; the provider fixture is a local process.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
    work: String,
    assignment: String,
    tool: PathBuf,
}
impl Fixture {
    fn new(broad: bool, copied: bool) -> Self {
        Self::with_observe(
            broad,
            copied,
            if broad {
                json!(["**"])
            } else {
                json!(["src/**"])
            },
        )
    }
    fn with_observe(broad: bool, copied: bool, observe: Value) -> Self {
        let root = support::temp(if copied {
            "managed edits 'quoted"
        } else {
            "managed-edits"
        });
        let executable = if copied {
            let path = root.join("exact candidate");
            support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &path);
            path
        } else {
            PathBuf::from(env!("CARGO_BIN_EXE_exitbind"))
        };
        let output = support::run(
            support::git_topology::command(&executable)
                .args(["init", "--mode", "portable", "--skip-skills", "--root"])
                .arg(&root),
        );
        assert!(output.status.success(), "{output:?}");
        let path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["agents"]["worker"]["observe"] = observe;
        config["agents"]["worker"]["write"] = if broad {
            json!(["**"])
        } else {
            json!(["src/**"])
        };
        fs::write(path, config.to_string()).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        let mut f = Self {
            root,
            executable,
            work: String::new(),
            assignment: String::new(),
            tool: PathBuf::new(),
        };
        let (work, assignment) = f.start();
        f.work = work;
        f.assignment = assignment;
        let prepared = f.ok(&["work", "file", "prepare", &f.work, &f.assignment], b"");
        f.tool = PathBuf::from(prepared["tool"].as_str().unwrap());
        f
    }
    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = support::git_topology::command(&self.executable)
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], input: &[u8]) -> Value {
        value(self.call(args, input))
    }
    fn start(&self) -> (String, String) {
        let begin = self.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "managed source edit",
                "--check-command",
                "true",
                "--review-policy",
                "omitted",
            ],
            b"",
        );
        let work = begin["work"].as_str().unwrap().to_owned();
        let lead = begin["next"]["assignment"].as_str().unwrap();
        self.ok(
            &["work", "return", &work, lead, "--outcome", "scoped"],
            b"scope",
        );
        let next = self.ok(&["work", "next", &work, "--full"], b"");
        (
            work,
            next["next"]["assignment"].as_str().unwrap().to_owned(),
        )
    }
    fn permit(&self) {
        assert_eq!(
            self.ok(
                &[
                    "work",
                    "permit",
                    &self.work,
                    &self.assignment,
                    "--operation",
                    "source-change"
                ],
                b""
            )["allowed"],
            true
        );
    }
    fn edit(&self, action: &str, path: &str, content: &[u8]) -> Output {
        let mut child = support::git_topology::command(&self.tool)
            .args([action, path])
            .current_dir(std::env::temp_dir())
            .env("PATH", "/missing-native-tools")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(content).unwrap();
        child.wait_with_output().unwrap()
    }
    fn edit_ok(&self, action: &str, path: &str, content: &[u8]) -> Value {
        value(self.edit(action, path, content))
    }
    fn reads_path(&self) -> PathBuf {
        self.tool.parent().unwrap().join("reads.json")
    }
    fn effect_path(&self) -> PathBuf {
        let paths = fs::read_dir(self.root.join(".exitbind/effects"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 1);
        paths[0].clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn value(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}
fn refused(output: Output, expected: &str) {
    assert!(!output.status.success(), "unexpected success: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "{output:?}"
    );
}

#[path = "managed_edits/managed_reads_and_writes.rs"]
mod managed_reads_and_writes;
#[path = "managed_edits/recovery_and_native_launch.rs"]
mod recovery_and_native_launch;
