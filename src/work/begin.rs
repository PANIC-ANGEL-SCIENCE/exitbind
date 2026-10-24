//! Governed Work creation and named-goal coverage binding.

use super::*;

pub(crate) struct BeginOptions<'a> {
    pub(crate) workflow: &'a str,
    pub(crate) goal: &'a str,
    pub(crate) check_command: &'a str,
    pub(crate) boundary: Option<&'a str>,
    pub(crate) harness_receipt: Option<&'a str>,
    pub(crate) proof_origin: Option<&'a str>,
    pub(crate) preserve_requirement: Option<&'a str>,
    pub(crate) preservation_check_command: Option<&'a str>,
    pub(crate) preservation_proof_origin: Option<&'a str>,
    pub(crate) basis: BasisOrigin<'a>,
    pub(crate) review_policy: Option<&'a str>,
    pub(crate) auto_goal_context: bool,
}

/// Distinguishes caller-controlled text from the opaque basis produced by the
/// explicit native journey integration preparation path.
pub(crate) enum BasisOrigin<'a> {
    Caller(Option<&'a str>),
    PreparedJourney(crate::session_goal::journey::Prepared),
}

#[cfg(test)]
pub(crate) fn begin(loaded: &Loaded, options: BeginOptions<'_>) -> Result<Value, String> {
    begin_with_pre_return(loaded, options, None, None, None)
}

