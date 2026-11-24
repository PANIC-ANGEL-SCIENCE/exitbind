// Presents read-only Goal and run status, including themed output.
// Refuses unsupported Goal status views; see `src/cli/mod.rs`.
//! Read-only goal and run status surfaces, including optional themed display.

use super::*;
use serde_json::Value;

pub(super) fn goal_status(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let mut budget = crate::run::inputs::ResumeInputBudget::new();
    crate::run::inputs::with_resume_budget(&mut budget, |_| goal_status_bounded(l, a))
}

fn observation_limit() -> Option<&'static str> {
    crate::run::inputs::active_resume_budget().and_then(|mut budget| budget.limited_reason())
}

fn incomplete_presentation(value: &mut Value, presentation: &mut Value, reason: &str) {
    let readiness = json!({"state":"unknown","reason":reason});
    let progress = json!({
        "overall":"unavailable",
        "goal":value.get("goal"),
        "decomposition":{"available":false,"reason":reason},
        "tasks":[],
        "resultReadiness":{"state":"unavailable","reason":reason},
        "systemText":"Goal currentness and task progress are unavailable for this observation",
    });
    value["currentReadiness"] = readiness.clone();
    value["goalProgress"] = progress.clone();
    if value.get("requirements").is_some() {
        value["requirements"] = json!({"state":"unavailable","reason":reason});
    }
    presentation["currentReadiness"] = readiness;
    presentation["explicitLeadClosure"] = json!(false);
    presentation["goalProgress"] = progress;
    presentation["terminal"] = Value::Null;
}

fn finish_status_observation(
    root: &std::path::Path,
    record: Option<&Value>,
    value: &mut Value,
    presentation: &mut Value,
) {
    crate::session_goal::selected_observation_still_current(root, record);
    if let Some(reason) = observation_limit() {
        incomplete_presentation(value, presentation, reason);
    } else if presentation["currentReadiness"]["state"] == "unknown" {
        let reason = presentation["currentReadiness"]["reason"]
            .as_str()
            .unwrap_or("currentness_unavailable")
            .to_owned();
        incomplete_presentation(value, presentation, &reason);
    }
}

fn finish_event_observation(root: &std::path::Path, record: Option<&Value>, response: &mut Value) {
    crate::session_goal::selected_observation_still_current(root, record);
    if let Some(reason) = observation_limit() {
        response["completionEvent"] = Value::Null;
        response["presentation"]["terminal"] = Value::Null;
        response["currentReadiness"] = json!({"state":"unknown","reason":reason});
    }
}

