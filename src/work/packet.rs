//! Residual continuation packets: projection from one captured run revision,
//! state-derived human help, and validation of a previously issued packet.
//!
//! A packet is a derived handoff snapshot, never an authority. Validation
//! reprojects the canonical packet from one snapshot, compares every
//! behavior-relevant field of the submitted object, and always returns the
//! canonical packet for the consumer to act on. Nothing in a submitted packet
//! is executed, and a terminal run is history, not permission to continue.

use crate::run::{InputContext, RunSnapshot};
use serde_json::{json, Value};
use std::io::Read;

pub(crate) const PACKET_VERSION: u64 = 2;
const MAX_PACKET_BYTES: u64 = 256 * 1024;

/// Fields whose values drive consumer behavior. `humanHelp` is display text
/// derived from the same facts and is replaced, never compared.
const BEHAVIOR_FIELDS: [&str; 13] = [
    "version",
    "work",
    "workflow",
    "goal",
    "historical",
    "snapshot",
    "currentSubject",
    "alreadyEstablished",
    "stillValid",
    "remaining",
    "next",
    "doNotRepeat",
    "invalidation",
];

pub(crate) fn project(work: &str, snapshot: &RunSnapshot, next: &Value) -> Result<Value, String> {
    let view = snapshot.inspect_view();
    let status = if view["status"] == "running" {
        snapshot.status_view()?
    } else {
        Value::Null
    };
    Ok(project_from(work, &view, &status, next, snapshot.inputs()))
}

/// Resolve the human-help decision once so every packet projection carries the
/// same actor, including failure, unavailable-input, and governor branches.
pub(crate) fn resolved_actor(
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Result<Value, String> {
    Ok(project(work, snapshot, next)?["humanHelp"]["nextAction"]["actor"].clone())
}

/// The canonical state facts the conversational layer classifies from.
///
/// They come from the same reduced snapshot, check targets, and review
/// assessment that drive work decisions, so rewording or translating human
/// help can never move a classification. Presentation reads these; it never
/// reads display text.
pub(crate) fn facts(
    snapshot: &RunSnapshot,
    next: &Value,
    current_inputs: impl FnOnce() -> Option<String>,
) -> Result<Value, String> {
    let view = snapshot.inspect_view();
    let status = if view["status"] == "running" {
        snapshot.status_view()?
    } else {
        Value::Null
    };
    let mut facts = facts_from(&view, &status, next);
    facts["acceptance"] = json!(acceptance_state(&view, current_inputs));
    Ok(facts)
}

/// Whether a recorded acceptance still describes the files present now.
///
/// A terminal run's identities are the historical ones: the kernel establishes
/// nothing about the current tree for it. So an acceptance may speak for the
/// present only while the exact tested inputs it was bound to are still the
/// ones on disk. Anything else - a changed file, an unreadable tree, a run
/// from before inputs were bound at all - leaves it as history.
/// The terminal display value for a run, or nothing. Every surface that
/// reports a decision offers the same wording from the same rule, so a reader
/// never has to compose one.
pub(crate) fn terminal_display(
    view: &Value,
    current_inputs: impl FnOnce() -> Option<String>,
) -> Option<&'static str> {
    (acceptance_state(view, current_inputs) == "current").then_some("EXIT READY")
}

fn acceptance_state(view: &Value, current_inputs: impl FnOnce() -> Option<String>) -> &'static str {
    if view["status"] != "accepted" {
        return "none";
    }
    let accepted = view["submissions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| event["role"] == "lead" && event["outcome"] == "accepted")
        .last()
        .and_then(|event| event["inputsSha256"].as_str().map(str::to_owned));
    let Some(accepted) = accepted else {
        return "unbound";
    };
    match current_inputs() {
        Some(now) if now == accepted => "current",
        Some(_) => "superseded",
        None => "unknown",
    }
}

