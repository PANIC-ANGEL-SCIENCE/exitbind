//! One read-only environment readiness assessment for every entry point.
//!
//! The facts are the ones a first record otherwise learns too late: the tested
//! input breadth against the walk limits `run::inputs` already enforces, the
//! Git worktree and its baseline commit, whether the frozen check command's
//! first program can run, the build-target environment the check inherits, and
//! the host's stated write posture. `check` prints the assessment; `setup
//! --apply`, `goal prepare` and `work begin` read it before their first write.
//! A non-Git or broad root stays a stated fact with its cost, not a refusal;
//! only a checker file that exists but cannot run refuses, with one route.

use crate::config::Loaded;
use crate::run::inputs::{WALK_DIRECTORY_LIMIT, WALK_PATH_LIMIT};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The shape of the facts object. Additive changes keep this name; a breaking
/// shape change takes a new name so a reader can tell the forms apart.
pub(crate) const FACTS: &str = "environment-readiness-v1";

/// The whole assessment answers in well under the five-second readiness bound.
const WALK_DEADLINE: Duration = Duration::from_millis(2000);
const GIT_LISTING_LIMIT: usize = 1024 * 1024;

pub(crate) struct Facts<'a> {
    pub(crate) root: &'a Path,
    pub(crate) state_root: &'a Path,
    pub(crate) check_command: Option<&'a str>,
    pub(crate) hosts: &'a [String],
}

/// The configured runtime hosts, in first-seen order. The readiness fact names
/// only hosts the project asked for; an empty list is itself a stated fact.
pub(crate) fn configured_hosts(loaded: &Loaded) -> Vec<String> {
    let mut hosts = Vec::new();
    for agent in loaded.agents.values() {
        if let Some(host) = agent.runtime.host.as_deref() {
            if !host.trim().is_empty() && !hosts.iter().any(|seen| seen == host) {
                hosts.push(host.to_owned());
            }
        }
    }
    hosts
}

pub(crate) fn assess(facts: &Facts<'_>) -> Value {
    let deadline = Instant::now() + WALK_DEADLINE;
    let git = git_fact(facts.root, deadline);
    let breadth = match git["state"].as_str() {
        Some("worktree") => git_breadth(facts.root, facts.state_root, deadline),
        _ => walk_breadth(facts.root, facts.state_root, deadline),
    };
    let checker = checker_fact(facts.root, facts.check_command);
    let target = target_directory(facts.root);
    let ready = breadth["withinLimit"] == json!(true)
        && git["state"] == json!("worktree")
        && git["baseline"].as_str().is_some();
    json!({
        "facts": FACTS,
        "state": if ready { "ready" } else { "attention" },
        "root": {
            "path": text(facts.root),
            "basis": breadth["basis"],
            "files": breadth["files"],
            "directories": breadth["directories"],
            "fileLimit": WALK_PATH_LIMIT,
            "directoryLimit": WALK_DIRECTORY_LIMIT,
            "withinLimit": breadth["withinLimit"],
            "sampled": breadth["sampled"],
            "detail": breadth["detail"],
        },
        "git": git,
        "checker": checker,
        "checkerEnvironment": {
            "cargoTargetDir": std::env::var_os("CARGO_TARGET_DIR")
                .and_then(|value| value.to_str().map(str::to_owned)),
            "targetDirectory": text(&target.0),
            "targetPresent": target.1,
            "targetOwnership": "not stated",
        },
        "hostWrite": host_write_fact(facts.hosts),
    })
}

/// The build directory the check will inherit: `CARGO_TARGET_DIR` when it is
/// set (relative values resolve against the product root), else `target/`
/// beside the project. Its presence is a fact; its ownership is not.
fn target_directory(root: &Path) -> (PathBuf, bool) {
    let path = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(value) if !value.is_empty() => {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        }
        _ => root.join("target"),
    };
    let present = path.is_dir();
    (path, present)
}

/// The one refusal readiness owns: a checker program that exists but cannot
/// run. A missing program has no single runnable route, so it stays a stated
/// fact and the check command's own failure route reports it.
pub(crate) fn checker_refusal(root: &Path, command: &str) -> Option<String> {
    let Some(program) = crate::session_goal::requirements::checker_program(command.trim()) else {
        return None;
    };
    let outcome = program_state(root, &program);
    if outcome.state != "not-executable" {
        return None;
    }
    let path = normalized(&outcome.path?);
    Some(format!(
        "checker program {} exists but is not executable; run `{}` and retry",
        shown_path(&path),
        executable_route(&path)
    ))
}

