//! Current obligations and material statuses for the selected actor.
use super::{classified, copy_if_present, known_keys, nonempty};
use serde_json::{json, Map, Value};

pub(super) fn obligations(
    actor: &mut Map<String, Value>,
    section: &Value,
    context: &Value,
    residual: &Value,
    work: &str,
) -> Result<(), String> {
    let mut remaining = residual["remaining"].clone();
    if remaining.is_null() {
        let aliases = section["projection"]["equalAliases"]
            .as_array()
            .ok_or("native obligations are unavailable")?;
        if aliases.iter().any(|item| {
            item["removed"] == "residual.remaining"
                && item["source"] == "assignment.context.obligations"
        }) {
            remaining = context["obligations"].clone();
        } else {
            return Err("native obligations are unavailable".into());
        }
    }
    actor.insert(
        "obligations".into(),
        json!({"assigned":context["obligations"],"remaining":remaining}),
    );
    for name in ["doNotRepeat", "stillValid"] {
        copy_if_present(actor, name, &residual[name]);
    }
    let established = residual["alreadyEstablished"]
        .as_array()
        .ok_or("native established facts are unavailable")?;
    let extra = established
        .iter()
        .filter(|item| {
            !matches!(
                item["fact"].as_str(),
                Some("work_identity_recorded" | "scope_recorded")
            ) || item["status"] != "completed"
                || !known_keys(item, &["fact", "status"])
        })
        .cloned()
        .collect::<Vec<_>>();
    if !extra.is_empty() {
        actor.insert("alreadyEstablished".into(), json!(extra));
    }
    let help = &residual["humanHelp"];
    classified(
        help,
        &[
            "nextAction",
            "ownerDecision",
            "whatHappened",
            "whatRemains",
            "preservationAssignment",
        ],
        "native human help",
    )?;
    let next = &help["nextAction"];
    classified(next, &["actor", "command", "summary"], "native help action")?;
    if next["command"].is_object() {
        classified(
            &next["command"],
            &["args", "argv", "program"],
            "native help command",
        )?;
    }
    let default_help = help["ownerDecision"] == "not_required"
        && help["whatHappened"] == "A worker assignment is pending."
        && help["whatRemains"] == remaining
        && help["nextAction"]["actor"] == "worker"
        && help["nextAction"]["summary"] == "run the pending worker assignment"
        && help["nextAction"]["command"]["program"] == "exitbind"
        && help["nextAction"]["command"]["args"]
            .as_array()
            .is_some_and(|args| {
                matches!(args.as_slice(), [family, action, target]
                    if family.as_str() == Some("work")
                        && action.as_str() == Some("next")
                        && target.as_str() == Some(work))
            });
    if !default_help {
        actor.insert("currentStatus".into(), json!({"whatHappened":help["whatHappened"],"whatRemains":help["whatRemains"],"ownerDecision":help["ownerDecision"],"actor":next["actor"],"summary":next["summary"]}));
    }
    let invalidation = &residual["invalidation"];
    classified(
        invalidation,
        &[
            "rule",
            "source",
            "owner",
            "sessionRestartInvalidates",
            "subjectChangeInvalidates",
            "basisChangeInvalidates",
            "checkerChangeInvalidates",
            "testedInputChangeInvalidates",
            "configChangeInvalidates",
            "inputCoverage",
            "checkerSha256",
            "configSha256",
        ],
        "native invalidation",
    )?;
    if invalidation["rule"] != "reuse only while the run is active and its ledger head, exact subject, and tested inputs remain current"
        || invalidation["source"] != "run_ledger" || invalidation["owner"] != "lead"
        || invalidation["inputCoverage"] != "product-root-files-v1"
        || invalidation["configSha256"] != context["scope"]["configSha256"]
        || invalidation["sessionRestartInvalidates"] != false
        || ["subjectChangeInvalidates", "basisChangeInvalidates", "checkerChangeInvalidates", "testedInputChangeInvalidates", "configChangeInvalidates"].iter().any(|name| invalidation[*name] != true)
    {
        actor.insert("invalidation".into(), invalidation.clone());
    }
    let recovery = &context["recovery"];
    classified(recovery, &["conflicts", "stale"], "native recovery status")?;
    if nonempty(&recovery["conflicts"]) || nonempty(&recovery["stale"]) {
        actor.insert("recovery".into(), recovery.clone());
    }
    Ok(())
}

