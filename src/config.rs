use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use crate::project_path::{absolute, file, rel};

pub struct Loaded {
    pub config: Value,
    pub agents: BTreeMap<String, crate::config_types::AgentConfig>,
    pub path: PathBuf,
    pub control_root: PathBuf,
    pub product_root: PathBuf,
    pub state_root: PathBuf,
    pub mode: crate::project_layout::Mode,
    pub project_id: Option<String>,
    pub source: String,
}

impl Loaded {
    pub fn agent(&self, name: &str) -> Option<&crate::config_types::AgentConfig> {
        self.agents.get(name)
    }
}

const STRING_ARRAY_FIELDS: &[&str] = &[
    "observe",
    "write",
    "commands",
    "memoryRead",
    "memoryWrite",
    "memoryReview",
    "memoryPromote",
    "memoryReject",
    "memoryRevoke",
    "memoryExpire",
    "memoryForget",
    "skills",
];
const MEMORY_SCOPE_FIELDS: &[&str] = &[
    "memoryRead",
    "memoryWrite",
    "memoryReview",
    "memoryPromote",
    "memoryReject",
    "memoryRevoke",
    "memoryExpire",
    "memoryForget",
];
const RETENTION_VALUES: &[&str] = &["task", "until-reviewed", "until-revoked", "explicit-expiry"];
const CROSS_CONTEXT_VALUES: &[&str] = &["none", "same-scope", "protocol-only", "synthetic-only"];

pub fn load(path: Option<&str>) -> Result<Loaded, String> {
    let requested = path.unwrap_or_else(|| crate::compatibility::profile().config);
    if requested.trim().is_empty() {
        return Err("configuration path must be a non-empty string".into());
    }
    let requested_path = absolute(Path::new(requested))?;
    let path = fs::canonicalize(requested_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            // The lead meets this message when it selects Exitbind in a
            // repository that is not configured yet. Name the one owner-owned
            // write and its exact command instead of a bare instruction.
            let caller = crate::compatibility::profile().caller;
            format!(
                "configuration not found: {requested}. This project is not configured for {caller} yet. It needs one owner-approved project write: `{caller} init --mode portable --root .` creates {requested}, private ignored state, reviewable role profiles, and project-local guidance. It installs no hook, starts no agent, and grants no host permission. Ask the owner for that write, then run the command again."
            )
        } else {
            error.to_string()
        }
    })?;
    if !path.is_file() {
        return Err(format!("configuration is not a regular file: {requested}"));
    }
    let source = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let config: Value = serde_json::from_str(&source)
        .map_err(|error| format!("invalid JSON in {requested}: {error}"))?;
    let errors = validate(&config);
    if !errors.is_empty() {
        return Err(format!("invalid configuration:\n- {}", errors.join("\n- ")));
    }
    let agents = crate::config_types::from_validated_agents(&config["agents"])
        .map_err(|error| format!("internal typed configuration projection failed: {error}"))?;
    let layout = crate::project_layout::resolve(&path, &config)?;
    Ok(Loaded {
        config,
        agents,
        path,
        control_root: layout.control_root,
        product_root: layout.product_root,
        state_root: layout.state_root,
        mode: layout.mode,
        project_id: layout.project_id,
        source,
    })
}

pub fn validate(config: &Value) -> Vec<String> {
    let Some(root) = config.as_object() else {
        return vec!["root must be an object".into()];
    };
    let mut errors = Vec::new();
    reject_unknown(
        root,
        &[
            "$schema",
            "version",
            "project",
            "orchestration",
            "memory",
            "agents",
            "workflows",
        ],
        "configuration",
        &mut errors,
    );
    if config["version"] != 1 {
        errors.push("version must be 1".into());
    }
    validate_project(&config["project"], &mut errors);
    validate_orchestration(&config["orchestration"], &mut errors);
    crate::memory_policy::validate(&config["memory"], &mut errors);

    let agents = config["agents"].as_object();
    if agents.is_none() || agents.is_some_and(Map::is_empty) {
        errors.push("agents must contain at least one named agent".into());
    } else if let Some(agents) = agents {
        let mut native_names = BTreeMap::new();
        for (name, agent) in agents {
            validate_agent(name, agent, &mut errors);
            if agent.is_object() {
                let native = native_name(name, agent);
                if valid_native_name(&native) {
                    if let Some(previous) = native_names.insert(native, name.clone()) {
                        errors.push(format!(
                            "agents.{name}.nativeName collides with agent '{previous}'"
                        ));
                    }
                }
            }
        }
        if let Some(lead) = config["orchestration"]["lead"].as_str() {
            if !agents.contains_key(lead) {
                errors.push(format!(
                    "orchestration.lead references unknown agent '{lead}'"
                ));
            }
        }
    }

    match config["workflows"].as_object() {
        None => errors.push("workflows must contain at least one workflow".into()),
        Some(workflows) if workflows.is_empty() => {
            errors.push("workflows must contain at least one workflow".into())
        }
        Some(workflows) => {
            for (name, workflow) in workflows {
                validate_workflow(name, workflow, agents, &mut errors);
            }
        }
    }
    errors
}

