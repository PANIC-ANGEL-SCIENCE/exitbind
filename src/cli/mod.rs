use serde_json::json;
use std::io::{IsTerminal, Read};

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

mod activity;
pub(crate) mod args;
mod help;
mod project;
mod replan_input;
mod retrospective;
mod run_cli;
mod status;
mod work;

use args::Arguments;
use run_cli::map_run_error;

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

fn context_fence(a: &Arguments, work: &str) -> Result<(u64, u64), String> {
    let context = a
        .options
        .get("context")
        .ok_or("work bind/child requires the emitted --context TOKEN")?;
    let parts = context.split(':').collect::<Vec<_>>();
    if parts.len() != 4 {
        return Err("work context must be the emitted mutationContext token".into());
    }
    let context_work = parts[0];
    let goal = parts[1]
        .parse::<u64>()
        .map_err(|_| "work context goal revision is invalid")?;
    let binding = parts[2]
        .parse::<u64>()
        .map_err(|_| "work context binding revision is invalid")?;
    let digest = parts[3];
    if context_work != work {
        return Err("work context belongs to another work locator".into());
    }
    let body = json!({"work":work,"goalRevision":goal,"bindingRevision":binding});
    if crate::evidence::hash::value(&body) != digest {
        return Err("work context token is invalid".into());
    }
    Ok((goal, binding))
}

fn bounded_cli<'a>(value: &'a str, field: &str, max: usize) -> Result<&'a str, String> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(format!("work {field} is required"));
    }
    if value.len() > max {
        let next = if field == "assignment" {
            "use a short label and keep the full task in the recorded requirements"
        } else {
            "report the oversized native field; no record was written"
        };
        return Err(format!("work {field} exceeds {max} bytes; {next}"));
    }
    if value.contains('\0') {
        return Err(format!("work {field} contains NUL"));
    }
    Ok(value)
}

fn child_result(a: &Arguments) -> Result<String, String> {
    let inline = a.options.get("result").map(String::as_str);
    let bytes = if let Some(value) = inline {
        if value.len() > 8 * 1024 {
            return Err("work child result exceeds 8192 bytes; keep the exact result and report that an assessed artifact route is needed; no child record was written".into());
        }
        value.as_bytes().to_vec()
    } else {
        if std::io::stdin().is_terminal() {
            return Err("work child requires --result or piped child result".into());
        }
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(8 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("work child result: {error}"))?;
        bytes
    };
    if bytes.len() > 8 * 1024 {
        return Err("work child result exceeds 8192 bytes; keep the exact result and report that an assessed artifact route is needed; no child record was written".into());
    }
    String::from_utf8(bytes).map_err(|_| "work child result must be UTF-8".into())
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
        args::assert_options("version", &parsed, &["version", "help", "json"])?;
        args::assert_positionals("version", &parsed, 0)?;
        if parsed.flags.contains_key("json") {
            println!("{}", crate::producer::build_identity());
        } else {
            println!("{VERSION}");
        }
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
    if command == "retrospective" {
        return retrospective::command(&parsed);
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
        "activity" => {
            let loaded = config::load(parsed.options.get("config").map(String::as_str))?;
            activity::command(&loaded, &parsed)
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
            | "project"
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
        "run" => run_cli::run_command(&loaded, a),
        "away" => away_command(&loaded, a),
        "migrate" => migrate_command(&loaded, a),
        "work" => work::work_command(&loaded, a),
        "project" => project::command(&loaded, a),
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
        "status" => status::goal_status(l, a),
        _ => Err("goal requires incorporate, close, or status".into()),
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
        "{product} {VERSION}\n\nUsage: {command} <command> [options]\n\nCore: init, brief, work, check, activity\n  init prepares portable project setup and reviewable agent configuration.\n  brief presents one bounded task to an existing agent host.\n  work drives a checked task through opaque next actions and managed evidence.\n  activity codex runs and privately records a direct small task from standard input.\n  check validates configuration, profiles, and declared boundaries; the host runs project tests and reports their result.\n\nRun '{command} benchmark' for the model-free checked-work demonstration.\nRun '{command} help advanced' for work/run actions, recovery, migration, hooks, receipts, and optional surfaces.\nRun '{command} context reduce|checkpoint|seal' for replayable bounded context state."
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
        "{product} {VERSION}\n\nDo the next change\n  {command} init --mode portable --root ROOT\n  {command} brief worker --task TASK --config CONFIG\n  {command} run start WORKFLOW --goal GOAL --ledger LEDGER [--check-command COMMAND]\n  {command} run next LEDGER [--text]\n  {command} run submit AGENT LEDGER --outcome OUTCOME --artifact ARTIFACT [--event-id]\n  {command} run record-check LEDGER --target EVENT_SHA --check-command COMMAND --exit-code CODE [--duration-ms MS]\n\nWhen work fails or changes\n  {command} run status LEDGER\n  {command} run explain LEDGER [--event PROTECTION_EVENT_SHA]\n  {command} run report LEDGER [LEDGER ...]\n  {command} run inspect LEDGER\n  {command} run supersede OLD_LEDGER --workflow WORKFLOW --goal GOAL --ledger NEW_LEDGER\n  --text prints a readable pending assignment; --event-id prints the submitted event hash.\n  Each output flag conflicts with --json; default JSON is unchanged.\n\nOptional surfaces\n  Advanced commands: bind, doctor, plan, verify, profile, migrate, memory (resolve/inspect/lifecycle), away, host, hooks, hook-protocol, hook-run, version.\n  Direct small task: {command} activity codex < PROMPT; inspect the private record with activity show ID.\n  '{command} host status' reports the managed bootstrap skill installed for each supported host; '{command} host install' reinstalls or refreshes it. Installation and update manage it for you.\n  Run value proof: the host executes the configured check, then reports its actual result with 'run record-check'; use 'run status', 'run explain', and 'run report' for bounded evidence views.\n  Run '{command} migrate layout --config CONFIG' to inspect a legacy profile migration, then repeat with --apply. Use 'migrate paths' for canonical harness and state directories.\n  Use '{command} run supersede OLD_LEDGER --workflow WORKFLOW --goal GOAL --ledger NEW_LEDGER' only when you want a successor bound to changed configuration, profile, memory, boundary, or harness inputs."
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
    println!("Run '{command} version --json' for the build commit and executable SHA-256 to compare with release evidence.");
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
