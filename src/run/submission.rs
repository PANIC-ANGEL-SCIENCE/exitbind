use super::*;

type ArtifactProducer<'a> = Box<dyn FnOnce() -> Result<String, String> + 'a>;

#[allow(clippy::too_many_arguments)]
pub fn submit(
    loaded: &Loaded,
    agent: &str,
    ledger: &str,
    outcome: &str,
    artifact: &str,
    artifact_root: Option<&str>,
    reason: Option<&str>,
    disposition: Option<&str>,
) -> Result<Value, String> {
    if agent.trim().is_empty() {
        return Err("agent is required".into());
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let artifact = artifact.to_owned();
    let pre_return_attempt = if outcome == "completed"
        && super::pre_return::enabled(loaded, ledger)?
    {
        super::pre_return::prepare_direct(loaded, ledger, agent, outcome, artifact_root, &artifact)?
    } else {
        None
    };
    let work = canonical_work_for_ledger(&path).ok();
    crate::host::session_recovery::with_saved_decisions(work.as_deref(), |decisions| {
        let result = submit_locked(
            loaded,
            &path,
            agent,
            outcome,
            reason,
            decisions,
            None,
            None,
            disposition,
            SubmitSubject {
                root: artifact_root,
                artifact: || Ok(artifact),
            },
            |_, _| Ok(()),
            None,
            pre_return_attempt.as_deref(),
        )?;
        let recorded = records_decisions(&result, decisions);
        Ok((result, recorded))
    })
}

/// Whether the recorded event carries `decisions` in its reason: the act
/// spent them.
fn records_decisions(result: &Value, decisions: &[String]) -> bool {
    !decisions.is_empty()
        && result["event"]["outcome"] != "disposition"
        && result["event"].get("reason").is_some()
}

/// Submit a work result after revalidating the façade's exact assignment
/// under the ledger lock. The artifact is produced only after that check.
/// `binding` checks the caller's view; it runs under every lock that appends
/// for this call, except that this call's own repair event does not make the
/// view stale.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_for_assignment<F, B, P>(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    outcome: &str,
    reason: Option<&str>,
    disposition: Option<&str>,
    artifact: F,
    binding: B,
    preflight: P,
    recorded_protection: &mut Option<RecordedProtection>,
) -> Result<Value, String>
where
    F: FnOnce() -> Result<String, String>,
    B: Fn(Option<(&[Value], &str)>) -> Result<(), String>,
    P: FnOnce(&[Value], &str) -> Result<(), String>,
{
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let agent = expected.agent.clone();
    let assignment_sha256 = crate::evidence::hash::text(assignment_handle);
    let mut own_repair = None;
    if expected.role == "worker" && outcome == "completed" {
        // A current Lead repair can precede the mutation permit that exhausts
        // the no-information streak. Record that decision's one forward
        // governor transition before the frozen pre-return check is admitted
        // and before the worker's retained result; both revalidate under the
        // ledger lock, and an interrupted call leaves a replayable repair
        // event, never a fabricated submission.
        own_repair = super::repair_recovery::before_worker_completion(
            loaded,
            &path,
            assignment_handle,
            &expected,
            &|| binding(None),
        )?;
    }
    let selected_pre_return = outcome == "completed"
        && expected.role == "worker"
        && super::pre_return::enabled(loaded, ledger)?;
    let (subject_artifact, pre_return_attempt, own_admission): (
        ArtifactProducer<'_>,
        Option<String>,
        Option<String>,
    ) = if selected_pre_return {
        let (artifact_path, attempt_id, admission) = super::pre_return::prepare_for_assignment(
            loaded,
            ledger,
            &agent,
            &expected,
            &assignment_sha256,
            artifact,
            super::pre_return::CallerBinding {
                check: &|| binding(None),
                own_repair: own_repair.as_deref(),
            },
        )?;
        (
            Box::new(move || Ok(artifact_path)),
            Some(attempt_id),
            admission,
        )
    } else {
        (Box::new(artifact), None, None)
    };
    let work = canonical_work_for_ledger(&path).ok();
    crate::host::session_recovery::with_saved_decisions(work.as_deref(), |decisions| {
        let result = submit_locked(
            loaded,
            &path,
            &agent,
            outcome,
            reason,
            decisions,
            Some(&expected),
            Some(&assignment_sha256),
            disposition,
            SubmitSubject {
                root: Some("state"),
                artifact: subject_artifact,
            },
            |events, source| {
                if let Some(prefix) = super::repair_recovery::own_event_prefix(
                    &events[..events.len().saturating_sub(1)],
                    own_repair.as_deref(),
                    own_admission.as_deref(),
                ) {
                    let prefix_source = source
                        .split_inclusive('\n')
                        .take(prefix)
                        .collect::<String>();
                    binding(Some((&events[..prefix], &prefix_source)))?;
                } else {
                    binding(None)?;
                }
                preflight(events, source)
            },
            Some(recorded_protection),
            pre_return_attempt.as_deref(),
        )?;
        let recorded = records_decisions(&result, decisions);
        Ok((result, recorded))
    })
}

