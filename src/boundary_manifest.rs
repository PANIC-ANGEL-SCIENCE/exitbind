//! Run-scoped filesystem boundaries narrowed from each configured agent maximum.

use crate::{config::Loaded, hash, project_path, run_error};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub(crate) fn apply(
    loaded: &Loaded,
    mut plan: Value,
    requested: Option<&str>,
) -> Result<Value, String> {
    let Some(requested) = requested else {
        return Ok(plan);
    };
    let relative = normalized_relative(requested, "boundary manifest")?;
    let bytes =
        project_path::secure_bytes(&loaded.control_root, &relative, "run boundary manifest")?;
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid run boundary manifest JSON: {error}"))?;
    let agents = validate_manifest(loaded, &manifest)?;
    let selected = selected_agents(&plan);
    for name in agents.keys() {
        if !selected.contains(name.as_str()) {
            return Err(format!(
                "run boundary manifest agent '{name}' is not selected by this workflow"
            ));
        }
    }
    let stages = plan
        .get_mut("stages")
        .and_then(Value::as_array_mut)
        .ok_or("workflow plan stages are missing")?;
    for stage in stages {
        let agents_in_stage = stage
            .get_mut("agents")
            .and_then(Value::as_array_mut)
            .ok_or("workflow stage agents are missing")?;
        for agent in agents_in_stage {
            let name = agent["name"].as_str().unwrap_or_default().to_owned();
            let Some(boundary) = agents.get(&name) else {
                continue;
            };
            let configured = loaded
                .agent(&name)
                .ok_or_else(|| format!("workflow plan references unknown agent '{name}'"))?;
            for (field, maxima) in [
                ("observe", configured.observe.as_slice()),
                ("write", configured.write.as_slice()),
            ] {
                let exact = exact_paths(&boundary[field], field, &loaded.product_root)?;
                assert_narrowed(&name, field, &exact, maxima)?;
                agent["declaredBoundary"][field] = json!(exact);
            }
        }
    }
    plan["boundaryManifest"] = json!({
        "path": relative,
        "sha256": hash::bytes(&bytes),
    });
    Ok(plan)
}

pub(crate) fn assert_current(loaded: &Loaded, plan: &Value) -> Result<(), String> {
    let Some(evidence) = plan.get("boundaryManifest") else {
        return Ok(());
    };
    let expected = evidence["sha256"].as_str().unwrap_or_default();
    let path = evidence["path"]
        .as_str()
        .ok_or("recorded run boundary manifest path is invalid")?;
    let bytes = project_path::secure_bytes(&loaded.control_root, path, "run boundary manifest")
        .map_err(|error| {
            format!(
                "run boundary manifest is missing or unreadable: {error}; restore the exact file or use the run supersede command"
            )
        })?;
    let current = hash::bytes(&bytes);
    if current != expected {
        return Err(run_error::machine_drift(run_error::DriftError::boundary(
            expected.to_owned(),
            current,
        )));
    }
    Ok(())
}

pub(crate) fn warnings(config: &Value) -> Vec<Value> {
    let mut warnings = Vec::new();
    let Some(agents) = config["agents"].as_object() else {
        return warnings;
    };
    for (name, agent) in agents {
        for field in ["observe", "write"] {
            for entry in agent[field].as_array().into_iter().flatten() {
                let Some(entry) = entry.as_str() else {
                    continue;
                };
                if !valid_maximum(entry) || entry.chars().any(char::is_whitespace) {
                    warnings.push(json!({
                        "classification": "boundary_placeholder",
                        "agent": name,
                        "field": field,
                        "entry": entry,
                        "detail": "entry is descriptive or unsupported; use an exact relative path, prefix/**, or ** before applying a run boundary"
                    }));
                }
            }
        }
    }
    warnings
}

pub(crate) fn validate_evidence(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == 2
        && normalized_relative(
            value["path"].as_str().unwrap_or_default(),
            "boundary manifest",
        )
        .is_ok()
        && is_sha(value["sha256"].as_str())
}

fn validate_manifest<'a>(
    loaded: &Loaded,
    manifest: &'a Value,
) -> Result<&'a Map<String, Value>, String> {
    let object = manifest
        .as_object()
        .ok_or("run boundary manifest must be an object")?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "$schema" | "version" | "agents"))
        || manifest["version"] != 1
        || !manifest["agents"].is_object()
        || manifest
            .get("$schema")
            .is_some_and(|value| !value.is_string())
    {
        return Err("run boundary manifest permits only $schema, version 1, and agents".into());
    }
    let agents = manifest["agents"]
        .as_object()
        .ok_or("run boundary manifest agents must be an object")?;
    if agents.is_empty() {
        return Err("run boundary manifest agents must not be empty".into());
    }
    for (name, boundary) in agents {
        if loaded.agent(name).is_none() {
            return Err(format!(
                "run boundary manifest references unknown agent '{name}'"
            ));
        }
        let fields = boundary
            .as_object()
            .ok_or_else(|| format!("run boundary for '{name}' must be an object"))?;
        if fields.len() != 2 || !fields.contains_key("observe") || !fields.contains_key("write") {
            return Err(format!(
                "run boundary for '{name}' requires only observe and write"
            ));
        }
        if !boundary["observe"].is_array() || !boundary["write"].is_array() {
            return Err(format!("run boundary for '{name}' must use path arrays"));
        }
    }
    Ok(agents)
}

