//! Bind declared requirements before Work execution, then inspect its evidence.

use super::*;
use std::path::Path;

const SCOPE_KIND: &str = "named_goal_scope";
const SCOPE_PROTOCOL: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContractScope {
    Project,
    Session,
}

impl ContractScope {
    fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Session => "session",
        }
    }
}

pub(crate) struct Prepared {
    pub(crate) basis: String,
    pub(crate) goal_id: String,
    pub(crate) requirements: String,
    pub(crate) scope: ContractScope,
    pub(crate) project_identity: String,
}

pub(crate) struct PrepareRequest<'a> {
    pub(crate) root: &'a Path,
    pub(crate) loaded: &'a Loaded,
    pub(crate) goal_id: &'a str,
    pub(crate) ids: &'a str,
    pub(crate) checker: &'a str,
    pub(crate) command: &'a str,
    pub(crate) basis: Option<&'a str>,
    pub(crate) integration: bool,
    pub(crate) scope: ContractScope,
}

/// Validate a declared checker with the same path, coverage and command-name
/// rules used by named requirement Works. Journey integration reuses this
/// owner instead of introducing a second checker policy.
pub(crate) fn checker_input(
    loaded: &Loaded,
    checker: &str,
    command: &str,
) -> Result<CheckerInput, String> {
    let checker_source = source(loaded, checker)?;
    if !crate::run::inputs::covered_file(loaded, checker)? {
        return Err("checker input is outside tested product-file coverage; use a non-ignored project checker".into());
    }
    if !command.contains(checker) {
        return Err("check command must name its declared project checker input".into());
    }
    Ok(CheckerInput {
        path: checker.into(),
        sha256: checker_source.sha256,
    })
}

pub(crate) fn prepare(request: PrepareRequest<'_>) -> Result<Prepared, String> {
    let PrepareRequest {
        root,
        loaded,
        goal_id,
        ids,
        checker,
        command,
        basis,
        integration,
        scope,
    } = request;
    let record = super::super::read(root)?.ok_or("no canonical goal is open")?;
    if record["goalId"] != goal_id {
        return Err("canonical goal identity does not match".into());
    }
    if record["closure"]["closed"] != false {
        return Err(
            "named Goal is closed; incorporate an explicit successor before assigning work".into(),
        );
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
    let checker_input = checker_input(loaded, checker, command)?;
    let references = selected(&goal, ids)?;
    let contract = Contract {
        goal_id: goal_id.into(),
        project_identity: goal.project_identity.clone(),
        integration,
        requirements: vec![],
        checker_input,
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
        scope,
        project_identity: contract.project_identity,
    })
}

/// Persist the chosen named-Goal journal separately from the immutable Work
/// start event. The record is a locator only; every later projection rechecks
/// the Work start, exact frozen contract, Goal identity, mapping and evidence.
pub(crate) fn write_scope_companion(
    loaded: &Loaded,
    work_id: &str,
    start_event_sha256: &str,
    prepared: &Prepared,
) -> Result<(), String> {
    if !valid_hash(start_event_sha256) || !valid_hash(&prepared.project_identity) {
        return Err("named Goal scope companion identity is malformed".into());
    }
    let fields = json!({
        "scope": prepared.scope.label(),
        "goalId": prepared.goal_id,
        "projectIdentitySha256": prepared.project_identity,
    });
    crate::session_goal::association::write_private_companion(
        loaded,
        work_id,
        crate::session_goal::association::PrivateCompanionWrite {
            suffix: ".named-goal-scope.json",
            label: "named Goal scope companion",
            kind: SCOPE_KIND,
            protocol: SCOPE_PROTOCOL,
            start_event_sha256,
            fields,
        },
    )
}

