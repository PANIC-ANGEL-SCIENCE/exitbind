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
    let raw = std::env::args_os().skip(1).collect::<Vec<_>>();
    let json_output = raw.iter().any(|argument| argument == "--json");
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
