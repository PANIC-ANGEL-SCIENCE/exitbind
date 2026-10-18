//! Source-grounded continuation on the existing Goal and Work protocols.
mod admission;
mod amendment;
mod association;
pub(crate) mod control;
mod journal;
mod model;
mod recovery;
pub(crate) use recovery::recover;
pub(crate) mod runtime;
mod sources;

use crate::{config::Loaded, evidence::hash, run::ledger};
use chrono::{DateTime, Utc};
use journal::Record;
use model::{Mode, Plan, Spec};
use serde_json::{json, Value};

pub(crate) fn form() -> Value {
    json!({"version":1,"readOnly":true,"authority":"The host records the owner's existing explicit mandate; documents and this form grant no permission.",
        "specificationFields":["version","projectId","mandate","sources"],
        "mandateFields":["id","revision","outcome","mode","allowedSources","write","commands","roles","preserve","priorityPolicy","expiresAt","maxWorks","maxActions","maxParallel","maxElapsedSeconds","externalPublication"],
        "sourceFields":["id","kind","repository","revision","path","section","sha256","acquiredAt","currentness","trust"],
        "planFields":["version","id","taskId","title","rationale","outcome","acceptance","preservation","sourceIds","dependencies","workflow","write","checkCommand","preservationCheckCommand","estimatedActions","overlapReason","goalId","requirements","checker","integration","preReturn"],
        "preReturn":"Optional null or {command,timeoutMs,captureLimitBytes}; exact mandate command and existing bounded Work pre-return policy.",
        "modes":["propose","cycle","continue"],"sourceKinds":["git","export"],"trust":["ownerDirection","reference"],
        "planning":"The native Lead inspects the approved direction, actual source/tests and current Goal/Work evidence, derives a useful bounded slice, explains priority/dependencies/preservation and eligible recipients, and prepares a plan. No prewritten task list or model call is part of this deterministic command.",
        "continuation":"After current check, required independent review and Lead acceptance, inspect the updated project and derive the next useful slice under the same finite mandate. Stop on authority, source, budget or genuine preference gaps. No daemon or background scheduling is created.",
        "learning":"Look up existing scoped skills/lessons first. Prefer a causal mechanical correction; otherwise preserve inactive original/different/negative trials, independent review, exact-byte promotion, fresh relevant delivery/use and unrelated exclusion/revocation through the existing memory lifecycle.",
        "scope":"Existing project Goal requirements express outcomes; plans map to them. Work owns actual execution/check/review/acceptance. Native session journey membership is not implied.",
        "timeAmendment":"A time-only mandate amendment applies only to an exact associated Work after a bound owner preview; frozen starts and claims are never rewritten.",
        "amendmentFields":["revision","expiresAt","maxElapsedSeconds"]})
}

pub(crate) fn amend(
    loaded: &Loaded,
    work: &str,
    reason: &str,
    request_id: &str,
    expected: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    amendment::amend(loaded, work, reason, request_id, expected, apply)
}

pub(crate) fn time_amendment_hold(
    loaded: &Loaded,
    work: &str,
    state: &Value,
    binding: &str,
) -> Result<Option<Value>, String> {
    amendment::held_action(loaded, work, state, binding)
}

pub(crate) fn propose(loaded: &Loaded, source: &str) -> Result<Value, String> {
    let (spec, digest) = sources::read(loaded, source)?;
    let relative = journal::relative(loaded, &spec.mandate.id)?;
    let (record, _) = journal::read(loaded, &relative)?;
    let mut value = json!({"version":1,"state":"proposal","readOnly":true,"effect":"no-change",
        "specificationSha256":digest,"direction":spec.mandate.outcome,"priorityPolicy":spec.mandate.priority_policy,
        "sources":spec.sources,"mode":spec.mandate.mode,"planningContract":form(),
        "projectContext":crate::project::context::snapshot_for_task(loaded,Some(&spec.mandate.outcome)),
        "eligibility":eligibility(&spec,record.as_ref()).err(),
        "externalProgress":"unchanged; no external source write is performed"});
    if let Some(record) = record {
        value["continuation"] = projection(loaded, &record)?;
    }
    Ok(value)
}

fn preview_binding(loaded: &Loaded, digest: &str, head: Option<&Record>, reason: &str) -> String {
    hash::value(
        &json!({"configuration":hash::text(&loaded.source),"specification":digest,
        "project":crate::session_goal::requirements::identity(loaded).ok(),
        "head":head.map(|r| &r.event_sha256),"reason":reason}),
    )
}

