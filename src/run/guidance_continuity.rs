//! Versioned separation between verification inputs and launch-only guidance.
//!
//! The marker is opt-in on new start events. Absence intentionally selects the
//! historical exact-currentness path.

use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) const PROTOCOL: u64 = 1;
pub(crate) const START_FIELD: &str = "guidanceContinuityProtocol";
pub(crate) const CONFIG_FIELD: &str = "guidanceConfigSha256";

pub(crate) fn enabled(state: &Value) -> bool {
    state[START_FIELD].as_u64() == Some(PROTOCOL)
}

pub(crate) fn add_start_marker(event: &mut Value, loaded: &crate::config::Loaded) {
    event[START_FIELD] = json!(PROTOCOL);
    event[CONFIG_FIELD] = json!(advisory_config_sha256(&loaded.config));
}

pub(crate) fn append_allowed_fields(fields: &mut Vec<&'static str>) {
    fields.push(START_FIELD);
    fields.push(CONFIG_FIELD);
}

pub(crate) fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    let Some(marker) = event.get(START_FIELD) else {
        if event.get(CONFIG_FIELD).is_some() {
            return Err(format!(
                "invalid run ledger line {line}: guidance configuration hash requires its protocol marker"
            ));
        }
        return Ok(());
    };
    let config_hash = event[CONFIG_FIELD].as_str().unwrap_or_default();
    if event["version"] != 8 || marker.as_u64() != Some(PROTOCOL) || !valid_sha(config_hash) {
        return Err(format!(
            "invalid run ledger line {line}: unsupported guidance continuity protocol"
        ));
    }
    Ok(())
}

/// Hash the validated configuration after removing advisory content and
/// selection fields. MemoryRead and crossContext are enforced independently by
/// the frozen recipient ceiling intersected with current live rights. Execution,
/// workflow, sandbox, review, checker and source fields remain strict.
pub(crate) fn advisory_config_sha256(config: &Value) -> String {
    let mut projection = config.clone();
    if let Some(agents) = projection["agents"].as_object_mut() {
        for agent in agents.values_mut() {
            if let Some(agent) = agent.as_object_mut() {
                agent.remove("purpose");
                agent.remove("displayName");
                agent.remove("skills");
                agent.remove("memoryRead");
                agent.remove("crossContext");
            }
        }
    }
    crate::evidence::hash::value(&projection)
}

