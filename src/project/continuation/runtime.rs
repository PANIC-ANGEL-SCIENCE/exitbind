//! Controlled-effect admission. The account lock never waits for a Work lock.
mod facts;
mod goal_linkage;
mod successor;
pub(crate) use goal_linkage::link_successor_goal;
pub(crate) use successor::prepare_successor;

use super::{admission, association, control, journal, sources};
use crate::{config::Loaded, evidence::hash, run::ledger};
use chrono::Utc;
use control::{Account, Association, Decision, Intent, Kind, Outcome, Policy, Role};
use serde_json::{json, Value};

pub(crate) struct Operation<'a> {
    pub kind: Kind,
    pub key: &'a str,
    pub role: &'a str,
    pub agent: Option<&'a str>,
    pub paths: &'a [String],
    pub commands: &'a [String],
    pub input: &'a Value,
    pub predecessor: Option<&'a str>,
}

pub(crate) struct Ticket {
    relative: String,
    association: control::VerifiedAssociation,
    key: String,
    intent: Intent,
}

impl Ticket {
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
}

fn refusal(reason: control::Refusal) -> String {
    format!("project continuation held: {reason:?}; inspect the exact Work and mandate")
}

fn role(name: &str) -> Result<Role, String> {
    match name {
        "lead" => Ok(Role::Lead),
        "worker" => Ok(Role::Worker),
        "reviewer" => Ok(Role::Reviewer),
        "adviser" => Ok(Role::Adviser),
        _ => Err("controlled continuation role is invalid".into()),
    }
}

fn policy(record: &journal::Record) -> Policy<'_> {
    Policy {
        project: &record.project_identity,
        specification_sha256: &record.specification_sha256,
        mandate: &record.specification.mandate,
    }
}

pub(super) fn check_selected(loaded: &Loaded, record: &journal::Record) -> Result<(), String> {
    let marker = Association {
        version: control::ASSOCIATION_VERSION,
        project: record.project_identity.clone(),
        mandate_id: record.specification.mandate.id.clone(),
        mandate_revision: record.specification.mandate.revision,
        specification_sha256: record.specification_sha256.clone(),
        task_id: "eligibility".into(),
        claim_root: format!("smw_{}", "0".repeat(64)),
    };
    let current = facts::Current::read(loaded, record, None);
    let observed = current.value(Role::Lead, &[], &[], &[], None, None, None);
    control::eligibility(&policy(record), &observed, &record.account, &marker).map_err(refusal)
}

pub(crate) fn guard(loaded: &Loaded, state: &Value) -> Result<(), String> {
    let Some(bound) = association::for_state(loaded, state)? else {
        return Ok(());
    };
    if bound.retired {
        return Err("associated Work is retired; no new controlled effect".into());
    }
    validate_contract(loaded, state, &bound.record, &bound.claim)?;
    let current = facts::Current::read(loaded, &bound.record, Some(state));
    let observed = current.value(Role::Lead, &[], &[], &[], None, None, None);
    control::eligibility(
        &policy(&bound.record),
        &observed,
        &bound.record.account,
        bound.verified.association(),
    )
    .map_err(refusal)
}

pub(crate) fn guard_work(loaded: &Loaded, work: &str) -> Result<(), String> {
    let relative = crate::work::resolve(loaded, work)?;
    let (_, events, _) = ledger::load(loaded, &relative)?;
    guard(loaded, &crate::run::reduce_live(loaded, &events)?)
}

pub(crate) fn guard_review_policy(
    loaded: &Loaded,
    state: &Value,
    decision: &str,
) -> Result<(), String> {
    if state.get("projectContinuation").is_none() {
        return Ok(());
    }
    if decision != "required" {
        return Err(
            "project continuation requires independent review; a policy transition cannot waive it"
                .into(),
        );
    }
    guard(loaded, state)
}

