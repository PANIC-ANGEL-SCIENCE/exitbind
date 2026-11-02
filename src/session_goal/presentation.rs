//! Validated overall-goal progress and automatic terminal presentation.

use super::*;
use adoption::Reason;

pub(crate) fn presentation(value: Option<&Value>) -> Value {
    let Some(record) = value else {
        return json!({"requestId":"unknown","explicitLeadClosure":false,"subgoals":["session goal not explicitly incorporated"],"findings":[],"blockers":[],"decisions":[],"externalActions":[]});
    };
    let request_id = format!(
        "{}:r{}",
        record["goalId"].as_str().unwrap_or("unknown"),
        record["revision"].as_u64().unwrap_or(0)
    );
    let closure_has_evidence = if record["closure"]["kind"] == "direct" {
        record["closure"]["inputsSha256"].is_string()
    } else {
        record["closure"]["resultRefs"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty() && refs.iter().all(Value::is_string))
    };
    let closure_current = record["closure"]["closed"] == true
        && record["closure"]["revision"] == record["revision"]
        && !unresolved(record)
        && closure_has_evidence;
    json!({
        "requestId":request_id,
        "explicitLeadClosure":closure_current,
        "completionMode": if record["closure"]["kind"] == "direct" { "direct" } else { "governed" },
        "subgoals":pending_items(record, "obligations", "subgoals"),
        "findings":pending_items(record, "findings", "findings"),
        "blockers":pending_items(record, "blockers", "blockers"),
        "decisions":pending_items(record, "decisions", "decisions"),
        "externalActions":pending_items(record, "externalActions", "external actions"),
        "source":record["source"],
        "revision":record["revision"]
    })
}

/// Display a cross-project native Goal without importing its closure or
/// owner evidence into the caller's configured project.
pub(crate) fn context_summary(
    loaded: &crate::config::Loaded,
    value: Option<&Value>,
    owner: Option<&super::native::Owner>,
) -> Value {
    let Some(record) = value else {
        return json!({
            "contextOnly":true,
            "authority":"none",
            "ownerRequired":true,
            "goalProgress":adoption::unavailable(loaded,None,Reason::Unavailable,"no native session Goal has been incorporated"),
        });
    };
    let goal_id = record["goalId"].as_str().unwrap_or_default();
    let progress = progress::context_only(record, None);
    json!({
        "contextOnly":true,
        "authority":"none",
        "ownerRequired":true,
        "goalIdSha256":crate::evidence::hash::text(goal_id),
        "goal":record["goal"],
        "revision":record["revision"],
        "ownerProjectId":owner.and_then(|value|value.project_id.clone()),
        "goalProgress":progress,
    })
}

fn pending_items(record: &Value, key: &str, label: &str) -> Vec<Value> {
    let unavailable = || vec![json!(format!("session goal {label} unavailable"))];
    let Some(category) = record["categories"][key].as_str() else {
        return unavailable();
    };
    if !matches!(category, "considered" | "none_applicable") {
        return unavailable();
    }
    let Some(items) = record[key].as_array() else {
        return unavailable();
    };
    let mut pending = Vec::new();
    for item in items {
        match item["disposition"].as_str() {
            Some("open") => {
                let value = if key == "externalActions" {
                    item["action"].as_str()
                } else {
                    item["text"].as_str()
                };
                let Some(value) = value else {
                    return unavailable();
                };
                pending.push(json!(value));
            }
            Some("accepted" | "outside_scope" | "successor" | "direct") => {}
            _ => return unavailable(),
        }
    }
    pending
}

pub(crate) fn presentation_for_loaded(
    loaded: &crate::config::Loaded,
    value: Option<&Value>,
) -> Result<Value, String> {
    presentation_for_selected(loaded, value, &loaded.state_root, false)
}

pub(crate) fn presentation_for_selected(
    loaded: &crate::config::Loaded,
    value: Option<&Value>,
    root: &std::path::Path,
    project_selected: bool,
) -> Result<Value, String> {
    let mut rendered = presentation(value);
    let readiness = current::evaluate(loaded, value, root, project_selected);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    rendered["goalProgress"] =
        progress::project_selected(loaded, root, project_selected, value, &rendered, None);
    rendered["terminal"] = json!(crate::presentation_events::whole_goal_terminal(&rendered));
    Ok(rendered)
}

