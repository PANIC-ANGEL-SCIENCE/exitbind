//! Versioned separation between verification inputs and launch-only guidance.
//!
//! The marker is opt-in on new start events. Absence intentionally selects the
//! historical exact-currentness path.

mod advisory_paths;

use self::advisory_paths::safe_relative;
#[cfg(test)]
use self::advisory_paths::tracked_paths;
pub(crate) use self::advisory_paths::unambiguous_paths;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) const PROTOCOL: u64 = 1;
pub(crate) const START_FIELD: &str = "guidanceContinuityProtocol";
pub(crate) const CONFIG_FIELD: &str = "guidanceConfigSha256";
pub(crate) const INPUT_CONTRACT_FIELD: &str = "verificationInputContract";

fn empty_input_contract() -> Value {
    json!({"version":1,"guidanceVerdictInputs":[]})
}

pub(crate) fn validate_input_contract(value: &Value, errors: &mut Vec<String>) {
    let Some(object) = value.as_object() else {
        errors.push("verificationInputContract must be an object".into());
        return;
    };
    if object.len() != 2 || value["version"] != 1 {
        errors.push(
            "verificationInputContract requires version 1 and guidanceVerdictInputs only".into(),
        );
        return;
    }
    let Some(paths) = value["guidanceVerdictInputs"].as_array() else {
        errors.push("verificationInputContract.guidanceVerdictInputs must be an array".into());
        return;
    };
    if paths.len() > 128 {
        errors.push("verificationInputContract has too many guidance verdict inputs".into());
    }
    let mut seen = BTreeSet::new();
    for item in paths {
        let Some(path) = item["path"].as_str() else {
            errors.push("verificationInputContract entries require a path".into());
            continue;
        };
        if item.as_object().map_or(true, |entry| entry.len() != 2)
            || item["projection"] != "file"
            || !safe_relative(path)
            || !seen.insert(path)
        {
            errors.push(
                "verificationInputContract entries require distinct safe paths and file projection"
                    .into(),
            );
        }
    }
}

pub(crate) fn enabled(state: &Value) -> bool {
    state[START_FIELD].as_u64() == Some(PROTOCOL)
}

pub(crate) fn add_start_marker(event: &mut Value, loaded: &crate::config::Loaded) {
    event[START_FIELD] = json!(PROTOCOL);
    event[CONFIG_FIELD] = json!(advisory_config_sha256(&loaded.config));
    event[INPUT_CONTRACT_FIELD] = loaded.config["project"][INPUT_CONTRACT_FIELD]
        .as_object()
        .map(|_| loaded.config["project"][INPUT_CONTRACT_FIELD].clone())
        .unwrap_or_else(empty_input_contract);
}

pub(crate) fn append_allowed_fields(fields: &mut Vec<&'static str>) {
    fields.push(START_FIELD);
    fields.push(CONFIG_FIELD);
    fields.push(INPUT_CONTRACT_FIELD);
}

