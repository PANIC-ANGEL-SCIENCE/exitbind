//! Explicit Lead review of compatible configuration lineage. Work is never imported.
use super::ledger::*;
use crate::{
    config::{self, Loaded},
    evidence::hash,
};
use serde_json::{json, Value};

/// At most this many ledgers are reviewed by one command.
const MAX_LEDGERS: usize = 64;

/// Review one configuration change for every listed lesson ledger. Every
/// ledger is checked with the same rule under one lock before any is
/// appended to; a refusal appends nothing. Each ledger then receives its own
/// ordinary revalidation event. A ledger already current is skipped, so an
/// interrupted run completes by running the same command again.
pub(crate) fn run(
    loaded: &Loaded,
    actor: &str,
    ledgers: &[&str],
    from: &str,
    reason: &str,
    apply: bool,
) -> Result<Value, String> {
    super::ensure_config_current(loaded)?;
    if Some(actor) != loaded.lead() || reason.trim().is_empty() || reason.len() > 1024 {
        return Err(
            "lesson revalidation requires the current configured Lead and reviewed reason".into(),
        );
    }
    if ledgers.is_empty() || ledgers.len() > MAX_LEDGERS {
        return Err(format!(
            "memory revalidate requires 1 to {MAX_LEDGERS} LEDGER arguments"
        ));
    }
    if (1..ledgers.len()).any(|index| ledgers[..index].contains(&ledgers[index])) {
        return Err("memory revalidate lists a LEDGER more than once".into());
    }
    let _lock = super::lessons::mutation_lock(loaded)?;
    let previous = load_previous(loaded, from)?;
    if hash::text(&previous.source) == hash::text(&loaded.source) {
        return Err("lesson configuration is unchanged; no revalidation is needed".into());
    }
    let mut prepared = Vec::new();
    let mut refused = Vec::new();
    for ledger in ledgers {
        match prepare(loaded, &previous, actor, ledger, reason) {
            Ok(ready) => prepared.push(ready),
            Err(error) => refused.push((*ledger, error)),
        }
    }
    if let Some((first, error)) = refused.first() {
        if ledgers.len() == 1 {
            return Err(error.clone());
        }
        let others = refused[1..]
            .iter()
            .fold(String::new(), |mut text, (ledger, error)| {
                text.push_str(&format!("{ledger}: {}; ", crate::drift::plain(error)));
                text
            });
        return Err(format!(
            "memory revalidate appended nothing to any of {} ledgers; {} refused. {others}{first}: {error}",
            ledgers.len(),
            refused.len()
        ));
    }
    let mut responses: Vec<Value> = Vec::new();
    let total = prepared.len();
    for ready in prepared {
        let response = match ready {
            Prepared::Current(response) => response,
            Prepared::Ready(ready) if apply => {
                let ledger = ready.ledger.clone();
                ready.apply(loaded, &previous, actor).map_err(|error| {
                    // Only a race after the shared checks reaches here; say
                    // what is already recorded and how the rest completes.
                    let recorded: Vec<&str> = responses
                        .iter()
                        .filter(|response| response["effect"] == "recorded")
                        .filter_map(|response| response["ledger"].as_str())
                        .collect();
                    if recorded.is_empty() {
                        return error;
                    }
                    let again = crate::emitted::Route::new(["memory", "revalidate", actor])
                        .words(ledgers.iter().copied())
                        .words(["--from-config", from, "--reason", reason, "--apply"])
                        .config(&loaded.path)
                        .shell_text();
                    format!(
                        "memory revalidate recorded {} of {total} ledgers ({}) and refused {ledger}: {error}; a ledger already recorded is skipped when this runs again: {again}",
                        recorded.len(),
                        recorded.join(", "),
                    )
                })?
            }
            Prepared::Ready(ready) => ready.response,
        };
        responses.push(response);
    }
    if responses.len() == 1 {
        return Ok(responses.remove(0));
    }
    let recorded = responses.iter().any(|r| r["effect"] == "recorded");
    Ok(json!({"effect":if recorded {"recorded"} else {"no-change"},
        "status":if apply {"revalidated"} else {"preview"},
        "configSha256":hash::text(&loaded.source),"ledgers":responses,
        "meaning":"each ledger carries its own reviewed revalidation event; no Work, task grant, check, review approval or acceptance is transferred"}))
}

/// This project's kept stamped copy sits beside the ledgers; its bytes are
/// read as they apply at this configuration's location. Any other file is
/// loaded where it is, so a foreign project's roots still refuse.
fn load_previous(loaded: &Loaded, from: &str) -> Result<Loaded, String> {
    if super::stamped_config::is_kept_copy(loaded, from) {
        config::load_bytes_at(from, &loaded.path)
    } else {
        config::load(Some(from))
    }
}

