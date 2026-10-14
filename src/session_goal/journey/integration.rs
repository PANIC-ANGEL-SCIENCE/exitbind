use super::*;

#[derive(Clone, Debug)]
struct Contract {
    header: Header,
    members: Vec<CoveredMember>,
}

/// A Work is either a context-only journey member or the explicitly selected
/// integration proof for the exact snapshot in its frozen start basis.
pub(crate) fn append_work(
    record: &mut Value,
    loaded: &Loaded,
    request: WorkLink<'_>,
) -> Result<(), String> {
    let WorkLink {
        work,
        start_event_sha256,
        session,
        owner,
        ..
    } = request;
    let Some(mut journey) = state(record)? else {
        return Ok(());
    };
    if journey.members.len() >= MAX_MEMBERS && journey.integrations.len() >= MAX_INTEGRATIONS {
        return Err("native journey Work bound reached; no new membership was recorded".into());
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work ledger has no start event")?;
    if start["action"] != "start" || start["eventSha256"] != start_event_sha256 {
        return Err("Work start identity changed before native journey linkage".into());
    }
    if let Some(contract) = parse_contract(start)? {
        validate_header_for_loaded(loaded, &contract.header)?;
        if contract.header.host != session.host
            || contract.header.session_key != session.key
            || contract.header.owner_identity_sha256 != owner.identity_sha256
            || contract.header.owner_project_id != owner.project_id
        {
            return Err("journey integration belongs to another native session or owner".into());
        }
        let expected = covered_snapshot(loaded, &journey.members, &contract.header)?;
        if expected != contract.members {
            return Err(
                "journey integration snapshot changed before its Work start was linked".into(),
            );
        }
        let member = new_member(loaded, request)?;
        if journey.integrations.len() >= MAX_INTEGRATIONS
            || journey
                .integrations
                .iter()
                .any(|entry| entry.member.work == work)
        {
            return Err("native journey integration bound or identity is invalid".into());
        }
        journey.integrations.push(IntegrationEntry {
            member,
            contract_sha256: contract_sha256(&contract),
        });
    } else {
        let member = new_member(loaded, request)?;
        if journey.members.len() >= MAX_MEMBERS
            || journey.members.iter().any(|item| item.work == work)
            || journey
                .integrations
                .iter()
                .any(|entry| entry.member.work == work)
        {
            return Err("native journey member bound or identity is invalid".into());
        }
        journey.members.push(member);
    }
    write_state(record, journey);
    Ok(())
}

fn validate_header_for_loaded(loaded: &Loaded, header: &Header) -> Result<(), String> {
    let scope = super::super::native::scope(loaded, false)?;
    let session = scope
        .session
        .ok_or("journey integration requires a current native session identity")?;
    let owner = scope
        .owner
        .ok_or("journey integration requires the fixed native session owner")?;
    let local = super::super::native::owner_for(loaded)?;
    if header.protocol != PROTOCOL
        || !matches!(header.host.as_str(), "codex" | "claude")
        || !valid_sha(&header.session_key)
        || !valid_sha(&header.owner_identity_sha256)
        || !(header.owner_project_id.is_none()
            || header
                .owner_project_id
                .as_ref()
                .is_some_and(|id| !id.trim().is_empty() && id.len() <= 128))
        || !valid_sha(&header.project_identity_sha256)
        || !valid_sha(&header.local_identity_sha256)
        || !valid_sha(&header.config_binding_sha256)
        || !valid_sha(&header.checker.sha256)
        || header.checker.path.trim().is_empty()
        || header.checker.path.len() > 256
        || header.checker.path.contains('\0')
        || header.host != session.host
        || header.session_key != session.key
        || header.owner_identity_sha256 != owner.identity_sha256
        || header.owner_project_id != owner.project_id
        || header.project_identity_sha256 != super::super::requirements::identity(loaded)?
        || header.local_identity_sha256 != local.identity_sha256
        || header.config_binding_sha256 != config_binding(loaded)?
    {
        return Err("journey integration scope or configured checker binding changed".into());
    }
    Ok(())
}

pub(super) fn marker(prefix: &str, value: &impl Serialize) -> Result<String, String> {
    let marker = format!(
        "{prefix}{}",
        serde_json::to_string(value).map_err(|error| error.to_string())?
    );
    if marker.len() > MAX_MARKER_BYTES {
        return Err("journey integration marker exceeds the frozen 1024-byte basis bound".into());
    }
    Ok(marker)
}

fn reject_reserved_constraints(basis: &Value) -> Result<(), String> {
    let constraints = basis["constraints"]
        .as_array()
        .ok_or("basis constraints are malformed")?;
    if constraints.iter().any(|item| {
        item.as_str()
            .is_some_and(|text| text.starts_with(RESERVED_PREFIX))
    }) {
        return Err("journey integration scope must be selected through --journey-integration, not an injected basis marker".into());
    }
    Ok(())
}

/// Refuse caller-controlled journey markers at the shared Work-start boundary.
pub(crate) fn reject_unprepared_markers(basis_text: &str) -> Result<(), String> {
    let basis = crate::kernel::basis::parse_basis_text(basis_text, "basis")?.value();
    reject_reserved_constraints(&basis)
}

fn parse_contract(start: &Value) -> Result<Option<Contract>, String> {
    let Some(basis) = start.get("basis") else {
        return Ok(None);
    };
    let constraints = basis["constraints"]
        .as_array()
        .ok_or("Work basis constraints are malformed")?;
    let mut header_markers = Vec::new();
    let mut member_markers = Vec::new();
    for value in constraints.iter().filter_map(Value::as_str) {
        if value.starts_with(RESERVED_PREFIX) && value.len() > MAX_MARKER_BYTES {
            return Err("journey integration marker exceeds its bounded size".into());
        }
        if let Some(raw) = value.strip_prefix(HEADER_PREFIX) {
            header_markers.push(raw);
        } else if let Some(raw) = value.strip_prefix(MEMBER_PREFIX) {
            member_markers.push(raw);
        } else if value.starts_with(RESERVED_PREFIX) {
            return Err("unsupported journey integration marker".into());
        }
    }
    if header_markers.is_empty() && member_markers.is_empty() {
        return Ok(None);
    }
    if header_markers.len() != 1 || member_markers.is_empty() || member_markers.len() > MAX_MEMBERS
    {
        return Err(
            "journey integration header/member markers are missing, duplicated or oversized".into(),
        );
    }
    let header: Header = serde_json::from_str(header_markers[0])
        .map_err(|_| "journey integration header is malformed or has unknown fields")?;
    let mut members = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in member_markers {
        let member: CoveredMember = serde_json::from_str(raw)
            .map_err(|_| "journey integration member is malformed or has unknown fields")?;
        if !valid_work(&member.work)
            || !valid_sha(&member.member_sha256)
            || !valid_sha(&member.accepted_endpoint_sha256)
            || !seen.insert(member.work.clone())
        {
            return Err("journey integration member identity is invalid or duplicated".into());
        }
        members.push(member);
    }
    members.sort_by(|a, b| a.work.cmp(&b.work));
    Ok(Some(Contract { header, members }))
}

fn contract_sha256(contract: &Contract) -> String {
    hash::value(&json!({"header":&contract.header,"members":&contract.members}))
}

fn member_contract_marker(contract: &Contract) -> Result<Vec<String>, String> {
    let mut result = vec![marker(HEADER_PREFIX, &contract.header)?];
    result.extend(
        contract
            .members
            .iter()
            .map(|member| marker(MEMBER_PREFIX, member))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(result)
}

fn current_scope(
    loaded: &Loaded,
) -> Result<
    (
        super::super::native::Scope,
        super::super::native::Session,
        super::super::native::Owner,
    ),
    String,
> {
    let scope = super::super::native::scope(loaded, false)?;
    let session = scope
        .session
        .clone()
        .ok_or("journey integration requires a current native session identity")?;
    let owner = scope
        .owner
        .clone()
        .ok_or("journey integration requires an incorporated current session Goal")?;
    Ok((scope, session, owner))
}

/// Freeze the exact accepted local journey set before creating the integration
/// Work's immutable start event.
pub(crate) fn prepare(
    loaded: &Loaded,
    supplied_basis: Option<&str>,
    checker: &str,
    command: &str,
) -> Result<Prepared, String> {
    if let Some(basis) = supplied_basis {
        reject_unprepared_markers(basis)?;
    }
    let (scope, session, owner) = current_scope(loaded)?;
    let record =
        super::super::read(&scope.root)?.ok_or("no native session Goal is incorporated")?;
    if record["closure"]["closed"] != false {
        return Err("native session Goal is closed; start a new side Work to reopen the journey before integration".into());
    }
    let journey = state(&record)?.ok_or("native session Goal has no explicit journey membership protocol; explicitly incorporate it before integration")?;
    if journey.integrations.len() >= MAX_INTEGRATIONS {
        return Err("native journey integration bound reached; no Work was started".into());
    }
    let project_identity_sha256 = super::super::requirements::identity(loaded)?;
    let local_identity_sha256 = super::super::native::owner_for(loaded)?.identity_sha256;
    let config_binding_sha256 = config_binding(loaded)?;
    let checker = super::super::requirements::checker_input(loaded, checker, command)?;
    let mut covered = covered_snapshot(
        loaded,
        &journey.members,
        &Header {
            protocol: PROTOCOL,
            host: session.host.to_owned(),
            session_key: session.key.clone(),
            owner_identity_sha256: owner.identity_sha256.clone(),
            owner_project_id: owner.project_id.clone(),
            project_identity_sha256: project_identity_sha256.clone(),
            local_identity_sha256: local_identity_sha256.clone(),
            config_binding_sha256: config_binding_sha256.clone(),
            checker: checker.clone(),
        },
    )?;
    covered.sort_by(|a, b| a.work.cmp(&b.work));
    if covered.is_empty() {
        return Err("no native journey members are recorded for this configured project".into());
    }
    let contract = Contract {
        header: Header {
            protocol: PROTOCOL,
            host: session.host.to_owned(),
            session_key: session.key,
            owner_identity_sha256: owner.identity_sha256,
            owner_project_id: owner.project_id,
            project_identity_sha256,
            local_identity_sha256,
            config_binding_sha256,
            checker,
        },
        members: covered,
    };
    validate_header_for_loaded(loaded, &contract.header)?;
    let mut basis = if let Some(basis) = supplied_basis {
        crate::kernel::basis::parse_basis_text(basis, "basis")?.value()
    } else {
        json!({"version":1,"constraints":["Preserve the selected task scope, existing authority and evidence."],
            "openZones":["Implementation within the approved project boundary."],
            "decisiveCases":["A current observed integration check covers the frozen accepted journey members."]})
    };
    basis
        .as_object_mut()
        .ok_or("basis is malformed")?
        .remove("sha256");
    let constraints = basis["constraints"]
        .as_array_mut()
        .ok_or("basis constraints are malformed")?;
    constraints.extend(
        member_contract_marker(&contract)?
            .into_iter()
            .map(Value::String),
    );
    let basis_text = serde_json::to_string(&basis).map_err(|error| error.to_string())?;
    crate::kernel::basis::parse_basis_text(&basis_text, "basis")?;
    Ok(Prepared { basis: basis_text })
}

pub(super) fn validate_member_loaded(
    loaded: &Loaded,
    member: &Member,
    header: &Header,
) -> Result<(), String> {
    if member.host != header.host
        || member.session_key != header.session_key
        || member.owner_identity_sha256 != header.owner_identity_sha256
        || member.owner_project_id != header.owner_project_id
        || member.project_identity_sha256 != super::super::requirements::identity(loaded)?
        || member.local_identity_sha256 != super::super::native::owner_for(loaded)?.identity_sha256
        || member.config_path != canonical_path(&loaded.path, "configured project path")?
        || member.config_sha256 != hash::text(&loaded.source)
        || member.product_root != canonical_path(&loaded.product_root, "configured product root")?
        || member.state_root != canonical_path(&loaded.state_root, "configured state root")?
    {
        return Err(
            "journey member belongs to another or changed project/session configuration".into(),
        );
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct Endpoint {
    pub(super) work: String,
    pub(super) identity_sha256: String,
}

pub(super) fn accepted_endpoint(
    loaded: &Loaded,
    member: &Member,
) -> Result<Option<Endpoint>, String> {
    const MAX_SUCCESSORS: usize = 64;
    let mut current_work = member.work.clone();
    let mut expected_start = Some(member.start_event_sha256.clone());
    let mut expected_claim: Option<Value> = None;
    let mut seen = BTreeSet::new();
    for depth in 0..=MAX_SUCCESSORS {
        let relative = crate::work::resolve(loaded, &current_work)?;
        if !seen.insert(relative.clone()) {
            return Err("journey member successor lineage contains a cycle".into());
        }
        let (ledger, events, source) = crate::run::ledger::load(loaded, &relative)?;
        let start = events
            .first()
            .ok_or("journey Work ledger has no start event")?;
        let start_sha = start["eventSha256"]
            .as_str()
            .ok_or("journey Work start identity is missing")?;
        if start["action"] != "start"
            || expected_start
                .as_deref()
                .is_some_and(|expected| expected != start_sha)
            || start["configSha256"] != member.config_sha256
            || hash::text(start["goal"].as_str().unwrap_or_default()) != member.work_goal_sha256
        {
            return Err(
                "journey Work start, configuration or frozen task Goal identity changed".into(),
            );
        }
        if let Some(claim) = expected_claim.take() {
            validate_successor(&claim, &relative, &ledger, &events)?;
        }
        let state = crate::run::state::reduce(&events)?;
        let accepted = state["status"] == "accepted"
            && events.last().is_some_and(|event| {
                event["action"] == "submit"
                    && event["role"] == "lead"
                    && event["outcome"] == "accepted"
            });
        let claim_path = crate::run::ledger::claim_path(&ledger);
        let Some(claim) = crate::run::ledger::valid_claim(&claim_path)? else {
            if !accepted {
                return Ok(None);
            }
            return Ok(Some(Endpoint {
                work: current_work.clone(),
                identity_sha256: hash::value(&json!({
                    "work":current_work,
                    "startEventSha256":start_sha,
                    "acceptanceEventSha256":events.last().unwrap()["eventSha256"],
                    "goalSha256":member.work_goal_sha256,
                    "configSha256":start["configSha256"],
                    "rawLedgerSha256":hash::bytes(source.as_bytes()),
                })),
            }));
        };
        if depth == MAX_SUCCESSORS {
            return Err("journey member successor lineage exceeds the supported bound".into());
        }
        validate_predecessor_claim(&claim, &relative, &events, &source)?;
        let successor = claim["newLedgerPath"]
            .as_str()
            .ok_or("journey successor claim has no successor ledger")?;
        let work = crate::work::locator_for_ledger(successor)
            .ok_or("journey successor claim names a non-Work ledger")?;
        if crate::work::resolve(loaded, &work)? != successor {
            return Err("journey successor claim does not bind the exact Work ledger path".into());
        }
        current_work = work;
        expected_start = None;
        expected_claim = Some(claim);
    }
    Ok(None)
}

/// Resolve a Work to an existing journey member through verified run
/// supersession claims. This is read-only lineage; it never creates members.
pub(super) fn descends_from_member(
    loaded: &Loaded,
    member: &Member,
    requested_work: &str,
) -> Result<bool, String> {
    const MAX_SUCCESSORS: usize = 64;
    let mut current_work = member.work.clone();
    let mut expected_start = Some(member.start_event_sha256.clone());
    let mut expected_claim: Option<Value> = None;
    let mut seen = BTreeSet::new();
    for depth in 0..=MAX_SUCCESSORS {
        let relative = crate::work::resolve(loaded, &current_work)?;
        if !seen.insert(relative.clone()) {
            return Err("journey member successor lineage contains a cycle".into());
        }
        let (ledger, events, source) = crate::run::ledger::load(loaded, &relative)?;
        let start = events
            .first()
            .ok_or("journey Work ledger has no start event")?;
        if start["action"] != "start"
            || expected_start
                .as_deref()
                .is_some_and(|expected| start["eventSha256"] != expected)
            || start["configSha256"] != member.config_sha256
            || hash::text(start["goal"].as_str().unwrap_or_default()) != member.work_goal_sha256
        {
            return Err(
                "journey Work start, configuration or frozen task Goal identity changed".into(),
            );
        }
        if let Some(claim) = expected_claim.take() {
            validate_successor(&claim, &relative, &ledger, &events)?;
        }
        if current_work == requested_work {
            return Ok(true);
        }
        let claim_path = crate::run::ledger::claim_path(&ledger);
        let Some(claim) = crate::run::ledger::valid_claim(&claim_path)? else {
            return Ok(false);
        };
        if depth == MAX_SUCCESSORS {
            return Err("journey member successor lineage exceeds the supported bound".into());
        }
        validate_predecessor_claim(&claim, &relative, &events, &source)?;
        let successor = claim["newLedgerPath"]
            .as_str()
            .ok_or("journey successor claim has no successor ledger")?;
        let work = crate::work::locator_for_ledger(successor)
            .ok_or("journey successor claim names a non-Work ledger")?;
        if crate::work::resolve(loaded, &work)? != successor {
            return Err("journey successor claim does not bind the exact Work ledger path".into());
        }
        current_work = work;
        expected_start = None;
        expected_claim = Some(claim);
    }
    Ok(false)
}

fn validate_predecessor_claim(
    claim: &Value,
    relative: &str,
    events: &[Value],
    source: &str,
) -> Result<(), String> {
    let start = events
        .first()
        .ok_or("journey predecessor ledger is empty")?;
    let head = events.last().ok_or("journey predecessor ledger is empty")?;
    if claim["oldLedgerPath"] != relative
        || claim["oldLedgerSha256"] != hash::bytes(source.as_bytes())
        || claim["oldRunId"] != start["runId"]
        || claim["oldHeadEventSha256"] != head["eventSha256"]
        || claim["oldConfigSha256"] != start["configSha256"]
        || claim["goalSha256"] != hash::text(start["goal"].as_str().unwrap_or_default())
    {
        return Err(
            "journey successor claim does not bind the exact predecessor or same task Goal".into(),
        );
    }
    Ok(())
}

fn validate_successor(
    claim: &Value,
    relative: &str,
    ledger: &crate::run::ledger::LedgerPath,
    events: &[Value],
) -> Result<(), String> {
    let start = events.first().ok_or("journey successor ledger is empty")?;
    let actual_relative = ledger
        .expected
        .strip_prefix(&ledger.root)
        .map_err(|_| "journey successor escapes its configured StateRoot")?
        .to_str()
        .ok_or("journey successor ledger path is not UTF-8")?;
    if actual_relative != relative
        || claim["newLedgerPath"] != relative
        || claim["newRunId"] != start["runId"]
        || claim["workflow"] != start["workflow"]
        || claim["configSha256"] != start["configSha256"]
        || claim["goalSha256"] != hash::text(start["goal"].as_str().unwrap_or_default())
        || start["supersedes"]["ledgerPath"] != claim["oldLedgerPath"]
        || start["supersedes"]["ledgerSha256"] != claim["oldLedgerSha256"]
        || start["supersedes"]["runId"] != claim["oldRunId"]
        || start["supersedes"]["headEventSha256"] != claim["oldHeadEventSha256"]
        || start["supersedes"]["configSha256"] != claim["oldConfigSha256"]
    {
        return Err("journey successor start does not match its validated same-goal claim".into());
    }
    Ok(())
}

fn covered_snapshot(
    loaded: &Loaded,
    members: &[Member],
    header: &Header,
) -> Result<Vec<CoveredMember>, String> {
    let mut covered = Vec::new();
    let project_identity = super::super::requirements::identity(loaded)?;
    for member in members
        .iter()
        .filter(|member| member.project_identity_sha256 == project_identity)
    {
        validate_member_loaded(loaded, member, header)?;
        let endpoint = accepted_endpoint(loaded, member)?
            .ok_or_else(|| format!("journey member {} has no terminal accepted result; integration cannot create acceptance", member.work))?;
        let evidence = crate::run::result_ref_evidence(loaded, &endpoint.work)?
            .ok_or_else(|| format!("journey member {} evidence is unavailable", member.work))?;
        if !evidence.accepted
            || !evidence.artifact_current
            || evidence.drift.is_some()
            || evidence.inputs_current.is_none()
        {
            return Err(format!(
                "journey member {} has unavailable acceptance/configuration evidence",
                member.work
            ));
        }
        covered.push(CoveredMember {
            work: member.work.clone(),
            member_sha256: member_identity(member),
            accepted_endpoint_sha256: endpoint.identity_sha256,
        });
    }
    covered.sort_by(|a, b| a.work.cmp(&b.work));
    Ok(covered)
}

fn contract_for_work(loaded: &Loaded, work: &str) -> Result<Option<(Contract, Value)>, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events
        .first()
        .ok_or("journey integration Work has no start event")?;
    let Some(contract) = parse_contract(start)? else {
        return Ok(None);
    };
    validate_header_for_loaded(loaded, &contract.header)?;
    Ok(Some((contract, start.clone())))
}

pub(super) fn current_integration_for_project(
    loaded: &Loaded,
    state: &JourneyState,
    members: &[Member],
    project_identity: &str,
) -> Result<Option<String>, String> {
    let project_members = members
        .iter()
        .filter(|member| member.project_identity_sha256 == project_identity)
        .collect::<Vec<_>>();
    if project_members.is_empty() {
        return Ok(None);
    }
    for entry in state
        .integrations
        .iter()
        .filter(|entry| entry.member.project_identity_sha256 == project_identity)
    {
        let member = &entry.member;
        let header_binding = Header {
            protocol: PROTOCOL,
            host: member.host.clone(),
            session_key: member.session_key.clone(),
            owner_identity_sha256: member.owner_identity_sha256.clone(),
            owner_project_id: member.owner_project_id.clone(),
            project_identity_sha256: member.project_identity_sha256.clone(),
            local_identity_sha256: member.local_identity_sha256.clone(),
            config_binding_sha256: config_binding(loaded)?,
            checker: super::super::requirements::CheckerInput {
                path: String::new(),
                sha256: String::new(),
            },
        };
        validate_member_loaded(loaded, member, &header_binding)?;
        let Some((contract, start)) = contract_for_work(loaded, &member.work)? else {
            continue;
        };
        if start["eventSha256"] != member.start_event_sha256
            || hash::text(start["goal"].as_str().unwrap_or_default()) != member.work_goal_sha256
            || contract_sha256(&contract) != entry.contract_sha256
        {
            continue;
        }
        let expected = covered_snapshot(loaded, members, &contract.header)?;
        if expected != contract.members {
            continue;
        }
        let evidence = super::super::requirements::current_checker_evidence(
            loaded,
            &member.work,
            &contract.header.checker,
        )?;
        if evidence["state"] == "current" && evidence["accepted"] == true {
            return Ok(Some(member.work.clone()));
        }
    }
    Ok(None)
}

pub(super) fn project_loaded(loaded: &Loaded, member: &Member) -> Result<Loaded, String> {
    if canonical_path(&loaded.path, "configured project path")? == member.config_path {
        return crate::config::load(Some(member.config_path.as_str()));
    }
    crate::config::load(Some(member.config_path.as_str()))
}
