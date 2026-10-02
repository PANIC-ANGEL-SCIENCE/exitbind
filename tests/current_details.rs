//! Scoped current views reject mixed Work/recipient evidence and preserve all
//! large instructions and task states through the existing read route.
mod support;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("current-details");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let fake_path = root.join(".exitbind/test-path");
        fs::create_dir(&fake_path).unwrap();
        fs::write(fake_path.join("exitbind"), "#!/bin/sh\nexit 99\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fake_path.join("exitbind"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        Self {
            config: root.join("exitbind.json"),
            root,
        }
    }
    fn call(&self, args: &[&str], body: Option<&[u8]>) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(body) = body {
            child.stdin.take().unwrap().write_all(body).unwrap();
        }
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args, None);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn begin(&self) -> String {
        self.json(&[
            "work",
            "begin",
            "change",
            "--goal",
            "deliver current scoped detail",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ])["work"]
            .as_str()
            .unwrap()
            .into()
    }
    fn return_result(&self, work: &str, outcome: &str, body: &[u8]) {
        let next = self.json(&["work", "next", work]);
        let output = self.call(
            &[
                "work",
                "return",
                work,
                next["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            Some(body),
        );
        assert!(output.status.success(), "{outcome}: {output:?}");
        let _: Value = serde_json::from_slice(&output.stdout).unwrap();
    }
    fn detail(&self, next: &Value, section: &str) -> (Value, usize) {
        let mut route = next["current"]["details"][section].clone();
        let mut bytes = Vec::new();
        let mut calls = 0;
        let mut sha = None;
        loop {
            let argv = route["command"].as_array().unwrap();
            assert_eq!(argv[0], env!("CARGO_BIN_EXE_exitbind"));
            let output = Command::new(argv[0].as_str().unwrap())
                .current_dir(std::env::temp_dir())
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        self.root.join(".exitbind/test-path").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .args(argv[1..].iter().map(|item| item.as_str().unwrap()))
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(output.stdout.len() <= 64 * 1024);
            let page: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(page["offset"], bytes.len());
            assert_eq!(page["binding"], next["current"]["binding"]);
            if let Some(ref sha) = sha {
                assert_eq!(&page["sectionSha256"], sha);
            } else {
                sha = Some(page["sectionSha256"].clone());
            }
            let hex = page["contentHex"].as_str().unwrap();
            bytes.extend(
                (0..hex.len())
                    .step_by(2)
                    .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap()),
            );
            calls += 1;
            if page["next"].is_null() {
                assert_eq!(page["totalBytes"], bytes.len());
                break;
            }
            assert_eq!(page["complete"], false);
            route = page["next"].clone();
        }
        use sha2::{Digest, Sha256};
        assert_eq!(sha.unwrap(), format!("{:x}", Sha256::digest(&bytes)));
        (serde_json::from_slice(&bytes).unwrap(), calls)
    }
    fn ledgers(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = fs::read_dir(self.root.join(".exitbind/runs"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .map(|path| (path.clone(), fs::read(path).unwrap()))
            .collect::<Vec<_>>();
        files.sort();
        files
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn current_detail_tracks_check_review_repair_and_terminal_without_read_effects() {
    let fixture = Fixture::new();
    let work = fixture.begin();
    let initial = fixture.json(&["work", "next", &work]);
    assert_eq!(initial["current"]["result"]["exists"], false);
    fixture.return_result(&work, "scoped", b"complete scope\n");
    let worker = fixture.json(&["work", "next", &work]);
    assert_eq!(worker["current"]["recipient"]["role"], "worker");
    let before = fixture.ledgers();
    let (assignment, _) = fixture.detail(&worker, "assignment");
    assert_eq!(assignment["assignment"]["role"], "worker");
    assert_eq!(
        assignment["assignment"]["context"]["subject"]["sha256"],
        worker["current"]["result"]["subject"]["sha256"]
    );
    let (evidence, _) = fixture.detail(&worker, "evidence");
    assert!(evidence["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["artifact"]["contentHex"] == "636f6d706c6574652073636f70650a"));
    assert_eq!(before, fixture.ledgers());
    fixture.return_result(&work, "completed", b"exact worker artifact\n");
    let check = fixture.json(&["work", "next", &work]);
    assert_eq!(check["current"]["action"], "check");
    assert_eq!(check["current"]["result"]["exists"], true);
    let stale = fixture.call(
        &[
            "work",
            "expand",
            &work,
            worker["current"]["details"]["assignment"]["reference"]
                .as_str()
                .unwrap(),
        ],
        None,
    );
    assert!(!stale.status.success());
    fixture.json(&["work", "check", &work]);
    let reviewer = fixture.json(&["work", "next", &work]);
    assert_eq!(reviewer["current"]["recipient"]["role"], "reviewer");
    let (evidence, _) = fixture.detail(&reviewer, "evidence");
    let items = evidence["items"].as_array().unwrap();
    assert!(items.iter().any(
        |item| item["artifact"]["contentHex"] == "657861637420776f726b65722061727469666163740a"
    ));
    assert!(items
        .iter()
        .any(|item| item["stdout"]["complete"] == true && item["result"]["code"] == 0));
    fixture.return_result(&work, "approved", b"independent review\n");
    let lead = fixture.json(&["work", "next", &work]);
    assert_eq!(lead["current"]["recipient"]["role"], "lead");
    fixture.return_result(&work, "rework", b"repair required\n");
    let repair = fixture.json(&["work", "next", &work]);
    assert_eq!(repair["current"]["recipient"]["role"], "worker");
    assert_eq!(repair["current"]["repair"]["outcome"], "rework");
    assert_eq!(repair["current"]["phase"]["attempt"], 2);
    assert_eq!(repair["current"]["result"]["exists"], false);
    assert_eq!(repair["current"]["result"]["retainedPreviousResult"], true);
    let (repair_evidence, _) = fixture.detail(&repair, "evidence");
    assert!(repair_evidence["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |item| item["artifact"]["contentHex"] == "7265706169722072657175697265640a"
                && item["historical"] == true
        ));
    fixture.return_result(&work, "completed", b"repaired worker\n");
    fixture.json(&["work", "check", &work]);
    fixture.return_result(&work, "approved", b"fresh review\n");
    fixture.return_result(&work, "accepted", b"lead accepts\n");
    let terminal = fixture.json(&["work", "next", &work]);
    assert_eq!(terminal["current"]["action"], "done");
    assert_eq!(terminal["current"]["phase"]["status"], "accepted");
    assert_eq!(terminal["current"]["result"]["historical"], true);
    assert_eq!(terminal["current"]["phase"]["stage"], 4);
    assert_eq!(terminal["current"]["phase"]["attempt"], 2);
    let (history, _) = fixture.detail(&terminal, "evidence");
    assert_eq!(history["historical"], true);
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["artifact"]["contentHex"] == "726570616972656420776f726b65720a"));
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["stdout"]["complete"] == true && item["result"]["code"] == 0));
}

#[test]
fn large_sections_and_all_tasks_are_reachable_with_scoped_refusals() {
    let fixture = Fixture::new();
    let work = fixture.begin();
    for n in 0..45 {
        fixture.json(&[
            "goal",
            "incorporate",
            "--goal-id",
            &work,
            "--goal",
            "external overall goal",
            "--obligation",
            &format!("task {n}"),
        ]);
    }
    let before = fixture.json(&["work", "next", &work]);
    fixture.return_result(&work, "scoped", &vec![b'x'; 40000]);
    let next = fixture.json(&["work", "next", &work]);
    assert!(serde_json::to_vec(&next).unwrap().len() < 8192);
    let (tasks, _) = fixture.detail(&next, "tasks");
    assert_eq!(tasks["tasks"].as_array().unwrap().len(), 45);
    assert_eq!(tasks["decomposition"]["omitted"], 0);
    let (evidence, calls) = fixture.detail(&next, "evidence");
    assert!(calls > 1);
    assert!(evidence["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["artifact"]["bytes"] == 40000));
    let stale = fixture.call(
        &[
            "work",
            "expand",
            &work,
            before["current"]["details"]["assignment"]["reference"]
                .as_str()
                .unwrap(),
        ],
        None,
    );
    assert!(!stale.status.success());
    let other = fixture.begin();
    let reference = next["current"]["details"]["tasks"]["reference"]
        .as_str()
        .unwrap();
    let wrong_work = fixture.call(&["work", "expand", &other, reference], None);
    assert!(!wrong_work.status.success());
    let other_project = Fixture::new();
    let other_work = other_project.begin();
    let wrong_project = other_project.call(&["work", "expand", &other_work, reference], None);
    assert!(!wrong_project.status.success());
    let unsafe_ref = fixture.call(
        &[
            "work",
            "expand",
            &work,
            "ref:work-section:../../private:tasks:0",
        ],
        None,
    );
    assert!(!unsafe_ref.status.success());
}
