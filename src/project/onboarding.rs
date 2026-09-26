use crate::project::skills as project_skills;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const PROFILES: [(&str, &str); 3] = [
    ("lead", "# Lead\n\nOwn the accepted goal, scope changes, verification, and final result.\n\nA reviewer finding is evidence, not a requirement, and a suggested fix is a proposal, not a decision. Classify each finding before any rework: repair it inside the task with your own repair boundary, defer it as valid but out of scope, reject it, or supersede the basis when the basis itself is wrong. For a clear defect inside the accepted task, record repair and proceed without asking the owner again; ask only if scope, authority, review choice, or an irreversible decision changes. Keep non-goals. When findings repeat, reconsider the basis, acceptance criteria, evidence, or reviewer scope before authorizing more work.\n"),
    ("worker", "# Worker\n\nImplement one bounded task and return changed files plus verification evidence.\n\nMake the smallest faithful change that satisfies the task and any Lead repair boundary. Do not treat reviewer speculation as a new commitment. If the repair boundary conflicts with the basis or is insufficient, return a contradiction or blocker instead of widening scope.\n"),
    ("reviewer", "# Reviewer\n\nReview the checked-out artifact and returned work, not their summary. Every finding needs a file:line reference, measured number, or short quote. Check guards, documented limitations, and deliberate decisions before reporting. For a whole-repository review, run the repository's mechanical inventory first and report adoption, long-term ownership, and agent-consumer findings separately. When a finding recurs, recommend the mechanical gate that would have caught it. Return role-scoped evidence; do not silently expand scope or claim the lead's final authority. A finding can be valid without being in the current scope; say which, and present any suggested implementation as a proposal, not an instruction.\n"),
];
const SOULMATE_SCHEMA: &str = concat!(
    "https://raw.githubusercontent.com/envy404/soulmate/v",
    env!("CARGO_PKG_VERSION"),
    "/schema/soulmate.schema.json"
);
const EXITBIND_SCHEMA: &str = concat!(
    "https://raw.githubusercontent.com/envy404/exitbind/v",
    env!("CARGO_PKG_VERSION"),
    "/schema/exitbind.schema.json"
);

pub struct InitOptions<'a> {
    pub product_root: &'a str,
    pub coffee: bool,
    pub skip_skills: bool,
    pub mode: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub control_root: Option<&'a str>,
    pub state_root: Option<&'a str>,
}

