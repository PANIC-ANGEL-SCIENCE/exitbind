//! Assemble a project configuration from owner-approved facts.
//!
//! `init` remains the compatibility-preserving, no-overwrite initializer. This
//! module adds a model-free route that previews the files and native mappings
//! implied by explicit facts, then applies only those facts with `--apply`.

use crate::{
    cli::args::{self, Arguments},
    config,
    host::settings,
    project::{layout_types::Mode, native_profiles, onboarding, skills as project_skills},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const ROLES: [&str; 3] = ["lead", "worker", "reviewer"];
const HOSTS: [&str; 2] = ["codex", "claude"];

#[derive(Debug, Clone)]
pub(crate) struct SetupFacts {
    root: String,
    mode: String,
    scope: Vec<String>,
    observe: Option<Vec<String>>,
    write: Option<Vec<String>>,
    commands: Option<Vec<String>>,
    check_command: Option<String>,
    review_policy: Option<String>,
    goal: Option<String>,
    hosts: Vec<String>,
    hosts_explicit: bool,
    project_id: Option<String>,
    control_root: Option<String>,
    state_root: Option<String>,
    skip_skills: bool,
    apply: bool,
    json: bool,
}

pub(crate) fn command(arguments: &Arguments) -> Result<(), String> {
    args::assert_options(
        "setup",
        arguments,
        &[
            "root",
            "mode",
            "scope",
            "observe",
            "write",
            "commands",
            "check-command",
            "review-policy",
            "goal",
            "hosts",
            "project-id",
            "control-root",
            "state-root",
            "apply",
            "skip-skills",
            "json",
        ],
    )?;
    args::assert_positionals("setup", arguments, 0)?;
    let facts = SetupFacts {
        root: arguments
            .options
            .get("root")
            .cloned()
            .unwrap_or_else(|| ".".into()),
        mode: arguments
            .options
            .get("mode")
            .cloned()
            .unwrap_or_else(|| "portable".into()),
        scope: split_list(arguments.options.get("scope"), "scope")?
            .unwrap_or_else(|| vec!["worker".into()]),
        observe: split_list(arguments.options.get("observe"), "observe")?,
        write: split_list(arguments.options.get("write"), "write")?,
        commands: single_command(arguments.options.get("commands"), "commands")?,
        check_command: arguments.options.get("check-command").cloned(),
        review_policy: arguments.options.get("review-policy").cloned(),
        goal: arguments.options.get("goal").cloned(),
        hosts: split_list(arguments.options.get("hosts"), "hosts")?.unwrap_or_default(),
        hosts_explicit: arguments.options.contains_key("hosts"),
        project_id: arguments.options.get("project-id").cloned(),
        control_root: arguments.options.get("control-root").cloned(),
        state_root: arguments.options.get("state-root").cloned(),
        skip_skills: arguments.flags.contains_key("skip-skills"),
        apply: arguments.flags.contains_key("apply"),
        json: arguments.flags.contains_key("json"),
    };
    let report = run(&facts)?;
    if facts.json {
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|error| error.to_string())?
        );
    } else {
        print_report(&report);
    }
    Ok(())
}

