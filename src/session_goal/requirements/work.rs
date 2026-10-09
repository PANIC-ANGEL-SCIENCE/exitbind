//! Bind declared requirements before Work execution, then inspect its evidence.

use super::*;

pub(crate) struct Prepared {
    pub(crate) basis: String,
    pub(crate) goal_id: String,
    pub(crate) requirements: String,
}

pub(crate) fn prepare(
    loaded: &Loaded,
    goal_id: &str,
    ids: &str,
    checker: &str,
    command: &str,
    basis: Option<&str>,
    integration: bool,
) -> Result<Prepared, String> {
    let record = super::super::read(&loaded.state_root)?.ok_or("no canonical goal is open")?;
    if record["goalId"] != goal_id {
        return Err("canonical goal identity does not match".into());
    }
    let goal = Goal::read(&record)?;
    if goal.project_identity != identity(loaded)? {
        return Err("named goal belongs to another project".into());
    }
    if !goal.source.coverage_confirmed {
        return Err(
            "confirm the agreed requirement set with goal cover before assigning work".into(),
        );
    }
    let checker_source = source(loaded, checker)?;
    if !crate::run::inputs::covered_file(loaded, checker)? {
        return Err("checker input is outside tested product-file coverage; use a non-ignored project checker".into());
    }
    if !command.contains(checker) {
        return Err("check command must name its declared project checker input".into());
    }
    let references = selected(&goal, ids)?;
    let contract = Contract {
        goal_id: goal_id.into(),
        project_identity: goal.project_identity,
        integration,
        requirements: vec![],
        checker_input: CheckerInput {
            path: checker.into(),
            sha256: checker_source.sha256,
        },
    };
    let marker = format!(
        "{PREFIX}header:{}",
        serde_json::to_string(&contract).map_err(|e| e.to_string())?
    );
    let mut value = if let Some(basis) = basis {
        crate::kernel::basis::parse_basis_text(basis, "basis")?.value()
    } else {
        json!({"version":1,"constraints":["Preserve the agreed requirement meanings and existing authority."],
            "openZones":["Implementation within the approved task boundary."],
            "decisiveCases":["Observed current checks and the selected review/Lead decisions support the declared requirements."]})
    };
    value
        .as_object_mut()
        .ok_or("basis is malformed")?
        .remove("sha256");
    let constraints = value["constraints"]
        .as_array_mut()
        .ok_or("basis constraints are malformed")?;
    if constraints
        .iter()
        .any(|item| item.as_str().is_some_and(|text| text.starts_with(PREFIX)))
    {
        return Err("named requirement coverage must be supplied through work begin, not a second basis marker".into());
    }
    constraints.push(json!(marker));
    for reference in &references {
        constraints.push(json!(format!(
            "{PREFIX}requirement:{}",
            serde_json::to_string(&BoundRequirement::from(reference)).map_err(|e| e.to_string())?
        )));
    }
    crate::kernel::basis::parse_basis(&value, "basis")?;
    Ok(Prepared {
        basis: serde_json::to_string(&value).map_err(|e| e.to_string())?,
        goal_id: goal_id.into(),
        requirements: ids.into(),
    })
}

pub(super) fn contract(loaded: &Loaded, work_id: &str) -> Result<Contract, String> {
    let ledger = crate::work::resolve(loaded, work_id)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work has no start event")?;
    let constraints = start["basis"]["constraints"]
        .as_array()
        .ok_or("Work has no frozen named requirement coverage")?;
    let header = format!("{PREFIX}header:");
    let mut markers = constraints
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|value| value.strip_prefix(&header));
    let marker = markers
        .next()
        .ok_or("Work has no frozen named requirement coverage")?;
    if markers.next().is_some() {
        return Err("Work has conflicting named requirement contracts".into());
    }
    let mut contract: Contract =
        serde_json::from_str(marker).map_err(|_| "Work requirement contract is malformed")?;
    if !contract.requirements.is_empty() {
        return Err("Work requirement header must not duplicate entries".into());
    }
    let requirement_prefix = format!("{PREFIX}requirement:");
    for entry in constraints
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|value| value.strip_prefix(&requirement_prefix))
    {
        let requirement: BoundRequirement =
            serde_json::from_str(entry).map_err(|_| "Work requirement entry is malformed")?;
        if contract
            .requirements
            .iter()
            .any(|item| item.id == requirement.id)
        {
            return Err("Work requirement entries repeat an ID".into());
        }
        contract.requirements.push(requirement);
    }
    if contract.requirements.is_empty() || contract.requirements.len() > MAX_REQUIREMENTS {
        return Err("Work requirement coverage is empty or oversized".into());
    }
    Ok(contract)
}

pub(super) fn evidence(
    loaded: &Loaded,
    work_id: &str,
    contract: &Contract,
) -> Result<Value, String> {
    let ledger = crate::work::resolve(loaded, work_id)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let state = crate::run::state::reduce(&events)?;
    let mut view = json!({"work":work_id,"state":"unknown","accepted":false,"subject":state["subject"],
        "integration":contract.integration,"checkerInput":contract.checker_input,
        "check":null,"acceptance":events.last().map(|item|item["eventSha256"].clone()),
        "inputCoverage":crate::run::inputs::COVERAGE,"limits":"Environment, ignored files, outside files, remote services and time are not covered."});
    let current = crate::run::result_ref_evidence(loaded, work_id)?
        .ok_or("Work result cannot be inspected")?;
    if current.drift.is_some() || current.inputs_current == Some(false) {
        view["state"] = json!("stale");
        return Ok(view);
    }
    if current.inputs_current.is_none() {
        return Ok(view);
    }
    if source(loaded, &contract.checker_input.path)?.sha256 != contract.checker_input.sha256
        || !crate::run::inputs::covered_file(loaded, &contract.checker_input.path)?
    {
        view["state"] = json!("stale");
        return Ok(view);
    }
    let check = events.iter().rev().find(|event| {
        event["action"] == "check"
            && event["requirementId"].is_null()
            && event["subjectSha256"] == state["subject"]["sha256"]
    });
    let Some(check) = check else {
        view["state"] = json!("pending");
        return Ok(view);
    };
    view["check"] = json!({"event":check["eventSha256"],"checker":check["checkCommandSha256"],"command":check["checkCommand"],
        "acquisition":check["acquisition"],"producer":check["producer"],"inputs":check["inputsSha256"],"result":check["result"]});
    if check["result"]["kind"] != "exit" || check["result"]["code"] != 0 {
        view["state"] = json!("failed");
        return Ok(view);
    }
    let event_id = check["eventSha256"]
        .as_str()
        .ok_or("check event identity is missing")?;
    let inspected = crate::run::inspect_event(loaded, &ledger, event_id)?;
    let verified = inspected["evidence"].as_array().is_some_and(|items| {
        !items.is_empty() && items.iter().all(|item| item["status"] == "verified")
    });
    if check["acquisition"] != "observed" || !verified {
        return Ok(view);
    }
    if !current.accepted || !current.artifact_current {
        view["state"] = json!("pending");
        return Ok(view);
    }
    view["accepted"] = json!(true);
    view["state"] = json!("current");
    Ok(view)
}