fn goal_status_bounded(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options(
        "goal status",
        a,
        &[
            "config",
            "project",
            "json",
            "themed",
            "view",
            "expected-sha256",
        ],
    )?;
    args::assert_positionals("goal status", a, 1)?;
    if a.options.get("view").is_some_and(|view| view != "event") {
        return Err("goal status --view accepts only event".into());
    }
    let scope = if a.flags.contains_key("project") {
        crate::session_goal::native::project_scope(l)
    } else {
        crate::session_goal::native::scope_readonly(l)?
    };
    let record = crate::session_goal::read_for_recovery(&scope.root)
        .map_err(|error| format!("goal status observation unavailable: {error}"))?;
    if scope.is_session() && !scope.owner_matches {
        if a.options.contains_key("expected-sha256") {
            return Err("goal expansion is unavailable outside the owning session".into());
        }
        let value = crate::session_goal::context_summary(l, record.as_ref(), scope.owner.as_ref());
        if !crate::session_goal::selected_observation_still_current(&scope.root, record.as_ref()) {
            return Err("goal status observation unavailable: selected Goal journal changed or could not be read".into());
        }
        print_json(&crate::producer::observed(value))?;
        return Ok(());
    }
    if let Some(expected) = a.options.get("expected-sha256") {
        if record
            .as_ref()
            .and_then(|item| item["eventSha256"].as_str())
            != Some(expected.as_str())
        {
            return Err("goal record changed; refresh the current Goal before expansion".into());
        }
    }
    if a.options.get("view").is_some() {
        if a.options.contains_key("expected-sha256") {
            return Err("goal status --expected-sha256 requires the complete view".into());
        }
        let mut response = super::goal_completion::read_response(
            l,
            record.as_ref(),
            a.flags.contains_key("project"),
            &scope.root,
        )?;
        finish_event_observation(&scope.root, record.as_ref(), &mut response);
        return print_json(&crate::producer::observed(response));
    }
    let mut value = record.clone().unwrap_or_else(|| json!({"closed": false}));
    if let Some(session) = &scope.session {
        if let Some(recovery) = crate::session_goal::owner_correction::pending_recovery(session, l)?
        {
            value["ownerCorrectionRecovery"] = recovery;
        }
    }
    let (mut presentation, selected_requirements) =
        crate::session_goal::presentation_and_requirements_for_selected(
            l,
            record.as_ref(),
            &scope.root,
            a.flags.contains_key("project"),
        )?;
    value["currentReadiness"] = presentation["currentReadiness"].clone();
    value["goalProgress"] = presentation["goalProgress"].clone();
    if scope.is_session()
        && scope.owner_matches
        && value["goalProgress"]["journeyCompletion"]["state"] == "blocked"
    {
        if let Some(form) = record
            .as_ref()
            .and_then(|goal| crate::session_goal::recovery::preview_form(l, goal))
        {
            value["goalProgress"]["journeyCompletion"]["recoveryForm"] = form;
        }
    }
    let named = record
        .as_ref()
        .is_some_and(crate::session_goal::requirements::named);
    if named {
        let requirements = selected_requirements.unwrap_or_else(|| {
            crate::session_goal::requirements::projection_selected(
                l,
                &scope.root,
                a.flags.contains_key("project"),
                record.as_ref().expect("named record"),
            )
        });
        match requirements {
            Ok(requirements) => value["requirements"] = requirements,
            Err(_) if observation_limit().is_some() => {
                value["requirements"] = Value::Null;
            }
            Err(error) => return Err(error),
        }
    }
    let native_governed = scope.is_session()
        && record
            .as_ref()
            .is_some_and(|goal| goal["closure"]["kind"] != "direct");
    if let Some(routes) = crate::host::session_recovery::pause_routes(l) {
        value["pauseRecord"] = routes;
    }
    if let Some(expected) = a.options.get("expected-sha256") {
        let fresh = crate::session_goal::read_for_recovery(&scope.root)
            .map_err(|error| format!("goal status observation unavailable: {error}"))?;
        if fresh.as_ref().and_then(|item| item["eventSha256"].as_str()) != Some(expected.as_str()) {
            return Err("goal record changed during expansion".into());
        }
    }
    finish_status_observation(&scope.root, record.as_ref(), &mut value, &mut presentation);
    if named || native_governed {
        value["presentation"] = json!({"terminal": presentation["terminal"]});
        if !a.flags.contains_key("json") {
            if let Some(terminal) = presentation["terminal"].as_str() {
                for line in value["pauseRecord"]["text"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    println!("{}", line.as_str().unwrap_or_default());
                }
                println!("{terminal}");
                return Ok(());
            }
        }
    }
    print_json(&crate::producer::observed(value))?;
    if !a.flags.contains_key("json") {
        if let Some(card) = crate::presentation_events::session_goal_direct_card_for_human(
            &l.state_root,
            &presentation,
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            !std::io::stdin().is_terminal(),
            a.flags.contains_key("themed"),
        ) {
            println!("{card}");
        }
    }
    Ok(())
}

