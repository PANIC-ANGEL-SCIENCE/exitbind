use super::*;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

fn fake_provider(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let bin = root.join("fake-provider-bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("codegraph");
    let log = root.with_extension("codegraph.log");
    let source = root.join("impact.rs");
    fs::write(
        &executable,
        format!(
            r#"#!/bin/sh
printf '%s|%s|%s\n' "$1" "${{CODEGRAPH_FORCE_WATCH:-}}" "${{CODEGRAPH_NO_WATCH:-}}" >> "$FAKE_CODEGRAPH_LOG"
case "$1" in
  --version) printf 'CodeGraph 1.6.0-fixture\n' ;;
  status)
    project="$3"
    index="$project/.codegraph"
    if [ ! -f "$index/fake-ready" ]; then
      printf '{{"initialized":false,"projectPath":"%s","indexPath":"%s","lastIndexed":null}}\n' "$project" "$index"
      exit 0
    fi
    generation=1
    if [ -f "$index/fake-generation" ]; then IFS= read -r generation < "$index/fake-generation"; fi
    modified=0
    if [ -f "$index/fake-dirty" ]; then modified=1; fi
    state=complete
    refs=0
    reindex=false
    if [ -f "$index/fake-partial" ]; then state=partial; refs=1; fi
    if [ -f "$index/fake-extraction-stale" ]; then reindex=true; fi
    printf '{{"initialized":true,"version":"1.6.0-fixture","projectPath":"%s","indexPath":"%s","lastIndexed":"fixture-%s","fileCount":1,"nodeCount":1,"edgeCount":1,"dbSizeBytes":128,"backend":"fixture","worktreeMismatch":null,"pendingChanges":{{"added":0,"modified":%s,"removed":0}},"index":{{"builtWithVersion":"1.6.0-fixture","builtWithExtractionVersion":"1","currentExtractionVersion":"1","reindexRecommended":%s,"state":"%s","pendingRefs":%s}}}}\n' "$project" "$index" "$generation" "$modified" "$reindex" "$state" "$refs"
    ;;
  init)
    mkdir -p "$2/.codegraph"
    : > "$2/.codegraph/fake-ready"
    printf '1\n' > "$2/.codegraph/fake-generation"
    ;;
  sync)
    if [ -f .codegraph/fake-change-on-sync ]; then
      printf 'fn changed_during_sync() {{}}\n' >> "$FAKE_SOURCE_FILE"
      rm .codegraph/fake-change-on-sync
    fi
    rm -f .codegraph/fake-dirty
    printf '2\n' > .codegraph/fake-generation
    ;;
  index)
    if [ -f .codegraph/fake-change-on-index ]; then
      printf 'fn changed_during_retry() {{}}\n' >> "$FAKE_SOURCE_FILE"
      rm .codegraph/fake-change-on-index
    fi
    rm -f .codegraph/fake-dirty .codegraph/fake-partial .codegraph/fake-extraction-stale
    printf '3\n' > .codegraph/fake-generation
    ;;
  context)
    case "$8" in
      failure) printf 'synthetic query failure\n' >&2; exit 9 ;;
      malformed) printf '{{"nodes":[]}}\n' ;;
      empty) printf '{{"query":"empty","summary":"No matches","nodes":[],"edges":[],"codeBlocks":[],"entryPoints":[],"relatedFiles":[],"stats":{{"nodes":0,"edges":0}}}}\n' ;;
      provider-replacement)
        printf '#!/bin/sh\nif [ "$1" = "--version" ]; then printf "CodeGraph replacement fixture\\n"; else exit 12; fi\n' > "$0.next"
        chmod +x "$0.next"
        mv "$0.next" "$0"
        printf '{{"query":"%s","summary":"parse_config callers","nodes":[{{"name":"parse_config"}}],"edges":[{{"from":"caller","to":"parse_config"}}],"codeBlocks":[{{"file":"src/config.rs","code":"fn parse_config() {{}}"}}],"entryPoints":[{{"name":"caller"}}],"relatedFiles":[{{"path":"src/config.rs"}}],"stats":{{"nodes":1,"edges":1}}}}\n' "$8"
        ;;
      *) printf '{{"query":"%s","summary":"parse_config callers","nodes":[{{"name":"parse_config"}}],"edges":[{{"from":"caller","to":"parse_config"}}],"codeBlocks":[{{"file":"src/config.rs","code":"fn parse_config() {{}}"}}],"entryPoints":[{{"name":"caller"}}],"relatedFiles":[{{"path":"src/config.rs"}}],"stats":{{"nodes":1,"edges":1}}}}\n' "$8" ;;
    esac
    ;;
  *) printf 'unexpected command\n' >&2; exit 12 ;;
esac
"#
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    (bin, executable, log, source)
}

