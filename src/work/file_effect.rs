//! Product-mediated UTF-8 file replacement. The run lock fences assignment
//! changes; a durable intent prevents a lost reply from repeating the effect.
//! Native tools outside this mediator retain their host's own permissions.

use crate::{config::Loaded, evidence::hash, project::path, run};
use serde_json::{json, Value};
use std::{io::Read, path::Component};

const MAX_BYTES: u64 = 256 * 1024;

pub(crate) struct WriteRequest<'a> {
    pub work: &'a str,
    pub assignment: &'a str,
    pub operation: &'a str,
    pub path: &'a str,
    pub expected: &'a str,
}

pub(crate) fn write(loaded: &Loaded, request: WriteRequest<'_>) -> Result<Value, String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("file replacement exceeds 256 KiB; no effect was admitted".into());
    }
    let content = String::from_utf8(bytes).map_err(|_| "file replacement must be UTF-8")?;
    replace(loaded, request, &content)
}

fn replace(loaded: &Loaded, request: WriteRequest<'_>, content: &str) -> Result<Value, String> {
    validate_path(request.path)?;
    if request.operation.is_empty()
        || request.operation.len() > 120
        || request.operation.chars().any(char::is_control)
    {
        return Err("file operation needs an identity of 1..=120 printable bytes".into());
    }
    if request.expected != "absent"
        && !(request.expected.len() == 64
            && request.expected.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("expected file state must be absent or its SHA-256".into());
    }
    let ledger = super::resolve(loaded, request.work)?;
    let lock = run::ledger::ledger_path(&loaded.state_root, &ledger, false)?;
    run::ledger::with_lock(&lock, || {
        let root = std::fs::canonicalize(&loaded.product_root).map_err(|e| e.to_string())?;
        let target = root.join(request.path);
        protect(loaded, &target, request.path)?;
        let journal_relative = format!(
            "{}/effects/{}.json",
            crate::project::layout_types::state_namespace(),
            hash::value(&json!([request.work, request.operation]))
        );
        let parameters = hash::value(&json!({
            "work":request.work,"assignment":request.assignment,"operation":request.operation,
            "path":request.path,"expected":request.expected,"contentSha256":hash::text(content)
        }));
        let previous = match path::secure_bytes_observation(
            &loaded.state_root,
            &journal_relative,
            "file effect",
        ) {
            path::SecureBytesResult::Absent(_) => None,
            path::SecureBytesResult::Bytes(bytes) => {
                Some(String::from_utf8(bytes).map_err(|_| "file effect journal is not UTF-8")?)
            }
            _ => return Err("file effect journal is unavailable; no effect admitted".into()),
        };
        if let Some(previous) = previous.as_deref() {
            let recorded: Value =
                serde_json::from_str(previous).map_err(|_| "file effect journal is malformed")?;
            if recorded["version"] != 1 || recorded["parametersSha256"] != parameters {
                return Err("file operation identity conflicts with its recorded request".into());
            }
            if recorded["status"] != "completed" {
                return Err("file operation has an unresolved admitted effect; inspect the file before choosing a new operation".into());
            }
            let now = path::secure_bytes(&root, request.path, "file replay target")?;
            if hash::bytes(&now) != hash::text(content) {
                return Err("recorded file result changed; replay cannot overwrite it".into());
            }
            return Ok(result(&request, &parameters, true));
        }

        if run::ledger::claim_path(&lock).exists() {
            return Err("work was superseded; no file effect admitted".into());
        }
        let (_, events, _) = run::ledger::load_at(loaded, &lock)?;
        let state = run::reduce_live(loaded, &events)?;
        run::assert_no_drift(loaded, &state)?;
        run::artifact::assert_current(loaded, &state)?;
        let assignment = run::assignment::pending(&state)
            .into_iter()
            .find(|a| {
                run::assignment::handle(request.work, a).ok().as_deref() == Some(request.assignment)
            })
            .ok_or("file effect requires the exact current pending assignment")?;
        if assignment["role"] != "worker" {
            return Err("only the current worker may request a file effect".into());
        }
        let allowed = assignment["declaredBoundary"]["write"]
            .as_array()
            .is_some_and(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|pattern| crate::config::boundary::maximum_contains(pattern, request.path))
            });
        if !allowed {
            return Err("file path is outside the current worker write boundary".into());
        }
        // A permit is cooperative authority, never a caller-supplied token.
        // Its canonical grant must belong to this current assignment and remain
        // unconsumed; no earlier Work or completed attempt can supply it.
        let grant = &state["governor"]["currentMutation"];
        if state["governor"]["enabled"] != true
            || state["governor"]["state"] != "ready"
            || grant["attempt"] != assignment["attempt"]
            || !events.iter().any(|event| {
                event["governorEvent"]["eventSha256"] == grant["eventSha256"]
                    && event["assignmentSha256"] == hash::text(request.assignment)
                    && event["governorEvent"]["action"] == "mutation"
            })
        {
            return Err("file effect needs a current unconsumed worker mutation permit".into());
        }
        let expected =
            match path::secure_bytes_observation(&root, request.path, "file effect target") {
                path::SecureBytesResult::Absent(_) if request.expected == "absent" => None,
                path::SecureBytesResult::Bytes(bytes)
                    if hash::bytes(&bytes) == request.expected =>
                {
                    Some(String::from_utf8(bytes).map_err(|_| "existing file must be UTF-8")?)
                }
                _ => return Err("file state differs from expected; no effect admitted".into()),
            };
        let journal_path = loaded.state_root.join(&journal_relative);
        let mode = replacement_mode(&target, expected.is_some())?;
        crate::project::managed_files::ensure_state_directory(
            &loaded.state_root,
            journal_path
                .parent()
                .ok_or("effect journal has no parent")?,
        )?;
        let mut record = json!({"version":1,"kind":"mediated_file_effect",
            "parametersSha256":parameters,"work":request.work,"assignment":request.assignment,
            "operation":request.operation,"path":request.path,"status":"admitted",
            "grantEventSha256":grant["eventSha256"],"resultSha256":hash::text(content)});
        let intent = format!("{record}\n");
        crate::host::settings::atomic_write(
            &journal_path,
            &intent,
            Some(0o600),
            None,
            &loaded.state_root,
        )?;
        // Keep the canonical run lock through the actual replacement. Revoked
        // assignments cannot pass admission after a Lead transition commits.
        crate::host::settings::atomic_write(&target, content, mode, expected.as_deref(), &root)?;
        record["status"] = json!("completed");
        crate::host::settings::atomic_write(
            &journal_path,
            &format!("{record}\n"),
            Some(0o600),
            Some(&intent),
            &loaded.state_root,
        )?;
        Ok(result(&request, &parameters, false))
    })
}

