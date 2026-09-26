//! Read-only continuation and applicability projection.

use super::*;

fn route(loaded: &Loaded, work_id: &str, tail: Vec<String>) -> Value {
    let mut suffix = vec!["work".into(), "continuation".into(), work_id.into()];
    suffix.extend(tail);
    crate::work::compact::continuation_route(&loaded.path, suffix)
}

fn mutation_context(work_id: &str, goal_revision: &Value, binding_revision: u64) -> Value {
    let body = json!({
        "work": work_id,
        "goalRevision": goal_revision,
        "bindingRevision": binding_revision,
    });
    json!({
        "work": work_id,
        "goalRevision": goal_revision,
        "bindingRevision": binding_revision,
        "token": format!(
            "{work_id}:{}:{}:{}",
            goal_revision.as_u64().unwrap_or_default(),
            binding_revision,
            hash::value(&body)
        ),
    })
}

pub(crate) fn has_unresolved(record: &Value) -> bool {
    let c = &record["continuation"];
    if c.is_null() {
        return false;
    }
    if c["source"]["coverageConfirmed"] != true {
        return true;
    }
    let Some(requirements) = c["requirements"].as_array() else {
        return true;
    };
    let Some(supports) = c["supports"].as_array() else {
        return true;
    };
    requirements.iter().any(|req| {
        !supports.iter().any(|support| {
            support["requirementId"] == req["id"]
                && support["requirementRevision"] == req["revision"]
                && support["sourceSha256"] == req["sourceSha256"]
        })
    })
}

pub(crate) fn support_current(loaded: &Loaded, record: &Value) -> Result<bool, String> {
    let c = &record["continuation"];
    if c.is_null() {
        return Ok(true);
    }
    if has_unresolved(record) {
        return Ok(false);
    }
    let ledger = verify_work(
        loaded,
        c["work"].as_str().ok_or("continuation work missing")?,
    )?;
    let (inputs, conditions) = current_conditions(loaded)?;
    for requirement in c["requirements"]
        .as_array()
        .ok_or("requirements malformed")?
    {
        let mut applicable = false;
        for support in c["supports"]
            .as_array()
            .ok_or("supports malformed")?
            .iter()
            .filter(|x| {
                x["requirementId"] == requirement["id"]
                    && x["requirementRevision"] == requirement["revision"]
                    && x["sourceSha256"] == requirement["sourceSha256"]
            })
        {
            if support["inputsSha256"] != inputs || support["conditionsSha256"] != conditions {
                continue;
            }
            if let Some(event) = support["resultEventSha256"].as_str() {
                if run::inspect_event(loaded, &ledger, event).is_ok_and(|found| {
                    evidence_verified(&found)
                        && acquired_config_current(loaded, &found["event"])
                        && found["eventIndex"].as_u64().unwrap_or(0)
                            >= requirement["checkFloorIndex"].as_u64().unwrap_or(0)
                }) {
                    applicable = true;
                    break;
                }
            }
        }
        if !applicable {
            return Ok(false);
        }
    }
    Ok(true)
}

