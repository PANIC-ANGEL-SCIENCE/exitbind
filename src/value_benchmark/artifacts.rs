use super::*;

pub(super) fn fixture_artifacts(
    fixture: &Path,
    paths: &[&str],
) -> Result<Vec<FixtureFile>, String> {
    paths
        .iter()
        .map(|relative| {
            let bytes = read_file(fixture.join(relative), "fixture artifact")?;
            let bundle_relative = format!("fixtures/{relative}");
            Ok(FixtureFile {
                bundle_relative,
                bytes,
            })
        })
        .collect()
}

pub(super) fn write_state_artifact(
    fixture: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    let relative = format!("{}/artifacts/{name}", state_namespace());
    write_file(fixture.join(&relative), bytes, "state artifact")?;
    Ok(relative)
}

pub(super) fn read_state_artifact(fixture: &Path, relative: &str) -> Result<Vec<u8>, String> {
    read_file(fixture.join(relative), "state artifact")
}

pub(super) fn read_ledger(fixture: &Path, name: &str) -> Result<Vec<u8>, String> {
    read_file(
        fixture.join(format!("{}/runs/{name}", state_namespace())),
        "run ledger",
    )
}

pub(super) fn parse_ledger(bytes: &[u8], label: &str) -> Result<Vec<Value>, String> {
    let source =
        std::str::from_utf8(bytes).map_err(|error| format!("{label} is not UTF-8: {error}"))?;
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| format!("{label} has invalid JSON: {error}"))
        })
        .collect()
}

pub(super) fn write_file(path: PathBuf, bytes: &[u8], label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label} has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| format!("{label} cannot create parent: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("{label} cannot be created: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("{label} cannot be written: {error}"))?;
    file.flush()
        .map_err(|error| format!("{label} cannot be flushed: {error}"))?;
    set_file_mode(&path, 0o600)?;
    Ok(())
}

pub(super) fn replace_file(path: PathBuf, bytes: &[u8], label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("{label} cannot be replaced: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} must be a regular file"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|error| format!("{label} cannot be replaced: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("{label} cannot be written: {error}"))?;
    file.flush()
        .map_err(|error| format!("{label} cannot be flushed: {error}"))?;
    set_file_mode(&path, 0o600)?;
    Ok(())
}

pub(super) fn read_file(path: PathBuf, label: &str) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("{label} cannot be read: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} must be a regular file"));
    }
    fs::read(path).map_err(|error| format!("{label} cannot be read: {error}"))
}

pub(super) fn resolve_output_destination(output: &str) -> Result<PathBuf, String> {
    if output.trim().is_empty() || output.contains('\0') {
        return Err("benchmark output path must be a non-empty UTF-8 path".into());
    }
    let requested = Path::new(output);
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("benchmark current directory cannot be resolved: {error}"))?
            .join(requested)
    };
    let parent = absolute
        .parent()
        .ok_or("benchmark output path has no parent")?;
    let real_parent = fs::canonicalize(parent)
        .map_err(|error| format!("benchmark output parent cannot be resolved: {error}"))?;
    let name = absolute
        .file_name()
        .ok_or("benchmark output path has no final directory name")?;
    let destination = real_parent.join(name);
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() {
            return Err("benchmark output path must not be a symlink".into());
        }
        return Err("benchmark output path already exists; refusing overwrite".into());
    }
    Ok(destination)
}

pub(super) fn reject_destination_inside_fixture(
    destination: &Path,
    fixture: &Path,
) -> Result<(), String> {
    if destination.starts_with(fixture) {
        Err("benchmark output path must be outside its disposable fixture".into())
    } else {
        Ok(())
    }
}

struct BundleGuard {
    path: PathBuf,
    created: bool,
}