pub(crate) fn run(facts: &SetupFacts) -> Result<Value, String> {
    validate_facts(facts)?;
    let root = ordinary_directory(Path::new(&facts.root), "project root")?;
    root.to_str().ok_or("project root is not valid UTF-8")?;
    let mode = if facts.mode == "local" {
        Mode::Local
    } else {
        Mode::Portable
    };
    let control = match mode {
        Mode::Portable => root.clone(),
        Mode::Local => ordinary_directory(
            Path::new(facts.control_root.as_deref().unwrap_or("")),
            "ControlRoot",
        )?,
    };
    let state = match mode {
        Mode::Portable => root.clone(),
        Mode::Local => ordinary_directory(
            Path::new(facts.state_root.as_deref().unwrap_or("")),
            "StateRoot",
        )?,
    };
    control.to_str().ok_or("ControlRoot is not valid UTF-8")?;
    state.to_str().ok_or("StateRoot is not valid UTF-8")?;
    let config_path = control.join(crate::compatibility::profile().config);
    let hosts = selected_hosts(facts);
    let roles = &facts.scope;
    let existing = inspect_existing_config(&config_path)?;
    let mut report = preview_report(
        facts,
        &root,
        &control,
        &state,
        &config_path,
        roles,
        &hosts,
        existing.as_ref(),
    )?;
    if !facts.apply {
        return Ok(report);
    }

    if existing.is_none() && !hosts.is_empty() {
        preflight_new_projections(&root, &hosts)?;
    }
    let loaded = if let Some(loaded) = existing {
        loaded
    } else {
        let config = onboarding::init_with_options(onboarding::InitOptions {
            product_root: &facts.root,
            coffee: false,
            skip_skills: facts.skip_skills,
            mode: Some(&facts.mode),
            project_id: facts.project_id.as_deref(),
            control_root: facts.control_root.as_deref(),
            state_root: facts.state_root.as_deref(),
        })
        .map_err(|error| format!("setup partially applied: {error}"))?;
        let config = config
            .to_str()
            .ok_or("configuration path is not valid UTF-8")?;
        config::load(Some(config)).map_err(|error| format!("setup partially applied: {error}"))?
    };
    if loaded.mode != mode {
        return Err(format!(
            "setup mode conflicts with existing configuration (requested {}, found {})",
            facts.mode,
            mode_name(loaded.mode)
        ));
    }
    validate_destination(&loaded, &root, &control, &state, facts)?;
    let guidance = if facts.skip_skills {
        Vec::new()
    } else {
        guidance_diagnostics(&loaded.control_root)
    };
    for agent in loaded.agents.values() {
        config::file(&loaded.control_root, &agent.profile)
            .map_err(|error| format!("setup refused unsafe or missing profile: {error}"))?;
    }
    let profiles = profile_diagnostics(&loaded)?;
    ensure_guidance_compatible(&guidance)?;
    let projection_before = if hosts.is_empty() {
        None
    } else {
        let status = native_profiles::status_for_hosts(&loaded, &host_refs(&hosts))?;
        if has_projection_conflict(&status) {
            return Err(
                "native agent projection conflicts with an external or unsafe file; no files changed"
                    .into(),
            );
        }
        Some(status)
    };
    let (next, config_changed) = updated_config(&loaded.config, facts, roles, &hosts)?;
    if config_changed {
        let mut source = serde_json::to_string_pretty(&next).map_err(|error| error.to_string())?;
        source.push('\n');
        let root = fs::canonicalize(&loaded.control_root).map_err(|error| error.to_string())?;
        settings::atomic_write(&loaded.path, &source, None, Some(&loaded.source), &root)
            .map_err(|error| format!("setup partially applied: {error}"))?;
    }

    let loaded_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let loaded = config::load(Some(loaded_path))
        .map_err(|error| format!("setup partially applied: {error}"))?;
    let projection_changed = projection_before
        .as_ref()
        .is_some_and(|status| projection_needs_write(status));
    if !hosts.is_empty() {
        let projection = native_profiles::apply_for_hosts(&loaded, &host_refs(&hosts))
            .map_err(|error| format!("setup partially applied: {error}"))?;
        report["projection"] = projection;
    } else {
        report["projection"] = json!({"status": "skipped", "writes": false, "reason": "no supported host was selected or detected"});
    }
    report["status"] = json!(if config_changed || projection_changed {
        "applied"
    } else {
        "unchanged"
    });
    report["configChanged"] = json!(config_changed);
    report["profiles"] = json!(profiles);
    report["guidance"] = json!(if facts.skip_skills {
        Vec::<Value>::new()
    } else {
        guidance_diagnostics(&loaded.control_root)
    });
    report["next"] = next_actions(&facts.root, &loaded.path, &hosts, facts)?;
    Ok(report)
}

