//! Native agent projection preflight for `setup`: refuse unsafe or external
//! projection files before init, and read projection status for the report.

use serde_json::Value;
use std::{fs, path::Path};

pub(super) fn preflight_new_projections(root: &Path, hosts: &[String]) -> Result<(), String> {
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

pub(super) fn has_projection_conflict(value: &Value) -> bool {
    value["projections"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| matches!(item["state"].as_str(), Some("unsafe" | "external")))
    })
}

pub(super) fn projection_needs_write(value: &Value) -> bool {
    value["projections"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| !matches!(item["state"].as_str(), Some("current")))
    })
}
