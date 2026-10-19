//! Local shipper: reconcile exact bytes before any bounded failed-stage retry.
use super::{current, store};
use crate::{config::Loaded, evidence::hash, project::path};
use serde_json::{json, Value};

pub(super) fn artifact_bytes(loaded: &Loaded, packet: &Value) -> Result<Vec<u8>, String> {
    let artifact = &packet["artifact"];
    let root = match artifact["root"].as_str() {
        Some("state") => &loaded.state_root,
        Some("product") => &loaded.product_root,
        _ => return Err("unknown accepted artifact root".into()),
    };
    let requested = artifact["path"]
        .as_str()
        .ok_or("missing accepted artifact path")?;
    let bytes = match path::secure_bytes_observation_single_link_bounded(
        root,
        requested,
        "accepted ship artifact",
        store::MAX_BYTES,
    ) {
        path::SecureBytesResult::Bytes(bytes) => bytes,
        _ => return Err("accepted ship artifact is unavailable, unsafe or exceeds 256 KiB".into()),
    };
    if artifact["sha256"] != hash::bytes(&bytes) || artifact["bytes"] != bytes.len() as u64 {
        return Err("accepted ship artifact changed".into());
    }
    Ok(bytes)
}

pub(super) fn run(
    loaded: &Loaded,
    id: &str,
    target: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    if !apply {
        return Err("ship consume requires explicit --apply and matching --target; packet is not a destination grant".into());
    }
    let packet = store::packet(loaded, id)?;
    if target.is_none() || packet["authority"]["target"].as_str() != target {
        return Err("shipping destination differs from explicit approval".into());
    }
    let work = packet["work"].as_str().ok_or("packet has no source Work")?;
    let ledger = crate::work::resolve(loaded, work)?;
    let lock = crate::run::ledger::ledger_path(&loaded.state_root, &ledger, false)?;
    crate::run::ledger::with_lock(&lock, || {
        if current(loaded, work)? != packet["source"] {
            return Err("fixed source or acceptance changed; shipment held".into());
        }
        if store::get(loaded, id, "repair")?.is_some() || super::observation::facts(loaded, id)?.1 {
            return status(loaded, id);
        }
        let bytes = artifact_bytes(loaded, &packet)?;
        // A missing reply is resolved by reading the single immutable destination.
        // Never overwrite an existing leaf, including incomplete bytes.
        if let Some(existing) = store::read(loaded, id, "artifact")? {
            if existing != bytes {
                return Err("unknown/partial local effect; held without overwrite or retry".into());
            }
            return status(loaded, id);
        }
        if store::get(loaded, id, "completed")?.is_some() {
            return Err(
                "previously staged artifact disappeared; held without repeating delivery".into(),
            );
        }
        let mut attempt = None;
        for n in 1..=3 {
            if store::get(loaded, id, &format!("attempt-{n}"))?.is_none() {
                attempt = Some(n);
                break;
            }
        }
        let n =
            attempt.ok_or("local infrastructure retry limit reached; inspect retained facts")?;
        let started = chrono::Utc::now();
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let cwd = cwd.to_str().ok_or("shipping cwd is not UTF-8")?;
        let config_path = loaded
            .path
            .to_str()
            .ok_or("shipping config path is not UTF-8")?;
        let intent = json!({"packet":id,"attempt":n,"collector":crate::producer::build_identity(),"cwd":cwd,"config":config_path,
            "tool":"exitbind ship consume","argv":["exitbind","ship","consume",id,"--target",target,"--apply"],"acquisition":"local_observation","startedAt":started.to_rfc3339(),
            "sourceSha256":hash::value(&packet["source"]),"artifactSha256":hash::bytes(&bytes),
            "scope":"StateRoot shipping operation","destination":store::relative(id,"artifact")?,
            "effectBefore":"absent","externalEffects":false});
        store::put(loaded, id, &format!("attempt-{n}"), &intent)?;
        let result = store::create(loaded, id, "artifact", &bytes);
        let actual = store::read(loaded, id, "artifact");
        let effect = match &actual {
            Ok(Some(staged)) if staged == &bytes => "verified_local_staging",
            Ok(None) => "absent",
            _ => "unknown",
        };
        let facts = json!({"packet":id,"attempt":n,"startedAt":started.to_rfc3339(),
            "endedAt":chrono::Utc::now().to_rfc3339(),"durationMs":(chrono::Utc::now()-started).num_milliseconds(),
            "effect":effect,"status":if result.is_ok(){"completed"}else{"failed"},
            "capture":"complete","signal":Value::Null,"subprocess":false,
            "error":result.err(),"artifactSha256":hash::bytes(&bytes),"bytes":bytes.len(),
            "streams":"not_applicable_in_process_copy","acquisition":"local_observation"});
        store::put(loaded, id, &format!("result-{n}"), &facts)?;
        if effect == "verified_local_staging" {
            store::put(
                loaded,
                id,
                "completed",
                &json!({"packet":id,"attempt":n,"sha256":hash::bytes(&bytes)}),
            )?;
        }
        status(loaded, id)
    })
}

