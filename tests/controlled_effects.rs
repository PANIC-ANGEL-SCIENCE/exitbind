//! Real product stdio transport; callers cannot supply assignment authority.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    work: String,
    worker: String,
    session: String,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("controlled-file");
        support::git_topology::repository(&root);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--skip-skills", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let cfg = root.join("exitbind.json");
        let mut data: Value = serde_json::from_slice(&fs::read(&cfg).unwrap()).unwrap();
        data["agents"]["worker"]["observe"] = json!(["**"]);
        data["agents"]["worker"]["write"] = json!(["src/**"]);
        fs::write(cfg, data.to_string()).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        let mut f = Self {
            root,
            work: String::new(),
            worker: String::new(),
            session: String::new(),
        };
        let begin = f.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "controlled fixture",
                "--check-command",
                "true",
                "--review-policy",
                "omitted",
            ],
            b"",
        );
        f.work = begin["work"].as_str().unwrap().into();
        f.ok(
            &[
                "work",
                "return",
                &f.work,
                begin["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            b"fixture scope",
        );
        f.worker = f.ok(&["work", "next", &f.work, "--full"], b"")["next"]["assignment"]
            .as_str()
            .unwrap()
            .into();
        f.session = f.ok(&["work", "file", "prepare", &f.work, &f.worker], b"")["session"]
            .as_str()
            .unwrap()
            .into();
        f
    }
    fn call(&self, args: &[&str], body: &[u8]) -> Output {
        let mut c = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(body).unwrap();
        c.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], body: &[u8]) -> Value {
        let out = self.call(args, body);
        assert!(out.status.success(), "{out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn mcp(&self, args: &[Value]) -> Vec<Value> {
        let mut input = Vec::new();
        for a in args {
            serde_json::to_writer(&mut input, &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"file","arguments":a}})).unwrap();
            input.push(b'\n');
        }
        let out = self.call(&["work", "file-serve", &self.session], &input);
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}

#[test]
fn host_bound_transport_requires_current_grant_and_strict_parameters() {
    let f = Fixture::new();
    let before = f.mcp(&[
        json!({"action":"read","path":"src/a.txt"}),
        json!({"action":"edit","path":"src/a.txt","content":"no grant"}),
    ]);
    assert_eq!(before[0]["result"]["isError"], false);
    assert_eq!(before[1]["result"]["isError"], true);
    assert!(!f.root.join("src/a.txt").exists());
    f.ok(
        &[
            "work",
            "permit",
            &f.work,
            &f.worker,
            "--operation",
            "fixture",
        ],
        b"",
    );
    let results = f.mcp(&[
        json!({"action":"refresh","path":"src/a.txt"}),
        json!({"action":"edit","path":"src/a.txt","content":"wrong","owner":"lead"}),
        json!({"action":"edit","path":"src/a.txt","content":"wrong","assignment":"forged"}),
        json!({"action":"read","path":"exitbind.json"}),
        json!({"action":"read","path":".exitbind/grant.json"}),
        json!({"action":"read","path":"src/../alias"}),
        json!({"action":"edit","path":"src/a.txt","content":"once"}),
    ]);
    assert!(results[1].get("error").is_some());
    assert!(results[2].get("error").is_some());
    for item in &results[3..6] {
        assert_eq!(item["result"]["isError"], true);
    }
    assert_eq!(results[6]["result"]["isError"], false);
    assert_eq!(
        fs::read_to_string(f.root.join("src/a.txt")).unwrap(),
        "once"
    );
    let inode = fs::metadata(f.root.join("src/a.txt")).unwrap().ino();
    f.ok(
        &[
            "work",
            "return",
            &f.work,
            &f.worker,
            "--outcome",
            "completed",
        ],
        b"fixture result",
    );
    let stale = f.mcp(&[
        json!({"action":"edit","path":"src/late.txt","content":"late"}),
        json!({"action":"edit","path":"src/a.txt","content":"once"}),
        json!({"action":"edit","path":"src/a.txt","content":"different"}),
    ]);
    assert_eq!(stale[0]["result"]["isError"], true);
    assert_eq!(stale[1]["result"]["isError"], false);
    assert_eq!(stale[2]["result"]["isError"], true);
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
    assert!(!f.root.join("src/late.txt").exists());
}

