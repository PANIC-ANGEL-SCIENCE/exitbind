// Projects current required facts for the selected native actor.
// Refuses unclassified non-empty fields; see `src/host/runtime.rs`.
//! Current, required facts for the selected native actor.
//!
//! The canonical Work sections and their fingerprint remain unchanged. This
//! presentation selects actor facts from their existing typed owners and
//! leaves Lead-only ledger and navigation machinery at the exact detail route.

use crate::config::Loaded;
use crate::work::action_forms::GrantOffer;
use serde_json::{json, Map, Value};

mod evidence;
mod status;
#[cfg(test)]
mod tests;

pub(crate) fn projection(
    loaded: &Loaded,
    work: &str,
    binding: &str,
    sections: &Value,
    check_command: &Value,
    grant_offer: Option<&GrantOffer>,
) -> Result<Value, String> {
    let reference = format!("{}{binding}", crate::work::readable::PREFIX);
    let detail = crate::emitted::Route::new(["work", "expand", work, &reference, "--json"])
        .config(&loaded.path)
        .full_argv()?;
    project_sections(work, binding, sections, &detail, check_command, grant_offer)
}

fn nonempty(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
        _ => true,
    }
}

fn classified(value: &Value, known: &[&str], label: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} is unavailable"))?;
    if let Some(key) = object
        .iter()
        .find_map(|(key, value)| (!known.contains(&key.as_str()) && nonempty(value)).then_some(key))
    {
        return Err(format!("{label} contains unclassified field '{key}'"));
    }
    Ok(())
}

fn copy_if_present(target: &mut Map<String, Value>, name: &str, source: &Value) {
    if nonempty(source) {
        target.insert(name.into(), source.clone());
    }
}