impl Drop for BundleGuard {
    fn drop(&mut self) {
        if !self.created {
            return;
        }
        if let Ok(metadata) = fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return;
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) fn export_bundle(
    destination: PathBuf,
    evidence: &ScenarioEvidence,
    result: &Value,
) -> Result<(), String> {
    if destination.exists() || fs::symlink_metadata(&destination).is_ok() {
        return Err("benchmark output path already exists; refusing overwrite".into());
    }
    fs::create_dir(&destination)
        .map_err(|error| format!("benchmark output directory cannot be created: {error}"))?;
    set_dir_mode(&destination, 0o700)?;
    let guard = BundleGuard {
        path: destination.clone(),
        created: true,
    };
    let result_bytes = pretty_json(result)?;
    write_bundle_file(&destination, "result.json", result_bytes.as_bytes())?;
    write_bundle_file(
        &destination,
        "result.md",
        result_markdown(result).as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "report.json",
        pretty_json(&evidence.report)?.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "report.md",
        evidence.report_markdown.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "scenario/README.md",
        SCENARIO_README.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "scenario/expected.json",
        pretty_json(&expected_contract()?)?.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "ledgers/blocked.jsonl",
        &evidence.blocked_ledger,
    )?;
    write_bundle_file(
        &destination,
        "ledgers/baseline.jsonl",
        &evidence.baseline_ledger,
    )?;
    write_bundle_file(&destination, "ledgers/final.jsonl", &evidence.final_ledger)?;
    write_bundle_file(
        &destination,
        &format!("fixtures/{}", config_name()),
        &evidence.config_bytes,
    )?;
    write_bundle_file(
        &destination,
        "fixtures/verification-invalid.json",
        &evidence.verification_invalid_bytes,
    )?;
    write_bundle_file(
        &destination,
        "fixtures/verification-valid.json",
        &evidence.verification_valid_bytes,
    )?;
    for file in &evidence.artifact_files {
        write_bundle_file(&destination, &file.bundle_relative, &file.bytes)?;
    }
    for (name, schema) in [
        ("run-event-v3.schema.json", RUN_EVENT_V3_SCHEMA),
        ("run-event-v4.schema.json", RUN_EVENT_V4_SCHEMA),
        ("value-report-v1.schema.json", VALUE_REPORT_V1_SCHEMA),
        ("value-proof-v1.schema.json", VALUE_PROOF_V1_SCHEMA),
    ] {
        write_bundle_file(&destination, &format!("schema/{name}"), schema.as_bytes())?;
    }
    if exitbind_surface() {
        write_bundle_file(
            &destination,
            "schema/run-event-v5.schema.json",
            RUN_EVENT_V5_SCHEMA.as_bytes(),
        )?;
    }

    let manifest = hash_manifest(&destination)?;
    write_bundle_file(
        &destination,
        "hash-manifest.json",
        pretty_json(&manifest)?.as_bytes(),
    )?;
    std::mem::forget(guard);
    Ok(())
}

pub(super) fn write_bundle_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        ensure_bundle_directory(root, parent)?;
    }
    write_file(path, bytes, "proof bundle file")
}

pub(super) fn ensure_bundle_directory(root: &Path, directory: &Path) -> Result<(), String> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "proof bundle path escapes bundle root")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("proof bundle path is not a regular directory".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    format!("proof bundle directory cannot be created: {error}")
                })?;
                set_dir_mode(&current, 0o700)?;
            }
            Err(error) => return Err(format!("proof bundle directory cannot be read: {error}")),
        }
    }
    Ok(())
}

pub(super) fn hash_manifest(root: &Path) -> Result<Value, String> {
    let mut files = Vec::new();
    collect_bundle_files(root, root, &mut files)?;
    files.sort_by(|left, right| {
        left["path"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["path"].as_str().unwrap_or_default())
    });
    Ok(json!({
        "version": 1,
        "files": files,
        "self": "excluded",
    }))
}

pub(super) fn collect_bundle_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<Value>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("proof bundle cannot be read: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("proof bundle cannot be read: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("proof bundle entry cannot be read: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("proof bundle must not contain symlinks".into());
        }
        if metadata.is_dir() {
            collect_bundle_files(root, &path, files)?;
        } else if metadata.is_file()
            && path.file_name().and_then(OsStr::to_str) != Some("hash-manifest.json")
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "proof bundle entry escapes root")?
                .to_str()
                .ok_or("proof bundle entry is not valid UTF-8")?
                .replace('\\', "/");
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            files.push(json!({
                "path": relative,
                "sha256": sha256(&bytes),
                "bytes": bytes.len(),
            }));
        }
    }
    Ok(())
}

pub(super) fn result_markdown(result: &Value) -> String {
    let assertions = result["assertions"].as_array().map_or(0, Vec::len);
    format!(
        "# False-completion v1\n\nClaim: {}\n\nOrigin: synthetic\n\nAssertions passed: {assertions}\n\nManual JSON edits: {}\nAutomated elapsed time (ms): {}\nHuman interaction time (ms): unmeasured\n\nThis synthetic result does not establish recurring user losses, avoided harm, novice success, market frequency, model compliance, or human time saved.\n",
        result["claim"],
        result["manualJsonEdits"],
        result["automatedElapsedMs"],
    )
}

pub(super) fn pretty_json(value: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|value| format!("{value}\n"))
        .map_err(|error| format!("proof JSON cannot be serialized: {error}"))
}

pub(super) fn set_file_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

pub(super) fn set_dir_mode(path: &Path, mode: u32) -> Result<(), String> {
    set_file_mode(path, mode)
}
