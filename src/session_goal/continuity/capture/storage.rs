//! Existing private child-intent persistence and its mutation lock.

use super::*;

fn relative() -> String {
    format!("{}/{FILE}", crate::project::layout_types::state_namespace())
}

struct Container {
    envelope: Value,
    intents: Vec<Value>,
}

fn read(loaded: &Loaded) -> Result<Container, String> {
    let path = loaded.state_root.join(relative());
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Container {
                envelope: json!({"version": 1}),
                intents: Vec::new(),
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let envelope: Value =
        serde_json::from_slice(&bytes).map_err(|_| "child intent state is not JSON")?;
    let intents = match (envelope["version"].as_u64(), envelope["intents"].as_array()) {
        (Some(1 | 2), Some(intents)) => intents.clone(),
        _ => return Err("child intent state is malformed".into()),
    };
    Ok(Container { envelope, intents })
}

pub(super) fn load(loaded: &Loaded) -> Result<Vec<Value>, String> {
    Ok(read(loaded)?.intents)
}

pub(super) fn version(loaded: &Loaded) -> Result<u64, String> {
    read(loaded)?.envelope["version"]
        .as_u64()
        .ok_or_else(|| "child intent version is unavailable".into())
}

fn save(loaded: &Loaded, container: &Container) -> Result<(), String> {
    let path = loaded.state_root.join(relative());
    let staged = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut envelope = container.envelope.clone();
    envelope["intents"] = json!(container.intents);
    let body = envelope.to_string();
    let result = (|| -> Result<(), String> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&staged).map_err(|error| error.to_string())?;
        file.write_all(body.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        fs::rename(&staged, &path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

// Publish version 2 only after its rename is durable. If directory syncing
// is unsupported or fails, do not acknowledge the transition or a return.
fn save_v2(loaded: &Loaded, container: &Container) -> Result<(), String> {
    save(loaded, container)?;
    let path = loaded.state_root.join(relative());
    let directory = path
        .parent()
        .ok_or("child intent directory is unavailable")?;
    fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("cannot synchronize child intent directory: {error}"))
}

fn upgrade_safe(intent: &Value) -> bool {
    if !intent["pendingResult"].is_null()
        || (!intent["pendingReturn"].is_null() && intent["pendingReturn"] != false)
    {
        return false;
    }
    match intent["state"].as_str() {
        Some("recorded" | "failed") => intent["nativeChild"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        Some("abandoned" | "superseded") => intent["nativeChild"].is_null(),
        _ => false,
    }
}

pub(super) fn can_upgrade(intents: &[Value]) -> bool {
    intents.iter().all(upgrade_safe)
}

fn prune(intents: &mut Vec<Value>, version: u64) {
    // Do not discard either side of a damaged retry lineage.
    if version == 2
        && intents
            .iter()
            .any(|intent| super::reviewer_retry::marker(intent).is_err())
    {
        return;
    }
    while intents.len() > MAX_KEPT {
        let Some(index) = intents.iter().position(|intent| {
            TERMINAL.iter().any(|state| intent["state"] == *state)
                && intent["rawObservation"].is_null()
                && intent["pendingResult"].is_null()
                && (version == 1
                    || (intent["retryOf"].is_null()
                        && !intents.iter().any(|candidate| {
                            candidate["retryOf"] == intent["id"] && !candidate["retryOf"].is_null()
                        })))
        }) else {
            break;
        };
        intents.remove(index);
    }
}

fn with_container<T>(
    loaded: &Loaded,
    retry: bool,
    action: impl FnOnce(&mut Vec<Value>) -> Result<T, String>,
) -> Result<T, String> {
    let ledger = crate::run::ledger::ledger_path(&loaded.state_root, &relative(), true)?;
    crate::run::ledger::with_lock(&ledger, || {
        let mut container = read(loaded)?;
        let upgrading = retry && container.envelope["version"] == 1;
        if upgrading && !can_upgrade(&container.intents) {
            return Err("child intent upgrade requires every sibling to be known ended and without a pending result".into());
        }
        let result = action(&mut container.intents)?;
        if upgrading {
            container.envelope["version"] = json!(2);
        }
        let version = container.envelope["version"]
            .as_u64()
            .ok_or("child intent version is unavailable")?;
        prune(&mut container.intents, version);
        if version == 2 {
            save_v2(loaded, &container)?;
        } else {
            save(loaded, &container)?;
        }
        Ok(result)
    })
}

/// Mutate the intents under their own lock, preserving the container version.
pub(super) fn with_intents<T>(
    loaded: &Loaded,
    action: impl FnOnce(&mut Vec<Value>) -> Result<T, String>,
) -> Result<T, String> {
    with_container(loaded, false, action)
}

/// Only an explicit eligible retry upgrades the state-root container.
pub(super) fn with_retry_intents<T>(
    loaded: &Loaded,
    action: impl FnOnce(&mut Vec<Value>) -> Result<T, String>,
) -> Result<T, String> {
    with_container(loaded, true, action)
}

/// Record the single retry return attempt while the caller holds the intent
/// lock, before entering the Work owner. A lost later save stays ambiguous.
pub(super) fn checkpoint_retry_return(loaded: &Loaded, intents: &[Value]) -> Result<(), String> {
    let mut container = read(loaded)?;
    if container.envelope["version"] != 2 {
        return Err("retry return requires the version 2 child intent container".into());
    }
    container.intents = intents.to_vec();
    save_v2(loaded, &container)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_requires_known_ended_siblings_and_no_pending_result() {
        for value in [
            json!({"state":"prepared"}),
            json!({"state":"claimed","nativeChild":"live"}),
            json!({"state":"abandoned","nativeChild":"unknown"}),
            json!({"state":"recorded","nativeChild":"ended","pendingReturn":"unknown"}),
            json!({"state":"recorded","nativeChild":"ended","pendingResult":"retained"}),
            json!({"state":"recorded"}),
            json!({"state":"unknown"}),
        ] {
            assert!(!can_upgrade(&[value.clone()]), "{value}");
        }
        assert!(can_upgrade(&[
            json!({"state":"recorded","nativeChild":"ended"}),
            json!({"state":"failed","nativeChild":"ended"}),
            json!({"state":"superseded"})
        ]));
    }

    #[test]
    fn pruning_preserves_v2_retry_lineage_without_changing_v1_cleanup() {
        let mut retry = json!({"retryOf":"a".repeat(64),"retryFence":{}});
        retry["id"] = json!(hash::value(&retry));
        retry["state"] = json!("recorded");
        retry["nativeChild"] = json!("new-child");
        let old = json!({"id":"a".repeat(64),"state":"recorded","nativeChild":"old-child"});
        let mut values = vec![old.clone(), retry.clone()];
        values.extend(
            (0..MAX_KEPT).map(|index| json!({"id":format!("other-{index}"),"state":"superseded"})),
        );
        let mut legacy = values.clone();
        prune(&mut legacy, 1);
        assert!(!legacy.iter().any(|item| item == &old));
        prune(&mut values, 2);
        assert!(values.iter().any(|item| item == &old));
        assert!(values.iter().any(|item| item == &retry));
        assert_eq!(values.len(), MAX_KEPT);
    }
}
