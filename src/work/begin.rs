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
    pub(crate) basis: Option<&'a str>,
    pub(crate) review_policy: Option<&'a str>,
    pub(crate) auto_goal_context: bool,
}

pub(crate) fn begin(loaded: &Loaded, options: BeginOptions<'_>) -> Result<Value, String> {
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
    let goal_candidate = if options.auto_goal_context {
        crate::session_goal::association::prepare(loaded)?
    } else {
        None
    };
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
    let _started = run::start_with_policy(
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
        options.basis,
        options.review_policy,
    )?;
    let work = format!("{WORK_PREFIX}{token}");
    if let Some(candidate) = goal_candidate {
        let association = (|| {
            let (_, events, _) = run::ledger::load(loaded, &ledger)?;
            let start = events.first().ok_or("Work ledger has no start event")?;
            if start["action"] != "start" {
                return Err("Work ledger does not begin with a start event".into());
            }
            let start_event_sha256 = start["eventSha256"]
                .as_str()
                .ok_or("Work start event identity is missing")?;
            crate::session_goal::association::record(
                loaded,
                &candidate,
                &work,
                start_event_sha256,
            )?;
            Ok::<(), String>(())
        })();
        if let Err(error) = association {
            return Err(format!(
                "Work {work} was recorded, but its context-only Goal association failed: {error}; inspect with `exitbind work detail {work} --json`; do not repeat work begin"
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
