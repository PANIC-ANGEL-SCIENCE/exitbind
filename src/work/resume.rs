//! `work resume`: select the current work from project-local state.
//!
//! The navigation focus written by `work begin` names current work; other
//! running ledgers stay history and the newest is never guessed.

use super::*;

struct UnverifiedSuccessor {
    work: String,
    ledger: String,
    error: String,
}

struct SupersessionStatus {
    has_claim: bool,
    verified: bool,
    unverified_successor: Option<UnverifiedSuccessor>,
}

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
    let mut claimed_predecessors = std::collections::BTreeSet::new();
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
        let discovered = (|| -> Result<(Option<Candidate>, SupersessionStatus), String> {
            let supersession = supersession_status(loaded, &ledger)?;
            if supersession.verified {
                return Ok((None, supersession));
            }
            let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
            let view = snapshot.inspect_view();
            if view["status"] == "running" {
                let progress = snapshot.next_view(loaded)?["progress"].clone();
                let identity = work_identity(loaded, Some(&ledger))?;
                return Ok((
                    Some((
                        work.clone(),
                        view["workflow"].clone(),
                        view["events"][0]["goal"].clone(),
                        ledger.clone(),
                        progress,
                        identity,
                    )),
                    supersession,
                ));
            }
            if let Some(at) = view["events"]
                .as_array()
                .and_then(|events| events.last())
                .and_then(|event| event["timestamp"].as_str())
            {
                finished.push((at.to_owned(), work.clone(), ledger.clone()));
            }
            Ok((None, supersession))
        })();
        match discovered {
            Ok((candidate, supersession)) => {
                if supersession.has_claim {
                    claimed_predecessors.insert(work.clone());
                }
                if let Some(successor) = supersession.unverified_successor {
                    push_unreadable(
                        loaded,
                        &mut unreadable,
                        &successor.work,
                        &successor.ledger,
                        &successor.error,
                    )?;
                }
                if let Some(candidate) = candidate {
                    candidates.push(candidate);
                }
            }
            Err(error) => push_unreadable(loaded, &mut unreadable, &work, &ledger, &error)?,
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    if !unreadable.is_empty() {
        // An explicit, validated focus is enough to resume that work even when
        // unrelated history cannot be replayed. Keep both the readable history
        // and every unreadable candidate in the response so this does not hide
        // the damaged or ambiguous records.
        if !history {
            if let focus::Focus::Work(selected) = focus::read(loaded)? {
                if !claimed_predecessors.contains(&selected) {
                    if let Some(index) = candidates
                        .iter()
                        .position(|candidate| candidate.0 == selected)
                    {
                        let works = candidate_values(loaded, &candidates, Some(index))?;
                        let (work, _, _, ledger, _, _) = candidates.swap_remove(index);
                        let mut result = resumed(loaded, &work, &ledger)?;
                        result["selection"] =
                            json!({"basis": "current_work_focus", "authority": "none"});
                        result["works"] = json!(works);
                        result["unreadable"] = json!(unreadable);
                        return Ok(result);
                    }
                }
            }
        }
        let status = if candidates.is_empty() {
            "unresolved"
        } else {
            "ambiguous"
        };
        let works = candidate_values(loaded, &candidates, None)?;
        let mut result = json!({
            "status": status,
            "reason": {"code": "unreadable_candidate"},
            "effect": "no-change",
            "compact": true,
            "omitted": ["candidate progress detail"],
            "nextAction": {
                "type": "inspect_candidates",
                "safe": true,
                "summary": "Run the read-only command shown for each unreadable candidate from its workingDirectory."
            },
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
            let works = candidate_values(loaded, &candidates, None)?;
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

fn candidate_values(
    loaded: &Loaded,
    candidates: &[Candidate],
    omit: Option<usize>,
) -> Result<Vec<Value>, String> {
    candidates
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != omit)
        .map(|(_, (work, workflow, goal, _, progress, identity))| {
            Ok(json!({
                "work": work,
                "workflow": workflow,
                "goal": goal,
                "progress": compact_progress(progress),
                "command": candidate_command(loaded, work)?,
                "ledgerProducer": identity["ledgerProducer"],
            }))
        })
        .collect()
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

fn supersession_status(loaded: &Loaded, ledger: &str) -> Result<SupersessionStatus, String> {
    let claim_path = loaded.state_root.join(format!("{ledger}.supersede"));
    let Some(claim) = crate::run::ledger::valid_claim(&claim_path)? else {
        return Ok(SupersessionStatus {
            has_claim: false,
            verified: false,
            unverified_successor: None,
        });
    };
    let successor = claim["newLedgerPath"]
        .as_str()
        .ok_or("supersession successor path is invalid")?;
    let issue = |error: String| SupersessionStatus {
        has_claim: true,
        verified: false,
        unverified_successor: Some(UnverifiedSuccessor {
            work: successor_work(successor),
            ledger: successor.to_owned(),
            error,
        }),
    };
    if claim["oldLedgerPath"] != ledger {
        return Ok(issue(
            "supersession claim does not match the predecessor ledger path".into(),
        ));
    }
    let (_, old_events, old_source) = crate::run::ledger::load(loaded, ledger)?;
    let Some(old_head) = old_events.last() else {
        return Ok(issue("predecessor ledger is empty".into()));
    };
    if claim["oldLedgerSha256"] != hash::bytes(old_source.as_bytes())
        || claim["oldRunId"] != old_events[0]["runId"]
        || claim["oldHeadEventSha256"] != old_head["eventSha256"]
        || claim["oldConfigSha256"] != old_events[0]["configSha256"]
    {
        return Ok(issue(
            "supersession claim does not match predecessor provenance".into(),
        ));
    }
    let (_, successor_events, _) = match crate::run::ledger::load(loaded, successor) {
        Ok(value) => value,
        Err(error) => {
            return Ok(issue(format!(
                "successor ledger could not be verified: {error}"
            )))
        }
    };
    let Some(start) = successor_events.first() else {
        return Ok(issue("successor ledger is empty".into()));
    };
    let link = &start["supersedes"];
    let verified = start["runId"] == claim["newRunId"]
        && start["workflow"] == claim["workflow"]
        && link["ledgerPath"] == claim["oldLedgerPath"]
        && link["ledgerSha256"] == claim["oldLedgerSha256"]
        && link["runId"] == claim["oldRunId"]
        && link["headEventSha256"] == claim["oldHeadEventSha256"]
        && link["configSha256"] == claim["oldConfigSha256"];
    if verified {
        Ok(SupersessionStatus {
            has_claim: true,
            verified: true,
            unverified_successor: None,
        })
    } else {
        Ok(issue(
            "successor provenance does not match the supersession claim".into(),
        ))
    }
}

fn successor_work(ledger: &str) -> String {
    let work = std::path::Path::new(ledger)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("work-"))
        .and_then(|name| name.strip_suffix(".jsonl"))
        .filter(|token| valid_token(token))
        .map(|token| format!("{WORK_PREFIX}{token}"));
    work.unwrap_or_else(|| "unverified_successor".into())
}

fn push_unreadable(
    loaded: &Loaded,
    unreadable: &mut Vec<Value>,
    work: &str,
    ledger: &str,
    error: &str,
) -> Result<(), String> {
    if unreadable
        .iter()
        .any(|candidate| candidate["ledger"] == ledger)
    {
        return Ok(());
    }
    unreadable.push(unreadable_candidate(loaded, work, ledger, error)?);
    Ok(())
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
