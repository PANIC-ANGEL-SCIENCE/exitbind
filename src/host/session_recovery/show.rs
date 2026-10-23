//! The `session recovery show` read model: the complete saved hint, its
//! currentness, executable-bound read routes, and the current Works derived
//! from the native Goal's journey.

use super::{
    checkout, current_work,
    hint::{
        canonical_path, current_executable_path, current_executable_sha256, read_hint,
        PendingOperation, WorkSelection, HOOK_BUDGET,
    },
    references::{
        generated_result_info, source_ref_current, verify_pending, verify_source_refs,
        verify_work_refs,
    },
};
use crate::emitted::Route;
use crate::{config, evidence::hash, session_goal};
use serde_json::{json, Value};
use std::time::Instant;

pub(crate) fn show() -> Result<Value, String> {
    let session = session_goal::native::current()?
        .ok_or("session recovery show requires a host-reported native session")?;
    let owner = session
        .owner()?
        .ok_or("native session Goal owner is unavailable")?;
    let record = session_goal::read_for_recovery(&session.root)?
        .ok_or("native session has no canonical Goal to recover")?;
    let hint_result = read_hint(&session.root);
    let (hint, hint_issue) = match hint_result {
        Ok(Some(hint)) if hint.belongs_to(&session, &owner, &record["goalId"]) => {
            (Some(hint), None)
        }
        Ok(Some(_)) => (None, Some("saved hint belongs to another session or Goal")),
        Ok(None) => (None, None),
        Err(_) => (None, Some("saved hint is malformed or unsafe")),
    };
    let mut freshness = json!({"goalCurrent":false,"ownerConfigCurrent":false,"workRefsCurrent":false,"declaredGuidanceSourcesCurrent":false,"pendingEvidenceCurrent":false,"savedStepCurrent":false,"selectedWorkBindingCurrent":false});
    let mut hint_value = Value::Null;
    let mut reference_freshness = Value::Null;
    let mut metadata_current = false;
    let mut dynamic_issue = hint_issue.map(str::to_owned);
    if let Some(hint) = hint.as_ref() {
        hint_value = serde_json::to_value(hint).map_err(|error| error.to_string())?;
        let goal_current = hint.goal_revision == record["revision"].as_u64().unwrap_or(0)
            && hint.goal_event_sha256 == record["eventSha256"].as_str().unwrap_or("");
        freshness["goalCurrent"] = json!(goal_current);
        let owner_check = config::load(Some(&hint.owner_config_path)).and_then(|loaded| {
            if canonical_path(&loaded.path, "native owner configuration")? != hint.owner_config_path
                || hash::text(&loaded.source) != hint.owner_config_sha256
            {
                return Err(crate::drift::refusal(
                    crate::drift::Class::RecoveryOwnerConfiguration,
                    &hint.owner_config_sha256,
                    Some(&hash::text(&loaded.source)),
                    None,
                    crate::drift::Route::RecoveryShow,
                ));
            }
            let scope = session_goal::native::scope(&loaded, false)?;
            if !scope.owner_matches
                || scope
                    .owner
                    .as_ref()
                    .map(|value| value.identity_sha256.as_str())
                    != Some(owner.identity_sha256.as_str())
            {
                return Err(
                    "saved owner configuration no longer matches the fixed Goal owner".into(),
                );
            }
            freshness["ownerConfigCurrent"] = json!(true);
            let work_ok = verify_work_refs(&record, &session, &owner, &hint.work_refs).is_ok();
            let guidance_ok = verify_source_refs(
                &loaded,
                &record,
                &session,
                &owner,
                &hint.work_refs,
                &hint.guidance_refs,
            )
            .is_ok();
            let evidence_ok = verify_source_refs(
                &loaded,
                &record,
                &session,
                &owner,
                &hint.work_refs,
                &hint.evidence_refs,
            )
            .is_ok()
                && hint.evidence_refs.iter()
                    .filter(|item| item.consumption == "generated_query_result")
                    .all(|item| source_ref_current(&loaded, &record, &session, &owner, &hint.work_refs, item, &current_executable_sha256().unwrap_or_default()).unwrap_or(false));
            let executable = current_executable_sha256().ok();
            reference_freshness = json!(hint.evidence_refs.iter().chain(&hint.guidance_refs).map(|item| {
                let graph = if item.consumption == "generated_query_result" {
                    generated_result_info(&loaded,&record,&session,&owner,&hint.work_refs,item).ok()
                } else { None };
                let current = if let Some(graph) = graph.as_ref() {
                    graph["sourceCurrent"].as_bool() == Some(true)
                } else {
                    executable.as_deref().is_some_and(|sha| source_ref_current(&loaded,&record,&session,&owner,&hint.work_refs,item,sha).unwrap_or(false))
                };
                json!({"root":item.root,"path":item.path,"expectedSha256":item.sha256,
                    "work":item.work,
                    "producerExecutableSha256":item.producer_executable_sha256,
                    "callerReportedRead":item.consumption == "caller_reported_read",
                    "kind":if item.consumption == "generated_query_result" {"prior_graph_observation"} else {"verified_file_read"},
                    "current":current,
                    "restoredPriorObservation":graph})
            }).collect::<Vec<_>>());
            let pending_ok = verify_pending(&loaded, &record, &session, &owner, &hint.work_refs, hint.pending_operation.as_ref()).is_ok();
            freshness["workRefsCurrent"] = json!(work_ok);
            freshness["declaredGuidanceSourcesCurrent"] = json!(guidance_ok);
            freshness["pendingEvidenceCurrent"] = json!(evidence_ok && pending_ok);
            Ok(work_ok && guidance_ok && evidence_ok && pending_ok)
        });
        match owner_check {
            Ok(true) => {
                metadata_current = goal_current;
                if !goal_current {
                    dynamic_issue = Some(
                        "Goal advanced; saved steps and pending details are historical".into(),
                    );
                }
            }
            Ok(false) => {
                dynamic_issue = Some(
                    "one or more saved source/member references changed or are unavailable".into(),
                )
            }
            Err(error) => dynamic_issue = Some(crate::drift::plain(&error)),
        }
    }
    // A hint never proves that a Work is still at the state observed when its
    // step was recorded. The hook intentionally does not call ordinary `next`.
    let selection = WorkSelection {
        state: "unresolved".into(),
        work: None,
    };
    let active_recovery = Value::Null;
    let executable = current_executable_path()?;
    let work_routes = hint
        .as_ref()
        .map(|hint| {
            hint.work_refs.iter().map(|work| json!({
        "work":work.work,
        "argv":Route::new(["work","detail",work.work.as_str(),"--json"]).config_text(&work.config_path).run_by(&executable).display_argv()
    })).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let route_owner = hint
        .as_ref()
        .and_then(|value| config::load(Some(&value.owner_config_path)).ok());
    let reference_routes = hint
        .as_ref()
        .map(|hint| {
            hint.evidence_refs.iter().chain(&hint.guidance_refs).map(|item| {
                if item.consumption == "generated_query_result" {
                    let restored = route_owner.as_ref().and_then(|loaded| {
                        generated_result_info(loaded, &record, &session, &owner, &hint.work_refs, item).ok()
                    });
                    let query_argv = restored.as_ref().and_then(|value| {
                        let work = item.work.as_deref()?;
                        let member = hint.work_refs.iter().find(|reference| reference.work == work)?;
                        Some(Route::new(["project","graph","query","--query",value["query"].as_str()?,"--json"]).config_text(&member.config_path).run_by(&executable).display_argv())
                    });
                    json!({
                        "label":item.label,
                        "kind":"prior_graph_observation",
                        "root":item.root,
                        "path":item.path,
                        "work":item.work,
                        "expectedSha256":item.sha256,
                        "producerExecutableSha256":item.producer_executable_sha256,
                        "consumption":"generated_query_result",
                        "authority":"read_only",
                        "restoredPriorObservation":restored,
                        "queryArgv":query_argv,
                        "revalidation":"run the exact bounded query route; unchanged source/provider/index can reuse the cached result"
                    })
                } else {
                    json!({
                        "label":item.label,"kind":"verified_file_read","root":item.root,"path":item.path,"work":item.work,
                        "expectedSha256":item.sha256,"producerExecutableSha256":item.producer_executable_sha256,
                        "consumption":item.consumption,"authority":"read_only"
                    })
                }
            }).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let goal_route = hint.as_ref().map(|value| {
        Route::new(["goal", "status", "--json"])
            .config_text(&value.owner_config_path)
            .run_by(&executable)
            .display_argv()
    });
    let pending_route = hint.as_ref().and_then(|value| {
        value.pending_operation.as_ref().map(|pending| match pending {
            PendingOperation::GoalStatus { .. } => json!({"argv":goal_route,"authority":"read_only","historical":true}),
            PendingOperation::WorkDetail { work, .. } => value.work_refs.iter().find(|item| &item.work == work).map(|item| json!({"argv":Route::new(["work","detail",work.as_str(),"--json"]).config_text(&item.config_path).run_by(&executable).display_argv(),"authority":"read_only","historical":true})).unwrap_or(Value::Null),
            PendingOperation::EvidenceFile { handle, root, path, work, sha256, bytes, .. } => json!({"kind":"verified_file_read","handle":handle,"root":root,"path":path,"work":work,"sha256":sha256,"bytes":bytes,"authority":"read_only","historical":true}),
        })
    });
    let verified_owner = route_owner.as_ref().filter(|loaded| {
        session_goal::native::scope(loaded, false).is_ok_and(|scope| scope.owner_matches)
    });
    // The Lead's saved step and decisions, whether or not the Goal advanced
    // since; `goalCurrent` says which.
    let active_step = hint.as_ref().map_or(Value::Null, |value| {
        super::pause::active_step(value, freshness["goalCurrent"] == true)
    });
    let pause_config = verified_owner
        .map(|loaded| loaded.path.clone())
        .or_else(|| {
            super::owner_config::goal_owner(&serde_json::Map::new())
                .and_then(|found| found.config)
                .map(|(path, _)| path.into())
        });
    let pause_record = match (
        pause_config,
        session_goal::goal_summary::closed_revision(&record),
    ) {
        (Some(path), None) => super::pause::routes(&path).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let deadline = Instant::now() + HOOK_BUDGET;
    let derived = current_work::derive(
        &record,
        &session,
        &owner,
        verified_owner,
        current_work::SHOW_WORK_LIMIT,
        deadline,
    );
    let roots = derived
        .works
        .iter()
        .map(|item| item.product_root.clone())
        .collect::<Vec<_>>();
    let trees = checkout::uncommitted(&roots, checkout::SHOW_TREE_LIMIT, deadline);
    let current_works =
        current_work::show_value(&derived, &executable, checkout::show_value(&trees));
    Ok(json!({
        "readOnly":true,
        "currentWorks":current_works,
        "authority":"none",
        "ownerProjectId":owner.project_id,
        "goal":{"goalId":record["goalId"],"revision":record["revision"],"eventSha256":record["eventSha256"]},
        "workSelection":selection,
        "currentWorkReadSnapshot":Value::Null,
        "savedState":match (hint.is_some(), metadata_current) {
            (false, _) => "none: no step is saved",
            (true, true) => "current: the Goal is at the revision the step was saved at; reread the exact Work detail before acting on activeSavedStep",
            (true, false) => "historical: the Goal moved past the revision the step was saved at; reread the Goal and the exact Work detail before acting on activeSavedStep",
        },
        "activeSavedStep":active_step,
        "pauseRecord":pause_record,
        "unmappedRequirements":session_goal::requirements::unmapped(&record).iter()
            .map(|(id, source)| json!({"id":id,"source":source})).collect::<Vec<_>>(),
        "savedHint":hint_value,
        "activeRecovery":active_recovery,
        "milestone":super::milestone::project(&record, &session, &owner, hint.as_ref()),
        "freshness":freshness,
        "issue":dynamic_issue,
        "readRoutes":{"recoveryShowArgv":Route::new(["session","recovery","show","--json"]).run_by(&executable).display_argv(),"goalStatusArgv":goal_route,"workDetailArgv":work_routes,"references":reference_routes,"pendingOperation":pending_route},
        "referenceFreshness":reference_freshness,
        "hintSha256":hint.as_ref().map(|value| value.sha256.as_str()),
        "hintIssue":hint_issue,
        "effect":"read-only navigation; saved values do not authorize actions"
    }))
}