/// The ledger's history against the prior configuration, refused for this
/// one ledger only.
fn history_against(previous: &Loaded, snapshot: &LedgerSnapshot) -> Result<(), String> {
    super::state::history_drift(previous, snapshot).map_err(|drift| match drift {
        super::state::HistoryDrift::Configuration(bound) => {
            super::stamped_config::changed_ledgers(previous, &[snapshot], &bound)
        }
        super::state::HistoryDrift::Other(error) => error,
    })
}

enum Prepared {
    Current(Value),
    Ready(Box<Ready>),
}

struct Ready {
    snapshot: LedgerSnapshot,
    item: Value,
    event: Value,
    ledger: String,
    response: Value,
}

/// Every check one ledger's revalidation needs, appending nothing. Also
/// used to decide which ledgers one refusal route can cover.
pub(crate) fn check(
    loaded: &Loaded,
    previous: &Loaded,
    actor: &str,
    ledger: &str,
) -> Result<bool, String> {
    Ok(matches!(
        prepare(loaded, previous, actor, ledger, "grouping check")?,
        Prepared::Ready(_)
    ))
}

fn prepare(
    loaded: &Loaded,
    previous: &Loaded,
    actor: &str,
    ledger: &str,
    reason: &str,
) -> Result<Prepared, String> {
    let snapshot = read_ledger(loaded, ledger, false)?;
    // A rerun of an interrupted route skips a ledger that is already current.
    if super::state::history_drift(loaded, &snapshot).is_ok()
        && super::validate_source_current(loaded, &snapshot).is_ok()
    {
        return Ok(Prepared::Current(
            json!({"effect":"no-change","status":"already_current","ledger":ledger,
                "configSha256":hash::text(&loaded.source)}),
        ));
    }
    history_against(previous, &snapshot)?;
    super::validate_source_current(previous, &snapshot)?;
    let item = snapshot
        .items
        .values()
        .next()
        .ok_or("lesson ledger is empty")?;
    let contextual_migration =
        !super::contextual::enabled(previous) && super::contextual::enabled(loaded);
    if item["scope"] != super::lessons::SCOPE
        && !(contextual_migration
            && crate::project::specialists::is_scope(item["scope"].as_str().unwrap_or("")))
    {
        return Err("revalidation is limited to project lessons".into());
    }
    let source = crate::project::path::secure_bytes(
        &loaded.product_root,
        item["source"]["path"]
            .as_str()
            .ok_or("lesson source missing")?,
        "lesson source",
    )?;
    let current_source = hash::bytes(&source);
    if current_source != item["source"]["sha256"] {
        return Err(crate::drift::refusal(
            crate::drift::Class::LessonSource,
            item["source"]["sha256"].as_str().unwrap_or_default(),
            Some(&current_source),
            None,
            crate::drift::Route::Inspect {
                ledger,
                config: &loaded.path,
            },
        ));
    }
    let lesson_id = if item["scope"] == super::lessons::SCOPE {
        super::lessons::parse(loaded, &source)?.id
    } else {
        crate::project::specialists::input_form(
            loaded,
            item["scope"].as_str().ok_or("invalid scope")?,
            item["source"]["path"].as_str(),
        )?;
        serde_json::from_slice::<Value>(&source).map_err(|e| e.to_string())?["id"]
            .as_str()
            .ok_or("invalid specialist identity")?
            .to_owned()
    };
    let differences = if contextual_migration {
        let actors = snapshot
            .events
            .iter()
            .filter_map(|event| event["actor"].as_str())
            .collect::<Vec<_>>();
        compatible_contextual(
            previous,
            loaded,
            actor,
            item["scope"].as_str().ok_or("invalid scope")?,
            &actors,
        )?
    } else {
        compatible(previous, loaded, actor, ledger)?
    };
    let old_hash = hash::text(&previous.source);
    let new_hash = hash::text(&loaded.source);
    let proof = json!({"version":1, "previousConfigSha256":old_hash,
        "projectIdentity":crate::host::assignment_context::project_identity(loaded)?,
        "owner":actor,"priorLedgerHeadSha256":snapshot.last_event_sha256,
        "differences":differences,"reason":reason});
    let response = json!({"effect":"no-change","status":"preview","ledger":ledger,
        "itemId":item["itemId"],"lessonId":lesson_id,"configSha256":new_hash,"revalidation":proof,
        "nextAction":{"command":lesson_route(loaded, actor, ledger, "inspect").full_argv()?,"readOnly":true},
        "meaning":"explicit reviewed project-fact lineage; no Work, task grant, check, review approval or acceptance is transferred"});
    let item = item.clone();
    // Every check the event itself needs runs here, before any ledger of a
    // multi-ledger run is appended to (F4a review D1).
    let event = revalidation_event(loaded, actor, &snapshot, &item, proof, &new_hash)?;
    crate::project::git_preflight::refuse_tracked_targets(
        &loaded.state_root,
        &[snapshot.path.as_path()],
    )?;
    Ok(Prepared::Ready(Box::new(Ready {
        snapshot,
        item,
        event,
        ledger: ledger.to_owned(),
        response,
    })))
}

