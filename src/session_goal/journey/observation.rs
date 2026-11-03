//! Bound one Work's journey projection to rechecked lineage and Goal facts.
use super::*;
use crate::run::ledger::observation::Observation;
use std::path::PathBuf;

#[derive(PartialEq, Eq)]
struct ScopeIdentity {
    root: PathBuf,
    session: Option<(&'static str, String, String, PathBuf)>,
    owner: Option<(Option<String>, String)>,
    owner_matches: bool,
}

impl From<&super::super::native::Scope> for ScopeIdentity {
    fn from(scope: &super::super::native::Scope) -> Self {
        Self {
            root: scope.root.clone(),
            session: scope.session.as_ref().map(|session| {
                (
                    session.host,
                    session.key.clone(),
                    session.selector_key.clone(),
                    session.root.clone(),
                )
            }),
            owner: scope
                .owner
                .as_ref()
                .map(|owner| (owner.project_id.clone(), owner.identity_sha256.clone())),
            owner_matches: scope.owner_matches,
        }
    }
}

struct Witness {
    lineage: Observation,
    scope: ScopeIdentity,
    history: Option<String>,
    record: Value,
    close_route_eligible: bool,
    access: super::super::native::Access,
}

impl Witness {
    fn finish(self, loaded: &Loaded) -> Result<(), String> {
        self.lineage.finish(loaded)?;
        crate::work::details::ensure_current_config(loaded)?;
        let scope = super::super::native::scope_with_access(loaded, self.access)?;
        if ScopeIdentity::from(&scope) != self.scope {
            return Err("native Goal owner or session changed during Work projection".into());
        }
        let history = super::super::raw_history(&scope.root)?;
        if history != self.history {
            return Err("native Goal history changed during Work projection".into());
        }
        super::super::records_from_raw(history.as_deref())?;
        Ok(())
    }
}

enum JourneyResult {
    Observed(Readiness),
    Fresh(Option<Value>),
    LocalHistoryUnavailable,
}

pub(in crate::session_goal) struct WorkJourney {
    result: JourneyResult,
    witness: Option<Witness>,
    #[cfg(test)]
    used_fresh: bool,
    #[cfg(test)]
    used_blocked_close_route: std::cell::Cell<bool>,
}

impl WorkJourney {
    pub(in crate::session_goal) fn value(&self) -> Option<Value> {
        match &self.result {
            JourneyResult::Observed(readiness) => Some(readiness.value()),
            JourneyResult::Fresh(value) => value.clone(),
            JourneyResult::LocalHistoryUnavailable => None,
        }
    }

    pub(in crate::session_goal) fn keep_in_projection(&self) -> bool {
        self.witness.is_some() || matches!(&self.result, JourneyResult::LocalHistoryUnavailable)
    }

    /// Reuse only a negative result from this exact Work projection. The
    /// ordinary owner/session gate still runs, and finish rechecks its facts.
    pub(in crate::session_goal) fn blocks_close_route(
        &self,
        loaded: &Loaded,
        root: &Path,
        record: &Value,
    ) -> bool {
        let (JourneyResult::Observed(readiness), Some(witness)) = (&self.result, &self.witness)
        else {
            return false;
        };
        if readiness.state != ReadinessState::Blocked
            || !witness.close_route_eligible
            || witness.record != *record
            || !witness.lineage.same_scope(loaded)
        {
            return false;
        }
        super::readiness::close_scope_with_access(loaded, root, witness.access)
            .ok()
            .flatten()
            .is_some_and(|scope| {
                #[cfg(test)]
                if ScopeIdentity::from(&scope) == witness.scope {
                    self.used_blocked_close_route.set(true);
                }
                ScopeIdentity::from(&scope) == witness.scope
            })
    }

    #[cfg(test)]
    pub(in crate::session_goal) fn used_fresh(&self) -> bool {
        self.used_fresh
    }

    #[cfg(test)]
    pub(in crate::session_goal) fn used_blocked_close_route(&self) -> bool {
        self.used_blocked_close_route.get()
    }

    pub(in crate::session_goal) fn finish(self, loaded: &Loaded) -> Result<(), String> {
        let Some(witness) = self.witness else {
            return Ok(());
        };
        witness.finish(loaded)
    }
}

fn fresh(loaded: &Loaded, record: &Value) -> WorkJourney {
    WorkJourney {
        result: JourneyResult::Fresh(super::readiness::progress_for_record_readonly(
            Some(loaded),
            record,
        )),
        witness: None,
        #[cfg(test)]
        used_fresh: true,
        #[cfg(test)]
        used_blocked_close_route: std::cell::Cell::new(false),
    }
}

fn absent_before_observation() -> WorkJourney {
    WorkJourney {
        result: JourneyResult::LocalHistoryUnavailable,
        witness: None,
        #[cfg(test)]
        used_fresh: false,
        #[cfg(test)]
        used_blocked_close_route: std::cell::Cell::new(false),
    }
}

/// Discover the selected Work's chain in the original member traversal.
/// Unsupported shapes take the unchanged reader after rechecking the facts
/// already observed; ordinary validation errors keep their original result.
pub(in crate::session_goal) fn observe_work(
    loaded: &Loaded,
    record: &Value,
    work: &str,
    local_goal: bool,
) -> Result<WorkJourney, String> {
    if !cfg!(unix) {
        return Ok(fresh(loaded, record));
    }
    let access = super::super::native::Access::ReadExisting;
    let association = match super::readiness::belongs_to_current_session(loaded, record, access) {
        Ok(association) => association,
        // A separately bound local Goal keeps its legacy absent-journey view
        // when the unrelated ambient native history cannot first be read.
        Err(super::readiness::SessionLookupError::History(_)) if local_goal => {
            return Ok(absent_before_observation());
        }
        Err(error) => return Err(error.into_message()),
    };
    let Some((scope, current)) = association else {
        return Ok(fresh(loaded, record));
    };
    if super::super::association::public_record(record) != current {
        return Ok(fresh(loaded, record));
    }
    let history = super::super::raw_history(&scope.root)?;
    let records = super::super::records_from_raw(history.as_deref())?;
    if records.last() != Some(&current) {
        return Err("native Goal changed during Work projection".into());
    }
    let selected = crate::work::resolve(loaded, work)?;
    let journey_state = state(&current)?;
    let close_route_eligible = journey_state
        .as_ref()
        .is_some_and(|state| state.protocol >= MEMBERSHIP_PROTOCOL && state.admissions.is_empty());
    let mut lineage = Observation::new(loaded);
    let readiness = super::readiness::readiness_for_state_inner(
        loaded,
        Some(&current),
        journey_state,
        Some(&mut lineage),
        Some(&selected),
        access,
    );
    lineage.seal_for(&selected);
    let fallback = lineage.fallback();
    let failed = lineage.failed();
    let witness = Witness {
        lineage,
        scope: ScopeIdentity::from(&scope),
        history,
        record: record.clone(),
        close_route_eligible,
        access,
    };
    if fallback || failed {
        witness.finish(loaded)?;
        return if fallback {
            Ok(fresh(loaded, record))
        } else {
            Ok(WorkJourney {
                result: JourneyResult::Fresh(Some(readiness.value())),
                witness: None,
                #[cfg(test)]
                used_fresh: false,
                #[cfg(test)]
                used_blocked_close_route: std::cell::Cell::new(false),
            })
        };
    }
    Ok(WorkJourney {
        result: JourneyResult::Observed(readiness),
        witness: Some(witness),
        #[cfg(test)]
        used_fresh: false,
        #[cfg(test)]
        used_blocked_close_route: std::cell::Cell::new(false),
    })
}
