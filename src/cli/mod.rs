use serde_json::json;
use std::io::IsTerminal;

use crate::config::profile;
use crate::{
    config,
    distribution::update,
    envelope,
    evidence::receipt,
    host::{away, hooks, runtime as hook_runtime},
    memory::{self, forgetting},
    run,
};

pub(crate) mod args;
mod help;
mod replan_input;

use args::Arguments;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn positional<'a>(a: &'a Arguments, index: usize, message: &str) -> Result<&'a str, String> {
    a.positional
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| message.to_owned())
}

fn option<'a>(a: &'a Arguments, name: &str, message: &str) -> Result<&'a str, String> {
    a.options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| message.to_owned())
}

pub fn run(argv: Vec<String>) -> Result<(), String> {
    if argv.is_empty() {
        print_help();
        return Ok(());
    }

    let command = match argv[0].as_str() {
        "--help" => "help",
        "--version" => "version",
        command => command,
    };
    let parsed = if matches!(argv[0].as_str(), "--help" | "--version") {
        args::parse(&argv)?
    } else {
        args::parse(&argv[1..])?
    };

    if command == "version" || parsed.flags.contains_key("version") {
        args::assert_options("version", &parsed, &["version", "help"])?;
        args::assert_positionals("version", &parsed, 0)?;
        println!("{VERSION}");
        return Ok(());
    }
    if command == "help" {
        args::assert_options(command, &parsed, &["help", "version"])?;
        if parsed.positional == ["advanced"] {
            print_advanced_help();
            return Ok(());
        }
        args::assert_positionals(command, &parsed, 0)?;
        print_help();
        return Ok(());
    }
    if parsed.flags.contains_key("help") {
        args::assert_options(command, &parsed, &["help", "version"])?;
        if let Some(help) = help::scoped_help(command, &parsed.positional) {
            println!("{help}");
            return Ok(());
        }
        args::assert_positionals(command, &parsed, 0)?;
        print_help();
        return Ok(());
    }
    if command == "update" {
        args::assert_options(command, &parsed, &[])?;
        args::assert_positionals(command, &parsed, 0)?;
        return update::explicit_update();
    }
    match command {
        "hook-protocol" => {
            args::assert_options(command, &parsed, &[])?;
            args::assert_positionals(command, &parsed, 0)?;
            println!("{}", hooks::PROTOCOL);
            Ok(())
        }
        "hook-run" => {
            args::assert_options(command, &parsed, &[])?;
            args::assert_positionals(command, &parsed, 0)?;
            hook_runtime::run()
        }
        "init" => crate::project::commands::init(&parsed),
        "bind" => crate::project::commands::bind(&parsed),
        "doctor" => crate::project::commands::doctor(&parsed),
        "hooks" => hooks_command(&parsed),
        "host" => host_command(&parsed),
        "profile" if parsed.positional.first().map(String::as_str) == Some("audit") => {
            profile_audit_command(&parsed)
        }
        "goal" => {
            let loaded = config::load(parsed.options.get("config").map(String::as_str))?;
            goal_command(&loaded, &parsed)
        }
        command => configured_command(command, &parsed),
    }
}

fn hooks_command(a: &Arguments) -> Result<(), String> {
    args::assert_options("hooks", a, &["hosts", "root", "json"])?;
    args::assert_positionals("hooks", a, 1)?;
    let action = positional(
        a,
        0,
        "hooks requires one action: plan, apply, status, or remove",
    )?;
    let hosts = option(
        a,
        "hosts",
        "hooks requires explicit --hosts (for example --hosts codex,claude)",
    )?;
    let items = hooks::manage(
        action,
        hosts,
        a.options.get("root").map(String::as_str).unwrap_or("."),
    )?;
    if a.flags.contains_key("json") {
        print_json(&json!({ "action": action, "hosts": items }))?;
    } else {
        for item in items {
            println!(
                "{}: {}\n  target: {}",
                item["host"], item["state"], item["targetPath"]
            );
        }
    }
    Ok(())
}

/// User-level host bridge: one install makes Exitbind discoverable in every
/// repository, separately from project configuration and hooks.
fn host_command(a: &Arguments) -> Result<(), String> {
    args::assert_options("host", a, &["hosts", "all", "json"])?;
    args::assert_positionals("host", a, 1)?;
    let action = positional(a, 0, "host requires one action: status or install")?;
    let hosts = a.options.get("hosts").map(String::as_str);
    match action {
        "status" => {
            let items = crate::host::bridge::status(hosts)?;
            if a.flags.contains_key("json") {
                print_json(&json!({
                    "binaryVersion": crate::project::skills::package_version(),
                    "hosts": items,
                }))
            } else {
                println!("Exitbind {}", crate::project::skills::package_version());
                for item in items {
                    println!(
                        "\n{}\n  host present: {}\n  bootstrap skill: {}{}\n  activation hook: {}\n  path: {}",
                        item["host"].as_str().unwrap_or_default(),
                        item["hostPresent"],
                        item["bootstrapSkill"].as_str().unwrap_or_default(),
                        item["installedVersion"]
                            .as_str()
                            .map(|version| format!(" (installed {version})"))
                            .unwrap_or_default(),
                        item["activationHook"].as_str().unwrap_or("unknown"),
                        item["path"].as_str().unwrap_or_default()
                    );
                }
                println!(
                    "\nDiscovery is not activation: a session is governed only once an Exitbind work lifecycle action exists."
                );
                Ok(())
            }
        }
        "install" => {
            let items = crate::host::bridge::install(hosts, a.flags.contains_key("all"))?;
            if a.flags.contains_key("json") {
                print_json(&json!({ "action": "install", "hosts": items }))
            } else {
                for item in items {
                    println!(
                        "{}: bootstrap {} ({})",
                        item["host"].as_str().unwrap_or_default(),
                        item["action"].as_str().unwrap_or_default(),
                        item["path"].as_str().unwrap_or_default()
                    );
                    let hook = &item["activationHook"];
                    let state = hook["state"].as_str().unwrap_or("unknown");
                    match hook["reason"].as_str() {
                        Some(reason) => println!(
                            "{}: activation hook {state}: {reason}",
                            item["host"].as_str().unwrap_or_default()
                        ),
                        None => println!(
                            "{}: activation hook {state}",
                            item["host"].as_str().unwrap_or_default()
                        ),
                    }
                }
                Ok(())
            }
        }
        _ => Err("host requires one action: status or install".into()),
    }
}

