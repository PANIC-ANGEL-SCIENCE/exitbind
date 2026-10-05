//! Optional Work evidence export over the canonical Exit Path receipt owner.
//! No run events, acceptance decisions or mandatory completion step live here.

use crate::{
    config::Loaded,
    evidence::{hash, receipt},
    project::path,
    run::RunSnapshot,
};
use serde_json::{json, Value};
use std::io::Write;

const MAX_RECEIPT_BYTES: u64 = 256 * 1024;

fn command(loaded: &Loaded, work: &str, options: &[&str]) -> Value {
    let mut suffix = vec!["work".into(), "closeout".into(), work.into()];
    suffix.extend(options.iter().map(|option| (*option).into()));
    suffix.extend(["--json".into(), "--config".into()]);
    let route = super::response_recovery::bounded_argv(suffix, loaded.path.to_str(), 768);
    json!({"argv": route.argv, "sameConfigRequired": route.same_config,
        "sameExecutableRequired": route.same_executable})
}

fn current_receipt(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = super::resolve(loaded, work)?;
    let canonical = receipt::exit_path(loaded, &ledger).map_err(|error| error.to_string())?;
    // Historical receipt validation deliberately retains original meaning.
    // Current Work export additionally uses the existing result-reference owner
    // to distinguish prior acceptance from today's applicable tested inputs.
    let evidence = crate::run::result_ref_evidence(loaded, work)?
        .ok_or("accepted Work evidence is unavailable")?;
    if !evidence.accepted || !evidence.artifact_current || evidence.drift.is_some() {
        return Err("accepted Work configuration/profile/artifact evidence is stale".into());
    }
    if evidence.inputs_current != Some(true) {
        return Err("accepted Work tested inputs are stale or have no current binding; historical evidence remains available through the receipt owner".into());
    }
    Ok(canonical)
}

fn bounded(value: Value) -> Result<Value, String> {
    let value = super::effective_action::attach(&value);
    if serde_json::to_vec(&value)
        .map_err(|error| error.to_string())?
        .len()
        + 1
        > super::compact::MAX_RESPONSE_BYTES
    {
        return Err("Work closeout response exceeds its bounded channel".into());
    }
    Ok(value)
}