/// The route the operator can run unedited, with the path shell-quoted.
fn executable_route(path: &Path) -> String {
    format!(
        "chmod +x {}",
        crate::presentation::shell_quote(&shown_path(path))
    )
}

/// A path as text with its control characters spelled out.
fn shown_path(path: &Path) -> String {
    crate::presentation::escaped(&path.to_string_lossy())
}

/// The path without `.` components, so a relative command names the same file
/// the operator sees rather than `./`-laden joins.
fn normalized(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            other => clean.push(other),
        }
    }
    clean
}

/// Human lines for `check`, one per fact. The caller prints them in place.
pub(crate) fn text_lines(readiness: &Value) -> Vec<String> {
    let root = &readiness["root"];
    let git = &readiness["git"];
    let checker = &readiness["checker"];
    let environment = &readiness["checkerEnvironment"];
    let mut lines = vec![format!(
        "Environment readiness: {} (before any Work record)",
        readiness["state"].as_str().unwrap_or("unknown")
    )];
    lines.push(format!(
        "  root {}: {}",
        crate::presentation::escaped(root["path"].as_str().unwrap_or("unknown")),
        breadth_sentence(root)
    ));
    lines.push(format!("  Git: {}", git_sentence(git)));
    lines.push(format!("  checker: {}", checker_sentence(checker)));
    // State the build directory only when a check would inherit one. Its
    // ownership stays unstated: a stale foreign target can be reused silently.
    if environment["cargoTargetDir"].is_string() || environment["targetPresent"] == json!(true) {
        lines.push(format!(
            "  checker environment: CARGO_TARGET_DIR {}; target directory {} at {}; target ownership not stated",
            environment["cargoTargetDir"]
                .as_str()
                .map(crate::presentation::escaped)
                .unwrap_or_else(|| "not set".into()),
            if environment["targetPresent"] == json!(true) {
                "present"
            } else {
                "absent"
            },
            crate::presentation::escaped(
                environment["targetDirectory"]
                    .as_str()
                    .unwrap_or("an unknown path")
            )
        ));
    }
    lines.push(format!(
        "  host write: {}",
        host_write_sentence(&readiness["hostWrite"])
    ));
    lines
}

fn breadth_sentence(root: &Value) -> String {
    let basis = root["basis"].as_str().unwrap_or("walk");
    let files = root["files"].as_u64().unwrap_or(0);
    let directories = root["directories"].as_u64();
    let file_limit = root["fileLimit"].as_u64().unwrap_or(0);
    let directory_limit = root["directoryLimit"].as_u64().unwrap_or(0);
    if root["sampled"] == json!(true) {
        return format!(
            "not counted within the {} ms readiness budget; the tested-input walk stops at {file_limit} paths or {directory_limit} directories",
            WALK_DEADLINE.as_millis()
        );
    }
    match (basis, root["withinLimit"].as_bool()) {
        ("git-listing", Some(true)) => format!(
            "{files} paths listed by Git; within the {file_limit}-path tested-input limit"
        ),
        ("git-listing", Some(false)) => format!(
            "{files} paths listed by Git, over the {file_limit}-path tested-input limit; `work resume` cannot fingerprint this root"
        ),
        (_, Some(true)) => format!(
            "{files} files and {} directories under the root; within the tested-input walk limit ({file_limit} paths / {directory_limit} directories)",
            directories.unwrap_or(0)
        ),
        (_, Some(false)) => format!(
            "more than {file_limit} files or {directory_limit} directories under the root, over the tested-input walk limit; a non-Git root is walked on every tested-input fingerprint, so a broad root costs minutes"
        ),
        (_, None) => format!(
            "not counted: {}; the count did not complete, so the root's tested-input breadth is unknown against {file_limit} paths / {directory_limit} directories",
            crate::presentation::escaped(
                root["detail"].as_str().unwrap_or("the count did not complete")
            )
        ),
    }
}