pub(crate) fn validate_start_association(
    loaded: &Loaded,
    verified: &control::VerifiedAssociation,
    ledger: &str,
) -> Result<Value, String> {
    let marker = verified.association();
    let (_, record, claim) = association::claim(loaded, marker)?;
    let work = association::ledger_work(ledger)?;
    if verified.work() != work || claim.work != work {
        return Err("typed start association is not its reserved root Work".into());
    }
    let admitted = record
        .account
        .operations
        .iter()
        .find(|operation| {
            operation.intent.kind == Kind::Admission
                && operation.intent.work == work
                && operation.association == *marker
        })
        .ok_or("associated start has no actual aggregate admission reservation")?;
    if admitted.outcome != Outcome::Reserved {
        return Err(
            "associated start reservation is already observed; inspect without retry".into(),
        );
    }
    check_selected(loaded, &record)?;
    serde_json::to_value(marker).map_err(|e| e.to_string())
}

pub(crate) fn begin(
    loaded: &Loaded,
    state: &Value,
    operation: Operation<'_>,
) -> Result<Option<Ticket>, String> {
    let Some(bound) = association::for_state(loaded, state)? else {
        return Ok(None);
    };
    if bound.retired {
        return Err("associated Work is retired; no fresh action is admitted".into());
    }
    validate_contract(loaded, state, &bound.record, &bound.claim)?;
    let selected = journal::prepare(loaded, &bound.relative)?;
    ledger::with_lock(&selected, || {
        let (record, raw) = journal::read(loaded, &bound.relative)?;
        let record = record.ok_or("selected continuation disappeared")?;
        let (write, commands) = facts::recipient(loaded, &record, &bound.claim, &operation)?;
        let states = facts::works(loaded, &record.account)?;
        let work_observations = facts::work_values(&states);
        let native = crate::work::action::continuation_native_facts(loaded, &bound.work, state)?;
        if operation.kind == Kind::Native && !native.all_ended {
            return Err(
                "an owned native execution is live or unresolved; inspect without a new launch"
                    .into(),
            );
        }
        let active = native.active.iter().find(|native| {
            operation.agent == Some(native.agent.as_str())
                && native.role == operation.role
                && current_assignment(
                    state,
                    &bound.work,
                    &native.role,
                    &native.agent,
                    &native.assignment,
                )
                && operation
                    .input
                    .get("assignment")
                    .map_or(true, |assignment| assignment == &native.assignment)
                && record.account.operations.iter().any(|old| {
                    old.key == native.key
                        && old.intent.kind == Kind::Native
                        && old.intent.work == native.work
                        && old.intent.role == role(&native.role).unwrap_or(Role::Lead)
                        && old.intent.input_sha256 == native.request_sha256
                        && old.outcome == Outcome::Reserved
                })
        });
        let current = facts::Current::read(loaded, &record, Some(state));
        let observed = current.value(
            role(operation.role)?,
            &write,
            &commands,
            &work_observations,
            active.map(|native| native.key.as_str()),
            None,
            None,
        );
        let intent = Intent {
            kind: operation.kind,
            work: bound.work.clone(),
            role: role(operation.role)?,
            paths: operation.paths.to_vec(),
            commands: operation.commands.to_vec(),
            input_sha256: hash::value(operation.input),
            predecessor: operation.predecessor.map(str::to_owned),
        };
        let request = control::Request::Attempt(control::Attempt {
            expected_revision: record.account.revision,
            key: operation.key,
            intent: &intent,
            association: &bound.verified,
        });
        match control::evaluate(&policy(&record), &observed, &record.account, &request).map_err(refusal)? {
            Decision::Execute { next } => {
                let mut next_record = record.clone(); next_record.account = next;
                journal::append(&selected, next_record, Some(&record), &raw)?;
                Ok(Some(Ticket { relative: bound.relative.clone(), association: bound.verified.clone(),
                    key: operation.key.into(), intent }))
            }
            Decision::RecoverExisting { .. } => Err("controlled operation is already reserved; recover its owning saved result without a new effect".into()),
            _ => Err("unexpected continuation admission decision".into()),
        }
    })
}