fn configured_command(command: &str, a: &Arguments) -> Result<(), String> {
    if command == "benchmark" {
        return benchmark_command(a);
    }
    if command == "context" {
        return context_command(a);
    }
    if command == "work" && a.positional.first().map(String::as_str) == Some("classify") {
        return work_classify_command(a);
    }
    if !matches!(
        command,
        "check"
            | "brief"
            | "plan"
            | "verify"
            | "profile"
            | "memory"
            | "run"
            | "away"
            | "migrate"
            | "work"
            | "receipt"
    ) {
        return Err(format!("unknown command '{command}'"));
    }
    let loaded = match config::load(a.options.get("config").map(String::as_str)) {
        Ok(loaded) => loaded,
        Err(error) if command == "verify" => return Err(machine_error(error)),
        Err(error) => return Err(error),
    };
    match command {
        "check" => crate::project::commands::check(&loaded, a),
        "brief" => brief_command(&loaded, a),
        "plan" => plan_command(&loaded, a),
        "verify" => verify_command(&loaded, a),
        "receipt" => exit_receipt_command(&loaded, a),
        "profile" => profile_command(&loaded, a),
        "memory" => memory_command(&loaded, a),
        "run" => run_command(&loaded, a),
        "away" => away_command(&loaded, a),
        "migrate" => migrate_command(&loaded, a),
        "work" => work_command(&loaded, a),
        _ => Err(format!("unknown command '{command}'")),
    }
}

fn context_command(a: &Arguments) -> Result<(), String> {
    let action = positional(
        a,
        0,
        "context requires reduce, checkpoint, sensor-request, or seal",
    )?;
    match action {
        "reduce" => {
            args::assert_options("context reduce", a, &["events", "json"])?;
            args::assert_positionals("context reduce", a, 1)?;
            let events = option(a, "events", "context reduce requires --events FILE")?;
            print_json(&crate::context::reduce_file(events)?)
        }
        "checkpoint" => {
            args::assert_options("context checkpoint", a, &["state", "proposal", "json"])?;
            args::assert_positionals("context checkpoint", a, 1)?;
            let state = crate::context::read_json(option(
                a,
                "state",
                "context checkpoint requires --state FILE",
            )?)?;
            let proposal = crate::context::read_json(option(
                a,
                "proposal",
                "context checkpoint requires --proposal FILE",
            )?)?;
            print_json(&crate::context::checkpoint(&state, &proposal))
        }
        "sensor-request" => {
            args::assert_options("context sensor-request", a, &["state", "json"])?;
            args::assert_positionals("context sensor-request", a, 1)?;
            let state = crate::context::read_json(option(
                a,
                "state",
                "context sensor-request requires --state FILE",
            )?)?;
            print_json(&crate::context::sensor_request(&state)?)
        }
        "seal" => {
            args::assert_options("context seal", a, &["event", "previous", "json"])?;
            args::assert_positionals("context seal", a, 1)?;
            let value = crate::context::read_json(option(
                a,
                "event",
                "context seal requires --event FILE",
            )?)?;
            let previous = a
                .options
                .get("previous")
                .map(|path| crate::context::read_json(path))
                .transpose()?;
            print_json(&crate::context::event(previous.as_ref(), value))
        }
        _ => Err("context requires reduce, checkpoint, sensor-request, or seal".into()),
    }
}

fn work_classify_command(a: &Arguments) -> Result<(), String> {
    args::assert_options(
        "work classify",
        a,
        &[
            "config",
            "material-consequence",
            "promotion-required",
            "json",
        ],
    )?;
    args::assert_positionals("work classify", a, 1)?;
    let parse_bool = |name: &str| -> Result<bool, String> {
        match a.options.get(name).map(String::as_str) {
            Some("true") => Ok(true),
            Some("false") => Ok(false),
            Some(_) => Err(format!("--{name} requires true or false")),
            None => Err(format!("work classify requires --{name} true|false")),
        }
    };
    let loaded = config::load(a.options.get("config").map(String::as_str)).ok();
    let current_dir = std::env::current_dir().map_err(|error| error.to_string())?;
    print_json(&crate::work::classify(
        loaded.as_ref(),
        &current_dir,
        parse_bool("material-consequence")?,
        parse_bool("promotion-required")?,
    ))
}

fn goal_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(a, 0, "goal requires incorporate, close, or status")?;
    match action {
        "incorporate" => {
            args::assert_options(
                "goal incorporate",
                a,
                &[
                    "config",
                    "goal-id",
                    "goal",
                    "obligation",
                    "finding",
                    "blocker",
                    "decision",
                    "external-action",
                    "scope",
                    "disposition",
                    "result-ref",
                    "consider",
                    "none-applicable",
                    "direct",
                    "external-scope",
                    "json",
                ],
            )?;
            args::assert_positionals("goal incorporate", a, 1)?;
            let goal_id = option(a, "goal-id", "goal incorporate requires --goal-id")?;
            let goal = option(a, "goal", "goal incorporate requires --goal")?;
            let direct = a.flags.contains_key("direct");
            let direct_items = [
                ("obligations", a.options.get("obligation")),
                ("findings", a.options.get("finding")),
                ("blockers", a.options.get("blocker")),
                ("decisions", a.options.get("decision")),
                ("externalActions", a.options.get("external-action")),
            ];
            let selected = direct_items
                .iter()
                .filter_map(|(category, value)| value.as_deref().map(|item| (*category, item)))
                .collect::<Vec<_>>();
            let value = if direct && selected.len() == 1 {
                let (category, item) = selected[0];
                crate::session_goal::direct_complete(
                    l,
                    goal_id,
                    goal,
                    category,
                    item,
                    a.options.get("external-scope").map(String::as_str),
                )?
            } else {
                crate::session_goal::incorporate(
                    l,
                    goal_id,
                    goal,
                    a.options.get("obligation").map(String::as_str),
                    a.options.get("finding").map(String::as_str),
                    a.options.get("blocker").map(String::as_str),
                    a.options.get("decision").map(String::as_str),
                    a.options.get("external-action").map(String::as_str),
                    a.options.get("scope").map(String::as_str),
                    a.options.get("disposition").map(String::as_str),
                    a.options.get("result-ref").map(String::as_str),
                    a.options.get("consider").map(String::as_str),
                    a.options.get("none-applicable").map(String::as_str),
                )?
            };
            print_json(&value)
        }
        "close" => {
            args::assert_options(
                "goal close",
                a,
                &["config", "goal-id", "result-ref", "direct", "json"],
            )?;
            args::assert_positionals("goal close", a, 1)?;
            let goal_id = option(a, "goal-id", "goal close requires --goal-id")?;
            let value = if a.flags.contains_key("direct") {
                crate::session_goal::close_direct(
                    l,
                    goal_id,
                    a.options.get("result-ref").map(String::as_str),
                )?
            } else {
                crate::session_goal::close(
                    l,
                    goal_id,
                    option(a, "result-ref", "goal close requires --result-ref")?,
                )?
            };
            print_json(&value)
        }
        "status" => {
            args::assert_options("goal status", a, &["config", "json"])?;
            args::assert_positionals("goal status", a, 1)?;
            let record = crate::session_goal::read(&l.state_root)?;
            let value = record.clone().unwrap_or_else(|| json!({"closed": false}));
            print_json(&value)?;
            if !a.flags.contains_key("json") {
                let presentation =
                    crate::session_goal::presentation_for_loaded(l, record.as_ref())?;
                if let Some(card) = crate::presentation_events::session_goal_direct_card_for_human(
                    &l.state_root,
                    &presentation,
                    std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
                    !std::io::stdin().is_terminal(),
                ) {
                    println!("{card}");
                }
            }
            Ok(())
        }
        _ => Err("goal requires incorporate, close, or status".into()),
    }
}