fn facts_from(view: &Value, status: &Value, next: &Value) -> Value {
    if view["status"] != "running" {
        // A terminal run is history; it carries no current obligation.
        return json!({"check": "none", "review": "none", "preservation": "none"});
    }
    let targets = status["checks"]["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let of_kind = |preservation: bool| {
        targets
            .iter()
            .filter(|target| (target["kind"] == "preservation") == preservation)
            .cloned()
            .collect::<Vec<Value>>()
    };
    let any = |group: &[Value], status_name: &str| {
        group.iter().any(|target| target["status"] == status_name)
    };
    let checks = of_kind(false);
    let preservations = of_kind(true);
    // Precedence is deliberate: a failure outranks a gap, and a gap outranks
    // the evidence that did pass, so a partial set is never reported current.
    let check = if any(&checks, "failed") {
        "failed"
    } else if let Some(missing) = checks.iter().find(|target| target["status"] == "missing") {
        if stale_check(view, missing) {
            "stale"
        } else {
            "missing"
        }
    } else if any(&checks, "passed") {
        "current"
    } else {
        "none"
    };
    let preservation = if any(&preservations, "failed") {
        "failed"
    } else if any(&preservations, "missing") {
        "active"
    } else if any(&preservations, "passed") {
        "satisfied"
    } else {
        "none"
    };
    let review = match review_state(view) {
        _ if next["role"] == "reviewer" => "missing",
        ReviewState::Current if next["action"] == "lead_decision" => "approved",
        ReviewState::Stale if next["action"] == "lead_decision" => "stale",
        _ => "none",
    };
    json!({"check": check, "review": review, "preservation": preservation})
}

fn head(view: &Value) -> Value {
    view["events"]
        .as_array()
        .and_then(|events| events.last())
        .map_or(Value::Null, |event| event["eventSha256"].clone())
}

