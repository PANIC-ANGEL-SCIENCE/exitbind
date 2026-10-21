use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn digest_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn native_command(root: &Path, session: &str, store: &Path) -> Command {
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(root)
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env_remove("CLAUDE_ENV_FILE");
    command
}

fn native_value(root: &Path, session: &str, store: &Path, args: &[&str]) -> Value {
    let output = native_command(root, session, store)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "native command failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn native_hook(root: &Path, session: &str, store: &Path, event: &str) -> std::process::Output {
    let payload = json!({"hook_event_name":event,"session_id":session,"cwd":root});
    let mut command = native_command(root, session, store);
    command
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&payload).unwrap())
    {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

fn native_graph_command(
    root: &Path,
    session: &str,
    store: &Path,
    bin: &Path,
    log: &Path,
    source: &Path,
    args: &[&str],
) -> std::process::Output {
    let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let mut command = native_command(root, session, store);
    command
        .env("PATH", path)
        .env("FAKE_CODEGRAPH_LOG", log)
        .env("FAKE_SOURCE_FILE", source)
        .args(args);
    command.output().unwrap()
}

fn install_native_provider(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let bin = root.join("recovery-provider-bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("codegraph");
    let log = root.with_extension("recovery-codegraph.log");
    fs::write(
        &executable,
        r##"#!/bin/sh
printf '%s\n' "$1" >> "$FAKE_CODEGRAPH_LOG"
case "$1" in
  --version) printf 'CodeGraph fixture 1.6\n' ;;
  init)
    mkdir -p "$2/.codegraph"
    : > "$2/.codegraph/recovery-ready"
    ;;
  index)
    : > .codegraph/recovery-ready
    ;;
  status)
    project="$3"
    if [ ! -f "$project/.codegraph/recovery-ready" ]; then
      printf '{"initialized":false,"projectPath":"%s","indexPath":"%s/.codegraph","lastIndexed":null}\n' "$project" "$project"
      exit 0
    fi
    printf '{"initialized":true,"version":"fixture","projectPath":"%s","indexPath":"%s/.codegraph","lastIndexed":"fixture-1","fileCount":1,"nodeCount":1,"edgeCount":1,"dbSizeBytes":64,"backend":"fixture","worktreeMismatch":null,"pendingChanges":{"added":0,"modified":0,"removed":0},"index":{"builtWithVersion":"fixture","builtWithExtractionVersion":"1","currentExtractionVersion":"1","reindexRecommended":false,"state":"complete","pendingRefs":0}}\n' "$project" "$project"
    ;;
  context)
    printf '{"query":"%s","summary":"callers of parse_config — éééééééééééééééééééééééééééééééééééééééééééééééééé","nodes":[{"name":"parse_config"}],"edges":[{"from":"caller","to":"parse_config"}],"codeBlocks":[{"file":"impact.rs","code":"fn parse_config() {}"}],"entryPoints":[{"name":"caller"}],"relatedFiles":[{"path":"impact.rs"}],"stats":{"nodes":1,"edges":1}}\n' "$8"
    ;;
  *) exit 12 ;;
esac
"##,
    )
    .unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    (bin, log, root.join("impact.rs"))
}