pub(super) fn projection(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Value {
    let view = snapshot.inspect_view();
    let canonical = current_receipt(loaded, work);
    let available = canonical.is_ok();
    let reason = canonical.err().map(|error| {
        error
            .chars()
            .scan(0, |used, ch| {
                *used += ch.len_utf8();
                (*used <= 512).then_some(ch)
            })
            .collect::<String>()
    });
    json!({"version": 1, "work": work,
        "workAcceptance": {"state": view["status"], "currentEvidence": available,
            "subjectSha256": view["subject"]["sha256"]},
        "goalProgress": super::compact::compact_goal_progress(&next["goalProgress"]),
        "receipt": {"state": if available { "not_requested" } else { "unavailable" },
            "available": available, "reason": reason,
            "export": if available { json!({"command": command(loaded, work, &["--export"]),
                "explicitRequestRequired": true, "advancesWork": false}) } else { Value::Null }},
        "source": {"owner": "receipt::exit_path / verify_exit_path", "runId": view["runId"],
            "ledgerProducer": view["events"][0]["producer"], "recorder": crate::producer::evidence()},
        "detail": {"command": command(loaded, work, &[]), "readOnly": true}})
}

fn relative(loaded: &Loaded, requested: &str) -> Result<String, String> {
    // Existing path ownership converts only an absolute path beneath StateRoot;
    // descriptor-level readers/creators below refuse aliases in every component.
    crate::evidence::receipt_path::state_relative(&loaded.state_root, requested)
}

fn read(loaded: &Loaded, destination: &str) -> Result<Option<Vec<u8>>, String> {
    match path::secure_bytes_observation_single_link_bounded(
        &loaded.state_root,
        destination,
        "Work receipt",
        MAX_RECEIPT_BYTES,
    ) {
        path::SecureBytesResult::Bytes(bytes) => Ok(Some(bytes)),
        path::SecureBytesResult::Absent(_) => Ok(None),
        path::SecureBytesResult::Unsafe(error) | path::SecureBytesResult::Unreadable(error) => {
            Err(error)
        }
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(error) => Err(error),
    }
}

fn verified(loaded: &Loaded, canonical: &Value, bytes: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid Work receipt JSON; refusing to overwrite: {error}"))?;
    if value["run"] != canonical["run"] || value["subject"] != canonical["subject"] {
        return Err(
            "receipt belongs to another Work or an earlier accepted subject; refusing to overwrite"
                .into(),
        );
    }
    let result = receipt::verify_exit_path_bytes(bytes, loaded)?;
    if result["valid"] != true {
        return Err(format!(
            "Work receipt verification failed; refusing to overwrite: {}",
            result["mismatches"]
        ));
    }
    Ok(value["producer"].clone())
}

fn receipt_reply(
    mut result: Value,
    loaded: &Loaded,
    work: &str,
    destination: &str,
    bytes: &[u8],
    producer: &Value,
    created: bool,
) -> Result<Value, String> {
    result["receipt"] = json!({"state": if created { "created_verified" } else { "existing_verified" },
        "available": true, "path": if destination.len() <= 1024 { json!(destination) } else { Value::Null },
        "pathOmitted": destination.len() > 1024, "sha256": hash::bytes(bytes), "producer": producer,
        "verification": {"valid": true, "format": "exit-path-v1", "exactWork": true},
        "verify": {"command": command(loaded, work, &["--receipt", destination]), "readOnly": true}});
    result["effect"] = json!(if created { "exported" } else { "no-change" });
    bounded(result)
}

pub(crate) fn closeout(
    loaded: &Loaded,
    work: &str,
    export: bool,
    output: Option<&str>,
    existing: Option<&str>,
) -> Result<Value, String> {
    if existing.is_some() && (export || output.is_some()) {
        return Err(
            "work closeout --receipt is read-only and cannot be combined with --export or --output"
                .into(),
        );
    }
    let ledger = super::resolve(loaded, work)?;
    let snapshot = RunSnapshot::capture(loaded, &ledger)?;
    let next = super::next_from(loaded, work, &snapshot)?;
    let mut result = projection(loaded, work, &snapshot, &next);
    result["compact"] = json!(true);
    result["effect"] = json!("no-change");
    result["current"] = json!({"action": next["action"], "binding": if next["action"] == "done" { next["current"]["binding"].clone() } else { Value::Null },
        "readiness": next["progress"]["state"]});
    if !export && output.is_none() && existing.is_none() {
        return bounded(result);
    }
    // The canonical owner rechecks exact artifacts, input currentness, review,
    // Lead acceptance and history/schema compatibility before touching output.
    let canonical = current_receipt(loaded, work)?;
    let default = format!(
        "{}/receipts/work-{}.json",
        crate::project::layout_types::state_namespace(),
        work.strip_prefix(super::WORK_PREFIX)
            .ok_or("invalid Work")?
    );
    let destination = relative(loaded, existing.or(output).unwrap_or(&default))?;
    let mut created = false;
    let bytes = match read(loaded, &destination)? {
        Some(bytes) => bytes,
        None if existing.is_some() => return Err("requested Work receipt is missing".into()),
        None => {
            if loaded.state_root == loaded.product_root {
                let private = format!(
                    "{}/receipts",
                    crate::project::layout_types::state_namespace()
                );
                if !std::path::Path::new(&destination).starts_with(&private) {
                    return Err(format!("new Work receipt must be under {private} when StateRoot is ProductRoot; export must not change checked product inputs"));
                }
            }
            let bytes = (serde_json::to_string_pretty(&canonical)
                .map_err(|error| error.to_string())?
                + "\n")
                .into_bytes();
            let producer = verified(loaded, &canonical, &bytes)?;
            // Refuse before creating evidence if its exact reply cannot fit.
            // Preflight replay too: a successful export must remain readable
            // after a lost reply, whose existing-state spelling is longer.
            for created in [true, false] {
                receipt_reply(
                    result.clone(),
                    loaded,
                    work,
                    &destination,
                    &bytes,
                    &producer,
                    created,
                )?;
            }
            #[cfg(unix)]
            match path::secure_create_new(&loaded.state_root, &destination, "Work receipt") {
                Ok(mut file) => {
                    file.write_all(&bytes).map_err(|error| error.to_string())?;
                    file.sync_all().map_err(|error| error.to_string())?;
                    created = true;
                    bytes
                }
                // A concurrent/lost-reply export can only reuse exact verified
                // bytes; no write attempt ever opens an existing leaf.
                Err(error) => read(loaded, &destination)?.ok_or(error)?,
            }
            #[cfg(not(unix))]
            return Err("Work receipt export requires Unix no-follow support".into());
        }
    };
    // A concurrent project/configuration change must not make the reply claim
    // current evidence after an otherwise valid historical receipt was written.
    super::details::ensure_current_config(loaded)?;
    if current_receipt(loaded, work)? != canonical {
        return Err("accepted Work changed during receipt export/verification".into());
    }
    let producer = verified(loaded, &canonical, &bytes)?;
    if read(loaded, &destination)?.as_deref() != Some(bytes.as_slice()) {
        return Err("Work receipt changed during verification".into());
    }
    receipt_reply(
        result,
        loaded,
        work,
        &destination,
        &bytes,
        &producer,
        created,
    )
}