fn git_sentence(git: &Value) -> String {
    let shown = |key: &str| {
        git[key]
            .as_str()
            .map(crate::presentation::escaped)
            .unwrap_or_else(|| "unknown".into())
    };
    match git["state"].as_str() {
        Some("worktree") => match git["baseline"].as_str() {
            Some(baseline) => format!(
                "worktree {}; baseline commit {baseline}",
                shown("worktree")
            ),
            None => format!(
                "worktree {}; no baseline commit yet (the tested-input identity and review baseline need one)",
                shown("worktree")
            ),
        },
        Some("not-git") => "not a Git worktree (no baseline commit); the Codex CLI refuses a product-managed Worker launch in a directory that is not a trusted Git worktree, and a non-Git root is walked on every tested-input fingerprint".into(),
        Some(_) => format!("unresolved: {}", shown("detail")),
        None => "not stated".into(),
    }
}

fn checker_sentence(checker: &Value) -> String {
    let shown = |key: &str| {
        checker[key]
            .as_str()
            .map(crate::presentation::escaped)
            .unwrap_or_else(|| "the program".into())
    };
    match checker["state"].as_str() {
        Some("not-stated") => "not stated; no frozen check command exists yet".into(),
        Some("runnable") => format!(
            "{} resolves to {}; runnable",
            shown("program"),
            shown("path")
        ),
        Some("not-executable") => format!(
            "{} exists but is not executable; work begin refuses until it can run (`{}`)",
            shown("path"),
            executable_route(Path::new(
                checker["path"].as_str().unwrap_or_default()
            ))
        ),
        Some("missing") => format!(
            "{} does not exist; the check will fail when it runs",
            shown("path")
        ),
        Some("unresolved") => format!(
            "`{}` is not on this shell's PATH; a bare word may be a shell builtin, so the product states it without refusing",
            shown("program")
        ),
        _ => "not stated".into(),
    }
}

