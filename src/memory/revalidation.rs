//! Explicit Lead review of compatible configuration lineage. Work is never imported.
use super::ledger::*;
use crate::{
    config::{self, Loaded},
    evidence::hash,
};
use serde_json::{json, Value};

pub(crate) fn run(
    loaded: &Loaded,
    actor: &str,
    ledger: &str,
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
    let _lock = super::lessons::mutation_lock(loaded)?;
    // This project's kept stamped copy sits beside the ledgers; its bytes
    // are read as they apply at this configuration's location. Any other
    // file is loaded where it is, so a foreign project's roots still refuse.
    let previous = if super::stamped_config::is_kept_copy(loaded, from) {
        config::load_bytes_at(from, &loaded.path)?
    } else {
        config::load(Some(from))?
    };
    let snapshot = read_ledger(loaded, ledger, false)?;
    super::validate_history_current(&previous, &snapshot)?;
    super::validate_source_current(&previous, &snapshot)?;
    let item = snapshot
        .items
        .values()
        .next()
        .ok_or("lesson ledger is empty")?;
    let contextual_migration =
        !super::contextual::enabled(&previous) && super::contextual::enabled(loaded);
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
        compatible_contextual(
            &previous,
            loaded,
            actor,
            item["scope"].as_str().ok_or("invalid scope")?,
        )?
    } else {
        compatible(&previous, loaded, actor, ledger)?
    };
    let old_hash = hash::text(&previous.source);
    let new_hash = hash::text(&loaded.source);
    if old_hash == new_hash {
        return Err("lesson configuration is unchanged; no revalidation is needed".into());
    }
    let proof = json!({"version":1, "previousConfigSha256":old_hash,
        "projectIdentity":crate::host::assignment_context::project_identity(loaded)?,
        "owner":actor,"priorLedgerHeadSha256":snapshot.last_event_sha256,
        "differences":differences,"reason":reason});
    let argv = |action: &str| -> Result<Value, String> {
        let mut argv = vec![
            crate::emitted::executable()?,
            "memory".into(),
            action.into(),
        ];
        if action != "inspect" {
            argv.push(actor.into());
        }
        argv.extend([
            ledger.into(),
            "--config".into(),
            loaded
                .path
                .to_str()
                .ok_or("configuration path is not UTF-8")?
                .into(),
        ]);
        Ok(json!(argv))
    };
    let mut response = json!({"effect":"no-change","status":"preview","itemId":item["itemId"],
        "lessonId":lesson_id,"configSha256":new_hash,"revalidation":proof,
        "nextAction":{"command":argv("inspect")?,"readOnly":true},
        "meaning":"explicit reviewed project-fact lineage; no Work, task grant, check, review approval or acceptance is transferred"});
    if !apply {
        return Ok(response);
    }
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
    super::ensure_config_current(&previous)?;
    super::ensure_config_current(loaded)?;
    super::validate_history_current(&previous, &snapshot)?;
    super::validate_source_current(loaded, &snapshot)?;
    crate::project::git_preflight::refuse_tracked_targets(
        &loaded.state_root,
        &[snapshot.path.as_path()],
    )?;
    let next = if item["state"] == "accepted" {
        "revoke"
    } else {
        "inspect"
    };
    response["nextAction"] = json!({"command":argv(next)?,"readOnly":next == "inspect",
        "meaning":"retire this immutable lesson before correcting it; current guards still determine delivery"});
    append_event(&snapshot, &event)?;
    response["effect"] = json!("recorded");
    response["status"] = json!("revalidated");
    response["event"] = event;
    Ok(response)
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
    // Deliberately narrow compatibility: additive recipient read rights,
    // read-only phase guidance (it grants nothing) and schema URL metadata.
    // Write/runtime/ownership/retention semantics stay exact.
    let strip_reads = |source: &str| -> Result<Value, String> {
        let mut value: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
        value
            .as_object_mut()
            .ok_or("configuration is not an object")?
            .remove("$schema");
        for agent in value
            .get_mut("agents")
            .and_then(Value::as_object_mut)
            .ok_or("agents missing")?
            .values_mut()
        {
            let agent = agent.as_object_mut().ok_or("agent is not an object")?;
            agent.remove("memoryRead");
            agent.remove("phaseGuidance");
        }
        Ok(value)
    };
    if strip_reads(&previous.source)? != strip_reads(&current.source)? {
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
        let guidance = |loaded: &Loaded| loaded.config["agents"][name]["phaseGuidance"].clone();
        if guidance(previous) != guidance(current) {
            differences.push(json!({"agent":name,"field":"phaseGuidance",
                "before":guidance(previous),"after":guidance(current)}));
        }
    }
    Ok(differences)
}

// Only the explicit contextual opt-in changes. Rights, roots, profiles and all other policy remain exact.
fn compatible_contextual(
    previous: &Loaded,
    current: &Loaded,
    actor: &str,
    scope: &str,
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
    let mut before: Value = serde_json::from_str(&previous.source).map_err(|e| e.to_string())?;
    let mut after: Value = serde_json::from_str(&current.source).map_err(|e| e.to_string())?;
    before
        .as_object_mut()
        .ok_or("invalid prior config")?
        .remove("$schema");
    after
        .as_object_mut()
        .ok_or("invalid current config")?
        .remove("$schema");
    let choice = after["memory"]
        .as_object_mut()
        .ok_or("memory policy missing")?
        .remove("contextual")
        .ok_or("contextual opt-in missing")?;
    if before != after || !super::contextual::valid_policy(&choice) {
        return Err("contextual migration permits only the explicit opt-in; rights and other configuration must remain unchanged".into());
    }
    let lead = current.agent(actor).ok_or("Lead unavailable")?;
    if !lead.memory_promote.iter().any(|s| s == scope)
        || !lead.memory_revoke.iter().any(|s| s == scope)
    {
        return Err("contextual migration requires the same configured Lead's existing promotion and revocation rights".into());
    }
    Ok(vec![
        json!({"field":"memory.contextual","before":null,"after":choice}),
    ])
}

pub(crate) fn validate_evidence(event: &Value, current: &Value) -> Result<(), String> {
    let proof = &event["revalidation"];
    let valid = event["version"] == 2
        && (event["scope"] == super::lessons::SCOPE
            || (crate::project::specialists::is_scope(event["scope"].as_str().unwrap_or(""))
                && proof["differences"].as_array().is_some_and(|d| {
                    d.len() == 1
                        && d[0]["field"] == "memory.contextual"
                        && d[0]["before"].is_null()
                        && super::contextual::valid_policy(&d[0]["after"])
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
    let mut argv = vec![
        crate::emitted::executable()?,
        "memory".into(),
        action.into(),
    ];
    if action != "inspect" {
        argv.push(actor.to_owned());
    }
    argv.extend([
        ledger.to_owned(),
        "--config".into(),
        loaded
            .path
            .to_str()
            .ok_or("configuration path is not UTF-8")?
            .to_owned(),
    ]);
    Ok(
        json!({"command":argv,"readOnly":action == "inspect", "state":state,
        "meaning":"current lesson lifecycle only; correction requires a new immutable source and ledger"}),
    )
}
