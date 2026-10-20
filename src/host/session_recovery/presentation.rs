use super::{
    current_work,
    hint::{canonical_path, read_hint, HOOK_BUDGET},
    references::{
        generated_result_info, known_work_locator, verify_pending, verify_source_refs,
        verify_work_refs,
    },
};
use crate::{config, evidence::hash, session_goal};
use serde_json::{json, Value};
use std::{path::Path, time::Instant};

/// The recovery part of the SessionStart text stays within this bound so the
/// host's oversized-context fallback, which keeps only the fixed session core
/// and this text, still fits its 4096-byte limit.
const RECOVERY_HOOK_BYTES: usize = 2048;

/// Hook-safe context. It reads the native Goal independently of the launch
/// project's config and reports unresolved ownership honestly when no exact
/// owner locator is available. When no valid saved hint selects a Work, the
/// open Works are derived from the Goal's own journey members.
pub(crate) fn hook_context(
    payload: &serde_json::Map<String, Value>,
    cwd_config_path: Option<&str>,
) -> Option<String> {
    let started = Instant::now();
    let session = match session_goal::native::current() {
        Ok(Some(session)) => session,
        _ => return None,
    };
    let Some(id) = payload.get("session_id").and_then(Value::as_str) else {
        return Some("Exitbind session recovery declined: host payload session ID is missing or malformed; no Work was selected.".into());
    };
    let reported = hash::value(&json!({"host":session.host,"identity":id}));
    if reported != session.key {
        return Some("Exitbind session recovery declined: host payload session ID disagrees with current session selectors; no Work was selected.".into());
    }
    let owner = match session.owner() {
        Ok(Some(owner)) => owner,
        _ => return Some("Exitbind session recovery is unresolved: the current native Goal owner record is absent or unavailable; no Work was selected.".into()),
    };
    let record = match session_goal::read_for_recovery(&session.root) {
        Ok(Some(record)) => record,
        Ok(None) => return Some("Exitbind session recovery is unresolved: no canonical native Goal is available; no Work was selected.".into()),
        Err(error) => return Some(format!("Exitbind session recovery is unresolved: {error}; no Work was selected.")),
    };
    let hinted = hint_text(payload, cwd_config_path, &session, &owner, &record, started);
    if !hinted.derive {
        return Some(hinted.text);
    }
    let deadline = started + HOOK_BUDGET;
    let derived = current_work::derive(
        &record,
        &session,
        &owner,
        hinted.owner_loaded.as_ref(),
        current_work::HOOK_WORK_LIMIT,
        deadline,
    );
    if derived.works.is_empty() {
        return Some(hinted.text);
    }
    let base = if hinted.no_hint {
        format!(
            "Exitbind recovered native Goal {} revision {} from its journey; no saved recovery hint selects a Work.",
            derived.goal_id,
            record["revision"].as_u64().unwrap_or(0)
        )
    } else {
        hinted.text
    };
    let budget = RECOVERY_HOOK_BYTES.saturating_sub(base.len() + 1);
    Some(format!("{base}\n{}", derived_hook_text(&derived, budget)))
}

/// One line per derived Work in the `work detail` form the agent can run as
/// printed.
fn derived_hook_text(derived: &current_work::Derived, budget: usize) -> String {
    let goal = bounded_hook_text(&derived.goal_id, 128);
    let total = derived.works.len() + derived.more;
    let mut out = String::new();
    let mut shown = 0;
    for item in &derived.works {
        let line = format!(
            "{}current Work {} ({} of Goal {goal}); read: exitbind work detail {} --view action --config {}; next action: {}.",
            if total > 1 { format!("{}) ", shown + 1) } else { String::new() },
            item.work,
            if item.source == "journey_member" { "member" } else { "mapped Work" },
            item.work,
            item.config_path,
            bounded_hook_text(item.next_action.as_deref().unwrap_or("read the Work detail"), 192),
        );
        // Room is kept for the count of Works not shown.
        if shown > 0 && out.len() + 1 + line.len() + 96 > budget {
            break;
        }
        if shown > 0 {
            out.push(' ');
        }
        out.push_str(&line);
        shown += 1;
    }
    let mut tail = String::new();
    if total > shown {
        tail.push_str(&format!(
            " {} more open Work(s): `exitbind session recovery show --json`.",
            total - shown
        ));
    }
    if derived.unverified > 0 {
        tail.push_str(&format!(
            " {} journey member(s) could not be verified now.",
            derived.unverified
        ));
    }
    out.push_str(&tail);
    out
}