fn project_from(
    work: &str,
    view: &Value,
    status: &Value,
    next: &Value,
    inputs: &InputContext,
) -> Value {
    let mut established = Vec::new();
    let mut still_valid = Vec::new();
    let mut remaining = Vec::new();
    let mut do_not_repeat = Vec::new();
    let goal = view["events"][0]["goal"].clone();
    let historical = view["status"] != "running";

    if goal.as_str().is_some_and(|goal| !goal.is_empty()) {
        established.push(json!({"fact": "work_identity_recorded", "status": "completed"}));
    }
    let targets = status["checks"]["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let review = review_state(view);
    // History stays inspectable in the ledger; it grants no continuation reuse.
    if !historical {
        if scope_recorded(view) {
            established.push(json!({"fact": "scope_recorded", "status": "completed"}));
            do_not_repeat.push(json!("scope"));
        }
        let mut check_obligation_recorded = false;
        for target in targets.iter().filter(|target| target["status"] == "passed") {
            let preservation = target["kind"] == "preservation";
            still_valid.push(json!({
                "evidence": if preservation { "preservation" } else { "current_check" },
                "status": "passed",
                "checkEventSha256": target["checkEventSha256"],
                "requirementId": target["requirementId"],
                "subject": "current",
                "inputs": "current",
            }));
            do_not_repeat.push(json!(if preservation {
                format!(
                    "preservation:{}",
                    target["requirementId"].as_str().unwrap_or("unknown")
                )
            } else {
                "passed_check".to_owned()
            }));
        }
        for target in targets
            .iter()
            .filter(|target| target["status"] == "missing")
        {
            remaining.push(
                json!({"obligation": target["kind"], "requirementId": target["requirementId"]}),
            );
            check_obligation_recorded = true;
        }
        for target in targets.iter().filter(|target| target["status"] == "failed") {
            remaining.push(json!({
                "obligation": if target["kind"] == "preservation" {
                    "rework_after_failed_preservation"
                } else {
                    "rework_after_failed_check"
                },
                "requirementId": target["requirementId"],
            }));
        }
        if next["action"] == "lead_decision" && next_outcome_is(next, "scoped") {
            remaining.push(json!({"obligation": "scope"}));
        } else if next["action"] == "lead_decision" {
            if review == ReviewState::Current {
                still_valid.push(
                    json!({"evidence": "current_review", "status": "approved", "inputs": "current"}),
                );
                do_not_repeat.push(json!("review"));
            }
            remaining.push(json!({"obligation": "lead_acceptance"}));
        } else if next["role"] == "reviewer" {
            remaining.push(json!({"obligation": "review"}));
        } else if next["role"] == "worker" {
            remaining.push(json!({"obligation": "implementation"}));
        } else if next["action"] == "check" && !check_obligation_recorded {
            remaining.push(json!({"obligation": "check"}));
        }
    }
    let input_bound = !matches!(inputs, InputContext::NotBound);
    let current_inputs = match inputs {
        InputContext::Current(digest) => json!(digest),
        _ => Value::Null,
    };
    let checker_sha256 = targets
        .iter()
        .find_map(|target| target["checkCommandSha256"].as_str())
        .map_or(Value::Null, |value| json!(value));
    let config_sha256 = view["subject"]["configSha256"].clone();
    let human_help = help(work, view, &targets, next, review, &remaining, inputs);
    let mut resolved_next = next.clone();
    resolved_next["resolvedActor"] = human_help["nextAction"]["actor"].clone();
    let mut packet = json!({
        "version": PACKET_VERSION,
        "work": work,
        "workflow": view["workflow"],
        "goal": goal,
        "historical": historical,
        "snapshot": {
            "eventCount": view["events"].as_array().map_or(0, Vec::len),
            "headEventSha256": head(view),
            "inputsSha256": current_inputs,
        },
        "currentSubject": view["subject"],
        "basis": view.get("basis").cloned().unwrap_or(Value::Null),
        "reviewPolicy": view.get("reviewPolicy").cloned().unwrap_or(Value::Null),
        "alreadyEstablished": established,
        "stillValid": still_valid,
        "remaining": remaining,
        "next": next["action"],
        "humanHelp": human_help,
        "doNotRepeat": do_not_repeat,
        "invalidation": {
            "rule": "reuse only while the run is active and its ledger head, exact subject, and tested inputs remain current",
            "source": "run_ledger",
            "owner": "lead",
            "sessionRestartInvalidates": false,
            "subjectChangeInvalidates": true,
            "basisChangeInvalidates": true,
            "checkerChangeInvalidates": true,
            "configChangeInvalidates": true,
            "configSha256": config_sha256,
            "checkerSha256": checker_sha256,
            "testedInputChangeInvalidates": input_bound,
            "inputCoverage": if input_bound { json!(crate::run::inputs::COVERAGE) } else { Value::Null },
        }
    });
    // The context projection is a thin, role-specific view over this same
    // validated snapshot.  It carries exact expansion references rather than
    // copying history into every prompt.
    if let Ok(context) = crate::context::project(work, view, status, &resolved_next) {
        packet["context"] = context;
    }
    packet
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewState {
    None,
    Current,
    Stale,
}

/// An approval in the current attempt is current only when (for v6) it was
/// given on the tested inputs present now.
fn review_state(view: &Value) -> ReviewState {
    let attempt = &view["attempt"];
    let approvals = view["submissions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| {
            &event["attempt"] == attempt
                && event["role"] == "reviewer"
                && event["outcome"] == "approved"
        });
    let mut state = ReviewState::None;
    for approval in approvals {
        if approval.get("inputsSha256").is_none()
            || approval["inputsSha256"] == view["inputsSha256"]
        {
            return ReviewState::Current;
        }
        state = ReviewState::Stale;
    }
    state
}

fn stale_check(view: &Value, target: &Value) -> bool {
    view["events"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|event| {
            event["action"] == "check"
                && event["targetEventSha256"] == target["targetEventSha256"]
                && event["requirementId"] == target["requirementId"]
        })
}

fn command(args: &[&str]) -> Value {
    json!({"program": crate::compatibility::profile().caller, "args": args})
}

fn label(target: &Value) -> String {
    match target["requirementId"].as_str() {
        Some(id) => format!("preservation check '{id}'"),
        None => "functional check".to_owned(),
    }
}

/// Human help derived from canonical state. `ownerDecision` distinguishes an
/// internal lead action from a decision only the human owner can make.
fn help(
    work: &str,
    view: &Value,
    targets: &[Value],
    next: &Value,
    review: ReviewState,
    remaining: &[Value],
    inputs: &InputContext,
) -> Value {
    let next_command = command(&["work", "next", work]);
    let (what, actor, summary, cmd, owner) = if view["status"] != "running" {
        let status = view["status"].as_str().unwrap_or("unknown");
        let (what, summary, owner) = match status {
            "accepted" => (
                "This run was accepted. That acceptance is history: it does not authorize skipping work against the files present now.",
                "start new governed work for any further change",
                "not_required",
            ),
            "blocked" => (
                "This run ended blocked. The requested outcome was not accepted.",
                "decide whether to start new work, change the goal, or stop",
                "required",
            ),
            _ => (
                "This run ended without acceptance. The requested outcome was not accepted.",
                "decide whether to start new work or stop",
                "required",
            ),
        };
        (
            what.to_owned(),
            "owner",
            summary.to_owned(),
            Value::Null,
            owner,
        )
    } else if let InputContext::Unavailable(reason) = inputs {
        (
            format!("Current tested inputs cannot be established ({reason}); no evidence that depends on them is current."),
            "lead",
            "resolve the input listing problem, then request the next action again".to_owned(),
            next_command,
            "unknown",
        )
    } else if let Some(failed) = targets.iter().find(|target| target["status"] == "failed") {
        let functional_passed = failed["requirementId"].is_string()
            && targets
                .iter()
                .any(|target| target["requirementId"].is_null() && target["status"] == "passed");
        (
            format!(
                "Not ready: the {} failed on the current result{}. A failing checker can also mean a broken checker or environment.",
                label(failed),
                if functional_passed { "; the functional check passed" } else { "" }
            ),
            "lead",
            "inspect the failure, then repair within the accepted requirement (lead outcome: rework)".to_owned(),
            next_command,
            "only_if_requirement_changes",
        )
    } else if let Some(missing) = targets.iter().find(|target| target["status"] == "missing") {
        let what = if stale_check(view, missing) {
            format!(
                "Earlier {} evidence no longer applies: the result or tested files changed after it ran. This is not a failure.",
                label(missing)
            )
        } else {
            format!("The {} has not run on the current result.", label(missing))
        };
        (
            what,
            "exitbind",
            format!("run the bound {}", label(missing)),
            command(&["work", "check", work]),
            "not_required",
        )
    } else if next["action"] == "lead_decision" && next_outcome_is(next, "scoped") {
        (
            "Work was started; scope has not been recorded.".to_owned(),
            "lead",
            "record scope (lead outcome: scoped) or blocked".to_owned(),
            next_command,
            "not_required",
        )
    } else if next["action"] == "lead_decision" && review == ReviewState::Stale {
        (
            "Not ready: the review approval was given on different tested files and cannot be reused.".to_owned(),
            "lead",
            "send the work back for another pass (lead outcome: rework)".to_owned(),
            next_command,
            "not_required",
        )
    } else if next["action"] == "lead_decision" {
        (
            "Required checks and review are current for this result.".to_owned(),
            "lead",
            "decide acceptance (lead outcome: accepted, rework, or blocked)".to_owned(),
            next_command,
            "not_required",
        )
    } else {
        let role = next["role"].as_str().unwrap_or("assigned agent");
        (
            format!("A {role} assignment is pending."),
            if matches!(role, "worker" | "reviewer" | "adviser") {
                role
            } else {
                "lead"
            },
            format!("run the pending {role} assignment"),
            next_command,
            "not_required",
        )
    };
    let mut help = json!({
        "whatHappened": what,
        "whatRemains": remaining,
        "nextAction": {"actor": actor, "summary": summary, "command": cmd},
        "ownerDecision": owner,
    });
    if let Some(evidence) = preservation_evidence(targets) {
        help["preservationEvidence"] = json!(evidence);
    }
    if targets
        .iter()
        .any(|target| target["kind"] == "preservation")
    {
        // A governed run carrying accepted preservation requirements is the
        // formal route, and the accepted policy assigns FULL to every formal
        // task. Resolve that here so the assignment travels with the
        // assignment packet instead of being guessed downstream. Exitbind
        // records the assignment; it cannot enforce a host's quality setting.
        help["preservationAssignment"] = json!({
            "route": "FORMAL",
            "quality": "FULL",
            "resolvedBy": "accepted_preservation_requirements",
            "enforcement": "recorded_not_enforced",
        });
    }
    help
}

/// How the preservation evidence in this run was acquired, in the run's own
/// terms. `command_observed` means Exitbind ran the requirement's check itself;
/// `agent_declared` means the host reported the result. Neither is independent
/// verification of what the check's output means.
fn preservation_evidence(targets: &[Value]) -> Option<&'static str> {
    let mut observed = false;
    let mut declared = false;
    for target in targets
        .iter()
        .filter(|target| target["kind"] == "preservation")
    {
        match target["acquisition"].as_str() {
            Some("observed") => observed = true,
            Some(_) => declared = true,
            // A requirement with no evidence yet says nothing about acquisition.
            None => {}
        }
    }
    match (observed, declared) {
        (true, false) => Some("command_observed"),
        (_, true) => Some("agent_declared"),
        (false, false) => None,
    }
}

