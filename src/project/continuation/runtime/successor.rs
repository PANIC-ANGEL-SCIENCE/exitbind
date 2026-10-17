//! An existing durable predecessor claim is the only successor authority.
use super::*;

pub(crate) fn prepare_successor(
    loaded: &Loaded,
    old_state: &Value,
    start: &mut Value,
    new_ledger: &str,
) -> Result<Option<Ticket>, String> {
    let Some(bound) = association::for_state(loaded, old_state)? else {
        return Ok(None);
    };
    if !bound.retired {
        return Err("associated successor lacks actual predecessor retirement".into());
    }
    validate_contract(loaded, old_state, &bound.record, &bound.claim)?;
    let marker = bound.verified.association();
    start["projectContinuation"] = serde_json::to_value(marker).map_err(|e| e.to_string())?;
    validate_contract(loaded, start, &bound.record, &bound.claim)?;
    let old_ledger = crate::work::resolve(loaded, &bound.work)?;
    let (old_path, _, _) = ledger::load(loaded, &old_ledger)?;
    let retirement = ledger::valid_claim(&ledger::claim_path(&old_path))?
        .ok_or("successor retirement claim disappeared")?;
    if retirement["newLedgerPath"] != new_ledger
        || retirement["newRunId"] != start["runId"]
        || start["supersedes"]["ledgerPath"] != old_ledger
    {
        return Err("successor differs from the actual retirement claim".into());
    }
    let work = association::ledger_work(new_ledger)?;
    let key = format!("successor-{}", hash::text(new_ledger));
    let intent = Intent {
        kind: Kind::Successor,
        work: work.clone(),
        role: Role::Lead,
        paths: Vec::new(),
        commands: Vec::new(),
        predecessor: Some(bound.work.clone()),
        input_sha256: hash::value(start),
    };
    let selected = journal::prepare(loaded, &bound.relative)?;
    ledger::with_lock(&selected, || {
        let (record, raw) = journal::read(loaded, &bound.relative)?;
        let record = record.ok_or("successor mandate journal disappeared")?;
        let verified = control::prepare_association(
            &policy(&record),
            &marker.task_id,
            &marker.claim_root,
            &work,
        )
        .map_err(refusal)?;
        // A matching committed start is saved recovery, including after a
        // stop. Its original debit must exist; an unclaimed file is not adopted.
        let new_path = ledger::ledger_path(&loaded.state_root, new_ledger, true)?;
        if std::fs::symlink_metadata(&new_path.path).is_ok() {
            let (_, events, _) = ledger::load_at(loaded, &new_path)?;
            if events.first() != Some(&crate::run::state::make_event(start.clone())) {
                return Err(
                    "existing associated successor differs; no adoption or overwrite".into(),
                );
            }
            let old = record
                .account
                .operations
                .iter()
                .find(|old| old.key == key)
                .ok_or("existing successor lacks its aggregate reservation")?;
            if old.intent != intent || old.association != *marker {
                return Err("existing successor reservation conflicts".into());
            }
            if matches!(old.outcome, Outcome::Reserved | Outcome::Unknown) {
                let current = facts::Current::read(loaded, &record, None);
                let observation = control::Observation {
                    operation: &key,
                    intent: &intent,
                    outcome: Outcome::Succeeded,
                    known_ended: true,
                };
                let observed =
                    current.value(Role::Lead, &[], &[], &[], None, None, Some(observation));
                let request = control::Request::Observe(control::Attempt {
                    expected_revision: record.account.revision,
                    key: &key,
                    intent: &intent,
                    association: &verified,
                });
                let Decision::RecordObservation { next: account } =
                    control::evaluate(&policy(&record), &observed, &record.account, &request)
                        .map_err(refusal)?
                else {
                    return Err("unexpected successor recovery observation".into());
                };
                let mut next = record.clone();
                next.account = account;
                journal::append(&selected, next, Some(&record), &raw)?;
            }
            return Ok(None);
        }
        let states = facts::works(loaded, &record.account)?;
        let work_observations = facts::work_values(&states);
        let current = facts::Current::read(loaded, &record, Some(old_state));
        let observed = current.value(
            Role::Lead,
            &[],
            &[],
            &work_observations,
            None,
            Some(&bound.work),
            None,
        );
        let request = control::Request::Attempt(control::Attempt {
            expected_revision: record.account.revision,
            key: &key,
            intent: &intent,
            association: &verified,
        });
        match control::evaluate(&policy(&record), &observed, &record.account, &request)
            .map_err(refusal)?
        {
            Decision::Execute { next } => {
                let mut next_record = record.clone();
                next_record.account = next;
                journal::append(&selected, next_record, Some(&record), &raw)?;
                Ok(Some(Ticket {
                    relative: bound.relative.clone(),
                    association: verified,
                    key,
                    intent,
                }))
            }
            Decision::RecoverExisting { .. } => Err(
                "successor reservation is unresolved; inspect without replaying its start".into(),
            ),
            _ => Err("unexpected successor accounting decision".into()),
        }
    })
}
