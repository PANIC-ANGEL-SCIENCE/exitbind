use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const MAX_INPUT: usize = 64 * 1024;
const MAX_PROFILE: u64 = 12 * 1024;
const MAX_OUTPUT: usize = 16 * 1024;
const EVENTS: [&str; 2] = ["SessionStart", "SubagentStart"];

/// Classify activation from consequence and promotion requirements. A count of
/// touched files is intentionally absent: harmless work can span many files,
/// while one consequential change can require governance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Activation {
    Direct,
    Governed,
    Blocked,
}

impl Activation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Governed => "governed",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActivationFacts {
    pub(crate) material_consequence: bool,
    pub(crate) promotion_required: bool,
    pub(crate) available: bool,
    pub(crate) activated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivationReason {
    NoMaterialConsequence,
    GovernanceAvailable,
    GovernanceUnavailable,
}

impl ActivationReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoMaterialConsequence => "no_material_consequence",
            Self::GovernanceAvailable => "governance_available",
            Self::GovernanceUnavailable => "governance_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActivationAssessment {
    pub(crate) activation: Activation,
    pub(crate) reason: ActivationReason,
    pub(crate) provenance: &'static str,
}

impl ActivationAssessment {
    pub(crate) fn value(self) -> Value {
        json!({
            "activation": self.activation.as_str(),
            "reason": self.reason.as_str(),
            "provenance": self.provenance,
        })
    }
}

pub(crate) fn assess_activation(facts: ActivationFacts) -> ActivationAssessment {
    let activation = classify_activation(
        facts.material_consequence,
        facts.promotion_required,
        facts.available,
        facts.activated,
    );
    let reason = match activation {
        Activation::Direct => ActivationReason::NoMaterialConsequence,
        Activation::Governed => ActivationReason::GovernanceAvailable,
        Activation::Blocked => ActivationReason::GovernanceUnavailable,
    };
    ActivationAssessment {
        activation,
        reason,
        provenance: "host_reported",
    }
}

