//! Bind controlled Run effects to their existing owner operation identities.
//! Runtime reservations are short account transactions, never Work locks.

use crate::project::continuation::{control::Kind, runtime};
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

pub(super) type Ticket = Option<runtime::Ticket>;

pub(super) fn begin_event(
    loaded: &Loaded,
    state: &Value,
    kind: Kind,
    event: &Value,
) -> Result<Ticket, String> {
    let input = event_input(event);
    let key = event_key(kind, &input);
    runtime::begin(
        loaded,
        state,
        runtime::Operation {
            kind,
            key: &key,
            role: event["role"].as_str().unwrap_or("lead"),
            agent: event.get("agent").and_then(Value::as_str),
            paths: &[],
            commands: &[],
            input: &input,
            predecessor: None,
        },
    )
}

pub(super) fn begin_check(
    loaded: &Loaded,
    state: &Value,
    owner: &str,
    input: &Value,
    command: &str,
    role: &str,
    agent: Option<&str>,
) -> Result<Ticket, String> {
    let key = check_key(owner, input);
    runtime::begin(
        loaded,
        state,
        runtime::Operation {
            kind: Kind::Check,
            key: &key,
            role,
            agent,
            paths: &[],
            commands: &[command.to_owned()],
            input,
            predecessor: None,
        },
    )
}

pub(super) fn complete(
    loaded: &Loaded,
    ticket: &Ticket,
    success: bool,
    known_ended: bool,
) -> Result<(), String> {
    runtime::complete(loaded, ticket, success, known_ended)
}

fn event_input(event: &Value) -> Value {
    let mut input = event.clone();
    if let Some(object) = input.as_object_mut() {
        object.remove("timestamp");
        object.remove("eventSha256");
    }
    input
}

fn event_key(kind: Kind, input: &Value) -> String {
    hash::value(&json!({"owner":"run-event","kind":kind,"input":input}))
}

fn check_key(owner: &str, input: &Value) -> String {
    hash::value(&json!({"owner":owner,"input":input}))
}

/// The caller has proved this exact recorded event through its owner replay.
pub(super) fn recover_event(
    loaded: &Loaded,
    state: &Value,
    kind: Kind,
    event: &Value,
) -> Result<(), String> {
    let input = event_input(event);
    runtime::recover_completed(loaded, state, &event_key(kind, &input), kind, &input)
}

pub(super) fn recover_check(
    loaded: &Loaded,
    state: &Value,
    owner: &str,
    input: &Value,
    success: bool,
) -> Result<(), String> {
    runtime::recover_observed(
        loaded,
        state,
        &check_key(owner, input),
        Kind::Check,
        input,
        success,
    )
}

/// Restore the pre-admission byte identity, never a reserialized substitute.
/// Only called after the observation owner has validated exact saved replay.
pub(super) fn recover_observed_check(
    loaded: &Loaded,
    ledger: &str,
    check: &Value,
) -> Result<(), String> {
    let (_, events, source) = crate::run::ledger::load(loaded, ledger)?;
    let state = crate::run::state::reduce(&events)?;
    if state.get("projectContinuation").is_none() {
        return Ok(());
    }
    if !events
        .iter()
        .any(|event| event == check && event["action"] == "check")
    {
        return Err("saved check is not an exact canonical ledger event".into());
    }
    let index = events
        .iter()
        .position(|event| {
            event["action"] == "check_observation"
                && event["eventSha256"] == check["observationEventSha256"]
        })
        .ok_or("saved check has no actual observation admission")?;
    for stream in ["stdout", "stderr"] {
        let artifact = check
            .get(stream)
            .ok_or("saved observed check capture is missing")?;
        crate::run::artifact::read_exact(
            loaded,
            artifact,
            "observed check capture",
            crate::run_value::MAX_CAPTURE_BYTES,
        )?;
    }
    let prefix = source.split_inclusive('\n').take(index).collect::<String>();
    let observation = &events[index]["observation"];
    let input = json!({"identity":observation["binding"],
        "ledgerSourceSha256":hash::text(&prefix),"timeoutMs":observation["timeoutMs"]});
    recover_check(
        loaded,
        &state,
        "observed-check",
        &input,
        check["result"]["kind"] == "exit" && check["result"]["code"] == 0,
    )
}

/// Every capture wait calls this owner. Actual journal/source inspection is
/// bounded to once per 250 ms; the initial call always checks eligibility.
pub(super) fn checker_control<'a>(
    loaded: &'a Loaded,
    state: &'a Value,
) -> impl FnMut() -> Result<(), String> + 'a {
    let mut last = None;
    let mut observed = Ok(());
    move || {
        let now = std::time::Instant::now();
        if last.map_or(true, |previous: std::time::Instant| {
            now.duration_since(previous) >= std::time::Duration::from_millis(250)
        }) {
            observed = runtime::guard(loaded, state);
            last = Some(now);
        }
        observed.clone()
    }
}