/// Preserve standalone Work display while a bound goal owns its completion.
/// An accepted part cannot claim the overall goal, and stale/unknown closure
/// cannot display readiness. No canonical record is changed by this route.
pub(crate) fn terminal_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    standalone: Option<&'static str>,
) -> Result<Option<&'static str>, String> {
    if standalone.is_none() {
        return Ok(None);
    }
    let association = super::association::context_for_work_with_root(loaded, work);
    let (record, goal_root) = match association.context {
        super::association::WorkContext::Owner(record) => {
            let Some(root) = association.owner_root else {
                return Ok(None);
            };
            (Some(record), root)
        }
        super::association::WorkContext::Session(_)
        | super::association::WorkContext::NotRequested
        | super::association::WorkContext::Unavailable
        | super::association::WorkContext::Invalid => return Ok(standalone),
        super::association::WorkContext::Local => {
            (read(&loaded.state_root)?, loaded.state_root.clone())
        }
    };
    let Some(record) = record else {
        return Ok(standalone);
    };
    if !progress::record_bound_to_work_with_lineage(loaded, &record, work)? {
        return Ok(standalone);
    }
    let rendered = presentation_for_selected(loaded, Some(&record), &goal_root, false)?;
    Ok(crate::presentation_events::whole_goal_terminal(&rendered))
}

pub(crate) fn progress_for_loaded(
    loaded: &crate::config::Loaded,
    result: &Value,
) -> Result<Value, String> {
    let record = read(&loaded.state_root)?;
    let mut rendered = presentation(record.as_ref());
    let readiness = current::evaluate(loaded, record.as_ref(), &loaded.state_root, false);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project(
        Some(loaded),
        record.as_ref(),
        &rendered,
        Some(result),
    ))
}

/// One verified Goal association for one Work assembly. It is never stored or
/// carried across the independent fresh reads that validate a delivery.
pub(crate) struct WorkGoalObservation<'a> {
    loaded: &'a crate::config::Loaded,
    work: &'a str,
    context: super::association::WorkContext,
    owner_root: Option<std::path::PathBuf>,
    local_record: Option<Result<Option<Value>, String>>,
    journey: progress::JourneyObservation,
}

impl<'a> WorkGoalObservation<'a> {
    pub(crate) fn acquire(loaded: &'a crate::config::Loaded, work: &'a str) -> Self {
        Self::from_resolution(
            loaded,
            work,
            super::association::context_for_work_with_root(loaded, work),
        )
    }

    fn from_resolution(
        loaded: &'a crate::config::Loaded,
        work: &'a str,
        association: super::association::WorkContextResolution,
    ) -> Self {
        let local_goal = matches!(&association.context, super::association::WorkContext::Local);
        let local_record = local_goal.then(|| read(&loaded.state_root));
        Self {
            loaded,
            work,
            context: association.context,
            owner_root: association.owner_root,
            local_record,
            journey: progress::JourneyObservation::for_work(work, local_goal),
        }
    }

    pub(crate) fn progress(&mut self, result: &Value) -> Result<Value, String> {
        let value = self.project(result, false)?;
        self.journey.status()?;
        Ok(value)
    }

    pub(crate) fn detail(&mut self, result: &Value) -> Result<Value, String> {
        let value = self.project(result, true)?;
        self.journey.status()?;
        Ok(value)
    }

    pub(crate) fn finish(self) -> Result<(), String> {
        self.journey.finish(self.loaded)
    }

    #[cfg(test)]
    pub(crate) fn has_journey_observation(&self) -> bool {
        self.journey.has_observation()
    }

    #[cfg(test)]
    pub(crate) fn used_blocked_close_route(&self) -> bool {
        self.journey.used_blocked_close_route()
    }

    fn project(&mut self, result: &Value, detail: bool) -> Result<Value, String> {
        let loaded = self.loaded;
        let work = self.work;
        let journey = &mut self.journey;
        let mut from_record = |record: Option<&Value>, root: &std::path::Path| {
            if detail {
                progress_detail_from_record(loaded, work, result, record, root, journey)
            } else {
                progress_from_record(loaded, work, result, record, root, journey)
            }
        };
        match &self.context {
            super::association::WorkContext::NotRequested => Ok(progress::not_requested(result)),
            super::association::WorkContext::Owner(record) => {
                let Some(root) = self.owner_root.as_deref() else {
                    return Ok(adoption::unavailable(
                        loaded,
                        Some(result),
                        Reason::ContextInvalid,
                        "verified native session Goal root is unavailable",
                    ));
                };
                from_record(Some(record), root)
            }
            super::association::WorkContext::Session(record) => {
                Ok(context_progress(record, work, result, detail))
            }
            super::association::WorkContext::Unavailable => Ok(adoption::unavailable(
                loaded,
                Some(result),
                Reason::Unavailable,
                "no verified native session Goal association",
            )),
            super::association::WorkContext::Invalid => Ok(adoption::unavailable(
                loaded,
                Some(result),
                Reason::ContextInvalid,
                "native session Goal context could not be verified",
            )),
            super::association::WorkContext::Local => {
                let record = self
                    .local_record
                    .as_ref()
                    .ok_or("local Goal observation is unavailable")?
                    .as_ref()
                    .map_err(Clone::clone)?;
                from_record(record.as_ref(), &loaded.state_root)
            }
        }
    }

