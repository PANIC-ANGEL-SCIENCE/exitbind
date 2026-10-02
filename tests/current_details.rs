//! Scoped current views reject mixed Work/recipient evidence and preserve all
//! large instructions and task states through the existing read route.
mod support;
use serde_json::{json, Value};
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
        let (value, calls, _, _) = self.detail_metrics(next, section);
        (value, calls)
    }
    fn detail_metrics(&self, next: &Value, section: &str) -> (Value, usize, usize, usize) {
        let mut transport_bytes = 0;
        let mut peak_bytes = 0;
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
            transport_bytes += output.stdout.len();
            peak_bytes = peak_bytes.max(output.stdout.len());
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
        (
            serde_json::from_slice(&bytes).unwrap(),
            calls,
            transport_bytes,
            peak_bytes,
        )
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

#[test]
fn grouped_detail_is_readable_and_keeps_the_current_fence() {
    let fixture = Fixture::new();
    let work = fixture.begin();
    fixture.return_result(&work, "scoped", "scope\n".as_bytes());
    let worker = fixture.json(&["work", "next", &work]);
    fixture.return_result(&work, "completed", "résumé exact\n".as_bytes());
    let current = fixture.json(&["work", "next", &work]);
    let output = fixture.call(&["work", "detail", &work], None);
    assert!(output.status.success(), "{output:?}");
    let detail: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(detail["kind"], "work_detail");
    assert_eq!(detail["version"], 1);
    assert_eq!(detail["binding"], current["current"]["binding"]);
    assert!(detail["sections"]["assignment"].is_object());
    assert!(detail["sections"]["tasks"].is_object());
    let evidence = &detail["sections"]["evidence"];
    let artifact = evidence["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("artifact"))
        .unwrap();
    assert_eq!(artifact["readable"], true);
    assert_eq!(artifact["content"], "résumé exact\n");
    assert!(!detail.to_string().contains("contentHex"));

    let stale = fixture.call(
        &[
            "work",
            "expand",
            &work,
            current["current"]["details"]["grouped"]["reference"]
                .as_str()
                .unwrap(),
        ],
        None,
    );
    assert!(stale.status.success(), "{stale:?}");
    let other = fixture.begin();
    let wrong = fixture.call(
        &[
            "work",
            "expand",
            &other,
            current["current"]["details"]["grouped"]["reference"]
                .as_str()
                .unwrap(),
        ],
        None,
    );
    assert!(
        !wrong.status.success(),
        "cross-work grouped detail was accepted"
    );
    let foreign = Fixture::new();
    let foreign_work = foreign.begin();
    let wrong_project = foreign.call(
        &[
            "work",
            "expand",
            &foreign_work,
            current["current"]["details"]["grouped"]["reference"]
                .as_str()
                .unwrap(),
        ],
        None,
    );
    assert!(!wrong_project.status.success());
    let malformed = fixture.call(
        &["work", "expand", &work, "ref:work-detail:v9:../unsafe"],
        None,
    );
    assert!(!malformed.status.success());
    assert!(worker["current"]["binding"].is_string());
}

#[test]
fn grouped_detail_retains_equivalent_large_content_and_rejects_revision_drift() {
    let fixture = Fixture::new();
    let work = fixture.begin();
    for n in 0..45 {
        fixture.json(&[
            "goal",
            "incorporate",
            "--goal-id",
            &work,
            "--goal",
            "multi-outcome goal",
            "--obligation",
            &format!("task {n}"),
        ]);
    }
    let scope = "é".repeat(20000);
    fixture.return_result(&work, "scoped", scope.as_bytes());
    let next = fixture.json(&["work", "next", &work]);
    let before = fixture.ledgers();
    let output = fixture.call(&["work", "detail", &work], None);
    assert!(output.status.success(), "{output:?}");
    let detail: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(detail["complete"], true);
    assert!(output.stdout.len() <= 256 * 1024);
    let (legacy_assignment, ac, ab, ap) = fixture.detail_metrics(&next, "assignment");
    let (legacy_evidence, ec, eb, ep) = fixture.detail_metrics(&next, "evidence");
    let (legacy_tasks, tc, tb, tp) = fixture.detail_metrics(&next, "tasks");
    let mut assignment = detail["sections"]["assignment"].clone();
    let context = &mut assignment["assignment"]["context"];
    for (alias, original) in [
        ("goal", "goal"),
        ("scope", "scope"),
        ("subject", "subject"),
        ("current", "evidence"),
        ("missing", "obligations"),
        ("loop", "loop"),
        ("next", "next"),
    ] {
        if context["recovery"].get(alias).is_none() && context.get(original).is_some() {
            context["recovery"][alias] = context[original].clone();
        }
    }
    for alias in assignment["projection"]["equalAliases"]
        .as_array()
        .unwrap()
        .clone()
    {
        let key = alias["removed"]
            .as_str()
            .unwrap()
            .strip_prefix("residual.")
            .unwrap();
        let source = alias["source"]
            .as_str()
            .unwrap()
            .strip_prefix("assignment.context")
            .unwrap();
        let value = if source.is_empty() {
            assignment["assignment"]["context"].clone()
        } else {
            assignment["assignment"]["context"][&source[1..]].clone()
        };
        assignment["residual"][key] = value;
    }
    assignment.as_object_mut().unwrap().remove("projection");
    assert_eq!(assignment, legacy_assignment);
    assert_eq!(detail["sections"]["tasks"], legacy_tasks);
    let artifacts = detail["sections"]["evidence"]["items"].as_array().unwrap();
    assert_eq!(
        artifacts.len(),
        legacy_evidence["items"].as_array().unwrap().len()
    );
    assert_eq!(artifacts[0]["artifact"]["content"], scope);
    assert_eq!(
        artifacts[0]["artifact"]["sha256"],
        legacy_evidence["items"][0]["artifact"]["sha256"]
    );
    assert_eq!(
        detail["sections"]["tasks"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        45
    );
    assert_eq!(before, fixture.ledgers());
    eprintln!(
        "R23_EQUIVALENT_TRANSPORT {}",
        json!({"legacyReads": ac+ec+tc,
        "legacyBytes": ab+eb+tb, "readableReads": 1, "readableBytes": output.stdout.len(),
        "artifactBytes": scope.len(), "taskCount": 45,
        "setupStatusBytes": serde_json::to_vec(&next).unwrap().len()+1,
        "legacyTotalReads": ac+ec+tc+1, "readableTotalReads": 2,
        "legacyTotalBytes": ab+eb+tb+serde_json::to_vec(&next).unwrap().len()+1,
        "readableTotalBytes": output.stdout.len()+serde_json::to_vec(&next).unwrap().len()+1,
        "legacyPeakPayloadBytes": ap.max(ep).max(tp), "readablePeakPayloadBytes": output.stdout.len(),
        "legacyDecodedSectionBytes": serde_json::to_vec(&legacy_assignment).unwrap().len()+
            serde_json::to_vec(&legacy_evidence).unwrap().len()+serde_json::to_vec(&legacy_tasks).unwrap().len(),
        "cache": "none"})
    );
    fixture.json(&[
        "goal",
        "incorporate",
        "--goal-id",
        &work,
        "--goal",
        "multi-outcome goal",
        "--obligation",
        "new task changes same recipient binding",
    ]);
    let stale = fixture.call(
        &[
            "work",
            "expand",
            &work,
            next["current"]["details"]["grouped"]["reference"]
                .as_str()
                .unwrap(),
        ],
        None,
    );
    assert!(!stale.status.success(), "stale task binding was consumed");
}

#[test]
fn grouped_unreadable_evidence_keeps_verified_origin_and_never_claims_complete() {
    for body in [vec![0xff, 0], vec![b'x'; 65537]] {
        let fixture = Fixture::new();
        let work = fixture.begin();
        fixture.return_result(&work, "scoped", &body);
        let before = fixture.ledgers();
        let value = fixture.json(&["work", "detail", &work]);
        assert_eq!(value["complete"], false);
        let artifact = &value["sections"]["evidence"]["items"][0]["artifact"];
        assert_eq!(artifact["bytes"], body.len());
        assert_eq!(artifact["verified"], true);
        assert_eq!(artifact["readable"], false);
        assert!(artifact["sha256"].is_string());
        assert!(artifact["content"].is_null());
        assert!(artifact["access"]["currentMetadata"]["command"].is_array());
        assert_eq!(before, fixture.ledgers());
    }
}

#[test]
fn grouped_missing_or_corrupt_required_artifact_is_refused_without_events() {
    for missing in [false, true] {
        let fixture = Fixture::new();
        let work = fixture.begin();
        fixture.return_result(&work, "scoped", b"exact scope");
        let artifact = fs::read_dir(fixture.root.join(".exitbind/artifacts"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_file())
            .unwrap();
        if missing {
            fs::remove_file(artifact).unwrap();
        } else {
            fs::write(artifact, b"corrupted scope").unwrap();
        }
        let before = fixture.ledgers();
        let output = fixture.call(&["work", "detail", &work], None);
        assert!(!output.status.success(), "invalid evidence was consumed");
        assert_eq!(before, fixture.ledgers());
    }
}
