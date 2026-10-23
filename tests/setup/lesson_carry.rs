//! R54 Step 7: `setup --lessons-from PREVIOUS_CONFIG` carries the previous
//! project's accepted, unexpired lessons into a new project through the
//! ordinary memory lifecycle. The test only prepares the previous project; it
//! copies no lesson file and edits no configuration of the new project.
use super::support;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

const SCOPE: &str = "project-lessons.v1";
const REVISION: &str = "1207ce96c36674747a4bab54692c46b5e243ff20";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn exitbind(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

fn ok(root: &Path, args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.extend(["--config", "exitbind.json"]);
    let output = exitbind(root, &all, b"");
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

/// A fixture project and the temp root that holds it. Files a test keeps next
/// to the project (`<project>.home`, `<project>.session-goals`) stay inside
/// that root, so the guard removes them with it.
pub(super) struct Root(pub(super) PathBuf, pub(super) support::TempRoot);

pub(super) fn repository(label: &str) -> Root {
    let base = support::temp(label);
    let root = base.join("project");
    fs::create_dir(&root).unwrap();
    support::git_topology::repository(&root);
    Root(root, base)
}

/// The previous project: opted in to project lessons by its owner, with the
/// Lead holding the lesson rights and the worker reading them.
pub(super) fn previous_project() -> Root {
    let root = repository("lesson-carry-previous");
    let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root.0)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let path = root.0.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["memory"] = json!({"root":".exitbind/memory","maxItems":8,"maxBytes":16384,
        "protocolScopes":[SCOPE],"syntheticScopes":[]});
    for right in [
        "memoryRead",
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryRevoke",
        "memoryExpire",
    ] {
        config["agents"]["lead"][right] = json!([SCOPE]);
    }
    config["agents"]["worker"]["memoryRead"] = json!([SCOPE]);
    for agent in ["lead", "worker"] {
        config["agents"][agent]["crossContext"] = json!("same-scope");
    }
    fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    root
}

/// Write one guarded lesson in the previous project and take it through the
/// Lead lifecycle up to `promote`.
pub(super) fn accept(root: &Path, id: &str, fact: &str, expires: &str) -> String {
    fs::create_dir_all(root.join("notes")).unwrap();
    let guard = format!("notes/{id}.md");
    fs::write(root.join(&guard), format!("{fact}\n")).unwrap();
    let identity = ok(root, &["project", "context", "--json"])["project"]["memoryIdentity"].clone();
    let lesson = json!({"version":1,"id":id,"fact":fact,"projectIdentity":identity,"owner":"lead",
        "provenance":["fixture reviewed fact"],"createdRevision":REVISION,"revalidatedRevision":REVISION,
        "appliesTo":{"agents":["worker"],"taskTerms":[]},
        "guards":[{"path":guard,"sha256":sha(format!("{fact}\n").as_bytes())}]});
    let source = format!("lessons/{id}.json");
    fs::create_dir_all(root.join("lessons")).unwrap();
    fs::write(root.join(&source), serde_json::to_vec(&lesson).unwrap()).unwrap();
    let ledger = format!(".exitbind/memory/{id}.jsonl");
    ok(
        root,
        &[
            "memory",
            "propose",
            "lead",
            &source,
            "--scope",
            SCOPE,
            "--expires-at",
            expires,
            "--ledger",
            &ledger,
        ],
    );
    ok(root, &["memory", "review", "lead", &ledger]);
    ok(root, &["memory", "promote", "lead", &ledger]);
    ledger
}

fn setup(root: &Path, previous: &Path, apply: bool) -> Value {
    let from = previous.join("exitbind.json");
    let mut args = vec![
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root.to_str().unwrap(),
        "--scope",
        "worker",
        "--observe",
        "notes",
        "--write",
        "notes",
        "--commands",
        "true",
        "--check-command",
        "true",
        "--goal",
        "Carry the reviewed lessons",
        "--review-policy",
        "required",
        "--hosts",
        "claude",
        "--skip-skills",
        "--lessons-from",
        from.to_str().unwrap(),
    ];
    if apply {
        args.push("--apply");
    }
    let output = exitbind(root, &args, b"");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn reasons(lessons: &Value) -> Vec<(String, String)> {
    let mut out = lessons["notCarried"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["id"].as_str().unwrap().to_owned(),
                item["reason"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}

fn carried(lessons: &Value) -> Vec<&str> {
    lessons["carried"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect()
}

fn subagent_context(root: &Path, agent: &str) -> String {
    let payload = json!({"hook_event_name":"SubagentStart","cwd":root,"agent_type":agent});
    let output = exitbind(root, &["hook-run"], payload.to_string().as_bytes());
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn setup_lessons_from_carries_only_accepted_unexpired_lessons_to_a_fresh_worker() {
    let previous = previous_project();
    let prev = previous.0.as_path();
    accept(
        prev,
        "carry-me",
        "Trace every delivery consumer.",
        "2100-01-01T00:00:00Z",
    );
    accept(
        prev,
        "expired-one",
        "An expired fact.",
        "2001-01-01T00:00:00Z",
    );
    let revoked = accept(
        prev,
        "revoked-one",
        "A retired fact.",
        "2100-01-01T00:00:00Z",
    );
    ok(prev, &["memory", "revoke", "lead", &revoked]);
    let before = fs::read(prev.join("exitbind.json")).unwrap();

    let fresh = repository("lesson-carry-new");
    let root = fresh.0.as_path();
    let preview = setup(root, prev, false);
    assert_eq!(carried(&preview["lessons"]), ["carry-me"], "{preview}");
    assert_eq!(preview["lessons"]["applied"], false);
    assert!(
        !root.join("exitbind.json").exists(),
        "preview changes nothing"
    );
    assert!(!root.join("notes").exists(), "preview copies no guard");

    let report = setup(root, prev, true);
    let lessons = &report["lessons"];
    assert_eq!(carried(lessons), ["carry-me"], "{report}");
    assert_eq!(
        reasons(lessons),
        [
            ("expired-one".to_owned(), "expired".to_owned()),
            ("revoked-one".to_owned(), "revoked".to_owned())
        ],
        "{report}"
    );
    let item = &lessons["carried"][0];
    assert_eq!(item["state"], "accepted", "{report}");
    assert_eq!(item["guards"][0]["action"], "copy", "{report}");
    assert_eq!(
        fs::read(root.join("notes/carry-me.md")).unwrap(),
        fs::read(prev.join("notes/carry-me.md")).unwrap(),
        "the absent guard is copied byte-identical"
    );
    let source: Value =
        serde_json::from_slice(&fs::read(root.join(item["source"].as_str().unwrap())).unwrap())
            .unwrap();
    let identity = ok(root, &["project", "context", "--json"])["project"]["memoryIdentity"].clone();
    assert_eq!(source["projectIdentity"], identity);
    assert_eq!(source["createdRevision"], REVISION);
    let provenance = source["provenance"].as_array().unwrap();
    assert!(provenance.len() <= 4 && provenance.iter().all(|p| p.as_str().unwrap().len() <= 160));
    assert!(provenance.iter().any(|p| p
        == &json!(format!(
            "Previous item {}",
            lessons["carried"][0]["previousItemId"].as_str().unwrap()
        ))));
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
    assert_eq!(config["memory"]["protocolScopes"], json!([SCOPE]));
    assert_eq!(config["agents"]["lead"]["memoryPromote"], json!([SCOPE]));
    assert_eq!(config["agents"]["worker"]["memoryRead"], json!([SCOPE]));
    assert_eq!(fs::read(prev.join("exitbind.json")).unwrap(), before);

    let again = setup(root, prev, true);
    assert_eq!(
        again["lessons"]["carried"][0]["state"], "already_carried",
        "{again}"
    );

    let begun = ok(
        root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Carry the reviewed lessons",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    );
    let work = begun["work"].as_str().unwrap();
    let scoped = exitbind(
        root,
        &[
            "work",
            "return",
            work,
            begun["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
            "--config",
            "exitbind.json",
        ],
        b"bounded fixture scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let detail = ok(root, &["work", "detail", work, "--json"]);
    let delivered = &detail["recipientContext"]["projectLessons"];
    let ids = delivered["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["lesson"]["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["carry-me"], "{detail}");
    let native = subagent_context(root, "worker");
    assert!(
        native.contains("Trace every delivery consumer."),
        "{native}"
    );
    assert!(!native.contains("A retired fact."), "{native}");
}

#[test]
fn a_guard_changed_in_the_new_project_excludes_the_lesson_for_revalidation() {
    let previous = previous_project();
    let prev = previous.0.as_path();
    accept(
        prev,
        "kept-guard",
        "The kept guard matches.",
        "2100-01-01T00:00:00Z",
    );
    accept(
        prev,
        "changed-guard",
        "The changed guard differs.",
        "2100-01-01T00:00:00Z",
    );
    let fresh = repository("lesson-carry-guard");
    let root = fresh.0.as_path();
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::copy(
        prev.join("notes/kept-guard.md"),
        root.join("notes/kept-guard.md"),
    )
    .unwrap();
    fs::write(
        root.join("notes/changed-guard.md"),
        "edited in the new project\n",
    )
    .unwrap();
    let changed = fs::read(root.join("notes/changed-guard.md")).unwrap();

    let report = setup(root, prev, true);
    let lessons = &report["lessons"];
    assert_eq!(carried(lessons), ["kept-guard"], "{report}");
    assert_eq!(
        lessons["carried"][0]["guards"][0]["action"], "kept",
        "{report}"
    );
    assert_eq!(
        reasons(lessons),
        [(
            "changed-guard".to_owned(),
            "guard_changed_needs_revalidation".to_owned()
        )]
    );
    assert_eq!(lessons["notCarried"][0]["path"], "notes/changed-guard.md");
    assert_eq!(
        fs::read(root.join("notes/changed-guard.md")).unwrap(),
        changed
    );
}
