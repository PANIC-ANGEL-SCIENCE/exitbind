use serde_json::{json, Value};
use std::io::{self, Read, Write};

mod activation;
mod actor;
mod child;
mod context;
pub(crate) mod multipart;
mod project;
mod stop;
pub(crate) use activation::{assess_activation, classify_activation, Activation, ActivationFacts};
pub(crate) use actor::projection as selected_actor_projection;
pub(crate) use child::preflight as preflight_child_transport;

const MAX_INPUT: usize = 64 * 1024;
const MAX_PROFILE: u64 = 12 * 1024;
const MAX_OUTPUT: usize = super::assignment_evidence::MAX_CONTEXT_OUTPUT;
const EVENTS: [&str; 7] = [
    "SessionStart",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "PostToolUse",
    "Stop",
];

/// Hook execution is deliberately fail-open: malformed, ambiguous, or unsafe
/// host input produces no output and never turns a host session into an error.
pub fn run(host: Option<&str>, slot: Option<usize>) -> Result<(), String> {
    let mut bytes = Vec::new();
    let mut input = io::stdin().take((MAX_INPUT + 1) as u64);
    input.read_to_end(&mut bytes).map_err(|_| String::new())?;
    if bytes.len() > MAX_INPUT {
        return Ok(());
    }
    let payload: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    let Some(object) = payload.as_object() else {
        return Ok(());
    };
    let event = object
        .get("hook_event_name")
        .or_else(|| object.get("hookEventName"))
        .or_else(|| object.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if !crate::producer::exitbind_surface() && matches!(event, "PreCompact" | "PostCompact") {
        return Ok(());
    }
    if !EVENTS.contains(&event) {
        return Ok(());
    }
    if slot.is_some() && event != "SubagentStart" {
        return Ok(());
    }
    if event == "Stop" {
        if crate::producer::exitbind_surface() && stop::should_block(host, object) {
            println!(
                "{}",
                json!({"decision":"block","reason":"The session Goal is open. Continue the authorized work."})
            );
        }
        return Ok(());
    }
    if event == "PostToolUse"
        && !(crate::producer::exitbind_surface() && super::plan_card::is_plan_approval(object))
    {
        return Ok(());
    }
    if event == "SessionStart" {
        super::native_session::export(object, std::env::var_os("CLAUDE_ENV_FILE").as_deref());
    }
    if matches!(event, "PreCompact" | "PostCompact") {
        super::session_recovery::checkpoint(object, event);
        return Ok(());
    }
    let update_context = if event == "SessionStart" {
        crate::distribution::update::session_context()
    } else {
        None
    };
    let (loaded, config_path, source, recovery) = match project::resolve(object) {
        project::Resolution::Project {
            loaded,
            config_path,
            source,
            recovery,
        } => (*loaded, config_path, source, recovery),
        project::Resolution::CleanAbsence => {
            emit_routing_with_recovery(
                event,
                update_context.as_deref(),
                Routing::CleanAbsence,
                object,
                None,
            )?;
            return Ok(());
        }
        project::Resolution::Unresolved(config_path) => {
            emit_routing_with_recovery(
                event,
                update_context.as_deref(),
                Routing::Unresolved,
                object,
                config_path.as_deref(),
            )?;
            return Ok(());
        }
    };
    let configuration = project::fact_line(&config_path, source);
    if event == "PostToolUse" {
        return match super::plan_card::on_approval(&loaded, object) {
            Some(line) => emit(event, &format!("{line} {configuration}")),
            None => Ok(()),
        };
    }
    // Claude Code names an environment file for SessionStart; Codex does not.
    if event == "SessionStart" && std::env::var_os("CLAUDE_ENV_FILE").is_some() {
        if let Some(identity) = object.get("session_id").and_then(Value::as_str) {
            crate::session_goal::display::refresh_claude_session(
                &loaded,
                identity,
                recovery.as_deref(),
            );
        }
    }
    let session_facts = (event == "SessionStart" && crate::producer::exitbind_surface())
        .then(|| crate::project::context::snapshot(&loaded));
    let recovery_context = if let Some(facts) = session_facts.as_ref() {
        let lead = crate::project::agent_display::lead(&loaded);
        let required = crate::project::context::session_required(&loaded, facts);
        let summary = minimal_session_summary(&lead, facts, false);
        let tight_summary = minimal_session_summary(&lead, facts, true);
        let compact = session_base(
            &summary,
            DIRECT_WORK_COMPACT,
            &super::receive_text::receive_work_compact(Some(&loaded.path)),
            &project::routed_fact_line(source),
            &required,
        );
        let tight = session_base(
            &tight_summary,
            DIRECT_WORK_TIGHT,
            &super::receive_text::receive_work_tight(Some(&loaded.path)),
            &project::tight_routed_fact_line(source),
            &required,
        );
        let available = 4096usize.saturating_sub(compact.len().min(tight.len()) + 1);
        super::session_recovery::hook_context(object, config_path.to_str(), available)
    } else {
        None
    };
    // A matched same-host child already identifies an owner-verified Work
    // session. Carry that context only for this callback's Goal projections;
    // do not mutate the process environment or replace conflicting selectors.
    let _callback_scope = if matches!(event, "SubagentStart" | "SubagentStop") {
        match crate::session_goal::child_callback_scope(&loaded, object, event) {
            Ok(scope) => scope,
            Err(reason) if event == "SubagentStart" => {
                if slot.is_some() {
                    multipart::invalidate(&loaded, object);
                }
                return emit_slot(
                    event,
                    &format!("Exitbind could not verify the selected child context; assignment context is unavailable: {reason}."),
                    slot,
                );
            }
            Err(reason) => {
                let _ = retry_busy(|| {
                    crate::session_goal::finalize_child(&loaded, object, Some(&reason))
                });
                return Ok(());
            }
        }
    } else {
        None
    };
    // Prepared child capture: claim at start, finalize at stop. Failures stay
    // inside Exitbind's private state and never fail the host.
    if event == "SubagentStart" {
        match retry_busy(|| crate::session_goal::claim_child(&loaded, object)) {
            Ok(Some(reason)) => {
                return emit_slot(
                    event,
                    &format!(
                        "Exitbind rejected the prepared child context before presentation: {reason}. The current assignment context was not presented. Native host child creation is host-controlled."
                    ),
                    slot,
                );
            }
            Ok(None) => {}
            Err(error) => {
                return emit_slot(
                    event,
                    &format!(
                        "Exitbind could not verify the prepared child context; no assignment context was presented: {error}. Native host child creation is host-controlled."
                    ),
                    slot,
                );
            }
        }
    }
    if event == "SubagentStop" {
        let _ = retry_busy(|| crate::session_goal::finalize_child(&loaded, object, None));
        return Ok(());
    }
    let mut presented_child = None;
    let text = if event == "SessionStart" {
        let lead = crate::project::agent_display::lead(&loaded);
        let mut text = session_summary(&loaded.config, Some(&loaded.path), &lead);
        if crate::producer::exitbind_surface() {
            let mut receive = super::receive_text::receive_work(Some(&loaded.path));
            let mut direct = DIRECT_WORK;
            let facts = session_facts.as_ref().expect("session facts");
            let required = crate::project::context::session_required(&loaded, facts);
            // Every route above prints the exact `--config` path; the fact
            // line says which configuration that is.
            let mut line = project::routed_fact_line(source);
            let fixed = |receive: &str, direct: &str, line: &str| {
                direct.len()
                    + receive.len()
                    + line.len()
                    + required.len()
                    + crate::project::context::session_reference_reserve(facts)
                    + 3
            };
            // Deep paths: the complete-lists route goes before the core does.
            if text.len() + fixed(&receive, direct, &line) > 3072 {
                text = session_summary(&loaded.config, None, &lead);
            }
            if text.len() + fixed(&receive, direct, &line) > 3072 {
                text = minimal_session_summary(&lead, facts, false);
            }
            if text.len() + fixed(&receive, direct, &line) > 3072 {
                receive = super::receive_text::receive_work_compact(Some(&loaded.path));
            }
            if text.len() + fixed(&receive, direct, &line) > 3072 {
                direct = DIRECT_WORK_COMPACT;
            }
            if text.len() + fixed(&receive, direct, &line) > 3072 {
                text = minimal_session_summary(&lead, facts, true);
                receive = super::receive_text::receive_work_tight(Some(&loaded.path));
                direct = DIRECT_WORK_TIGHT;
                line = project::tight_routed_fact_line(source);
            }
            if text.len() + fixed(&receive, direct, &line) > 3072 {
                return Err(
                    "complete mandatory session context and routes exceed 3072 bytes".into(),
                );
            }
            // Keep the complete update notice when it fits with the required
            // context and the selected navigation routes.
            let update_bytes = update_context.as_ref().map_or(0, |update| update.len() + 1);
            let mut update_reserve = 0;
            if update_bytes > 0 {
                if text.len() + fixed(&receive, direct, &line) + update_bytes <= 3072 {
                    update_reserve = update_bytes;
                } else {
                    let minimal = minimal_session_summary(&lead, facts, false);
                    if minimal.len() + fixed(&receive, direct, &line) + update_bytes <= 3072 {
                        text = minimal;
                        update_reserve = update_bytes;
                    } else {
                        let compact = super::receive_text::receive_work_compact(Some(&loaded.path));
                        if minimal.len() + fixed(&compact, direct, &line) + update_bytes <= 3072 {
                            text = minimal;
                            receive = compact;
                            update_reserve = update_bytes;
                        }
                    }
                }
            }
            text.push('\n');
            text.push_str(direct);
            text.push('\n');
            text.push_str(&receive);
            text.push('\n');
            text.push_str(&line);
            // Required facts precede the reserved complete notice; remaining
            // space may hold optional diagnostics and references.
            let budget = 3072usize.saturating_sub(text.len() + update_reserve);
            text.push_str(&crate::project::context::session_addendum(
                &loaded, facts, budget,
            ));
        } else if let Some(memory) = super::project_memory::session_context(&loaded) {
            text.push('\n');
            text.push_str(&memory);
        }
        if let Some(update) = update_context {
            if !crate::producer::exitbind_surface() || text.len() + update.len() < 3072 {
                text.push('\n');
                text.push_str(&update);
            }
        }
        if let Some(recovery) = recovery_context.as_deref() {
            text.push('\n');
            text.push_str(recovery);
        }
        if crate::producer::exitbind_surface() {
            if crate::session_goal::native::current()
                .ok()
                .flatten()
                .is_some()
            {
                text = text.replace("\nCurrent work:", "\nProject-scoped Work focus:");
            }
            if text.len() > 4096 {
                let fallback = format!(
                    "Use `{}` for the exact bounded recovery record.",
                    crate::emitted::Route::new(["session", "recovery", "show", "--json"])
                        .config(&loaded.path)
                        .shell_text()
                );
                let recovery = recovery_context.as_deref().unwrap_or(&fallback);
                let receive = super::receive_text::receive_work_compact(Some(&loaded.path));
                let facts = session_facts.as_ref().expect("session facts");
                let required = crate::project::context::session_required(&loaded, facts);
                let base = |direct| {
                    session_base(
                        &minimal_session_summary(&lead, facts, false),
                        direct,
                        &receive,
                        &project::routed_fact_line(source),
                        &required,
                    )
                };
                let tight = session_base(
                    &minimal_session_summary(&lead, facts, true),
                    DIRECT_WORK_TIGHT,
                    &super::receive_text::receive_work_tight(Some(&loaded.path)),
                    &project::tight_routed_fact_line(source),
                    &required,
                );
                text = [
                    format!("{}\n{recovery}", base(DIRECT_WORK)),
                    format!("{}\n{recovery}", base(DIRECT_WORK_COMPACT)),
                    format!("{tight}\n{recovery}"),
                    format!("{}\n{fallback}", base(DIRECT_WORK_COMPACT)),
                    format!("{tight}\n{fallback}"),
                ]
                .into_iter()
                .find(|candidate| candidate.len() <= 4096)
                .ok_or(
                    "complete mandatory session context and recovery routes exceed 4096 bytes",
                )?;
            }
        }
        text
    } else {
        let Some(agent) = exact_agent(object, &loaded.agents) else {
            if slot.is_some() {
                multipart::invalidate(&loaded, object);
            }
            return Ok(());
        };
        let Some(configured) = loaded.agent(&agent) else {
            if slot.is_some() {
                multipart::invalidate(&loaded, object);
            }
            return Ok(());
        };
        let Some((profile, source)) = child::profile(&loaded, configured) else {
            if slot.is_some() {
                multipart::invalidate(&loaded, object);
            }
            return Ok(());
        };
        let selected = super::assignment_context::selection(
            &loaded,
            &agent,
            &crate::evidence::hash::text(&source),
        );
        if let super::assignment_context::Selection::Mismatch(reason) = &selected {
            if event == "SubagentStart" {
                if slot.is_some() {
                    multipart::invalidate(&loaded, object);
                }
                return emit_slot(
                    event,
                    &format!(
                        "Exitbind could not present child context: assignment context mismatch: {reason}. Native host child creation is host-controlled."
                    ),
                    slot,
                );
            }
            return Ok(());
        }
        if let super::assignment_context::Selection::Unavailable(reason) = &selected {
            if slot.is_some() {
                multipart::invalidate(&loaded, object);
            }
            return emit_slot(
                event,
                &format!(
                    "Exitbind could not verify the current assignment; profile acquisition is unavailable: {reason}."
                ),
                slot,
            );
        }
        let Some(mut context) = child::base(
            &loaded,
            &agent,
            configured,
            &profile,
            &source,
            &selected,
            &configuration,
        ) else {
            if slot.is_some() {
                multipart::invalidate(&loaded, object);
            }
            return Ok(());
        };
        let mut finalized_context = false;
        let mut claude_delivery = false;
        if event == "SubagentStart" {
            match crate::session_goal::claimed_child_context(&loaded, object) {
                Ok(Some(child)) => {
                    claude_delivery = child["deliveryHost"] == "claude";
                    let matches = child["configuredAgent"] == agent
                        && matches!(
                            &selected,
                            super::assignment_context::Selection::Bound { work, assignment, guidance, .. }
                                if child["work"] == work.as_str()
                                    && child["assignment"] == assignment.as_str()
                                    && child["guidance"] == *guidance
                        );
                    if !matches {
                        if slot.is_some() {
                            multipart::invalidate(&loaded, object);
                        }
                        return emit_slot(
                            event,
                            &format!(
                                "Exitbind could not present child context: prepared child scope conflicts with the selected assignment (configured agent {agent}). Native host child creation is host-controlled."
                            ),
                            slot,
                        );
                    }
                    if child["deliveryHost"] == "codex" {
                        let evidence = match &selected {
                            super::assignment_context::Selection::Bound { evidence, .. } => {
                                evidence.as_slice()
                            }
                            _ => &[],
                        };
                        context = context::codex_unverified_with_evidence(event, evidence);
                        finalized_context = true;
                    } else {
                        let rendered = child::render(
                            &loaded,
                            &child,
                            &selected,
                            &mut context,
                            event,
                            slot.is_some(),
                        );
                        if rendered.is_err() && slot.is_some() {
                            multipart::invalidate(&loaded, object);
                        }
                        if rendered? {
                            presented_child = Some(child.clone());
                            finalized_context = true;
                        }
                    }
                }
                Err(_) => {
                    let evidence = match &selected {
                        super::assignment_context::Selection::Bound { evidence, .. } => {
                            evidence.as_slice()
                        }
                        _ => &[],
                    };
                    let message = context::unavailable_with_evidence(event, evidence);
                    if slot.is_some() {
                        multipart::invalidate(&loaded, object);
                        return emit_slot(event, &message, slot);
                    }
                    emit(event, &message)?;
                    context::report_delivery_record(retry_busy(|| {
                        context::note_delivery(&loaded, object, None)
                    }));
                    return Ok(());
                }
                Ok(None) => {}
            }
        }
        if finalized_context {
            context
        } else {
            let evidence = match &selected {
                super::assignment_context::Selection::Bound { evidence, .. } => evidence.as_slice(),
                _ => &[],
            };
            context::fallback_context(event, &context, claude_delivery, evidence)?
        }
    };
    if let Some(index) = slot {
        if let Some(child) = presented_child.as_ref() {
            return multipart::deliver(&loaded, object, child, &text, index);
        }
        return emit_slot(event, &text, slot);
    }
    emit(event, &text)?;
    if event == "SubagentStart" {
        context::report_delivery_record(retry_busy(|| {
            context::note_delivery(&loaded, object, presented_child.as_ref())
        }));
    }
    Ok(())
}

fn emit_slot(event: &str, text: &str, slot: Option<usize>) -> Result<(), String> {
    if slot.is_some_and(|index| index != 0) {
        return Ok(());
    }
    if slot.is_some() && super::assignment_evidence::append_for_claude(text, event, &[]).is_none() {
        return emit(event, "Exitbind could not fit the complete child profile and context into the Claude hook envelope; acquisition is unavailable.");
    }
    emit(event, text)
}

/// A session in a repository Exitbind does not govern yet still needs to know
/// Exitbind exists and when to reach for it. This stays small: the detailed
/// protocol arrives only after the project is configured.
#[derive(Clone, Copy)]
enum Routing {
    CleanAbsence,
    Unresolved,
}

fn minimal_session_summary(lead: &str, facts: &Value, omit_duplicate: bool) -> String {
    let duplicate = facts["lead"]["agentId"].as_str() == Some(lead)
        && facts["lead"]["nativeName"].as_str() == Some(lead);
    let label = if omit_duplicate && duplicate {
        String::new()
    } else {
        format!("Lead: {}\n", safe_inline(lead))
    };
    let title = if omit_duplicate {
        "Exitbind plan-only."
    } else {
        "Exitbind plan-only project context."
    };
    let core = if omit_duplicate {
        MANDATORY_SESSION_CORE_TIGHT
    } else {
        MANDATORY_SESSION_CORE
    };
    format!("{title}\n{label}{core}")
}

fn session_base(summary: &str, direct: &str, receive: &str, line: &str, required: &str) -> String {
    format!("{summary}\n{direct}\n{receive}\n{line}{required}")
}

const NATIVE_ABSENCE: &str = "Exitbind is available but not active for this task. Ordinary work may use the native host without initialization. Use Exitbind only when the task or existing project policy requires governed acceptance.";
const UNRESOLVED: &str = "Exitbind could not verify this target or its configuration. Check the target path and existing project policy before proceeding; no native-only status was established.";
const DIRECT_WORK: &str = "For small, low-consequence reversible edits, work directly: do not run Exitbind commands, initialize a project, or ask workflow or review-policy questions. An instruction or configuration filename alone does not make a change consequential; assess its actual effects and applicable project requirements. Classification is the lead's job, not a user questionnaire. Reuse existing scoped authorization and review decisions; ask only when a genuinely new decision is needed.";
const DIRECT_WORK_COMPACT: &str = "For small, low-consequence reversible edits, work directly: do not run Exitbind commands, initialize a project, or ask workflow or review-policy questions. Judge actual effects and project requirements, not instruction or configuration filenames; the Lead classifies. Reuse scoped authorization and review decisions; ask only for a genuinely new decision.";
const DIRECT_WORK_TIGHT: &str = "Minor reversible edits: do not run Exitbind commands, initialize a project or ask workflow/review; the Lead classifies effects, not filenames. Reuse scoped authority/review decisions; ask only new choices.";

const MANDATORY_SESSION_CORE: &str = "Preserve the existing root host conversation; project context only augments it. Do not replace, reset, fork, or request compaction of it for role loading or handoff. Keep recent user corrections and rejected approaches with their rationale; refer frozen-run conflicts to the existing lead for explicit supersession. Native conversational recall is distinct from durable role memory. When a Work or Goal response carries a non-null `presentation.terminal`, print that value on a line of its own exactly as given. For a Goal `completionEvent`, print once per event ID; consume the normal closure response without status polling. Recover a lost closure reply through its exact read-only route. No model was selected or launched; declarations are not an OS sandbox.";
// Keep all mandatory semantics when only the final tight fallback can fit.
const MANDATORY_SESSION_CORE_TIGHT: &str = "Preserve the existing root host conversation; project context only augments it. Do not replace, reset, fork, or request compaction for role loading or handoff. Keep recent user corrections and rejected approaches with reasons; refer frozen-run conflicts to the existing lead for explicit supersession. Native conversational recall differs from durable role memory. Print non-null Work/Goal `presentation.terminal` verbatim on its own line. Print a Goal's `presentation.terminal` once per `completionEvent.id` from the normal closure response; do not poll status. Recover lost closure replies via their exact read-only routes. No model was selected or launched; declarations are not an OS sandbox.";

fn retry_busy<T>(action: impl Fn() -> Result<T, String>) -> Result<T, String> {
    for _ in 0..20 {
        match action() {
            Err(error) if error.contains("busy") => {
                std::thread::sleep(std::time::Duration::from_millis(100))
            }
            other => return other,
        }
    }
    Err("child capture state stayed busy".into())
}

fn routing_text(
    event: &str,
    update: Option<&str>,
    routing: Routing,
    recovery: Option<&str>,
) -> Option<String> {
    // The legacy Soulmate surface keeps its original silent contract; only the
    // Exitbind surface offers the bootstrap.
    if event != "SessionStart" || !crate::producer::exitbind_surface() {
        let mut text = update.map(str::to_owned).unwrap_or_default();
        if let Some(recovery) = recovery {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(recovery);
        }
        return (!text.is_empty()).then_some(text);
    }
    let mut text = match routing {
        Routing::CleanAbsence => NATIVE_ABSENCE.to_owned(),
        Routing::Unresolved => UNRESOLVED.to_owned(),
    };
    if let Some(update) = update {
        text.push('\n');
        text.push_str(update);
    }
    if let Some(recovery) = recovery {
        text.push('\n');
        text.push_str(recovery);
    }
    if text.len() > 4096 {
        return Some(format!("Exitbind could not fit the complete session context without cutting a route. Use the exact read-only recovery route; no saved action is replayed.\n{MANDATORY_SESSION_CORE}\n{DIRECT_WORK}\n{}", super::receive_text::receive_work(None)));
    }
    Some(bounded(text))
}

fn emit_routing_with_recovery(
    event: &str,
    update: Option<&str>,
    routing: Routing,
    payload: &serde_json::Map<String, Value>,
    cwd_config_path: Option<&str>,
) -> Result<(), String> {
    let recovery = if event == "SessionStart" && crate::producer::exitbind_surface() {
        super::session_recovery::hook_context(payload, cwd_config_path, 2048)
    } else {
        None
    };
    if let Some(text) = routing_text(event, update, routing, recovery.as_deref()) {
        emit(event, &text)?;
    }
    Ok(())
}

/// Emit within the host's output cap. An oversized context is shortened with
/// a marker instead of being dropped, so the routing rules at its start stay.
fn emit(event: &str, text: &str) -> Result<(), String> {
    let mut text = text.to_owned();
    if event == "SessionStart" && text.len() > 4096 {
        text = format!("Exitbind session context is too large to present without cutting a route. Use the exact read-only recovery route; no saved action is replayed.\n{MANDATORY_SESSION_CORE}\n{DIRECT_WORK}\n{}", super::receive_text::receive_work(None));
    }
    loop {
        let output = serde_json::to_string(
            &json!({"hookSpecificOutput":{"hookEventName":event,"additionalContext":text}}),
        )
        .map_err(|_| String::new())?;
        if output.len() <= MAX_OUTPUT {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(output.as_bytes())
                .and_then(|()| stdout.write_all(b"\n"))
                .and_then(|()| stdout.flush())
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        let keep = text.len() * 3 / 4;
        if keep < 64 {
            return Ok(());
        }
        let end = text
            .char_indices()
            .take_while(|(index, _)| *index < keep)
            .map(|(index, _)| index)
            .last()
            .unwrap_or(0);
        text = format!("{}\n[context truncated]", &text[..end]);
    }
}

fn exact_agent(
    payload: &serde_json::Map<String, Value>,
    agents: &std::collections::BTreeMap<String, crate::config::types::AgentConfig>,
) -> Option<String> {
    let mut selected: Option<String> = None;
    for key in ["agent_name", "agent_type", "subagent_type"] {
        let Some(candidate) = payload.get(key).and_then(Value::as_str) else {
            continue;
        };
        let matches = agents
            .iter()
            .filter(|(name, agent)| {
                candidate == agent.native_name(name)
                    || (!crate::producer::exitbind_surface() && candidate == name.as_str())
            })
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        let [name] = matches[..] else {
            return None;
        };
        if selected
            .as_deref()
            .is_some_and(|previous| previous != name.as_str())
        {
            return None;
        }
        selected = Some((*name).clone());
    }
    selected
}

/// The plan-only session summary; `path` adds the route to the complete agent
/// and workflow lists when a list was truncated.
fn session_summary(config: &Value, path: Option<&std::path::Path>, lead_display: &str) -> String {
    let compact = crate::producer::exitbind_surface();
    let mut truncated = false;
    let mut names = |key: &str| {
        let Some(map) = config[key].as_object() else {
            return if compact { "none" } else { "" }.to_owned();
        };
        let mut names = map.keys().cloned().collect::<Vec<_>>();
        names.sort();
        if compact && names.len() > 4 {
            let omitted = names.len() - 4;
            names.truncate(4);
            truncated = true;
            format!("{}, +{omitted} more", names.join(", "))
        } else if names.is_empty() {
            if compact { "none" } else { "" }.to_owned()
        } else {
            names.join(", ")
        }
    };
    let agents = names("agents");
    let workflows = names("workflows");
    // One complete-list route for every truncated list.
    let complete = match path.filter(|_| truncated) {
        Some(path) => format!(
            "Complete lists: {}\n",
            crate::emitted::command_line(&["project", "agents", "--json"], Some(path))
        ),
        None => String::new(),
    };
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    bounded(format!("{product} plan-only project context.\nLead: {}\nNamed agents: {}\nWorkflows: {}\n{complete}{MANDATORY_SESSION_CORE}", safe_inline(lead_display), safe_inline(if agents.is_empty() { "none" } else { &agents }), safe_inline(if workflows.is_empty() { "none" } else { &workflows })))
}

fn bounded(value: String) -> String {
    if value.len() <= MAX_OUTPUT {
        return safe_multiline(&value);
    }
    let limit = MAX_OUTPUT - 32;
    let end = value
        .char_indices()
        .take_while(|(index, _)| *index < limit)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0);
    format!("{}\n[context truncated]", safe_multiline(&value[..end]))
}
pub(crate) fn redact(value: &str) -> String {
    let source = safe_multiline(value);
    let home_prefix = concat!("/", "home", "/");
    let user_prefix = concat!("/", "Users", "/");
    redact_prefix(
        &redact_prefix(&source, home_prefix, "[home path redacted]"),
        user_prefix,
        "[user path redacted]",
    )
}
fn redact_prefix(source: &str, prefix: &str, replacement: &str) -> String {
    let mut output = String::new();
    let mut rest = source;
    while let Some(index) = rest.find(prefix) {
        output.push_str(&rest[..index]);
        let end = rest[index..]
            .find(char::is_whitespace)
            .map(|offset| index + offset)
            .unwrap_or(rest.len());
        output.push_str(replacement);
        rest = &rest[end..];
    }
    output.push_str(rest);
    output
}
fn safe_inline(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
fn safe_multiline(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{assess_activation, classify_activation, redact, Activation, ActivationFacts};

    #[test]
    fn activation_uses_consequence_and_promotion_not_file_count() {
        assert_eq!(
            classify_activation(false, false, false, false),
            Activation::Direct
        );
        assert_eq!(
            classify_activation(true, false, true, true),
            Activation::Governed
        );
        assert_eq!(
            classify_activation(true, false, false, false),
            Activation::Blocked
        );
        assert_eq!(
            classify_activation(false, true, true, false),
            Activation::Blocked
        );
    }

    #[test]
    fn activation_assessment_is_typed_and_provenance_labeled() {
        let assessment = assess_activation(ActivationFacts {
            material_consequence: false,
            promotion_required: false,
            available: false,
            activated: false,
        });
        assert_eq!(assessment.activation, Activation::Direct);
        assert_eq!(assessment.reason.as_str(), "no_material_consequence");
        assert_eq!(assessment.provenance, "host_reported");
        assert_eq!(
            assessment.value(),
            json!({
                "activation": "direct",
                "reason": "no_material_consequence",
                "provenance": "host_reported"
            })
        );
    }

    #[test]
    fn redact_replaces_machine_home_paths_and_preserves_non_paths() {
        let home_path = ["/", "home", "/", "account", "/project"].concat();
        let user_path = ["/", "Users", "/", "account", "/project"].concat();
        let input = format!("home={home_path}\nuser={user_path}\nordinary text stays");

        assert_eq!(
            redact(&input),
            "home=[home path redacted]\nuser=[user path redacted]\nordinary text stays"
        );
    }
}