fn invoke_route(
    route: &Value,
    session: &str,
    store: &Path,
    bin: &Path,
    log: &Path,
    source: &Path,
) -> Value {
    let argv = route
        .as_array()
        .expect("a restored graph observation exposes its exact query argv")
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(argv.len() >= 2);
    let executable = Path::new(&argv[0]);
    let root = source.parent().unwrap();
    let output = native_graph_command(
        root,
        session,
        store,
        bin,
        log,
        source,
        &argv[1..].iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert!(
        output.status.success(),
        "restored query route failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::canonicalize(executable).unwrap(),
        fs::canonicalize(env!("CARGO_BIN_EXE_exitbind")).unwrap()
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn existing_native_hint_checkpoint_restores_and_revalidates_graph_without_losing_pending_state() {
    let root = project("project-graph-recovery-composition");
    fs::write(root.join("impact.rs"), "fn parse_config() {}\n").unwrap();
    let git = |args: &[&str]| {
        let output = support::git_topology::git(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    git(&["config", "user.name", "Graph recovery fixture"]);
    git(&["config", "user.email", "graph-recovery@example.invalid"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "native graph baseline"]);

    let session = "project-graph-recovery-session";
    let store = support::temp("project-graph-recovery-session-store");
    let mut store_permissions = fs::metadata(&store).unwrap().permissions();
    store_permissions.set_mode(0o700);
    fs::set_permissions(&store, store_permissions).unwrap();
    native_value(
        &root,
        session,
        &store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "graph-recovery-goal",
            "--goal",
            "Continue the existing impact review",
            "--obligation",
            "Preserve a pending Work route and revalidate impact findings",
            "--json",
        ],
    );
    let started = native_value(
        &root,
        session,
        &store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Validate current callers of parse_config",
            "--check-command",
            "true",
            "--json",
        ],
    );
    let work = started["work"].as_str().unwrap().to_owned();
    let goal = native_value(&root, session, &store, &["goal", "status", "--json"]);
    let member = goal["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["work"] == work)
        .unwrap();

    let producer = native_value(&root, session, &store, &["version", "--json"]);
    fs::write(
        root.join("caller-guidance.md"),
        "Read current callers before acting.\n",
    )
    .unwrap();
    let guidance = "Read current callers before acting.\n";
    let record = json!({
        "sessionId":session,
        "currentStep":"Graph inspection is historical; reread the exact Work before action",
        "currentWork":{"state":"known","work":work},
        "latestCorrections":["Do not infer current acceptance from the saved graph."],
        "workRefs":[{
            "work":work,
            "configPath":member["configPath"],
            "configSha256":member["configSha256"],
            "startEventSha256":member["startEventSha256"],
            "relation":"member"
        }],
        "pendingOperation":{"kind":"work_detail","description":"Reread the exact member before acting","work":work},
        "guidanceRefs":[{
            "label":"caller-read impact guidance",
            "root":"product",
            "path":"caller-guidance.md",
            "work":work,
            "sha256":digest_text(guidance),
            "producerExecutableSha256":producer["executableSha256"],
            "consumption":"caller_reported_read"
        }]
    });
    let mut record_command = native_command(&root, session, &store);
    record_command
        .args(["session", "recovery", "record", "--config"])
        .arg(root.join("exitbind.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = record_command.spawn().unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&record).unwrap())
    {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let recorded = child.wait_with_output().unwrap();
    assert!(recorded.status.success(), "{recorded:?}");

    let before_hint = fs::read_dir(&store)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("recovery-hint.json"))
        .find(|path| path.is_file())
        .unwrap();
    let original_hint = fs::read(&before_hint).unwrap();
    let invalid_guidance = json!({
        "sessionId":session,
        "currentStep":"invalid generated guidance must be rejected",
        "currentWork":{"state":"known","work":work},
        "workRefs":record["workRefs"],
        "guidanceRefs":[{
            "label":"must not imply caller consumption",
            "root":"state",
            "path":format!(".exitbind/codegraph/{}.json", digest_text("not an artifact")),
            "work":work,
            "sha256":"a".repeat(64),
            "producerExecutableSha256":producer["executableSha256"],
            "consumption":"generated_query_result"
        }]
    });
    let mut invalid_command = native_command(&root, session, &store);
    invalid_command
        .args(["session", "recovery", "record", "--config"])
        .arg(root.join("exitbind.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = invalid_command.spawn().unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&invalid_guidance).unwrap())
    {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let rejected = child.wait_with_output().unwrap();
    assert!(!rejected.status.success());
    assert_eq!(fs::read(&before_hint).unwrap(), original_hint);

    let (bin, log, source) = install_native_provider(&root);
    let member_config = member["configPath"].as_str().unwrap();
    let enabled = native_graph_command(
        &root,
        session,
        &store,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "enable",
            "--config",
            member_config,
            "--json",
        ],
    );
    assert!(enabled.status.success(), "{enabled:?}");
    let query = [
        "project",
        "graph",
        "query",
        "--query",
        "callers of parse_config",
        "--config",
        member_config,
        "--json",
    ];
    let queried = native_graph_command(&root, session, &store, &bin, &log, &source, &query);
    assert!(queried.status.success(), "{queried:?}");
    let queried: Value = serde_json::from_slice(&queried.stdout).unwrap();
    assert_eq!(queried["status"], "usable");
    assert!(queried["context"]["summary"].as_str().unwrap().len() > 117);
    assert!(queried["context"]["summary"]
        .as_str()
        .unwrap()
        .contains('é'));
    assert_eq!(queried["continuation"]["attached"], true);

    let pre = native_hook(&root, session, &store, "PreCompact");
    assert!(pre.status.success(), "{pre:?}");
    let post = native_hook(&root, session, &store, "PostCompact");
    assert!(post.status.success(), "{post:?}");
    let shown = native_value(
        &root,
        session,
        &store,
        &["session", "recovery", "show", "--json"],
    );
    assert_eq!(
        shown["savedHint"]["latestCorrections"][0],
        record["latestCorrections"][0]
    );
    assert_eq!(shown["savedHint"]["currentWork"], record["currentWork"]);
    assert_eq!(
        shown["savedHint"]["pendingOperation"],
        record["pendingOperation"]
    );
    assert_eq!(shown["savedHint"]["lastCheckpoint"]["event"], "PostCompact");
    assert_eq!(
        shown["savedHint"]["guidanceRefs"][0]["consumption"],
        "caller_reported_read"
    );
    assert_eq!(
        shown["savedHint"]["evidenceRefs"][0]["consumption"],
        "generated_query_result"
    );
    assert!(shown["savedHint"]["evidenceRefs"][0]["path"]
        .as_str()
        .unwrap()
        .starts_with(".exitbind/codegraph/"));
    let attached_label = shown["savedHint"]["evidenceRefs"][0]["label"]
        .as_str()
        .unwrap();
    assert!(attached_label.starts_with("CodeGraph: callers of parse_config"));
    assert!(attached_label.len() <= 128);
    assert_eq!(
        shown["referenceFreshness"][0]["kind"],
        "prior_graph_observation"
    );
    assert_eq!(
        shown["referenceFreshness"][0]["restoredPriorObservation"]["sourceCurrent"],
        Value::Null
    );
    assert_eq!(
        shown["referenceFreshness"][0]["restoredPriorObservation"]["sourceCheck"],
        "not_revalidated"
    );
    assert_eq!(
        shown["readRoutes"]["references"][0]["queryArgv"][0],
        fs::canonicalize(env!("CARGO_BIN_EXE_exitbind"))
            .unwrap()
            .to_str()
            .unwrap()
    );

    let restored_route = shown["readRoutes"]["references"][0]["queryArgv"].clone();
    let indexes_before_reuse = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter(|line| *line == "index")
        .count();
    let cached = invoke_route(&restored_route, session, &store, &bin, &log, &source);
    assert_eq!(cached["cache"]["reused"], true);
    let provider_log = fs::read_to_string(&log).unwrap();
    assert_eq!(
        provider_log
            .lines()
            .filter(|line| *line == "context")
            .count(),
        1
    );
    assert_eq!(
        provider_log.lines().filter(|line| *line == "index").count(),
        indexes_before_reuse
    );

    let resumed = native_hook(&root, session, &store, "SessionStart");
    assert!(resumed.status.success(), "{resumed:?}");
    let resumed: Value = serde_json::from_slice(&resumed.stdout).unwrap();
    let message = resumed["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        message.contains("Prior CodeGraph observation: callers of parse_config"),
        "{message}"
    );
    assert!(
        message.contains("Original query `callers of parse_config`"),
        "{message}"
    );
    assert!(
        message.contains("provider/index currentness is not asserted"),
        "{message}"
    );
    assert!(message.contains(&work), "{message}");
    assert!(message.len() <= 4096, "{} bytes: {message}", message.len());
    let recovery_line = message
        .lines()
        .find(|line| line.starts_with("Exitbind recovery:"))
        .expect("bounded recovery presentation is present");
    assert!(
        recovery_line.len() <= 900,
        "{} bytes: {recovery_line}",
        recovery_line.len()
    );

    let user_source_dir = root.join("codegraph");
    fs::create_dir_all(&user_source_dir).unwrap();
    fs::write(
        user_source_dir.join("source.rs"),
        "fn user_authored_codegraph_source() {}\n",
    )
    .unwrap();
    let stale = native_value(
        &root,
        session,
        &store,
        &["session", "recovery", "show", "--json"],
    );
    assert_eq!(
        stale["referenceFreshness"][0]["restoredPriorObservation"]["sourceCurrent"],
        Value::Null
    );
    assert_eq!(
        stale["referenceFreshness"][0]["restoredPriorObservation"]["sourceCheck"],
        "not_revalidated"
    );
    let refreshed = invoke_route(&restored_route, session, &store, &bin, &log, &source);
    assert_eq!(refreshed["status"], "usable");
    assert_eq!(refreshed["cache"]["reused"], false);
    let provider_log = fs::read_to_string(&log).unwrap();
    assert_eq!(
        provider_log
            .lines()
            .filter(|line| *line == "context")
            .count(),
        2
    );
    assert!(provider_log.lines().filter(|line| *line == "index").count() > indexes_before_reuse);
    let after_refresh = native_value(
        &root,
        session,
        &store,
        &["session", "recovery", "show", "--json"],
    );
    assert_eq!(
        after_refresh["savedHint"]["latestCorrections"][0],
        record["latestCorrections"][0]
    );
    assert_eq!(
        after_refresh["savedHint"]["pendingOperation"],
        record["pendingOperation"]
    );
    assert_eq!(
        after_refresh["savedHint"]["currentWork"],
        record["currentWork"]
    );
    assert_eq!(
        after_refresh["savedHint"]["guidanceRefs"][0]["consumption"],
        "caller_reported_read"
    );
    assert_eq!(
        after_refresh["referenceFreshness"][0]["restoredPriorObservation"]["sourceCurrent"],
        Value::Null
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(store).unwrap();
    let _ = fs::remove_file(log);
}