#[test]
fn persistent_transport_observes_configuration_revocation_before_replay() {
    let f = Fixture::new();
    f.ok(
        &[
            "work",
            "permit",
            &f.work,
            &f.worker,
            "--operation",
            "fixture",
        ],
        b"",
    );
    let first = f.mcp(&[
        json!({"action":"read","path":"src/a.txt"}),
        json!({"action":"edit","path":"src/a.txt","content":"once"}),
    ]);
    assert_eq!(first[1]["result"]["isError"], false);
    let inode = fs::metadata(f.root.join("src/a.txt")).unwrap().ino();
    let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args([
            "work",
            "file-serve",
            &f.session,
            "--config",
            "exitbind.json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"})
    )
    .unwrap();
    input.flush().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert!(serde_json::from_str::<Value>(&line).unwrap()["result"].is_object());
    let cfg = f.root.join("exitbind.json");
    let mut data: Value = serde_json::from_slice(&fs::read(&cfg).unwrap()).unwrap();
    data["agents"]["worker"]["write"] = json!([]);
    fs::write(cfg, data.to_string()).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"file","arguments":{"action":"edit","path":"src/a.txt","content":"once"}}})).unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    drop(input);
    drop(output);
    let end = child.wait_with_output().unwrap();
    assert!(end.status.success(), "{end:?}");
    let replay: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(replay["result"]["isError"], true, "{replay}");
    assert_eq!(fs::metadata(f.root.join("src/a.txt")).unwrap().ino(), inode);
}

#[test]
fn controlled_option_refuses_other_versions_project_config_and_capability_override() {
    let f = Fixture::new();
    let fake = f.root.join("codex-fixture");
    fs::write(&fake, "#!/bin/sh\nif [ \"$1\" = '--version' ]; then printf 'codex-cli 0.159.0\\n'; exit 0; fi\nprintf unexpected-provider > provider-started\nexit 1\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let invoke = |extra: &[&str]| {
        let mut args = vec![
            "work",
            "act",
            &f.work,
            "--controlled-effects",
            "--codex-bin",
            fake.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        f.call(&args, b"")
    };
    let override_attempt = invoke(&["--sandbox-mode", "workspace-write"]);
    assert!(!override_attempt.status.success());
    assert!(String::from_utf8_lossy(&override_attempt.stderr).contains("sandbox overrides"));
    let no_permit = invoke(&[]);
    assert!(!no_permit.status.success());
    assert!(
        String::from_utf8_lossy(&no_permit.stderr)
            .contains("host-issued current worker mutation permit"),
        "{no_permit:?}"
    );
    assert!(!f.root.join("provider-started").exists());
    f.ok(
        &[
            "work",
            "permit",
            &f.work,
            &f.worker,
            "--operation",
            "fixture",
        ],
        b"",
    );
    let old = invoke(&[]);
    assert!(!old.status.success());
    if !cfg!(target_os = "linux") {
        assert!(
            String::from_utf8_lossy(&old.stderr).contains("Linux read-only worker"),
            "{old:?}"
        );
        assert!(!f.root.join("provider-started").exists());
        return;
    }
    if String::from_utf8_lossy(&old.stderr)
        .contains("controlled effects do not support system or managed Codex configuration")
        && std::env::var("EXITBIND_TEST_CONTROLLED_SYSTEM_CONFIG_UNAVAILABLE").as_deref() == Ok("1")
    {
        assert!(!f.root.join("provider-started").exists());
        eprintln!("COVERAGE UNAVAILABLE: controlled version/project-config subcases: system or managed Codex configuration preempted them; earlier sandbox/permit refusals and no provider spawn were exercised");
        return;
    }
    assert!(
        String::from_utf8_lossy(&old.stderr).contains("0.160.0"),
        "{old:?}"
    );
    fs::create_dir(f.root.join(".codex")).unwrap();
    fs::write(
        f.root.join(".codex/config.toml"),
        "[mcp_servers.other]\ncommand='false'\n",
    )
    .unwrap();
    let config = invoke(&[]);
    assert!(!config.status.success());
    assert!(String::from_utf8_lossy(&config.stderr).contains("project Codex configuration"));
    assert!(!f.root.join("provider-started").exists());
}
