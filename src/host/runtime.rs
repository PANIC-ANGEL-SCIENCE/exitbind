use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

mod activation;
mod context;
pub(crate) use activation::{assess_activation, classify_activation, Activation, ActivationFacts};

const MAX_INPUT: usize = 64 * 1024;
const MAX_PROFILE: u64 = 12 * 1024;
const MAX_OUTPUT: usize = super::assignment_evidence::MAX_CONTEXT_OUTPUT;
const EVENTS: [&str; 6] = [
    "SessionStart",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "PostToolUse",
];

/// Hook execution is deliberately fail-open: malformed, ambiguous, or unsafe
/// host input produces no output and never turns a host session into an error.
pub fn run() -> Result<(), String> {
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
    let cwd = object
        .get("cwd")
        .or_else(|| object.get("current_directory"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let project = match absolute_directory(cwd) {
        Ok(project) => project,
        Err(()) => {
            emit_routing_with_recovery(
                event,
                update_context.as_deref(),
                Routing::Unresolved,
                object,
                None,
            )?;
            return Ok(());
        }
    };
    let portable = if crate::producer::exitbind_surface() {
        project.join("exitbind.json")
    } else {
        project.join("soulmate.json")
    };
    let portable_present = match fs::symlink_metadata(&portable) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => {
            emit_routing_with_recovery(
                event,
                update_context.as_deref(),
                Routing::Unresolved,
                object,
                None,
            )?;
            return Ok(());
        }
    };
    let config_path = if portable_present {
        let safe = fs::symlink_metadata(&portable)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            && contained(&project, &portable)
            && contained_existing(&project, &portable);
        if !safe {
            emit_routing_with_recovery(
                event,
                update_context.as_deref(),
                Routing::Unresolved,
                object,
                None,
            )?;
            return Ok(());
        }
        portable
    } else {
        match crate::project::portability::discover_from(&project) {
            Ok(Some(path)) => path,
            Ok(None) => {
                emit_routing_with_recovery(
                    event,
                    update_context.as_deref(),
                    Routing::CleanAbsence,
                    object,
                    None,
                )?;
                return Ok(());
            }
            Err(_) => {
                emit_routing_with_recovery(
                    event,
                    update_context.as_deref(),
                    Routing::Unresolved,
                    object,
                    None,
                )?;
                return Ok(());
            }
        }
    };
    let loaded = match config_path
        .to_str()
        .ok_or(())
        .and_then(|path| crate::config::load(Some(path)).map_err(|_| ()))
    {
        Ok(value) => value,
        Err(_) => {
            emit_routing_with_recovery(
                event,
                update_context.as_deref(),
                Routing::Unresolved,
                object,
                config_path.to_str(),
            )?;
            return Ok(());
        }
    };
    if !project.starts_with(&loaded.product_root)
        || (loaded.mode == crate::project::layout_types::Mode::Portable
            && !contained_existing(&loaded.product_root, &loaded.control_root))
    {
        emit_routing_with_recovery(
            event,
            update_context.as_deref(),
            Routing::Unresolved,
            object,
            config_path.to_str(),
        )?;
        return Ok(());
    }
    if event == "PostToolUse" {
        return match super::plan_card::on_approval(&loaded, object) {
            Some(line) => emit(event, &line),
            None => Ok(()),
        };
    }
    // Claude Code names an environment file for SessionStart; Codex does not.
    if event == "SessionStart" && std::env::var_os("CLAUDE_ENV_FILE").is_some() {
        if let Some(identity) = object.get("session_id").and_then(Value::as_str) {
            crate::session_goal::display::refresh_claude_session(&loaded, identity);
        }
    }
    let recovery_context = if event == "SessionStart" && crate::producer::exitbind_surface() {
        super::session_recovery::hook_context(object, config_path.to_str())
    } else {
        None
    };
    // Prepared child capture: claim at start, finalize at stop. Failures stay
    // inside Exitbind's private state and never fail the host.
    if event == "SubagentStart" {
        match retry_busy(|| crate::session_goal::claim_child(&loaded, object)) {
            Ok(Some(reason)) => {
                return emit(
                    event,
                    &format!(
                        "Exitbind rejected the prepared child context before presentation: {reason}. The current assignment context was not presented. Native host child creation is host-controlled."
                    ),
                );
            }
            Ok(None) => {}
            Err(error) => {
                return emit(
                    event,
                    &format!(
                        "Exitbind could not verify the prepared child context; no assignment context was presented: {error}. Native host child creation is host-controlled."
                    ),
                );
            }
        }
    }
    if event == "SubagentStop" {
        let _ = retry_busy(|| crate::session_goal::finalize_child(&loaded, object));
        return Ok(());
    }
    let text = if event == "SessionStart" {
        let mut text = session_summary(
            &loaded.config,
            &crate::project::agent_display::lead(&loaded),
        );
        let mut soft = 3072;
        if crate::producer::exitbind_surface() {
            let receive = super::receive_text::receive_work(Some(&loaded.path));
            soft = 3072 + super::receive_text::route_bytes(&receive);
            let fixed = DIRECT_WORK.len() + receive.len() + 2;
            if text.len() + fixed + 128 > soft {
                text = "Exitbind plan-only project context. Preserve the existing root conversation and current user corrections. No model was selected or launched; declarations are not an OS sandbox.".into();
            }
            text.push('\n');
            text.push_str(DIRECT_WORK);
            text.push('\n');
            text.push_str(&receive);
            let budget = soft.saturating_sub(text.len());
            text.push_str(&crate::project::context::session_addendum(&loaded, budget));
        } else if let Some(memory) = super::project_memory::session_context(&loaded) {
            text.push('\n');
            text.push_str(&memory);
        }
        if let Some(update) = update_context {
            if !crate::producer::exitbind_surface() || text.len() + update.len() < soft {
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
                    crate::emitted::command_text("session recovery show --json")
                );
                let recovery = recovery_context.as_deref().unwrap_or(&fallback);
                let receive = super::receive_text::receive_work(Some(&loaded.path));
                text = format!("Exitbind plan-only project context.\n{MANDATORY_SESSION_CORE}\n{DIRECT_WORK}\n{receive}\n{recovery}");
            }
            if text.len() > 4096 {
                let receive = super::receive_text::receive_work(Some(&loaded.path));
                text = format!("Exitbind session context is too large to present without cutting a route. Use the exact read-only recovery route; no saved action is replayed.\n{MANDATORY_SESSION_CORE}\n{DIRECT_WORK}\n{receive}");
            }
        }
        text
    } else {
        let Some(agent) = exact_agent(object, &loaded.agents) else {
            return Ok(());
        };
        let Some(configured) = loaded.agent(&agent) else {
            return Ok(());
        };
        let profile = match crate::config::file(&loaded.control_root, &configured.profile) {
            Ok(path) => path,
            Err(_) => return Ok(()),
        };
        let metadata = match fs::metadata(&profile) {
            Ok(info) => info,
            Err(_) => return Ok(()),
        };
        if metadata.len() > MAX_PROFILE {
            return Ok(());
        }
        let source = match fs::read_to_string(&profile) {
            Ok(value) => value,
            Err(_) => return Ok(()),
        };
        if !contained_existing(&loaded.control_root, &profile) {
            return Ok(());
        }
        let selected = super::assignment_context::selection(
            &loaded,
            &agent,
            &crate::evidence::hash::text(&source),
        );
        if let super::assignment_context::Selection::Mismatch(reason) = &selected {
            if event == "SubagentStart" {
                return emit(
                    event,
                    &format!(
                        "Exitbind could not present child context: assignment context mismatch: {reason}. Native host child creation is host-controlled."
                    ),
                );
            }
            return Ok(());
        }
        if let super::assignment_context::Selection::Unavailable(reason) = &selected {
            return emit(
                event,
                &format!(
                    "Exitbind could not verify the current assignment; profile acquisition is unavailable: {reason}."
                ),
            );
        }
        let Some(mut context) = context::format_agent_context(
            &loaded.control_root,
            &agent,
            configured,
            &profile,
            &source,
            &selected,
        ) else {
            return Ok(());
        };
        if event == "SubagentStart" {
            match crate::session_goal::claimed_child_context(&loaded, object) {
                Ok(Some(child)) => {
                    let matches = child["configuredAgent"] == agent
                        && matches!(
                            &selected,
                            super::assignment_context::Selection::Bound { work, assignment, guidance, .. }
                                if child["work"] == work.as_str()
                                    && child["assignment"] == assignment.as_str()
                                    && child["guidance"] == *guidance
                        );
                    if !matches {
                        return emit(
                            event,
                            &format!(
                                "Exitbind could not present child context: prepared child scope conflicts with the selected assignment (configured agent {agent}). Native host child creation is host-controlled."
                            ),
                        );
                    }
                    context.push_str("\nSelected task perspectives for this assignment:");
                    if let Some(specialist) = child.get("specialistExperience") {
                        context.push_str("\nCurrent qualified specialist experience (complete; observed use remains separate):\n");
                        context.push_str(
                            &serde_json::to_string(specialist).map_err(|e| e.to_string())?,
                        );
                    }
                    if let Some(capabilities) = child.get("capabilities") {
                        context.push_str("\nCurrent approved project capabilities (complete; native use remains separate):\n");
                        context.push_str(
                            &serde_json::to_string(capabilities).map_err(|e| e.to_string())?,
                        );
                    }
                    for perspective in child["perspectives"].as_array().into_iter().flatten() {
                        context.push_str(&format!(
                            "\nPerspective {} (source SHA-256 {}, presented SHA-256 {}, complete):\n{}",
                            perspective["id"].as_str().unwrap_or("?"),
                            perspective["sourceSha256"].as_str().unwrap_or("?"),
                            perspective["presentedSha256"].as_str().unwrap_or("?"),
                            perspective["content"].as_str().unwrap_or("")
                        ));
                    }
                }
                Err(_) => {
                    return emit(event, "Exitbind could not verify the selected child context; assignment context is unavailable.");
                }
                Ok(None) => {}
            }
        }
        let core_serialized = serde_json::to_vec(&json!({"hookSpecificOutput":{
            "hookEventName":event,"additionalContext":context}}))
        .map_err(|_| String::new())?;
        if core_serialized.len() > MAX_OUTPUT {
            "Exitbind profile, architecture or perspective exceeds the complete child context envelope; acquisition is unavailable.".to_owned()
        } else {
            let evidence = match &selected {
                super::assignment_context::Selection::Bound { evidence, .. } => evidence.as_slice(),
                _ => &[],
            };
            match super::assignment_evidence::append_within_budget(&context, event, evidence) {
                Some(context) => {
                    let serialized = serde_json::to_vec(&json!({"hookSpecificOutput":{
                        "hookEventName":event,"additionalContext":context}}))
                    .map_err(|_| String::new())?;
                    if serialized.len() <= MAX_OUTPUT {
                        context
                    } else {
                        "Exitbind profile, architecture or perspective exceeds the complete child context envelope; acquisition is unavailable.".to_owned()
                    }
                }
                None => "Evidence route or shortened-preview notice cannot fit beside the required profile and perspectives; assignment context is unavailable.".to_owned(),
            }
        }
    };
    emit(event, &text)
}