pub(super) fn status(loaded: &Loaded, id: &str) -> Result<Value, String> {
    let packet = store::packet(loaded, id)?;
    let source_current = packet["work"]
        .as_str()
        .and_then(|work| current(loaded, work).ok())
        .is_some_and(|source| source == packet["source"]);
    let repair = store::get(loaded, id, "repair")?;
    let (provider_facts, defect) = super::observation::facts(loaded, id)?;
    let held_defect = repair.is_some() || defect;
    let mut attempts = Vec::new();
    let mut complete = true;
    for n in 1..=3 {
        if store::get(loaded, id, &format!("attempt-{n}"))?.is_some() {
            let result = store::get(loaded, id, &format!("result-{n}"))?;
            complete &= result.is_some();
            // Public projection omits private errors, raw arguments and paths.
            attempts.push(
                json!({"attempt":n,"status":result.as_ref().map(|v| &v["status"]),
                "effect":result.as_ref().map(|v| &v["effect"])}),
            );
        }
    }
    let staged = store::read(loaded, id, "artifact");
    let effect = match &staged {
        Ok(Some(bytes)) if packet["artifact"]["sha256"] == hash::bytes(bytes) => {
            "verified_local_staging"
        }
        Ok(None) => "absent",
        _ => "unknown",
    };
    let completed = store::get(loaded, id, "completed")?.is_some();
    let state = if held_defect {
        "held_product_defect"
    } else if !source_current {
        "held_stale_source"
    } else if effect == "unknown" || (completed && effect == "absent") {
        "held_unknown_effect"
    } else if effect == "verified_local_staging" && (!complete || !completed) {
        "staged_capture_incomplete"
    } else if effect == "verified_local_staging" {
        "locally_staged"
    } else if attempts.is_empty() {
        "handed_off"
    } else {
        "held_infrastructure"
    };
    Ok(
        json!({"version":1,"id":id,"state":state,"sourceAccepted":if source_current {json!(true)}else{Value::Null},"sourceCurrent":source_current,
        "effect":effect,"captureComplete":complete && (effect != "verified_local_staging" || completed),
        "artifactSha256":packet["artifact"]["sha256"],"attempts":attempts,
        "repairRequest":repair.as_ref().map(|r| &r["id"]),"repairRecordPending":defect && repair.is_none(),"providerFacts":provider_facts,
        "report":{"projection":"safe_mechanical_facts","sourceIdentity":hash::value(&packet["source"]),
            "deliveryScope":"task_owned_local_staging","liveShipment":false,"installedPlatformVerification":false,
            "overallUserDelivery":"not_established","causalJudgment":"shipper_supplied_separately",
            "nextStep":if held_defect{"separately_scope_repair"}else if state=="locally_staged"{"return_local_delivery_report"}else{"inspect_facts_before_retry"}},
        "privateFacts":store::relative(id,"packet")?,"providerEvidence":"imported observations never count as local delivery or acceptance"}),
    )
}