pub(crate) fn select(
    loaded: &Loaded,
    source: &str,
    reason: &str,
    expected: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    model::bounded(reason, "selection reason", 4096)?;
    let (spec, digest) = sources::read(loaded, source)?;
    eligibility(&spec, None)?;
    let relative = journal::relative(loaded, &spec.mandate.id)?;
    let (prior, _) = journal::read(loaded, &relative)?;
    let binding = preview_binding(loaded, &digest, prior.as_ref(), reason);
    let mut response = json!({"version":1,"state":"preview","effect":"no-change","currentBinding":binding,
        "selection":spec,"reason":reason,"declaration":"Caller records an existing owner decision; this is not authentication or an inferred approval.",
        "apply":{"command":argv(loaded,&["select",source,"--reason",reason,"--current-binding",&binding,"--apply"])?}});
    if !apply {
        return Ok(response);
    }
    if expected != Some(binding.as_str()) {
        return Err("continuation selection preview is stale; no selection was recorded".into());
    }
    let selected = journal::prepare(loaded, &relative)?;
    ledger::with_lock(&selected, || {
        crate::work::details::ensure_current_config(loaded)?;
        let (fresh, fresh_digest) = sources::read(loaded, source)?;
        let (prior, raw) = journal::read(loaded, &relative)?;
        if fresh_digest != digest
            || preview_binding(loaded, &digest, prior.as_ref(), reason) != binding
        {
            return Err("continuation selection changed before apply".into());
        }
        eligibility(&fresh, None)?;
        if let Some(prior) = prior.as_ref() {
            if prior.specification_sha256 == digest && !prior.stopped {
                response["state"] = json!("unchanged");
                return Ok(response);
            }
            if fresh.mandate.revision <= prior.specification.mandate.revision {
                return Err("changed or stopped mandate needs a real owner-selected higher revision; spent claims are retained".into());
            }
        }
        let selected_at = prior
            .as_ref()
            .map_or_else(|| Utc::now().to_rfc3339(), |r| r.selected_at.clone());
        let project_identity = crate::session_goal::requirements::identity(loaded)?;
        let account = match &prior {
            Some(prior) => prior.account.clone(),
            None => control::Account::new(
                &control::Policy {
                    project: &project_identity,
                    specification_sha256: &digest,
                    mandate: &fresh.mandate,
                },
                DateTime::parse_from_rfc3339(&selected_at)
                    .map_err(|_| "invalid original selected time")?
                    .timestamp(),
            )
            .map_err(|reason| format!("invalid initial continuation account: {reason:?}"))?,
        };
        let record = Record {
            version: 2,
            revision: 0,
            project_identity,
            configuration_sha256: hash::text(&loaded.source),
            specification_path: source.into(),
            specification_sha256: digest.clone(),
            specification: fresh,
            selected_at,
            reason: reason.into(),
            stopped: false,
            claims: prior.as_ref().map_or_else(Vec::new, |r| r.claims.clone()),
            account,
            previous_event_sha256: None,
            event_sha256: String::new(),
        };
        let written = journal::append(&selected, record, prior.as_ref(), &raw)?;
        response["state"] = json!("selected");
        response["effect"] = json!("recorded");
        response["continuation"] = projection(loaded, &written)?;
        Ok(response)
    })
}

fn eligibility(spec: &Spec, record: Option<&Record>) -> Result<(), String> {
    let expiry = DateTime::parse_from_rfc3339(&spec.mandate.expires_at)
        .map_err(|_| "invalid mandate expiry")?;
    if Utc::now() >= expiry {
        return Err("mandate expired; no new admission or controlled action is permitted".into());
    }
    if let Some(record) = record {
        if record.stopped {
            return Err(
                "mandate stopped; running effects require actual host interruption/readback".into(),
            );
        }
        let start = DateTime::parse_from_rfc3339(&record.selected_at)
            .map_err(|_| "invalid selected time")?;
        if Utc::now().signed_duration_since(start).num_seconds()
            >= spec.mandate.max_elapsed_seconds as i64
        {
            return Err("aggregate elapsed-time bound exhausted".into());
        }
    }
    Ok(())
}

pub(crate) fn status(loaded: &Loaded, mandate: &str) -> Result<Value, String> {
    let relative = journal::relative(loaded, mandate)?;
    let (record, _) = journal::read(loaded, &relative)?;
    let Some(record) = record else {
        return Ok(json!({"state":"not_selected","readOnly":true,"claims":[]}));
    };
    projection(loaded, &record)
}

fn current(loaded: &Loaded, record: &Record) -> Result<(), String> {
    runtime::check_selected(loaded, record)
}

fn work_status(loaded: &Loaded, claim: &journal::Claim) -> Result<Value, String> {
    let relative = crate::work::resolve(loaded, &claim.work)?;
    let (_, events, _) = ledger::load(loaded, &relative)?;
    let start = events.first().ok_or("claimed Work has no start")?;
    if crate::run::start_event_intent_sha256(start)? != claim.start_intent_sha256 {
        return Err("claim Work start intent differs; inspect without retry".into());
    }
    let head = association::current_head(loaded, claim)?;
    let head_relative = crate::work::resolve(loaded, &head)?;
    let mut status = crate::run::status(loaded, &head_relative)?;
    status["rootWork"] = json!(claim.work);
    status["currentWork"] = json!(head);
    status["currentEvidence"] = crate::session_goal::requirements::current_checker_evidence(
        loaded,
        &head,
        &crate::session_goal::requirements::CheckerInput {
            path: claim.plan.checker.clone(),
            sha256: claim.checker_sha256.clone(),
        },
    )?;
    Ok(status)
}