/// A session in a repository Exitbind does not govern yet still needs to know
/// Exitbind exists and when to reach for it. This stays small: the detailed
/// protocol arrives only after the project is configured.
#[derive(Clone, Copy)]
enum Routing {
    CleanAbsence,
    Unresolved,
}

const NATIVE_ABSENCE: &str = "Exitbind is available but not active for this task. Ordinary work may use the native host without initialization. Use Exitbind only when the task or existing project policy requires governed acceptance.";
const UNRESOLVED: &str = "Exitbind could not verify this target or its configuration. Check the target path and existing project policy before proceeding; no native-only status was established.";
const DIRECT_WORK: &str = "For small, low-consequence reversible edits, work directly: do not run Exitbind commands, initialize a project, or ask workflow or review-policy questions. An instruction or configuration filename alone does not make a change consequential; assess its actual effects and applicable project requirements. Classification is the lead's job, not a user questionnaire. Reuse existing scoped authorization and review decisions; ask only when a genuinely new decision is needed.";

const MANDATORY_SESSION_CORE: &str = "Preserve the existing root host conversation; project context only augments it. Do not replace, reset, fork, or request compaction of it for role loading or handoff. Keep recent user corrections and rejected approaches with their rationale; refer frozen-run conflicts to the existing lead for explicit supersession. Native conversational recall is distinct from durable role memory. When a Work or Goal response carries a non-null `presentation.terminal`, print that value on a line of its own exactly as given. For a Goal `completionEvent`, print once per event ID; consume the normal closure response without status polling. Recover a lost closure reply through its exact read-only route. No model was selected or launched; declarations are not an OS sandbox.";

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
        super::session_recovery::hook_context(payload, cwd_config_path)
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
            println!("{output}");
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