pub(crate) fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    let Some(marker) = event.get(START_FIELD) else {
        if event.get(CONFIG_FIELD).is_some() || event.get(INPUT_CONTRACT_FIELD).is_some() {
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
    if let Some(contract) = event.get(INPUT_CONTRACT_FIELD) {
        let mut errors = Vec::new();
        validate_input_contract(contract, &mut errors);
        if !errors.is_empty() {
            return Err(format!(
                "invalid run ledger line {line}: malformed verification input contract"
            ));
        }
    }
    Ok(())
}

/// Hash the validated configuration after removing advisory content and
/// selection fields. MemoryRead and crossContext are enforced independently by
/// the frozen recipient ceiling intersected with current live rights. Execution,
/// workflow, sandbox, review, checker and source fields remain strict.
pub(crate) fn advisory_config_sha256(config: &Value) -> String {
    let mut projection = config.clone();
    let replaceable_advisers = adviser_only_agents(config);
    if let Some(agents) = projection["agents"].as_object_mut() {
        for (name, agent) in agents.iter_mut() {
            if let Some(agent) = agent.as_object_mut() {
                agent.remove("purpose");
                agent.remove("displayName");
                agent.remove("skills");
                agent.remove("memoryRead");
                agent.remove("crossContext");
                if replaceable_advisers.contains(name) {
                    if let Some(runtime) = agent.get_mut("runtime").and_then(Value::as_object_mut) {
                        for field in ["host", "model", "reasoningEffort"] {
                            runtime.remove(field);
                        }
                        if runtime.is_empty() {
                            agent.remove("runtime");
                        }
                    }
                }
            }
        }
    }
    crate::evidence::hash::value(&projection)
}

/// Runtime replacement is launch-only only for an agent assigned exclusively
/// as an adviser and only when the owner has supplied an explicit floor.
pub(crate) fn adviser_runtime_replaceable(config: &Value, name: &str) -> bool {
    adviser_only_agents(config).contains(name)
}

fn adviser_only_agents(config: &Value) -> BTreeSet<String> {
    if config["project"]["adviserFloor"]["version"].as_u64() != Some(1) {
        return BTreeSet::new();
    }
    let mut roles = std::collections::BTreeMap::<String, BTreeSet<&str>>::new();
    if let Some(lead) = config["orchestration"]["lead"].as_str() {
        roles.entry(lead.to_owned()).or_default().insert("lead");
    }
    if let Some(workflows) = config["workflows"].as_object() {
        for workflow in workflows.values() {
            for (role, field) in [
                ("adviser", "advisers"),
                ("worker", "workers"),
                ("reviewer", "reviewers"),
            ] {
                for name in workflow[field].as_array().into_iter().flatten() {
                    if let Some(name) = name.as_str() {
                        roles.entry(name.to_owned()).or_default().insert(role);
                    }
                }
            }
        }
    }
    roles
        .into_iter()
        .filter_map(|(name, roles)| (roles.len() == 1 && roles.contains("adviser")).then_some(name))
        .collect()
}

pub(crate) fn assert_config_current(
    loaded: &crate::config::Loaded,
    state: &Value,
) -> Result<(), String> {
    let (bound, current) = if enabled(state) {
        (
            state[CONFIG_FIELD].as_str(),
            advisory_config_sha256(&loaded.config),
        )
    } else {
        (
            state["configSha256"].as_str(),
            crate::evidence::hash::text(&loaded.source),
        )
    };
    if bound != Some(current.as_str()) {
        return Err(crate::drift::refusal(
            crate::drift::Class::GuidanceConfiguration,
            bound.unwrap_or_default(),
            Some(&current),
            None,
            crate::drift::Route::Supersede {
                ledger: None,
                config: Some(&loaded.path),
            },
        ));
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_memory_sources_are_reported_and_untracked_ones_are_not() {
        let root = crate::test_root::temp("guidance-tracked");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/RULES.md"), "rule").unwrap();
        std::fs::write(root.join("note.md"), "advice").unwrap();
        std::fs::write(root.join(":colon.md"), "literal").unwrap();
        let git = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap()
                .status
                .success())
        };
        git(&["init", "-q"]);
        git(&["add", "docs/RULES.md"]);
        git(&["--literal-pathspecs", "add", "--", ":colon.md"]);
        let paths = [
            "docs/RULES.md".to_owned(),
            "note.md".to_owned(),
            ":colon.md".to_owned(),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let tracked = tracked_paths(&root, &paths).unwrap();
        assert!(tracked.contains("docs/RULES.md"));
        assert!(!tracked.contains("note.md"));
        assert!(
            tracked.contains(":colon.md"),
            "a tracked name beginning with ':' is matched literally"
        );
        assert_eq!(
            tracked_paths(&root.join(".git"), &paths),
            Some(BTreeSet::new()),
            "a non-worktree answer reports nothing tracked"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
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

    fn config_with_adviser_floor() -> Value {
        let mut config = config();
        config["project"]["adviserFloor"] = json!({"version":1,"bindings":[{
            "host":"codex","model":"gpt-approved","reasoningEffort":"xhigh",
            "requiredCapabilities":[]
        }]});
        config["agents"]["adviser"] = json!({
            "profile":"agents/adviser.md", "purpose":"advise", "retention":"task",
            "crossContext":"none", "runtime":{"host":"codex","model":"gpt-approved",
                "reasoningEffort":"xhigh","fallback":"none"}
        });
        config["workflows"]["change"]["advisers"] = json!(["adviser"]);
        config
    }

    #[test]
    fn verdict_input_contract_rejects_ambiguous_or_duplicate_paths() {
        let valid = json!({"version":1,"guidanceVerdictInputs":[
            {"path":"memory/acceptance.md","projection":"file"}
        ]});
        let mut errors = Vec::new();
        validate_input_contract(&valid, &mut errors);
        assert!(errors.is_empty());
        for bad in [
            json!({"version":1,"guidanceVerdictInputs":[
                {"path":"memory/acceptance.md","projection":"file"},
                {"path":"memory/acceptance.md","projection":"file"}
            ]}),
            json!({"version":1,"guidanceVerdictInputs":[
                {"path":"../private.md","projection":"file"}
            ]}),
            json!({"version":1,"guidanceVerdictInputs":[
                {"path":"memory/acceptance.md","projection":"unknown"}
            ]}),
        ] {
            let mut errors = Vec::new();
            validate_input_contract(&bad, &mut errors);
            assert!(!errors.is_empty());
        }
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
    fn marked_floor_allows_only_adviser_runtime_replacement_and_keeps_policy_strict() {
        let original = config_with_adviser_floor();
        let state = json!({"guidanceContinuityProtocol":PROTOCOL,
            "guidanceConfigSha256":advisory_config_sha256(&original),"configSha256":"frozen"});
        let mut replacement = original.clone();
        replacement["agents"]["adviser"]["runtime"]["model"] = json!("gpt-approved-v2");
        replacement["agents"]["adviser"]["runtime"]["reasoningEffort"] = json!("max");
        assert!(assert_config_current(&loaded(replacement.clone()), &state).is_ok());

        let mut changed_policy = replacement.clone();
        changed_policy["project"]["adviserFloor"]["bindings"][0]["model"] =
            json!("gpt-approved-v2");
        assert!(assert_config_current(&loaded(changed_policy), &state).is_err());

        let mut changed_fallback = replacement.clone();
        changed_fallback["agents"]["adviser"]["runtime"]["fallback"] =
            json!({"host":"codex","model":"other","reasoningEffort":"high"});
        assert!(assert_config_current(&loaded(changed_fallback), &state).is_err());

        let mut overlap = original.clone();
        overlap["workflows"]["change"]["workers"] = json!(["adviser"]);
        let overlap_state = json!({"guidanceContinuityProtocol":PROTOCOL,
            "guidanceConfigSha256":advisory_config_sha256(&overlap),"configSha256":"frozen"});
        overlap["agents"]["adviser"]["runtime"]["model"] = json!("replacement");
        assert!(assert_config_current(&loaded(overlap), &overlap_state).is_err());

        let legacy = json!({"configSha256":crate::evidence::hash::text(
            &serde_json::to_string(&original).unwrap()
        )});
        assert!(assert_config_current(&loaded(original.clone()), &legacy).is_ok());
        let mut legacy_replacement = original;
        legacy_replacement["agents"]["adviser"]["runtime"]["model"] = json!("replacement");
        assert!(assert_config_current(&loaded(legacy_replacement), &legacy).is_err());
    }

    #[test]
    fn selected_guidance_is_advisory_only_without_executable_consumers() {
        let loaded = loaded(config());
        let mut state = json!({
            "guidanceContinuityProtocol":PROTOCOL,
            "plan":{"stages":[{"agents":[{"memoryReferences":[
                {"sourcePath":"memory/selected.md","ledgerPath":"memory/selected.jsonl"},
                {"sourcePath":"memory/architecture.md"}
            ]}]}],
                "architectureContract":{"sourcePath":"memory/architecture.md"}}
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
        assert!(unambiguous_paths(&loaded, &state).contains("memory/selected.md"));
    }

    #[test]
    fn checker_reads_do_not_make_all_guidance_verdict_inputs() {
        let loaded = loaded(config());
        for command in [
            "python3 checks/check.py",
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
                unambiguous_paths(&loaded, &state).contains("memory/advice.md"),
                "incidental reads do not establish verdict dependence: {command}"
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
        assert!(unambiguous_paths(&loaded, &preservation).contains("memory/advice.md"));

        let pre_return = json!({
            "guidanceContinuityProtocol":PROTOCOL,
            "plan":{"stages":[{"agents":[{"memoryReferences":[
                {"sourcePath":"memory/advice.md","ledgerPath":"memory/advice.jsonl"}
            ]}]}]},
            "preReturnPolicy":{"command":"sh finalize.sh","checkerSources":[]},
            "verificationCheckerSources":[{"path":"/product/finalize.sh"}]
        });
        assert!(unambiguous_paths(&loaded, &pre_return).contains("memory/advice.md"));

        let legacy = json!({
            "plan":{"stages":[{"agents":[{"memoryReferences":[
                {"sourcePath":"memory/advice.md"}
            ]}]}]}
        });
        assert!(unambiguous_paths(&loaded, &legacy).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn incidental_checker_read_preserves_evidence_but_declared_verdict_input_does_not() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let _unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = crate::test_root::temp("guidance-checker");
        std::fs::create_dir_all(root.join("memory")).unwrap();
        std::fs::write(root.join("memory/advice.md"), "first advice\n").unwrap();
        let checker = root.join("check.py");
        std::fs::write(
            &checker,
            "from pathlib import Path\nprint(Path('memory/advice.md').read_text())\n",
        )
        .unwrap();
        let observed = std::process::Command::new("python3")
            .current_dir(&root)
            .arg("check.py")
            .output()
            .unwrap();
        assert!(observed.status.success());
        assert!(String::from_utf8_lossy(&observed.stdout).contains("first advice"));

        let mut loaded = loaded(config());
        loaded.product_root = root.to_path_buf();
        loaded.state_root = root.join(".exitbind");
        loaded.path = root.join("exitbind.json");
        let state = json!({
            "guidanceContinuityProtocol":PROTOCOL,
            "plan":{"stages":[{"agents":[{"memoryReferences":[
                {"sourcePath":"memory/advice.md","ledgerPath":"memory/advice.jsonl"}
            ]}]}]},
            "verificationPreparationProtocol":1,
            "checkPolicy":{"command":"python3 check.py"},
            "verificationCheckerSources":[{
                "path":checker,"argumentPath":"check.py","sha256":"a".repeat(64)
            }]
        });
        let before = crate::run::inputs::fingerprint_for_state(&loaded, &state).unwrap();
        std::fs::write(root.join("memory/advice.md"), "changed advice\n").unwrap();
        let after = crate::run::inputs::fingerprint_for_state(&loaded, &state).unwrap();
        assert_eq!(
            before, after,
            "an incidental read does not make guidance a verdict input"
        );
        let mut declared = state.clone();
        declared[INPUT_CONTRACT_FIELD] = json!({"version":1,"guidanceVerdictInputs":[
            {"path":"memory/advice.md","projection":"file"}
        ]});
        let declared_after = crate::run::inputs::fingerprint_for_state(&loaded, &declared).unwrap();
        std::fs::write(root.join("memory/advice.md"), "new verdict advice\n").unwrap();
        let declared_changed =
            crate::run::inputs::fingerprint_for_state(&loaded, &declared).unwrap();
        assert_ne!(
            declared_after, declared_changed,
            "declared verdict input drift blocks reuse"
        );

        let mut checker_bound = state.clone();
        checker_bound["verificationCheckerSources"] = json!([{
            "path":root.join("memory/advice.md"),
            "argumentPath":"memory/advice.md",
            "sha256":"a".repeat(64)
        }]);
        let source_before =
            crate::run::inputs::fingerprint_for_state(&loaded, &checker_bound).unwrap();
        std::fs::write(root.join("memory/advice.md"), "changed checker source\n").unwrap();
        let source_after =
            crate::run::inputs::fingerprint_for_state(&loaded, &checker_bound).unwrap();
        assert_ne!(
            source_before, source_after,
            "direct checker source remains strict"
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