fn host_write_sentence(host_write: &Value) -> String {
    let hosts = host_write["hosts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if hosts.is_empty() {
        return "not stated (no runtime host is configured); a Codex launch requests workspace-write for an ordinary worker and read-only for a reviewer or controlled worker, and Claude Code's own permissions and project hook decide".into();
    }
    hosts
        .iter()
        .map(|entry| match entry["host"].as_str() {
            Some("codex") => "Codex: the launch requests workspace-write for an ordinary worker and read-only for a reviewer or controlled worker".to_owned(),
            Some("claude") => "Claude: not stated; Claude Code's permissions and the project hook decide, and the product chooses no sandbox".to_owned(),
            Some(other) => format!("{other}: not stated; the product has no sandbox fact for this host"),
            None => "not stated".into(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn git_fact(root: &Path, deadline: Instant) -> Value {
    match crate::project::git_preflight::worktree_root(root) {
        Ok(Some(top)) => json!({
            "state": "worktree",
            "worktree": text(&top),
            "baseline": git_head(&top, deadline),
        }),
        Ok(None) => json!({"state": "not-git", "worktree": null, "baseline": null}),
        Err(error) => json!({
            "state": "problem",
            "worktree": null,
            "baseline": null,
            "detail": error,
        }),
    }
}

fn git_head(top: &Path, deadline: Instant) -> Value {
    let Some(worktree) = top.to_str() else {
        return json!(null);
    };
    let mut command = Command::new("git");
    command.args(["-C", worktree, "rev-parse", "--verify", "-q", "HEAD"]);
    match crate::kernel::bounded_process::run(&mut command, 4096, 4096, deadline) {
        Ok(output) if output.status.success() => {
            let sha = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if sha.len() == 40 && sha.chars().all(|byte| byte.is_ascii_hexdigit()) {
                json!(sha)
            } else {
                json!(null)
            }
        }
        _ => json!(null),
    }
}

fn git_breadth(root: &Path, state_root: &Path, deadline: Instant) -> Value {
    let Some(text) = root.to_str() else {
        return unavailable_breadth("git-listing", "the product root is not valid UTF-8");
    };
    let mut command = Command::new("git");
    command.arg("-C").arg(text).args([
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
        ".",
    ]);
    let output =
        match crate::kernel::bounded_process::run(&mut command, GIT_LISTING_LIMIT, 65536, deadline)
        {
            Ok(output) => output,
            Err(error) => {
                return unavailable_breadth(
                    "git-listing",
                    &format!("git listing {}: {}", error.reason, error.detail),
                )
            }
        };
    if !output.status.success() {
        return unavailable_breadth("git-listing", "Git could not list the tested inputs");
    }
    let mut files = 0usize;
    for path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let Ok(path) = std::str::from_utf8(path) else {
            continue;
        };
        if crate::run::inputs::excluded(root, state_root, Path::new(path)) {
            continue;
        }
        files += 1;
        if files > WALK_PATH_LIMIT {
            break;
        }
    }
    json!({
        "basis": "git-listing",
        "files": files,
        "directories": null,
        "withinLimit": files <= WALK_PATH_LIMIT,
        "sampled": false,
        "detail": null,
    })
}

fn walk_breadth(root: &Path, state_root: &Path, deadline: Instant) -> Value {
    let mut files = 0usize;
    let mut directories = 1usize;
    let mut pending = vec![PathBuf::new()];
    let mut sampled = false;
    let mut unknown = false;
    let mut detail = None;
    'walk: while let Some(relative) = pending.pop() {
        if Instant::now() >= deadline {
            sampled = true;
            break;
        }
        let listed = if relative.as_os_str().is_empty() {
            ".".to_owned()
        } else {
            shown_path(&relative)
        };
        let entries = match std::fs::read_dir(root.join(&relative)) {
            Ok(entries) => entries,
            Err(error) => {
                detail = Some(format!(
                    "the tested-input walk cannot list {listed}: {error}"
                ));
                unknown = true;
                break;
            }
        };
        for entry in entries {
            if Instant::now() >= deadline {
                sampled = true;
                break 'walk;
            }
            let Ok(entry) = entry else {
                detail = Some(format!(
                    "the tested-input walk cannot read an entry under {listed}"
                ));
                unknown = true;
                break 'walk;
            };
            let path = relative.join(entry.file_name());
            if crate::run::inputs::excluded(root, state_root, &path) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                detail = Some(format!(
                    "the tested-input walk cannot read the type of {}",
                    shown_path(&path)
                ));
                unknown = true;
                break 'walk;
            };
            if kind.is_dir() {
                directories += 1;
                if directories > WALK_DIRECTORY_LIMIT {
                    return json!({
                        "basis": "walk",
                        "files": files,
                        "directories": directories,
                        "withinLimit": false,
                        "sampled": false,
                        "detail": "the root has more directories than the tested-input walk limit",
                    });
                }
                pending.push(path);
            } else {
                files += 1;
                if files > WALK_PATH_LIMIT {
                    return json!({
                        "basis": "walk",
                        "files": files,
                        "directories": directories,
                        "withinLimit": false,
                        "sampled": false,
                        "detail": "the root has more files than the tested-input walk limit",
                    });
                }
            }
        }
    }
    json!({
        "basis": "walk",
        "files": files,
        "directories": directories,
        "withinLimit": if sampled || unknown { Value::Null } else { json!(true) },
        "sampled": sampled,
        "detail": detail,
    })
}

fn unavailable_breadth(basis: &str, detail: &str) -> Value {
    json!({
        "basis": basis,
        "files": Value::Null,
        "directories": null,
        "withinLimit": Value::Null,
        "sampled": false,
        "detail": detail,
    })
}

struct ProgramState {
    state: &'static str,
    path: Option<PathBuf>,
    detail: Option<String>,
}

fn checker_fact(root: &Path, command: Option<&str>) -> Value {
    let Some(command) = command.map(str::trim).filter(|command| !command.is_empty()) else {
        return json!({
            "state": "not-stated",
            "program": null,
            "path": null,
            "detail": null,
        });
    };
    let Some(program) = crate::session_goal::requirements::checker_program(command) else {
        return json!({
            "state": "not-stated",
            "program": null,
            "path": null,
            "detail": null,
        });
    };
    let outcome = program_state(root, &program);
    json!({
        "state": outcome.state,
        "program": program,
        "path": outcome.path.as_deref().map(|path| text(&normalized(path))),
        "detail": outcome.detail,
    })
}

fn program_state(root: &Path, program: &str) -> ProgramState {
    if program.contains('/') {
        let path = if Path::new(program).is_absolute() {
            PathBuf::from(program)
        } else {
            root.join(program)
        };
        return match std::fs::metadata(&path) {
            Ok(metadata) if !metadata.is_file() => ProgramState {
                state: "missing",
                path: Some(path),
                detail: Some("the program path is not a regular file".into()),
            },
            Ok(_) if crate::host::settings::executable_file(&path) => ProgramState {
                state: "runnable",
                path: Some(path),
                detail: None,
            },
            Ok(_) => ProgramState {
                state: "not-executable",
                path: Some(path),
                detail: None,
            },
            Err(error) => ProgramState {
                state: "missing",
                path: Some(path),
                detail: Some(error.to_string()),
            },
        };
    }
    if let Some(path) = find_on_path(program) {
        return ProgramState {
            state: "runnable",
            path: Some(path),
            detail: None,
        };
    }
    ProgramState {
        state: "unresolved",
        path: None,
        detail: Some(
            "a bare program is resolved by the shell that runs the check; this shell's PATH does not name it"
                .into(),
        ),
    }
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| crate::host::settings::executable_file(candidate))
}

