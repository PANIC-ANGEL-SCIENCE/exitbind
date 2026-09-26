use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) struct LoadedSettings {
    pub(crate) target: PathBuf,
    pub(crate) real_root: PathBuf,
    pub(crate) source: Option<String>,
    pub(crate) mode: Option<u32>,
    pub(crate) document: Value,
}

pub(crate) struct StagedSettings {
    pub(crate) document: Value,
    pub(crate) serialized: Option<String>,
    pub(crate) changed: bool,
    pub(crate) actions: Vec<String>,
}

pub(crate) fn load(host: &str, root: &str) -> Result<LoadedSettings, String> {
    if root.trim().is_empty() || root.contains('\0') {
        return Err("hooks root must be a non-empty path without NUL bytes".into());
    }
    let project = absolute(Path::new(root))?;
    let real_root = ordinary_directory(&project, "project root")?;
    let target = target_path(host, root)?;
    check_existing_parents(
        &real_root,
        target
            .parent()
            .ok_or("hook settings target has no parent")?,
    )?;
    let (source, mode) = match fs::symlink_metadata(&target) {
        Ok(info) => {
            if info.file_type().is_symlink() {
                return Err(format!(
                    "hook settings file must not be a symlink: {}",
                    target.display()
                ));
            }
            if !info.is_file() {
                return Err(format!(
                    "hook settings path is not a regular file: {}",
                    target.display()
                ));
            }
            contained(
                &real_root,
                &fs::canonicalize(&target).map_err(|e| e.to_string())?,
                &target,
            )?;
            (Some(read_utf8(&target)?), Some(file_mode(&info)))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
        Err(error) => return Err(error.to_string()),
    };
    let document = match source.as_deref() {
        None => json!({}),
        Some(raw) => parse_settings(raw, &target)?,
    };
    Ok(LoadedSettings {
        target,
        real_root,
        source,
        mode,
        document,
    })
}

pub(crate) fn target_path(host: &str, root: &str) -> Result<PathBuf, String> {
    Ok(absolute(Path::new(root))?.join(if host == "codex" {
        ".codex/hooks.json"
    } else {
        ".claude/settings.json"
    }))
}

pub(crate) fn stage(
    action: &str,
    document: &Value,
    source_exists: bool,
    exact: &[usize],
    events: &[&str],
    expected: &Value,
) -> Result<StagedSettings, String> {
    let mut next = document.clone();
    let mut actions = Vec::new();
    let mut changed = false;
    let expected_object = expected
        .as_object()
        .ok_or("expected hook handler must be an object")?;
    if action == "apply" {
        let object = next
            .as_object_mut()
            .ok_or("hook settings root must be an object")?;
        if !object.get("hooks").is_some_and(Value::is_object) {
            object.insert("hooks".into(), json!({}));
        }
        let hooks = object
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .ok_or("hook settings hooks must be an object")?;
        for (i, event) in events.iter().enumerate() {
            if exact[i] > 0 {
                actions.push(format!("keep exact {event} handler"));
                continue;
            }
            let groups = hooks
                .entry((*event).to_owned())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| format!("unexpected hooks shape for {event}"))?;
            groups.push(json!({"hooks": [expected]}));
            actions.push(format!(
                "{} {event} handler",
                if source_exists { "add" } else { "create" }
            ));
            changed = true;
        }
    } else {
        let Some(hooks) = next.get_mut("hooks").and_then(Value::as_object_mut) else {
            return Ok(StagedSettings {
                document: next,
                serialized: None,
                changed: false,
                actions: vec!["no changes".into()],
            });
        };
        for event in events.iter().copied() {
            let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
                continue;
            };
            let mut retained = Vec::new();
            let mut removed = 0usize;
            for mut group in groups.drain(..) {
                let object = group
                    .as_object_mut()
                    .ok_or_else(|| format!("unexpected hooks shape for {event}"))?;
                let handlers = object
                    .get_mut("hooks")
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| format!("unexpected hooks shape for {event}"))?;
                let before = handlers.len();
                handlers.retain(|handler| {
                    !handler
                        .as_object()
                        .is_some_and(|handler| handler == expected_object)
                });
                removed += before - handlers.len();
                if before == handlers.len() || !handlers.is_empty() {
                    retained.push(group);
                }
            }
            if removed > 0 {
                if retained.is_empty() {
                    hooks.remove(event);
                } else {
                    hooks.insert(event.into(), Value::Array(retained));
                }
                actions.push(format!(
                    "remove {removed} exact {event} handler{}",
                    if removed == 1 { "" } else { "s" }
                ));
                changed = true;
            }
        }
    }
    let serialized = if changed {
        Some(format!(
            "{}\n",
            serde_json::to_string_pretty(&next).map_err(|error| error.to_string())?
        ))
    } else {
        None
    };
    Ok(StagedSettings {
        serialized,
        document: next,
        changed,
        actions: if actions.is_empty() {
            vec!["no changes".into()]
        } else {
            actions
        },
    })
}

