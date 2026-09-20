//! CLI commands that initialize, bind, and diagnose a project layout.

use crate::{
    cli::args,
    cli::args::Arguments,
    config,
    project::{onboarding, skills as project_skills},
};
use serde_json::json;
use std::path::Path;

const EMPTY_STARTER_DETAIL: &str = "empty starter boundary: review observe, write, and commands before tasks needing project files or commands; empty declarations do not grant host permission or establish task readiness";

pub(crate) fn init(arguments: &Arguments) -> Result<(), String> {
    args::assert_options(
        "init",
        arguments,
        &[
            "root",
            "with-coffee",
            "skip-skills",
            "refresh-skills",
            "mode",
            "project-id",
            "control-root",
            "state-root",
        ],
    )?;
    args::assert_positionals("init", arguments, 0)?;
    let skip_skills = arguments.flags.contains_key("skip-skills");
    let with_coffee = arguments.flags.contains_key("with-coffee");
    let refresh_skills = arguments.flags.contains_key("refresh-skills");
    if skip_skills && (with_coffee || refresh_skills) {
        return Err(
            "--skip-skills cannot be combined with --with-coffee or --refresh-skills".into(),
        );
    }
    if crate::project::layout_types::exitbind_surface() && with_coffee {
        return Err("--with-coffee is retired; Exitbind readiness guidance is built into the Exitbind skill".into());
    }
    let root = arguments
        .options
        .get("root")
        .map(String::as_str)
        .unwrap_or(".");
    if refresh_skills {
        let statuses = onboarding::refresh(root, with_coffee)?;
        let product = if crate::project::layout_types::exitbind_surface() {
            "Exitbind"
        } else {
            "Soulmate"
        };
        println!(
            "Refreshed project skills with {product} {}:\n{}",
            project_skills::package_version(),
            statuses
                .iter()
                .map(|status| {
                    let state = match status.state {
                        project_skills::SkillRefreshState::Created => "created",
                        project_skills::SkillRefreshState::Refreshed => "refreshed",
                        project_skills::SkillRefreshState::Unchanged => "unchanged",
                    };
                    format!(
                        "  {state} {} ({} embedded SHA256 {})",
                        status.path, status.skill, status.embedded_sha256
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        );
        return Ok(());
    }
    let path = onboarding::init_with_options(onboarding::InitOptions {
        product_root: root,
        coffee: with_coffee,
        skip_skills,
        mode: arguments.options.get("mode").map(String::as_str),
        project_id: arguments.options.get("project-id").map(String::as_str),
        control_root: arguments.options.get("control-root").map(String::as_str),
        state_root: arguments.options.get("state-root").map(String::as_str),
    })?;
    let created = crate::config::load(Some(
        path.to_str()
            .ok_or("configuration path is not valid UTF-8")?,
    ))?;
    let empty_starter = is_empty_starter(&created.agents);
    let coffee = if with_coffee { " + opt-in Coffee" } else { "" };
    let quoted_config = crate::presentation::shell_quote(
        path.to_str()
            .ok_or("configuration path is not valid UTF-8")?,
    );
    let skill_name = if crate::project::layout_types::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    if skip_skills {
        println!(
            "Created {}\nProject skill projection skipped by explicit --skip-skills.\n\nBounded setup facts for your existing root agent:\n  Configuration: {quoted_config}\n  Review the declared task boundary and the host's native worker/reviewer mapping before project-scoped work.\n  Setup did not inspect, install, or change project skill destinations, host software, or host permissions; resolve skill ownership before running init --refresh-skills --root ROOT to request project skill projection.\n  Setup does not start agents or grant host permissions.\n\nIllustrative request in this conversation:\n  Inspect the configuration above, review my task and actual check command, then decide whether the governed path is appropriate. Keep ordinary reversible work direct and report what still needs doing before acceptance.\n\nCLI reference (replace YOUR_TEST_COMMAND with a real project check command):\n  {skill_name} brief worker --task \"Describe the change you want to make\" --config={quoted_config}\n  {skill_name} work begin change --goal \"Describe the bounded change\" --check-command \"YOUR_TEST_COMMAND\" --config={quoted_config}\n  {skill_name} check --config={quoted_config}\nThe host executes the frozen check command and reports its actual result. '{skill_name} check' validates configuration, profiles, and declared boundaries; it does not run project tests.",
            path.display()
        );
        println!(
            "Installation: no host software installation performed; {skill_name} is available in this invoking binary.\nProject skill projection: skipped by explicit --skip-skills; no project skill destination was inspected or changed.\nFresh-session discovery: unverified (setup did not probe the host).\nActive-session discovery: unverified (setup does not inspect or refresh an already-loaded session).\nSelection: not performed by setup; the lead decides task by task.\nActivation: not performed by setup; only a successful {skill_name} work begin result creates machine-confirmed governed state."
        );
    } else {
        let skill = path
            .parent()
            .ok_or("configuration path has no parent")?
            .join(format!(".agents/skills/{skill_name}/SKILL.md"));
        let quoted_skill = crate::presentation::shell_quote(
            skill.to_str().ok_or("skill path is not valid UTF-8")?,
        );
        println!(
            "Created {}\nPrepared project skills for Codex and Claude: {skill_name}{coffee}.\n\nBounded setup facts for your existing root agent:\n  Configuration: {quoted_config}\n  {skill_name} skill: {quoted_skill}\n  Review the declared task boundary and the host's native worker/reviewer mapping before project-scoped work.\n  Setup did not install host software or change host permissions; ask before any future installation or permission change.\n  Setup does not start agents or grant host permissions.\n\nIllustrative request in this conversation:\n  Inspect the configuration and skill above, review my task and actual check command, then decide whether the governed path is appropriate. Keep ordinary reversible work direct and report what still needs doing before acceptance.\n\nCLI reference (replace YOUR_TEST_COMMAND with a real project check command):\n  {skill_name} brief worker --task \"Describe the change you want to make\" --config={quoted_config}\n  {skill_name} work begin change --goal \"Describe the bounded change\" --check-command \"YOUR_TEST_COMMAND\" --config={quoted_config}\n  {skill_name} check --config={quoted_config}\nThe host executes the frozen check command and reports its actual result. '{skill_name} check' validates configuration, profiles, and declared boundaries; it does not run project tests.",
            path.display()
        );
        println!(
            "Installation: no host software installation performed; {skill_name} is available in this invoking binary.\nProject-scoped selective preference: confirmed in the projected {skill_name} skill. At the start of each material task, classify once; for a governed trigger that materially matters, select {skill_name} automatically and run high-level {skill_name} work begin before any scoped implementation or mutation, proceeding only after it succeeds and returns a work handle and next action. Tiny, obvious, reversible work stays direct without activation.\nProjection/materialization: confirmed for Codex and Claude project skill paths (exact embedded bytes).\nFresh-session discovery: unverified (no host probe was performed).\nActive-session discovery: unverified (setup does not inspect or refresh an already-loaded session).\nSelection: not performed by setup; the lead decides task by task.\nActivation: not performed by setup; only a successful {skill_name} work begin result creates machine-confirmed governed state.\nProjected bytes and setup text do not prove session discovery, future compliance, selection, or activation."
        );
    }
    if empty_starter {
        println!("warning: {EMPTY_STARTER_DETAIL}");
    }
    Ok(())
}

pub(crate) fn bind(arguments: &Arguments) -> Result<(), String> {
    args::assert_options("bind", arguments, &["config", "root", "state-root"])?;
    args::assert_positionals("bind", arguments, 0)?;
    let config_path = required(arguments, "config", "bind requires --config CONFIG")?;
    let product = required(arguments, "root", "bind requires --root PRODUCT")?;
    let state = required(arguments, "state-root", "bind requires --state-root STATE")?;
    let path = std::path::PathBuf::from(config_path);
    let source = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&source)
        .map_err(|error| format!("invalid JSON in {config_path}: {error}"))?;
    let errors = config::validate(&config);
    if !errors.is_empty() {
        return Err(format!("invalid configuration:\n- {}", errors.join("\n- ")));
    }
    let binding = crate::project::layout_types::bind_from_config(
        &std::fs::canonicalize(path).map_err(|error| error.to_string())?,
        &config,
        std::path::Path::new(product),
        std::path::Path::new(state),
    )?;
    println!("Local project binding is current: {}", binding.display());
    Ok(())
}

pub(crate) fn doctor(arguments: &Arguments) -> Result<(), String> {
    args::assert_options("doctor", arguments, &["config"])?;
    args::assert_positionals("doctor", arguments, 0)?;
    let checks = onboarding::doctor(arguments.options.get("config").map(String::as_str));
    let required_failed = checks.iter().any(|item| {
        !item["ok"].as_bool().unwrap_or(false)
            && matches!(item["name"].as_str(), Some("binary" | "config"))
    });
    for item in checks {
        println!(
            "{} {}: {}",
            if item["ok"].as_bool().unwrap_or(false) {
                "ok"
            } else {
                "--"
            },
            item["name"],
            item["detail"]
        );
    }
    if required_failed {
        Err("doctor required checks failed".into())
    } else {
        Ok(())
    }
}

pub(crate) fn check(loaded: &config::Loaded, arguments: &Arguments) -> Result<(), String> {
    args::assert_options("check", arguments, &["config", "json"])?;
    args::assert_positionals("check", arguments, 0)?;
    for agent in loaded.agents.values() {
        config::file(&loaded.control_root, &agent.profile)?;
    }
    let mut warnings = crate::config::boundary::warnings(&loaded.config);
    if is_empty_starter(&loaded.agents) {
        warnings.push(json!({
            "classification": "empty_starter_boundary",
            "detail": EMPTY_STARTER_DETAIL
        }));
    }
    let skill_diagnostics = project_skills::diagnose(&loaded.control_root);
    let mode = match loaded.mode {
        crate::project::layout_types::Mode::Local => "local",
        crate::project::layout_types::Mode::Portable => "portable",
    };
    if arguments.flags.contains_key("json") {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "valid": true,
                "mode": mode,
                "projectId": loaded.project_id,
                "warnings": warnings
            }))
            .map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "{} configuration is valid ({mode} mode).",
            if crate::project::layout_types::exitbind_surface() {
                "Exitbind"
            } else {
                "Soulmate"
            }
        );
        for warning in warnings {
            if warning["classification"] == "empty_starter_boundary" {
                println!(
                    "warning: {}",
                    warning["detail"]
                        .as_str()
                        .unwrap_or("unknown warning detail")
                );
            } else {
                println!(
                    "warning: agents.{}.{} entry '{}' is descriptive or unsupported for exact run narrowing",
                    warning["agent"], warning["field"], warning["entry"]
                );
            }
        }
        print_skill_diagnostics(&skill_diagnostics);
    }
    print_skill_warnings(&skill_diagnostics, &loaded.control_root);
    Ok(())
}

