//! Durable native assignment journal.

use super::*;

pub(super) fn read_journal(path: &Path) -> Result<Option<Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("native assignment journal is not a regular file".into());
    }
    if metadata.len() > MAX_JOURNAL_BYTES {
        return Err("native assignment journal exceeds its bound".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("native assignment journal is invalid: {error}"))
}

pub(super) fn verify_journal(
    journal: &Value,
    work: &str,
    identity: &Identity,
    role: &str,
    agent: &str,
) -> Result<(), String> {
    if journal["version"] != 1
        || journal["work"] != work
        || journal["assignment"] != identity.assignment
        || journal["assignmentSha256"] != identity.packet_sha256
        || journal["role"] != role
        || journal["agent"] != agent
    {
        return Err("native assignment journal does not match the current assignment".into());
    }
    Ok(())
}

pub(super) fn journal_result(journal: &Value) -> Result<Vec<u8>, String> {
    let encoded = journal["result"]
        .as_str()
        .ok_or("completed native journal has no result")?;
    if encoded.len() > MAX_RESULT_BYTES * 2 {
        return Err("completed native journal result exceeds its bound".into());
    }
    let bytes = hex_decode(encoded)?;
    if bytes.len() > MAX_RESULT_BYTES {
        return Err("completed native journal result exceeds its bound".into());
    }
    Ok(bytes)
}

pub(super) fn write_journal(path: &Path, value: &Value, exclusive: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_JOURNAL_BYTES as usize {
        return Err("native assignment journal exceeds its bound".into());
    }
    atomic_publish(path, &bytes, exclusive)
}

fn atomic_publish(path: &Path, bytes: &[u8], exclusive: bool) -> Result<(), String> {
    let parent = path.parent().ok_or("native journal has no parent")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("native journal has an invalid name")?;
    let temporary = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        timestamp()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        if exclusive {
            fs::hard_link(&temporary, path).map_err(|error| error.to_string())?;
        } else {
            fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        }
        sync_directory(parent)?;
        Ok::<(), String>(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

pub(super) fn claim_started(path: &Path) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    match journal["status"].as_str() {
        Some("started") => {}
        Some("running") => return Ok(()),
        _ => return Err("native assignment journal cannot be claimed".into()),
    }
    journal["status"] = json!("running");
    write_journal(path, &journal, false)
}

pub(super) fn record_error(path: &Path, error: &str) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    journal["error"] = json!(error.chars().take(1024).collect::<String>());
    write_journal(path, &journal, false)
}

pub(super) fn update_started(path: &Path, observation: &Value) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    journal["status"] = json!("started");
    journal["observation"] = observation.clone();
    journal["threadId"] = observation["threadId"].clone();
    write_journal(path, &journal, false)
}

pub(super) fn write_completed(
    path: &Path,
    observation: &Value,
    result: &[u8],
) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    journal["status"] = json!("completed");
    journal["completedAt"] = json!(timestamp());
    journal["observation"] = observation.clone();
    journal["threadId"] = observation["threadId"].clone();
    journal["result"] = json!(hex_encode(result));
    write_journal(path, &journal, false)
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .map_err(|error| error.to_string())?
            .sync_all()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_publication_preserves_the_first_complete_record() {
        let directory = std::env::temp_dir().join(format!(
            "exitbind-journal-{}-{}",
            std::process::id(),
            timestamp()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("one.json");
        write_journal(&path, &json!({"status":"started"}), true).unwrap();
        assert!(write_journal(&path, &json!({"status":"wrong"}), true).is_err());
        assert_eq!(read_journal(&path).unwrap().unwrap()["status"], "started");
        write_journal(&path, &json!({"status":"completed"}), false).unwrap();
        assert_eq!(read_journal(&path).unwrap().unwrap()["status"], "completed");
        fs::remove_dir_all(directory).unwrap();
    }
}