pub(crate) fn require_compatible_command(protocol: &str) -> Result<(), String> {
    let path = std::env::var("PATH").unwrap_or_default();
    let command = if crate::producer::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    let executable = std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(command))
        .find(|candidate| executable_file(candidate))
        .ok_or(format!(
            "{command} executable was not found on PATH; install the CLI before applying hooks"
        ))?;
    let mut child = Command::new(executable)
        .arg("hook-protocol")
        .env("PATH", &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| format!("PATH-installed {command} does not support the required hook protocol; update the CLI before applying hooks"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| format!("PATH-installed {command} does not support the required hook protocol; update the CLI before applying hooks"))?
        {
            let mut stdout = String::new();
            if let Some(mut pipe) = child.stdout.take() {
                pipe.read_to_string(&mut stdout).ok();
            }
            let accepted = crate::compatibility::ACCEPTED_HOOK_PROTOCOLS
                .contains(&stdout.trim())
                || stdout.trim() == protocol;
            if status.success() && accepted {
                return Ok(());
            }
            return Err(format!("PATH-installed {command} does not support the required hook protocol; update the CLI before applying hooks"));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("PATH-installed {command} does not support the required hook protocol; update the CLI before applying hooks"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Replace one managed file atomically, refusing anything that is not the
/// ordinary file this process expects.
///
/// The parent chain is validated, the target must be a regular file with the
/// exact expected contents (`None` means it must not exist yet), the temporary
/// is created exclusively, and the rename replaces the name rather than
/// writing through it - so a symlink planted at the target overwrites nothing.
/// Both checks run again immediately before the rename. That narrows the race
/// window; it is not a guarantee against a determined same-account attacker
/// who can win it.
pub(crate) fn atomic_write(
    target: &Path,
    serialized: &str,
    mode: Option<u32>,
    expected_source: Option<&str>,
    real_root: &Path,
) -> Result<(), String> {
    let directory = target.parent().ok_or("managed file has no parent")?;
    ensure_directory(directory, real_root, target)?;
    assert_unchanged(target, expected_source)?;
    let temporary = directory.join(format!(
        ".exitbind-{}-{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    if let Some(mode) = mode {
        set_mode(&file, mode)?;
    }
    file.write_all(serialized.as_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().ok();
    drop(file);
    let result = (|| {
        ensure_directory(directory, real_root, target)?;
        assert_unchanged(target, expected_source)?;
        fs::rename(&temporary, target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn assert_unchanged(target: &Path, expected: Option<&str>) -> Result<(), String> {
    let info = match fs::symlink_metadata(target) {
        Ok(info) => info,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return if expected.is_none() {
                Ok(())
            } else {
                Err(format!(
                    "managed file changed during update: {}",
                    target.display()
                ))
            };
        }
        Err(error) => return Err(error.to_string()),
    };
    if info.file_type().is_symlink() || !info.is_file() {
        return Err(format!(
            "managed file must be an ordinary file: {}",
            target.display()
        ));
    }
    match expected {
        Some(expected) => {
            let current = read_utf8(target)?;
            if current != expected {
                Err(format!(
                    "managed file changed during update: {}",
                    target.display()
                ))
            } else {
                Ok(())
            }
        }
        None => Err(format!(
            "managed file changed during update: {}",
            target.display()
        )),
    }
}

fn check_existing_parents(root: &Path, path: &Path) -> Result<(), String> {
    let mut current = path.to_path_buf();
    loop {
        match fs::symlink_metadata(&current) {
            Ok(info) => {
                if info.file_type().is_symlink() {
                    return Err(format!(
                        "hook settings path must not contain a symlink: {}",
                        current.display()
                    ));
                }
                if !info.is_dir() {
                    return Err(format!(
                        "hook settings parent is not a directory: {}",
                        current.display()
                    ));
                }
                contained(
                    root,
                    &fs::canonicalize(&current).map_err(|e| e.to_string())?,
                    &current,
                )?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = current.parent() else {
                    return Err("cannot resolve hook settings parent".into());
                };
                if parent == current {
                    return Err("cannot resolve hook settings parent".into());
                }
                current = parent.to_path_buf();
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn ensure_directory(path: &Path, root: &Path, target: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    check_existing_parents(root, path).map_err(|e| format!("{e}: {}", target.display()))
}

fn ordinary_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let info =
        fs::symlink_metadata(path).map_err(|e| format!("{label} does not exist: {path:?}: {e}"))?;
    if info.file_type().is_symlink() || !info.is_dir() {
        return Err(format!("{label} must be a non-symlink directory"));
    }
    fs::canonicalize(path).map_err(|e| e.to_string())
}

fn contained(root: &Path, target: &Path, label: &Path) -> Result<(), String> {
    if !target.starts_with(root) {
        return Err(format!(
            "hook settings path escapes project root: {}",
            label.display()
        ));
    }
    Ok(())
}

fn read_utf8(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| format!("invalid UTF-8 in {}", path.display()))
}

fn parse_settings(source: &str, target: &Path) -> Result<Value, String> {
    let value: Value = serde_json::from_str(source)
        .map_err(|e| format!("invalid JSON in {}: {e}", target.display()))?;
    if !value.is_object() {
        return Err(format!(
            "invalid JSON in {}: root must be an object",
            target.display()
        ));
    }
    Ok(value)
}

fn executable_file(path: &Path) -> bool {
    let Ok(info) = fs::metadata(path) else {
        return false;
    };
    if !info.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        info.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|error| format!("current directory cannot be resolved: {error}"))?
            .join(path))
    }
}

fn file_mode(info: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        info.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = info;
        0o600
    }
}

fn set_mode(file: &File, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = (file, mode);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::atomic_write;
    use std::fs;

    #[test]
    fn a_target_that_changed_since_it_was_read_is_not_replaced() {
        let root = std::env::temp_dir().join(format!("exitbind-cas-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let target = root.join("managed.json");
        fs::write(&target, b"what someone else wrote\n").unwrap();

        // The writer is handed the contents of an earlier read; the file no
        // longer holds them, so it keeps what is actually there.
        let refused = atomic_write(
            &target,
            "replacement\n",
            Some(0o600),
            Some("what this process read\n"),
            &root,
        );
        assert!(refused.is_err(), "{refused:?}");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "what someone else wrote\n"
        );

        // Expecting no file at all is equally refused once one exists.
        assert!(atomic_write(&target, "replacement\n", None, None, &root).is_err());
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "what someone else wrote\n"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