/// The submitted subject: an artifact root and a producer deferred until the
/// assignment has been revalidated under the ledger lock.
struct SubmitSubject<'r, F> {
    root: Option<&'r str>,
    artifact: F,
}

pub(super) fn matching_mutation_grants(
    events: &[Value],
    state: &Value,
    assignment: &Value,
    assignment_sha256: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    let Some(subject) = state["subject"]["sha256"].as_str() else {
        return Err("run subject is not available for grant matching".into());
    };
    let Some(_result_input) = state["inputsSha256"].as_str() else {
        return Err("tested input identity is not available for grant matching".into());
    };
    let lineage = state["governor"]["lineageSha256"].as_str();
    let mut acknowledged = std::collections::BTreeSet::new();
    for event in events {
        if let Some(grants) = event["governorEvent"]["grantEventSha256s"].as_array() {
            acknowledged.extend(grants.iter().filter_map(Value::as_str));
        }
    }
    let mut result = Vec::new();
    for event in events {
        let nested = &event["governorEvent"];
        if event["action"] != "govern"
            || event["runId"] != state["runId"]
            || event["subjectSha256"] != subject
            || event["stage"] != assignment["stage"]
            || event["attempt"] != assignment["attempt"]
            || event["agent"] != assignment["agent"]
            || event["role"] != assignment["role"]
            || assignment_sha256.map_or(false, |hash| event["assignmentSha256"] != hash)
            || nested["action"] != "mutation"
            || nested["runId"] != state["runId"]
            || nested["subjectSha256"] != subject
            || nested["attempt"] != state["attempt"]
            || nested["inputSha256"] != event["inputsSha256"]
            || nested["carryLineage"] != true
            || lineage.map_or(true, |value| nested["lineageSha256"] != value)
        {
            continue;
        }
        let Some(hash) = event["eventSha256"].as_str() else {
            return Err("mutation grant has no outer event identity".into());
        };
        if !acknowledged.contains(hash) {
            let source = nested["inputSha256"]
                .as_str()
                .ok_or("mutation grant has no source input identity")?;
            result.push((hash.to_owned(), source.to_owned()));
        }
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn submit_locked<F, P>(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    agent: &str,
    outcome: &str,
    reason: Option<&str>,
    decisions: &[String],
    expected: Option<&AssignmentIdentity>,
    assignment_sha256: Option<&str>,
    disposition: Option<&str>,
    subject: SubmitSubject<'_, F>,
    preflight: P,
    recorded_protection: Option<&mut Option<RecordedProtection>>,
    pre_return_attempt_id: Option<&str>,
) -> Result<Value, String>
where
    F: FnOnce() -> Result<String, String>,
    P: FnOnce(&[Value], &str) -> Result<(), String>,
{
    let SubmitSubject { root, artifact } = subject;
    let artifact_root = root;
    let targets = [path.path.as_path(), path.lock.as_path()];
    with_lock(path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, path)?;
        let state = reduce_live(loaded, &events)?;
        if state["governor"]["enabled"] == true && state["governor"]["state"] == "blocked" {
            return Err("governor is durably blocked; work result refused".into());
        }
        if state["status"] != "running" {
            return Err("run has already reached a terminal state; no mutation was made".into());
        }
        let warning = action_drift_warning(loaded, &state)?;
        crate::memory::contextual::assert_plan(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        if agent == "lead"
            && outcome == "accepted"
            && state["reviewPolicy"]["decision"] == "required"
            && state["plan"]["stages"].as_array().is_some_and(|stages| {
                stages.iter().any(|stage| {
                    stage["stage"] == state["currentStage"]
                        && stage["agents"].as_array().is_some_and(|agents| {
                            agents
                                .iter()
                                .any(|candidate| candidate["role"] == "reviewer")
                        })
                })
            })
        {
            return Err("canonical acceptance requires reviewer approval".into());
        }
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == agent)
            .ok_or_else(|| format!("agent '{agent}' is not currently pending"))?;
        if let Some(expected) = expected {
            if !expected.matches(&assignment) {
                return Err(
                    "assignment changed while work result was being read; no mutation was made"
                        .into(),
                );
            }
        }
        // The Lead's reason and this Work's saved decisions, refused above
        // their bound before any write.
        let version = state["version"]
            .as_u64()
            .ok_or("run state is missing version")?;
        let lead_reason = if version >= super::lead_reason::FIRST_VERSION {
            super::lead_reason::for_submission(
                assignment["role"].as_str().unwrap_or_default(),
                outcome,
                reason,
                disposition.is_some(),
                decisions,
            )?
        } else {
            None
        };
        let known_work = super::governor::canonical_work_for_ledger(path).ok();
        let known_assignment = known_work
            .as_deref()
            .and_then(|work| crate::run::assignment::handle(work, &assignment).ok());
        let parsed_disposition = crate::run::disposition::submission(
            &state,
            &assignment["role"],
            outcome,
            disposition.or(reason),
            disposition.is_some(),
            &loaded.path,
            crate::run::disposition::Known {
                work: known_work.as_deref(),
                assignment: known_assignment.as_deref(),
            },
        )?;
        let mut grant_hashes = Vec::new();
        if state["governor"]["enabled"] == true
            && assignment["role"] == "worker"
            && outcome == "completed"
        {
            if state["governor"]["state"] != "ready" {
                return Err(GOVERNOR_COMPLETION_REFUSAL.into());
            }
            grant_hashes =
                matching_mutation_grants(&events, &state, &assignment, assignment_sha256)?;
            if assignment_sha256.is_none() && !grant_hashes.is_empty() {
                return Err(
                    "exact assignment identity is required to acknowledge mutation grants".into(),
                );
            }
        }
        let artifact = artifact()?;
        let artifact_value = crate::run::artifact::evidence(loaded, artifact_root, &artifact)?;
        let saved_native = if matches!(assignment["role"].as_str(), Some("worker" | "reviewer")) {
            crate::project::continuation::runtime::allow_saved_result(
                loaded,
                &state,
                agent,
                outcome,
                &artifact_value,
            )?
        } else {
            false
        };
        if assignment["role"] == "worker" && outcome == "completed" {
            super::pre_return::assert_pass_current(
                loaded,
                path,
                &state,
                &assignment,
                assignment_sha256,
                &artifact_value,
                pre_return_attempt_id,
            )?;
        }
        let last = events.last().ok_or("run ledger has no head event")?;
        // Preserve the ledger's historical event version (read above).
        // Optional fields must not silently upgrade a v3 checked run on its
        // next submission.
        let result_inputs = if version >= 6 {
            Some(live_inputs(&state)?)
        } else {
            None
        };
        if assignment["role"] == "lead" && outcome == "accepted" {
            let assessment = crate::run_exit::reduce(&state)?.assessment;
            if assessment.is_blocked() {
                if assessment.has_refusal_evidence() {
                    let timestamp = nondecreasing(&last["timestamp"])?;
                    let protection = crate::run_value::protection_event(
                        &state,
                        agent,
                        &assignment["stage"],
                        &assignment["attempt"],
                        last,
                        &timestamp,
                    )?;
                    let protection = run_state::make_event(protection);
                    let mut protected = events.clone();
                    protected.push(protection.clone());
                    run_state::reduce(&protected)?;
                    let line =
                        serde_json::to_string(&protection).map_err(|error| error.to_string())?;
                    append(path, &protection, false, &source)?;
                    if let Some(recorded) = recorded_protection {
                        *recorded = Some(RecordedProtection {
                            event: protection,
                            ledger_sha256: hash::bytes(format!("{source}{line}\n").as_bytes()),
                        });
                    }
                    return Err(format!(
                        "acceptance refused: configured check evidence is {}",
                        assessment.reason().unwrap_or("blocked")
                    ));
                }
                return Err(
                    "acceptance refused: checked worker completion or result is not observed"
                        .into(),
                );
            }
        }
        let mut event_value = json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "submit",
            "runId": state["runId"],
            "stage": assignment["stage"],
            "attempt": assignment["attempt"],
            "agent": agent,
            "role": assignment["role"],
            "outcome": outcome,
            "artifact": artifact_value,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
        });
        if version >= 6 {
            if let Some(assignment_sha256) = assignment_sha256 {
                event_value["assignmentSha256"] = json!(assignment_sha256);
            }
        }
        if version >= 6
            && matches!(
                (assignment["role"].as_str(), outcome),
                (Some("reviewer"), "approved")
                    | (Some("lead"), "accepted")
                    | (Some("worker"), "completed")
            )
        {
            event_value["inputsSha256"] = result_inputs.clone().unwrap();
        }
        if version >= 5 {
            event_value["subjectSha256"] =
                if assignment["role"] == "worker" && outcome == "completed" {
                    crate::run::state::subject_for_submission(&state, &assignment, &artifact_value)
                        ["sha256"]
                        .clone()
                } else {
                    state["subject"]["sha256"].clone()
                };
        }
        if version >= 8 && state.get("basisProtocol").is_some() {
            event_value["basisSha256"] = state["basis"]["sha256"].clone();
            event_value["reviewDecisionSha256"] = state["reviewPolicy"]["sha256"].clone();
        }
        if let Some(disposition) = parsed_disposition {
            // Record the live observation this decision rests on, so a later
            // ledger read validates the fact instead of the host's state.
            if disposition.decision == Some(crate::kernel::disposition::Decision::Environmental)
                && super::environmental_check::closed_owned_group(&state)
            {
                event_value[super::owned_group::ENDED_FIELD] = json!(super::owned_group::ENDED);
            }
            event_value["disposition"] = disposition.value();
        }
        if let Some(fallback) = fallback_provenance(&assignment, outcome, reason, version)? {
            event_value["fallback"] = fallback;
        }
        if let Some(lead_reason) = lead_reason {
            event_value["reason"] = json!(lead_reason);
        }
        if state["governor"]["enabled"] == true
            && assignment["role"] == "worker"
            && outcome == "completed"
        {
            let previous_governor = state["governor"]["headSha256"]
                .as_str()
                .map(|head| json!({"eventSha256": head}));
            let lineage = state["governor"]["lineageSha256"]
                .as_str()
                .map_or_else(|| state["subject"]["sha256"].clone(), |value| json!(value));
            let input_sha = result_inputs
                .clone()
                .unwrap_or_else(|| json!(crate::evidence::hash::text("unbound")));
            // Pre-mutation permits and completed submissions share one
            // canonical governor lineage.  Counting submissions alone would
            // replay a post-permit completion at checkpoint 1 and make the
            // ledger unreducible (or silently reset the bound).
            let explicit = !grant_hashes.is_empty();
            let (grant_event_sha256s, source_inputs_sha256s): (Vec<_>, Vec<_>) =
                grant_hashes.iter().cloned().unzip();
            let mutation = json!({
                "action": "mutation",
                "runId": state["runId"],
                "subjectSha256": state["subject"]["sha256"],
                "attempt": state["attempt"],
                "checkpoint": if grant_hashes.is_empty() {
                    json!(state["governor"]["spent"].as_u64().unwrap_or_default().saturating_add(1))
                } else {
                    state["governor"]["spent"].clone()
                },
                "inputSha256": input_sha,
                "lineageSha256": lineage,
                "carryLineage": true,
                "unit": "worker",
                "operation": "completed_submission",
                "newEvidenceSha256": artifact_value["sha256"],
                "authorizationMode": if explicit { "explicit" } else { "implicit" },
                "grantEventSha256s": grant_event_sha256s,
                "sourceInputsSha256s": source_inputs_sha256s,
            });
            event_value["governorEvent"] =
                crate::context::event(previous_governor.as_ref(), mutation);
        }
        if let Some(marker) = state.get("quarantineVerification") {
            if assignment["role"] == "worker" && outcome == "completed" {
                event_value["quarantineVerificationAck"] =
                    crate::run::quarantine::acknowledgement(marker, &event_value)?;
            }
        }
        let event = run_state::make_event(event_value);
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        let line = serde_json::to_string(&event).map_err(|error| error.to_string())?;
        let projected_source = format!("{source}{line}\n");
        preflight(&all, &projected_source)?;
        let eligibility = crate::project::continuation::runtime::guard(loaded, &state);
        let recovering_saved = saved_native && eligibility.is_err();
        let ticket = if !recovering_saved
            && matches!(assignment["role"].as_str(), Some("lead" | "reviewer"))
            && !matches!(outcome, "blocked" | "unavailable")
        {
            eligibility?;
            super::continuation_effect::begin_event(
                loaded,
                &state,
                crate::project::continuation::control::Kind::Effect,
                &event,
            )?
        } else {
            if !recovering_saved && !matches!(outcome, "blocked" | "unavailable") {
                eligibility?;
            }
            None
        };
        append(path, &event, false, &source)?;
        super::continuation_effect::complete(loaded, &ticket, true, true)?;
        Ok(json!({
            "valid": true,
            "event": event,
            "status": next_state["status"],
            "currentStage": if next_state["status"] == "running" { next_state["currentStage"].clone() } else { Value::Null },
            "attempt": if next_state["status"] == "running" { next_state["attempt"].clone() } else { Value::Null },
            "assignments": crate::run::assignment::pending(&next_state),
            "warnings": warning.map_or_else(|| json!([]), |warning| json!([warning]))
        }))
    })
}
