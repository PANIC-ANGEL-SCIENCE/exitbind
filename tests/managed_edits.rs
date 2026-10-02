//! Real CLI and launcher boundary; the provider fixture is a local process.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
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
            Command::new(&executable)
                .args(["init", "--mode", "portable", "--skip-skills", "--root"])
                .arg(&root),
        );
        assert!(output.status.success(), "{output:?}");
        let path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["agents"]["worker"]["observe"] = if broad {
            json!(["**"])
        } else {
            json!(["src/**"])
        };
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
        let mut child = Command::new(&self.executable)
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
        let mut child = Command::new(&self.tool)
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

#[test]
fn normal_create_replace_replay_and_check_progression() {
    let f = Fixture::new(false, false);
    f.permit();
    assert_eq!(f.edit_ok("read", "src/a.txt", b"")["exists"], false);
    let first = f.edit_ok("edit", "src/a.txt", b"hello");
    assert_eq!(first["effect"], "file-replaced");
    assert!(first.get("operation").is_none());
    let inode = fs::metadata(f.root.join("src/a.txt")).unwrap().ino();
    assert_eq!(
        f.edit_ok("edit", "src/a.txt", b"hello")["effect"],
        "no-change"
    );
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
    refused(f.edit("edit", "src/a.txt", b"different"), "conflicts");
    fs::set_permissions(f.root.join("src/a.txt"), fs::Permissions::from_mode(0o640)).unwrap();
    assert_eq!(f.edit_ok("refresh", "src/a.txt", b"")["content"], "hello");
    f.edit_ok("edit", "src/a.txt", b"next");
    assert_eq!(
        fs::metadata(f.root.join("src/a.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    f.ok(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
        ],
        b"source changed",
    );
    f.ok(&["work", "check", &f.work], b"");
    let next = f.ok(&["work", "next", &f.work, "--full"], b"");
    assert_eq!(next["next"]["role"], "lead");
    let lead = next["next"]["assignment"].as_str().unwrap();
    f.ok(
        &["work", "return", &f.work, lead, "--outcome", "accepted"],
        b"checked",
    );
    refused(
        f.edit("edit", "src/a.txt", b"next"),
        "current worker assignment",
    );
}

#[test]
fn stale_and_absent_read_conflicts_preserve_external_creator() {
    let f = Fixture::new(false, false);
    f.permit();
    fs::write(f.root.join("src/a.txt"), "A").unwrap();
    assert_eq!(f.edit_ok("read", "src/a.txt", b"")["content"], "A");
    fs::write(f.root.join("src/a.txt"), "B").unwrap();
    assert_eq!(f.edit_ok("read", "src/a.txt", b"")["content"], "A");
    refused(
        f.edit("edit", "src/a.txt", b"based on A"),
        "differs from expected",
    );
    assert_eq!(fs::read_to_string(f.root.join("src/a.txt")).unwrap(), "B");
    assert_eq!(
        f.edit_ok("inspect", "src/a.txt", b"")["requestStatus"],
        "refused"
    );
    assert_eq!(f.edit_ok("refresh", "src/a.txt", b"")["content"], "B");
    f.edit_ok("edit", "src/a.txt", b"based on B");
    f.edit_ok("read", "src/new.txt", b"");
    fs::write(f.root.join("src/new.txt"), "other creator").unwrap();
    refused(
        f.edit("edit", "src/new.txt", b"creation"),
        "differs from expected",
    );
    assert_eq!(
        fs::read_to_string(f.root.join("src/new.txt")).unwrap(),
        "other creator"
    );
}

#[test]
fn focus_is_navigation_and_grants_and_config_cannot_be_inherited() {
    let f = Fixture::new(false, false);
    f.edit_ok("read", "src/a.txt", b"");
    refused(
        f.edit("edit", "src/a.txt", b"needs grant"),
        "mutation permit",
    );
    assert!(!f.root.join("src/a.txt").exists());
    f.permit();
    f.edit_ok("refresh", "src/a.txt", b"");
    let (other, assignment) = f.start();
    let tool = f.ok(&["work", "file", "prepare", &other, &assignment], b"");
    let other_tool = PathBuf::from(tool["tool"].as_str().unwrap());
    f.edit_ok("edit", "src/a.txt", b"first work");
    value(
        Command::new(&other_tool)
            .args(["read", "src/b.txt"])
            .output()
            .unwrap(),
    );
    let mut child = Command::new(&other_tool)
        .args(["edit", "src/b.txt"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"second work")
        .unwrap();
    refused(child.wait_with_output().unwrap(), "mutation permit");
    assert!(!f.root.join("src/b.txt").exists());
    let config = f.root.join("exitbind.json");
    let mut bytes = fs::read(&config).unwrap();
    bytes.push(b' ');
    fs::write(config, bytes).unwrap();
    refused(
        f.edit("edit", "src/a.txt", b"first work"),
        "binding changed",
    );
    assert_eq!(
        fs::read_to_string(f.root.join("src/a.txt")).unwrap(),
        "first work"
    );
}

#[test]
fn exact_executable_and_config_survive_hostile_path_and_cwd() {
    let f = Fixture::new(false, true);
    f.permit();
    f.edit_ok("read", "src/a.txt", b"");
    f.edit_ok("edit", "src/a.txt", b"pinned");
    let replacement = f.root.join("replacement executable");
    fs::copy(&f.executable, &replacement).unwrap();
    let mut bytes = fs::read(&replacement).unwrap();
    bytes.extend_from_slice(b"changed digest");
    fs::write(&replacement, bytes).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(replacement, &f.executable).unwrap();
    refused(f.edit("read", "src/a.txt", b""), "binding changed");
}

#[test]
fn managed_reads_enforce_observe_scope_utf8_bounds_and_control_aliases() {
    let f = Fixture::new(true, false);
    f.permit();
    for file in [
        "../escape",
        "/absolute",
        "src//alias",
        "src/../alias",
        "src/.codex/config.toml",
        "src/AGENTS.md",
        "src/.exitbind/log",
        "src/CLAUDE.md",
    ] {
        refused(
            f.edit("read", file, b""),
            if file.starts_with("src/.codex")
                || file.ends_with("AGENTS.md")
                || file.contains(".exitbind")
                || file.ends_with("CLAUDE.md")
            {
                "cannot replace"
            } else {
                "normalized"
            },
        );
    }
    fs::hard_link(
        f.root.join("exitbind.json"),
        f.root.join("src/control-alias"),
    )
    .unwrap();
    refused(f.edit("read", "src/control-alias", b""), "cannot replace");
    std::os::unix::fs::symlink(f.root.join("exitbind.json"), f.root.join("src/link")).unwrap();
    refused(f.edit("read", "src/link", b""), "unsafe");
    fs::write(f.root.join("src/binary"), [0xff]).unwrap();
    refused(f.edit("read", "src/binary", b""), "UTF-8");
    fs::write(f.root.join("src/large"), vec![b'a'; 256 * 1024 + 1]).unwrap();
    refused(f.edit("read", "src/large", b""), "unreadable");
    let narrow = Fixture::new(false, false);
    refused(narrow.edit("read", "other.txt", b""), "read boundary");
    narrow.edit_ok("read", "src/a.txt", b"");
    refused(narrow.edit("edit", "src/a.txt", &[0xff]), "UTF-8");
    refused(
        narrow.edit("edit", "src/a.txt", &vec![b'a'; 256 * 1024 + 1]),
        "256 KiB",
    );
}

#[test]
fn lost_reply_and_uncertain_or_missing_effects_never_start_new_request() {
    let f = Fixture::new(false, false);
    f.permit();
    f.edit_ok("read", "src/a.txt", b"");
    f.edit_ok("edit", "src/a.txt", b"once");
    let reads_path = f.reads_path();
    let mut reads: Value = serde_json::from_slice(&fs::read(&reads_path).unwrap()).unwrap();
    reads["files"]["src/a.txt"]["request"]["status"] = json!("submitted");
    fs::write(&reads_path, reads.to_string()).unwrap();
    let inode = fs::metadata(f.root.join("src/a.txt")).unwrap().ino();
    assert_eq!(
        f.edit_ok("edit", "src/a.txt", b"once")["effect"],
        "no-change"
    );
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
    let effect = f.effect_path();
    let completed = fs::read(&effect).unwrap();
    let mut pending: Value = serde_json::from_slice(&completed).unwrap();
    pending["status"] = json!("admitted");
    fs::write(&effect, pending.to_string()).unwrap();
    refused(f.edit("edit", "src/a.txt", b"once"), "unresolved");
    refused(f.edit("refresh", "src/a.txt", b""), "uncertain");
    assert_eq!(
        f.edit_ok("inspect", "src/a.txt", b"")["effectStatus"],
        "admitted"
    );
    fs::write(&effect, b"{").unwrap();
    refused(f.edit("edit", "src/a.txt", b"once"), "corrupt");
    refused(f.edit("refresh", "src/a.txt", b""), "corrupt");
    fs::write(&effect, completed).unwrap();
    fs::remove_file(effect).unwrap();
    refused(f.edit("edit", "src/a.txt", b"once"), "missing effect");
    refused(
        f.edit("refresh", "src/a.txt", b""),
        "effect record is missing",
    );
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
}

#[test]
fn missing_corrupt_read_and_binding_state_cannot_be_recreated() {
    for corruption in [false, true] {
        let f = Fixture::new(false, false);
        f.permit();
        f.edit_ok("read", "src/a.txt", b"");
        if corruption {
            fs::write(f.reads_path(), b"{").unwrap();
        } else {
            fs::remove_file(f.reads_path()).unwrap();
        }
        for action in ["read", "refresh", "edit", "inspect"] {
            refused(
                f.edit(action, "src/a.txt", b"content"),
                if corruption { "corrupt" } else { "missing" },
            );
        }
        refused(
            f.call(&["work", "file", "prepare", &f.work, &f.assignment], b""),
            if corruption { "corrupt" } else { "missing" },
        );
        assert!(!f.root.join("src/a.txt").exists());
    }
    let f = Fixture::new(false, false);
    fs::remove_file(f.tool.parent().unwrap().join("binding.json")).unwrap();
    refused(f.edit("read", "src/a.txt", b""), "binding is missing");
    refused(
        f.call(&["work", "file", "prepare", &f.work, &f.assignment], b""),
        "binding is missing",
    );
}

#[test]
fn supported_native_launch_delivers_and_uses_real_managed_tool() {
    let f = Fixture::new(false, false);
    f.permit();
    let fake = f.root.join("fake-codex");
    let capture = f.root.join("prompt");
    let script = format!(
        r##"#!/bin/sh
/bin/cat > '{}'
tool=''
while IFS= read -r line; do
  case "$line" in 'MANAGED FILE TOOL: '*) tool=${{line#MANAGED FILE TOOL: }} ;; esac
done < '{}'
test -n "$tool" || exit 90
"$tool" read src/native.txt >/dev/null || exit 91
printf '%s' 'native replacement' | "$tool" edit src/native.txt >/dev/null || exit 92
printf '%s\n' '{{"type":"thread.started","thread_id":"managed-native-fixture"}}' '{{"type":"item.completed","item":{{"type":"agent_message","text":"{{\"outcome\":\"completed\",\"summary\":\"managed native edit\",\"reason\":\"\"}}"}}}}' '{{"type":"turn.completed","status":"completed","usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}}}'
"##,
        capture.display(),
        capture.display()
    );
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    f.ok(
        &[
            "work",
            "act",
            &f.work,
            "--codex-bin",
            fake.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(
        fs::read_to_string(f.root.join("src/native.txt")).unwrap(),
        "native replacement"
    );
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(prompt.contains("MANAGED FILE TOOL:"));
    assert!(!prompt.contains("--expected-sha256"));
    assert!(!prompt.contains("--operation STABLE_ID"));
    f.ok(&["work", "check", &f.work], b"");
}