pub(crate) fn read_bounded(path: &str) -> Result<Value, String> {
    let file =
        std::fs::File::open(path).map_err(|error| format!("packet cannot be read: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_PACKET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("packet cannot be read: {error}"))?;
    if bytes.len() as u64 > MAX_PACKET_BYTES {
        return Err("packet exceeds the supported size".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "packet is not valid JSON".into())
}

/// Structure, consistency, and applicability in that order. The canonical
/// packet is always returned; only `usable` permits skipping the work it lists.
pub(crate) fn validate(
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    packet: &Value,
) -> Result<Value, String> {
    let fresh = project(work, snapshot, next)?;
    let (result, reason, mismatched) = judge(work, packet, &fresh);
    let usable = result == "usable";
    Ok(json!({
        "work": work,
        "result": result,
        "reason": reason,
        "mismatchedFields": mismatched,
        "reuse": if usable { fresh["doNotRepeat"].clone() } else { json!([]) },
        "humanHelp": fresh["humanHelp"],
        "packet": fresh,
    }))
}

type Judgment = (&'static str, &'static str, Vec<&'static str>);

fn judge(work: &str, packet: &Value, fresh: &Value) -> Judgment {
    const CANNOT: &str = "cannot_establish_applicability";
    let Some(object) = packet.as_object() else {
        return (CANNOT, "packet_not_an_object", Vec::new());
    };
    if packet["version"].as_u64() != Some(PACKET_VERSION) {
        return (CANNOT, "unsupported_packet_version", Vec::new());
    }
    // Optional extensions are ignored; declared required meanings we do not
    // know are never downgraded to reuse.
    if object
        .get("requires")
        .is_some_and(|required| !required.as_array().is_some_and(|items| items.is_empty()))
    {
        return (CANNOT, "unsupported_required_semantics", Vec::new());
    }
    let missing = BEHAVIOR_FIELDS
        .iter()
        .any(|field| !object.contains_key(*field));
    let shaped = !missing
        && packet["work"].is_string()
        && packet["snapshot"]["headEventSha256"].is_string()
        && packet["snapshot"]["eventCount"].is_u64()
        && packet["stillValid"].is_array()
        && packet["doNotRepeat"].is_array()
        && packet["remaining"].is_array();
    if !shaped {
        return (CANNOT, "malformed_packet", Vec::new());
    }
    if packet["work"] != work {
        return (CANNOT, "different_work", Vec::new());
    }
    if fresh["historical"] == true {
        return ("not_continuable", "terminal_run_is_history", Vec::new());
    }
    // A missing identity never establishes equality.
    if fresh["snapshot"]["inputsSha256"].is_null() {
        return (CANNOT, "tested_inputs_not_bound", Vec::new());
    }
    if packet["snapshot"]["inputsSha256"] != fresh["snapshot"]["inputsSha256"] {
        return ("refresh_required", "tested_inputs_changed", Vec::new());
    }
    if packet["snapshot"] != fresh["snapshot"] {
        return ("refresh_required", "ledger_advanced", Vec::new());
    }
    // Preserve the v0.21 applicability precedence for the canonical packet
    // before judging the additive v0.22 context projection.  A changed
    // tested input, ledger, or terminal state must retain its established
    // refusal reason even though that same change also makes context stale.
    if packet.get("context").is_some() {
        if let Err(error) = crate::context::validate_projection(packet, fresh) {
            let malformed = error.contains("malformed") || error.contains("missing");
            return (
                if malformed {
                    CANNOT
                } else {
                    "refresh_required"
                },
                if malformed {
                    "malformed_context_projection"
                } else {
                    "context_projection_changed"
                },
                vec!["context"],
            );
        }
    }
    let mismatched: Vec<&'static str> = BEHAVIOR_FIELDS
        .iter()
        .copied()
        .filter(|field| packet[*field] != fresh[*field])
        .collect();
    if !mismatched.is_empty() {
        return ("refresh_required", "claims_disagree_with_state", mismatched);
    }
    ("usable", "current", Vec::new())
}

fn scope_recorded(view: &Value) -> bool {
    view["submissions"].as_array().is_some_and(|submissions| {
        submissions
            .iter()
            .any(|event| event["role"] == "lead" && event["outcome"] == "scoped")
    })
}

fn next_outcome_is(next: &Value, expected: &str) -> bool {
    next["outcomes"]
        .as_array()
        .is_some_and(|outcomes| outcomes.iter().any(|outcome| outcome == expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Value {
        let mut packet = json!({
            "version": 2, "work": "w", "workflow": "change", "goal": "g", "historical": false,
            "snapshot": {"eventCount": 3, "headEventSha256": "h", "inputsSha256": "i"},
            "currentSubject": {"sha256": "s"}, "alreadyEstablished": [],
            "stillValid": [{"evidence": "current_check"}], "remaining": [{"obligation": "review"}],
            "next": "spawn", "doNotRepeat": ["passed_check"], "invalidation": {"rule": "r"}
        });
        packet["humanHelp"] = json!({"whatHappened": "x"});
        packet
    }

    #[test]
    fn judge_refuses_every_non_current_packet_and_accepts_the_matching_one() {
        let fresh = fresh();
        assert_eq!(judge("w", &fresh, &fresh).0, "usable");
        type Mutation = fn(&mut Value);
        let cases: [(&str, Mutation, &str, &str); 13] = [
            (
                "w",
                |p| p["version"] = json!(1),
                CANNOT_ESTABLISH,
                "unsupported_packet_version",
            ),
            (
                "w",
                |p| p["requires"] = json!(["future"]),
                CANNOT_ESTABLISH,
                "unsupported_required_semantics",
            ),
            (
                "w",
                |p| p["stillValid"] = json!("x"),
                CANNOT_ESTABLISH,
                "malformed_packet",
            ),
            (
                "w",
                |p| {
                    p.as_object_mut().unwrap().remove("remaining");
                },
                CANNOT_ESTABLISH,
                "malformed_packet",
            ),
            ("other", |_| {}, CANNOT_ESTABLISH, "different_work"),
            (
                "w",
                |p| p["snapshot"]["eventCount"] = json!(2),
                "refresh_required",
                "ledger_advanced",
            ),
            (
                "w",
                |p| p["snapshot"]["inputsSha256"] = json!("old"),
                "refresh_required",
                "tested_inputs_changed",
            ),
            (
                "w",
                |p| p["doNotRepeat"] = json!(["passed_check", "review"]),
                "refresh_required",
                "claims_disagree_with_state",
            ),
            (
                "w",
                |p| p["goal"] = json!("weaker goal"),
                "refresh_required",
                "claims_disagree_with_state",
            ),
            (
                "w",
                |p| p["currentSubject"] = json!({"sha256": "other"}),
                "refresh_required",
                "claims_disagree_with_state",
            ),
            (
                "w",
                |p| p["remaining"] = json!([]),
                "refresh_required",
                "claims_disagree_with_state",
            ),
            (
                "w",
                |p| p["next"] = json!("lead_decision"),
                "refresh_required",
                "claims_disagree_with_state",
            ),
            (
                "w",
                |p| p["historical"] = json!(true),
                "refresh_required",
                "claims_disagree_with_state",
            ),
        ];
        for (work, mutate, result, reason) in cases {
            let mut packet = fresh.clone();
            mutate(&mut packet);
            let judged = judge(work, &packet, &fresh);
            assert_eq!((judged.0, judged.1), (result, reason), "{reason}");
        }
        let mut unbound = fresh.clone();
        unbound["snapshot"]["inputsSha256"] = Value::Null;
        assert_eq!(judge("w", &unbound, &unbound).1, "tested_inputs_not_bound");
        let mut terminal = fresh.clone();
        terminal["historical"] = json!(true);
        assert_eq!(judge("w", &terminal, &terminal).0, "not_continuable");
        let mut display = fresh.clone();
        display["humanHelp"] = json!({"whatHappened": "edited display text"});
        display["futureHint"] = json!(true);
        assert_eq!(judge("w", &display, &fresh).0, "usable");
    }

    const CANNOT_ESTABLISH: &str = "cannot_establish_applicability";
}
