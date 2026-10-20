use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub(in crate::session_goal::journey) struct Endpoint {
    pub(in crate::session_goal::journey) work: String,
    pub(in crate::session_goal::journey) identity_sha256: String,
}

/// Walk backward from the requested Work through its exact predecessor claims.
/// This selects likely original members without opening unrelated historical
/// project configurations. A matched member still goes through the complete
/// forward validation in `descends_from_member` below.
pub(in crate::session_goal::journey) fn ancestors(
    selected: &Loaded,
    requested_work: &str,
) -> Result<BTreeSet<String>, String> {
    const MAX_SUCCESSORS: usize = 64;
    let mut current_work = requested_work.to_owned();
    let mut seen = BTreeSet::new();
    let mut ancestors = BTreeSet::new();
    for depth in 0..=MAX_SUCCESSORS {
        let relative = crate::work::resolve(selected, &current_work)?;
        if !seen.insert(relative.clone()) {
            return Err("journey member successor lineage contains a cycle".into());
        }
        ancestors.insert(current_work.clone());
        let (ledger, events, _) = crate::run::ledger::load(selected, &relative)?;
        let start = events
            .first()
            .ok_or("journey Work ledger has no start event")?;
        if start["action"] != "start" || start["eventSha256"].as_str().is_none() {
            return Err("journey Work start or predecessor identity is invalid".into());
        }
        if start["supersedes"].is_null() {
            return Ok(ancestors);
        }
        if depth == MAX_SUCCESSORS {
            return Err("journey member successor lineage exceeds the supported bound".into());
        }
        let predecessor_relative = start["supersedes"]["ledgerPath"]
            .as_str()
            .ok_or("journey successor start has no exact predecessor ledger")?;
        let predecessor_work = crate::work::locator_for_ledger(predecessor_relative)
            .ok_or("journey successor start names a non-Work predecessor")?;
        if crate::work::resolve(selected, &predecessor_work)? != predecessor_relative {
            return Err("journey successor start names a noncanonical predecessor ledger".into());
        }
        let (predecessor, predecessor_events, predecessor_source) =
            crate::run::ledger::load(selected, predecessor_relative)?;
        let claim_path = crate::run::ledger::claim_path(&predecessor);
        let claim = crate::run::ledger::valid_claim(&claim_path)?
            .ok_or("journey successor predecessor has no valid claim")?;
        validate_predecessor_claim(
            &claim,
            predecessor_relative,
            &predecessor_events,
            &predecessor_source,
        )?;
        validate_successor(&claim, &relative, &ledger, &events)?;
        ancestors.insert(predecessor_work.clone());
        current_work = predecessor_work;
    }
    unreachable!("bounded lineage loop returns or errors")
}

/// Resolve the accepted end of a member's verified same-Goal successor chain.
/// The saved member configuration and the explicitly selected current
/// configuration are the only configuration identities accepted in a chain.
pub(in crate::session_goal::journey) fn accepted_endpoint(
    selected: &Loaded,
    member: &Member,
) -> Result<Option<Endpoint>, String> {
    const MAX_SUCCESSORS: usize = 64;
    let original = super::project_loaded(selected, member)?;
    let original_config_sha256 = hash::text(&original.source);
    let selected_config_sha256 = hash::text(&selected.source);
    let mut current_work = member.work.clone();
    let mut expected_start = Some(member.start_event_sha256.clone());
    let mut expected_claim: Option<Value> = None;
    let mut seen = BTreeSet::new();
    for depth in 0..=MAX_SUCCESSORS {
        let relative = crate::work::resolve(selected, &current_work)?;
        if !seen.insert(relative.clone()) {
            return Err("journey member successor lineage contains a cycle".into());
        }
        let (ledger, events, source) = crate::run::ledger::load(selected, &relative)?;
        let start = events
            .first()
            .ok_or("journey Work ledger has no start event")?;
        let start_sha = start["eventSha256"]
            .as_str()
            .ok_or("journey Work start identity is missing")?;
        let known_config = start["configSha256"] == original_config_sha256
            || start["configSha256"] == selected_config_sha256;
        if start["action"] != "start"
            || expected_start
                .as_deref()
                .is_some_and(|expected| expected != start_sha)
            || !known_config
            || (expected_start.is_some() && start["configSha256"] != member.config_sha256)
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
            if !accepted || start["configSha256"] != selected_config_sha256 {
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
        if crate::work::resolve(selected, &work)? != successor {
            return Err("journey successor claim does not bind the exact Work ledger path".into());
        }
        current_work = work;
        expected_start = None;
        expected_claim = Some(claim);
    }
    Ok(None)
}

/// Resolve an in-flight or completed Work only through exact saved lineage.
/// This is navigation evidence and never adds membership or acceptance.
/// `requested_ancestors` is `ancestors(selected, requested_work)`; it does not
/// depend on the member, so a caller checking several members computes it once.
pub(in crate::session_goal::journey) fn descends_from_member(
    selected: &Loaded,
    member: &Member,
    requested_work: &str,
    requested_ancestors: &BTreeSet<String>,
) -> Result<bool, String> {
    const MAX_SUCCESSORS: usize = 64;
    if !requested_ancestors.contains(&member.work) {
        return Ok(false);
    }
    let original = super::project_loaded(selected, member)?;
    let original_config_sha256 = hash::text(&original.source);
    let selected_config_sha256 = hash::text(&selected.source);
    let mut current_work = member.work.clone();
    let mut expected_start = Some(member.start_event_sha256.clone());
    let mut expected_claim: Option<Value> = None;
    let mut seen = BTreeSet::new();
    for depth in 0..=MAX_SUCCESSORS {
        let relative = crate::work::resolve(selected, &current_work)?;
        if !seen.insert(relative.clone()) {
            return Err("journey member successor lineage contains a cycle".into());
        }
        let (ledger, events, source) = crate::run::ledger::load(selected, &relative)?;
        let start = events
            .first()
            .ok_or("journey Work ledger has no start event")?;
        let known_config = start["configSha256"] == original_config_sha256
            || start["configSha256"] == selected_config_sha256;
        if start["action"] != "start"
            || expected_start
                .as_deref()
                .is_some_and(|expected| start["eventSha256"] != expected)
            || !known_config
            || (expected_start.is_some() && start["configSha256"] != member.config_sha256)
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
            return Ok(start["configSha256"] == selected_config_sha256);
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
        if crate::work::resolve(selected, &work)? != successor {
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
