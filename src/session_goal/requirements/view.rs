//! Read-only current requirement-to-evidence projection and useful next routes.

use super::*;

fn route(loaded: &Loaded, suffix: Vec<String>) -> Value {
    crate::work::compact::continuation_route(&loaded.path, suffix)
}

fn begin_route(
    loaded: &Loaded,
    record: &Value,
    ids: &str,
    goal: &str,
    integration: bool,
    project_selected: bool,
) -> Value {
    let mut args = vec![
        "work".into(),
        "begin".into(),
        "change".into(),
        "--goal".into(),
        goal.into(),
        "--goal-id".into(),
        record["goalId"].as_str().unwrap_or_default().into(),
        "--requirement".into(),
        ids.into(),
        "--artifact".into(),
        "PROJECT_CHECKER".into(),
        "--check-command".into(),
        "CHECK_COMMAND".into(),
        "--review-policy".into(),
        "REVIEW_DECISION".into(),
    ];
    if project_selected {
        args.push("--project-goal".into());
    }
    let mut result = route(loaded, args);
    result["placeholders"] = json!({"PROJECT_CHECKER":"a non-ignored project checker file included in tested inputs",
        "CHECK_COMMAND":"the actual check command naming that checker","REVIEW_DECISION":"the owner's required/omitted choice within this scope"});
    result["effect"] = json!("records a new legitimate Work when executed; no old grant is copied");
    if integration {
        result["command"]
            .as_array_mut()
            .expect("route command")
            .extend([json!("--scope"), json!("integration")]);
    }
    let mut assignment = vec![
        "goal".into(),
        "assign".into(),
        "--goal-id".into(),
        record["goalId"].as_str().unwrap_or_default().into(),
        "--requirement".into(),
        ids.into(),
        "--result-ref".into(),
        "NEW_ACCEPTED_WORK".into(),
        "--disposition".into(),
        "replace".into(),
    ];
    if project_selected {
        assignment.push("--project".into());
    }
    result["afterCurrentAcceptance"] = route(loaded, assignment);
    result
}

pub(crate) fn projection(loaded: &Loaded, record: &Value) -> Result<Value, String> {
    projection_selected(loaded, &loaded.state_root, false, record)
}

