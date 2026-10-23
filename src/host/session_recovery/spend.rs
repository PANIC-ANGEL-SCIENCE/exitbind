//! A Lead act spends the saved Lead decisions bound to the Work it acts on:
//! the act records them after the Lead's own text, and they leave this
//! native session's record under its lock. Decisions bound to another Work,
//! or to none, stay.

use super::hint::{read_hint, validate_hint, write_hint_locked, Hint, FILE};
use crate::{evidence::hash, session_goal};

/// Run `act` with the saved Lead decisions bound to `work`, holding the
/// record's lock while a decision is bound to it. When `act` reports them
/// recorded, they are removed from the record before the lock is released.
/// Without a native session, a record or a decision bound to `work`, `act`
/// runs with none and the record is not touched.
pub(crate) fn with_saved_decisions<T>(
    work: Option<&str>,
    act: impl FnOnce(&[String]) -> Result<(T, bool), String>,
) -> Result<T, String> {
    let session = work.and_then(|work| {
        let session = session_goal::native::current().ok().flatten()?;
        let hint = read_hint(&session.root).ok().flatten()?;
        (!bound(&hint, &session, work).is_empty()).then_some(session)
    });
    let (Some(session), Some(work)) = (session, work) else {
        return act(&[]).map(|(value, _)| value);
    };
    let ledger = crate::run::ledger::ledger_path(&session.root, FILE, true)?;
    let mut entered = false;
    let result = crate::run::ledger::with_lock(&ledger, || {
        entered = true;
        let Some(mut hint) = read_hint(&session.root).ok().flatten() else {
            return act(&[]).map(|(value, _)| value);
        };
        let decisions = bound(&hint, &session, work);
        if decisions.is_empty() {
            return act(&[]).map(|(value, _)| value);
        }
        spend(&mut hint, work);
        validate_hint(&hint)?;
        hint.sha256.clear();
        hint.sha256 = hash::value(&serde_json::to_value(&hint).map_err(|error| error.to_string())?);
        let (value, recorded) = act(&decisions)?;
        if recorded {
            // The act is already recorded with these decisions in its reason.
            // A failed write leaves them saved, to be recorded again by the
            // next Lead act on this Work: repeated, never lost. It is
            // reported, and the recorded act stands.
            if let Err(error) = write_hint_locked(&session, &hint) {
                eprintln!(
                    "exitbind: the Lead act on {work} is recorded with {} saved Lead decisions in its reason; they could not be removed from the session recovery record ({error}) and will be recorded again by the next Lead act on this Work",
                    decisions.len()
                );
            }
        }
        Ok(value)
    });
    if entered {
        result
    } else {
        // The lock was not taken: name the store that holds it.
        result.map_err(|_| {
            "the session recovery record is busy or unavailable; no mutation was made".to_owned()
        })
    }
}

/// The decisions of `hint` bound to `work`, oldest first, when the record is
/// this session's own.
fn bound(hint: &Hint, session: &session_goal::native::Session, work: &str) -> Vec<String> {
    if hint.host != session.host || hint.session_key != session.key {
        return Vec::new();
    }
    hint.latest_corrections
        .iter()
        .zip(&hint.latest_correction_works)
        .filter(|(_, bound)| bound.as_deref() == Some(work))
        .map(|(decision, _)| decision.clone())
        .collect()
}

/// Remove the decisions bound to `work`; an all-unbound remainder stores no
/// bindings.
fn spend(hint: &mut Hint, work: &str) {
    if hint.latest_correction_works.is_empty() {
        return;
    }
    let (kept, works): (Vec<_>, Vec<_>) = std::mem::take(&mut hint.latest_corrections)
        .into_iter()
        .zip(std::mem::take(&mut hint.latest_correction_works))
        .filter(|(_, bound)| bound.as_deref() != Some(work))
        .unzip();
    hint.latest_corrections = kept;
    hint.latest_correction_works = if works.iter().all(Option::is_none) {
        Vec::new()
    } else {
        works
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(decisions: &[(&str, Option<&str>)]) -> Hint {
        serde_json::from_value(serde_json::json!({
            "kind": "exitbind_session_recovery_hint", "protocol": 2, "host": "codex",
            "sessionKey": "a".repeat(64), "ownerConfigPath": "/p/exitbind.json",
            "ownerConfigSha256": "b".repeat(64), "ownerProjectId": null,
            "ownerIdentitySha256": "c".repeat(64), "goalId": "goal", "goalRevision": 1,
            "goalEventSha256": "d".repeat(64), "currentStep": "step",
            "currentWork": {"state": "unresolved", "work": null},
            "latestCorrections": decisions.iter().map(|(text, _)| *text).collect::<Vec<_>>(),
            "latestCorrectionWorks": decisions.iter().map(|(_, work)| *work).collect::<Vec<_>>(),
            "evidenceRefs": [], "workRefs": [], "pendingOperation": null,
            "guidanceRefs": [], "lastCheckpoint": null, "sha256": "",
        }))
        .unwrap()
    }

    #[test]
    fn only_the_acted_works_decisions_leave_the_record() {
        let mut record = hint(&[
            ("a", Some("smw_w")),
            ("free", None),
            ("other", Some("smw_x")),
            ("b", Some("smw_w")),
        ]);
        spend(&mut record, "smw_w");
        assert_eq!(record.latest_corrections, ["free", "other"]);
        assert_eq!(
            record.latest_correction_works,
            [None, Some("smw_x".to_owned())]
        );
        validate_hint(&record).unwrap();
        spend(&mut record, "smw_x");
        assert_eq!(record.latest_corrections, ["free"]);
        assert!(record.latest_correction_works.is_empty());
        validate_hint(&record).unwrap();
    }

    #[test]
    fn a_record_without_bindings_keeps_its_old_shape() {
        let record = hint(&[]);
        let value = serde_json::to_value(record).unwrap();
        assert!(value.get("latestCorrectionWorks").is_none(), "{value}");
        let mut uneven = hint(&[("a", None)]);
        uneven.latest_correction_works.push(Some("smw_w".into()));
        assert!(validate_hint(&uneven).is_err());
    }
}
