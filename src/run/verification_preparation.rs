//! Per-attempt, task-owned outputs for explicitly opted-in verification.
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

mod events;
pub(crate) use events::{
    add_start_fields, append_allowed_fields, validate_start_fields, EVENT_FIELDS,
};

pub(crate) const PROTOCOL: u64 = 1;
const ENV_SCRATCH: &str = "EXITBIND_VERIFICATION_SCRATCH";

#[derive(Clone, Debug)]
pub(crate) struct RuntimePreparation {
    scratch: PathBuf,
    checker_sources: Vec<crate::run_value::CheckerSource>,
}

impl RuntimePreparation {
    pub(crate) fn environment(&self) -> BTreeMap<OsString, OsString> {
        let scratch = self.scratch.as_os_str().to_os_string();
        BTreeMap::from([
            ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
            ("TMPDIR".into(), scratch.clone()),
            ("TMP".into(), scratch.clone()),
            ("TEMP".into(), scratch.clone()),
            (OsString::from(ENV_SCRATCH), scratch),
        ])
    }

    pub(crate) fn identity(&self) -> Result<Value, String> {
        let scratch = self
            .scratch
            .to_str()
            .ok_or("verification scratch path is not valid UTF-8")?;
        Ok(json!({
            "protocol": PROTOCOL,
            "pythonBytecode": "disabled",
            "scratch": "taskOwned",
            "scratchPath": scratch,
            "environment": ["PYTHONDONTWRITEBYTECODE=1", "TMPDIR", "TMP", "TEMP", ENV_SCRATCH],
            "checkerSources": crate::run_value::checker_sources_value(&self.checker_sources)
        }))
    }

    pub(crate) fn sha256(&self) -> Result<String, String> {
        Ok(hash::value(&self.identity()?))
    }
}

/// Create one fresh scratch directory for one admitted run operation. Existing
/// paths are never reused or cleaned because they may belong to an uncertain
/// earlier process.
pub(crate) fn prepare(
    loaded: &Loaded,
    state: &Value,
    operation_id: &str,
    checker_sources: &[crate::run_value::CheckerSource],
) -> Result<Option<RuntimePreparation>, String> {
    let Some(preparation) = planned(loaded, state, operation_id, checker_sources)? else {
        return Ok(None);
    };
    let scratch = preparation.scratch.clone();
    refuse_input_aliases(loaded, state, &scratch, checker_sources)?;
    let base = scratch
        .parent()
        .ok_or("verification scratch has no owner directory")?;
    crate::project::managed_files::ensure_state_directory(&loaded.state_root, base)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(&scratch).map_err(|error| {
            format!(
                "verification scratch is not exclusively available; no check was launched: {error}"
            )
        })?;
    }
    #[cfg(not(unix))]
    fs::create_dir(&scratch).map_err(|error| {
        format!("verification scratch is not exclusively available; no check was launched: {error}")
    })?;
    let canonical = fs::canonicalize(&scratch).map_err(|error| error.to_string())?;
    if !canonical.starts_with(fs::canonicalize(&loaded.state_root).map_err(|e| e.to_string())?) {
        return Err("verification scratch escaped managed state; no check was launched".into());
    }
    Ok(Some(RuntimePreparation {
        scratch: canonical,
        checker_sources: checker_sources.to_vec(),
    }))
}

pub(crate) fn planned(
    loaded: &Loaded,
    state: &Value,
    operation_id: &str,
    checker_sources: &[crate::run_value::CheckerSource],
) -> Result<Option<RuntimePreparation>, String> {
    if state.get("verificationPreparationProtocol").is_none() {
        return Ok(None);
    }
    if state["verificationPreparationProtocol"].as_u64() != Some(PROTOCOL) {
        return Err("unsupported verification preparation protocol; no check was launched".into());
    }
    let selected = loaded.verification_preparation.as_ref().ok_or(
        "verification preparation is selected by this run but is absent from current configuration",
    )?;
    if selected.version != 1
        || selected.python_bytecode != "disabled"
        || selected.scratch != "taskOwned"
    {
        return Err("verification preparation settings differ from the supported protocol".into());
    }
    if !valid_identifier(state["runId"].as_str()) || !valid_identifier(Some(operation_id)) {
        return Err(
            "verification run or operation identity is invalid; no check was launched".into(),
        );
    }
    let scratch = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("verification")
        .join(state["runId"].as_str().ok_or("run identity is missing")?)
        .join(operation_id);
    Ok(Some(RuntimePreparation {
        scratch,
        checker_sources: checker_sources.to_vec(),
    }))
}