/// The saved-hint part of the hook text, and whether the current Work must
/// still be derived from the Goal's journey (no valid hint selected one).
struct HintText {
    text: String,
    derive: bool,
    no_hint: bool,
    owner_loaded: Option<config::Loaded>,
}

fn hint_text(
    payload: &serde_json::Map<String, Value>,
    cwd_config_path: Option<&str>,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    record: &Value,
    started: Instant,
) -> HintText {
    let finish = |text: String, derive: bool, owner_loaded: Option<config::Loaded>| HintText {
        text,
        derive,
        no_hint: false,
        owner_loaded,
    };
    let hint = match read_hint(&session.root) {
        Ok(value) => value,
        Err(error) => return finish(format!("Exitbind session recovery hint is unusable: {error}; reread the native Goal and Work before acting."), true, None),
    };
    let mut owner_match = false;
    let mut hinted_owner_config_current = false;
    let mut owner_loaded: Option<config::Loaded> = None;
    let mut config_candidates = Vec::new();
    if let Some(hint) = &hint {
        if hint.host == session.host
            && hint.session_key == session.key
            && hint.owner_identity_sha256 == owner.identity_sha256
            && hint.owner_project_id == owner.project_id
        {
            config_candidates.push(hint.owner_config_path.clone());
        }
    }
    if let Some(path) = cwd_config_path {
        if Path::new(path).is_absolute() && !config_candidates.iter().any(|value| value == path) {
            config_candidates.push(path.to_owned());
        }
    }
    for candidate in config_candidates.iter().take(16) {
        if started.elapsed() > HOOK_BUDGET {
            return finish("Exitbind session recovery exceeded its four-second hook budget; use the exact Goal recovery route.".into(), false, None);
        }
        let Ok(loaded) = config::load(Some(candidate)) else {
            continue;
        };
        if !canonical_path(&loaded.path, "native owner configuration")
            .is_ok_and(|path| path == *candidate)
        {
            continue;
        }
        let Ok(scope) = session_goal::native::scope(&loaded, false) else {
            continue;
        };
        if scope.owner_matches
            && scope.owner.as_ref().is_some_and(|actual| {
                actual.identity_sha256 == owner.identity_sha256
                    && actual.project_id == owner.project_id
            })
        {
            owner_match = true;
            owner_loaded = Some(loaded);
            if hint.as_ref().is_some_and(|hint| {
                hint.owner_config_path == *candidate
                    && owner_loaded
                        .as_ref()
                        .is_some_and(|value| hint.owner_config_sha256 == hash::text(&value.source))
            }) {
                hinted_owner_config_current = true;
            }
            break;
        }
    }
    if started.elapsed() > HOOK_BUDGET {
        return finish("Exitbind session recovery exceeded its four-second hook budget; use the exact Goal recovery route.".into(), false, None);
    }
    let goal_id = record["goalId"].as_str().unwrap_or("unknown");
    let revision = record["revision"].as_u64().unwrap_or(0);
    let current_event = record["eventSha256"].as_str().unwrap_or("unknown");
    let Some(hint) = hint else {
        let mut text = finish(format!(
            "Exitbind recovered native Goal {goal_id} revision {revision} (event {current_event}); its owner is {}. No current Work is selected by this hook. Use `exitbind session recovery show --json`; record an exact hint only with the verified owner configuration, then reread Goal and known Work details before acting.",
            if owner_match { "verified for the available configuration" } else { "known but its exact configuration is unresolved" },
        ), true, owner_loaded);
        text.no_hint = true;
        return text;
    };
    if hint.host != session.host
        || hint.session_key != session.key
        || hint.owner_identity_sha256 != owner.identity_sha256
        || hint.owner_project_id != owner.project_id
        || !hinted_owner_config_current
        || hint.goal_id != goal_id
    {
        return finish(format!(
            "Exitbind found a stale or foreign recovery hint for native Goal {goal_id} revision {revision}; do not use its saved action. Reread the exact Goal and Work detail before acting."
        ), true, owner_loaded);
    }
    let Some(owner_config) = owner_loaded else {
        return finish(format!("Exitbind native Goal {goal_id} is known, but its fixed owner configuration is unresolved. Use the exact read route `exitbind session recovery show --json`; do not infer a Work from the saved hint."), true, None);
    };
    let owner_loaded = &owner_config;
    if verify_work_refs(record, session, owner, &hint.work_refs).is_err()
        || verify_source_refs(
            owner_loaded,
            record,
            session,
            owner,
            &hint.work_refs,
            &hint.evidence_refs,
        )
        .is_err()
        || verify_source_refs(
            owner_loaded,
            record,
            session,
            owner,
            &hint.work_refs,
            &hint.guidance_refs,
        )
        .is_err()
        || verify_pending(
            owner_loaded,
            record,
            session,
            owner,
            &hint.work_refs,
            hint.pending_operation.as_ref(),
        )
        .is_err()
    {
        return finish(format!(
            "Exitbind recovery hint Work references are stale or no longer match native Goal {goal_id}; reread the exact Goal and Work detail before acting."
        ), true, Some(owner_config));
    }
    let graph_observation = hint
        .evidence_refs
        .iter()
        .find(|item| item.consumption == "generated_query_result")
        .and_then(|item| {
            generated_result_info(owner_loaded, record, session, owner, &hint.work_refs, item).ok()
        });
    if started.elapsed() > HOOK_BUDGET {
        return finish("Exitbind session recovery exceeded its four-second hook budget; use the exact Goal recovery route.".into(), false, None);
    }
    let source = payload
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "startup" | "resume" | "compact"))
        .map(|value| format!("Host lifecycle: {value}. "))
        .unwrap_or_default();
    let fresh_goal = hint.goal_event_sha256 == current_event && hint.goal_revision == revision;
    let fresh_sources = verify_source_refs(
        owner_loaded,
        record,
        session,
        owner,
        &hint.work_refs,
        &hint.guidance_refs,
    )
    .is_ok();
    let known_locator = known_work_locator(&hint.current_work, &hint.work_refs)
        .ok()
        .flatten();
    if started.elapsed() > HOOK_BUDGET {
        return finish("Exitbind session recovery exceeded its four-second hook budget; use the exact Goal recovery route.".into(), false, None);
    }
    let locator = if let Some(reference) = known_locator {
        format!("exact Goal-member locator {} is retained", reference.work)
    } else {
        "no exact Work locator is selected".to_owned()
    };
    let mut out = if let Some(observation) = graph_observation.as_ref() {
        graph_hook_presentation(
            goal_id,
            revision,
            owner_match,
            fresh_goal,
            fresh_sources,
            &locator,
            observation,
        )
    } else {
        format!(
            "Exitbind recovery: {source}Goal {goal_id} rev {revision}, event {current_event}; owner={} goalCurrent={fresh_goal} declaredGuidanceCurrent={fresh_sources}. A saved step, corrections, or pending operation remains historical until the exact Work detail is reread; no current Work/action binding was acquired. {locator}.",
            if owner_match { "verified" } else { "unresolved" }
        )
    };
    if graph_observation.is_none() {
        out.push_str(" Use the executable-bound read routes in `session recovery show --json`; no saved action is replayed.");
    }
    if graph_observation.is_none() && out.len() > 900 {
        out = format!("Exitbind native Goal {goal_id} revision {revision} is known. Saved step/corrections/pending details remain historical until exact Work detail is reread; use `session recovery show --json`. No saved action is replayed.");
    }
    let selected = known_locator.is_some();
    finish(out, !selected, Some(owner_config))
}