pub(crate) fn projection_selected(
    loaded: &Loaded,
    root: &Path,
    project_selected: bool,
    record: &Value,
) -> Result<Value, String> {
    let goal = Goal::read(record)?;
    if goal.project_identity != identity(loaded)? {
        return Err("named goal belongs to another project".into());
    }
    let mut rows = Vec::new();
    let mut evidence = std::collections::BTreeMap::<String, Value>::new();
    for requirement in &goal.requirements {
        let wanted = requirement.reference();
        let mappings = goal
            .mappings
            .iter()
            .filter(|mapping| mapping.active && mapping.requirement.id == requirement.id)
            .collect::<Vec<_>>();
        let mut supports = Vec::new();
        for mapping in &mappings {
            let mut item = json!({"mapping":mapping,"state":"unknown"});
            if mapping.requirement != wanted {
                item["state"] = json!("stale");
                item["reason"] = json!("requirement_revision_changed");
            } else {
                let observed = (|| {
                    let ledger = crate::work::resolve(loaded, &mapping.work)?;
                    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
                    if events.first().map_or(true, |start| {
                        start["eventSha256"] != mapping.start_event_sha256
                    }) {
                        return Err("mapped Work start identity changed".to_owned());
                    }
                    let contract = work::contract(loaded, &mapping.work)?;
                    if contract.goal_id != record["goalId"]
                        || contract.project_identity != goal.project_identity
                        || !contract.covers(&wanted)
                    {
                        return Err("mapped Work does not cover the current requirement".to_owned());
                    }
                    match evidence.get(&mapping.work) {
                        Some(cached) => Ok(cached.clone()),
                        None => work::evidence(loaded, &mapping.work, &contract),
                    }
                })();
                let observed = observed.unwrap_or_else(
                    |error| json!({"work":mapping.work,"state":"unknown","reason":error}),
                );
                item["state"] = observed["state"].clone();
                item["evidence"] = observed.clone();
                evidence.insert(mapping.work.clone(), observed);
            }
            supports.push(item);
        }
        let state = if mappings.is_empty() {
            "unmapped"
        } else if supports.iter().any(|item| item["state"] == "failed") {
            "failed"
        } else if supports.iter().any(|item| item["state"] == "stale") {
            "stale"
        } else if supports.iter().any(|item| item["state"] == "unknown") {
            "unknown"
        } else if supports.iter().any(|item| item["state"] == "pending") {
            "pending"
        } else {
            "current"
        };
        let action = if state == "pending" {
            mappings
                .iter()
                .find(|mapping| {
                    supports.iter().any(|item| {
                        item["mapping"]["work"] == mapping.work && item["state"] == "pending"
                    })
                })
                .map(|mapping| {
                    route(
                        loaded,
                        vec![
                            "work".into(),
                            "detail".into(),
                            mapping.work.clone(),
                            "--json".into(),
                        ],
                    )
                })
                .unwrap_or(Value::Null)
        } else if state != "current" {
            begin_route(
                loaded,
                record,
                &requirement.id,
                &format!("Validate or repair: {}", requirement.text),
                false,
                project_selected,
            )
        } else {
            Value::Null
        };
        rows.push(json!({"id":requirement.id,"revision":requirement.revision,"text":requirement.text,
            "source":{"ref":requirement.source.reference,"sha256":requirement.source.sha256},
            "history":requirement.history,"mapped":!mappings.is_empty(),"state":state,"satisfied":state=="current",
            "support":supports,"nextAction":action}));
    }
    let complete =
        goal.source.coverage_confirmed && rows.iter().all(|row| row["satisfied"] == true);
    let next = if !goal.source.coverage_confirmed {
        let mut args = vec![
            "goal".into(),
            "cover".into(),
            "--goal-id".into(),
            record["goalId"].as_str().unwrap_or_default().into(),
        ];
        if project_selected {
            args.push("--project".into());
        }
        route(loaded, args)
    } else if let Some(row) = rows.iter().find(|row| row["satisfied"] != true) {
        row["nextAction"].clone()
    } else {
        let full = goal
            .mappings
            .iter()
            .filter(|mapping| mapping.active)
            .find_map(|mapping| {
                let contract = work::contract(loaded, &mapping.work).ok()?;
                (contract.integration
                    && goal
                        .requirements
                        .iter()
                        .all(|requirement| contract.covers(&requirement.reference()))
                    && evidence
                        .get(&mapping.work)
                        .is_some_and(|result| result["state"] == "current"))
                .then_some(mapping.work.clone())
            });
        if let Some(work) = full {
            let validation = super::validate_close_candidate(
                loaded,
                root,
                record,
                record["goalId"].as_str().unwrap_or_default(),
                &work,
                complete,
            );
            if validation.as_ref().err().is_some_and(|error| {
                error == "additional direct completion is stale or unknown"
                    || error.starts_with("additional goal result in ")
            }) {
                super::super::progress::categories::category_revalidation_route(
                    loaded,
                    record,
                    project_selected,
                )
                .unwrap_or(Value::Null)
            } else if validation.is_ok() {
                let mut args = vec![
                    "goal".into(),
                    "close".into(),
                    "--goal-id".into(),
                    record["goalId"].as_str().unwrap_or_default().into(),
                    "--result-ref".into(),
                    work,
                    "--view".into(),
                    "event".into(),
                    "--json".into(),
                ];
                if project_selected {
                    args.push("--project".into());
                }
                route(loaded, args)
            } else {
                let ids = goal
                    .requirements
                    .iter()
                    .map(|item| item.id.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                begin_route(
                    loaded,
                    record,
                    &ids,
                    "Validate the final integration of every agreed requirement.",
                    true,
                    project_selected,
                )
            }
        } else {
            let ids = goal
                .requirements
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            begin_route(
                loaded,
                record,
                &ids,
                "Validate the final integration of every agreed requirement.",
                true,
                project_selected,
            )
        }
    };
    let preparation_ids = if !complete {
        rows.iter()
            .find(|row| row["satisfied"] != true)
            .and_then(|row| row["id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                goal.requirements
                    .iter()
                    .map(|item| item.id.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
    } else {
        goal.requirements
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    let preparation = if goal.source.coverage_confirmed {
        let mut current = vec![
            "goal".into(),
            "prepare".into(),
            "--goal-id".into(),
            record["goalId"].as_str().unwrap_or_default().into(),
            "--requirement".into(),
            preparation_ids.clone(),
            "--artifact".into(),
            "PROJECT_CHECKER".into(),
            "--check-command".into(),
            "CHECK_COMMAND".into(),
            "--json".into(),
        ];
        if project_selected {
            current.push("--project".into());
        }
        let mut project = current.clone();
        if !project_selected {
            project.push("--project".into());
        }
        if complete {
            current.extend(["--scope".into(), "integration".into()]);
            project.extend(["--scope".into(), "integration".into()]);
        }
        let current_route = route(loaded, current);
        let project_route = route(loaded, project);
        json!({"available":true,"phase":"before_work_freeze","selectedRequirementIds":preparation_ids.split(',').collect::<Vec<_>>(),
            "scopeRoutes":{"currentGoal":current_route,"projectGoal":project_route},
            "placeholders":{"PROJECT_CHECKER":"current non-ignored project checker file","CHECK_COMMAND":"check command that names PROJECT_CHECKER"},
            "effect":"read-only validation; the returned beginRoute remains a separate Work-creation decision"})
    } else {
        json!({"available":false,"reason":"lead_must_confirm_finite_coverage_first","selectedRequirementIds":preparation_ids.split(',').collect::<Vec<_>>()})
    };
    Ok(
        json!({"goalId":record["goalId"],"goalRevision":record["revision"],"source":{"ref":goal.source.reference,"sha256":goal.source.sha256},
        "coverageConfirmed":goal.source.coverage_confirmed,"coverageOwner":"lead","coverageLimit":"Declared finite coverage is not proof of complete natural-language interpretation.",
        "requirements":rows,"coverageCurrent":complete,"preFreezePreparation":preparation,"historicalClosure":record["closure"],"nextAction":next,"readOnly":true}),
    )
}

pub(crate) fn all_current(loaded: &Loaded, record: &Value) -> Result<bool, String> {
    Ok(projection(loaded, record)?["coverageCurrent"] == true)
}