/// Caller holds the existing mandate lock. The returned claim and account are
/// persisted together, then that lock is released before starting one Work.
pub(super) fn reserve_admission(
    loaded: &Loaded,
    record: &journal::Record,
    plan: &super::model::Plan,
    work: &str,
    start_intent: &str,
) -> Result<(control::VerifiedAssociation, Account, Ticket), String> {
    let verified = control::prepare_association(&policy(record), &plan.task_id, work, work)
        .map_err(refusal)?;
    let intent = Intent {
        kind: Kind::Admission,
        work: work.into(),
        role: Role::Lead,
        paths: Vec::new(),
        commands: Vec::new(),
        predecessor: None,
        input_sha256: hash::value(
            &json!({"plan":admission::plan_digest(plan)?,"startIntent":start_intent}),
        ),
    };
    let key = format!("admission-{}", hash::text(work));
    let states = facts::works(loaded, &record.account)?;
    let work_values = facts::work_values(&states);
    let current = facts::Current::read(loaded, record, None);
    let observed = current.value(Role::Lead, &[], &[], &work_values, None, None, None);
    let request = control::Request::Attempt(control::Attempt {
        expected_revision: record.account.revision,
        key: &key,
        intent: &intent,
        association: &verified,
    });
    match control::evaluate(&policy(record), &observed, &record.account, &request)
        .map_err(refusal)?
    {
        Decision::Execute { next } => Ok((
            verified.clone(),
            next,
            Ticket {
                relative: journal::relative(loaded, &record.specification.mandate.id)?,
                association: verified,
                key,
                intent,
            },
        )),
        _ => {
            Err("Work admission already accounted; inspect its exact claim without starting".into())
        }
    }
}

pub(crate) fn complete(
    loaded: &Loaded,
    ticket: &Option<Ticket>,
    success: bool,
    known_ended: bool,
) -> Result<(), String> {
    let Some(ticket) = ticket else {
        return Ok(());
    };
    let selected = journal::prepare(loaded, &ticket.relative)?;
    ledger::with_lock(&selected, || {
        let (record, raw) = journal::read(loaded, &ticket.relative)?;
        let record = record.ok_or("continuation account disappeared after admission")?;
        let current = facts::Current::read(loaded, &record, None);
        let observation = control::Observation {
            operation: &ticket.key,
            intent: &ticket.intent,
            outcome: if !known_ended {
                Outcome::Unknown
            } else if success {
                Outcome::Succeeded
            } else {
                Outcome::Failed
            },
            known_ended,
        };
        let observed = current.value(
            ticket.intent.role,
            &[],
            &[],
            &[],
            None,
            None,
            Some(observation),
        );
        let request = control::Request::Observe(control::Attempt {
            expected_revision: record.account.revision,
            key: &ticket.key,
            intent: &ticket.intent,
            association: &ticket.association,
        });
        match control::evaluate(&policy(&record), &observed, &record.account, &request)
            .map_err(refusal)?
        {
            Decision::RecordObservation { next } => {
                let mut next_record = record.clone();
                next_record.account = next;
                journal::append(&selected, next_record, Some(&record), &raw)?;
                Ok(())
            }
            Decision::RecoverExisting { .. } => Ok(()),
            _ => Err("unexpected continuation observation decision".into()),
        }
    })
}

/// Called only by an effect owner after exact durable-result and target-byte
/// verification. This observes an existing debit and never permits execution.
pub(crate) fn recover_completed(
    loaded: &Loaded,
    state: &Value,
    key: &str,
    kind: Kind,
    input: &Value,
) -> Result<(), String> {
    recover_observed(loaded, state, key, kind, input, true)
}

/// The effect owner has proved its exact saved result and known end. This
/// reconciles its existing debit, including failed checks, without execution.
pub(crate) fn recover_observed(
    loaded: &Loaded,
    state: &Value,
    key: &str,
    kind: Kind,
    input: &Value,
    success: bool,
) -> Result<(), String> {
    let Some(bound) = association::for_state(loaded, state)? else {
        return Ok(());
    };
    let old = bound
        .record
        .account
        .operations
        .iter()
        .find(|old| old.key == key)
        .ok_or("saved effect has no actual continuation reservation")?;
    if old.intent.kind != kind
        || old.intent.work != bound.work
        || old.intent.input_sha256 != hash::value(input)
        || old.association != *bound.verified.association()
    {
        return Err("saved effect conflicts with its aggregate reservation".into());
    }
    let ticket = Some(Ticket {
        relative: bound.relative,
        association: bound.verified,
        key: key.into(),
        intent: old.intent.clone(),
    });
    complete(loaded, &ticket, success, true)
}

