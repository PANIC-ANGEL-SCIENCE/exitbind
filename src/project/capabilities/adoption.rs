//! Bounded reviewed registration through the existing configuration writer.
use super::*;
use std::fs;

pub(crate) fn select(
    loaded: &Loaded,
    source: &str,
    decision: &str,
    reason: &str,
    binding: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    if decision != "reviewed" || !text(reason, 512) || !relative(source) {
        return Err("capability registration requires a confined descriptor path, --decision reviewed and a bounded --reason for the existing owner approval".into());
    }
    let bytes = bounded_read(loaded, source)?;
    let d = descriptor(loaded, &bytes)?;
    let content_bytes = bounded_read(loaded, &d.path)?;
    std::str::from_utf8(&content_bytes).map_err(|_| "capability content must be UTF-8")?;
    let proposed = Selection {
        id: d.id,
        revision: d.revision,
        descriptor_path: source.into(),
        descriptor_sha256: hash::bytes(&bytes),
        content_sha256: hash::bytes(&content_bytes),
        approved_by: d.owner,
        approval_reason: reason.into(),
    };
    let mut entries = loaded.capabilities.clone();
    let prior = entries.iter().find(|s| s.id == proposed.id).cloned();
    entries.retain(|s| s.id != proposed.id);
    entries.push(proposed.clone());
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    selections(&serde_json::to_value(&entries).map_err(|e| e.to_string())?)?;
    let changed = prior.as_ref() != Some(&proposed);
    let current = hash::value(
        &json!({"configPath":loaded.path,"projectRoot":loaded.product_root,
        "configurationSha256":hash::text(&loaded.source),"selection":proposed,"decision":decision,"reason":reason}),
    );
    let config_path = loaded
        .path
        .to_str()
        .ok_or("capability configuration path is not UTF-8")?;
    let mut report = json!({"version":1,"state":"preview","effect":"no-change","changed":changed,
        "selection":proposed,"currentBinding":current,
        "approval":{"declaration":decision,"reason":reason,"meaning":"existing owner approval asserted by the configured caller; no new permission inferred"},
        "nextLaunch":"pending next launch; existing Work keeps its frozen configuration and requires its supported supersession after a changed selection",
        "apply":{"command":crate::emitted::Route::new(["project","capabilities","select",source,"--decision",decision,"--reason",reason,
            "--current-binding",current.as_str(),"--apply","--json"]).config_text(config_path).full_argv()?}});
    let unchanged = || {
        fs::read_to_string(&loaded.path)
            .map_err(|e| e.to_string())
            .and_then(|s| {
                if s == loaded.source {
                    Ok(())
                } else {
                    Err("capability configuration changed; inspect before retrying".into())
                }
            })
    };
    unchanged()?;
    if !apply {
        return Ok(report);
    }
    if binding != Some(current.as_str()) {
        return Err("capability registration binding is stale or approval is missing; rerun the read-only preview; no configuration was written".into());
    }
    let _lock = lock(loaded)?;
    unchanged()?;
    if hash::bytes(&bounded_read(loaded, source)?) != proposed.descriptor_sha256
        || hash::bytes(&bounded_read(loaded, &d.path)?) != proposed.content_sha256
    {
        return Err(
            "capability source changed before registration; no configuration was written".into(),
        );
    }
    if !changed {
        report["state"] = json!("unchanged");
        return Ok(report);
    }
    let mut config: Value = serde_json::from_str(&loaded.source).map_err(|e| e.to_string())?;
    config["project"]["capabilities"] = serde_json::to_value(entries).map_err(|e| e.to_string())?;
    let errors = crate::config::validate(&config);
    if !errors.is_empty() {
        return Err(format!(
            "invalid capability registration: {}",
            errors.join("; ")
        ));
    }
    let source = format!(
        "{}\n",
        serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?
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
    crate::host::settings::atomic_write(&loaded.path, &source, mode, Some(&loaded.source), &root)?;
    report["effect"] = json!("applied");
    report["state"] = json!("registered");
    report["configurationSha256"] = json!(hash::text(&source));
    report["currentBinding"] = Value::Null;
    report["apply"] = Value::Null;
    report["nextAction"] = json!({"command":crate::emitted::Route::new(["project","capabilities","--json"]).config_text(config_path).full_argv()?,"safe":true,"effect":"no-change"});
    if fs::read_to_string(&loaded.path).ok().as_deref() != Some(source.as_str()) {
        report["state"] = json!("applied_current_changed");
    }
    Ok(report)
}
fn lock(loaded: &Loaded) -> Result<fs::File, String> {
    #[cfg(unix)]
    {
        use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
        let f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&loaded.path)
            .map_err(|e| e.to_string())?;
        if !f.metadata().map_err(|e| e.to_string())?.is_file()
            || unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
        {
            return Err(
                "capability configuration is unsafe or another selection is applying".into(),
            );
        }
        Ok(f)
    }
    #[cfg(not(unix))]
    {
        let _ = loaded;
        Err(
            "capability registration apply requires POSIX file locking; preview remains read-only"
                .into(),
        )
    }
}