/// The ordinary revalidation event for one ledger, validated against that
/// ledger as it stands.
fn revalidation_event(
    loaded: &Loaded,
    actor: &str,
    snapshot: &LedgerSnapshot,
    item: &Value,
    proof: Value,
    new_hash: &str,
) -> Result<Value, String> {
    let last = snapshot
        .events
        .last()
        .ok_or("lesson ledger has no events")?;
    let agent = loaded.agent(actor).ok_or("configured Lead missing")?;
    let profile = project_file(&loaded.control_root, &agent.profile, "actor profile")?;
    let mut event = json!({"version":2,"kind":"memory","producer":crate::producer::evidence(),
        "action":"revalidate","itemId":item["itemId"],"scope":item["scope"],"actor":actor,
        "source":item["source"],"configSha256":new_hash,"actorProfile":{
            "path":relative_project_path(&loaded.control_root,&profile)?,
            "sha256":hash::text(&stable_text(&profile,"actor profile")?)},
        "previousEventSha256":snapshot.last_event_sha256,"timestamp":super::now(),"revalidation":proof});
    if let Some(expiry) = last.get("expiresAt") {
        event["expiresAt"] = expiry.clone();
    }
    event["eventSha256"] = json!(hash::value(&event));
    super::state::validate_event(
        &event,
        &loaded.product_root,
        &loaded.control_root,
        snapshot.events.len() + 1,
        snapshot.last_event_sha256.as_deref(),
        snapshot.last_timestamp.as_deref(),
        &snapshot.items,
    )?;
    Ok(event)
}

/// The lesson lifecycle route `memory ACTION [ACTOR] LEDGER --config CONFIG`;
/// `inspect` names no actor.
fn lesson_route(loaded: &Loaded, actor: &str, ledger: &str, action: &str) -> crate::emitted::Route {
    crate::emitted::Route::new(["memory", action])
        .words((action != "inspect").then_some(actor))
        .words([ledger])
        .config(&loaded.path)
}

impl Ready {
    fn apply(self, loaded: &Loaded, previous: &Loaded, actor: &str) -> Result<Value, String> {
        let Ready {
            snapshot,
            item,
            event,
            ledger,
            mut response,
        } = self;
        super::ensure_config_current(previous)?;
        super::ensure_config_current(loaded)?;
        history_against(previous, &snapshot)?;
        super::validate_source_current(loaded, &snapshot)?;
        let next = if item["state"] == "accepted" {
            "revoke"
        } else {
            "inspect"
        };
        response["nextAction"] = json!({"command":lesson_route(loaded, actor, &ledger, next).full_argv()?,
            "readOnly":next == "inspect",
            "meaning":"retire this immutable lesson before correcting it; current guards still determine delivery"});
        append_event(&snapshot, &event)?;
        response["effect"] = json!("recorded");
        response["status"] = json!("revalidated");
        response["event"] = event;
        Ok(response)
    }
}

fn compatible(
    previous: &Loaded,
    current: &Loaded,
    actor: &str,
    ledger: &str,
) -> Result<Vec<Value>, String> {
    let refuse = |class| {
        crate::drift::refusal(
            class,
            &hash::text(&previous.source),
            Some(&hash::text(&current.source)),
            None,
            crate::drift::Route::Inspect {
                ledger,
                config: &current.path,
            },
        )
    };
    if previous.product_root != current.product_root
        || previous.control_root != current.control_root
        || previous.state_root != current.state_root
        || previous.project_id != current.project_id
        || previous.lead() != Some(actor)
        || previous.lead() != current.lead()
    {
        return Err(refuse(crate::drift::Class::LessonOwnership));
    }
    // Deliberately narrow compatibility: additive recipient read rights, the
    // agent fields that are not lesson authority (`lesson_authority`) and
    // schema URL metadata. Write/runtime/ownership/retention stay exact.
    if super::lesson_authority::projection(&previous.source, true)?
        != super::lesson_authority::projection(&current.source, true)?
    {
        return Err(refuse(crate::drift::Class::LessonAuthority));
    }
    let lead = current.agent(actor).ok_or("Lead missing")?;
    for right in [
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryRevoke",
    ] {
        if !super::authorized(&lead.boundary_value(), right, super::lessons::SCOPE) {
            return Err(format!("Lead lacks required lesson right {right}"));
        }
    }
    let mut differences = Vec::new();
    for (name, old) in &previous.agents {
        let new = current.agent(name).ok_or("recipient removed")?;
        if old
            .memory_read
            .iter()
            .any(|right| !new.memory_read.contains(right))
        {
            return Err("authority-reducing recipient read change is incompatible".into());
        }
        if old.memory_read != new.memory_read {
            differences.push(json!({"agent":name,"right":"memoryRead","before":old.memory_read,"after":new.memory_read}));
        }
    }
    differences.extend(super::lesson_authority::differences(
        &previous.config,
        &current.config,
    ));
    Ok(differences)
}