fn work_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(
        a,
        0,
        "work requires begin, next, permit, replan, evidence, sensor-request, sensor-result, return, check, validate, expand, or resume",
    )?;
    match action {
        "begin" => {
            args::assert_options(
                "work begin",
                a,
                &[
                    "config",
                    "goal",
                    "check-command",
                    "boundary",
                    "harness-receipt",
                    "proof-origin",
                    "preserve-requirement",
                    "preservation-check-command",
                    "preservation-proof-origin",
                    "basis",
                    "review-policy",
                ],
            )?;
            args::assert_positionals("work begin", a, 2)?;
            print_json(&crate::work::begin(
                l,
                crate::work::BeginOptions {
                    workflow: positional(a, 1, "work begin requires WORKFLOW")?,
                    goal: option(a, "goal", "work begin requires --goal GOAL")?,
                    check_command: option(
                        a,
                        "check-command",
                        "work begin requires --check-command COMMAND",
                    )?,
                    boundary: a.options.get("boundary").map(String::as_str),
                    harness_receipt: a.options.get("harness-receipt").map(String::as_str),
                    proof_origin: a.options.get("proof-origin").map(String::as_str),
                    preserve_requirement: a.options.get("preserve-requirement").map(String::as_str),
                    preservation_check_command: a
                        .options
                        .get("preservation-check-command")
                        .map(String::as_str),
                    preservation_proof_origin: a
                        .options
                        .get("preservation-proof-origin")
                        .map(String::as_str),
                    basis: a.options.get("basis").map(String::as_str),
                    review_policy: a.options.get("review-policy").map(String::as_str),
                },
            )?)
        }
        "next" => {
            args::assert_options("work next", a, &["config", "json", "full"])?;
            args::assert_positionals("work next", a, 2)?;
            let result = crate::work::next(
                l,
                positional(a, 1, "work next requires WORK")?,
            )?;
            let result = if a.flags.contains_key("full") {
                result
            } else {
                crate::work::compact::project(&result, &l.path, "next")?
            };
            print_json(&result)
        }
        "permit" => {
            args::assert_options("work permit", a, &["config", "operation", "request-id"])?;
            args::assert_positionals("work permit", a, 3)?;
            print_json(&crate::work::permit(
                l,
                positional(a, 1, "work permit requires WORK ASSIGNMENT")?,
                positional(a, 2, "work permit requires WORK ASSIGNMENT")?,
                option(a, "operation", "work permit requires --operation OPERATION")?,
                a.options.get("request-id").map(String::as_str),
            )?)
        }
        "replan" => {
            args::assert_options(
                "work replan",
                a,
                &[
                    "config",
                    "hypothesis",
                    "evidence-request",
                    "scope-decision",
                    "blocker",
                    "replan-file",
                ],
            )?;
            args::assert_positionals("work replan", a, 3)?;
            let input = replan_input::ReplanInput::from_args(a)?;
            print_json(&crate::work::replan(
                l,
                positional(a, 1, "work replan requires WORK ASSIGNMENT")?,
                positional(a, 2, "work replan requires WORK ASSIGNMENT")?,
                input.hypothesis.as_deref(),
                input.evidence_request.as_deref(),
                input.scope_decision.as_deref(),
                input.blocker.as_deref(),
            )?)
        }
        "evidence" => {
            args::assert_options("work evidence", a, &["config", "artifact", "artifact-root"])?;
            args::assert_positionals("work evidence", a, 3)?;
            print_json(&crate::work::evidence(
                l,
                positional(a, 1, "work evidence requires WORK ASSIGNMENT")?,
                positional(a, 2, "work evidence requires WORK ASSIGNMENT")?,
                a.options
                    .get("artifact-root")
                    .map(String::as_str),
                option(a, "artifact", "work evidence requires --artifact PATH")?,
            )?)
        }
        "sensor-request" => {
            args::assert_options("work sensor-request", a, &["config"])?;
            args::assert_positionals("work sensor-request", a, 3)?;
            print_json(&crate::work::sensor_request(
                l,
                positional(a, 1, "work sensor-request requires WORK ASSIGNMENT")?,
                positional(a, 2, "work sensor-request requires WORK ASSIGNMENT")?,
            )?)
        }
        "sensor-result" => {
            args::assert_options(
                "work sensor-result",
                a,
                &["config", "assessment", "confidence", "input-digest", "identity-source"],
            )?;
            args::assert_positionals("work sensor-result", a, 3)?;
            print_json(&crate::work::sensor_result(
                l,
                positional(a, 1, "work sensor-result requires WORK ASSIGNMENT")?,
                positional(a, 2, "work sensor-result requires WORK ASSIGNMENT")?,
                option(a, "assessment", "work sensor-result requires --assessment VALUE")?,
                a.options.get("confidence").map(String::as_str),
                option(a, "input-digest", "work sensor-result requires --input-digest HEX")?,
                a.options
                    .get("identity-source")
                    .map(String::as_str)
                    .unwrap_or("host-reported"),
            )?)
        }
        "return" => {
            args::assert_options(
                "work return",
                a,
                &["config", "outcome", "reason", "disposition", "result-ref", "json"],
            )?;
            args::assert_positionals("work return", a, 3)?;
            print_json(&crate::work::return_result(
                l,
                positional(a, 1, "work return requires WORK ASSIGNMENT")?,
                positional(a, 2, "work return requires WORK ASSIGNMENT")?,
                option(a, "outcome", "work return requires --outcome OUTCOME")?,
                a.options.get("reason").map(String::as_str),
                a.options.get("disposition").map(String::as_str),
                a.options.get("result-ref").map(String::as_str),
            )?)
        }
        "check" => {
            args::assert_options("work check", a, &["config", "json"])?;
            args::assert_positionals("work check", a, 2)?;
            print_json(&crate::work::check(
                l,
                positional(a, 1, "work check requires WORK")?,
            )?)
        }
        "validate" => {
            args::assert_options("work validate", a, &["config", "packet"])?;
            args::assert_positionals("work validate", a, 2)?;
            let packet = a
                .options
                .get("packet")
                .ok_or("work validate requires --packet FILE")?;
            print_json(&crate::work::validate(
                l,
                positional(a, 1, "work validate requires WORK")?,
                packet,
            )?)
        }
        "expand" => {
            args::assert_options("work expand", a, &["config"])?;
            args::assert_positionals("work expand", a, 3)?;
            print_json(&crate::work::expand(
                l,
                positional(a, 1, "work expand requires WORK REF")?,
                positional(a, 2, "work expand requires WORK REF")?,
            )?)
        }
        "resume" => {
            args::assert_options("work resume", a, &["config", "json", "full"])?;
            args::assert_positionals("work resume", a, 1)?;
            let result = crate::work::resume(l)?;
            let result = if a.flags.contains_key("full") || result["status"] != "resumed" {
                result
            } else {
                crate::work::compact::project(&result, &l.path, "resume")?
            };
            print_json(&result)
        }
        _ => Err(
            "work requires begin, next, permit, replan, evidence, sensor-request, sensor-result, return, check, validate, expand, or resume"
                .into(),
        ),
    }
}