fn is_empty_starter(
    agents: &std::collections::BTreeMap<String, crate::config::types::AgentConfig>,
) -> bool {
    !agents.is_empty()
        && agents.values().all(|agent| {
            agent.observe.is_empty() && agent.write.is_empty() && agent.commands.is_empty()
        })
}

fn print_skill_diagnostics(observations: &[project_skills::SkillObservation]) {
    let binary = invoking_binary();
    let product = if crate::project::layout_types::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    println!(
        "Managed skill diagnostics ({product} package {}; invoking binary {}):",
        project_skills::package_version(),
        binary.display
    );
    for observation in observations {
        if observation.optional
            && observation.state == project_skills::SkillObservationState::Absent
        {
            continue;
        }
        println!(
            "  {} {}: {} (embedded SHA256 {}; observed SHA256 {})",
            observation.skill,
            observation.path,
            skill_state_label(observation.state),
            observation.embedded_sha256,
            observation.observed_sha256.as_deref().unwrap_or("unknown")
        );
    }
}

fn print_skill_warnings(observations: &[project_skills::SkillObservation], control_root: &Path) {
    let binary = invoking_binary();
    let product = if crate::project::layout_types::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    let refresh = refresh_instruction(&binary, control_root);
    for observation in observations {
        let warning = match observation.state {
            project_skills::SkillObservationState::ManagedDifferent => Some(format!(
                "warning: {} managed skill {} differs from this binary's embedded skill ({product} package {}; invoking binary {}; embedded SHA256 {}; observed SHA256 {}). Inspect the invoking binary version/path; {} Install the intended release if needed.",
                observation.skill,
                observation.path,
                project_skills::package_version(),
                binary.display,
                observation.embedded_sha256,
                observation.observed_sha256.as_deref().unwrap_or("unknown"),
                refresh
            )),
            project_skills::SkillObservationState::Unsafe
            | project_skills::SkillObservationState::Unreadable
            | project_skills::SkillObservationState::Unsupported => Some(format!(
                "warning: {} skill {} could not be safely inspected ({}; {product} package {}; invoking binary {}; embedded SHA256 {}; observed SHA256 unknown). Inspect the invoking binary version/path, repair the skill path, then {} Install the intended release if needed.",
                observation.skill,
                observation.path,
                skill_state_label(observation.state),
                project_skills::package_version(),
                binary.display,
                observation.embedded_sha256,
                refresh
            )),
            _ => None,
        };
        if let Some(warning) = warning {
            eprintln!("{warning}");
        }
    }
}

