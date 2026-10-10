mod cli;
mod compatibility;
mod config;
mod context;
mod distribution;
mod envelope;
mod evidence;
mod host;
mod kernel;
mod memory;
mod presentation;
mod presentation_events;
mod producer;
mod project;
mod run;
mod run_exit;
mod run_human;
mod run_presentation;
mod run_progress;
mod run_value;
mod session_goal;
mod value_benchmark;
mod work;

fn main() {
    let argv0 = std::env::args_os().next();
    let current_exe = std::env::current_exe().ok();
    if crate::compatibility::invoked_as_retired_soulmate(
        argv0.as_deref(),
        current_exe.as_deref().map(|path| path.as_os_str()),
    ) {
        eprintln!(
            "Soulmate is no longer supported; run Exitbind from its current executable name."
        );
        std::process::exit(2);
    }
    let raw = std::env::args_os().skip(1).collect::<Vec<_>>();
    let json_output = raw.iter().any(|argument| argument == "--json");
    let raw_for_error = raw.clone();
    let arguments = raw
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "arguments must be valid UTF-8".to_owned())
        })
        .collect::<Result<Vec<_>, _>>();
    let result = arguments.and_then(cli::run);
    if let Err(error) = result {
        if let Some(machine) = error
            .strip_prefix("EXITBIND_JSON:")
            .or_else(|| error.strip_prefix("SOULMATE_JSON:"))
        {
            println!("{machine}");
            emit_work_progress(&raw_for_error, machine);
        } else if let Some(machine) = error.strip_prefix(crate::work::DIAGNOSTIC_PREFIX) {
            if json_output {
                println!("{machine}");
            } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(machine) {
                let name = std::env::args_os()
                    .next()
                    .and_then(|x| x.into_string().ok())
                    .and_then(|x| {
                        std::path::Path::new(&x)
                            .file_name()
                            .and_then(|x| x.to_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "exitbind".into());
                eprintln!(
                    "{}: {}",
                    name,
                    value["error"].as_str().unwrap_or("work discovery failed")
                );
            } else {
                eprintln!("{machine}");
            }
        } else if json_output {
            println!("{}", serde_json::json!({ "error": error }));
        } else {
            let name = std::env::args_os()
                .next()
                .and_then(|x| x.into_string().ok())
                .and_then(|x| {
                    std::path::Path::new(&x)
                        .file_name()
                        .and_then(|x| x.to_str())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "exitbind".into());
            eprintln!("{name}: {error}");
        }
        std::process::exit(1);
    }
}

/// Keep the machine error contract on stdout while surfacing the same
/// product-owned progress line as successful Work mutations. This is a
/// read-only best effort: a malformed error, missing Work, or unavailable
/// config produces no additional human text.
fn emit_work_progress(raw: &[std::ffi::OsString], machine: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(machine) else {
        return;
    };
    let Some(work) = value["work"].as_str() else {
        return;
    };
    let config = raw.iter().enumerate().find_map(|(index, argument)| {
        let argument = argument.to_str()?;
        if let Some(value) = argument.strip_prefix("--config=") {
            return Some(value.to_owned());
        }
        (argument == "--config")
            .then(|| raw.get(index + 1))
            .flatten()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
    });
    let Ok(loaded) = crate::config::load(config.as_deref()) else {
        return;
    };
    let result = value
        .get("next")
        .and_then(|next| next.get("progress"))
        .unwrap_or(&value);
    let Ok(progress) = crate::session_goal::progress_for_work(&loaded, work, result) else {
        return;
    };
    let Some(text) = progress["systemText"].as_str() else {
        return;
    };
    let mut line = text.to_owned();
    if let Some(repair) = value["next"]["current"]["repair"].as_object() {
        let reason = repair["reason"].as_str().unwrap_or("repair required");
        let outcome = repair["outcome"].as_str().unwrap_or("rework");
        line.push_str(" | Repair: ");
        line.push_str(&crate::run_presentation::inert(reason));
        line.push_str(" (");
        line.push_str(&crate::run_presentation::inert(outcome));
        line.push(')');
    }
    if let Some(action) = value["next"]["action"]
        .as_str()
        .filter(|action| !matches!(*action, "done" | "unavailable"))
    {
        line.push_str(" | Next: ");
        line.push_str(&crate::run_presentation::inert(action));
    }
    eprintln!("{line}");
}