// Only the explicit contextual opt-in and the agent fields that are not lesson
// authority change. Rights, roots and all other policy remain exact.
fn compatible_contextual(
    previous: &Loaded,
    current: &Loaded,
    actor: &str,
    scope: &str,
    ledger_actors: &[&str],
) -> Result<Vec<Value>, String> {
    if previous.product_root != current.product_root
        || previous.control_root != current.control_root
        || previous.state_root != current.state_root
        || previous.project_id != current.project_id
        || previous.lead() != Some(actor)
        || current.lead() != Some(actor)
    {
        return Err("contextual migration cannot change project roots or ownership".into());
    }
    let before = super::lesson_authority::projection(&previous.source, false)?;
    let mut after = super::lesson_authority::projection(&current.source, false)?;
    let choice = after["memory"]
        .as_object_mut()
        .ok_or("memory policy missing")?
        .remove("contextual")
        .ok_or("contextual opt-in missing")?;
    if before != after || !super::contextual::valid_policy(&choice) {
        return Err("contextual migration permits only the explicit opt-in; rights and other configuration must remain unchanged".into());
    }
    // A specialist ledger's author and reviewer profiles are qualification
    // evidence on its whole history; they stay exact, refused before any append.
    if crate::project::specialists::is_scope(scope) {
        for name in ledger_actors {
            let profile = |loaded: &Loaded| loaded.config["agents"][*name]["profile"].clone();
            if profile(previous) != profile(current) {
                return Err(format!(
                    "contextual migration keeps the profile of specialist ledger actor '{name}' exact; it is qualification evidence for this ledger"
                ));
            }
        }
    }
    let lead = current.agent(actor).ok_or("Lead unavailable")?;
    if !lead.memory_promote.iter().any(|s| s == scope)
        || !lead.memory_revoke.iter().any(|s| s == scope)
    {
        return Err("contextual migration requires the same configured Lead's existing promotion and revocation rights".into());
    }
    let mut differences = vec![json!({"field":"memory.contextual","before":null,"after":choice})];
    differences.extend(super::lesson_authority::differences(
        &previous.config,
        &current.config,
    ));
    Ok(differences)
}

pub(crate) fn validate_evidence(event: &Value, current: &Value) -> Result<(), String> {
    let proof = &event["revalidation"];
    let valid = event["version"] == 2
        && (event["scope"] == super::lessons::SCOPE
            || (crate::project::specialists::is_scope(event["scope"].as_str().unwrap_or(""))
                && proof["differences"].as_array().is_some_and(|d| {
                    !d.is_empty()
                        && d[0]["field"] == "memory.contextual"
                        && d[0]["before"].is_null()
                        && super::contextual::valid_policy(&d[0]["after"])
                        && d[1..]
                            .iter()
                            .all(super::lesson_authority::is_recorded_difference)
                })))
        && proof.as_object().is_some_and(|object| object.len() == 7)
        && proof["version"] == 1
        && proof["previousConfigSha256"] == current["configSha256"]
        && proof["priorLedgerHeadSha256"] == event["previousEventSha256"]
        && proof["owner"] == event["actor"]
        && proof["projectIdentity"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        && proof["differences"].is_array()
        && proof["reason"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.len() <= 1024);
    if valid {
        Ok(())
    } else {
        Err("invalid reviewed lesson configuration lineage".into())
    }
}

pub(crate) fn next_action(
    loaded: &Loaded,
    actor: &str,
    ledger: &str,
    state: &str,
) -> Result<Value, String> {
    let action = match state {
        "proposed" => "review",
        "reviewed" => "promote",
        _ => "inspect",
    };
    Ok(
        json!({"command":lesson_route(loaded, actor, ledger, action).full_argv()?,"readOnly":action == "inspect", "state":state,
        "meaning":"current lesson lifecycle only; correction requires a new immutable source and ledger"}),
    )
}