pub(super) fn governor(
    actor: &mut Map<String, Value>,
    value: &Value,
    action: &Value,
    role: &str,
    evidence: &Value,
) -> Result<(), String> {
    classified(
        value,
        &[
            "afterReplan",
            "attempt",
            "budget",
            "completionInformation",
            "consumedGrants",
            "currentGrantEventSha256",
            "currentMutation",
            "currentReplan",
            "currentSensorRequest",
            "defaults",
            "enabled",
            "evidence",
            "grantProtocol",
            "headSha256",
            "lineageSha256",
            "noInformationStreak",
            "observations",
            "postReplanSpent",
            "replanCount",
            "runId",
            "seenEvidenceSha256",
            "seenMutations",
            "seenSensors",
            "spent",
            "state",
            "subjectSha256",
            "version",
        ],
        "native governor",
    )?;
    if value["currentMutation"].is_object() {
        classified(
            &value["currentMutation"],
            &[
                "runId",
                "eventSha256",
                "carryLineage",
                "subjectSha256",
                "attempt",
                "checkpoint",
                "inputSha256",
                "unit",
                "operation",
            ],
            "native current mutation",
        )?;
    } else if nonempty(&value["currentMutation"]) {
        return Err("native current mutation is unavailable".into());
    }
    if role == "worker" && value["currentMutation"].is_object() && action["currentGrant"].is_null()
    {
        return Err("current native mutation lacks its canonical grant form".into());
    }
    if role == "worker"
        && value["currentMutation"]["operation"].is_string()
        && value["currentMutation"]["operation"] != action["currentGrant"]["operation"]
    {
        return Err("current native mutation and grant operation differ".into());
    }
    if action["currentGrant"].is_object()
        && (role != "worker"
            || value["currentMutation"]["eventSha256"]
                != action["currentGrant"]["governorEventSha256"])
    {
        return Err("current native grant identity differs".into());
    }
    if let Some(items) = value["evidence"].as_array() {
        for item in items {
            classified(
                item,
                &[
                    "path",
                    "root",
                    "sha256",
                    "subjectSha256",
                    "attempt",
                    "inputSha256",
                ],
                "native governor evidence",
            )?;
            let sha = item["sha256"]
                .as_str()
                .ok_or("native governor evidence identity is unavailable")?;
            let represented = evidence["items"].as_array().is_some_and(|items| {
                items.iter().any(|source| {
                    ["artifact", "stdout", "stderr"]
                        .iter()
                        .any(|field| source[*field]["sha256"].as_str() == Some(sha))
                })
            });
            if !represented {
                return Err("native governor evidence original is unavailable".into());
            }
        }
    } else if nonempty(&value["evidence"]) {
        return Err("native governor evidence is unclassified".into());
    }
    if let Some(observations) = value["observations"].as_array() {
        for observation in observations {
            classified(
                observation,
                &["key", "checkEventSha256"],
                "native governor observation",
            )?;
            if !observation["key"].as_str().is_some_and(|key| {
                key.len() == 64
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            }) {
                return Err("native governor observation is unclassified".into());
            }
        }
    } else if nonempty(&value["observations"]) {
        return Err("native governor observations are unclassified".into());
    }
    let preserve_mutations = mutation_history(value, evidence)?;
    if value["defaults"].is_object() {
        classified(
            &value["defaults"],
            &[
                "budget",
                "grantProtocol",
                "noInformationLimit",
                "postReplanLimit",
                "replanBinding",
                "version",
            ],
            "native governor defaults",
        )?;
    }
    let mut selected =
        json!({"state":value["state"],"budget":value["budget"],"spent":value["spent"]});
    if let Some(streak) = value["noInformationStreak"].as_u64() {
        if streak > 0 {
            let limit = value["defaults"]["noInformationLimit"]
                .as_u64()
                .ok_or("native no-information limit is unavailable")?;
            selected["noInformationStreak"] = json!(streak);
            selected["noInformationLimit"] = json!(limit);
        }
    } else if !value["noInformationStreak"].is_null() {
        return Err("native no-information streak is unavailable".into());
    }
    if (!value["afterReplan"].is_null() && !value["afterReplan"].is_boolean())
        || (!value["postReplanSpent"].is_null() && !value["postReplanSpent"].is_u64())
    {
        return Err("native post-replan status is unavailable".into());
    }
    if value["afterReplan"] == true || value["postReplanSpent"].as_u64().unwrap_or(0) > 0 {
        let spent = value["postReplanSpent"]
            .as_u64()
            .ok_or("native post-replan count is unavailable")?;
        let limit = value["defaults"]["postReplanLimit"]
            .as_u64()
            .ok_or("native post-replan limit is unavailable")?;
        selected["afterReplan"] = value["afterReplan"].clone();
        selected["postReplanSpent"] = json!(spent);
        selected["postReplanLimit"] = json!(limit);
    }
    for name in [
        "completionInformation",
        "currentReplan",
        "currentSensorRequest",
    ] {
        copy_if_present(selected.as_object_mut().unwrap(), name, &value[name]);
    }
    if preserve_mutations {
        selected["currentMutation"] = value["currentMutation"].clone();
        selected["seenMutations"] = value["seenMutations"].clone();
    }
    actor.insert("governor".into(), selected);
    Ok(())
}