fn exact_paths(value: &Value, field: &str, root: &Path) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    let values = value
        .as_array()
        .ok_or_else(|| format!("run boundary {field} must use a path array"))?;
    for value in values {
        let raw = value
            .as_str()
            .ok_or_else(|| format!("run boundary {field} entries must be strings"))?;
        let path = normalized_relative(raw, &format!("run boundary {field}"))?;
        if path.contains('*') || path.contains('?') || path.contains('[') || path.contains(']') {
            return Err(format!("run boundary {field} must use exact paths: {raw}"));
        }
        check_product_path(root, &path, field == "observe")?;
        if !seen.insert(path.clone()) {
            return Err(format!(
                "run boundary {field} contains duplicate path '{path}'"
            ));
        }
        paths.push(path);
    }
    Ok(paths)
}

fn assert_narrowed(
    agent: &str,
    field: &str,
    exact: &[String],
    configured: &[String],
) -> Result<(), String> {
    for path in exact {
        let allowed = configured
            .iter()
            .any(|maximum| maximum_contains(maximum, path));
        if !allowed {
            return Err(format!(
                "run boundary widens agents.{agent}.{field} with '{path}'"
            ));
        }
    }
    Ok(())
}

fn maximum_contains(maximum: &str, path: &str) -> bool {
    if maximum == "**" {
        return true;
    }
    if let Some(prefix) = maximum.strip_suffix("/**") {
        return normalized_relative(prefix, "configured boundary")
            .is_ok_and(|prefix| path == prefix || path.starts_with(&format!("{prefix}/")));
    }
    normalized_relative(maximum, "configured boundary").is_ok_and(|maximum| maximum == path)
}

fn valid_maximum(value: &str) -> bool {
    value == "**"
        || value.strip_suffix("/**").map_or_else(
            || {
                !value.chars().any(|character| "*?[]".contains(character))
                    && normalized_relative(value, "configured boundary").is_ok()
            },
            |prefix| {
                !prefix.chars().any(|character| "*?[]".contains(character))
                    && normalized_relative(prefix, "configured boundary").is_ok()
            },
        )
}

fn normalized_relative(value: &str, label: &str) -> Result<String, String> {
    if value.trim().is_empty()
        || value != value.trim()
        || value.contains('\0')
        || value.contains('\\')
        || Path::new(value).is_absolute()
    {
        return Err(format!("{label} must use a normalized relative path"));
    }
    let mut parts = Vec::new();
    for component in Path::new(value).components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| format!("{label} must use a UTF-8 relative path"))?
                    .to_owned(),
            ),
            _ => return Err(format!("{label} must use a normalized relative path")),
        }
    }
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(format!("{label} must use a normalized relative path"));
    }
    Ok(parts.join("/"))
}

fn check_product_path(root: &Path, relative: &str, require_existing: bool) -> Result<(), String> {
    let mut current = root.to_path_buf();
    let mut missing = false;
    for component in Path::new(relative).components() {
        current.push(component);
        if missing {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "run boundary path must not traverse a symlink: {relative}"
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing = true,
            Err(error) => return Err(error.to_string()),
        }
    }
    if require_existing && missing {
        return Err(format!(
            "run boundary observe path does not exist: {relative}"
        ));
    }
    Ok(())
}

fn selected_agents(plan: &Value) -> BTreeSet<&str> {
    plan["stages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
        .filter_map(|agent| agent["name"].as_str())
        .collect()
}

fn is_sha(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_and_maxima_are_normalized() {
        assert_eq!(
            normalized_relative("src/lib.rs", "path").unwrap(),
            "src/lib.rs"
        );
        for invalid in ["", "../src", "/src", "src\\lib.rs", " src/lib.rs"] {
            assert!(normalized_relative(invalid, "path").is_err(), "{invalid}");
        }
        assert!(valid_maximum("**"));
        assert!(valid_maximum("src/**"));
        assert!(valid_maximum("src/lib.rs"));
        assert!(!valid_maximum("src/*.rs"));
    }

    #[test]
    fn narrowing_accepts_only_configured_maxima() {
        let maxima = vec!["src/**".to_owned(), "Cargo.toml".to_owned()];
        assert!(assert_narrowed("worker", "write", &["src/lib.rs".into()], &maxima).is_ok());
        assert!(assert_narrowed("worker", "write", &["README.md".into()], &maxima).is_err());
    }

    #[test]
    fn exact_paths_reject_globs_duplicates_and_missing_observe_files() {
        let root =
            std::env::temp_dir().join(format!("soulmate-boundary-unit-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fixture\n").unwrap();
        assert_eq!(
            exact_paths(&json!(["src/lib.rs"]), "observe", &root).unwrap(),
            vec!["src/lib.rs"]
        );
        assert!(exact_paths(&json!(["src/*.rs"]), "observe", &root).is_err());
        assert!(exact_paths(&json!(["src/lib.rs", "src/lib.rs"]), "observe", &root).is_err());
        assert!(exact_paths(&json!(["src/missing.rs"]), "observe", &root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