fn complete_view(loaded: &Loaded, work_id: &str) -> Result<Value, String> {
    let ledger = verify_work(loaded, work_id)?;
    let record = read(&loaded.state_root)?.ok_or("continuation has not been initialized")?;
    if record["goalId"] != work_id || record["continuation"]["work"] != work_id {
        return Err("current goal belongs to another work".into());
    }
    let c = &record["continuation"];
    let (current_inputs, current_conditions) = current_conditions(loaded)?;
    let binding_revision = c["binding"]["revision"].as_u64().unwrap_or(0);
    let mut requirements = Vec::new();
    for requirement in c["requirements"]
        .as_array()
        .ok_or("requirements malformed")?
    {
        let supports = c["supports"].as_array().ok_or("supports malformed")?;
        let mut valid = Vec::new();
        for support in supports.iter().filter(|x| {
            x["requirementId"] == requirement["id"]
                && x["requirementRevision"] == requirement["revision"]
                && x["sourceSha256"] == requirement["sourceSha256"]
        }) {
            let event = support["resultEventSha256"].as_str().unwrap_or_default();
            let found = run::inspect_event(loaded, &ledger, event);
            let current = found.as_ref().is_ok_and(evidence_verified);
            let acquired_conditions_current = found
                .as_ref()
                .is_ok_and(|found| acquired_config_current(loaded, &found["event"]));
            let revision_current = found.as_ref().is_ok_and(|found| {
                found["eventIndex"].as_u64().unwrap_or(0)
                    >= requirement["checkFloorIndex"].as_u64().unwrap_or(0)
            });
            let applicable = current
                && acquired_conditions_current
                && revision_current
                && support["inputsSha256"] == current_inputs
                && support["conditionsSha256"] == current_conditions;
            valid.push(
                json!({"relation":support,"integrityCurrent":current,"acquiredConditionsCurrent":acquired_conditions_current,
                "revisionCurrent":revision_current,"applicableCurrent":applicable,
                "exactRead":{"command":"run inspect","ledger":ledger,"event":event}}),
            );
        }
        requirements.push(json!({"requirement":requirement,"support":valid,"unresolved":valid.iter().all(|x| x["applicableCurrent"]!=true)}));
    }
    let mut result = json!({"work":work_id,"goalRevision":record["revision"],
        "mutationContext":mutation_context(work_id, &record["revision"], binding_revision),
        "source":c["source"],
        "requirements":requirements,"binding":c["binding"],"corrections":c["corrections"],
        "operations":c["operations"],"diagnoses":c["diagnoses"],"children":c["children"],
        "currentInputsSha256":current_inputs,"currentConditionsSha256":current_conditions,
        "wholeGoalReady":false,"readOnly":true});
    result["wholeGoalReady"] = json!(
        c["source"]["coverageConfirmed"] == true
            && result["requirements"]
                .as_array()
                .is_some_and(|xs| xs.iter().all(|x| x["unresolved"] == false))
            && crate::session_goal::presentation_for_loaded(loaded, Some(&record))?
                ["explicitLeadClosure"]
                == true
    );
    Ok(result)
}

const SECTIONS: [&str; 7] = [
    "source",
    "requirements",
    "binding",
    "corrections",
    "operations",
    "diagnoses",
    "children",
];

fn bounded(value: &Value) -> Result<bool, String> {
    Ok(serde_json::to_string_pretty(value)
        .map_err(|e| e.to_string())?
        .len()
        < 64 * 1024)
}

/// The supported receiving sequence, so a receiving host binds and records a
/// native child from this read-only view without studying help or state.
fn receive_routes(loaded: &Loaded, work_id: &str, token: &Value) -> Value {
    let bound_context = token.as_str().unwrap_or("TOKEN").to_owned();
    let mut bind = vec![
        "work".into(),
        "bind".into(),
        work_id.into(),
        "--context".into(),
    ];
    bind.extend(
        [
            bound_context.as_str(),
            "--host",
            "HOST",
            "--session",
            "NATIVE_SESSION",
            "--host-version",
            "HOST_VERSION",
        ]
        .map(String::from),
    );
    let child = [
        "work",
        "child",
        work_id,
        "SHORT_ASSIGNMENT",
        "--context",
        "FRESH_TOKEN",
        "--native-child",
        "CHILD_ID",
    ]
    .map(String::from)
    .to_vec();
    json!({
        "bind": crate::work::compact::continuation_route(&loaded.path, bind)["command"],
        "child": crate::work::compact::continuation_route(&loaded.path, child)["command"],
        "placeholders": {
            "HOST": "the receiving host, such as claude or codex",
            "HOST_VERSION": "the host's reported version",
            "NATIVE_SESSION": "$EXITBIND_NATIVE_SESSION_ID; if unset, report that limit instead of inventing one",
            "FRESH_TOKEN": "mutationContext.token from the bind reply's nextAction.command",
            "SHORT_ASSIGNMENT": "a short description of the child's task",
            "CHILD_ID": "the host-reported native child ID; Claude Code shows a subagent's agentId when the subagent runs in the background, not in a foreground result. If none is visible, report that limit instead of inventing one",
        },
        "childResult": "the child's final message exactly as the host delivered it, on standard input: no summary, translation, or added notes; at most 8 KiB",
    })
}

