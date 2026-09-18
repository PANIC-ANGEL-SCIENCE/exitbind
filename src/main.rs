mod args;
mod away;
mod boundary_manifest;
mod cli;
mod compatibility;
mod config;
mod config_types;
mod envelope;
mod forgetting;
mod git_preflight;
mod harness_manifest;
mod hash;
mod hook_runtime;
mod hook_settings;
mod hooks;
mod host_bridge;
mod layout_migration;
mod managed_files;
mod memory;
mod memory_discovery;
mod memory_policy;
mod memory_selection;
mod onboarding;
mod presentation;
mod producer;
mod profile;
mod project_commands;
mod project_layout;
mod project_path;
mod project_skills;
mod receipt;
mod run;
mod run_artifact;
mod run_assignment;
mod run_error;
mod run_exit;
mod run_human;
mod run_inputs;
mod run_ledger;
mod run_presentation;
mod run_progress;
mod run_state;
mod run_value;
mod update;
mod value_benchmark;
mod work;
mod work_packet;

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
