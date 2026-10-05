//! Apply an explicitly declared reviewed selection through the existing
//! project configuration owner; preview is read-only, never approval evidence.
use super::*;
use std::fs;

#[cfg(test)]
thread_local! {
    static AFTER_WRITE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn select(
    loaded: &Loaded,
    source: &str,
    decision: &str,
    reason: &str,
    binding: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    if decision != "reviewed"
        || reason.trim().is_empty()
        || reason.len() > 4096
        || reason.chars().any(char::is_control)
    {
        return Err("architecture selection requires --decision reviewed and a nonempty bounded --reason describing the existing project decision".into());
    }
    let proposed = candidate(loaded, source)?;
    let selected = serde_json::to_value(&proposed.0).map_err(|e| e.to_string())?;
    let configuration = loaded
        .path
        .to_str()
        .ok_or("configuration path is not UTF-8")?;
    let identity = hash::value(&json!({"configurationPath": configuration,
        "productRoot": loaded.product_root.to_str().ok_or("ProductRoot is not UTF-8")?,
        "configurationSha256": hash::text(&loaded.source), "selection": selected,
        "decision": decision, "reason": reason}));
    let previous = loaded
        .architecture_contract
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| e.to_string())?;
    let changed = previous.as_ref() != Some(&selected);
    let mut report = json!({"version": 1, "state": "preview", "changed": changed,
        "previous": previous, "selection": selected, "sourceSummary": proposed.1, "configurationSha256": hash::text(&loaded.source),
        "currentBinding": identity, "decision": {"declaration": decision, "reason": reason,
            "evidence": "caller declaration of existing project review; no human approval inferred"},
        "activeWork": "existing Work retains frozen configuration; an approved change requires its supported supersession",
        "checks": "selection reads no checker input and launches no command"});
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    report["apply"] = json!({"command": [executable.to_str().ok_or("executable path is not UTF-8")?,
        "project", "architecture", "select", source, "--decision", decision, "--reason", reason,
        "--current-binding", identity, "--apply", "--json", "--config", configuration],
        "meaning": "select the exact reviewed source in this configuration; rerun preview if inputs change"});
    if fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source {
        return Err(
            "configuration changed during selection preview; no configuration was written".into(),
        );
    }
    if !apply {
        return Ok(report);
    }
    if binding != Some(identity.as_str()) {
        return Err("architecture selection preview is stale or belongs to another source, project, configuration or decision; run preview again; no configuration was written".into());
    }
    let _lock = lock_configuration(loaded)?;
    // Re-read both authorities after acquiring the existing file's lock.
    if fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source
        || serde_json::to_value(candidate(loaded, source)?.0).map_err(|e| e.to_string())?
            != selected
    {
        return Err(
            "architecture selection inputs changed before apply; no configuration was written"
                .into(),
        );
    }
    if !changed {
        report["state"] = json!("unchanged");
        return Ok(report);
    }
    // The exact raw configuration is retained for compatibility: replace only
    // the selected field, then use the sole configuration validation owner.
    let mut configuration: Value =
        serde_json::from_str(&loaded.source).map_err(|e| e.to_string())?;
    configuration["project"]["architectureContract"] = selected;
    let errors = crate::config::validate(&configuration);
    if !errors.is_empty() {
        return Err(format!(
            "invalid selected configuration: {}",
            errors.join("; ")
        ));
    }
    let serialized = format!(
        "{}\n",
        serde_json::to_string_pretty(&configuration).map_err(|e| e.to_string())?
    );
    let root = fs::canonicalize(&loaded.control_root).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(
            fs::metadata(&loaded.path)
                .map_err(|e| e.to_string())?
                .permissions()
                .mode()
                & 0o777,
        )
    };
    #[cfg(not(unix))]
    let mode = None;
    crate::host::settings::atomic_write(
        &loaded.path,
        &serialized,
        mode,
        Some(&loaded.source),
        &root,
    )?;
    report["state"] = json!("applied");
    report["effect"] = json!("applied");
    report["configurationSha256"] = json!(hash::text(&serialized));
    report["appliedBinding"] = report["currentBinding"].clone();
    #[cfg(test)]
    {
        let callback = AFTER_WRITE.with(|hook| hook.borrow_mut().take());
        if let Some(callback) = callback {
            callback();
        }
    }
    // Rename replaces the locked inode. Another legitimate writer can then
    // win; never rebind this old reviewed intent to its new configuration.
    let refreshed = (|| {
        if fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != serialized {
            return Err("configuration changed after selection was applied".to_string());
        }
        let current = crate::config::load(loaded.path.to_str())?;
        if current.source != serialized {
            return Err("configuration changed while reloading applied selection".into());
        }
        assert_current(&current)?;
        let preview = select(&current, source, decision, reason, None, false)?;
        if preview["changed"] != false
            || preview["selection"] != report["selection"]
            || preview["configurationSha256"] != hash::text(&serialized)
        {
            return Err(
                "source or configuration changed while refreshing applied selection".into(),
            );
        }
        Ok(preview)
    })();
    match refreshed {
        Ok(preview) => {
            report["currentBinding"] = preview["currentBinding"].clone();
            report["apply"] = preview["apply"].clone();
        }
        Err(error) => {
            report["state"] = json!("applied_current_changed");
            report["currentBinding"] = Value::Null;
            report["apply"] = Value::Null;
            report["diagnostic"] = json!(error
                .chars()
                .filter(|c| !c.is_control())
                .take(512)
                .collect::<String>());
            report["nextAction"] = json!({"type": "inspect", "safe": true,
                "command": [executable.to_str().ok_or("executable path is not UTF-8")?,
                    "project", "architecture", "--json", "--config", loaded.path.to_str().ok_or("configuration path is not UTF-8")?]});
        }
    }
    Ok(report)
}