pub(super) fn run_status(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_positionals("run status", a, 2)?;
    let ledger = positional(a, 1, "run status requires LEDGER")?;
    let json_output = a.flags.contains_key("json");
    if json_output {
        let mut value =
            run::status(l, ledger).map_err(|error| map_run_error(error, true, &l.path))?;
        value["goalProgress"] = crate::session_goal::progress_for_loaded(l, &value["progress"])?;
        if let Some(terminal) = run::terminal_display(l, ledger) {
            value["terminal"] = json!(terminal);
        }
        print_json(&crate::producer::observed(value))?;
        return Ok(());
    }
    let config = a.options.get("config").map(String::as_str).map_or_else(
        || {
            l.path
                .to_str()
                .ok_or("configuration path is not valid UTF-8")
        },
        Ok,
    )?;
    let value = run::human_status(l, ledger).map_err(|error| {
        if run::inspect(l, ledger).is_ok() {
            eprintln!(
                "Inspect: {}",
                crate::presentation::read_command("inspect", config, ledger)
            );
        }
        map_run_error(error, false, &l.path)
    })?;
    let current_goal = crate::session_goal::read(&l.state_root)?;
    let session_goal = crate::session_goal::presentation_for_loaded(l, current_goal.as_ref())?;
    let card_emitted = crate::run_presentation::print_status(
        &value,
        &session_goal,
        &l.state_root,
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        !std::io::stdin().is_terminal(),
        a.flags.contains_key("themed"),
    );
    if !card_emitted {
        if value.status != "running" || value.artifact_status != "current" {
            println!(
                "Inspect: {}",
                crate::presentation::read_command("inspect", config, ledger)
            );
        } else {
            match run::next(l, ledger) {
                Ok(next) if next["status"] == "running" => println!(
                    "Next: {}",
                    crate::presentation::read_command("next", config, ledger)
                ),
                _ => {
                    println!("Guidance: no validated pending progression is available; inspect the run before recovery.");
                    println!(
                        "Inspect: {}",
                        crate::presentation::read_command("inspect", config, ledger)
                    );
                }
            }
        }
    }
    // The same block the work facade offers, last, so a reader who
    // drilled down to the run surface still has the product's own
    // wording instead of composing a sentence from the report above.
    if !card_emitted {
        if let Some(terminal) = run::terminal_display(l, ledger) {
            println!("{terminal}");
        }
    }
    Ok(())
}