fn validate_project(value: &Value, errors: &mut Vec<String>) {
    let Some(project) = value.as_object() else {
        errors.push("project.root must be a string".into());
        return;
    };
    reject_unknown(project, &["root", "mode", "id"], "project", errors);
    match project.get("root").and_then(Value::as_str) {
        None => errors.push("project.root must be a string".into()),
        Some(root) if root.trim().is_empty() => {
            errors.push("project.root must be a non-empty path".into())
        }
        Some(root) if root.contains('\0') => {
            errors.push("project.root must not contain NUL bytes".into())
        }
        _ => {}
    }
    if let Some(mode) = project.get("mode") {
        if !mode
            .as_str()
            .is_some_and(|value| matches!(value, "local" | "portable"))
        {
            errors.push("project.mode must be 'local' or 'portable'".into());
        }
    }
    if let Some(id) = project.get("id") {
        if !id.as_str().is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric())
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        }) {
            errors.push("project.id must be a portable identifier".into());
        }
    }
    match project.get("mode").and_then(Value::as_str) {
        Some("local") => {
            if project.get("id").is_none() {
                errors.push("local projects require project.id".into());
            }
            if project.get("root").and_then(Value::as_str) != Some(".") {
                errors.push("local project.root must be '.' relative to ControlRoot".into());
            }
        }
        Some("portable") if project.get("id").is_some() => {
            errors.push("portable projects must not declare project.id".into());
        }
        _ => {}
    }
}

fn validate_orchestration(value: &Value, errors: &mut Vec<String>) {
    let Some(orchestration) = value.as_object() else {
        errors.push("orchestration must be an object".into());
        return;
    };
    reject_unknown(
        orchestration,
        &["lead", "maxParallel"],
        "orchestration",
        errors,
    );
    if !is_name(orchestration.get("lead").and_then(Value::as_str)) {
        errors.push("orchestration.lead must be an agent name".into());
    }
    if orchestration
        .get("maxParallel")
        .and_then(Value::as_u64)
        .map_or(true, |value| value == 0)
    {
        errors.push("orchestration.maxParallel must be a positive integer".into());
    }
}

fn validate_agent(name: &str, value: &Value, errors: &mut Vec<String>) {
    if !is_name(Some(name)) {
        errors.push(format!("agent name '{name}' is not portable"));
    }
    let Some(agent) = value.as_object() else {
        errors.push(format!("agents.{name} must be an object"));
        return;
    };
    let mut allowed = vec![
        "profile",
        "purpose",
        "displayName",
        "nativeName",
        "retention",
        "crossContext",
        "runtime",
    ];
    allowed.extend_from_slice(STRING_ARRAY_FIELDS);
    reject_unknown(agent, &allowed, &format!("agents.{name}"), errors);

    if let Some(display) = agent.get("displayName") {
        let valid = display.as_str().is_some_and(|text| {
            !text.is_empty()
                && text.len() <= 80
                && text.trim() == text
                && !text.chars().any(char::is_control)
        });
        if !valid {
            errors.push(format!(
                "agents.{name}.displayName must be a non-empty string of at most 80 characters"
            ));
        }
    }
    if !valid_native_name(&native_name(name, value)) {
        errors.push(format!(
            "agents.{name}.nativeName must match /^[a-z0-9][a-z0-9_]{{0,63}}$/"
        ));
    }
    if value["purpose"]
        .as_str()
        .map_or(true, |purpose| purpose.trim().is_empty())
    {
        errors.push(format!("agents.{name}.purpose must be a non-empty string"));
    }
    match value["profile"].as_str() {
        None => errors.push(format!(
            "agents.{name}.profile must be a non-empty relative path"
        )),
        Some(profile) if profile.trim().is_empty() => errors.push(format!(
            "agents.{name}.profile must be a non-empty relative path"
        )),
        Some(profile) if Path::new(profile).is_absolute() || profile.contains('\0') => errors.push(
            format!("agents.{name}.profile must stay inside the project"),
        ),
        _ => {}
    }
    for field in STRING_ARRAY_FIELDS {
        if agent
            .get(*field)
            .is_some_and(|field_value| !is_string_array(field_value))
        {
            errors.push(format!("agents.{name}.{field} must be an array of strings"));
        }
    }
    for field in MEMORY_SCOPE_FIELDS {
        if agent
            .get(*field)
            .is_some_and(|value| !crate::memory_policy::valid_scope_array(value))
        {
            errors.push(format!(
                "agents.{name}.{field} must contain unique, non-empty exact scopes"
            ));
        }
    }
    if !value["retention"]
        .as_str()
        .is_some_and(|entry| RETENTION_VALUES.contains(&entry))
    {
        errors.push(format!(
            "agents.{name}.retention must be one of: {}",
            RETENTION_VALUES.join(", ")
        ));
    }
    if !value["crossContext"]
        .as_str()
        .is_some_and(|entry| CROSS_CONTEXT_VALUES.contains(&entry))
    {
        errors.push(format!(
            "agents.{name}.crossContext must be one of: {}",
            CROSS_CONTEXT_VALUES.join(", ")
        ));
    }
    if let Some(runtime) = agent.get("runtime") {
        validate_runtime(name, runtime, errors);
    }
}