    pub(crate) fn record(&self) -> Option<Value> {
        match &self.context {
            super::association::WorkContext::Owner(record)
            | super::association::WorkContext::Session(record) => {
                Some(super::association::public_record(record))
            }
            super::association::WorkContext::Local => self
                .local_record
                .as_ref()
                .and_then(|record| record.as_ref().ok())
                .cloned()
                .flatten(),
            super::association::WorkContext::NotRequested
            | super::association::WorkContext::Unavailable
            | super::association::WorkContext::Invalid => None,
        }
    }
}

/// Project progress for one Work while enforcing its Goal association.
pub(crate) fn progress_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
) -> Result<Value, String> {
    let mut observation = WorkGoalObservation::acquire(loaded, work);
    let value = observation.progress(result)?;
    observation.finish()?;
    Ok(value)
}

fn progress_from_record(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
    record: Option<&Value>,
    root: &std::path::Path,
    journey: &mut progress::JourneyObservation,
) -> Result<Value, String> {
    let mut rendered = presentation(record);
    let readiness = current::evaluate(loaded, record, root, false);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project_for_work(
        loaded,
        work,
        root,
        record,
        &rendered,
        Some(result),
        journey,
    ))
}

/// Full task projection for an already bound Work. Callers must paginate or
/// group the returned task data before placing it on a bounded response.
pub(crate) fn progress_detail_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
) -> Result<Value, String> {
    let mut observation = WorkGoalObservation::acquire(loaded, work);
    let value = observation.detail(result)?;
    observation.finish()?;
    Ok(value)
}

fn progress_detail_from_record(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
    record: Option<&Value>,
    root: &std::path::Path,
    journey: &mut progress::JourneyObservation,
) -> Result<Value, String> {
    let mut rendered = presentation(record);
    let readiness = current::evaluate(loaded, record, root, false);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project_detail_for_work(
        loaded,
        work,
        root,
        record,
        &rendered,
        Some(result),
        journey,
    ))
}

fn context_progress(record: &Value, work: &str, result: &Value, detail: bool) -> Value {
    let mut view = progress::context_only(record, Some(result));
    if detail {
        view["detailRoute"] = json!({
            "state":"preview",
            "reason":"owner_project_evidence_required",
        });
    }
    if let Some(association) = super::association::for_work(record, work) {
        view["workAssociation"] = json!({
            "kind":"context_only",
            "protocol":super::association::PROTOCOL,
            "work":association["work"],
            "startEventSha256":association["startEventSha256"],
        });
    }
    view["goalIdentitySha256"] = json!(crate::evidence::hash::value(&record["goalId"]));
    view
}

pub(crate) fn record_for_work(loaded: &crate::config::Loaded, work: &str) -> Option<Value> {
    WorkGoalObservation::acquire(loaded, work).record()
}