fn project_sections(
    work: &str,
    binding: &str,
    sections: &Value,
    detail: &[String],
    check_command: &Value,
    grant_offer: Option<&GrantOffer>,
) -> Result<Value, String> {
    if !check_command.is_null() && !check_command.is_string() {
        return Err("native frozen check command is unavailable".into());
    }
    classified(
        sections,
        &["assignment", "evidence", "tasks", "currentAction"],
        "native sections",
    )?;
    let section = &sections["assignment"];
    classified(
        section,
        &[
            "assignment",
            "available",
            "checkPreparation",
            "memoryQuery",
            "outcomes",
            "projection",
            "residual",
        ],
        "native assignment",
    )?;
    if section["available"] != true {
        return Err("current native assignment is unavailable".into());
    }
    classified(
        &section["projection"],
        &["equalAliases", "recoveryAliases"],
        "native projection metadata",
    )?;
    if section["projection"]["recoveryAliases"] != "equal recovery goal/scope/subject/current/missing/loop/next refer to context goal/scope/subject/evidence/obligations/loop/next" {
        return Err("native projection aliases are unclassified".into());
    }
    if let Some(aliases) = section["projection"]["equalAliases"].as_array() {
        for alias in aliases {
            classified(alias, &["removed", "source"], "native projection alias")?;
            if !matches!(
                (alias["removed"].as_str(), alias["source"].as_str()),
                (Some("residual.context"), Some("assignment.context"))
                    | (Some("residual.goal"), Some("assignment.context.goal"))
                    | (
                        Some("residual.currentSubject"),
                        Some("assignment.context.subject")
                    )
                    | (
                        Some("residual.remaining"),
                        Some("assignment.context.obligations")
                    )
            ) {
                return Err("native projection alias is unclassified".into());
            }
        }
    } else {
        return Err("native projection aliases are unavailable".into());
    }
    let packet = &section["assignment"];
    classified(
        packet,
        &[
            "agent",
            "attempt",
            "basisSha256",
            "checkEvidence",
            "context",
            "declaredBoundary",
            "displayName",
            "frozenCrossContext",
            "frozenMemoryRead",
            "goal",
            "guidanceConfigSha256",
            "guidanceContinuityProtocol",
            "nativeTaskName",
            "profile",
            "profileSha256",
            "purpose",
            "reviewDecisionSha256",
            "role",
            "runtime",
            "stage",
            "upstreamArtifacts",
            "upstreamArtifactsImmutable",
            "preservationAssignment",
            "reviewPolicy",
            "reviewPurpose",
            "substitution",
            "quarantineVerification",
            "leadRepair",
        ],
        "native assignment packet",
    )?;
    if nonempty(&packet["leadRepair"]) {
        classified(
            &packet["leadRepair"],
            &[
                "authority",
                "decision",
                "reason",
                "repairBoundary",
                "decisiveRegression",
                "findingSha256s",
                "dispositionSha256",
                "basisSha256",
            ],
            "native Lead repair",
        )?;
    }
    if packet["checkEvidence"] != sections["evidence"]["checkEvidence"] {
        return Err("native assignment check evidence conflicts with canonical evidence".into());
    }
    classified(
        &packet["profile"],
        &["path", "sha256"],
        "native profile identity",
    )?;
    if packet["profile"]["sha256"] != packet["profileSha256"] {
        return Err("native profile identities differ".into());
    }
    classified(
        &packet["runtime"],
        &["host", "model", "reasoningEffort", "fallback"],
        "native runtime identity",
    )?;
    if packet["runtime"]["fallback"].is_object() {
        classified(
            &packet["runtime"]["fallback"],
            &["host", "model", "reasoningEffort"],
            "native fallback identity",
        )?;
    }
    if packet["substitution"].is_object() {
        classified(
            &packet["substitution"],
            &[
                "kind",
                "reason",
                "primaryRuntime",
                "runtime",
                "identitySource",
            ],
            "native substitution",
        )?;
    }
    classified(
        &packet["declaredBoundary"],
        &[
            "observe",
            "write",
            "commands",
            "skills",
            "memoryRead",
            "memoryWrite",
            "memoryReview",
            "memoryPromote",
            "memoryReject",
            "memoryRevoke",
            "memoryExpire",
            "memoryForget",
            "retention",
            "crossContext",
        ],
        "native declared boundary",
    )?;
    let action = &sections["currentAction"];
    classified(
        action,
        &[
            "binding",
            "returnChoices",
            "beforeEditing",
            "currentGrant",
            "nextRequest",
            "leadChoiceRequired",
        ],
        "native current action",
    )?;
    if action["beforeEditing"].is_object() {
        classified(
            &action["beforeEditing"],
            &[
                "command",
                "fill",
                "inputConstraints",
                "meaning",
                "placeholders",
                "required",
            ],
            "native before-edit form",
        )?;
        let form = &action["beforeEditing"];
        if form["command"].is_object() {
            classified(
                &form["command"],
                &["argv", "sameConfigRequired", "sameExecutableRequired"],
                "native permit command",
            )?;
        }
        if form["fill"].is_object() {
            classified(
                &form["fill"],
                &["artifactOption", "choice", "options"],
                "native permit fill",
            )?;
        }
        if let Some(constraints) = form["inputConstraints"].as_object() {
            for constraint in constraints.values() {
                classified(
                    constraint,
                    &[
                        "type",
                        "unit",
                        "minimumBytes",
                        "maximumBytes",
                        "nonBlank",
                        "forbiddenCharacters",
                        "example",
                        "allowedValues",
                    ],
                    "native permit input constraint",
                )?;
            }
        } else if nonempty(&form["inputConstraints"]) {
            return Err("native permit input constraints are unavailable".into());
        }
    }
    if action["currentGrant"].is_object() {
        classified(
            &action["currentGrant"],
            &["governorEventSha256", "operation", "state", "use"],
            "native current grant",
        )?;
    }
    let context = &packet["context"];
    classified(
        context,
        &[
            "digest",
            "evidence",
            "expansions",
            "goal",
            "loop",
            "next",
            "obligations",
            "recovery",
            "role",
            "run",
            "scope",
            "subject",
            "version",
        ],
        "native assignment context",
    )?;
    let next = &context["next"];
    classified(
        next,
        &[
            "action",
            "agent",
            "assignment",
            "check",
            "freshness",
            "outcomes",
            "owner",
            "role",
            "source",
        ],
        "native next action",
    )?;
    let scope = &context["scope"];
    classified(
        scope,
        &[
            "attempt",
            "basisSha256",
            "configSha256",
            "currentStage",
            "freshness",
            "owner",
            "planSha256",
            "reviewDecisionSha256",
            "source",
        ],
        "native scope",
    )?;
    let subject = &context["subject"];
    classified(
        subject,
        &[
            "attempt",
            "configSha256",
            "goalSha256",
            "planSha256",
            "previousSubjectSha256",
            "runId",
            "sha256",
            "transitionSha256",
            "version",
            "workerArtifactSha256",
        ],
        "native subject",
    )?;
    classified(&context["run"], &["id", "workflow"], "native run identity")?;
    if !packet["agent"].is_string()
        || !packet["role"].is_string()
        || !packet["nativeTaskName"].is_string()
        || !packet["stage"].is_u64()
        || !packet["attempt"].is_u64()
        || !packet["goal"].is_string()
        || !packet["purpose"].is_string()
        || !packet["declaredBoundary"].is_object()
        || !next["assignment"].is_string()
        || next["action"] != "spawn"
        || next["agent"] != packet["agent"]
        || next["role"] != packet["role"]
        || next["freshness"] != "current"
        || next["source"] != "run_state"
        || context["role"] != packet["role"]
        || scope["freshness"] != "current"
        || scope["attempt"] != packet["attempt"]
        || scope["currentStage"] != packet["stage"]
        || scope["basisSha256"] != packet["basisSha256"]
        || !subject["sha256"].is_string()
        || sections["currentAction"]["binding"] != binding
    {
        return Err("native actor identity, scope or current binding is inconsistent".into());
    }
    let residual = &section["residual"];
    classified(
        residual,
        &[
            "version",
            "work",
            "workflow",
            "goal",
            "historical",
            "snapshot",
            "currentSubject",
            "basis",
            "reviewPolicy",
            "alreadyEstablished",
            "stillValid",
            "remaining",
            "next",
            "humanHelp",
            "doNotRepeat",
            "invalidation",
            "context",
        ],
        "native residual",
    )?;
    if residual["work"] != work || residual["historical"] != false || residual["next"] != "spawn" {
        return Err("native actor Work or current status is inconsistent".into());
    }
    if residual["snapshot"].is_object() {
        classified(
            &residual["snapshot"],
            &["eventCount", "headEventSha256", "inputsSha256"],
            "native residual snapshot",
        )?;
    } else if nonempty(&residual["snapshot"]) {
        return Err("native residual snapshot is unavailable".into());
    }
    if residual["currentSubject"].is_object() {
        classified(
            &residual["currentSubject"],
            &[
                "attempt",
                "configSha256",
                "goalSha256",
                "planSha256",
                "previousSubjectSha256",
                "runId",
                "sha256",
                "transitionSha256",
                "version",
                "workerArtifactSha256",
            ],
            "native residual subject",
        )?;
    } else if nonempty(&residual["currentSubject"]) {
        return Err("native residual subject is unavailable".into());
    }
    classified(
        &section["checkPreparation"],
        &[
            "state",
            "program",
            "path",
            "detail",
            "workingDirectory",
            "input",
        ],
        "native check preparation",
    )?;
    if section["checkPreparation"]["input"].is_object() {
        classified(
            &section["checkPreparation"]["input"],
            &["state", "argument", "path", "meaning", "reason"],
            "native check input",
        )?;
    } else if nonempty(&section["checkPreparation"]["input"]) {
        return Err("native check input is unavailable".into());
    }
    let mut actor = Map::new();
    actor.insert("work".into(), json!(work));
    actor.insert("assignment".into(), next["assignment"].clone());
    for name in [
        "role",
        "agent",
        "nativeTaskName",
        "stage",
        "attempt",
        "goal",
        "purpose",
        "runtime",
    ] {
        actor.insert(name.into(), packet[name].clone());
    }
    actor.insert("subject".into(), subject["sha256"].clone());
    actor.insert("boundary".into(), packet["declaredBoundary"].clone());
    actor.insert("currentAction".into(), sections["currentAction"].clone());
    actor.insert(
        "checkPreparation".into(),
        section["checkPreparation"].clone(),
    );
    actor.insert("checkCommand".into(), check_command.clone());
    actor.insert("ownerActionDetail".into(), json!(detail));
    if !residual["goal"].is_null() && residual["goal"] != packet["goal"] {
        actor.insert("residualGoal".into(), residual["goal"].clone());
    }
    if !residual["currentSubject"].is_null() && residual["currentSubject"] != context["subject"] {
        actor.insert("residualSubject".into(), residual["currentSubject"].clone());
    }
    if !residual["context"].is_null() && residual["context"] != *context {
        return Err("native residual context conflicts with current actor facts".into());
    }
    if context["run"]["workflow"] != residual["workflow"] {
        actor.insert(
            "workflowStatus".into(),
            json!({"run":context["run"]["workflow"],"residual":residual["workflow"]}),
        );
    }
    if scope["source"] != "run_ledger" || scope["owner"] != "lead" {
        actor.insert(
            "scopeStatus".into(),
            json!({"source":scope["source"],"owner":scope["owner"]}),
        );
    }
    if next["owner"] != packet["role"] {
        actor.insert("nextOwner".into(), next["owner"].clone());
    }
    copy_if_present(&mut actor, "nextOutcomes", &next["outcomes"]);
    copy_if_present(&mut actor, "nextCheck", &next["check"]);
    if !context["goal"].is_null() && context["goal"] != packet["goal"] {
        actor.insert("contextGoal".into(), context["goal"].clone());
    }
    for (name, boundary_name) in [
        ("frozenMemoryRead", "memoryRead"),
        ("frozenCrossContext", "crossContext"),
    ] {
        if !packet[name].is_null() && packet[name] != packet["declaredBoundary"][boundary_name] {
            actor.insert(name.into(), packet[name].clone());
        }
    }
    let basis = &residual["basis"];
    if packet["basisSha256"].is_string()
        && (!basis.is_object() || basis["sha256"] != packet["basisSha256"])
    {
        return Err("complete current actor basis is unavailable".into());
    }
    actor.insert("basis".into(), basis.clone());
    for name in [
        "reviewPolicy",
        "reviewPurpose",
        "substitution",
        "quarantineVerification",
        "leadRepair",
    ] {
        copy_if_present(&mut actor, name, &packet[name]);
    }
    if !packet["upstreamArtifactsImmutable"].is_null()
        && packet["upstreamArtifactsImmutable"] != true
    {
        actor.insert(
            "upstreamArtifactsImmutable".into(),
            packet["upstreamArtifactsImmutable"].clone(),
        );
    }
    if nonempty(&residual["reviewPolicy"]) {
        if actor
            .get("reviewPolicy")
            .is_some_and(|policy| policy != &residual["reviewPolicy"])
        {
            actor.insert(
                "additionalReviewPolicy".into(),
                residual["reviewPolicy"].clone(),
            );
        } else {
            actor.insert("reviewPolicy".into(), residual["reviewPolicy"].clone());
        }
    }
    copy_if_present(&mut actor, "memoryQuery", &section["memoryQuery"]);
    copy_if_present(&mut actor, "outcomes", &section["outcomes"]);
    preservation(&mut actor, packet, residual);
    actor.insert(
        "evidence".into(),
        evidence::actor_evidence(&sections["evidence"])?,
    );
    evidence::source_context(&mut actor, context, &sections["evidence"], work)?;
    evidence::upstream_reasons(
        &mut actor,
        &packet["upstreamArtifacts"],
        &sections["evidence"],
    )?;
    status::obligations(&mut actor, section, context, residual, work)?;
    status::governor(
        &mut actor,
        packet,
        &sections["currentAction"],
        grant_offer,
        &sections["evidence"],
    )?;
    let recipient = super::context::recipient_assignment(sections, detail);
    status::statuses(&mut actor, &recipient["tasks"], work, &packet["goal"])?;
    Ok(Value::Object(actor))
}

fn preservation(actor: &mut Map<String, Value>, packet: &Value, residual: &Value) {
    let first = &packet["preservationAssignment"];
    let second = &residual["humanHelp"]["preservationAssignment"];
    if !nonempty(first) {
        copy_if_present(actor, "preservationAssignment", second);
    } else {
        copy_if_present(actor, "preservationAssignment", first);
    }
    if nonempty(first) && nonempty(second) && second != first {
        actor.insert("additionalPreservationAssignment".into(), second.clone());
    }
}

fn known_keys(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| keys.contains(&key.as_str())))
}