struct PathPresentation {
    display: String,
    command: Option<String>,
}

fn invoking_binary() -> PathPresentation {
    std::env::current_exe()
        .map(|path| path_presentation(&path))
        .unwrap_or_else(|_| PathPresentation {
            display: "<invoking binary path unavailable>".to_owned(),
            command: None,
        })
}

fn path_presentation(path: &Path) -> PathPresentation {
    let value = path.to_string_lossy();
    let display = crate::presentation::shell_quote(value.as_ref());
    let command = path
        .to_str()
        .filter(|value| !value.chars().any(char::is_control))
        .map(crate::presentation::shell_quote);
    PathPresentation { display, command }
}

fn refresh_instruction(binary: &PathPresentation, control_root: &Path) -> String {
    let root = path_presentation(control_root);
    match (binary.command.as_deref(), root.command.as_deref()) {
        (Some(binary), Some(root)) => {
            format!("Explicitly run matching binary {binary} init --refresh-skills --root {root}.")
        }
        _ => format!(
            "No copyable refresh command is available: inspect invoking binary {} and ControlRoot {}; move control-bearing or non-UTF-8 paths to safe paths, then run that exact binary with init --refresh-skills.",
            binary.display, root.display
        ),
    }
}

fn skill_state_label(state: project_skills::SkillObservationState) -> &'static str {
    match state {
        project_skills::SkillObservationState::Absent => "absent",
        project_skills::SkillObservationState::EmbeddedMatch => "embedded match",
        project_skills::SkillObservationState::ManagedDifferent => "managed different",
        project_skills::SkillObservationState::Unmanaged => "unmanaged",
        project_skills::SkillObservationState::Unsafe => "unsafe path or nonregular",
        project_skills::SkillObservationState::Unreadable => "unreadable",
        project_skills::SkillObservationState::Unsupported => "unsupported inspection",
    }
}

fn required<'a>(arguments: &'a Arguments, name: &str, message: &str) -> Result<&'a str, String> {
    arguments
        .options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| message.to_owned())
}