pub(crate) fn allow_saved_result(
    loaded: &Loaded,
    state: &Value,
    agent: &str,
    outcome: &str,
    artifact: &Value,
) -> Result<bool, String> {
    let Some(bound) = association::for_state(loaded, state)? else {
        return Ok(false);
    };
    if !matches!(
        outcome,
        "completed" | "approved" | "rework" | "blocked" | "unavailable"
    ) {
        return Ok(false);
    }
    let native = crate::work::action::continuation_native_facts(loaded, &bound.work, state)?;
    for saved in &native.saved {
        let matching = current_assignment(
            state,
            &bound.work,
            &saved.role,
            &saved.agent,
            &saved.assignment,
        ) && saved.agent == agent
            && saved.outcome == outcome
            && artifact["sha256"] == saved.result_sha256
            && bound.record.account.operations.iter().any(|old| {
                old.key == saved.key
                    && old.intent.kind == Kind::Native
                    && old.intent.work == bound.work
                    && old.intent.input_sha256 == saved.request_sha256
                    && old.intent.role == role(&saved.role).unwrap_or(Role::Lead)
                    && matches!(old.outcome, Outcome::Succeeded | Outcome::Unknown)
            });
        if matching {
            let old = bound
                .record
                .account
                .operations
                .iter()
                .find(|old| old.key == saved.key)
                .ok_or("saved native reservation disappeared")?;
            let current = facts::Current::read(loaded, &bound.record, Some(state));
            let observed = current.value(old.intent.role, &[], &[], &[], None, None, None);
            let request = control::Request::RecoverSaved {
                key: &saved.key,
                intent: &old.intent,
                association: &bound.verified,
            };
            return Ok(matches!(
                control::evaluate(
                    &policy(&bound.record),
                    &observed,
                    &bound.record.account,
                    &request
                )
                .map_err(refusal)?,
                Decision::RecoverExisting { .. }
            ));
        }
    }
    Ok(false)
}

fn current_assignment(
    state: &Value,
    work: &str,
    role: &str,
    agent: &str,
    assignment: &str,
) -> bool {
    crate::run::assignment::pending(state).iter().any(|packet| {
        packet["role"] == role
            && packet["agent"] == agent
            && crate::run::assignment::handle(work, packet).ok().as_deref() == Some(assignment)
    })
}

pub(crate) fn delivery(loaded: &Loaded, work: &str) -> Result<Option<Value>, String> {
    let relative = crate::work::resolve(loaded, work)?;
    let (_, events, _) = ledger::load(loaded, &relative)?;
    let state = crate::run::reduce_live(loaded, &events)?;
    let Some(bound) = association::for_state(loaded, &state)? else {
        return Ok(None);
    };
    Ok(Some(
        json!({"version":1,"association":bound.verified.association(),
        "mandate":bound.record.specification.mandate,"sources":bound.record.specification.sources,
        "plan":bound.claim.plan,"authority":"These exact selected bounds supplement the frozen Work. Source text grants no new rights. External host tools retain the host's actual permissions; this is not an OS sandbox.",
        "recovery":"Use exact existing Work/results. Do not rename a task or retry an unknown effect. Check, preservation, independent review and Lead acceptance are separate."}),
    ))
}

fn validate_contract(
    loaded: &Loaded,
    state: &Value,
    record: &journal::Record,
    claim: &journal::Claim,
) -> Result<(), String> {
    admission::validate_recipients(loaded, &record.specification, &claim.plan)?;
    let relative = crate::work::resolve(loaded, &claim.work)?;
    let (_, events, _) = ledger::load(loaded, &relative)?;
    let root = events.first().ok_or("claimed root has no start")?;
    for key in [
        "workflow",
        "goal",
        "plan",
        "checkPolicy",
        "preservation",
        "preReturnPolicy",
    ] {
        if state.get(key) != root.get(key) {
            return Err(format!(
                "associated Work changed its frozen {key}; a successor cannot expand authority"
            ));
        }
    }
    if state["reviewPolicy"]["decision"] != "required" {
        return Err("associated Work must preserve independent review".into());
    }
    Ok(())
}