/// Resolve the frozen scope from its exact Work-local companion. Older named
/// Works predate the companion and retain their project-local Goal behavior.
fn scope_companion(
    loaded: &Loaded,
    work_id: &str,
    start: &Value,
    contract: &Contract,
) -> Result<Option<ContractScope>, String> {
    let Some(value) = crate::session_goal::association::read_private_companion(
        loaded,
        work_id,
        ".named-goal-scope.json",
        "named Goal scope companion",
        SCOPE_KIND,
        SCOPE_PROTOCOL,
        &["scope", "goalId", "projectIdentitySha256"],
    )?
    else {
        return Ok(None);
    };
    validate_scope_companion(&value)?;
    let start_sha = start["eventSha256"]
        .as_str()
        .ok_or("Work start event identity is missing")?;
    if value["work"] != work_id
        || value["startEventSha256"] != start_sha
        || value["goalId"] != contract.goal_id
        || value["projectIdentitySha256"] != contract.project_identity
    {
        return Err("named Goal scope companion does not match the frozen Work contract".into());
    }
    match value["scope"].as_str() {
        Some("project") => Ok(Some(ContractScope::Project)),
        Some("session") => Ok(Some(ContractScope::Session)),
        _ => Err("named Goal scope companion selects an unsupported scope".into()),
    }
}