pub(super) fn run_explain(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_positionals("run explain", a, 2)?;
    if a.flags.contains_key("json") {
        let machine = run::explain(
            l,
            positional(a, 1, "run explain requires LEDGER")?,
            a.options.get("event").map(String::as_str),
        )
        .map_err(|error| map_run_error(error, true, &l.path))?;
        print_json(&machine)?;
    } else {
        let value = run::human_explain(
            l,
            positional(a, 1, "run explain requires LEDGER")?,
            a.options.get("event").map(String::as_str),
        )
        .map_err(|error| map_run_error(error, false, &l.path))?;
        let current_goal = crate::session_goal::read(&l.state_root)?;
        let session_goal = crate::session_goal::presentation_for_loaded(l, current_goal.as_ref())?;
        crate::run_presentation::print_explain(
            &value,
            &session_goal,
            &l.state_root,
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            !std::io::stdin().is_terminal(),
            a.flags.contains_key("themed"),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_goal::native;

    fn closed_project() -> (
        crate::test_root::TempRoot,
        config::Loaded,
        Value,
        crate::session_goal::native::CallbackSessionGuard,
    ) {
        let root = crate::test_root::temp("goal-status-snapshot");
        let path = crate::project::onboarding::init_with_options(
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
        let loaded = config::load(path.to_str()).unwrap();
        let store = loaded.state_root.join(".exitbind/test-native-session");
        crate::project::portability::create_private_directory(&store).unwrap();
        let (host, identity) = native::current_identity()
            .unwrap()
            .unwrap_or(("codex", "status-snapshot-fixture".to_owned()));
        let key = native::session_key(host, &identity);
        let session = native::Session {
            host,
            key: key.clone(),
            selector_key: key.clone(),
            root: store.join(format!("session-{key}")),
        };
        assert!(
            native::scope_for(&loaded, session.clone(), true)
                .unwrap()
                .owner_matches
        );
        let guard = native::scoped_callback_session(session).unwrap();
        crate::session_goal::incorporate(
            &loaded.state_root,
            &loaded,
            "snapshot",
            "Close the fixture Goal",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("obligations,findings,blockers,decisions,externalActions"),
            false,
        )
        .unwrap();
        let closed =
            crate::session_goal::close_direct(&loaded.state_root, &loaded, "snapshot", None)
                .unwrap();
        (root, loaded, closed, guard)
    }

    #[test]
    fn changed_or_deleted_selected_journal_cannot_present_old_completion() {
        for remove in [false, true] {
            let (_root, loaded, closed, _session) = closed_project();
            let captured = crate::session_goal::read_for_recovery(&loaded.state_root)
                .unwrap()
                .unwrap();
            assert_eq!(captured["eventSha256"], closed["eventSha256"]);
            let rendered = crate::session_goal::presentation_for_selected(
                &loaded,
                Some(&captured),
                &loaded.state_root,
                true,
            )
            .unwrap();
            assert_eq!(rendered["currentReadiness"]["state"], "current");
            let event = super::super::goal_completion::read_response(
                &loaded,
                Some(&captured),
                true,
                &loaded.state_root,
            )
            .unwrap();
            assert!(event["presentation"]["terminal"].is_string());

            if remove {
                std::fs::remove_file(loaded.state_root.join(".exitbind/session-goal.jsonl"))
                    .unwrap();
            } else {
                let appended = crate::session_goal::incorporate(
                    &loaded.state_root,
                    &loaded,
                    "snapshot",
                    "Close the fixture Goal",
                    Some("A later ordinary obligation"),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                )
                .unwrap();
                assert_ne!(appended["eventSha256"], captured["eventSha256"]);
            }
            let expected = if remove {
                "goal_journal_missing"
            } else {
                "goal_journal_changed"
            };
            let mut status_value = captured.clone();
            let mut status_presentation = rendered.clone();
            let mut status_budget = crate::run::inputs::ResumeInputBudget::new();
            crate::run::inputs::with_resume_budget(&mut status_budget, |_| {
                finish_status_observation(
                    &loaded.state_root,
                    Some(&captured),
                    &mut status_value,
                    &mut status_presentation,
                );
            });
            assert_eq!(status_value["currentReadiness"]["state"], "unknown");
            assert_eq!(status_value["currentReadiness"]["reason"], expected);
            assert_eq!(status_value["goalProgress"]["overall"], "unavailable");
            assert_eq!(status_presentation["explicitLeadClosure"], false);
            assert!(status_presentation["terminal"].is_null());

            let mut stale_event = event;
            let mut event_budget = crate::run::inputs::ResumeInputBudget::new();
            crate::run::inputs::with_resume_budget(&mut event_budget, |_| {
                finish_event_observation(&loaded.state_root, Some(&captured), &mut stale_event);
            });
            assert_eq!(stale_event["currentReadiness"]["reason"], expected);
            assert!(stale_event["completionEvent"].is_null());
            assert!(stale_event["presentation"]["terminal"].is_null());

            let mut projection_budget = crate::run::inputs::ResumeInputBudget::new();
            let projected = crate::run::inputs::with_resume_budget(&mut projection_budget, |_| {
                crate::session_goal::presentation_for_selected(
                    &loaded,
                    Some(&captured),
                    &loaded.state_root,
                    true,
                )
                .unwrap()
            });
            assert_eq!(projected["currentReadiness"]["reason"], expected);
            assert_eq!(projected["explicitLeadClosure"], false);
            assert!(projected["terminal"].is_null());
            // No-budget callers retain their pre-existing behavior.
            assert!(crate::session_goal::selected_observation_still_current(
                &loaded.state_root,
                Some(&captured)
            ));
        }
    }
}