fn bounded_hook_text(value: &str, max_bytes: usize) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim();
    if sanitized.len() <= max_bytes {
        return sanitized.to_owned();
    }
    let mut end = max_bytes.saturating_sub('…'.len_utf8());
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &sanitized[..end])
}

fn graph_hook_presentation(
    goal_id: &str,
    revision: u64,
    owner_match: bool,
    fresh_goal: bool,
    fresh_sources: bool,
    locator: &str,
    observation: &Value,
) -> String {
    let prefix = format!(
        "Exitbind recovery: native Goal {goal_id} rev {revision}; owner={} Goal current={fresh_goal} declared guidance current={fresh_sources}. {locator}. Saved step/corrections/pending remain historical; no current Work/action binding. Exact Goal/member/project query routes: `exitbind session recovery show --json`; no saved action is replayed. Prior CodeGraph observation: ",
        if owner_match { "verified" } else { "unresolved" }
    );
    let between = ". Source identity is not revalidated on hook; provider/index currentness is not asserted. Original query `";
    let suffix = "` (display may be abbreviated); full query and exact project route are in the read route above. Requery before relying on the finding.";
    let available = 900usize.saturating_sub(prefix.len() + between.len() + suffix.len());
    if available < 8 {
        return "Exitbind recovery: a prior CodeGraph observation exists; saved details remain historical with no current Work/action binding. Source identity is not revalidated and provider/index currentness is not asserted. Use `exitbind session recovery show --json` for the exact Goal, member locator, full query, and project route. No saved action is replayed; requery before relying on the finding.".into();
    }
    let mut summary_budget = 120.min(available / 2);
    let mut query_budget = 112.min(available - summary_budget);
    summary_budget += (120 - summary_budget).min(available - summary_budget - query_budget);
    query_budget += (112 - query_budget).min(available - summary_budget - query_budget);
    let summary = bounded_hook_text(
        observation["summary"]
            .as_str()
            .unwrap_or("bounded graph findings"),
        summary_budget,
    );
    let query = bounded_hook_text(
        observation["query"].as_str().unwrap_or("saved graph query"),
        query_budget,
    );
    let out = format!("{prefix}{summary}{between}{query}{suffix}");
    if out.len() <= 900 {
        return out;
    }

    "Exitbind recovery: a prior CodeGraph observation exists; saved details remain historical with no current Work/action binding. Source identity is not revalidated and provider/index currentness is not asserted. Use `exitbind session recovery show --json` for the exact Goal, member locator, full query, and project route. No saved action is replayed; requery before relying on the finding.".into()
}