fn validate_scope_companion(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("named Goal scope companion is malformed")?;
    if object.len() != 8
        || value["kind"] != SCOPE_KIND
        || value["protocol"].as_u64() != Some(SCOPE_PROTOCOL)
        || value["work"].as_str().is_none()
        || value["goalId"].as_str().map_or(true, |id| {
            id.trim().is_empty() || id.len() > 128 || id.contains('\0')
        })
        || value["projectIdentitySha256"]
            .as_str()
            .map_or(true, |hash| !valid_hash(hash))
        || !matches!(value["scope"].as_str(), Some("project" | "session"))
    {
        return Err("named Goal scope companion is invalid or unsupported".into());
    }
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn parse_contract(loaded: &Loaded, work_id: &str) -> Result<(Contract, Value), String> {
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
    Ok((contract, start.clone()))
}

pub(super) fn contract(loaded: &Loaded, work_id: &str) -> Result<Contract, String> {
    let (contract, start) = parse_contract(loaded, work_id)?;
    let _ = contract_scope_for_start(loaded, work_id, &start, &contract)?;
    Ok(contract)
}

pub(crate) fn contract_scope(loaded: &Loaded, work_id: &str) -> Result<ContractScope, String> {
    let (contract, start) = parse_contract(loaded, work_id)?;
    contract_scope_for_start(loaded, work_id, &start, &contract)
}

fn contract_scope_for_start(
    loaded: &Loaded,
    work_id: &str,
    start: &Value,
    contract: &Contract,
) -> Result<ContractScope, String> {
    if let Some(scope) = scope_companion(loaded, work_id, start, contract)? {
        return Ok(scope);
    }
    match crate::session_goal::association::context_for_work(loaded, work_id) {
        crate::session_goal::association::WorkContext::Local => Ok(ContractScope::Project),
        _ => Err(
            "named Goal journal selector is missing; refusing to infer project or session scope"
                .into(),
        ),
    }
}

pub(crate) fn scope_matches_root(
    loaded: &Loaded,
    work_id: &str,
    root: &Path,
) -> Result<bool, String> {
    match contract_scope(loaded, work_id)? {
        ContractScope::Project => Ok(root == loaded.state_root),
        ContractScope::Session => {
            let scope = crate::session_goal::native::scope(loaded, false)?;
            scope.require_owner()?;
            Ok(scope.is_session() && root == scope.root)
        }
    }
}

pub(crate) fn project_progress_for_work(
    loaded: &Loaded,
    work_id: &str,
) -> Result<Option<Value>, String> {
    let ledger = crate::work::resolve(loaded, work_id)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work has no start event")?;
    let Some(contract) = contract_if_named(loaded, work_id, start)? else {
        return Ok(None);
    };
    let selected_scope = match contract_scope_for_start(loaded, work_id, start, &contract) {
        Ok(scope) => scope,
        Err(error) => {
            return Ok(Some(
                json!({"kind":"project_named_contract","state":"unavailable",
                "reason":"named_goal_scope_unavailable","detail":error,"readOnly":true}),
            ))
        }
    };
    if selected_scope != ContractScope::Project {
        return Ok(None);
    }
    if contract.project_identity != identity(loaded)? {
        return Ok(Some(
            json!({"kind":"project_named_contract","scope":"project",
            "state":"unavailable","reason":"named_goal_belongs_to_another_configured_project",
            "readOnly":true}),
        ));
    }
    let Some(record) = super::super::read(&loaded.state_root)? else {
        return Ok(Some(
            json!({"kind":"project_named_contract","scope":"project",
            "goalId":contract.goal_id,"state":"stale","reason":"project_goal_unavailable",
            "readOnly":true}),
        ));
    };
    if record["goalId"] != contract.goal_id {
        return Ok(Some(
            json!({"kind":"project_named_contract","scope":"project",
            "goalId":contract.goal_id,"state":"stale","reason":"project_goal_identity_changed",
            "readOnly":true}),
        ));
    }
    let goal = Goal::read(&record)?;
    if goal.project_identity != contract.project_identity {
        return Ok(Some(
            json!({"kind":"project_named_contract","scope":"project",
            "goalId":contract.goal_id,"state":"stale","reason":"project_goal_owner_changed",
            "readOnly":true}),
        ));
    }
    let start_sha = start["eventSha256"]
        .as_str()
        .ok_or("Work start event identity is missing")?;
    let observed = evidence(loaded, work_id, &contract)?;
    let mut rows = Vec::new();
    for bound in &contract.requirements {
        let current = goal
            .requirements
            .iter()
            .find(|requirement| requirement.id == bound.id);
        let current_ref =
            current.map(|requirement| BoundRequirement::from(&requirement.reference()));
        let mappings = goal
            .mappings
            .iter()
            .filter(|mapping| {
                mapping.active
                    && mapping.work == work_id
                    && mapping.start_event_sha256 == start_sha
                    && mapping.requirement.id == bound.id
            })
            .collect::<Vec<_>>();
        let exact_mapping = mappings.iter().any(|mapping| {
            current.is_some_and(|requirement| mapping.requirement == requirement.reference())
        });
        let state = if current_ref.as_ref() != Some(bound) {
            "stale"
        } else if !exact_mapping {
            if mappings.is_empty() {
                "unmapped"
            } else {
                "stale"
            }
        } else {
            observed["state"].as_str().unwrap_or("unknown")
        };
        rows.push(json!({"id":bound.id,"revision":bound.revision,
            "state":state,"mapped":exact_mapping,"support":observed}));
    }
    let current =
        goal.source.coverage_confirmed && rows.iter().all(|row| row["state"] == "current");
    Ok(Some(
        json!({"kind":"project_named_contract","scope":"project",
        "authority":"project_goal_owner","goalId":contract.goal_id,
        "work":work_id,"startEventSha256":start_sha,"integration":contract.integration,
        "coverageConfirmed":goal.source.coverage_confirmed,"contractCurrent":current,
        "requirements":rows,"readOnly":true,
        "closureAuthority":"goal close in the configured project; this projection never closes the session Goal"}),
    ))
}

fn contract_if_named(
    loaded: &Loaded,
    work_id: &str,
    start: &Value,
) -> Result<Option<Contract>, String> {
    // A Work begun without --basis records no basis, so it cannot carry a
    // named-contract header; only a recorded basis can be malformed.
    if start["basis"].is_null() {
        return Ok(None);
    }
    let constraints = start["basis"]["constraints"]
        .as_array()
        .ok_or("Work basis constraints are malformed")?;
    if !constraints
        .iter()
        .filter_map(Value::as_str)
        .any(|value| value.starts_with(&format!("{PREFIX}header:")))
    {
        return Ok(None);
    }
    parse_contract(loaded, work_id).map(|(contract, _)| Some(contract))
}

pub(super) fn evidence(
    loaded: &Loaded,
    work_id: &str,
    contract: &Contract,
) -> Result<Value, String> {
    let mut view = current_checker_evidence(loaded, work_id, &contract.checker_input)?;
    view["integration"] = json!(contract.integration);
    Ok(view)
}

/// Read the exact accepted result and its observed passing check for a frozen
/// checker. Both named and session journey integration use this evidence
/// owner; neither may replace it with a new capture or acceptance mechanism.
pub(crate) fn current_checker_evidence(
    loaded: &Loaded,
    work_id: &str,
    checker_input: &CheckerInput,
) -> Result<Value, String> {
    let ledger = crate::work::resolve(loaded, work_id)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let state = crate::run::state::reduce(&events)?;
    let mut view = json!({"work":work_id,"state":"unknown","accepted":false,"subject":state["subject"],
        "checkerInput":checker_input,
        "check":null,"acceptance":events.last().map(|item|item["eventSha256"].clone()),
        "inputCoverage":crate::run::inputs::COVERAGE,"limits":"Environment, ignored files, outside files, remote services and time are not covered."});
    let check = events.iter().rev().find(|event| {
        event["action"] == "check"
            && event["requirementId"].is_null()
            && event["subjectSha256"] == state["subject"]["sha256"]
    });
    if let Some(check) = check {
        view["check"] = json!({"event":check["eventSha256"],"checker":check["checkCommandSha256"],"command":check["checkCommand"],
            "acquisition":check["acquisition"],"producer":check["producer"],"inputs":check["inputsSha256"],"result":check["result"]});
    }
    let accepted = state["status"] == "accepted"
        && events.last().is_some_and(|event| {
            event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "accepted"
        });
    if !accepted {
        if check
            .is_some_and(|event| event["result"]["kind"] != "exit" || event["result"]["code"] != 0)
        {
            view["state"] = json!("failed");
        } else {
            view["state"] = json!("pending");
        }
        return Ok(view);
    }
    let current = crate::run::result_ref_evidence(loaded, work_id)?
        .ok_or("Work result cannot be inspected")?;
    if current.drift.is_some() || current.inputs_current == Some(false) {
        view["state"] = json!("stale");
        return Ok(view);
    }
    if current.inputs_current.is_none() {
        return Ok(view);
    }
    if source(loaded, &checker_input.path)?.sha256 != checker_input.sha256
        || !crate::run::inputs::covered_file(loaded, &checker_input.path)?
    {
        view["state"] = json!("stale");
        return Ok(view);
    }
    let Some(check) = check else {
        view["state"] = json!("pending");
        return Ok(view);
    };
    if check["result"]["kind"] != "exit" || check["result"]["code"] != 0 {
        view["state"] = json!("failed");
        return Ok(view);
    }
    if !check["checkCommand"]
        .as_str()
        .is_some_and(|command| command.contains(&checker_input.path))
    {
        view["state"] = json!("stale");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn loaded() -> Loaded {
        Loaded {
            config: json!({}),
            agents: BTreeMap::new(),
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            workflows: BTreeMap::new(),
            verification_preparation: None,
            path: PathBuf::from("config.json"),
            control_root: PathBuf::from("fixture-root"),
            product_root: PathBuf::from("fixture-root"),
            state_root: PathBuf::from("fixture-root"),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: String::new(),
        }
    }

    #[test]
    fn absent_basis_has_no_named_contract() {
        let start = json!({"action": "start"});
        assert!(contract_if_named(&loaded(), "work", &start)
            .unwrap()
            .is_none());
    }

    #[test]
    fn recorded_basis_without_constraints_array_stays_malformed() {
        let start = json!({"basis": {"version": 1, "constraints": "not-an-array"}});
        assert_eq!(
            contract_if_named(&loaded(), "work", &start)
                .err()
                .as_deref(),
            Some("Work basis constraints are malformed")
        );
    }

    #[test]
    fn recorded_basis_with_missing_constraints_stays_malformed() {
        let start = json!({"basis": {"version": 1}});
        assert_eq!(
            contract_if_named(&loaded(), "work", &start)
                .err()
                .as_deref(),
            Some("Work basis constraints are malformed")
        );
    }

    #[test]
    fn recorded_basis_with_null_constraints_stays_malformed() {
        let start = json!({"basis": {"version": 1, "constraints": null}});
        assert_eq!(
            contract_if_named(&loaded(), "work", &start)
                .err()
                .as_deref(),
            Some("Work basis constraints are malformed")
        );
    }

    #[test]
    fn recorded_basis_without_named_header_has_no_named_contract() {
        let start = json!({"basis": {"version": 1, "constraints": ["preserve history"]}});
        assert!(contract_if_named(&loaded(), "work", &start)
            .unwrap()
            .is_none());
    }
}