/// Preserve a separately selected project-local named contract when the
/// current native session also supplies its independent parent Goal context.
pub(crate) fn project_goal_progress_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
) -> Result<Option<Value>, String> {
    // Legacy named Works are project scoped too. The named-scope companion
    // selects project or session independently from the optional parent
    // context; absence of a parent companion must not hide a local contract.
    super::requirements::project_progress_for_work(loaded, work)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_goal::association::{WorkContext, WorkContextResolution};

    #[test]
    fn presentation_does_not_close_from_ready() {
        let value = json!({"goalId":"a","revision":1,"categories":{"obligations":"none_applicable","findings":"none_applicable","blockers":"none_applicable","decisions":"none_applicable","externalActions":"none_applicable"},"obligations":[],"findings":[],"blockers":[],"decisions":[],"externalActions":[],"closure":{"closed":false,"revision":null,"resultRefs":[]}});
        assert_eq!(presentation(Some(&value))["explicitLeadClosure"], false);
    }

    #[test]
    fn work_observation_preserves_each_goal_context_projection() {
        let root = crate::test_root::temp("goal-observation-variants");
        let config = crate::project::onboarding::init_with_options(
            crate::project::onboarding::InitOptions {
                product_root: root.to_str().unwrap(),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            },
        )
        .unwrap();
        let loaded = crate::config::load(config.to_str()).unwrap();
        let work = "observation-fixture-work";
        let record = crate::session_goal::incorporate(
            &loaded.state_root,
            &loaded,
            work,
            "Complete the fixture Goal",
            Some("A required task"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("findings,blockers,decisions,externalActions"),
            false,
        )
        .unwrap();
        let result = json!({"state":"running","completed":0});
        let observe = |context, owner_root| {
            WorkGoalObservation::from_resolution(
                &loaded,
                work,
                WorkContextResolution {
                    context,
                    owner_root,
                },
            )
        };
        let mut owner = observe(WorkContext::Owner(record.clone()), Some(root.to_path_buf()));
        assert_eq!(
            owner.progress(&result).unwrap(),
            progress_from_record(
                &loaded,
                work,
                &result,
                Some(&record),
                &root,
                &mut progress::JourneyObservation::default()
            )
            .unwrap()
        );
        assert_eq!(
            owner.detail(&result).unwrap(),
            progress_detail_from_record(
                &loaded,
                work,
                &result,
                Some(&record),
                &root,
                &mut progress::JourneyObservation::default()
            )
            .unwrap()
        );
        assert_eq!(owner.record(), Some(association::public_record(&record)));

        let mut session = observe(WorkContext::Session(record.clone()), None);
        assert_eq!(
            session.progress(&result).unwrap(),
            context_progress(&record, work, &result, false)
        );
        assert_eq!(
            session.detail(&result).unwrap(),
            context_progress(&record, work, &result, true)
        );
        assert_eq!(session.record(), Some(association::public_record(&record)));

        let mut local = observe(WorkContext::Local, None);
        assert_eq!(
            local.progress(&result).unwrap(),
            progress_from_record(
                &loaded,
                work,
                &result,
                Some(&record),
                &loaded.state_root,
                &mut progress::JourneyObservation::default()
            )
            .unwrap()
        );
        assert_eq!(
            local.detail(&result).unwrap(),
            progress_detail_from_record(
                &loaded,
                work,
                &result,
                Some(&record),
                &loaded.state_root,
                &mut progress::JourneyObservation::default()
            )
            .unwrap()
        );
        assert_eq!(local.record(), Some(record.clone()));

        let mut absent = observe(WorkContext::NotRequested, None);
        assert_eq!(
            absent.progress(&result).unwrap(),
            progress::not_requested(&result)
        );
        assert_eq!(
            absent.detail(&result).unwrap(),
            progress::not_requested(&result)
        );
        assert!(absent.record().is_none());
        for (context, reason, description) in [
            (
                WorkContext::Unavailable,
                Reason::Unavailable,
                "no verified native session Goal association",
            ),
            (
                WorkContext::Invalid,
                Reason::ContextInvalid,
                "native session Goal context could not be verified",
            ),
        ] {
            let mut observation = observe(context, None);
            let expected = adoption::unavailable(&loaded, Some(&result), reason, description);
            assert_eq!(observation.progress(&result).unwrap(), expected);
            assert_eq!(observation.detail(&result).unwrap(), expected);
            assert!(observation.record().is_none());
        }
        let mut missing_root = observe(WorkContext::Owner(record.clone()), None);
        let unavailable = adoption::unavailable(
            &loaded,
            Some(&result),
            Reason::ContextInvalid,
            "verified native session Goal root is unavailable",
        );
        assert_eq!(missing_root.progress(&result).unwrap(), unavailable);
        assert_eq!(missing_root.detail(&result).unwrap(), unavailable);
        assert_eq!(
            missing_root.record(),
            Some(association::public_record(&record))
        );

        let journal = root.join(format!(
            "{}/session-goal.jsonl",
            crate::project::layout_types::state_namespace()
        ));
        std::fs::write(journal, "invalid journal\n").unwrap();
        let local_error = read(&loaded.state_root).unwrap_err();
        let mut changed = observe(WorkContext::Local, None);
        assert_eq!(changed.progress(&result).unwrap_err(), local_error);
        assert_eq!(changed.detail(&result).unwrap_err(), local_error);
        assert!(changed.record().is_none());
    }
}