pub(crate) fn continuation_view(loaded: &Loaded, work_id: &str) -> Result<Value, String> {
    let mut full = complete_view(loaded, work_id)?;
    let receive = receive_routes(loaded, work_id, &full["mutationContext"]["token"]);
    full["receive"] = receive.clone();
    if bounded(&full)? {
        return Ok(full);
    }
    let sections = SECTIONS
        .iter()
        .map(|name| {
            let route = route(loaded, work_id, vec!["--section".into(), (*name).into()]);
            json!({"name":name,"command":route["command"],
                "sameConfigRequired":route["sameConfigRequired"],
                "sameExecutableRequired":route["sameExecutableRequired"]})
        })
        .collect::<Vec<_>>();
    Ok(json!({"work":work_id,"goalRevision":full["goalRevision"],
        "mutationContext":full["mutationContext"],"binding":full["binding"],
        "sourceSha256":full["source"]["sha256"],"currentInputsSha256":full["currentInputsSha256"],
        "currentConditionsSha256":full["currentConditionsSha256"],
        "wholeGoalReady":full["wholeGoalReady"],"readOnly":true,"requiresExpansion":true,
        "sections":sections,"receive":receive}))
}

pub(crate) fn continuation_section(
    loaded: &Loaded,
    work_id: &str,
    section: &str,
    index: Option<&str>,
    history_index: Option<&str>,
) -> Result<Value, String> {
    if !SECTIONS.contains(&section) {
        return Err("unknown continuation section".into());
    }
    let full = complete_view(loaded, work_id)?;
    let value = &full[section];
    if let Some(index) = index {
        let index = index
            .parse::<usize>()
            .map_err(|_| "invalid section index")?;
        let item = value
            .as_array()
            .and_then(|items| items.get(index))
            .ok_or("section item is unavailable")?;
        if let Some(history_index) = history_index {
            if section != "requirements" {
                return Err("--history-index requires the requirements section".into());
            }
            let history_index = history_index
                .parse::<usize>()
                .map_err(|_| "invalid history index")?;
            let history = item["requirement"]["history"]
                .as_array()
                .and_then(|items| items.get(history_index))
                .ok_or("requirement history item is unavailable")?;
            return Ok(json!({"work":work_id,"goalRevision":full["goalRevision"],
                "section":section,"index":index,"historyIndex":history_index,
                "historyEntry":history,"readOnly":true}));
        }
        let result = json!({"work":work_id,"goalRevision":full["goalRevision"],
            "section":section,"index":index,"item":item,"readOnly":true});
        if bounded(&result)? {
            return Ok(result);
        }
        if section != "requirements" {
            return Err("section item exceeds 64 KiB".into());
        }
        let history = item["requirement"]["history"]
            .as_array()
            .ok_or("oversized requirement has no history")?;
        let mut compact_item = item.clone();
        compact_item["requirement"]["history"] = Value::Null;
        let history_route = route(
            loaded,
            work_id,
            vec![
                "--section".into(),
                "requirements".into(),
                "--index".into(),
                index.to_string(),
                "--history-index".into(),
                "N".into(),
            ],
        );
        let summary = json!({"work":work_id,"goalRevision":full["goalRevision"],
            "section":section,"index":index,"item":compact_item,
            "historyCount":history.len(),
            "historyRead":history_route["command"],
            "historyReadSameConfigRequired":history_route["sameConfigRequired"],
            "historyReadSameExecutableRequired":history_route["sameExecutableRequired"],
            "requiresExpansion":true,"readOnly":true});
        if !bounded(&summary)? {
            return Err("requirement item exceeds 64 KiB after history expansion".into());
        }
        return Ok(summary);
    }
    if history_index.is_some() {
        return Err("--history-index requires --index".into());
    }
    let result = json!({"work":work_id,"goalRevision":full["goalRevision"],
        "section":section,"value":value,"readOnly":true});
    if bounded(&result)? {
        return Ok(result);
    }
    let items = value.as_array().ok_or("section exceeds 64 KiB")?;
    let routes = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let route = route(
                loaded,
                work_id,
                vec![
                    "--section".into(),
                    section.into(),
                    "--index".into(),
                    index.to_string(),
                ],
            );
            json!({"index":index,"id":item["id"],"nativeChild":item["nativeChild"],
                "resultSha256":item["resultSha256"],"command":route["command"],
                "sameConfigRequired":route["sameConfigRequired"],
                "sameExecutableRequired":route["sameExecutableRequired"]})
        })
        .collect::<Vec<_>>();
    let summary = json!({"work":work_id,"goalRevision":full["goalRevision"],
        "section":section,"count":items.len(),"requiresExpansion":true,
        "items":routes,"readOnly":true});
    if !bounded(&summary)? {
        return Err("section index exceeds 64 KiB".into());
    }
    Ok(summary)
}