/// Return the frozen sources for a selected run after proving the current
/// command still names the same argument paths and bytes. Legacy runs keep
/// their prior behavior and never acquire this new binding.
pub(crate) fn current_checker_sources(
    loaded: &Loaded,
    state: &Value,
    command: &str,
) -> Result<Vec<crate::run_value::CheckerSource>, String> {
    if state.get("verificationPreparationProtocol").is_none() {
        return Ok(Vec::new());
    }
    let frozen =
        crate::run_value::checker_sources_from_value(&state["verificationCheckerSources"], 1)?;
    crate::run_value::validate_checker_sources(&frozen, "verification")?;
    let current = crate::run_value::checker_sources(loaded, command)
        .map_err(|error| format!("verification checker source is unavailable: {error}"))?;
    if current.iter().any(|source| !frozen.contains(source)) {
        return Err("verification checker source path or contents changed since Work start; no check was launched".into());
    }
    Ok(frozen)
}

/// Keep selected external checker bytes current through observation commit and
/// every live evidence/currentness check, not only before process launch.
pub(crate) fn assert_checker_sources_current(state: &Value) -> Result<(), String> {
    if state.get("verificationPreparationProtocol").is_none() {
        return Ok(());
    }
    let frozen =
        crate::run_value::checker_sources_from_value(&state["verificationCheckerSources"], 1)?;
    crate::run_value::validate_checker_sources(&frozen, "verification")
}

fn refuse_input_aliases(
    loaded: &Loaded,
    state: &Value,
    scratch: &Path,
    checker_sources: &[crate::run_value::CheckerSource],
) -> Result<(), String> {
    let target = resolve_missing(scratch)?;
    let mut pending = vec![loaded.product_root.clone()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if path == loaded.product_root.join(".git")
                || path == loaded.product_root.join(".exitbind")
                || path == loaded.product_root.join(".soulmate")
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                let link = fs::read_link(&path).map_err(|error| error.to_string())?;
                let resolved = resolve_missing(&if link.is_absolute() {
                    link
                } else {
                    path.parent()
                        .ok_or("input symlink has no parent")?
                        .join(link)
                })?;
                if overlaps(&resolved, &target) {
                    return Err(format!("verification scratch aliases a product input symlink {}; no check was launched", path.display()));
                }
            } else if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    for source in checker_sources {
        let path = PathBuf::from(&source.path);
        if overlaps(&resolve_missing(&path)?, &target) {
            return Err(
                "verification scratch aliases an approved checker source; no check was launched"
                    .into(),
            );
        }
    }
    if let Some(memory) = crate::memory::policy::get(&loaded.config) {
        let path = loaded.state_root.join(memory.root);
        if overlaps(&resolve_missing(&path)?, &target) {
            return Err(
                "verification scratch aliases the configured memory root; no check was launched"
                    .into(),
            );
        }
        if let Some(contextual) = memory.contextual {
            let path = loaded.product_root.join(contextual.source_path);
            if overlaps(&resolve_missing(&path)?, &target) {
                return Err("verification scratch aliases the configured context source; no check was launched".into());
            }
        }
    }
    if let Some(stages) = state["plan"]["stages"].as_array() {
        for reference in stages
            .iter()
            .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
            .flat_map(|agent| agent["memoryReferences"].as_array().into_iter().flatten())
        {
            if let Some(source) = reference["sourcePath"].as_str() {
                let path = loaded.product_root.join(source);
                if overlaps(&resolve_missing(&path)?, &target) {
                    return Err("verification scratch aliases a selected memory source; no check was launched".into());
                }
            }
        }
    }
    Ok(())
}