#[cfg(test)]
mod tests {
    use super::super::hint::{
        checkpoint_is_ambiguous_older, unseal, validate_hint, Checkpoint, Hint, PendingOperation,
        Reference, WorkRef, WorkSelection,
    };
    use super::*;

    fn hint() -> Hint {
        Hint {
            kind: "exitbind_session_recovery_hint".into(),
            protocol: 2,
            host: "codex".into(),
            session_key: "a".repeat(64),
            owner_config_path: "/tmp/owner/exitbind.json".into(),
            owner_config_sha256: "b".repeat(64),
            owner_project_id: Some("project".into()),
            owner_identity_sha256: "c".repeat(64),
            goal_id: "goal".into(),
            goal_revision: 1,
            goal_event_sha256: "d".repeat(64),
            current_step: "read the exact current Work".into(),
            current_work: WorkSelection {
                state: "unresolved".into(),
                work: None,
            },
            latest_corrections: Vec::new(),
            evidence_refs: Vec::new(),
            work_refs: Vec::new(),
            pending_operation: Some(PendingOperation::GoalStatus {
                description: "inspect retained current Goal status".into(),
            }),
            guidance_refs: Vec::new(),
            last_checkpoint: None,
            sha256: String::new(),
        }
    }

    #[test]
    fn recovery_hints_remain_navigation_only_and_reject_work_selection_or_mutation_routes() {
        let safe = hint();
        validate_hint(&safe).unwrap();

        let mut invalid_selection = hint();
        invalid_selection.current_work = WorkSelection {
            state: "selected".into(),
            work: Some("work-token".into()),
        };
        assert!(validate_hint(&invalid_selection).is_err());
        let invalid_pending: Result<PendingOperation, _> = serde_json::from_value(
            json!({"kind":"shell","description":"inspect","readRoute":"exitbind work check W --apply"}),
        );
        assert!(invalid_pending.is_err());

        let mut known_without_member = hint();
        known_without_member.current_work = WorkSelection {
            state: "known".into(),
            work: Some("smw_not_a_member".into()),
        };
        assert!(validate_hint(&known_without_member).is_err());

        let mut known = hint();
        let work = "smw_exact_member".to_owned();
        known.current_work = WorkSelection {
            state: "known".into(),
            work: Some(work.clone()),
        };
        known.work_refs.push(WorkRef {
            work: work.clone(),
            config_path: "/tmp/project/exitbind.json".into(),
            config_sha256: "e".repeat(64),
            start_event_sha256: "f".repeat(64),
            relation: "member".into(),
        });
        validate_hint(&known).expect("known is a locator shape, not a fresh action binding");

        let mut pending_work = hint();
        pending_work.pending_operation = Some(PendingOperation::WorkDetail {
            description: "inspect the existing Work".into(),
            work,
        });
        assert!(
            validate_hint(&pending_work).is_err(),
            "pending Work routes must match exact retained members"
        );
    }