fn validate_runtime(name: &str, value: &Value, errors: &mut Vec<String>) {
    let Some(runtime) = value.as_object() else {
        errors.push(format!("agents.{name}.runtime must be an object"));
        return;
    };
    let fields = ["host", "model", "reasoningEffort", "fallback"];
    reject_unknown(runtime, &fields, &format!("agents.{name}.runtime"), errors);
    for field in BINDING_FIELDS {
        if runtime
            .get(*field)
            .is_some_and(|value| value.as_str().map_or(true, |entry| entry.trim().is_empty()))
        {
            errors.push(format!(
                "agents.{name}.runtime.{field} must be a non-empty string"
            ));
        }
    }
    // A fallback is an alternate execution binding for this same agent: it can
    // move where a role runs, never which contract it runs. Naming another
    // agent is refused outright, so no configuration can substitute a second
    // profile, purpose, or boundary for the one the plan selected.
    match runtime.get("fallback") {
        None => {}
        Some(Value::String(declined)) if declined == "none" => {}
        Some(Value::Object(binding)) => validate_fallback_binding(name, binding, errors),
        Some(_) => errors.push(format!(
            "agents.{name}.runtime.fallback must be \"none\" or an alternate runtime binding of host, model, and reasoningEffort"
        )),
    }
}

const BINDING_FIELDS: &[&str] = &["host", "model", "reasoningEffort"];

fn validate_fallback_binding(name: &str, binding: &Map<String, Value>, errors: &mut Vec<String>) {
    reject_unknown(
        binding,
        BINDING_FIELDS,
        &format!("agents.{name}.runtime.fallback"),
        errors,
    );
    for field in BINDING_FIELDS {
        if binding
            .get(*field)
            .is_some_and(|value| value.as_str().map_or(true, |entry| entry.trim().is_empty()))
        {
            errors.push(format!(
                "agents.{name}.runtime.fallback.{field} must be a non-empty string"
            ));
        }
    }
    if !BINDING_FIELDS
        .iter()
        .any(|field| binding.get(*field).and_then(Value::as_str).is_some())
    {
        errors.push(format!(
            "agents.{name}.runtime.fallback must bind at least one of host, model, or reasoningEffort"
        ));
    }
}

fn validate_workflow(
    name: &str,
    value: &Value,
    agents: Option<&Map<String, Value>>,
    errors: &mut Vec<String>,
) {
    if !is_name(Some(name)) {
        errors.push(format!("workflow name '{name}' is not portable"));
    }
    let Some(workflow) = value.as_object() else {
        errors.push(format!("workflows.{name} must be an object"));
        return;
    };
    reject_unknown(
        workflow,
        &["advisers", "workers", "reviewers"],
        &format!("workflows.{name}"),
        errors,
    );
    for field in ["advisers", "workers", "reviewers"] {
        let Some(values) = workflow.get(field) else {
            continue;
        };
        if !is_string_array(values) {
            errors.push(format!(
                "workflows.{name}.{field} must be an array of agent names"
            ));
            continue;
        }
        for agent_name in values.as_array().into_iter().flatten() {
            let agent_name = agent_name.as_str().unwrap_or_default();
            if agents.map_or(true, |known| !known.contains_key(agent_name)) {
                errors.push(format!(
                    "workflows.{name}.{field} references unknown agent '{agent_name}'"
                ));
            }
        }
    }
}

fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    prefix: &str,
    errors: &mut Vec<String>,
) {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    for key in object.keys() {
        if !allowed.contains(key.as_str()) {
            errors.push(format!("{prefix}.{key} is not allowed"));
        }
    }
}

fn is_string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
}

pub fn is_name(value: Option<&str>) -> bool {
    value.is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric())
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
    })
}

fn valid_native_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

pub fn native_name(name: &str, agent: &Value) -> String {
    agent["nativeName"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| name.to_lowercase().replace('-', "_"))
}