fn host_write_fact(hosts: &[String]) -> Value {
    if hosts.is_empty() {
        return json!({
            "state": "not-stated",
            "hosts": [],
            "detail": "no runtime host is configured; a Codex launch requests workspace-write for an ordinary worker and read-only for a reviewer or controlled worker, and Claude Code's own permissions and project hook decide",
        });
    }
    let entries: Vec<Value> = hosts
        .iter()
        .map(|host| match host.as_str() {
            "codex" => json!({
                "host": "codex",
                "requested": "workspace-write",
                "state": "stated",
                "basis": "the launch request selects workspace-write for an ordinary worker; a reviewer or controlled worker is read-only",
            }),
            "claude" => json!({
                "host": "claude",
                "requested": null,
                "state": "not-stated",
                "basis": "Claude Code's permissions and the project hook decide; the product chooses no sandbox",
            }),
            other => json!({
                "host": other,
                "requested": null,
                "state": "not-stated",
                "basis": "the product has no sandbox fact for this host",
            }),
        })
        .collect();
    let stated = entries
        .iter()
        .any(|entry| entry["state"] == json!("stated"));
    json!({
        "state": if stated { "stated" } else { "not-stated" },
        "hosts": entries,
        "detail": null,
    })
}

fn text(path: &Path) -> Value {
    path.to_str().map_or(Value::Null, |value| json!(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(label: &str) -> crate::test_root::TempRoot {
        crate::test_root::temp(label)
    }

    #[test]
    fn walk_breadth_stops_at_the_shared_limit() {
        let root = fixture("readiness-walk-limit");
        let bulk = root.join("bulk");
        std::fs::create_dir(&bulk).unwrap();
        for index in 0..(WALK_PATH_LIMIT + 8) {
            std::fs::write(bulk.join(format!("entry-{index:04}")), b"x").unwrap();
        }
        let value = walk_breadth(&root, &root, Instant::now() + Duration::from_secs(10));
        assert_eq!(value["withinLimit"], json!(false));
        assert_eq!(value["basis"], json!("walk"));
        assert!(value["files"].as_u64().unwrap() <= (WALK_PATH_LIMIT + 1) as u64);
    }

    #[test]
    fn checker_fact_separates_missing_and_not_executable() {
        let root = fixture("readiness-checker");
        let script = root.join("check.sh");
        std::fs::write(&script, b"#!/bin/sh\nexit 0\n").unwrap();
        let missing = checker_fact(&root, Some("./absent.sh"));
        assert_eq!(missing["state"], json!("missing"));
        let not_executable = checker_fact(&root, Some("./check.sh"));
        assert_eq!(not_executable["state"], json!("not-executable"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(
                checker_fact(&root, Some("./check.sh"))["state"],
                json!("runnable")
            );
            assert!(checker_refusal(&root, "./check.sh").is_none());
        }
        assert!(checker_refusal(&root, "./absent.sh").is_none());
        assert!(checker_fact(&root, None)["state"] == json!("not-stated"));
    }

    #[test]
    fn a_bare_word_that_is_a_shell_builtin_is_stated_without_refusing() {
        let root = fixture("readiness-builtin");
        let value = checker_fact(&root, Some("exec sh check.sh"));
        assert_eq!(value["program"], json!("exec"));
        assert!(matches!(
            value["state"].as_str(),
            Some("unresolved" | "runnable")
        ));
        assert!(checker_refusal(&root, "exec sh check.sh").is_none());
    }
}
