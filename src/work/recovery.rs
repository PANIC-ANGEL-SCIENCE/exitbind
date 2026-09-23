use crate::config::Loaded;
use serde_json::{json, Value};

pub(super) type Candidate = (String, Value, Value, String, Value, Value);

pub(super) fn candidate_command(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let config = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    Ok(candidate_command_for_config(config, work))
}

fn candidate_command_for_config(config: &str, work: &str) -> Value {
    json!([
        crate::compatibility::profile().caller,
        "work",
        "next",
        work,
        "--json",
        "--config",
        config,
    ])
}

pub(super) fn compact_progress(progress: &Value) -> Value {
    json!({
        "state": progress["state"],
        "percent": progress["percent"],
        "reason": progress["reason"],
    })
}

pub(super) fn unreadable_candidate(
    loaded: &Loaded,
    work: &str,
    ledger: &str,
    error: &str,
) -> Result<Value, String> {
    Ok(json!({
        "work": work,
        "ledger": ledger,
        "reason": classify_discovery_error(error),
        "error": error,
        "command": inspect_command(loaded, ledger)?,
    }))
}

pub(super) fn inspect_command(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    let config = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    Ok(inspect_command_for_config(config, ledger))
}

pub(super) fn inspect_command_for_config(config: &str, ledger: &str) -> Value {
    json!([
        crate::compatibility::profile().caller,
        "run",
        "inspect",
        ledger,
        "--json",
        "--config",
        config,
    ])
}

fn classify_discovery_error(error: &str) -> &'static str {
    if error.contains("invalid run ledger") || error.contains("no events") {
        "corrupt_ledger"
    } else if error.contains("artifact") || error.contains("receipt") {
        "evidence_unreadable"
    } else {
        "candidate_unreadable"
    }
}

pub(super) fn work_identity(loaded: &Loaded, ledger: Option<&str>) -> Result<Value, String> {
    let recorder = crate::producer::evidence();
    let ledger_producer = ledger
        .map(|path| {
            crate::run::ledger::load(loaded, path).map(|(_, events, _)| {
                events
                    .first()
                    .and_then(|event| event.get("producer"))
                    .cloned()
                    .unwrap_or(Value::Null)
            })
        })
        .transpose()?
        .unwrap_or(Value::Null);
    let recorder_limitation = older_recorder(&ledger_producer, &recorder);
    Ok(json!({
        "recorder": recorder,
        "ledgerProducer": ledger_producer,
        "recorderLimitation": recorder_limitation,
    }))
}

fn older_recorder(ledger: &Value, recorder: &Value) -> bool {
    let Some(ledger_version) = version_order(ledger["version"].as_str()) else {
        return false;
    };
    let Some(recorder_version) = version_order(recorder["version"].as_str()) else {
        return false;
    };
    ledger_version > recorder_version
}

fn version_order(value: Option<&str>) -> Option<(u64, u64, u64, u8, u64)> {
    let value = value?;
    let (core, prerelease) = value
        .split_once('-')
        .map_or((value, None), |(core, pre)| (core, Some(pre)));
    let mut parts = core.split('.');
    let pre_rank = u8::from(prerelease.is_none());
    let pre_number = prerelease
        .and_then(|pre| pre.rsplit('.').next())
        .and_then(|part| part.parse().ok())
        .unwrap_or(0);
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        pre_rank,
        pre_number,
    ))
}

pub(super) fn add_identity(value: &mut Value, identity: &Value) {
    if let (Some(target), Some(source)) = (value.as_object_mut(), identity.as_object()) {
        for key in ["recorder", "ledgerProducer", "recorderLimitation"] {
            if let Some(item) = source.get(key) {
                target.insert(key.to_owned(), item.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{candidate_command_for_config, compact_progress, older_recorder};
    use serde_json::json;

    #[test]
    fn ambiguous_candidate_command_is_executable_argv() {
        assert_eq!(
            candidate_command_for_config("/project/exitbind.json", "smw_abc"),
            json!([
                crate::compatibility::profile().caller,
                "work",
                "next",
                "smw_abc",
                "--json",
                "--config",
                "/project/exitbind.json",
            ])
        );
    }

    #[test]
    fn newer_ledger_producer_marks_recorder_limitation() {
        assert!(older_recorder(
            &json!({"version": "99.0.0"}),
            &json!({"version": env!("CARGO_PKG_VERSION")})
        ));
        assert!(!older_recorder(
            &json!({"version": env!("CARGO_PKG_VERSION")}),
            &json!({"version": env!("CARGO_PKG_VERSION")})
        ));
        assert!(older_recorder(
            &json!({"version": "0.25.0-rc.2"}),
            &json!({"version": "0.25.0-rc.1"})
        ));
    }

    #[test]
    fn compact_progress_keeps_only_recovery_state_fields() {
        let value = compact_progress(&json!({
            "state": "IN_PROGRESS",
            "percent": 20,
            "reason": {"code": "pending"},
            "components": {"worker": {"total": 25}},
        }));
        assert_eq!(
            value,
            json!({
                "state": "IN_PROGRESS",
                "percent": 20,
                "reason": {"code": "pending"},
            })
        );
    }
}