// Preserve classified mutation facts from the canonical snapshot. This
// projection does not reconstruct events or grant mutation permission.
fn mutation_history(value: &Value, evidence: &Value) -> Result<bool, String> {
    let Some(mutations) = value["seenMutations"].as_array() else {
        return if value["seenMutations"].is_null() && value["currentMutation"].is_null() {
            Ok(false)
        } else {
            Err("native mutation history is unclassified".into())
        };
    };
    let identity_keys = [
        "runId",
        "subjectSha256",
        "attempt",
        "checkpoint",
        "inputSha256",
        "unit",
        "operation",
    ];
    let preserve = !mutations.is_empty() || value["currentMutation"].is_object();
    for mutation in mutations {
        classified(
            mutation,
            &["digest", "identity", "evidence"],
            "native mutation history",
        )?;
        let identity = &mutation["identity"];
        classified(identity, &identity_keys, "native mutation identity")?;
        if !known_keys(mutation, &["digest", "identity", "evidence"])
            || !known_keys(identity, &identity_keys)
            || !valid_mutation_identity(identity)
            || !valid_digest(&mutation["digest"])
        {
            return Err("native mutation history is malformed".into());
        }
        let seen = mutation
            .get("evidence")
            .ok_or("native mutation evidence is unavailable")?;
        match seen {
            Value::Null => {}
            Value::String(sha) if valid_digest(seen) => {
                let represented = evidence["items"].as_array().is_some_and(|items| {
                    items.iter().any(|item| {
                        ["artifact", "stdout", "stderr"]
                            .iter()
                            .any(|name| item[*name]["sha256"].as_str() == Some(sha.as_str()))
                    })
                });
                if !represented {
                    return Err("native mutation evidence original is unavailable".into());
                }
            }
            Value::Object(_) => {
                let known = known_keys(
                    seen,
                    &[
                        "path",
                        "root",
                        "sha256",
                        "subjectSha256",
                        "attempt",
                        "inputSha256",
                    ],
                );
                let represented = value["evidence"]
                    .as_array()
                    .is_some_and(|items| items.contains(seen));
                if !known || !represented {
                    return Err("native mutation evidence is unclassified".into());
                }
            }
            _ => return Err("native mutation evidence is unclassified".into()),
        }
    }
    if !preserve {
        return Ok(false);
    }
    let current = &value["currentMutation"];
    if !known_keys(
        current,
        &[
            "runId",
            "eventSha256",
            "carryLineage",
            "subjectSha256",
            "attempt",
            "checkpoint",
            "inputSha256",
            "unit",
            "operation",
        ],
    ) || !valid_mutation_identity(current)
        || !valid_digest(&current["eventSha256"])
        || (!current["carryLineage"].is_null() && !current["carryLineage"].is_boolean())
    {
        return Err("native current mutation is malformed".into());
    }
    Ok(true)
}