fn benchmark_command(a: &Arguments) -> Result<(), String> {
    args::assert_options("benchmark", a, &["output", "json"])?;
    args::assert_positionals("benchmark", a, 0)?;
    let value = crate::value_benchmark::run(a.options.get("output").map(String::as_str))?;
    if a.flags.contains_key("json") {
        print_json(&value)
    } else {
        print!("{}", crate::value_benchmark::render(&value)?);
        Ok(())
    }
}

fn migrate_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options("migrate", a, &["config", "apply"])?;
    args::assert_positionals("migrate", a, 1)?;
    let apply = a.flags.contains_key("apply");
    let value = match positional(a, 0, "migrate requires layout or paths")? {
        "layout" => crate::project::layout::run(l, apply),
        "paths" => crate::project::layout::prepare_paths(l, apply),
        _ => return Err("migrate requires layout or paths".into()),
    }?;
    print_json(&value)
}

fn away_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(a, 0, "away requires start, list, or show")?;
    match action {
        "start" => {
            args::assert_options(
                "away start",
                a,
                &["config", "name", "require-harness-receipt", "sandbox-mode"],
            )?;
            args::assert_positionals("away start", a, 3)?;
            let result = away::start(
                l,
                positional(a, 1, "away start requires AGENT LEDGER")?,
                positional(a, 2, "away start requires AGENT LEDGER")?,
                a.options.get("name").map(String::as_str).unwrap_or("away"),
                a.flags.contains_key("require-harness-receipt"),
                a.options.get("sandbox-mode").map(String::as_str),
            )?;
            println!(
                "run_id={}\nsocket={}\nsession={}\nstate={}",
                result["runId"].as_str().unwrap_or(""),
                result["socket"].as_str().unwrap_or(""),
                result["session"].as_str().unwrap_or(""),
                result["state"].as_str().unwrap_or("")
            );
            if let Some(warnings) = result["warnings"].as_array() {
                for warning in warnings {
                    if let Some(classification) = warning["classification"].as_str() {
                        eprintln!("warning={classification}");
                    }
                }
            }
            Ok(())
        }
        "list" => {
            args::assert_options("away list", a, &["config"])?;
            args::assert_positionals("away list", a, 1)?;
            let runs = away::list(l)?;
            if runs.is_empty() {
                println!(
                    "no {} away runs",
                    if crate::producer::exitbind_surface() {
                        "Exitbind"
                    } else {
                        "Soulmate"
                    }
                );
            } else {
                for (run, status) in runs {
                    println!("{run}\t{status}");
                }
            }
            Ok(())
        }
        "show" => {
            args::assert_options("away show", a, &["config"])?;
            args::assert_positionals("away show", a, 2)?;
            for (name, value) in away::show(l, positional(a, 1, "away show requires RUN_ID")?)? {
                println!("{name}={value}");
            }
            Ok(())
        }
        "_run" => {
            args::assert_options("away _run", a, &["config"])?;
            args::assert_positionals("away _run", a, 7)?;
            away::run_child(
                l,
                positional(a, 1, "invalid internal away command")?,
                positional(a, 2, "invalid internal away command")?,
                positional(a, 3, "invalid internal away command")?,
                (
                    positional(a, 4, "invalid internal away command")?,
                    positional(a, 5, "invalid internal away command")?,
                ),
                positional(a, 6, "invalid internal away command")?,
            )
        }
        _ => Err("away requires start, list, or show".into()),
    }
}

fn brief_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options(
        "brief",
        a,
        &["config", "task", "receipt", "harness-manifest", "json"],
    )?;
    args::assert_positionals("brief", a, 1)?;
    let name = positional(a, 0, "brief accepts 1 positional argument")?;
    let task = option(a, "task", "--task requires a non-empty value")?;
    let value = envelope::brief(l, name, task)?;
    if let Some(path) = a.options.get("receipt") {
        receipt::write(
            path,
            l,
            &value,
            a.options.get("harness-manifest").map(String::as_str),
        )?;
    }
    if a.flags.contains_key("json") {
        print_json(&value)?;
    } else {
        print!("{}", envelope::render(&value));
    }
    Ok(())
}

fn plan_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options(
        "plan",
        a,
        &["config", "goal", "receipt", "harness-manifest"],
    )?;
    args::assert_positionals("plan", a, 1)?;
    let workflow = positional(a, 0, "plan accepts 1 positional argument")?;
    let goal = option(a, "goal", "--goal requires a non-empty value")?;
    let value = envelope::plan(l, workflow, goal)?;
    if let Some(path) = a.options.get("receipt") {
        receipt::write(
            path,
            l,
            &value,
            a.options.get("harness-manifest").map(String::as_str),
        )?;
    }
    print_json(&value)?;
    Ok(())
}

