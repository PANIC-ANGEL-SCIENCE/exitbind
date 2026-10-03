//! Explicit, project-owned native agent projections. Existing external files
//! are reported and left alone, so dotagents and user-managed profiles retain ownership.

use crate::{config::Loaded, evidence::hash, host::settings, project::managed_files};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

const MARKER: &str = "exitbind-managed-agent:v1";

struct Projection {
    agent: String,
    host: &'static str,
    path: PathBuf,
    content: String,
    source_sha256: String,
}

fn projections(loaded: &Loaded, hosts: &[&str]) -> Result<Vec<Projection>, String> {
    let mut result = Vec::new();
    for (id, agent) in &loaded.agents {
        let native = agent.native_name(id);
        if native.is_empty()
            || native.len() > 64
            || !native.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
            })
        {
            return Err(format!("native agent name is unsafe for a file: {native}"));
        }
        let bytes = crate::project::path::secure_bytes(
            &loaded.control_root,
            &agent.profile,
            "agent profile",
        )?;
        let source = String::from_utf8(bytes.clone())
            .map_err(|_| format!("agent profile is not UTF-8: {}", agent.profile))?;
        let sha = hash::bytes(&bytes);
        let purpose = serde_json::to_string(&agent.purpose).map_err(|e| e.to_string())?;
        let name = serde_json::to_string(&native).map_err(|e| e.to_string())?;
        let instructions = serde_json::to_string(&source).map_err(|e| e.to_string())?;
        if hosts.contains(&"codex") {
            result.push(Projection {
                agent:id.clone(), host:"codex",
                path:loaded.product_root.join(format!(".codex/agents/{native}.toml")),
                content:format!("# {MARKER}\nname = {name}\ndescription = {purpose}\ndeveloper_instructions = {instructions}\n"),
                source_sha256:sha.clone(),
            });
        }
        if hosts.contains(&"claude") {
            result.push(Projection {
                agent: id.clone(),
                host: "claude",
                path: loaded
                    .product_root
                    .join(format!(".claude/agents/{native}.md")),
                content: format!(
                    "---\nname: {name}\ndescription: {purpose}\n---\n<!-- {MARKER} -->\n{source}"
                ),
                source_sha256: sha,
            });
        }
    }
    Ok(result)
}

fn state(item: &Projection) -> Result<(&'static str, Option<String>, Option<String>), String> {
    let info = match fs::symlink_metadata(&item.path) {
        Ok(info) => info,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(("missing", None, None))
        }
        Err(error) => return Err(error.to_string()),
    };
    if info.file_type().is_symlink() || !info.is_file() {
        return Ok(("unsafe", None, None));
    }
    let bytes = fs::read(&item.path).map_err(|e| e.to_string())?;
    let sha = hash::bytes(&bytes);
    let Ok(source) = String::from_utf8(bytes) else {
        return Ok(("external", Some(sha), None));
    };
    if source == item.content {
        return Ok(("current", Some(sha), Some(source)));
    }
    let managed = if item.host == "codex" {
        source.starts_with(&format!("# {MARKER}\n"))
    } else {
        let marker = format!("<!-- {MARKER} -->");
        source.lines().nth(4) == Some(marker.as_str())
    };
    if managed {
        Ok(("stale_managed", Some(sha), Some(source)))
    } else {
        Ok(("external", Some(sha), Some(source)))
    }
}

pub(crate) fn status(loaded: &Loaded) -> Result<Value, String> {
    status_for_hosts(loaded, &["codex", "claude"])
}

pub(crate) fn status_for_hosts(loaded: &Loaded, hosts: &[&str]) -> Result<Value, String> {
    let mut items = Vec::new();
    for item in projections(loaded, hosts)? {
        let (state, observed_sha256, _) = state(&item)?;
        let relative = item
            .path
            .strip_prefix(&loaded.product_root)
            .map_err(|_| "projection path escaped product root")?
            .to_str()
            .ok_or("projection path is not UTF-8")?;
        items.push(
            json!({"agentId":item.agent,"host":item.host,"path":relative,
            "state":state,"sourceSha256":item.source_sha256,
            "expectedSha256":hash::text(&item.content),"observedSha256":observed_sha256}),
        );
    }
    Ok(json!({"version":1,"projections":items,
        "evidence":"source-to-artifact only; native discovery and launch are separate"}))
}

pub(crate) fn apply(loaded: &Loaded) -> Result<Value, String> {
    apply_for_hosts(loaded, &["codex", "claude"])
}

pub(crate) fn apply_for_hosts(loaded: &Loaded, hosts: &[&str]) -> Result<Value, String> {
    let items = projections(loaded, hosts)?;
    let states = items.iter().map(state).collect::<Result<Vec<_>, _>>()?;
    if items
        .iter()
        .zip(&states)
        .any(|(_, (state, _, _))| matches!(*state, "unsafe" | "external"))
    {
        return Err(
            "native agent projection conflicts with an external or unsafe file; no files changed"
                .into(),
        );
    }
    let root = fs::canonicalize(&loaded.product_root).map_err(|e| e.to_string())?;
    for (item, (state, _, observed_source)) in items.iter().zip(&states) {
        if *state == "current" {
            continue;
        }
        let parent = item.path.parent().ok_or("native profile has no parent")?;
        managed_files::ensure_managed_directory(&loaded.product_root, parent)?;
        settings::atomic_write(
            &item.path,
            &item.content,
            None,
            observed_source.as_deref(),
            &root,
        )?;
    }
    status_for_hosts(loaded, hosts)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn an_external_replacement_after_preflight_cannot_become_the_expected_source() {
        let root = std::env::temp_dir().join(format!(
            "exitbind-native-projection-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(root.join(".codex/agents")).unwrap();
        let item = Projection {
            agent: "worker".into(),
            host: "codex",
            path: root.join(".codex/agents/worker.toml"),
            content: format!("# {MARKER}\nnew profile\n"),
            source_sha256: String::new(),
        };
        fs::write(&item.path, format!("# {MARKER}\nold profile\n")).unwrap();
        let (kind, _, observed_source) = state(&item).unwrap();
        assert_eq!(kind, "stale_managed");
        fs::write(&item.path, "# generated by dotagents\nexternal profile\n").unwrap();
        let result = settings::atomic_write(
            &item.path,
            &item.content,
            None,
            observed_source.as_deref(),
            &fs::canonicalize(&root).unwrap(),
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(&item.path).unwrap(),
            "# generated by dotagents\nexternal profile\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