pub(crate) fn assert_config_current(
    loaded: &crate::config::Loaded,
    state: &Value,
) -> Result<(), String> {
    let matches = if enabled(state) {
        state[CONFIG_FIELD].as_str() == Some(advisory_config_sha256(&loaded.config).as_str())
    } else {
        state["configSha256"].as_str() == Some(crate::evidence::hash::text(&loaded.source).as_str())
    };
    if !matches {
        return Err("project configuration changed outside the marked advisory fields".into());
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Exact memory source and selection-ledger paths frozen into the run. Only
/// these validated paths can be omitted from the tested-input fingerprint.
pub(crate) fn selected_source_paths(state: &Value) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    if !enabled(state) {
        return paths;
    }
    let Some(stages) = state["plan"]["stages"].as_array() else {
        return paths;
    };
    for stage in stages {
        for agent in stage["agents"].as_array().into_iter().flatten() {
            for reference in agent["memoryReferences"].as_array().into_iter().flatten() {
                for field in ["sourcePath", "ledgerPath"] {
                    if let Some(path) = reference[field].as_str() {
                        if safe_relative(path) {
                            paths.insert(path.to_owned());
                        }
                    }
                }
            }
        }
    }
    paths
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Keep any guidance file strict when it aliases a frozen checker or an
/// architecture contract source. Unknown checker dependencies are not guessed.
pub(crate) fn unambiguous_paths(loaded: &crate::config::Loaded, state: &Value) -> BTreeSet<String> {
    let mut candidates = selected_source_paths(state);
    if !enabled(state) {
        return BTreeSet::new();
    }
    if has_unresolved_dynamic_checker_dependency(state) {
        return BTreeSet::new();
    }
    add_product_path(&loaded.product_root, &loaded.path, &mut candidates);
    if let Some(stages) = state["plan"]["stages"].as_array() {
        for agent in stages
            .iter()
            .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
        {
            let Some(name) = agent["name"].as_str() else {
                continue;
            };
            let Some(configured) = loaded.agent(name) else {
                continue;
            };
            if let Ok(path) = crate::config::file(&loaded.control_root, &configured.profile) {
                add_product_path(&loaded.product_root, &path, &mut candidates);
            }
            let Some(scope_ceiling) = agent["declaredBoundary"]["memoryRead"].as_array() else {
                return BTreeSet::new();
            };
            let Some(scope_ceiling) = scope_ceiling
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
            else {
                return BTreeSet::new();
            };
            let Some(cross_context) = agent["declaredBoundary"]["crossContext"].as_str() else {
                return BTreeSet::new();
            };
            let Some(task) = state["goal"].as_str() else {
                return BTreeSet::new();
            };
            let Ok(current) = crate::memory::selection::resolve_for_task_optional_with_ceiling(
                loaded,
                name,
                Some(task),
                &scope_ceiling,
                cross_context,
            ) else {
                return BTreeSet::new();
            };
            for reference in current.references {
                for field in ["sourcePath", "ledgerPath"] {
                    if let Some(path) = reference[field].as_str() {
                        if safe_relative(path) {
                            candidates.insert(path.to_owned());
                        }
                    }
                }
            }
        }
    }
    // Harness receipts retain their own exact product binding. Keep every
    // input strict for them because a path may be evidence even when it is not
    // named by the selected verification recipe.
    if state.get("harnessReceipt").is_some() {
        return BTreeSet::new();
    }
    let mut strict = BTreeSet::new();
    if state.get("checkPolicy").is_some() && state.get("verificationPreparationProtocol").is_none()
    {
        // Without a frozen source inventory we cannot distinguish launch
        // guidance from a checker dependency.
        return BTreeSet::new();
    }
    if let Some(sources) = state["verificationCheckerSources"].as_array() {
        for source in sources {
            for field in ["path", "argumentPath"] {
                if let Some(path) = source[field].as_str() {
                    if let Ok(relative) =
                        std::path::Path::new(path).strip_prefix(&loaded.product_root)
                    {
                        if let Some(relative) = relative.to_str() {
                            strict.insert(relative.replace('\\', "/"));
                        }
                    }
                }
            }
        }
    } else if state.get("checkPolicy").is_some() {
        // A checked run without a frozen source list cannot safely distinguish
        // dynamic checker dependencies from optional guidance.
        return BTreeSet::new();
    }
    if let Some(path) = state["plan"]["architectureContract"]["sourcePath"].as_str() {
        strict.insert(path.to_owned());
    }
    candidates.retain(|path| !strict.contains(path));
    candidates
}

fn has_unresolved_dynamic_checker_dependency(state: &Value) -> bool {
    fn dynamic(command: &str) -> bool {
        command
            .chars()
            .any(|character| matches!(character, '$' | '*' | '?' | '`' | '[' | ']'))
    }

    if state.get("checkPolicy").is_some() {
        let Some(command) = state["checkPolicy"]["command"].as_str() else {
            return true;
        };
        if dynamic(command) {
            return true;
        }
    }
    if let Some(preservation) = state.get("preservation") {
        let Some(requirements) = preservation["requirements"].as_array() else {
            return true;
        };
        if requirements
            .iter()
            .any(|requirement| requirement["command"].as_str().map_or(true, dynamic))
        {
            return true;
        }
    }
    false
}

fn add_product_path(root: &std::path::Path, path: &std::path::Path, paths: &mut BTreeSet<String>) {
    if let Ok(path) = path.canonicalize() {
        if let Ok(relative) = path.strip_prefix(root) {
            if let Some(relative) = relative.to_str() {
                let value = relative.replace('\\', "/");
                if safe_relative(&value) {
                    paths.insert(value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Loaded;
    use serde_json::json;
    use std::{collections::BTreeMap, path::PathBuf};

    fn loaded(config: Value) -> Loaded {
        let source = serde_json::to_string(&config).unwrap();
        Loaded {
            config,
            agents: BTreeMap::new(),
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            workflows: BTreeMap::new(),
            verification_preparation: None,
            path: PathBuf::from("/control/exitbind.json"),
            control_root: PathBuf::from("/control"),
            product_root: PathBuf::from("/product"),
            state_root: PathBuf::from("/state"),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source,
        }
    }

    fn config() -> Value {
        json!({
            "agents": {"worker": {
                "profile":"agents/worker.md", "purpose":"implement one task",
                "displayName":"Worker", "runtime":{"host":"codex"},
                "memoryRead":["task"]
            }},
            "project":{"root":".","verificationPreparation":{
                "version":1,"pythonBytecode":"disabled","scratch":"taskOwned"
            }},
            "workflows":{"change":{"workers":["worker"]}}
        })
    }

    #[test]
    fn marked_configuration_currentness_ignores_only_presentation_fields() {
        let original = config();
        let baseline = advisory_config_sha256(&original);
        let original_loaded = loaded(original.clone());
        let state = json!({"guidanceContinuityProtocol":PROTOCOL,
            "guidanceConfigSha256":baseline,"configSha256":"frozen"});

        let mut advisory = original.clone();
        advisory["agents"]["worker"]["purpose"] = json!("updated task guidance");
        advisory["agents"]["worker"]["displayName"] = json!("Build Agent");
        assert!(assert_config_current(&loaded(advisory), &state).is_ok());
        let legacy = json!({"configSha256":crate::evidence::hash::text(&original_loaded.source)});
        assert!(assert_config_current(&original_loaded, &legacy).is_ok());
        let mut changed_legacy = original.clone();
        changed_legacy["agents"]["worker"]["purpose"] = json!("updated task guidance");
        assert!(assert_config_current(&loaded(changed_legacy), &legacy).is_err());

        for (path, value) in [
            ("profile", json!("agents/new.md")),
            ("runtime", json!({"host":"claude"})),
        ] {
            let mut changed = original.clone();
            changed["agents"]["worker"][path] = value;
            assert!(
                assert_config_current(&loaded(changed), &state).is_err(),
                "{path}"
            );
        }
        let mut changed_sandbox = original.clone();
        changed_sandbox["project"]["verificationPreparation"]["scratch"] =
            json!("unownedSharedScratch");
        assert!(
            assert_config_current(&loaded(changed_sandbox), &state).is_err(),
            "verification preparation is part of strict execution identity"
        );
        let mut current_rights = original.clone();
        current_rights["agents"]["worker"]["memoryRead"] = json!([]);
        current_rights["agents"]["worker"]["crossContext"] = json!("none");
        current_rights["agents"]["worker"]["skills"] = json!(["review"]);
        assert!(assert_config_current(&loaded(current_rights), &state).is_ok());
    }

    #[test]
    fn only_unambiguous_selected_guidance_paths_leave_the_input_fingerprint() {
        let loaded = loaded(config());
        let mut state = json!({
            "guidanceContinuityProtocol":PROTOCOL,
            "plan":{"stages":[{"agents":[{"memoryReferences":[
                {"sourcePath":"memory/selected.md","ledgerPath":"memory/selected.jsonl"},
                {"sourcePath":"memory/checker.md"},
                {"sourcePath":"memory/architecture.md"}
            ]}]}],
                "architectureContract":{"sourcePath":"memory/architecture.md"}},
            "verificationPreparationProtocol":1,
            "checkPolicy":{"command":"cargo test"},
            "verificationCheckerSources":[{"path":"/product/memory/checker.md"}]
        });
        assert_eq!(
            unambiguous_paths(&loaded, &state),
            ["memory/selected.md", "memory/selected.jsonl"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );

        state["harnessReceipt"] = json!({"sha256":"a".repeat(64)});
        assert!(unambiguous_paths(&loaded, &state).is_empty());
        state.as_object_mut().unwrap().remove("harnessReceipt");
        state["verificationCheckerSources"] = Value::Null;
        assert!(unambiguous_paths(&loaded, &state).is_empty());
    }

    #[test]
    fn unresolved_dynamic_checker_dependencies_keep_selected_guidance_strict() {
        let loaded = loaded(config());
        for command in [
            "python3 checks/check.py \"$ADVICE_PATH\"",
            "python3 checks/check.py memory/*.md",
        ] {
            let state = json!({
                "guidanceContinuityProtocol":PROTOCOL,
                "plan":{"stages":[{"agents":[{"memoryReferences":[
                    {"sourcePath":"memory/advice.md","ledgerPath":"memory/advice.jsonl"}
                ]}]}]},
                "verificationPreparationProtocol":1,
                "checkPolicy":{"command":command},
                "verificationCheckerSources":[{
                    "path":"/product/checks/check.py",
                    "argumentPath":"checks/check.py",
                    "sha256":"a".repeat(64)
                }]
            });

            assert!(
                unambiguous_paths(&loaded, &state).is_empty(),
                "dynamic checker dependency must keep selected guidance strict: {command}"
            );
        }

        let preservation = json!({
            "guidanceContinuityProtocol":PROTOCOL,
            "plan":{"stages":[{"agents":[{"memoryReferences":[
                {"sourcePath":"memory/advice.md","ledgerPath":"memory/advice.jsonl"}
            ]}]}]},
            "verificationPreparationProtocol":1,
            "checkPolicy":{"command":"python3 checks/check.py"},
            "preservation":{"requirements":[{"command":"python3 checks/check.py \"$ADVICE_PATH\""}]},
            "verificationCheckerSources":[{
                "path":"/product/checks/check.py",
                "argumentPath":"checks/check.py",
                "sha256":"a".repeat(64)
            }]
        });
        assert!(
            unambiguous_paths(&loaded, &preservation).is_empty(),
            "dynamic preservation checker dependency must keep selected guidance strict"
        );
    }
}