fn replacement_mode(target: &std::path::Path, exists: bool) -> Result<Option<u32>, String> {
    if !exists {
        return Ok(None);
    }
    let info = std::fs::symlink_metadata(target).map_err(|e| e.to_string())?;
    if !info.is_file() || info.file_type().is_symlink() {
        return Err("file effect target must remain an ordinary file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Preserve ordinary permissions, without retaining elevated set-id bits.
        Ok(Some(info.permissions().mode() & 0o777))
    }
    #[cfg(not(unix))]
    {
        Ok(None)
    }
}

fn result(request: &WriteRequest<'_>, parameters: &str, replay: bool) -> Value {
    json!({"work":request.work,"assignment":request.assignment,"operation":request.operation,
        "path":request.path,"status":"completed","requestSha256":parameters,
        "replay":replay,"effect":if replay {"no-change"} else {"file-replaced"},
        "authority":"none","mediatedClass":"utf8_file_replace"})
}

fn validate_path(value: &str) -> Result<(), String> {
    let p = std::path::Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || p.is_absolute()
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
        || p.components()
            .map(|c| {
                c.as_os_str()
                    .to_str()
                    .ok_or("file path component is not UTF-8")
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("/")
            != value
    {
        return Err("file effect needs a normalized project-relative path".into());
    }
    Ok(())
}

fn protect(loaded: &Loaded, target: &std::path::Path, relative: &str) -> Result<(), String> {
    let first = relative.split('/').next().unwrap_or_default();
    if relative.split('/').any(|component| {
        matches!(
            component.to_ascii_lowercase().as_str(),
            ".git"
                | ".exitbind"
                | ".soulmate"
                | ".agents"
                | ".claude"
                | ".codex"
                | "agents.md"
                | "claude.md"
        )
    }) || matches!(first.to_ascii_lowercase().as_str(), "exitbind" | "soulmate")
        || same_control_file(target, &loaded.path)
        || target.starts_with(
            loaded
                .state_root
                .join(crate::project::layout_types::state_namespace()),
        )
        || loaded
            .agents
            .values()
            .any(|a| same_control_file(target, &loaded.control_root.join(&a.profile)))
    {
        return Err("file effect cannot replace authority, evidence or host-control files".into());
    }
    Ok(())
}

fn same_control_file(target: &std::path::Path, control: &std::path::Path) -> bool {
    if target == control
        || target
            .to_str()
            .zip(control.to_str())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (
            std::fs::symlink_metadata(target),
            std::fs::symlink_metadata(control),
        ) {
            // Detect the same configured control file on case-insensitive
            // filesystems and through hard-link aliases, without lossy paths.
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    false
}
