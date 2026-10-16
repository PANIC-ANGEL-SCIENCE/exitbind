//! Read-only observations for the pure decision; no Work lock is acquired here.
use super::*;

pub(super) struct Current {
    now: i64,
    stopped: bool,
    source: bool,
    configuration: bool,
    memory: bool,
}

impl Current {
    pub(super) fn read(loaded: &Loaded, record: &journal::Record, state: Option<&Value>) -> Self {
        let configuration = crate::work::details::ensure_current_config(loaded).is_ok()
            && record.configuration_sha256 == hash::text(&loaded.source);
        let source = sources::read(loaded, &record.specification_path)
            .is_ok_and(|(_, digest)| digest == record.specification_sha256);
        let memory = state.map_or(true, |state| {
            crate::run::assert_no_drift(loaded, state).is_ok()
                && crate::memory::contextual::assert_plan(loaded, state).is_ok()
        });
        Self {
            now: Utc::now().timestamp(),
            stopped: record.stopped,
            source,
            configuration,
            memory,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn value<'a>(
        &self,
        recipient_role: Role,
        recipient_write: &'a [String],
        recipient_commands: &'a [String],
        works: &'a [control::WorkObservation<'a>],
        active_native_operation: Option<&'a str>,
        successor_of: Option<&'a str>,
        observation: Option<control::Observation<'a>>,
    ) -> control::Facts<'a> {
        control::Facts {
            now: self.now,
            stopped: self.stopped,
            source_current: self.source,
            configuration_current: self.configuration,
            mandate_current: self.source,
            memory_current: self.memory,
            recipient_role,
            recipient_write,
            recipient_commands,
            works,
            active_native_operation,
            successor_of,
            observation,
        }
    }
}

pub(super) fn recipient(
    loaded: &Loaded,
    record: &journal::Record,
    claim: &journal::Claim,
    operation: &Operation<'_>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let workflow = loaded
        .workflows
        .get(&claim.plan.workflow)
        .ok_or("selected workflow is missing")?;
    let names = match operation.role {
        "lead" => vec![loaded.lead().ok_or("selected Lead is missing")?.to_string()],
        "worker" => workflow.workers.clone(),
        "reviewer" => workflow.reviewers.clone(),
        "adviser" => workflow.advisers.clone(),
        _ => return Err("invalid operation role".into()),
    };
    if let Some(name) = operation.agent {
        if !names.iter().any(|selected| selected == name) {
            return Err("operation agent is not a selected workflow recipient".into());
        }
        let agent = loaded
            .agent(name)
            .ok_or("selected recipient is unavailable")?;
        return Ok((agent.write.clone(), agent.commands.clone()));
    }
    // Internal checker commands are frozen owner policy, not a new agent tool
    // permission. Other anonymous effects cannot inherit a role's union.
    if operation.kind == Kind::Check {
        let mut commands = vec![
            claim.plan.check_command.clone(),
            claim.plan.preservation_check_command.clone(),
        ];
        if let Some(pre_return) = &claim.plan.pre_return {
            commands.push(pre_return.command.clone());
        }
        commands.sort();
        commands.dedup();
        if commands
            .iter()
            .any(|command| !record.specification.mandate.commands.contains(command))
        {
            return Err("frozen checker exceeds the current mandate".into());
        }
        return Ok((Vec::new(), commands));
    }
    if matches!(operation.kind, Kind::Admission | Kind::Successor)
        && operation.paths.is_empty()
        && operation.commands.is_empty()
        && operation.role == "lead"
    {
        return Ok((Vec::new(), Vec::new()));
    }
    Err("controlled continuation effect needs its exact configured recipient".into())
}

pub(super) fn works(
    loaded: &Loaded,
    account: &Account,
) -> Result<Vec<(String, control::WorkState)>, String> {
    account
        .operations
        .iter()
        .filter(|operation| matches!(operation.intent.kind, Kind::Admission | Kind::Successor))
        .map(|operation| {
            let work = &operation.intent.work;
            let observed: Result<control::WorkState, String> = (|| {
                let relative = crate::work::resolve(loaded, work)?;
                let (selected, events, raw) = ledger::load(loaded, &relative)?;
                let state = crate::run::reduce_live(loaded, &events)?;
                if state["projectContinuation"]
                    != serde_json::to_value(&operation.association).map_err(|e| e.to_string())?
                {
                    return Err("observed Work association differs".into());
                }
                let retired = match ledger::valid_claim(&ledger::claim_path(&selected))? {
                    None => false,
                    Some(link) => {
                        if link["oldLedgerPath"] != relative
                            || link["oldLedgerSha256"] != hash::text(&raw)
                            || link["oldRunId"] != state["runId"]
                            || link["oldHeadEventSha256"]
                                != events.last().ok_or("empty Work")?["eventSha256"]
                        {
                            return Err("retirement is not bound to this exact Work".into());
                        }
                        true
                    }
                };
                let native = crate::work::action::continuation_native_facts(loaded, work, &state)?;
                let terminal = state["status"] != "running";
                if (terminal || retired) && native.all_ended {
                    Ok(control::WorkState::Ended)
                } else if retired || terminal {
                    Ok(control::WorkState::Unknown)
                } else {
                    Ok(control::WorkState::Active)
                }
            })();
            Ok((
                work.clone(),
                observed.unwrap_or(control::WorkState::Unknown),
            ))
        })
        .collect()
}

pub(super) fn work_values(
    states: &[(String, control::WorkState)],
) -> Vec<control::WorkObservation<'_>> {
    states
        .iter()
        .map(|(work, state)| control::WorkObservation {
            work,
            state: *state,
        })
        .collect()
}
