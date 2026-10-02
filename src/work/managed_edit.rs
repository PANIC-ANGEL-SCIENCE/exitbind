//! Assignment-bound file interaction. Native tools outside it retain host authority.
mod state;
use super::file_effect;
use crate::{config::Loaded, evidence::hash, project::managed_files, run};
use serde_json::{json, Value};
use state::{Baseline, Binding, Reads, Status, Submission};
use std::{collections::BTreeMap, io::Read};

pub(crate) fn prepare(loaded: &Loaded, work: &str, assignment: &str) -> Result<Value, String> {
    let binding = state::binding(loaded, work, assignment)?;
    let session = hash::value(&json!([work, assignment]));
    let ledger = super::resolve(loaded, work)?;
    let lock = run::ledger::ledger_path(&loaded.state_root, &ledger, false)?;
    run::ledger::with_lock(&lock, || {
        current_worker(loaded, &binding, &lock)?;
        let directory = loaded
            .state_root
            .join(state::relative(&session, "binding.json")?)
            .parent()
            .ok_or("managed edit directory missing")?
            .to_path_buf();
        let parent = directory.parent().ok_or("managed edit parent missing")?;
        managed_files::ensure_state_directory(&loaded.state_root, parent)?;
        let tool = directory.join("edit");
        let prepared = state::preparation(loaded, &lock, &session, &binding)?;
        let script = format!("#!/bin/sh\nif [ \"$#\" -ne 2 ]; then printf '%s\\n' 'Usage: edit read|refresh|edit|inspect PATH' >&2; exit 2; fi\nexec {} work file \"$1\" {} \"$2\" --config {}\n",
            crate::presentation::shell_quote(&binding.executable),
            crate::presentation::shell_quote(&session),
            crate::presentation::shell_quote(&binding.config));
        if directory.exists() {
            if !prepared {
                return Err(
                    "managed preparation registry is missing; no session reconstructed".into(),
                );
            }
            if state::load_binding(loaded, &session)? != binding {
                return Err("managed edit binding changed; no new session created".into());
            }
            state::load_reads(loaded, &session)?;
            let bytes = state::observe(
                &loaded.state_root,
                &state::relative(&session, "edit")?,
                16 * 1024,
            )?;
            if bytes.as_deref() != Some(script.as_bytes()) {
                return Err(
                    "managed edit tool is missing or changed; no session reconstructed".into(),
                );
            }
        } else {
            if prepared {
                return Err(
                    "managed edit session state is missing; no session reconstructed".into(),
                );
            }
            state::record_preparation(loaded, &lock, &session, &binding)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&directory)
                    .map_err(|e| e.to_string())?;
            }
            #[cfg(not(unix))]
            return Err("managed editing requires Unix no-follow support".into());
            let record = serde_json::to_string(&binding).map_err(|e| e.to_string())?;
            for (leaf, bytes, mode) in [
                ("binding.json", record, 0o600),
                (
                    "reads.json",
                    serde_json::to_string(&Reads {
                        version: 1,
                        files: BTreeMap::new(),
                    })
                    .map_err(|e| e.to_string())?,
                    0o600,
                ),
                ("edit", script, 0o700),
            ] {
                crate::host::settings::atomic_write(
                    &directory.join(leaf),
                    &bytes,
                    Some(mode),
                    None,
                    &loaded.state_root,
                )?;
            }
        }
        Ok(
            json!({"tool": tool.to_str().ok_or("managed edit tool path is not UTF-8")?,
            "session":session,"authority":"none","status":"ready"}),
        )
    })
}