fn candidate(loaded: &Loaded, source: &str) -> Result<(Selection, Value), String> {
    validation::selection(&Selection {
        source_path: source.into(),
        source_sha256: "0".repeat(64),
        revision: "preview".into(),
    })?;
    let bytes = read(loaded, source, MAX_SOURCE_BYTES)?;
    let contract: Contract = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid architecture contract: {e}"))?;
    validation::contract(&contract)?;
    let summary = json!({"schemaVersion": contract.version,
        "responsibilities": contract.responsibilities.iter().map(|item| json!({"id": item.id, "paths": item.paths})).collect::<Vec<_>>(),
        "dependencyCount": contract.dependencies.len(), "interfaceCount": contract.interfaces.len(),
        "checks": contract.checks.iter().map(|item| json!({"id": item.id, "path": item.path, "assertion": item.assertion})).collect::<Vec<_>>()});
    let candidate = Selection {
        source_path: source.into(),
        source_sha256: hash::bytes(&bytes),
        revision: contract.revision,
    };
    validation::selection(&candidate)?;
    Ok((candidate, summary))
}

fn lock_configuration(loaded: &Loaded) -> Result<fs::File, String> {
    #[cfg(unix)]
    {
        use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&loaded.path)
            .map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file()
            || unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
        {
            return Err("another architecture selection is applying or configuration is unsafe; no configuration was written".into());
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        let _ = loaded;
        Err(
            "architecture selection apply requires POSIX file locking; preview remains read-only"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn committed_selection_never_rebinds_a_noop_to_a_concurrent_replacement() {
        if !crate::compatibility::is_exitbind() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "exitbind-selection-race-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        let mut git = std::process::Command::new("git");
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_CEILING_DIRECTORIES",
            "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        ] {
            git.env_remove(key);
        }
        assert!(git
            .args(["-C", root.to_str().unwrap(), "init", "--quiet"])
            .status()
            .unwrap()
            .success());
        let configuration = crate::project::onboarding::init_with_options(
            crate::project::onboarding::InitOptions {
                product_root: root.to_str().unwrap(),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            },
        )
        .unwrap();
        for revision in ["A", "B"] {
            fs::write(
                root.join(format!("{revision}.json")),
                json!({"version":1,"revision":revision,
                "responsibilities":[{"id":"core","summary":"core","paths":["core"]}],
                "dependencies":[],"interfaces":[],"checks":[]})
                .to_string(),
            )
            .unwrap();
        }
        let loaded = crate::config::load(configuration.to_str()).unwrap();
        let preview = select(
            &loaded,
            "A.json",
            "reviewed",
            "project selected A",
            None,
            false,
        )
        .unwrap();
        let path = configuration.clone();
        AFTER_WRITE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let current = crate::config::load(path.to_str()).unwrap();
                let proposed = select(
                    &current,
                    "B.json",
                    "reviewed",
                    "project selected B",
                    None,
                    false,
                )
                .unwrap();
                let applied = select(
                    &current,
                    "B.json",
                    "reviewed",
                    "project selected B",
                    proposed["currentBinding"].as_str(),
                    true,
                )
                .unwrap();
                assert_eq!(applied["state"], "applied");
            }))
        });
        let result = select(
            &loaded,
            "A.json",
            "reviewed",
            "project selected A",
            preview["currentBinding"].as_str(),
            true,
        )
        .unwrap();
        assert_eq!(result["effect"], "applied");
        assert_eq!(result["selection"]["revision"], "A");
        assert_eq!(result["state"], "applied_current_changed");
        assert!(result["apply"].is_null());
        assert!(result["currentBinding"].is_null());
        assert_eq!(result["nextAction"]["type"], "inspect");
        assert_eq!(result["nextAction"]["safe"], true);
        let current = crate::config::load(configuration.to_str()).unwrap();
        assert_eq!(
            current.architecture_contract.as_ref().unwrap().revision,
            "B"
        );
        assert!(select(
            &current,
            "A.json",
            "reviewed",
            "project selected A",
            preview["currentBinding"].as_str(),
            true
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