fn validate_facts(facts: &SetupFacts) -> Result<(), String> {
    if facts.root.trim().is_empty() || facts.root.contains('\0') {
        return Err("setup root must be a non-empty path without NUL bytes".into());
    }
    if !matches!(facts.mode.as_str(), "local" | "portable") {
        return Err("--mode must be local or portable".into());
    }
    if facts.mode == "local" {
        if facts.project_id.is_none() {
            return Err("local setup requires --project-id".into());
        }
        if facts.control_root.is_none() || facts.state_root.is_none() {
            return Err("local setup requires --control-root and --state-root".into());
        }
    } else if facts.project_id.is_some()
        || facts.control_root.is_some()
        || facts.state_root.is_some()
    {
        return Err(
            "portable setup does not accept --project-id, --control-root, or --state-root".into(),
        );
    }
    if facts.scope.is_empty() {
        return Err("--scope must name at least one role".into());
    }
    for role in &facts.scope {
        if !ROLES.contains(&role.as_str()) {
            return Err(format!(
                "unsupported setup scope '{role}'; use lead, worker, or reviewer"
            ));
        }
    }
    if facts.hosts_explicit {
        if facts.hosts.is_empty() {
            return Err("--hosts must name codex, claude, or both".into());
        }
        for host in &facts.hosts {
            if !HOSTS.contains(&host.as_str()) {
                return Err(format!("unsupported host '{host}'; use codex or claude"));
            }
        }
    }
    if let Some(policy) = facts.review_policy.as_deref() {
        if !matches!(policy, "required" | "omitted") {
            return Err("--review-policy must be required or omitted".into());
        }
    }
    if facts
        .goal
        .as_deref()
        .is_some_and(|goal| goal.trim().is_empty() || goal.contains('\0'))
    {
        return Err("--goal must be a non-empty value without NUL bytes".into());
    }
    if facts
        .check_command
        .as_deref()
        .is_some_and(|command| command.trim().is_empty() || command.contains('\0'))
    {
        return Err("--check-command must be a non-empty value without NUL bytes".into());
    }
    Ok(())
}

fn single_command(value: Option<&String>, label: &str) -> Result<Option<Vec<String>>, String> {
    let Some(value) = value else { return Ok(None) };
    if value.trim().is_empty() || value.contains('\0') {
        return Err(format!(
            "--{label} must be a non-empty value without NUL bytes"
        ));
    }
    Ok(Some(vec![value.clone()]))
}

fn split_list(value: Option<&String>, label: &str) -> Result<Option<Vec<String>>, String> {
    let Some(value) = value else { return Ok(None) };
    let mut result = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() || item.contains('\0') {
            return Err(format!("--{label} contains an empty or NUL value"));
        }
        result.push(item.to_owned());
    }
    let mut unique = BTreeSet::new();
    result.retain(|item| unique.insert(item.clone()));
    Ok(Some(result))
}

fn selected_hosts(facts: &SetupFacts) -> Vec<String> {
    if facts.hosts_explicit {
        return facts.hosts.clone();
    }
    HOSTS
        .iter()
        .filter(|host| find_on_path(host).is_some())
        .map(|host| (*host).to_owned())
        .collect()
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return fs::canonicalize(&candidate)
                .ok()
                .or_else(|| crate::project::path::absolute(&candidate).ok());
        }
    }
    None
}

fn host_refs(hosts: &[String]) -> Vec<&str> {
    hosts.iter().map(String::as_str).collect()
}