pub(crate) fn interact(
    loaded: &Loaded,
    action: &str,
    session: &str,
    file: &str,
) -> Result<Value, String> {
    if !matches!(action, "read" | "refresh" | "edit" | "inspect") {
        return Err("managed file action must be read, refresh, edit or inspect".into());
    }
    file_path(file)?;
    let content = if action == "edit" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(file_effect::MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > file_effect::MAX_BYTES {
            return Err("file replacement exceeds 256 KiB; no edit admitted".into());
        }
        Some(String::from_utf8(bytes).map_err(|_| "file replacement must be UTF-8")?)
    } else {
        None
    };
    let binding = state::load_binding(loaded, session)?;
    let ledger = super::resolve(loaded, &binding.work)?;
    let lock = run::ledger::ledger_path(&loaded.state_root, &ledger, false)?;
    run::ledger::with_lock(&lock, || {
        let assignment = current_worker(loaded, &binding, &lock)?;
        if !state::preparation(loaded, &lock, session, &binding)? {
            return Err("managed preparation registry is missing; no edit admitted".into());
        }
        let observable = assignment["declaredBoundary"]["observe"]
            .as_array()
            .is_some_and(|patterns| {
                patterns
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|p| crate::config::boundary::maximum_contains(p, file))
            });
        if !observable {
            return Err("file is outside the current worker read boundary".into());
        }
        file_effect::protect(loaded, &loaded.product_root.join(file), file)?;
        let (mut reads, mut original) = state::load_reads(loaded, session)?;
        if action == "inspect" {
            return inspect(loaded, session, file, reads.files.get(file));
        }
        if matches!(action, "read" | "refresh") {
            let old = reads.files.get(file);
            if action == "refresh" {
                if let Some(old) = old {
                    resolved_for_refresh(loaded, &binding, session, file, old)?;
                }
            }
            if old.is_none() || action == "refresh" {
                let generation = old.map_or(Ok(1), |b| {
                    b.generation
                        .checked_add(1)
                        .ok_or("managed read generation exhausted")
                })?;
                let content = state::observe(&loaded.product_root, file, file_effect::MAX_BYTES)?
                    .map(String::from_utf8)
                    .transpose()
                    .map_err(|_| "managed file must be UTF-8")?;
                let expected_sha256 = content
                    .as_deref()
                    .map(hash::text)
                    .unwrap_or_else(|| "absent".into());
                reads.files.insert(
                    file.into(),
                    Baseline {
                        generation,
                        content,
                        expected_sha256,
                        request: None,
                    },
                );
                state::save_reads(loaded, session, &reads, &original)?;
            }
            let baseline = reads
                .files
                .get(file)
                .ok_or("managed baseline unavailable")?;
            return Ok(
                json!({"path":file,"status":"read","exists":baseline.content.is_some(),
                "content":baseline.content,"baseline":"captured","authority":"none"}),
            );
        }
        let content = content
            .as_deref()
            .ok_or("managed replacement unavailable")?;
        let baseline = reads
            .files
            .get_mut(file)
            .ok_or("read this file with the supplied tool before editing it")?;
        if let Some(request) = baseline.request.as_ref() {
            if request.content_sha256 != hash::text(content) {
                return Err("submitted edit conflicts with its durable request; inspect it, then refresh only for an intentional new edit".into());
            }
            if request.status == Status::Refused {
                return Err(format!(
                    "previous edit was refused: {}; use inspect, then refresh for a new edit",
                    request.refusal.as_deref().unwrap_or("unavailable")
                ));
            }
            let effect = effect_record(loaded, &binding, session, file, baseline)?;
            if effect.as_ref().map_or(true, |e| e["status"] != "completed") {
                return Err("edit has an unresolved or missing effect record; use inspect and do not retry blindly".into());
            }
        } else {
            baseline.request = Some(Submission {
                content_sha256: hash::text(content),
                status: Status::Submitted,
                refusal: None,
            });
            state::save_reads(loaded, session, &reads, &original)?;
            original = serde_json::to_string(&reads).map_err(|e| e.to_string())?;
        }
        let baseline = reads
            .files
            .get(file)
            .ok_or("managed baseline missing")?
            .clone();
        let operation = state::operation(session, file, &baseline);
        let expected = state::expected(&baseline);
        let result = file_effect::replace_locked(
            loaded,
            file_effect::WriteRequest {
                work: &binding.work,
                assignment: &binding.assignment,
                operation: &operation,
                path: file,
                expected: &expected,
            },
            content,
            &lock,
        );
        let request = reads
            .files
            .get_mut(file)
            .and_then(|b| b.request.as_mut())
            .ok_or("managed submission missing")?;
        match &result {
            Ok(_) => request.status = Status::Completed,
            Err(error) => {
                if effect_record(loaded, &binding, session, file, &baseline)?.is_none() {
                    request.status = Status::Refused;
                    request.refusal = Some(error.chars().take(1024).collect());
                }
            }
        }
        state::save_reads(loaded, session, &reads, &original)?;
        result
            .map(|value| {
                json!({"path":file,"status":value["status"],"effect":value["effect"],
            "replay":value["replay"],"authority":"none","mediatedClass":value["mediatedClass"]})
            })
            .map_err(|e| {
                format!("{e}; use the supplied tool's inspect action before choosing refresh")
            })
    })
}