    #[test]
    fn references_require_explicit_read_source_and_current_producer_binding() {
        let mut value = hint();
        value.guidance_refs.push(Reference {
            label: "reviewed instructions".into(),
            root: "product".into(),
            path: "AGENTS.md".into(),
            work: Some("smw_exact_member".into()),
            sha256: "a".repeat(64),
            producer_executable_sha256: "b".repeat(64),
            consumption: "caller_reported_read".into(),
        });
        value.work_refs.push(WorkRef {
            work: "smw_exact_member".into(),
            config_path: "/tmp/project/exitbind.json".into(),
            config_sha256: "e".repeat(64),
            start_event_sha256: "f".repeat(64),
            relation: "member".into(),
        });
        validate_hint(&value).unwrap();
        value.guidance_refs[0].work = Some("smw_unlisted".into());
        assert!(validate_hint(&value).is_err());
        value.guidance_refs[0].work = Some("smw_exact_member".into());
        value.guidance_refs[0].consumption = "present_in_directory".into();
        assert!(validate_hint(&value).is_err());
        value.guidance_refs[0].consumption = "caller_reported_read".into();
        value.guidance_refs[0].path = "../outside".into();
        assert!(validate_hint(&value).is_err());
    }

    #[test]
    fn ambiguous_older_compaction_checkpoint_does_not_replace_newer_marker() {
        let later = Checkpoint {
            event: "PostCompact".into(),
            goal_revision: 3,
            goal_event_sha256: "a".repeat(64),
        };
        let earlier = Checkpoint {
            event: "PreCompact".into(),
            goal_revision: 3,
            goal_event_sha256: "a".repeat(64),
        };
        assert!(checkpoint_is_ambiguous_older(&later, &earlier));
        let advanced = Checkpoint {
            event: "PreCompact".into(),
            goal_revision: 4,
            goal_event_sha256: "b".repeat(64),
        };
        assert!(!checkpoint_is_ambiguous_older(&later, &advanced));
    }

    #[test]
    fn recovery_hint_integrity_rejects_changed_navigation_bytes() {
        let mut value = hint();
        value.sha256 = hash::value(&serde_json::to_value(&value).unwrap());
        let sealed = value.clone();
        assert!(unseal(sealed).is_ok());
        value.current_step.push_str(" changed");
        assert!(unseal(value).is_err());
    }

    #[test]
    fn several_derived_works_are_listed_up_to_the_limit_with_a_count_of_the_rest() {
        let work = |index: usize| super::current_work::CurrentWork {
            work: format!("smw_{}", format!("{index}").repeat(64)),
            config_path: "/tmp/project/exitbind.json".into(),
            source: "journey_member",
            next_action: Some("lead_decision".into()),
        };
        let derived = super::current_work::Derived {
            goal_id: "R54".into(),
            works: (1..=4).map(work).collect(),
            more: 2,
            unverified: 0,
            incomplete: false,
        };
        let text = derived_hook_text(&derived, RECOVERY_HOOK_BYTES);
        assert!(text.len() <= RECOVERY_HOOK_BYTES, "{text}");
        assert!(
            text.contains(&format!("1) current Work smw_{}", "1".repeat(64))),
            "{text}"
        );
        assert!(text.contains("more open Work(s)"), "{text}");
        let listed = (1..=4)
            .filter(|index| text.contains(&format!("{index}) current Work")))
            .count();
        assert!((1..=4).contains(&listed), "{text}");
        let shown_more = 6 - listed;
        assert!(
            text.contains(&format!(" {shown_more} more open Work(s)")),
            "{text}"
        );

        let small = derived_hook_text(&derived, 300);
        assert!(
            small.contains("1) current Work"),
            "the first Work is always named"
        );
        assert!(small.contains(" 5 more open Work(s)"), "{small}");
    }

    #[test]
    fn graph_hook_presentation_keeps_long_unicode_observation_within_existing_bound() {
        let observation = json!({
            "summary":"é findings ".repeat(40),
            "query":"callers of 配置解析関数 ".repeat(30),
            "sourceCurrent":null
        });
        let message = graph_hook_presentation(
            "r51-product-architecture-joint-delivery",
            85,
            true,
            true,
            true,
            &format!(
                "exact Goal-member locator smw_{} is retained",
                "a".repeat(64)
            ),
            &observation,
        );

        assert!(message.len() <= 900, "{message}");
        assert!(
            message.contains("Prior CodeGraph observation: "),
            "{message}"
        );
        assert!(
            message.contains("Original query `callers of 配置解析関数"),
            "{message}"
        );
        assert!(
            message.contains(&format!("smw_{}", "a".repeat(64))),
            "{message}"
        );
        assert!(
            message.contains("session recovery show --json"),
            "{message}"
        );
        assert!(message.contains("historical"), "{message}");
        assert!(message.contains("no saved action is replayed"), "{message}");
        assert!(
            message.contains("Source identity is not revalidated"),
            "{message}"
        );
        assert!(
            message.contains("provider/index currentness is not asserted"),
            "{message}"
        );
        assert!(
            message.contains('…'),
            "long Unicode fields should be visibly abbreviated"
        );
    }
}