pub(crate) fn begin_with_pre_return(
    loaded: &Loaded,
    options: BeginOptions<'_>,
    pre_return_command: Option<&str>,
    pre_return_timeout_ms: Option<&str>,
    pre_return_capture_bytes: Option<&str>,
) -> Result<Value, String> {
    let basis = match &options.basis {
        BasisOrigin::Caller(basis) => {
            if let Some(basis) = basis {
                crate::session_goal::journey::reject_unprepared_markers(basis)?;
            }
            *basis
        }
        BasisOrigin::PreparedJourney(prepared) => Some(prepared.basis()),
    };
    // Environment readiness is checked before any ledger mutation, so a
    // checker that exists but cannot run refuses before the first record. The
    // facts themselves stay on `check`, `setup` and `goal prepare`: the Work
    // envelope is agent-facing and carries no transport paths, and the key
    // `readiness` on Work output means its progress state.
    if let Some(refusal) =
        crate::project::readiness::checker_refusal(&loaded.product_root, options.check_command)
    {
        return Err(format!(
            "work begin refused before recording a Work: {refusal}"
        ));
    }
    // `work begin` is the governed activation boundary. Direct, harmless work
    // never enters this path. Availability is read from the loaded project
    // preconditions before any ledger mutation; the final boolean means those
    // preconditions have been activated by this boundary, not that a prior run
    // already existed.
    let material_consequence = !options.goal.trim().is_empty();
    let promotion_required = !options.check_command.trim().is_empty();
    let activation_available = loaded.path.is_file() && loaded.state_root.is_dir();
    let managed_namespace = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace());
    let activation_ready = activation_available
        && managed_namespace.is_dir()
        && managed_namespace.join("runs").is_dir()
        && managed_namespace.join("locks").is_dir();
    if crate::host::runtime::classify_activation(
        material_consequence,
        promotion_required,
        activation_available,
        activation_ready,
    ) != crate::host::runtime::Activation::Governed
    {
        return Err("governed activation is unavailable".into());
    }
    let goal_scope = if options.auto_goal_context {
        crate::session_goal::native::optional_work_scope(loaded)
    } else {
        None
    };
    let goal_candidate = goal_scope
        .as_ref()
        .map(crate::session_goal::association::prepare)
        .transpose()?
        .flatten();
    let token = hash::text(&format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        options.workflow,
        options.goal,
        options.check_command,
        loaded.source,
        std::process::id(),
        timestamp_nanos()
    ));
    let ledger = format!("{}/work-{token}.jsonl", runs_dir());
    let work = format!("{WORK_PREFIX}{token}");
    if let Some(candidate) = goal_candidate.as_ref() {
        let intent = run::start_intent_sha256_with_pre_return(
            loaded,
            options.workflow,
            options.goal,
            options.boundary,
            options.harness_receipt,
            Some(options.check_command),
            options.proof_origin,
            options.preserve_requirement,
            options.preservation_check_command,
            options.preservation_proof_origin,
            basis,
            options.review_policy,
            pre_return_command,
            pre_return_timeout_ms,
            pre_return_capture_bytes,
        )?;
        let scope = goal_scope
            .as_ref()
            .ok_or("native session Goal scope disappeared before Work admission")?;
        crate::session_goal::association::admit(
            &scope.root,
            loaded,
            candidate,
            &work,
            &intent,
            options.goal,
        )?;
    }
    if let Err(error) = run::start_with_policy_and_pre_return(
        loaded,
        options.workflow,
        options.goal,
        &ledger,
        options.boundary,
        options.harness_receipt,
        Some(options.check_command),
        options.proof_origin,
        options.preserve_requirement,
        options.preservation_check_command,
        options.preservation_proof_origin,
        basis,
        options.review_policy,
        pre_return_command,
        pre_return_timeout_ms,
        pre_return_capture_bytes,
    ) {
        if goal_candidate.is_some() {
            let link = crate::emitted::command_line(
                &["work", "link-session", &work, "--json"],
                Some(&loaded.path),
            );
            return Err(format!(
                "native journey admission remains unresolved for Work {work}; Work start may or may not exist: {error}. Inspect the native Goal status; if an exact Work start exists, recover only with `{link}`. Do not repeat work begin"
            ));
        }
        return Err(error);
    }
    if goal_candidate.is_some() {
        if let Err(error) = crate::session_goal::association::link_session(loaded, &work) {
            let detail = crate::emitted::command_line(
                &["work", "detail", &work, "--json"],
                Some(&loaded.path),
            );
            let link = crate::emitted::command_line(
                &["work", "link-session", &work, "--json"],
                Some(&loaded.path),
            );
            return Err(format!(
                "Work {work} was started, but its native journey admission remains unresolved: {error}; inspect with `{detail}` and recover only with `{link}`; do not repeat work begin"
            ));
        }
    }
    let next = next_for(loaded, &work, &ledger)?;
    // Preserve the committed Work even if navigation fails.
    let focus = match focus::write(loaded, &work) {
        Ok(()) => json!({"updated": true, "authority": "none"}),
        Err(error) => focus::recovery(loaded, &work, &error),
    };
    Ok(json!({"work": work, "next": next, "focus": focus}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn caller_basis_cannot_inject_native_journey_markers_at_common_begin_boundary() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let _sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = crate::test_root::temp("work-begin-basis-origin");
        let config_path = crate::project::onboarding::init_with_options(
            crate::project::onboarding::InitOptions {
                product_root: root.to_str().expect("UTF-8 test project path"),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            },
        )
        .expect("initialize isolated fixture project");
        let loaded = crate::config::load(Some(
            config_path.to_str().expect("UTF-8 fixture config path"),
        ))
        .expect("load isolated fixture project");
        let basis = json!({
            "version": 1,
            "constraints": ["exitbind.session-journey-integration.v1.header:{}"],
            "openZones": ["fixture zone"],
            "decisiveCases": ["fixture case"]
        })
        .to_string();

        let result = begin(
            &loaded,
            BeginOptions {
                workflow: "change",
                goal: "reject an injected journey marker",
                check_command: "true",
                boundary: None,
                harness_receipt: None,
                proof_origin: None,
                preserve_requirement: None,
                preservation_check_command: None,
                preservation_proof_origin: None,
                basis: BasisOrigin::Caller(Some(&basis)),
                review_policy: Some("required"),
                auto_goal_context: false,
            },
        );
        assert!(result
            .unwrap_err()
            .contains("journey integration scope must be selected through --journey-integration"));
        let runs = loaded
            .state_root
            .join(crate::project::layout_types::state_namespace())
            .join("runs");
        assert_eq!(
            fs::read_dir(runs).expect("read Work run directory").count(),
            0,
            "caller-controlled reserved markers must be rejected before any Work start"
        );

        let ordinary_basis = json!({
            "version": 1,
            "constraints": ["Preserve the ordinary caller review constraint."],
            "openZones": ["fixture zone"],
            "decisiveCases": ["fixture case"]
        })
        .to_string();
        let started = begin(
            &loaded,
            BeginOptions {
                workflow: "change",
                goal: "accept an ordinary nonreserved basis",
                check_command: "true",
                boundary: None,
                harness_receipt: None,
                proof_origin: None,
                preserve_requirement: None,
                preservation_check_command: None,
                preservation_proof_origin: None,
                basis: BasisOrigin::Caller(Some(&ordinary_basis)),
                review_policy: Some("required"),
                auto_goal_context: false,
            },
        )
        .expect("ordinary caller basis remains supported");
        let work = started["work"].as_str().expect("ordinary Work handle");
        let ledger = loaded
            .state_root
            .join(crate::work::resolve(&loaded, work).expect("resolve ordinary Work ledger"));
        let start: serde_json::Value = serde_json::from_str(
            fs::read_to_string(ledger)
                .expect("read ordinary Work ledger")
                .lines()
                .next()
                .expect("ordinary Work start event"),
        )
        .expect("parse ordinary Work start event");
        assert_eq!(
            start["basis"]["constraints"][0], "Preserve the ordinary caller review constraint.",
            "nonreserved caller basis must still reach the immutable Work start"
        );
    }
}