fn file_path(file: &str) -> Result<(), String> {
    if file.len() > 4096 {
        return Err("managed file path exceeds its bound".into());
    }
    file_effect::validate_path(file)
}

fn current_worker(
    loaded: &Loaded,
    binding: &Binding,
    lock: &run::ledger::LedgerPath,
) -> Result<Value, String> {
    if run::ledger::claim_path(lock).exists() {
        return Err("managed edit Work was superseded".into());
    }
    let (_, events, _) = run::ledger::load_at(loaded, lock)?;
    let state = run::reduce_live(loaded, &events)?;
    run::assert_no_drift(loaded, &state)?;
    run::artifact::assert_current(loaded, &state)?;
    run::assignment::pending(&state)
        .into_iter()
        .find(|a| {
            a["role"] == "worker"
                && run::assignment::handle(&binding.work, a).ok().as_deref()
                    == Some(&binding.assignment)
        })
        .ok_or_else(|| {
            "managed edit requires its exact current worker assignment; no retargeting is allowed"
                .into()
        })
}

fn effect_record(
    loaded: &Loaded,
    binding: &Binding,
    session: &str,
    file: &str,
    baseline: &Baseline,
) -> Result<Option<Value>, String> {
    let operation = state::operation(session, file, baseline);
    let name = format!(
        "{}/effects/{}.json",
        crate::project::layout_types::state_namespace(),
        hash::value(&json!([binding.work, operation]))
    );
    let Some(bytes) = state::observe(&loaded.state_root, &name, 16 * 1024)? else {
        return Ok(None);
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "edit effect record is corrupt; use inspect, never refresh")?;
    let request = baseline
        .request
        .as_ref()
        .ok_or("unexpected effect without a submitted edit")?;
    let parameters = hash::value(&json!({"work":binding.work,"assignment":binding.assignment,
        "operation":operation,"path":file,"expected":state::expected(baseline),"contentSha256":request.content_sha256}));
    if value["version"] != 1
        || value["parametersSha256"] != parameters
        || value["resultSha256"] != request.content_sha256
        || !matches!(value["status"].as_str(), Some("admitted" | "completed"))
    {
        return Err(
            "edit effect record conflicts or is corrupt; use inspect, never refresh".into(),
        );
    }
    Ok(Some(value))
}

fn resolved_for_refresh(
    loaded: &Loaded,
    binding: &Binding,
    session: &str,
    file: &str,
    baseline: &Baseline,
) -> Result<(), String> {
    if let Some(request) = baseline.request.as_ref() {
        let effect = effect_record(loaded, binding, session, file, baseline)?;
        let resolved = match request.status {
            Status::Refused => effect.is_none(),
            Status::Completed | Status::Submitted => {
                effect.is_some_and(|e| e["status"] == "completed")
            }
        };
        if !resolved {
            return Err("prior edit is uncertain or its effect record is missing; inspect it, do not refresh".into());
        }
    }
    Ok(())
}

fn inspect(
    loaded: &Loaded,
    session: &str,
    file: &str,
    baseline: Option<&Baseline>,
) -> Result<Value, String> {
    let binding = state::load_binding(loaded, session)?;
    let current = state::observe(&loaded.product_root, file, file_effect::MAX_BYTES)?;
    let observed = current
        .as_deref()
        .map(hash::bytes)
        .unwrap_or_else(|| "absent".into());
    let effect = baseline
        .filter(|b| b.request.is_some())
        .map(|b| effect_record(loaded, &binding, session, file, b))
        .transpose()?
        .flatten();
    Ok(
        json!({"path":file,"status":"inspection","observedSha256":observed,
        "baselineSha256":baseline.map(state::expected),"requestStatus":baseline.and_then(|b| b.request.as_ref()).map(|r| &r.status),
        "effectStatus":effect.as_ref().map(|e| &e["status"]),"authority":"none",
        "next":"refresh only for a deliberate new edit after a refused or completed request; unresolved or missing admitted effects require Lead inspection and cannot be reset here"}),
    )
}