pub fn init_with_options(options: InitOptions<'_>) -> Result<PathBuf, String> {
    let InitOptions {
        product_root,
        coffee,
        skip_skills,
        mode,
        project_id,
        control_root,
        state_root,
    } = options;
    let requested = absolute(Path::new(product_root))?;
    let product = ordinary_directory(&requested, "project path")?;
    let in_worktree = match crate::project::git_preflight::worktree_root(&product) {
        Ok(root) => root.is_some(),
        Err(error) if crate::project::git_preflight::has_git_marker(&product) => return Err(error),
        Err(_) => false,
    };
    let selected_mode = match mode {
        Some("local") => "local",
        Some("portable") => "portable",
        Some(_) => return Err("--mode must be local or portable".into()),
        None if in_worktree || crate::project::git_preflight::has_git_marker(&product) => {
            return Err("init in a Git worktree requires explicit --mode local or portable".into())
        }
        None => "portable",
    };
    let control = if selected_mode == "local" {
        ordinary_directory(
            &absolute(Path::new(
                control_root.ok_or("local init requires --control-root")?,
            ))?,
            "ControlRoot",
        )?
    } else {
        product.clone()
    };
    let state = if selected_mode == "local" {
        ordinary_directory(
            &absolute(Path::new(
                state_root.ok_or("local init requires --state-root")?,
            ))?,
            "StateRoot",
        )?
    } else {
        product.clone()
    };
    if selected_mode == "local" {
        let id = project_id.ok_or("local init requires --project-id")?;
        crate::project::layout_types::validate_id(id)?;
        crate::project::layout_types::ensure_binding_available(id)?;
        crate::project::git_preflight::reject_roots_under_worktree(&product, &control, &state)?;
    }
    let config = control.join(config_name());
    if fs::symlink_metadata(&config).is_ok() {
        return Err(format!(
            "{} already exists; init never overwrites it",
            config_name()
        ));
    }
    let agents_dir = control.join(crate::project::layout_types::agents_dir());
    let state_namespace = crate::project::layout_types::state_namespace();
    let control_skill = if crate::project::layout_types::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    let mut target_paths = vec![
        config.clone(),
        state.join(format!("{state_namespace}/.gitignore")),
        agents_dir.join("lead.md"),
        agents_dir.join("worker.md"),
        agents_dir.join("reviewer.md"),
    ];
    if !skip_skills {
        target_paths.extend([
            control.join(format!(".agents/skills/{control_skill}/SKILL.md")),
            control.join(format!(".claude/skills/{control_skill}/SKILL.md")),
        ]);
    }
    target_paths.extend(
        crate::project::layout_types::control_dirs().map(|relative| control.join(relative)),
    );
    target_paths
        .extend(crate::project::layout_types::state_dirs().map(|relative| state.join(relative)));
    if coffee && !skip_skills {
        target_paths.push(control.join(".agents/skills/coffee/SKILL.md"));
        target_paths.push(control.join(".claude/skills/coffee/SKILL.md"));
    }
    let control_targets = target_paths
        .iter()
        .filter(|path| path.starts_with(&control))
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    let state_targets = target_paths
        .iter()
        .filter(|path| path.starts_with(&state))
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    crate::project::git_preflight::refuse_tracked_targets(&control, &control_targets)?;
    crate::project::git_preflight::refuse_tracked_targets(&state, &state_targets)?;
    if !skip_skills {
        project_skills::activate(
            &control,
            coffee && !crate::project::layout_types::exitbind_surface(),
        )?;
    }
    crate::project::managed_files::ensure_state_directory(&state, &state.join(state_namespace))?;
    let state_dir = state.join(state_namespace);
    preserve_or_create(
        &state_dir.join(".gitignore"),
        "*\n!.gitignore\n",
        &format!("{state_namespace}/.gitignore"),
        &state,
        true,
    )?;
    for relative in crate::project::layout_types::state_dirs() {
        crate::project::managed_files::ensure_state_directory(&state, &state.join(relative))?;
    }
    for relative in crate::project::layout_types::control_dirs() {
        crate::project::managed_files::ensure_managed_directory(&control, &control.join(relative))?;
    }
    for (name, content) in PROFILES {
        let relative = format!("{}/{name}.md", crate::project::layout_types::agents_dir());
        preserve_or_create(
            &control.join(&relative),
            content,
            &relative,
            &control,
            false,
        )?;
    }
    if selected_mode == "local" {
        crate::project::layout_types::create_binding(
            project_id.ok_or("local init requires --project-id")?,
            &control,
            &product,
            &state,
        )?;
    }
    let mut config_source =
        serde_json::to_string_pretty(&default_config(selected_mode, project_id))
            .map_err(|error| error.to_string())?;
    config_source.push('\n');
    crate::project::managed_files::write_exclusive(&config, config_source.as_bytes())?;
    Ok(config)
}

pub fn refresh(
    root: &str,
    coffee: bool,
) -> Result<Vec<project_skills::SkillRefreshStatus>, String> {
    let project = ordinary_directory(&absolute(Path::new(root))?, "project path")?;
    let config_path = project.join(config_name());
    let config_name = config_path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let loaded = crate::config::load(Some(config_name))?;
    if loaded.mode == crate::project::layout_types::Mode::Portable
        && fs::canonicalize(&loaded.product_root).map_err(|error| error.to_string())? != project
    {
        return Err(format!(
            "{} project root does not match --root",
            config_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("configuration")
        ));
    }
    project_skills::refresh(&loaded.control_root, coffee)
}

pub fn doctor(path: Option<&str>) -> Vec<Value> {
    let mut checks = Vec::new();
    let mut project_dotagents_config = false;
    let binary = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "soulmate".into());
    checks.push(json!({"name":"binary","ok":true,"detail":binary}));
    match crate::config::load(path) {
        Ok(loaded) => {
            project_dotagents_config = loaded.control_root.join(".agents/agents.toml").is_file();
            checks.push(
                json!({"name":"config","ok":true,"detail":loaded.path.display().to_string()}),
            );
            if let Some(agents) = loaded.config["agents"].as_object() {
                for (name, agent) in agents {
                    let profile = agent["profile"].as_str().unwrap_or("");
                    let ok = crate::config::file(&loaded.control_root, profile).is_ok();
                    checks.push(json!({"name":format!("profile:{name}"),"ok":ok,"detail":profile}));
                }
            }
            for warning in crate::config::boundary::warnings(&loaded.config) {
                checks.push(json!({
                    "name": format!("boundary:{}:{}", warning["agent"].as_str().unwrap_or("unknown"), warning["field"].as_str().unwrap_or("unknown")),
                    "ok": false,
                    "detail": warning["detail"]
                }));
            }
        }
        Err(error) => checks.push(json!({"name":"config","ok":false,"detail":error})),
    }
    for name in ["codex", "claude"] {
        checks
            .push(json!({"name":name,"ok":command_exists(name),"detail":"optional host command"}));
    }
    checks.push(dotagents_check(project_dotagents_config));
    checks
}