fn valid_mutation_identity(identity: &Value) -> bool {
    ["runId", "subjectSha256", "inputSha256", "unit", "operation"]
        .iter()
        .all(|name| {
            identity[*name]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        })
        && identity["attempt"].is_u64()
        && identity["checkpoint"].is_u64()
}

fn valid_digest(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub(super) fn statuses(
    actor: &mut Map<String, Value>,
    tasks: &Value,
    work: &str,
    goal: &Value,
) -> Result<(), String> {
    classified(
        tasks,
        &[
            "categoryDispositions",
            "decomposition",
            "goal",
            "journeyCompletion",
            "nextAction",
            "overall",
            "resultReadiness",
            "tasks",
            "workAssociation",
            "coverageConfirmed",
        ],
        "native task status",
    )?;
    if !tasks["goal"].is_null() && &tasks["goal"] != goal {
        actor.insert("taskGoal".into(), tasks["goal"].clone());
    }
    if let Some(items) = tasks["tasks"].as_array() {
        for item in items {
            classified(
                item,
                &[
                    "id",
                    "taskId",
                    "label",
                    "state",
                    "disposition",
                    "performed",
                    "current",
                    "currentReason",
                    "canonicalId",
                ],
                "native task item",
            )?;
        }
    } else if nonempty(&tasks["tasks"]) {
        return Err("native task items are unavailable".into());
    }
    copy_if_present(actor, "tasks", &tasks["tasks"]);
    classified(
        &tasks["decomposition"],
        &["available", "reason", "total", "complete", "omitted"],
        "native decomposition",
    )?;
    if tasks["decomposition"]["available"] != false
        || !matches!(
            tasks["decomposition"]["reason"].as_str(),
            Some("task_decomposition_unavailable" | "native_session_goal_not_requested")
        )
        || tasks["decomposition"]
            .as_object()
            .is_some_and(|value| value.len() != 2)
    {
        copy_if_present(actor, "taskDecomposition", &tasks["decomposition"]);
    }
    if tasks["resultReadiness"]
        != json!({"applicable":true,"detail":"current Exit Path prerequisites are incomplete","reason":"prerequisites_incomplete","state":"IN_PROGRESS"})
    {
        copy_if_present(actor, "resultReadiness", &tasks["resultReadiness"]);
    }
    if tasks["overall"] != "in_progress" {
        copy_if_present(actor, "overall", &tasks["overall"]);
    }
    copy_if_present(actor, "taskNextAction", &tasks["nextAction"]);
    copy_if_present(actor, "coverageConfirmed", &tasks["coverageConfirmed"]);
    let categories = &tasks["categoryDispositions"];
    if let Some(categories) = categories.as_object() {
        let mut relevant = Map::new();
        for (name, category) in categories {
            if ![
                "obligations",
                "findings",
                "blockers",
                "decisions",
                "externalActions",
            ]
            .contains(&name.as_str())
            {
                if nonempty(category) {
                    return Err(format!("native task category '{name}' is unclassified"));
                }
                continue;
            }
            let category = if let Some(alias) = category["sameAs"].as_str() {
                classified(category, &["sameAs"], "native task category alias")?;
                let target = alias
                    .strip_prefix("tasks.categoryDispositions.")
                    .ok_or("native task category alias is malformed")?;
                let value = categories
                    .get(target)
                    .ok_or("native task category alias is unavailable")?;
                if value["sameAs"].is_string() {
                    return Err("native task category alias is ambiguous".into());
                }
                value
            } else {
                category
            };
            classified(
                category,
                &[
                    "state",
                    "items",
                    "itemsOmitted",
                    "ownerDecisionRequired",
                    "nextActions",
                    "validStates",
                    "validItemDispositions",
                ],
                "native task category",
            )?;
            if let Some(items) = category["items"].as_array() {
                for item in items {
                    classified(
                        item,
                        &[
                            "id",
                            "disposition",
                            "valid",
                            "nextActions",
                            "ownerDecisionRequired",
                            "directEvidence",
                            "resultEvidence",
                        ],
                        "native task category item",
                    )?;
                    for name in ["directEvidence", "resultEvidence"] {
                        if item[name].is_object() {
                            classified(&item[name], &["state", "reason"], "native item evidence")?;
                        } else if nonempty(&item[name]) {
                            return Err("native task item evidence is unavailable".into());
                        }
                    }
                }
            } else if nonempty(&category["items"]) {
                return Err("native task category items are unavailable".into());
            }
            let omitted = match category.get("itemsOmitted") {
                None | Some(Value::Null) => 0,
                Some(value) => value
                    .as_u64()
                    .ok_or("native task omission count is malformed")?,
            };
            if nonempty(&category["items"])
                || omitted > 0
                || !matches!(
                    category["state"].as_str(),
                    Some("unavailable" | "considered" | "none_applicable")
                )
            {
                relevant.insert(name.clone(), category.clone());
            }
        }
        if !relevant.is_empty() {
            actor.insert("taskCategories".into(), Value::Object(relevant));
        }
    } else if nonempty(categories) {
        return Err("native task categories are unavailable".into());
    }
    let journey = &tasks["journeyCompletion"];
    if journey.is_object() {
        classified(
            journey,
            &[
                "integrationWork",
                "members",
                "pendingAdmissions",
                "protocol",
                "readOnly",
                "reason",
                "state",
                "unresolved",
            ],
            "native journey status",
        )?;
        classified(
            &journey["members"],
            &[
                "current",
                "historical",
                "integrationSupported",
                "quarantinedUncredited",
                "retainedHistorical",
                "retainedQuarantinedUncredited",
                "total",
            ],
            "native journey members",
        )?;
        if let Some(items) = journey["unresolved"].as_array() {
            for item in items {
                classified(item, &["state", "work"], "native unresolved journey item")?;
                if item["state"] != "unresolved" || !item["work"].is_string() {
                    return Err("native unresolved journey status is unclassified".into());
                }
            }
        } else if nonempty(&journey["unresolved"]) {
            return Err("native unresolved journey status is unavailable".into());
        }
        if journey["state"] != "blocked"
            || journey["reason"] != "journey_members_need_current_acceptance_or_checked_integration"
            || journey["pendingAdmissions"].as_u64() != Some(0)
            || journey["members"]["quarantinedUncredited"].as_u64() != Some(0)
        {
            actor.insert("journeyStatus".into(), json!({"state":journey["state"],"reason":journey["reason"],"pendingAdmissions":journey["pendingAdmissions"],"quarantinedUncredited":journey["members"]["quarantinedUncredited"]}));
        }
    } else if nonempty(journey) {
        return Err("native journey status is unavailable".into());
    }
    let association = &tasks["workAssociation"];
    if association.is_object() {
        classified(
            association,
            &[
                "goalId",
                "kind",
                "membershipProtocol",
                "startEventSha256",
                "work",
            ],
            "native Goal association",
        )?;
        if association["work"] != work {
            return Err("native Goal association belongs to another Work".into());
        }
        actor.insert(
            "goalAssociation".into(),
            json!({"goalId":association["goalId"],"kind":association["kind"]}),
        );
    } else if nonempty(association) {
        return Err("native Goal association is unavailable".into());
    }
    Ok(())
}