fn verify_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options("verify", a, &["config"])?;
    args::assert_positionals("verify", a, 1)?;
    let path = positional(a, 0, "verify requires a receipt path")?;
    let value = match receipt::verify_exit_path(path, l) {
        Ok(value) if value["format"] == "exit-path-v1" => value,
        Ok(_) => receipt::verify(path, l)?,
        Err(error) if crate::producer::exitbind_surface() => match receipt::verify(path, l) {
            Ok(value) => value,
            Err(_) => return Err(machine_error(error)),
        },
        Err(_) => receipt::verify(path, l)?,
    };
    let exit_path = value["format"] == "exit-path-v1";
    if !value["valid"].as_bool().unwrap_or(false) {
        if exit_path {
            let machine = serde_json::to_string(&value).map_err(|error| error.to_string())?;
            return Err(format!("EXITBIND_JSON:{machine}"));
        }
        print_json(&value)?;
        return Err("receipt verification failed".into());
    }
    print_json(&value)?;
    Ok(())
}

fn exit_receipt_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options("receipt", a, &["config", "json", "output"])?;
    args::assert_positionals("receipt", a, 1)?;
    let ledger = positional(a, 0, "receipt requires a checked run ledger path")?;
    let value = receipt::exit_path(l, ledger).map_err(|error| match error {
        receipt::ExitPathError::Decision(decision) => exit_path_failure(decision, None),
        receipt::ExitPathError::Failure(detail) => detail,
    })?;
    if let Some(path) = a.options.get("output") {
        let target = receipt::exit_receipt_path(l, path)?;
        let source =
            serde_json::to_string_pretty(&value).map_err(|error| error.to_string())? + "\n";
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(target)
            .map_err(|error| error.to_string())?;
        file.write_all(source.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    print_json(&value)
}

fn exit_path_failure(decision: crate::run_exit::ExitDecision, detail: Option<String>) -> String {
    let (outcome, code, wire_detail) = decision.wire();
    format!(
        "EXITBIND_JSON:{}",
        serde_json::json!({
            "product": "exitbind",
            "format": "exit-path-v1",
            "valid": false,
            "outcome": outcome,
            "reason": {"code": code, "detail": detail.unwrap_or_else(|| wire_detail.to_owned())},
        })
    )
}

/// The prefix `main` strips before printing a machine payload.
fn machine_json(machine: &str) -> String {
    let prefix = if crate::producer::exitbind_surface() {
        "EXITBIND_JSON:"
    } else {
        "SOULMATE_JSON:"
    };
    format!("{prefix}{machine}")
}

fn machine_error(error: String) -> String {
    let prefix = if crate::producer::exitbind_surface() {
        "EXITBIND_JSON:"
    } else {
        "SOULMATE_JSON:"
    };
    format!("{prefix}{}", serde_json::json!({"error": error}))
}

fn profile_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = a.positional.first().map(String::as_str).unwrap_or("");
    match action {
        "audit" => profile_audit_command(a),
        "import" => {
            args::assert_options("profile import", a, &["config", "purpose", "forbid-term"])?;
            args::assert_positionals("profile import", a, 3)?;
            let result = profile::import(
                l,
                positional(a, 1, "profile import requires NAME SOURCE")?,
                positional(a, 2, "profile import requires NAME SOURCE")?,
                option(a, "purpose", "profile import requires --purpose")?,
                a.options.get("forbid-term").map(String::as_str),
            )?;
            print!("{result}");
            Ok(())
        }
        "" => Err("profile requires AGENT".into()),
        _ => {
            args::assert_options("profile", a, &["config"])?;
            args::assert_positionals("profile", a, 1)?;
            let name = positional(a, 0, "profile requires AGENT")?;
            let agent = l
                .agent(name)
                .ok_or_else(|| format!("unknown agent '{name}'"))?;
            let path = config::file(&l.control_root, &agent.profile)?;
            print!(
                "{}",
                std::fs::read_to_string(path).map_err(|e| e.to_string())?
            );
            Ok(())
        }
    }
}

fn profile_audit_command(a: &Arguments) -> Result<(), String> {
    args::assert_options("profile audit", a, &["config", "forbid-term", "json"])?;
    args::assert_positionals("profile audit", a, 2)?;
    let value = profile::audit(
        positional(a, 1, "profile audit requires SOURCE")?,
        a.options.get("forbid-term").map(String::as_str),
    )?;
    if a.flags.contains_key("json") {
        print_json(&value)?;
    } else {
        println!(
            "Profile audit {}: {}",
            if value["valid"].as_bool().unwrap_or(false) {
                "passed"
            } else {
                "failed"
            },
            value["source"]
        );
    }
    if value["valid"].as_bool().unwrap_or(false) {
        Ok(())
    } else {
        Err("profile audit failed".into())
    }
}

fn memory_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(a, 0, "memory requires an action")?;
    match action {
        "resolve" => {
            args::assert_options("memory resolve", a, &["config", "json"])?;
            args::assert_positionals("memory resolve", a, 2)?;
            let agent = positional(a, 1, "memory resolve requires AGENT")?;
            let value = memory::resolve(l, agent)?;
            if a.flags.contains_key("json") {
                print_json(&value)?;
            } else if let Some(references) = value["references"].as_array() {
                for reference in references {
                    println!(
                        "{} {} {} {}",
                        reference["itemId"].as_str().unwrap_or(""),
                        reference["scope"].as_str().unwrap_or(""),
                        reference["sourcePath"].as_str().unwrap_or(""),
                        reference["sourceSha256"].as_str().unwrap_or("")
                    );
                }
            }
            Ok(())
        }
        "attest-forgotten" => {
            args::assert_options("memory attest-forgotten", a, &["config", "receipt"])?;
            args::assert_positionals("memory attest-forgotten", a, 3)?;
            let actor = positional(a, 1, "memory attest-forgotten requires AGENT LEDGER")?;
            let ledger = positional(a, 2, "memory attest-forgotten requires AGENT LEDGER")?;
            let receipt = option(a, "receipt", "memory attest-forgotten requires --receipt")?;
            let value = forgetting::attest(l, actor, ledger, receipt)?;
            print_json(&value)?;
            Ok(())
        }
        "inspect" => {
            args::assert_options("memory inspect", a, &["config", "json"])?;
            args::assert_positionals("memory inspect", a, 2)?;
            let value = memory::inspect(l, positional(a, 1, "memory inspect accepts LEDGER")?)?;
            if a.flags.contains_key("json") {
                print_json(&value)?;
            } else if let Some(items) = value["items"].as_array() {
                for item in items {
                    println!("{} {} {}", item["itemId"], item["state"], item["scope"]);
                }
            }
            Ok(())
        }
        "propose" => memory_transition(l, a, true),
        _ => memory_transition(l, a, false),
    }
}

