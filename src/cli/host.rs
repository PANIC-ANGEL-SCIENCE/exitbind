// Presents host bridge status and installation commands.
// Refuses unknown host actions; see `src/cli/mod.rs`.
//! Host bridge status and installation CLI presentation.

use super::{args, positional, print_json, Arguments, HOST_ACTIONS};
use serde_json::json;

pub(super) fn command(a: &Arguments) -> Result<(), String> {
    args::assert_options("host", a, &["hosts", "all", "json"])?;
    args::assert_positionals("host", a, 1)?;
    let action = positional(a, 0, "host requires one action: status or install")?;
    if !HOST_ACTIONS.contains(&action) {
        return Err(format!(
            "host requires one action: {}",
            HOST_ACTIONS.join(" or ")
        ));
    }
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
                        "\n{}\n  host present: {}\n  bootstrap skill: {}{}\n  competence reference: {} ({})\n  activation hook: {}\n  path: {}",
                        item["host"].as_str().unwrap_or_default(),
                        item["hostPresent"],
                        item["bootstrapSkill"].as_str().unwrap_or_default(),
                        item["installedVersion"]
                            .as_str()
                            .map(|version| format!(" (installed {version})"))
                            .unwrap_or_default(),
                        item["competenceReference"].as_str().unwrap_or_default(),
                        item["competenceReferencePath"].as_str().unwrap_or_default(),
                        crate::host::bridge::activation_hook_text(&item),
                        item["path"].as_str().unwrap_or_default()
                    );
                    if let Some(references) = item["referenceAssets"].as_array() {
                        for reference in references.iter().skip(1) {
                            let label = if reference["name"]
                                .as_str()
                                .unwrap_or_default()
                                .starts_with("resources/")
                            {
                                "resource"
                            } else {
                                "role reference"
                            };
                            println!(
                                "  {label}: {} ({})",
                                reference["state"].as_str().unwrap_or_default(),
                                reference["path"].as_str().unwrap_or_default()
                            );
                        }
                    }
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
                    println!(
                        "{}: competence reference {} ({})",
                        item["host"].as_str().unwrap_or_default(),
                        item["competenceReferenceAction"]
                            .as_str()
                            .unwrap_or_default(),
                        item["competenceReferencePath"].as_str().unwrap_or_default()
                    );
                    if let Some(references) = item["referenceAssets"].as_array() {
                        for reference in references.iter().skip(1) {
                            let label = if reference["name"]
                                .as_str()
                                .unwrap_or_default()
                                .starts_with("resources/")
                            {
                                "resource"
                            } else {
                                "role reference"
                            };
                            println!(
                                "{}: {label} {} ({})",
                                item["host"].as_str().unwrap_or_default(),
                                reference["action"].as_str().unwrap_or_default(),
                                reference["path"].as_str().unwrap_or_default()
                            );
                        }
                    }
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