fn projection(loaded: &Loaded, record: &Record) -> Result<Value, String> {
    let claims = record.claims.iter().map(|claim| {
        let status = work_status(loaded,claim);
        Ok(json!({"taskId":claim.task_id,"planId":claim.plan.id,"plan":claim.plan,"work":claim.work,
            "actualWork":status.as_ref().ok(),"unresolved":status.err(),
            "detail":{"command":work_argv(loaded,&claim.work)?},
            "savedStartRecovery":{"command":argv(loaded,&["recover",&record.specification.mandate.id,"--task",&claim.task_id])?}}))
    }).collect::<Result<Vec<Value>,String>>()?;
    let estimated: u64 = record.claims.iter().map(|c| c.plan.estimated_actions).sum();
    let unresolved = record
        .account
        .operations
        .iter()
        .filter(|operation| {
            matches!(
                operation.outcome,
                control::Outcome::Reserved | control::Outcome::Unknown
            )
        })
        .count();
    Ok(
        json!({"version":1,"readOnly":true,"state":if record.stopped {"stopped"} else {"selected"},
        "mandateId":record.specification.mandate.id,"mandateRevision":record.specification.mandate.revision,
        "specification":record.specification_path,"sourceSha256":record.specification_sha256,
        "head":record.event_sha256,"eligibility":current(loaded,record).err(),"claims":claims,
        "budget":{"claimedTasks":record.claims.len(),"actualWorks":record.account.works_spent(),
            "actualActions":record.account.actions_spent(),"estimatedActions":estimated,"unresolvedReservations":unresolved,
            "maxWorks":record.specification.mandate.max_works,"maxActions":record.specification.mandate.max_actions,
            "maxElapsedSeconds":record.specification.mandate.max_elapsed_seconds,"since":record.selected_at,
            "accounting":"One controlled action per selected plan/Work admission, native attempt, mutation grant, checker/pre-return attempt, decision, managed effect or successor. Failed/unknown reservations remain spent; labels and revised policy never reset them. Read-only proposals/recovery make no new execution. Original elapsed time includes host planning; unobserved host/model billing and calls remain unknown. Work governors independently apply."},
        "next":"Inspect actual Work before resuming. After acceptance, derive the next useful slice from updated project evidence. A claim is not completion.",
        "sourceProgress":"local derived progress only; external source unchanged"}),
    )
}

pub(crate) fn stop(loaded: &Loaded, mandate: &str, reason: &str) -> Result<Value, String> {
    model::bounded(reason, "stop reason", 4096)?;
    let relative = journal::relative(loaded, mandate)?;
    let selected = journal::prepare(loaded, &relative)?;
    ledger::with_lock(&selected, || {
        let (prior, raw) = journal::read(loaded, &relative)?;
        let prior = prior.ok_or("mandate has not been selected")?;
        let mut next = prior.clone();
        next.stopped = true;
        next.reason = reason.into();
        let written = if prior.stopped {
            prior.clone()
        } else {
            journal::append(&selected, next, Some(&prior), &raw)?
        };
        let mut response = projection(loaded, &written)?;
        response["readOnly"] = json!(prior.stopped);
        response["effect"] = json!(if prior.stopped {
            "no-change"
        } else {
            "stop_recorded"
        });
        response["interruption"] = json!({"owner":"the existing owned native/check execution","requested":true,"termination":"pending_or_unavailable",
            "action":"Active associated execution rechecks this durable stop at its next owned poll, interrupts safely and retains actual exit/group facts or uncertainty. Inspect each actual Work/native journal for confirmation. An absent owner cannot confirm interruption; unknown effects remain held."});
        Ok(response)
    })
}

pub(super) fn argv(loaded: &Loaded, tail: &[&str]) -> Result<Vec<String>, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut args = vec![
        executable
            .to_str()
            .ok_or("executable path is not UTF-8")?
            .into(),
        "project".into(),
        "advance".into(),
    ];
    args.extend(tail.iter().map(|s| s.to_string()));
    args.extend([
        "--config".into(),
        loaded
            .path
            .to_str()
            .ok_or("configuration path is not UTF-8")?
            .into(),
        "--json".into(),
    ]);
    Ok(args)
}

pub(super) fn work_argv(loaded: &Loaded, work: &str) -> Result<Vec<String>, String> {
    let mut args = argv(loaded, &[])?;
    args.splice(1..3, ["work".into(), "detail".into(), work.into()]);
    Ok(args)
}

pub(crate) use admission::admit;