#[allow(dead_code)]
pub(crate) fn classify_activation(
    material_consequence: bool,
    promotion_required: bool,
    available: bool,
    activated: bool,
) -> Activation {
    if !(material_consequence || promotion_required) {
        return Activation::Direct;
    }
    if available && activated {
        Activation::Governed
    } else {
        Activation::Blocked
    }
}

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
    if !EVENTS.contains(&event) {
        return Ok(());
    }
    if event == "SessionStart" {
        super::native_session::export(object, std::env::var_os("CLAUDE_ENV_FILE").as_deref());
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
            if let Some(text) = routing_text(event, update_context.as_deref(), Routing::Unresolved)
            {
                emit(event, &text)?;
            }
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
            if let Some(text) = routing_text(event, update_context.as_deref(), Routing::Unresolved)
            {
                emit(event, &text)?;
            }
            return Ok(());
        }
    };
    let config_path = if portable_present {
        let safe = fs::symlink_metadata(&portable)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            && contained(&project, &portable)
            && contained_existing(&project, &portable);
        if !safe {
            if let Some(text) = routing_text(event, update_context.as_deref(), Routing::Unresolved)
            {
                emit(event, &text)?;
            }
            return Ok(());
        }
        portable
    } else {
        match crate::project::layout_types::config_for_product(&project) {
            Ok(Some(path)) => path,
            Ok(None) => {
                if let Some(text) =
                    routing_text(event, update_context.as_deref(), Routing::CleanAbsence)
                {
                    emit(event, &text)?;
                }
                return Ok(());
            }
            Err(_) => {
                if let Some(text) =
                    routing_text(event, update_context.as_deref(), Routing::Unresolved)
                {
                    emit(event, &text)?;
                }
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
            if let Some(text) = routing_text(event, update_context.as_deref(), Routing::Unresolved)
            {
                emit(event, &text)?;
            }
            return Ok(());
        }
    };
    if fs::canonicalize(&loaded.product_root).ok().as_deref() != Some(project.as_path())
        || (loaded.mode == crate::project::layout_types::Mode::Portable
            && !contained_existing(&project, &loaded.control_root))
    {
        if let Some(text) = routing_text(event, update_context.as_deref(), Routing::Unresolved) {
            emit(event, &text)?;
        }
        return Ok(());
    }
    let text = if event == "SessionStart" {
        let mut text = session_summary(&loaded.config);
        if crate::producer::exitbind_surface() {
            text.push('\n');
            text.push_str(DIRECT_WORK);
            text.push('\n');
            text.push_str(RECEIVE_WORK);
        }
        if let Some(update) = update_context {
            text.push('\n');
            text.push_str(&update);
        }
        text
    } else {
        let Some(agent) = exact_agent(object, &loaded.config) else {
            return Ok(());
        };
        let configured = &loaded.config["agents"][&agent];
        let profile_requested = configured
            .get("profile")
            .and_then(Value::as_str)
            .unwrap_or("");
        let profile = match crate::config::file(&loaded.control_root, profile_requested) {
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
        let Some(context) =
            format_agent_context(&loaded.control_root, &agent, configured, &profile, &source)
        else {
            return Ok(());
        };
        bounded(context)
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

const RECEIVE_WORK: &str = "When the user gives an explicit smw_ work locator, first run `exitbind work continuation WORK`; its `receive` block gives the bind and native-child commands. Do not read raw .exitbind state to recover it.";

fn routing_text(event: &str, update: Option<&str>, routing: Routing) -> Option<String> {
    // The legacy Soulmate surface keeps its original silent contract; only the
    // Exitbind surface offers the bootstrap.
    if event != "SessionStart" || !crate::producer::exitbind_surface() {
        return update.map(str::to_owned);
    }
    let mut text = match routing {
        Routing::CleanAbsence => NATIVE_ABSENCE.to_owned(),
        Routing::Unresolved => UNRESOLVED.to_owned(),
    };
    if let Some(update) = update {
        text.push('\n');
        text.push_str(update);
    }
    Some(bounded(text))
}

fn emit(event: &str, text: &str) -> Result<(), String> {
    let output = serde_json::to_string(
        &json!({"hookSpecificOutput":{"hookEventName":event,"additionalContext":text}}),
    )
    .map_err(|_| String::new())?;
    if output.len() <= MAX_OUTPUT {
        println!("{output}");
    }
    Ok(())
}

fn exact_agent(payload: &serde_json::Map<String, Value>, config: &Value) -> Option<String> {
    let mut found = Vec::new();
    let agents = config["agents"].as_object()?;
    for key in ["agent_name", "agent_type", "subagent_type"] {
        let Some(candidate) = payload.get(key).and_then(Value::as_str) else {
            continue;
        };
        for (name, agent) in agents {
            if (candidate == name || candidate == crate::config::native_name(name, agent))
                && !found.contains(name)
            {
                found.push(name.clone());
            }
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}

fn format_agent_context(
    project: &Path,
    agent: &str,
    config: &Value,
    path: &Path,
    source: &str,
) -> Option<String> {
    let list = |name: &str| {
        config[name]
            .as_array()
            .map(|items| {
                if items.is_empty() {
                    "none".into()
                } else {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(safe_inline)
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            })
            .unwrap_or_else(|| "none".into())
    };
    let relative = path
        .strip_prefix(project)
        .ok()?
        .to_str()?
        .replace('\\', "/");
    let native = config["nativeName"]
        .as_str()
        .unwrap_or(agent)
        .to_ascii_lowercase()
        .replace('-', "_");
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    let mut lines = vec![
        format!("{product} plan-only context for {agent} (role selected by native host event)."),
        format!("Agent ID: {}", safe_inline(agent)),
        format!("Native task name: {}", safe_inline(&native)),
        format!("Profile selected/presented: {}", safe_inline(&relative)),
        format!("Profile SHA-256: {}", crate::evidence::hash::text(source)),
        "Evidence is selected/presented bytes; it does not prove a model read or followed the profile.".into(),
        "Declared boundary:".into(),
    ];
    for key in [
        "observe",
        "write",
        "commands",
        "skills",
        "memoryRead",
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryReject",
        "memoryRevoke",
        "memoryExpire",
        "memoryForget",
    ] {
        lines.push(format!("  {key}: {}", list(key)));
    }
    lines.push(format!(
        "  retention: {}",
        safe_inline(config["retention"].as_str().unwrap_or(""))
    ));
    lines.push(format!(
        "  crossContext: {}",
        safe_inline(config["crossContext"].as_str().unwrap_or(""))
    ));
    lines.push("Profile bytes:".into());
    lines.push(redact(source));
    Some(lines.join("\n"))
}

fn session_summary(config: &Value) -> String {
    let agents = config["agents"]
        .as_object()
        .map(|map| {
            let mut names = map.keys().cloned().collect::<Vec<_>>();
            names.sort();
            names.join(", ")
        })
        .unwrap_or_default();
    let workflows = config["workflows"]
        .as_object()
        .map(|map| {
            let mut names = map.keys().cloned().collect::<Vec<_>>();
            names.sort();
            names.join(", ")
        })
        .unwrap_or_default();
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    bounded(format!("{product} plan-only project context.\nLead: {}\nNamed agents: {}\nWorkflows: {}\nPreserve the existing root host conversation; {product} context only augments it. Do not replace, reset, fork, or request compaction of it for role loading or handoff.\nKeep recent user corrections and rejected approaches with their rationale; refer frozen-run conflicts to the existing lead for explicit supersession. Native conversational recall is distinct from durable role memory.\nWhen a work response carries a non-null `presentation.terminal`, print that value on a line of its own, exactly as given, with nothing else on that line.\nNo model was selected or launched; declarations are not an OS sandbox.", safe_inline(config["orchestration"]["lead"].as_str().unwrap_or("")), safe_inline(if agents.is_empty() { "none" } else { &agents }), safe_inline(if workflows.is_empty() { "none" } else { &workflows })))
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
fn redact(value: &str) -> String {
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
