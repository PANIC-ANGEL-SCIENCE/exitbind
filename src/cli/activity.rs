//! Lightweight observation for an explicitly direct Codex task.

use super::{args, config, positional, print_json, Arguments};
use crate::host::{change_snapshot, codex_exec, settings};
use crate::project::{layout_types, managed_files};
use serde_json::{json, Value};
use std::fs;
use std::io::{IsTerminal, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_PROMPT_BYTES: u64 = 64 * 1024;

pub(super) fn command(loaded: &config::Loaded, args: &Arguments) -> Result<(), String> {
    match positional(args, 0, "activity requires codex or show")? {
        "codex" => run_codex(loaded, args),
        "show" => show(loaded, args),
        _ => Err("activity requires codex or show".into()),
    }
}

fn run_codex(loaded: &config::Loaded, args: &Arguments) -> Result<(), String> {
    args::assert_options(
        "activity codex",
        args,
        &[
            "config",
            "json",
            "codex-bin",
            "model",
            "reasoning-effort",
            "sandbox-mode",
            "timeout-ms",
        ],
    )?;
    args::assert_positionals("activity codex", args, 1)?;
    if std::io::stdin().is_terminal() {
        return Err("activity codex requires a task prompt on standard input".into());
    }
    let mut prompt = Vec::new();
    std::io::stdin()
        .take(MAX_PROMPT_BYTES + 1)
        .read_to_end(&mut prompt)
        .map_err(|error| format!("direct task prompt could not be read: {error}"))?;
    if prompt.is_empty() || prompt.len() as u64 > MAX_PROMPT_BYTES {
        return Err("direct task prompt must contain 1–65536 bytes".into());
    }
    let prompt = String::from_utf8(prompt)
        .map_err(|_| "direct task prompt must be valid UTF-8".to_owned())?;
    let timeout = args
        .options
        .get("timeout-ms")
        .map_or(Ok(300_000_u64), |value| {
            value
                .parse::<u64>()
                .ok()
                .filter(|value| (1..=1_800_000).contains(value))
                .ok_or("--timeout-ms must be between 1 and 1800000")
        })?;
    let executable = codex_exec::resolve_codex(args.options.get("codex-bin").map(Path::new))
        .map_err(|error| error.to_string())?;
    let request = codex_exec::Request {
        executable,
        cwd: loaded.product_root.clone(),
        prompt,
        model: args.options.get("model").cloned(),
        effort: args.options.get("reasoning-effort").cloned(),
        sandbox: Some(
            args.options
                .get("sandbox-mode")
                .cloned()
                .unwrap_or_else(|| "workspace-write".into()),
        ),
        output_schema: None,
        resume_thread_id: None,
        persist_session: false,
        timeout: Duration::from_millis(timeout),
    };
    let directory = loaded
        .state_root
        .join(layout_types::state_namespace())
        .join("activity");
    managed_files::ensure_state_directory(&loaded.state_root, &directory)?;
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let id = format!("act_{created:x}_{:x}", std::process::id());
    let path = directory.join(format!("{id}.json"));
    let started = json!({
        "version": 1,
        "id": id,
        "governance": "direct",
        "status": "started",
        "taskOutcome": "unjudged",
        "host": "codex",
        "cwd": loaded.product_root,
        "model": request.model,
        "reasoningEffort": request.effort,
        "retention": "private_project_state",
    });
    let started_text = started.to_string();
    let mut marker = managed_files::open_state_file(&path).map_err(|error| error.to_string())?;
    marker
        .write_all(started_text.as_bytes())
        .and_then(|_| marker.sync_all())
        .map_err(|error| error.to_string())?;
    drop(marker);

    let before = change_snapshot::capture(&loaded.product_root);
    let result = codex_exec::run(&request);
    let changed_result =
        change_snapshot::result(before, change_snapshot::capture(&loaded.product_root));
    let mut record = started;
    match result {
        Ok(observation) => {
            record["status"] = json!(if observation.process.success
                && observation.turn == codex_exec::TurnStatus::Completed
            {
                "observed"
            } else {
                "incomplete"
            });
            record["observation"] = projection(&observation);
            record["observation"]["changedResult"] = changed_result;
        }
        Err(error) => {
            record["status"] = json!("capture_gap");
            record["observation"] = json!({
                "coverageGap": ["native_result_unavailable"],
                "error": error.to_string(),
                "changedResult": changed_result,
            });
        }
    }
    let real_root = fs::canonicalize(&loaded.state_root).map_err(|error| error.to_string())?;
    settings::atomic_write(
        &path,
        &record.to_string(),
        Some(0o600),
        Some(&started_text),
        &real_root,
    )
    .map_err(|error| {
        format!(
            "direct native action may have run; capture update failed at {}: {error}",
            path.display()
        )
    })?;
    let response = json!({
        "activity": id,
        "governance": "direct",
        "status": record["status"],
        "taskOutcome": "unjudged",
        "observation": record["observation"],
        "record": relative_record(&path, &loaded.state_root)?,
        "acceptance": "not_applicable",
    });
    print_json(&response)
}

fn show(loaded: &config::Loaded, args: &Arguments) -> Result<(), String> {
    args::assert_options("activity show", args, &["config", "json"])?;
    args::assert_positionals("activity show", args, 2)?;
    let id = positional(args, 1, "activity show requires ACTIVITY_ID")?;
    let parts = id
        .strip_prefix("act_")
        .map(|suffix| suffix.split('_').collect::<Vec<_>>());
    if id.len() > 80
        || !parts.as_ref().is_some_and(|parts| {
            parts.len() == 2
                && parts.iter().all(|part| {
                    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        })
    {
        return Err("activity ID is invalid".into());
    }
    let directory = loaded
        .state_root
        .join(layout_types::state_namespace())
        .join("activity");
    let dir_info = fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
    if dir_info.file_type().is_symlink() || !dir_info.is_dir() {
        return Err("activity directory is unsafe".into());
    }
    #[cfg(unix)]
    if dir_info.permissions().mode() & 0o777 != 0o700 {
        return Err("activity directory permissions are not private".into());
    }
    let real_root = fs::canonicalize(&loaded.state_root).map_err(|error| error.to_string())?;
    let real_directory = fs::canonicalize(&directory).map_err(|error| error.to_string())?;
    if !real_directory.starts_with(real_root) {
        return Err("activity directory escaped private state".into());
    }
    let path = directory.join(format!("{id}.json"));
    let info = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if info.file_type().is_symlink() || !info.is_file() || info.len() > 64 * 1024 {
        return Err("activity record is unsafe or exceeds 65536 bytes".into());
    }
    #[cfg(unix)]
    if info.permissions().mode() & 0o777 != 0o600 {
        return Err("activity record permissions are not private".into());
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let record: Value =
        serde_json::from_slice(&bytes).map_err(|_| "activity record is malformed".to_owned())?;
    if record["id"] != id || record["governance"] != "direct" {
        return Err("activity record identity is invalid".into());
    }
    print_json(&record)
}

fn relative_record(path: &Path, state_root: &Path) -> Result<String, String> {
    path.strip_prefix(state_root)
        .map_err(|_| "activity record escaped private state".to_owned())?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "activity record path is not UTF-8".to_owned())
}

fn projection(observation: &codex_exec::Observation) -> Value {
    let commands = observation
        .command_outcomes
        .iter()
        .map(|item| {
            json!({
                "hostItemId": item.host_item_id,
                "invocationSha256": item.invocation_sha256,
                "status": item.status,
                "exitCode": item.exit_code,
                "outputBytes": item.output_bytes,
            })
        })
        .collect::<Vec<_>>();
    let usage = observation.usage.as_ref().map(|usage| {
        json!({
            "source": usage.source,
            "inputTokens": usage.input_tokens,
            "cachedInputTokens": usage.cached_input_tokens,
            "outputTokens": usage.output_tokens,
        })
    });
    json!({
        "host": "codex",
        "threadId": observation.thread_id,
        "process": {
            "code": observation.process.code,
            "signal": observation.process.signal,
            "success": observation.process.success,
            "timedOut": observation.process.timed_out,
        },
        "turn": observation.turn.as_str(),
        "commands": commands,
        "usage": usage,
        "coverageGap": observation.coverage_gap.iter().map(|gap| gap.as_str()).collect::<Vec<_>>(),
        "changedResult": "not_observed_by_native_event_stream",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn direct_projection_preserves_failed_command_without_acceptance() {
        let observation = codex_exec::Observation {
            executable: PathBuf::from("/bin/codex"),
            cwd: PathBuf::from("/project"),
            ephemeral: true,
            process: codex_exec::ProcessOutcome {
                code: Some(0),
                signal: None,
                success: true,
                timed_out: false,
            },
            turn: codex_exec::TurnStatus::Completed,
            command_outcomes: vec![codex_exec::CommandOutcome {
                status: "failed".into(),
                exit_code: Some(7),
                output_bytes: 0,
                invocation_sha256: Some("id".into()),
                host_item_id: Some("item-1".into()),
            }],
            usage: None,
            thread_id: Some("thread-1".into()),
            final_result: None,
            coverage_gap: vec![codex_exec::CoverageGap::MissingUsage],
            interrupted: false,
        };
        let value = projection(&observation);
        assert_eq!(value["commands"][0]["exitCode"], 7);
        assert_eq!(value["process"]["code"], 0);
        assert_eq!(value["coverageGap"][0], "missing_usage");
        assert!(value.get("accepted").is_none());
    }
}