fn saved_event_kind(event: &Value) -> Option<Kind> {
    match event["action"].as_str()? {
        "submit" if matches!(event["role"].as_str(), Some("lead" | "reviewer")) => {
            Some(Kind::Effect)
        }
        "review_policy" => Some(Kind::Effect),
        "govern" if event["governorEvent"]["action"] == "mutation" => Some(Kind::Mutation),
        "govern" if event["governorEvent"]["action"] == "replan" => Some(Kind::Effect),
        _ => None,
    }
}

/// Read-only identities of canonical committed decisions. A listed event grants
/// no effect; the account must independently contain its exact prior receipt.
pub(crate) fn saved_continuation_events(
    loaded: &Loaded,
    ledger: &str,
) -> Result<Vec<Value>, String> {
    let (_, events, source) = crate::run::ledger::load(loaded, ledger)?;
    let state = crate::run::reduce_live(loaded, &events)?;
    if state.get("projectContinuation").is_none() {
        return Ok(Vec::new());
    }
    let mut saved = Vec::new();
    for event in &events {
        if let Some((input, success)) = saved_check_input(&events, &source, event)? {
            saved.push(json!({"eventSha256":event["eventSha256"],
                "operation":check_key("observed-check",&input), "kind":Kind::Check,
                "inputSha256":hash::value(&input),"success":success}));
            continue;
        }
        if let Some(kind) = saved_event_kind(event) {
            let input = event_input(event);
            saved.push(
                json!({"eventSha256":event["eventSha256"],"operation":event_key(kind,&input),
                "kind":kind,"inputSha256":hash::value(&input)}),
            );
        }
    }
    Ok(saved)
}

/// Verify the actual immutable event again at application. This can only
/// observe an existing matching debit; no event, permission or effect is made.
pub(crate) fn recover_continuation_event(
    loaded: &Loaded,
    ledger: &str,
    event_sha256: &str,
) -> Result<(), String> {
    let (_, events, source) = crate::run::ledger::load(loaded, ledger)?;
    let state = crate::run::reduce_live(loaded, &events)?;
    let event = events
        .iter()
        .find(|event| event["eventSha256"] == event_sha256)
        .ok_or("saved decision is absent from the canonical Work chain")?;
    if let Some((input, success)) = saved_check_input(&events, &source, event)? {
        let captures = if event["action"] == "check" {
            vec![event["stdout"].clone(), event["stderr"].clone()]
        } else {
            event["observation"]["facts"]["partialCaptures"]
                .as_array()
                .ok_or("saved failed check has no capture facts")?
                .clone()
        };
        for capture in captures {
            crate::run::artifact::read_exact(
                loaded,
                &capture,
                "saved check capture",
                crate::run_value::MAX_CAPTURE_BYTES,
            )?;
        }
        return recover_check(loaded, &state, "observed-check", &input, success);
    }
    let kind =
        saved_event_kind(event).ok_or("event is not a supported saved continuation decision")?;
    recover_event(loaded, &state, kind, event)
}

fn saved_check_input(
    events: &[Value],
    source: &str,
    event: &Value,
) -> Result<Option<(Value, bool)>, String> {
    let (admission, success) = match event["action"].as_str() {
        Some("check") if event.get("observationEventSha256").is_some() => (
            &event["observationEventSha256"],
            event["result"]["kind"] == "exit" && event["result"]["code"] == 0,
        ),
        Some("check_observation_failed")
            if event["observation"]["facts"]["termination"] == "ended" =>
        {
            (&event["observation"]["admissionEventSha256"], false)
        }
        _ => return Ok(None),
    };
    let index = events
        .iter()
        .position(|prior| {
            prior["action"] == "check_observation" && prior["eventSha256"] == *admission
        })
        .ok_or("saved check admission is missing")?;
    let event_index = events
        .iter()
        .position(|prior| prior == event)
        .ok_or("saved check is absent from its canonical chain")?;
    if index >= event_index {
        return Err("saved check predates its admission".into());
    }
    let prefix = source.split_inclusive('\n').take(index).collect::<String>();
    let observation = &events[index]["observation"];
    Ok(Some((
        json!({"identity":observation["binding"],"ledgerSourceSha256":hash::text(&prefix),
        "timeoutMs":observation["timeoutMs"]}),
        success,
    )))
}