fn inspect_existing_config(path: &Path) -> Result<Option<config::Loaded>, String> {
    match fs::symlink_metadata(path) {
        Ok(info) if info.file_type().is_symlink() => Err(format!(
            "configuration path is unsafe (symlink): {}",
            path.display()
        )),
        Ok(info) if !info.is_file() => Err(format!(
            "configuration path is not a regular file: {}",
            path.display()
        )),
        Ok(_) => {
            let name = path
                .to_str()
                .ok_or("configuration path is not valid UTF-8")?;
            config::load(Some(name)).map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn preview_report(
    facts: &SetupFacts,
    root: &Path,
    control: &Path,
    state: &Path,
    config_path: &Path,
    roles: &[String],
    hosts: &[String],
    existing: Option<&config::Loaded>,
) -> Result<Value, String> {
    let mut affected = vec![config_path.to_path_buf()];
    if existing.is_none() {
        affected.extend([
            control.join(crate::project::layout_types::agents_dir()),
            state.join(crate::project::layout_types::state_namespace()),
        ]);
        if !facts.skip_skills {
            affected.push(control.join(".agents/skills"));
            affected.push(control.join(".claude/skills"));
        }
    }
    for host in hosts {
        affected.push(root.join(match host.as_str() {
            "codex" => ".codex/agents",
            "claude" => ".claude/agents",
            _ => "",
        }));
    }
    let mut conflicts = Vec::new();
    if let Some(existing) = existing {
        for (name, agent) in &existing.agents {
            if let Err(error) = config::file(&existing.control_root, &agent.profile) {
                conflicts.push(json!({"agent": name, "path": agent.profile, "reason": error}));
            }
        }
        if conflicts.is_empty() && !hosts.is_empty() {
            if let Ok(status) = native_profiles::status_for_hosts(existing, &host_refs(hosts)) {
                if let Some(items) = status["projections"].as_array() {
                    for item in items {
                        if matches!(item["state"].as_str(), Some("unsafe" | "external")) {
                            conflicts.push(json!({
                                "host": item["host"],
                                "path": item["path"],
                                "reason": "native projection is external or unsafe",
                            }));
                        }
                    }
                }
            }
        }
    }
    let root = root.to_str().ok_or("project root is not valid UTF-8")?;
    let affected = affected
        .iter()
        .map(|path| {
            path.to_str()
                .ok_or("affected setup path is not valid UTF-8")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let missing = [
        ("observe", facts.observe.is_none()),
        ("write", facts.write.is_none()),
        ("commands", facts.commands.is_none()),
        ("check-command", facts.check_command.is_none()),
        ("review-policy", facts.review_policy.is_none()),
        ("goal", facts.goal.is_none()),
    ]
    .into_iter()
    .filter_map(|(name, missing)| missing.then_some(name))
    .collect::<Vec<_>>();
    let host_mapping = hosts
        .iter()
        .map(|host| {
            let executable = find_on_path(host)
                .map(|path| {
                    path.to_str()
                        .map(str::to_owned)
                        .ok_or("selected host executable path is not valid UTF-8")
                })
                .transpose()?;
            let path_status = if executable.is_some() {
                "available"
            } else {
                "missing"
            };
            Ok(json!({
                "host": host,
                "executable": executable,
                "pathStatus": path_status,
                "supported": true,
                "activation": "not performed",
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let guidance = existing
        .map(|loaded| guidance_diagnostics(&loaded.control_root))
        .unwrap_or_default();
    let profiles = existing
        .and_then(|loaded| profile_diagnostics(loaded).ok())
        .unwrap_or_default();
    let projection = existing
        .filter(|_| !hosts.is_empty())
        .and_then(|loaded| native_profiles::status_for_hosts(loaded, &host_refs(hosts)).ok())
        .unwrap_or_else(|| json!({"status": "preview", "writes": false}));
    Ok(json!({
        "version": 1,
        "status": "preview",
        "applyRequired": true,
        "root": root,
        "mode": facts.mode,
        "scope": roles,
        "approvedFacts": {"observe": facts.observe, "write": facts.write, "commands": facts.commands, "checkCommand": facts.check_command, "reviewPolicy": facts.review_policy, "goal": facts.goal},
        "affectedPaths": affected,
        "conflicts": conflicts,
        "missingOwnerDecisions": missing,
        "hosts": host_mapping,
        "guidance": guidance,
        "profiles": profiles,
        "projection": projection,
        "next": next_actions(&facts.root, config_path, hosts, facts)?,
    }))
}

fn updated_config(
    source: &Value,
    facts: &SetupFacts,
    roles: &[String],
    hosts: &[String],
) -> Result<(Value, bool), String> {
    let mut next = source.clone();
    let mut changed = false;
    for role in roles {
        let agent = next["agents"]
            .get_mut(role)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("configuration has no '{role}' agent"))?;
        for (name, value) in [
            ("observe", facts.observe.as_ref()),
            ("write", facts.write.as_ref()),
            ("commands", facts.commands.as_ref()),
        ] {
            if let Some(value) = value {
                let value = json!(value);
                if agent.get(name) != Some(&value) {
                    agent.insert(name.to_owned(), value);
                    changed = true;
                }
            }
        }
        if hosts.len() == 1 {
            let runtime = agent.entry("runtime").or_insert_with(|| json!({}));
            let runtime = runtime
                .as_object_mut()
                .ok_or_else(|| format!("agents.{role}.runtime must be an object"))?;
            let host = json!(hosts[0]);
            if runtime.get("host") != Some(&host) {
                runtime.insert("host".into(), host);
                changed = true;
            }
        }
    }
    let errors = config::validate(&next);
    if !errors.is_empty() {
        return Err(format!(
            "approved setup facts would invalidate configuration:\n- {}",
            errors.join("\n- ")
        ));
    }
    Ok((next, changed))
}

fn validate_destination(
    loaded: &config::Loaded,
    root: &Path,
    control: &Path,
    state: &Path,
    facts: &SetupFacts,
) -> Result<(), String> {
    let expected_id = facts.project_id.as_deref();
    let same = loaded.product_root == root
        && loaded.control_root == control
        && loaded.state_root == state
        && loaded.project_id.as_deref() == expected_id;
    if same {
        return Ok(());
    }
    Err(format!(
        "setup destination conflicts with existing configuration: requested mode={} projectId={} productRoot={} controlRoot={} stateRoot={}, found mode={} projectId={} productRoot={} controlRoot={} stateRoot={}",
        facts.mode,
        expected_id.unwrap_or("none"),
        root.display(),
        control.display(),
        state.display(),
        mode_name(loaded.mode),
        loaded.project_id.as_deref().unwrap_or("none"),
        loaded.product_root.display(),
        loaded.control_root.display(),
        loaded.state_root.display(),
    ))
}

fn preflight_new_projections(root: &Path, hosts: &[String]) -> Result<(), String> {
    for host in hosts {
        let (directory, names): (&str, Vec<String>) = match host.as_str() {
            "codex" => (
                ".codex/agents",
                vec!["lead.toml", "worker.toml", "reviewer.toml"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            ),
            "claude" => (
                ".claude/agents",
                vec!["lead.md", "worker.md", "reviewer.md"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            ),
            _ => continue,
        };
        let directory = root.join(directory);
        ensure_projection_parent_safe(root, &directory)?;
        for name in names {
            let path = directory.join(name);
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
                Ok(info) if info.file_type().is_symlink() || !info.is_file() => {
                    return Err(format!(
                        "native agent projection conflict at {}; existing path is unsafe",
                        path.display()
                    ));
                }
                Ok(_) => {
                    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
                    let managed = if host == "codex" {
                        bytes.starts_with(b"# exitbind-managed-agent:v1\n")
                    } else {
                        bytes
                            .windows(b"<!-- exitbind-managed-agent:v1 -->".len())
                            .any(|window| window == b"<!-- exitbind-managed-agent:v1 -->")
                    };
                    if !managed {
                        return Err(format!(
                            "native agent projection conflict at {}; external file is preserved and setup stopped before init",
                            path.display()
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn ensure_projection_parent_safe(root: &Path, directory: &Path) -> Result<(), String> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "native projection path escaped project root")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(info) if info.file_type().is_symlink() || !info.is_dir() => {
                return Err(format!(
                    "native agent projection parent is unsafe: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn has_projection_conflict(value: &Value) -> bool {
    value["projections"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| matches!(item["state"].as_str(), Some("unsafe" | "external")))
    })
}

fn projection_needs_write(value: &Value) -> bool {
    value["projections"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| !matches!(item["state"].as_str(), Some("current")))
    })
}

fn guidance_diagnostics(control: &Path) -> Vec<Value> {
    project_skills::diagnose(control)
        .into_iter()
        .map(|item| {
            json!({
                "skill": item.skill,
                "path": item.path,
                "state": skill_state_label(item.state),
                "embeddedSha256": item.embedded_sha256,
                "observedSha256": item.observed_sha256,
                "customPreserved": matches!(item.state, project_skills::SkillObservationState::Unmanaged),
            })
        })
        .collect()
}

fn profile_diagnostics(loaded: &config::Loaded) -> Result<Vec<Value>, String> {
    loaded
        .agents
        .iter()
        .map(|(name, agent)| {
            let profile = onboarding::profile_diagnostic(&loaded.control_root, &agent.profile)?;
            Ok(json!({"agent": name, "profile": profile}))
        })
        .collect()
}

fn ensure_guidance_compatible(items: &[Value]) -> Result<(), String> {
    for item in items {
        let state = item["state"].as_str().unwrap_or("unknown");
        if matches!(
            state,
            "absent"
                | "managed different"
                | "unsafe path or nonregular"
                | "unreadable"
                | "unsupported inspection"
        ) {
            return Err(format!(
                "setup refused managed guidance state for {} ({}; embedded SHA256 {}; observed SHA256 {}). Preserve the file and recover it with the intended binary before applying setup",
                item["path"].as_str().unwrap_or("unknown"),
                state,
                item["embeddedSha256"].as_str().unwrap_or("unknown"),
                item["observedSha256"].as_str().unwrap_or("unknown")
            ));
        }
    }
    Ok(())
}

fn skill_state_label(state: project_skills::SkillObservationState) -> &'static str {
    match state {
        project_skills::SkillObservationState::Absent => "absent",
        project_skills::SkillObservationState::EmbeddedMatch => "embedded match",
        project_skills::SkillObservationState::ManagedDifferent => "managed different",
        project_skills::SkillObservationState::Unmanaged => "custom",
        project_skills::SkillObservationState::Unsafe => "unsafe path or nonregular",
        project_skills::SkillObservationState::Unreadable => "unreadable",
        project_skills::SkillObservationState::Unsupported => "unsupported inspection",
    }
}

fn next_actions(
    root: &str,
    config: &Path,
    hosts: &[String],
    facts: &SetupFacts,
) -> Result<Value, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("invoking executable cannot be resolved: {error}"))?;
    let executable = fs::canonicalize(executable)
        .map_err(|error| format!("invoking executable cannot be canonicalized: {error}"))?;
    let executable = executable
        .to_str()
        .ok_or("invoking executable path is not valid UTF-8")?;
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let cwd = fs::canonicalize(cwd).map_err(|error| error.to_string())?;
    let cwd = cwd.to_str().ok_or("working directory is not valid UTF-8")?;
    let config = config
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let check_argv = vec![executable, "check", "--config", config];
    let mut actions = vec![check_argv
        .iter()
        .map(|item| shell_quote(item))
        .collect::<Vec<_>>()
        .join(" ")];
    let mut argv = vec![check_argv];
    if let (Some(check), Some(review_policy), Some(goal)) =
        (&facts.check_command, &facts.review_policy, &facts.goal)
    {
        let work_argv = vec![
            executable,
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            check,
            "--review-policy",
            review_policy,
            "--config",
            config,
        ];
        actions.push(
            work_argv
                .iter()
                .map(|item| shell_quote(item))
                .collect::<Vec<_>>()
                .join(" "),
        );
        argv.push(work_argv);
    }
    Ok(
        json!({"root": root, "workingDirectory": cwd, "argv": argv, "commands": actions, "hosts": hosts, "sessionReload": "not performed"}),
    )
}

fn shell_quote(value: &str) -> String {
    crate::presentation::shell_quote(value)
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Local => "local",
        Mode::Portable => "portable",
    }
}

fn ordinary_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err(format!("{label} is required"));
    }
    let info = fs::symlink_metadata(path)
        .map_err(|error| format!("{label} does not exist: {} ({error})", path.display()))?;
    if info.file_type().is_symlink() || !info.is_dir() {
        return Err(format!("{label} must be a regular directory"));
    }
    fs::canonicalize(path).map_err(|error| error.to_string())
}

fn print_report(report: &Value) {
    println!("Setup {}.", report["status"].as_str().unwrap_or("unknown"));
    if let Some(paths) = report["affectedPaths"].as_array() {
        println!("Affected paths:");
        for path in paths {
            println!("  {}", path.as_str().unwrap_or("<invalid path>"));
        }
    }
    if let Some(missing) = report["missingOwnerDecisions"].as_array() {
        if !missing.is_empty() {
            println!(
                "Owner decisions still missing: {}",
                missing
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if let Some(next) = report["next"]["commands"].as_array() {
        println!("Next:");
        for command in next {
            println!("  {}", command.as_str().unwrap_or("<invalid command>"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn facts(root: String) -> SetupFacts {
        SetupFacts {
            root,
            mode: "portable".into(),
            scope: vec!["worker".into()],
            observe: Some(vec!["README.md".into()]),
            write: Some(vec!["src".into()]),
            commands: Some(vec!["cargo test --locked".into()]),
            check_command: Some("cargo test --locked".into()),
            review_policy: Some("required".into()),
            goal: Some("test setup".into()),
            hosts: Vec::new(),
            hosts_explicit: false,
            project_id: None,
            control_root: None,
            state_root: None,
            skip_skills: true,
            apply: false,
            json: false,
        }
    }

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "exitbind-setup-test-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn preview_reports_approved_facts_without_creating_files() {
        let root = temp_root();
        let report = run(&facts(root.to_str().unwrap().to_owned())).unwrap();
        assert_eq!(report["status"], "preview");
        assert_eq!(report["applyRequired"], true);
        assert!(root.join("exitbind.json").symlink_metadata().is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_host_is_rejected_before_any_path_inspection() {
        let root = temp_root();
        let mut facts = facts(root.to_str().unwrap().to_owned());
        facts.hosts = vec!["unknown".into()];
        facts.hosts_explicit = true;
        let error = run(&facts).unwrap_err();
        assert!(error.contains("unsupported host"));
        fs::remove_dir_all(root).unwrap();
    }
}