fn memory_transition(l: &config::Loaded, a: &Arguments, propose: bool) -> Result<(), String> {
    let allowed = if propose {
        &["config", "ledger", "scope", "expires-at"][..]
    } else {
        &["config"][..]
    };
    args::assert_options("memory", a, allowed)?;
    args::assert_positionals("memory", a, 3)?;
    let actor = positional(a, 1, "memory action requires AGENT LEDGER")?;
    let ledger = if propose {
        option(a, "ledger", "memory propose requires --ledger")?
    } else {
        positional(a, 2, "memory transition requires LEDGER")?
    };
    let value = memory::action(
        l,
        actor,
        positional(a, 0, "memory requires an action")?,
        if propose {
            a.positional.get(2).map(String::as_str)
        } else {
            None
        },
        a.options.get("scope").map(String::as_str),
        ledger,
        a.options.get("expires-at").map(String::as_str),
    )?;
    print_json(&value["event"])?;
    Ok(())
}

fn run_command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(a, 0, "run requires an action")?;
    let allowed = match action {
        "start" => &[
            "config",
            "goal",
            "ledger",
            "boundary",
            "harness-receipt",
            "check-command",
            "proof-origin",
            "preserve-requirement",
            "preservation-check-command",
            "preservation-proof-origin",
            "basis",
            "review-policy",
        ][..],
        "next" => &["config", "json", "text"][..],
        "inspect" => &["config", "event", "json"][..],
        "submit" => &["config", "outcome", "artifact", "artifact-root", "reason", "disposition", "json", "event-id"][..],
        "review-policy" => &["config", "decision", "reason", "json"][..],
        "record-check" => &[
            "config",
            "target",
            "check-command",
            "exit-code",
            "duration-ms",
            "requirement",
            "json",
        ][..],
        "observe-check" => &["config", "target", "requirement", "timeout-ms", "json"][..],
        "status" => &["config", "json", "session-closed"][..],
        "explain" => &["config", "event", "json", "session-closed"][..],
        "report" => &["config", "json"][..],
        "supersede" => &[
            "config",
            "workflow",
            "goal",
            "ledger",
            "boundary",
            "harness-receipt",
            "check-command",
            "proof-origin",
            "preserve-requirement",
            "preservation-check-command",
            "preservation-proof-origin",
            "basis",
            "review-policy",
            "json",
        ][..],
        _ => {
            return Err("run requires one action: start, next, submit, review-policy, record-check, observe-check, status, explain, report, inspect, or supersede".into())
        }
    };
    args::assert_options("run", a, allowed)?;
    for alternative in ["event-id", "text"] {
        if a.flags.contains_key(alternative) && a.flags.contains_key("json") {
            return Err(format!("--{alternative} and --json are mutually exclusive"));
        }
    }
    let value = match action {
        "start" => {
            args::assert_positionals("run start", a, 2)?;
            let workflow = positional(a, 1, "run start requires WORKFLOW")?;
            let goal = option(a, "goal", "run start requires --goal")?;
            let ledger = option(a, "ledger", "run start requires --ledger")?;
            let boundary = a.options.get("boundary").map(String::as_str);
            let receipt = a.options.get("harness-receipt").map(String::as_str);
            let check_command = a.options.get("check-command").map(String::as_str);
            let proof_origin = a.options.get("proof-origin").map(String::as_str);
            let preserve_requirement = a.options.get("preserve-requirement").map(String::as_str);
            let preservation_check_command = a
                .options
                .get("preservation-check-command")
                .map(String::as_str);
            let preservation_proof_origin = a
                .options
                .get("preservation-proof-origin")
                .map(String::as_str);
            if check_command.is_none()
                && proof_origin.is_none()
                && preserve_requirement.is_none()
                && preservation_check_command.is_none()
                && preservation_proof_origin.is_none()
                && a.options.get("basis").is_none()
                && a.options.get("review-policy").is_none()
            {
                run::start(l, workflow, goal, ledger, boundary, receipt)
            } else {
                run::start_with_policy(
                    l,
                    workflow,
                    goal,
                    ledger,
                    boundary,
                    receipt,
                    check_command,
                    proof_origin,
                    preserve_requirement,
                    preservation_check_command,
                    preservation_proof_origin,
                    a.options.get("basis").map(String::as_str),
                    a.options.get("review-policy").map(String::as_str),
                )
            }
        }
        "next" => {
            args::assert_positionals("run next", a, 2)?;
            run::next(l, positional(a, 1, "run next requires LEDGER")?)
        }
        "submit" => {
            args::assert_positionals("run submit", a, 3)?;
            run::submit(
                l,
                positional(a, 1, "run submit requires AGENT")?,
                positional(a, 2, "run submit requires LEDGER")?,
                option(a, "outcome", "run submit requires --outcome")?,
                option(a, "artifact", "run submit requires --artifact")?,
                a.options.get("artifact-root").map(String::as_str),
                a.options.get("reason").map(String::as_str),
                a.options.get("disposition").map(String::as_str),
            )
        }
        "review-policy" => {
            args::assert_positionals("run review-policy", a, 3)?;
            run::review_policy(
                l,
                positional(a, 1, "run review-policy requires AGENT")?,
                positional(a, 2, "run review-policy requires LEDGER")?,
                option(a, "decision", "run review-policy requires --decision required|omitted")?,
                a.options.get("reason").map(String::as_str),
            )
        }
        "inspect" => {
            args::assert_positionals("run inspect", a, 2)?;
            let ledger = positional(a, 1, "run inspect requires LEDGER")?;
            match a.options.get("event").map(String::as_str) {
                Some(event) => run::inspect_event(l, ledger, event),
                None => run::inspect(l, ledger),
            }
        }
        "record-check" => {
            args::assert_positionals("run record-check", a, 2)?;
            let ledger = positional(a, 1, "run record-check requires LEDGER")?;
            let target = option(a, "target", "run record-check requires --target")?;
            let check_command = option(
                a,
                "check-command",
                "run record-check requires --check-command",
            )?;
            let exit_code = option(a, "exit-code", "run record-check requires --exit-code")?;
            let duration_ms = a.options.get("duration-ms").map(String::as_str);
            if let Some(requirement) = a.options.get("requirement").map(String::as_str) {
                run::record_check_for_requirement(
                    l,
                    ledger,
                    target,
                    Some(requirement),
                    check_command,
                    exit_code,
                    duration_ms,
                )
            } else {
                run::record_check(l, ledger, target, check_command, exit_code, duration_ms)
            }
        }
        "observe-check" => {
            args::assert_positionals("run observe-check", a, 2)?;
            let ledger = positional(a, 1, "run observe-check requires LEDGER")?;
            let target = option(a, "target", "run observe-check requires --target")?;
            let timeout_ms = a.options.get("timeout-ms").map(String::as_str);
            if let Some(requirement) = a.options.get("requirement").map(String::as_str) {
                run::observe_check_for_requirement(l, ledger, target, Some(requirement), timeout_ms)
            } else {
                run::observe_check(l, ledger, target, timeout_ms)
            }
        }
        "status" => {
            args::assert_positionals("run status", a, 2)?;
            let ledger = positional(a, 1, "run status requires LEDGER")?;
            let json_output = a.flags.contains_key("json");
            if json_output {
                let mut value = run::status(l, ledger)
                    .map_err(|error| map_run_error(error, true))?;
                if let Some(terminal) = run::terminal_display(l, ledger) {
                    value["terminal"] = json!(terminal);
                }
                print_json(&value)?;
                return Ok(());
            }
            let config = a.options.get("config").map(String::as_str).map_or_else(
                || l.path.to_str().ok_or("configuration path is not valid UTF-8"),
                Ok,
            )?;
            let value = run::human_status(l, ledger).map_err(|error| {
                if run::inspect(l, ledger).is_ok() {
                    eprintln!(
                        "Inspect: {}",
                        crate::presentation::read_command("inspect", config, ledger)
                    );
                }
                map_run_error(error, false)
            })?;
            let current_goal = crate::session_goal::read(&l.state_root)?;
            let session_goal = crate::session_goal::presentation_for_loaded(
                l,
                current_goal.as_ref(),
            )?;
            let card_emitted = crate::run_presentation::print_status(
                &value,
                &session_goal,
                &l.state_root,
                std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
                !std::io::stdin().is_terminal(),
            );
            if !card_emitted {
                if value.status != "running" || value.artifact_status != "current" {
                    println!("Inspect: {}", crate::presentation::read_command("inspect", config, ledger));
                } else {
                    match run::next(l, ledger) {
                        Ok(next) if next["status"] == "running" => println!(
                            "Next: {}",
                            crate::presentation::read_command("next", config, ledger)
                        ),
                        _ => {
                            println!("Guidance: no validated pending progression is available; inspect the run before recovery.");
                            println!("Inspect: {}", crate::presentation::read_command("inspect", config, ledger));
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
            return Ok(());
        }
        "explain" => {
            args::assert_positionals("run explain", a, 2)?;
            if a.flags.contains_key("json") {
                let machine = run::explain(
                    l,
                    positional(a, 1, "run explain requires LEDGER")?,
                    a.options.get("event").map(String::as_str),
                )
                .map_err(|error| map_run_error(error, true))?;
                print_json(&machine)?;
            } else {
                let value = run::human_explain(
                    l,
                    positional(a, 1, "run explain requires LEDGER")?,
                    a.options.get("event").map(String::as_str),
                )
                .map_err(|error| map_run_error(error, false))?;
                let current_goal = crate::session_goal::read(&l.state_root)?;
                let session_goal = crate::session_goal::presentation_for_loaded(
                    l,
                    current_goal.as_ref(),
                )?;
                crate::run_presentation::print_explain(
                    &value,
                    &session_goal,
                    &l.state_root,
                    std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
                    !std::io::stdin().is_terminal(),
                );
            }
            return Ok(());
        }
        "report" => {
            if a.positional.len() < 2 {
                return Err("run report requires at least one LEDGER".into());
            }
            let ledgers = a
                .positional
                .iter()
                .skip(1)
                .map(String::as_str)
                .collect::<Vec<_>>();
            let report = run::report(l, &ledgers)?;
            if a.flags.contains_key("json") {
                print_json(&report)?;
            } else {
                print!("{}", run::report_markdown(&report));
            }
            return Ok(());
        }
        "supersede" => {
            args::assert_positionals("run supersede", a, 2)?;
            let old_ledger = positional(a, 1, "run supersede requires OLD_LEDGER")?;
            let workflow = option(a, "workflow", "run supersede requires --workflow")?;
            let goal = option(a, "goal", "run supersede requires --goal")?;
            let ledger = option(a, "ledger", "run supersede requires --ledger")?;
            let boundary = a.options.get("boundary").map(String::as_str);
            let receipt = a.options.get("harness-receipt").map(String::as_str);
            let check_command = a.options.get("check-command").map(String::as_str);
            let proof_origin = a.options.get("proof-origin").map(String::as_str);
            let preserve_requirement = a.options.get("preserve-requirement").map(String::as_str);
            let preservation_check_command = a
                .options
                .get("preservation-check-command")
                .map(String::as_str);
            let preservation_proof_origin = a
                .options
                .get("preservation-proof-origin")
                .map(String::as_str);
            if check_command.is_none()
                && proof_origin.is_none()
                && preserve_requirement.is_none()
                && preservation_check_command.is_none()
                && preservation_proof_origin.is_none()
                && a.options.get("basis").is_none()
                && a.options.get("review-policy").is_none()
            {
                run::supersede(l, old_ledger, workflow, goal, ledger, boundary, receipt)
            } else {
                run::supersede_with_policy(
                    l,
                    old_ledger,
                    workflow,
                    goal,
                    ledger,
                    boundary,
                    receipt,
                    check_command,
                    proof_origin,
                    preserve_requirement,
                    preservation_check_command,
                    preservation_proof_origin,
                    a.options.get("basis").map(String::as_str),
                    a.options.get("review-policy").map(String::as_str),
                )
            }
        }
        _ => return Err("unsupported run action".into()),
    }
    .map_err(|error| map_run_error(error, a.flags.contains_key("json")))?;
    if action == "start" {
        if a.options.contains_key("check-command") {
            eprintln!("Checked run: v8 acceptance may use local observe-check or a caller-reported record-check for each current worker submission, bound to the frozen command. Historical v1-v7 runs remain readable.");
        } else {
            eprintln!("Unchecked run: no check-result requirement is configured. Start with --check-command to require check evidence before acceptance.");
        }
    }
    if action == "submit" && a.flags.contains_key("event-id") {
        println!(
            "{}",
            value["event"]["eventSha256"]
                .as_str()
                .expect("successful submission has a validated event hash")
        );
    } else if action == "next" && a.flags.contains_key("text") {
        crate::presentation::print_next(&value)?;
    } else {
        print_json(&value)?;
    }
    Ok(())
}

fn map_run_error(error: String, json_output: bool) -> String {
    let Some(machine) = error
        .strip_prefix(crate::run::error::DRIFT_PREFIX)
        .or_else(|| error.strip_prefix(crate::run::error::LEGACY_DRIFT_PREFIX))
    else {
        return error;
    };
    let command = if crate::producer::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    if json_output {
        return machine_json(machine);
    } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(machine) {
        if value["classification"] == "config_drift" {
            eprintln!(
                "configuration drift detected after run start (expected {}, current {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                value["expectedConfigSha256"], value["currentConfigSha256"]
            );
        } else if value["classification"] == "profile_drift" {
            eprintln!(
                "profile drift detected after run start for {} (expected {}, current {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                value["agent"], value["expectedProfileSha256"], value["currentProfileSha256"]
            );
        } else if value["classification"] == "boundary_drift" {
            if value["currentBoundaryState"] == "absent" {
                eprintln!(
                    "run boundary manifest is absent after run start (expected hash {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                    value["expectedBoundarySha256"]
                );
            } else {
                eprintln!(
                    "run boundary manifest drift detected after run start (expected {}, current {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                    value["expectedBoundarySha256"], value["currentBoundarySha256"]
                );
            }
        } else if value["classification"] == "harness_receipt_drift" {
            eprintln!(
                "semantic harness drift detected after run start (expected {}, current {}); continue with the recorded plan. Exact receipt integrity failures are refused separately.",
                value["expectedHarnessReceiptSha256"], value["currentHarnessReceiptSha256"]
            );
        } else {
            eprintln!(
                "memory drift detected after run start for {} (expected set {}, current set {}); continue with the recorded plan. Use '{command} run supersede' only to bind a new run to changed inputs.",
                value["agent"], value["expectedMemorySetSha256"], value["currentMemorySetSha256"]
            );
        }
    }
    if machine.contains("\"classification\":\"boundary_drift\"") {
        "run boundary drift".to_string()
    } else if machine.contains("\"classification\":\"harness_receipt_drift\"") {
        "harness receipt drift".to_string()
    } else if machine.contains("\"classification\":\"memory_drift\"") {
        "run memory drift".to_string()
    } else if machine.contains("\"classification\":\"profile_drift\"") {
        "run profile drift".to_string()
    } else {
        "run configuration drift".to_string()
    }
}

fn print_help() {
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    let command = if crate::producer::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    println!(
        "{product} {VERSION}\n\nUsage: {command} <command> [options]\n\nCore: init, brief, work, check\n  init prepares portable project setup and reviewable agent configuration.\n  brief presents one bounded task to an existing agent host.\n  work drives a checked task through opaque next actions and managed evidence.\n  check validates configuration, profiles, and declared boundaries; the host runs project tests and reports their result.\n\nRun '{command} benchmark' for the model-free checked-work demonstration.\nRun '{command} help advanced' for work/run actions, recovery, migration, hooks, receipts, and optional surfaces.\nRun '{command} context reduce|checkpoint|seal' for replayable bounded context state."
    );
}

fn print_advanced_help() {
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    let command = if crate::producer::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    let help = format!(
        "{product} {VERSION}\n\nDo the next change\n  {command} init --mode portable --root ROOT\n  {command} brief worker --task TASK --config CONFIG\n  {command} run start WORKFLOW --goal GOAL --ledger LEDGER [--check-command COMMAND]\n  {command} run next LEDGER [--text]\n  {command} run submit AGENT LEDGER --outcome OUTCOME --artifact ARTIFACT [--event-id]\n  {command} run record-check LEDGER --target EVENT_SHA --check-command COMMAND --exit-code CODE [--duration-ms MS]\n\nWhen work fails or changes\n  {command} run status LEDGER\n  {command} run explain LEDGER [--event PROTECTION_EVENT_SHA]\n  {command} run report LEDGER [LEDGER ...]\n  {command} run inspect LEDGER\n  {command} run supersede OLD_LEDGER --workflow WORKFLOW --goal GOAL --ledger NEW_LEDGER\n  --text prints a readable pending assignment; --event-id prints the submitted event hash.\n  Each output flag conflicts with --json; default JSON is unchanged.\n\nOptional surfaces\n  Advanced commands: bind, doctor, plan, verify, profile, migrate, memory (resolve/inspect/lifecycle), away, host, hooks, hook-protocol, hook-run, version.\n  '{command} host status' reports the managed bootstrap skill installed for each supported host; '{command} host install' reinstalls or refreshes it. Installation and update manage it for you.\n  Run value proof: the host executes the configured check, then reports its actual result with 'run record-check'; use 'run status', 'run explain', and 'run report' for bounded evidence views.\n  Run '{command} migrate layout --config CONFIG' to inspect a legacy profile migration, then repeat with --apply. Use 'migrate paths' for canonical harness and state directories.\n  Use '{command} run supersede OLD_LEDGER --workflow WORKFLOW --goal GOAL --ledger NEW_LEDGER' only when you want a successor bound to changed configuration, profile, memory, boundary, or harness inputs."
    );
    let value_help = if crate::producer::exitbind_surface() {
        "Run value proof: new v8 runs may observe the frozen check locally with 'run observe-check' or record a host report with 'run record-check'; historical v3-v7 runs remain readable. Use 'run status', 'run explain', and 'run report' for bounded evidence views."
    } else {
        "Run value proof: new v4 runs may observe the frozen check locally with 'run observe-check' or record a host report with 'run record-check'; historical v3 runs are reported-only. Use 'run status', 'run explain', and 'run report' for bounded evidence views."
    };
    println!("{}", help.replace(
        "Run value proof: the host executes the configured check, then reports its actual result with 'run record-check'; use 'run status', 'run explain', and 'run report' for bounded evidence views.",
        value_help,
    ));
    println!("  Checked {} runs may use: {command} run observe-check LEDGER --target EVENT_SHA [--timeout-ms MS]", if crate::producer::exitbind_surface() { "v8" } else { "v4" });
    println!("  Local observation defaults to a 1,800,000 ms (30 minute) timeout; --timeout-ms must be positive.");
    println!("Run '{command} update' to explicitly install the newest allowed release.");
}

fn print_json(value: &serde_json::Value) -> Result<(), String> {
    println!("{}", serialize_json(value)?);
    Ok(())
}

fn serialize_json(value: &serde_json::Value) -> Result<String, String> {
    if value["compact"] == true {
        serde_json::to_string(value).map_err(|error| error.to_string())
    } else {
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::serialize_json;

    #[test]
    fn compact_responses_omit_only_serialization_whitespace() {
        let compact = serialize_json(&serde_json::json!({
            "compact": true,
            "works": ["smw_example"],
        }))
        .unwrap();
        assert_eq!(compact, r#"{"compact":true,"works":["smw_example"]}"#);

        let pretty = serialize_json(&serde_json::json!({"works": ["smw_example"]})).unwrap();
        assert!(pretty.contains('\n'));
    }
}
