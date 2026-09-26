//! `work resume`: select the current work from project-local state.
//!
//! The navigation focus written by `work begin` names current work; other
//! running ledgers stay history and the newest is never guessed.

use super::*;

pub(crate) fn resume(loaded: &Loaded, history: bool) -> Result<Value, String> {
    let directory = loaded.state_root.join(runs_dir());
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return none_result(loaded, Vec::new())
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut candidates = Vec::new();
    let mut unreadable = Vec::new();
    let mut finished: Vec<(String, String, String)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let info = entry.file_type().map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or("work ledger filename is not valid UTF-8")?
            .to_owned();
        let Some(token) = name
            .strip_prefix("work-")
            .and_then(|name| name.strip_suffix(".jsonl"))
        else {
            continue;
        };
        if !info.is_file() || !valid_token(token) {
            continue;
        }
        let ledger = format!("{}/{}", runs_dir(), name);
        let work = format!("{WORK_PREFIX}{token}");
        let discovered = (|| -> Result<Option<Candidate>, String> {
            if superseded_by_valid_claim(loaded, &ledger)? {
                return Ok(None);
            }
            let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
            let view = snapshot.inspect_view();
            if view["status"] == "running" {
                let progress = snapshot.next_view(loaded)?["progress"].clone();
                let identity = work_identity(loaded, Some(&ledger))?;
                return Ok(Some((
                    work.clone(),
                    view["workflow"].clone(),
                    view["events"][0]["goal"].clone(),
                    ledger.clone(),
                    progress,
                    identity,
                )));
            }
            if let Some(at) = view["events"]
                .as_array()
                .and_then(|events| events.last())
                .and_then(|event| event["timestamp"].as_str())
            {
                finished.push((at.to_owned(), work.clone(), ledger.clone()));
            }
            Ok(None)
        })();
        match discovered {
            Ok(Some(candidate)) => candidates.push(candidate),
            Ok(None) => {}
            Err(error) => unreadable.push(unreadable_candidate(loaded, &work, &ledger, &error)?),
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    if !unreadable.is_empty() {
        let status = if candidates.is_empty() {
            "unresolved"
        } else {
            "ambiguous"
        };
        let works = candidates
            .iter()
            .map(|(work, workflow, goal, _, progress, identity)| {
                Ok(json!({
                    "work": work,
                    "workflow": workflow,
                    "goal": goal,
                    "progress": compact_progress(progress),
                    "command": candidate_command(loaded, work)?,
                    "ledgerProducer": identity["ledgerProducer"],
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut result = json!({
            "status": status,
            "reason": {"code": "unreadable_candidate"},
            "effect": "no-change",
            "compact": true,
            "omitted": ["candidate progress detail"],
            "nextAction": safe_action("inspect_candidates"),
            "works": works,
            "unreadable": unreadable,
        });
        add_identity(&mut result, &work_identity(loaded, None)?);
        return Ok(result);
    }
    // The navigation focus written by `work begin` selects current work.
    // Running ledgers it does not name stay history; the newest is never
    // guessed. Without a focus, the legacy count-based behavior remains.
    if !history {
        let focus = focus::read(loaded)?;
        if let focus::Focus::Unusable(detail) = &focus {
            let mut result = none_result(loaded, finished)?;
            result["reason"] = json!({"code": "focus_unusable", "detail": detail});
            result["next"]["reason"] = json!("focus_unusable");
            if !candidates.is_empty() {
                result["history"] = history_reference(loaded, candidates.len())?;
            }
            return Ok(result);
        }
        if let focus::Focus::Work(selected) = focus {
            let others = candidates.len();
            let Some(index) = candidates
                .iter()
                .position(|candidate| candidate.0 == selected)
            else {
                let mut result = none_result(loaded, finished)?;
                result["reason"] = json!({"code": "no_current_work"});
                result["next"]["reason"] = json!("no_current_work");
                result["focus"] = json!({"work": selected, "resumable": false});
                if others > 0 {
                    result["history"] = history_reference(loaded, others)?;
                }
                return Ok(result);
            };
            let (work, _, _, ledger, _, _) = candidates.swap_remove(index);
            let mut result = resumed(loaded, &work, &ledger)?;
            result["selection"] = json!({"basis": "current_work_focus", "authority": "none"});
            if others > 1 {
                result["history"] = history_reference(loaded, others - 1)?;
            }
            return Ok(result);
        }
    }
    match candidates.len() {
        0 => none_result(loaded, finished),
        1 if !history => {
            let (work, _, _, ledger, _, _) = candidates.pop().expect("one candidate exists");
            resumed(loaded, &work, &ledger)
        }
        _ => {
            let works = candidates
                .iter()
                .map(|(work, workflow, goal, _, progress, identity)| {
                    Ok(json!({
                        "work": work.clone(),
                        "workflow": workflow,
                        "goal": goal,
                        "progress": compact_progress(progress),
                        "command": candidate_command(loaded, work)?,
                        "ledgerProducer": identity["ledgerProducer"],
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let (status, reason) = if history {
                ("history", "history_requested")
            } else {
                ("ambiguous", "ambiguous_candidates")
            };
            let mut result = json!({
            "status": status,
            "reason": {"code": reason},
            "effect": "no-change",
            "compact": true,
            "omitted": ["candidate progress detail"],
            "reference": {"works": candidates.iter().map(|(work, _, _, _, _, _)| work).collect::<Vec<_>>()},
            "nextAction": safe_action("choose_explicit_handle"),
            "works": works
            });
            add_identity(&mut result, &work_identity(loaded, None)?);
            Ok(result)
        }
    }
}

fn history_reference(loaded: &Loaded, running: usize) -> Result<Value, String> {
    let config = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    Ok(json!({
        "running": running,
        "command": [crate::compatibility::profile().caller, "work", "resume", "--history", "--config", config],
    }))
}

fn resumed(loaded: &Loaded, work: &str, ledger: &str) -> Result<Value, String> {
    let (next, residual, presentation) = next_and_residual(loaded, work, ledger, true)
        .map_err(|error| discovery_error(error, work))?;
    let mut result = json!({
        "status": "resumed",
        "work": work,
        "next": next,
        "residual": residual,
        "presentation": presentation
    });
    add_identity(&mut result, &work_identity(loaded, Some(ledger))?);
    attach_continuation(loaded, work, &mut result)?;
    Ok(result)
}

fn superseded_by_valid_claim(loaded: &Loaded, ledger: &str) -> Result<bool, String> {
    let claim_path = loaded.state_root.join(format!("{ledger}.supersede"));
    let Some(claim) = crate::run::ledger::valid_claim(&claim_path)? else {
        return Ok(false);
    };
    if claim["oldLedgerPath"] != ledger {
        return Ok(false);
    }
    let (_, old_events, old_source) = crate::run::ledger::load(loaded, ledger)?;
    let Some(old_head) = old_events.last() else {
        return Ok(false);
    };
    if claim["oldLedgerSha256"] != hash::bytes(old_source.as_bytes())
        || claim["oldRunId"] != old_events[0]["runId"]
        || claim["oldHeadEventSha256"] != old_head["eventSha256"]
        || claim["oldConfigSha256"] != old_events[0]["configSha256"]
    {
        return Ok(false);
    }
    let successor = claim["newLedgerPath"].as_str().unwrap_or_default();
    let (_, successor_events, _) = match crate::run::ledger::load(loaded, successor) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    let Some(start) = successor_events.first() else {
        return Ok(false);
    };
    let link = &start["supersedes"];
    Ok(start["runId"] == claim["newRunId"]
        && start["workflow"] == claim["workflow"]
        && link["ledgerPath"] == claim["oldLedgerPath"]
        && link["ledgerSha256"] == claim["oldLedgerSha256"]
        && link["runId"] == claim["oldRunId"]
        && link["headEventSha256"] == claim["oldHeadEventSha256"]
        && link["configSha256"] == claim["oldConfigSha256"])
}

/// No work is active. The most recently finished run is still reported, with
/// the presentation the product would print for it, so the answer to "where
/// does this stand" comes from recorded state rather than from a reader's
/// summary of the ledger. Its terminal block appears only while that decision
/// still describes the files present now.
fn none_result(
    loaded: &Loaded,
    mut finished: Vec<(String, String, String)>,
) -> Result<Value, String> {
    let mut result =
        json!({"status": "none", "next": {"action": "none", "reason": "no_active_work"}});
    finished.sort_by(|left, right| left.0.cmp(&right.0));
    if let Some((_, work, ledger)) = finished.pop() {
        let (next, _residual, presentation) = next_and_residual(loaded, &work, &ledger, false)
            .map_err(|error| discovery_error(error, &work))?;
        // `presentation` sits where every other work response carries it, so
        // one documented path holds for every answer a host has to render.
        result["recent"] = json!({"work": work, "exitState": next["progress"]["state"]});
        result["presentation"] = presentation;
        add_identity(&mut result, &work_identity(loaded, Some(&ledger))?);
    } else {
        add_identity(&mut result, &work_identity(loaded, None)?);
    }
    Ok(result)
}