fn resolve_missing(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    resolve_components(&absolute, 0)
}

fn resolve_components(path: &Path, depth: usize) -> Result<PathBuf, String> {
    if depth > 40 {
        return Err("verification path contains too many symlink aliases".into());
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => result.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(name) => {
                let candidate = result.join(name);
                match fs::symlink_metadata(&candidate) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        let link = fs::read_link(&candidate).map_err(|error| error.to_string())?;
                        let target = if link.is_absolute() {
                            link
                        } else {
                            result.join(link)
                        };
                        result = resolve_components(&target, depth + 1)?;
                    }
                    Ok(_) => {
                        result = fs::canonicalize(candidate).map_err(|error| error.to_string())?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        result.push(name);
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
    }
    normalize(&result)
}

fn normalize(path: &Path) -> Result<PathBuf, String> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn valid_identifier(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(config: Value) -> (Loaded, PathBuf, Value) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("exitbind-r50-alias-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join(".exitbind")).unwrap();
        let loaded = Loaded {
            config,
            agents: Default::default(),
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            workflows: Default::default(),
            verification_preparation: Some(crate::config::types::VerificationPreparation {
                version: 1,
                python_bytecode: "disabled".into(),
                scratch: "taskOwned".into(),
            }),
            path: root.join("exitbind.json"),
            control_root: root.clone(),
            product_root: root.clone(),
            state_root: root.clone(),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: String::new(),
        };
        let scratch = root.join(".exitbind/verification/run/operation");
        let state = json!({"runId":"run","plan":{"stages":[{"agents":[{"memoryReferences":[]}]}]}});
        (loaded, scratch, state)
    }

    fn disjoint_fixture(config: Value) -> (Loaded, PathBuf, Value, PathBuf) {
        let (mut loaded, _, state) = fixture(config);
        let base = loaded.product_root.clone();
        loaded.control_root = base.join("control");
        loaded.product_root = base.join("product");
        loaded.state_root = base.join("state");
        for root in [
            &loaded.control_root,
            &loaded.product_root,
            &loaded.state_root,
        ] {
            fs::create_dir_all(root).unwrap();
        }
        let scratch = loaded
            .state_root
            .join(".exitbind/verification/run/operation");
        let cleanup = base;
        (loaded, scratch, state, cleanup)
    }

    fn memory(root: &str, contextual: Option<Value>) -> Value {
        let mut value = json!({"root":root,"maxItems":8,"maxBytes":32768,"protocolScopes":[],"syntheticScopes":[]});
        if let Some(contextual) = contextual {
            value["contextual"] = contextual;
        }
        value
    }

    #[test]
    fn refuses_checker_context_memory_root_and_selected_memory_aliases() {
        let cases = [
            (
                memory(
                    ".exitbind/memory",
                    Some(json!({"version":1,"sourcePath":".exitbind/verification"})),
                ),
                None,
                "configured context source",
            ),
            (
                memory(".exitbind/verification", None),
                None,
                "configured memory root",
            ),
            (
                memory(".exitbind/memory", None),
                Some(".exitbind/verification"),
                "selected memory source",
            ),
        ];
        for (config, selected_reference, diagnostic) in cases {
            let (loaded, scratch, mut state) = fixture(json!({"memory":config}));
            if let Some(source) = selected_reference {
                state["plan"]["stages"][0]["agents"][0]["memoryReferences"] =
                    json!([{"sourcePath":source}]);
            }
            let result = refuse_input_aliases(&loaded, &state, &scratch, &[]).unwrap_err();
            assert!(result.contains(diagnostic), "{diagnostic}: {result}");
            fs::remove_dir_all(loaded.product_root).unwrap();
        }

        let (loaded, scratch, state) = fixture(json!({}));
        let source = crate::run_value::CheckerSource {
            argument_path: None,
            path: loaded
                .product_root
                .join(".exitbind/verification")
                .to_str()
                .expect("fixture path is valid UTF-8")
                .to_owned(),
            sha256: "a".repeat(64),
        };
        let result = refuse_input_aliases(&loaded, &state, &scratch, &[source]).unwrap_err();
        assert!(result.contains("approved checker source"), "{result}");
        fs::remove_dir_all(loaded.product_root).unwrap();
    }

    #[test]
    fn resolves_memory_context_and_selection_under_their_owned_local_roots() {
        let (loaded, scratch, state, cleanup) =
            disjoint_fixture(json!({"memory":memory(".exitbind/verification", None)}));
        let result = refuse_input_aliases(&loaded, &state, &scratch, &[]).unwrap_err();
        assert!(result.contains("configured memory root"), "{result}");
        fs::remove_dir_all(cleanup).unwrap();

        for (config, selected_reference, description) in [
            (
                json!({"memory":memory(".exitbind/memory", Some(json!({"version":1,"sourcePath":".exitbind/verification"})))}),
                None,
                "context source belongs to product root",
            ),
            (
                json!({"memory":memory(".exitbind/memory", None)}),
                Some(".exitbind/verification"),
                "selected source belongs to product root",
            ),
        ] {
            let (loaded, scratch, mut state, cleanup) = disjoint_fixture(config);
            if let Some(source) = selected_reference {
                state["plan"]["stages"][0]["agents"][0]["memoryReferences"] =
                    json!([{"sourcePath":source}]);
            }
            fs::create_dir_all(loaded.product_root.join(".exitbind/verification")).unwrap();
            refuse_input_aliases(&loaded, &state, &scratch, &[])
                .unwrap_or_else(|error| panic!("{description}: {error}"));
            fs::remove_dir_all(cleanup).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_product_symlink_into_missing_descendant_of_owned_tree() {
        use std::os::unix::fs::symlink;
        let (loaded, scratch, state) = fixture(json!({}));
        symlink(
            ".exitbind/verification/run/operation/cache",
            loaded.product_root.join("output-alias"),
        )
        .unwrap();
        let result = refuse_input_aliases(&loaded, &state, &scratch, &[]).unwrap_err();
        assert!(result.contains("product input symlink"), "{result}");
        fs::remove_dir_all(loaded.product_root).unwrap();
    }

    #[test]
    fn refuses_preexisting_foreign_operation_scratch_without_reuse_or_cleanup() {
        let (loaded, _, _) = fixture(json!({}));
        let run_id = "a".repeat(64);
        let operation_id = "b".repeat(64);
        let state =
            json!({"runId":run_id,"verificationPreparationProtocol":1,"plan":{"stages":[]}});
        let scratch = loaded
            .state_root
            .join(".exitbind/verification")
            .join(&run_id)
            .join(&operation_id);
        fs::create_dir_all(&scratch).unwrap();
        fs::write(scratch.join("owner-data"), b"keep").unwrap();
        let error = prepare(&loaded, &state, &operation_id, &[]).unwrap_err();
        assert!(error.contains("not exclusively available"), "{error}");
        assert_eq!(fs::read(scratch.join("owner-data")).unwrap(), b"keep");
        fs::remove_dir_all(loaded.product_root).unwrap();
    }

    #[test]
    fn refuses_config_file_inside_exact_owned_operation_scratch() {
        let (mut loaded, _, _) = fixture(json!({}));
        let run_id = "a".repeat(64);
        let operation_id = "b".repeat(64);
        let state =
            json!({"runId":run_id,"verificationPreparationProtocol":1,"plan":{"stages":[]}});
        let scratch = loaded
            .state_root
            .join(".exitbind/verification")
            .join(&run_id)
            .join(&operation_id);
        fs::create_dir_all(&scratch).unwrap();
        loaded.path = scratch.join("exitbind.json");
        fs::write(&loaded.path, b"config-owner-data").unwrap();
        let error = prepare(&loaded, &state, &operation_id, &[]).unwrap_err();
        assert!(error.contains("not exclusively available"), "{error}");
        assert_eq!(fs::read(&loaded.path).unwrap(), b"config-owner-data");
        fs::remove_dir_all(loaded.product_root).unwrap();
    }
}