fn session_summary(config: &Value, lead_display: &str) -> String {
    let compact = crate::producer::exitbind_surface();
    let names = |key: &str| {
        let Some(map) = config[key].as_object() else {
            return if compact { "none" } else { "" }.to_owned();
        };
        let mut names = map.keys().cloned().collect::<Vec<_>>();
        names.sort();
        if compact && names.len() > 4 {
            let omitted = names.len() - 4;
            names.truncate(4);
            format!(
                "{}, +{omitted} more (see project agents --json)",
                names.join(", ")
            )
        } else if names.is_empty() {
            if compact { "none" } else { "" }.to_owned()
        } else {
            names.join(", ")
        }
    };
    let agents = names("agents");
    let workflows = names("workflows");
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    bounded(format!("{product} plan-only project context.\nLead: {}\nNamed agents: {}\nWorkflows: {}\n{MANDATORY_SESSION_CORE}", safe_inline(lead_display), safe_inline(if agents.is_empty() { "none" } else { &agents }), safe_inline(if workflows.is_empty() { "none" } else { &workflows })))
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
fn absolute_directory(value: &str) -> Result<PathBuf, ()> {
    if value.is_empty() || value.contains('\0') || !Path::new(value).is_absolute() {
        return Err(());
    }
    let path = PathBuf::from(value);
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|_| ())?;
        if metadata.file_type().is_symlink() {
            return Err(());
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
    if !metadata.is_dir() {
        return Err(());
    }
    fs::canonicalize(path).map_err(|_| ())
}
fn contained(root: &Path, target: &Path) -> bool {
    target.starts_with(root)
}
fn contained_existing(root: &Path, target: &Path) -> bool {
    fs::canonicalize(target)
        .ok()
        .is_some_and(|path| path.starts_with(root))
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