fn graph_command(
    root: &Path,
    bin: &Path,
    log: &Path,
    source: &Path,
    args: &[&str],
) -> std::process::Output {
    let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .env("PATH", path)
        .env("FAKE_CODEGRAPH_LOG", log)
        .env("FAKE_SOURCE_FILE", source)
        .args(args)
        .output()
        .unwrap()
}

fn graph_json(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "graph command failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn log_count(path: &Path, command: &str) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with(&format!("{command}|")))
        .count()
}

fn commit_fixture(root: &Path) {
    let git = |args: &[&str]| {
        let output = support::git_topology::git(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    git(&["config", "user.name", "Graph fixture"]);
    git(&["config", "user.email", "graph@example.invalid"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "fixture baseline"]);
}

#[test]
fn project_graph_uses_exact_config_from_different_launch_cwd_and_never_borrows_nearby_index() {
    let target = project("project-graph-bound-target");
    let launch = project("project-graph-launch-checkout");
    fs::create_dir_all(launch.join(".codegraph")).unwrap();
    fs::write(
        launch.join(".codegraph/foreign-index.fixture"),
        "fixture belongs only to the launch checkout",
    )
    .unwrap();
    let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&launch)
        .env("GIT_DIR", launch.join(".git"))
        .env("GIT_WORK_TREE", &launch)
        .env("GIT_INDEX_FILE", launch.join(".git/index"))
        .args([
            "project",
            "graph",
            "query",
            "--query",
            "callers of parse_config",
            "--config",
        ])
        .arg(target.join("exitbind.json"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "unavailable graph is a normal query result"
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        result["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("authority_denied"),
        "the explicit target config must resolve before generic launch-cwd discovery: {output:?}"
    );
    assert_eq!(
        result["target"],
        target.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(!target.join(".codegraph").exists());
    assert_eq!(
        fs::read_to_string(launch.join(".codegraph/foreign-index.fixture")).unwrap(),
        "fixture belongs only to the launch checkout"
    );
    fs::remove_dir_all(target).unwrap();
    fs::remove_dir_all(launch).unwrap();
}

#[test]
fn project_graph_reuses_structured_results_and_reconciles_freshness_classes() {
    let root = project("project-graph-protocol");
    fs::write(root.join("impact.rs"), "fn parse_config() {}\n").unwrap();
    let (bin, _, log, source) = fake_provider(&root);
    commit_fixture(&root);
    let config = root.join("exitbind.json");

    let enabled = graph_json(graph_command(
        &root,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "enable",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(enabled["enabled"], true);
    assert_eq!(log_count(&log, "init"), 1);
    assert_eq!(log_count(&log, "sync"), 0);

    let query = [
        "project",
        "graph",
        "query",
        "--query",
        "parse_config",
        "--config",
        config.to_str().unwrap(),
        "--json",
    ];
    let result = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(result["status"], "usable");
    assert_eq!(result["context"]["edges"].as_array().unwrap().len(), 1);
    assert_eq!(result["context"]["codeBlocks"].as_array().unwrap().len(), 1);
    assert_eq!(result["cache"]["reused"], false);
    let reused = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(reused["cache"]["reused"], true);
    assert_eq!(log_count(&log, "context"), 1);

    let empty = graph_json(graph_command(
        &root,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "query",
            "--query",
            "empty",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(empty["status"], "empty");
    let malformed = graph_json(graph_command(
        &root,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "query",
            "--query",
            "malformed",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(malformed["status"], "unavailable");
    assert!(malformed["reason"]
        .as_str()
        .unwrap()
        .contains("query_failure"));
    let failed = graph_json(graph_command(
        &root,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "query",
            "--query",
            "failure",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(failed["status"], "unavailable");
    assert!(failed["reason"]
        .as_str()
        .unwrap()
        .contains("provider_command_failed"));

    // A clean HEAD move has zero provider-pending changes but still requires a
    // full index because the saved prepared source identity is different.
    fs::write(
        &source,
        "fn parse_config() {}\nfn new_committed_caller() {}\n",
    )
    .unwrap();
    let git = |args: &[&str]| {
        let output = support::git_topology::git(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    git(&["add", "impact.rs"]);
    git(&["commit", "-qm", "clean source update"]);
    let before_index = log_count(&log, "index");
    let after_head = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(after_head["status"], "usable");
    assert_eq!(log_count(&log, "index"), before_index + 1);

    // An ordinary dirty transition takes the incremental synchronization path.
    fs::write(&source, "fn parse_config() {}\nfn dirty_caller() {}\n").unwrap();
    fs::write(root.join(".codegraph/fake-dirty"), "1").unwrap();
    let before_sync = log_count(&log, "sync");
    let dirty = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(dirty["status"], "usable");
    assert_eq!(log_count(&log, "sync"), before_sync + 1);

    // Partial and extraction-stale states both require bounded full indexing.
    fs::write(root.join(".codegraph/fake-partial"), "1").unwrap();
    let before_partial_index = log_count(&log, "index");
    let partial = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(partial["status"], "usable");
    assert_eq!(log_count(&log, "index"), before_partial_index + 1);
    fs::write(root.join(".codegraph/fake-extraction-stale"), "1").unwrap();
    let before_stale_index = log_count(&log, "index");
    let stale = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(stale["status"], "usable");
    assert_eq!(log_count(&log, "index"), before_stale_index + 1);

    // One concurrent source edit during sync is reconciled by one full index.
    fs::write(&source, "fn parse_config() {}\nfn edit_before_sync() {}\n").unwrap();
    fs::write(root.join(".codegraph/fake-dirty"), "1").unwrap();
    fs::write(root.join(".codegraph/fake-change-on-sync"), "1").unwrap();
    let before_retry_index = log_count(&log, "index");
    let reconciled = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(reconciled["status"], "usable");
    assert_eq!(log_count(&log, "index"), before_retry_index + 1);

    // A second concurrent edit during that single retry cannot be credited.
    fs::write(&source, "fn parse_config() {}\nfn edit_again() {}\n").unwrap();
    fs::write(root.join(".codegraph/fake-dirty"), "1").unwrap();
    fs::write(root.join(".codegraph/fake-change-on-sync"), "1").unwrap();
    fs::write(root.join(".codegraph/fake-change-on-index"), "1").unwrap();
    let second_change = graph_json(graph_command(&root, &bin, &log, &source, &query));
    assert_eq!(second_change["status"], "unavailable");
    assert!(second_change["reason"]
        .as_str()
        .unwrap()
        .contains("source_changed"));

    let calls = fs::read_to_string(&log).unwrap();
    assert!(!["install", "hook", "upgrade", "serve", "daemon"]
        .iter()
        .any(|name| calls
            .lines()
            .any(|line| line.starts_with(&format!("{name}|")))));
    assert!(calls.lines().any(|line| line == "init|1|"));
    fs::remove_dir_all(root).unwrap();
    let _ = fs::remove_file(log);
}

#[test]
fn project_graph_refuses_provider_identity_drift_before_crediting_query() {
    let root = project("project-graph-provider-drift");
    fs::write(root.join("impact.rs"), "fn parse_config() {}\n").unwrap();
    let (bin, _, log, source) = fake_provider(&root);
    commit_fixture(&root);
    let config = root.join("exitbind.json");
    let enabled = graph_json(graph_command(
        &root,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "enable",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(enabled["enabled"], true);

    let result = graph_json(graph_command(
        &root,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "query",
            "--query",
            "provider-replacement",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(result["status"], "unavailable");
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .contains("provider_changed"));
    assert_eq!(log_count(&log, "context"), 1);
    assert!(!root.join(".codegraph/exitbind-query-cache.json").exists());
    fs::remove_dir_all(root).unwrap();
    let _ = fs::remove_file(log);
}

#[test]
fn project_graph_default_discovery_and_missing_provider_stay_project_bound() {
    let root = project("project-graph-default-target");
    fs::write(root.join("impact.rs"), "fn caller() {}\n").unwrap();
    let (bin, _, log, source) = fake_provider(&root);
    commit_fixture(&root);
    let enable = graph_json(graph_command(
        &root,
        &bin,
        &log,
        &source,
        &["project", "graph", "enable", "--json"],
    ));
    assert_eq!(enable["enabled"], true);
    let path = std::env::join_paths([root.join("empty-provider-path")]).unwrap();
    let unavailable = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .env("PATH", path)
        .args(["project", "graph", "query", "--query", "caller", "--json"])
        .output()
        .unwrap();
    assert!(
        unavailable.status.success(),
        "provider absence is an unavailable result"
    );
    let unavailable: Value = serde_json::from_slice(&unavailable.stdout).unwrap();
    assert_eq!(unavailable["status"], "unavailable");
    assert!(unavailable["reason"]
        .as_str()
        .unwrap()
        .contains("provider_unavailable"));
    assert!(root.join(".codegraph/exitbind-authority.json").exists());
    fs::remove_dir_all(root).unwrap();
    let _ = fs::remove_file(log);
}

#[test]
fn project_graph_reports_aggregate_dirty_byte_bound_without_indexing() {
    let root = project("project-graph-resource-bound");
    let (bin, _, log, source) = fake_provider(&root);
    commit_fixture(&root);
    fs::write(
        root.join("large-dirty.rs"),
        vec![b'x'; 16 * 1024 * 1024 + 1],
    )
    .unwrap();
    let config = root.join("exitbind.json");
    let output = graph_command(
        &root,
        &bin,
        &log,
        &source,
        &[
            "project",
            "graph",
            "enable",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(!output.status.success());
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(failure["error"]
        .as_str()
        .unwrap()
        .contains("source_identity_limit"));
    assert_eq!(log_count(&log, "init"), 0);
    assert_eq!(log_count(&log, "index"), 0);
    fs::remove_dir_all(root).unwrap();
    let _ = fs::remove_file(log);
}

#[path = "graph_recovery.rs"]
mod recovery;