fn default_config(mode: &str, project_id: Option<&str>) -> Value {
    let mut project = json!({"root":"."});
    if mode == "local" {
        project["mode"] = json!("local");
        project["id"] = json!(project_id.unwrap_or_default());
    }
    json!({"$schema":if config_name() == "exitbind.json" { EXITBIND_SCHEMA } else { SOULMATE_SCHEMA },"version":1,"project":project,"orchestration":{"lead":"lead","maxParallel":2},"agents":{"lead":agent("lead","Own the goal, scope changes, verification, and final result."),"worker":agent("worker","Implement one bounded task without redefining architecture."),"reviewer":agent("reviewer","Review evidence and return findings to the lead.")},"workflows":{"change":{"advisers":[],"workers":["worker"],"reviewers":["reviewer"]}}})
}
fn agent(name: &str, purpose: &str) -> Value {
    json!({"profile":format!("{}/{name}.md", crate::project::layout_types::agents_dir()),"purpose":purpose,"observe":[],"write":[],"commands":[],"skills":[],"memoryRead":[],"memoryWrite":[],"memoryForget":[],"retention":"task","crossContext":"none"})
}

fn preserve_or_create(
    path: &Path,
    content: &str,
    label: &str,
    root: &Path,
    private_state: bool,
) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(info) => {
            if info.file_type().is_symlink() {
                return Err(format!("managed file must not be a symlink: {label}"));
            }
            if !info.is_file() {
                return Err(format!("managed path must be a regular file: {label}"));
            }
            if label.ends_with("/.gitignore") {
                let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
                let patterns = source
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .collect::<Vec<_>>();
                if !patterns.contains(&"*")
                    || !patterns.contains(&"!.gitignore")
                    || patterns
                        .iter()
                        .any(|p| p.starts_with('!') && *p != "!.gitignore")
                {
                    return Err(format!(
                        "{}/.gitignore must protect '*' and may unignore only '.gitignore'; refusing unsafe existing content",
                        crate::project::layout_types::state_namespace()
                    ));
                }
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or("managed path has no parent")?;
            if private_state {
                crate::project::managed_files::ensure_state_directory(root, parent)?;
                crate::project::managed_files::write_state_exclusive(path, content.as_bytes())
            } else {
                crate::project::managed_files::ensure_managed_directory(root, parent)?;
                crate::project::managed_files::write_exclusive(path, content.as_bytes())
            }
        }
        Err(error) => Err(error.to_string()),
    }
}
fn ordinary_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let info = fs::symlink_metadata(path)
        .map_err(|_| format!("{label} does not exist: {}", path.display()))?;
    if !info.is_dir() {
        return Err(format!("{label} must be a directory"));
    }
    fs::canonicalize(path).map_err(|e| e.to_string())
}
fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}
fn dotagents_check(project_config: bool) -> Value {
    if command_exists("dotagents") {
        return json!({"name":"dotagents","ok":true,"detail":"optional distribution command on PATH; host-specific caches and active-session reload were not inspected"});
    }
    let npx = command_exists("npx");
    let global_config = std::env::var_os("HOME")
        .map(PathBuf::from)
        .is_some_and(|home| home.join(".agents/agents.toml").is_file());
    let config = project_config || global_config;
    let detail = match (npx, config) {
        (true, true) => {
            "command absent; npx launcher and agents.toml observed; package not invoked"
        }
        (true, false) => "command absent; npx launcher observed; package not verified",
        (false, true) => "command absent; agents.toml observed; launcher not verified",
        (false, false) => "optional distribution command absent",
    };
    json!({"name":"dotagents","ok":false,"detail":format!("{detail}; host-specific caches and active-session reload were not inspected")})
}
fn config_name() -> &'static str {
    crate::compatibility::profile().config
}
fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| format!("current directory cannot be resolved: {error}"))
    }
}
