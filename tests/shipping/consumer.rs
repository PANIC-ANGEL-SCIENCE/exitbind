//! Each call is a new process with only the packet identity and current config.
use super::*;
use sha2::{Digest, Sha256};

fn packet(f: &Fixture, work: &str, target: &str) -> Value {
    f.json(
        &["ship", "prepare", work, "--target", target, "--apply"],
        b"",
    )
}
fn consume(f: &Fixture, id: &str, target: &str) -> Value {
    f.json(&["ship", "consume", id, "--target", target, "--apply"], b"")
}
fn dir(f: &Fixture, id: &str) -> PathBuf {
    f.root.join(".exitbind/shipping").join(id)
}
fn observe(f: &Fixture, id: &str, class: &str, status: &str, conclusion: Value) -> Output {
    let status_view = f.json(&["ship", "status", id], b"");
    let v = json!({"id":"observation-1","classification":class,"sourceSha256":status_view["report"]["sourceIdentity"],
        "provider":"controlled-fixture","runId":"fixture-run-1","status":status,"conclusion":conclusion,
        "evidence":"task-local controlled observation", "reason":"explicit shipper classification"});
    f.call(
        &["ship", "observe", id, "--apply"],
        &serde_json::to_vec(&v).unwrap(),
    )
}
#[test]
fn fresh_consumer_delivers_exact_accepted_artifact_and_lost_reply_is_noop() {
    let f = Fixture::new("ship-consumer");
    let (work, accepted) = f.accepted();
    let ledger = f
        .root
        .join(accepted["reference"]["ledger"].as_str().unwrap());
    let before = fs::read(&ledger).unwrap();
    let p = packet(&f, &work, "task-output");
    let id = p["id"].as_str().unwrap();
    assert_eq!(packet(&f, &work, "task-output")["id"], id);
    let result = consume(&f, id, "task-output");
    assert_eq!(result["state"], "locally_staged");
    let bytes = fs::read(dir(&f, id).join("artifact")).unwrap();
    assert_eq!(
        result["artifactSha256"],
        format!("{:x}", Sha256::digest(bytes))
    );
    let before_files = fs::read_dir(dir(&f, id)).unwrap().count();
    assert_eq!(consume(&f, id, "task-output"), result);
    assert_eq!(packet(&f, &work, "task-output")["state"], "locally_staged");
    assert_eq!(fs::read_dir(dir(&f, id)).unwrap().count(), before_files);
    assert_eq!(fs::read(ledger).unwrap(), before);
    assert_eq!(result["report"]["liveShipment"], false);
    assert_eq!(result["attempts"].as_array().unwrap().len(), 1);
}
#[test]
fn rights_staleness_and_packet_mutation_refuse_without_effect() {
    let f = Fixture::new("ship-rights");
    let pending = f.begin();
    assert!(!f
        .call(
            &["ship", "prepare", &pending, "--target", "x", "--apply"],
            b""
        )
        .status
        .success());
    let (work, _) = f.accepted();
    let p = packet(&f, &work, "approved");
    let id = p["id"].as_str().unwrap();
    for args in [
        vec!["ship", "consume", id, "--target", "approved"],
        vec!["ship", "consume", id, "--target", "wrong", "--apply"],
    ] {
        assert!(!f.call(&args, b"").status.success());
    }
    assert!(!dir(&f, id).join("artifact").exists());
    let stored = dir(&f, id).join("packet");
    let original = fs::read(&stored).unwrap();
    let mut v: Value = serde_json::from_slice(&original).unwrap();
    v["authority"]["target"] = json!("wrong");
    fs::write(&stored, serde_json::to_vec(&v).unwrap()).unwrap();
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "wrong", "--apply"],
            b""
        )
        .status
        .success());
    fs::write(&stored, original).unwrap();
    fs::write(f.root.join("stale-source.txt"), "new source").unwrap();
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "approved", "--apply"],
            b""
        )
        .status
        .success());
}
#[test]
fn product_defect_holds_separate_request_without_reopening_source() {
    let f = Fixture::new("ship-defect");
    let (work, accepted) = f.accepted();
    let p = packet(&f, &work, "defective");
    let id = p["id"].as_str().unwrap();
    let ledger = f
        .root
        .join(accepted["reference"]["ledger"].as_str().unwrap());
    let before = fs::read(&ledger).unwrap();
    assert!(
        observe(&f, id, "product-defect", "completed", json!("failure"))
            .status
            .success()
    );
    let result = consume(&f, id, "defective");
    assert_eq!(result["state"], "held_product_defect");
    assert_eq!(
        packet(&f, &work, "defective")["state"],
        "held_product_defect"
    );
    assert!(!dir(&f, id).join("artifact").exists());
    let repair: Value =
        serde_json::from_slice(&fs::read(dir(&f, id).join("repair")).unwrap()).unwrap();
    assert_eq!(repair["state"], "requested_not_authorized");
    assert_eq!(fs::read(ledger).unwrap(), before);
}
#[test]
fn provider_success_never_substitutes_for_delivery_or_pending_run() {
    let f = Fixture::new("ship-provider");
    let (work, _) = f.accepted();
    let p = packet(&f, &work, "provider");
    let id = p["id"].as_str().unwrap();
    assert!(
        !observe(&f, id, "provider", "in_progress", json!("success"))
            .status
            .success()
    );
    assert!(observe(&f, id, "provider", "completed", json!("failure"))
        .status
        .success());
    assert_eq!(f.json(&["ship", "status", id], b"")["state"], "handed_off");
    assert!(!dir(&f, id).join("artifact").exists());
}
#[test]
fn unknown_partial_and_missing_completion_do_not_replay() {
    let f = Fixture::new("ship-partial");
    let (work, _) = f.accepted();
    let p = packet(&f, &work, "partial");
    let id = p["id"].as_str().unwrap();
    fs::write(dir(&f, id).join("artifact"), b"partial").unwrap();
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "partial", "--apply"],
            b""
        )
        .status
        .success());
    assert_eq!(
        f.json(&["ship", "status", id], b"")["state"],
        "held_unknown_effect"
    );
    fs::remove_file(dir(&f, id).join("artifact")).unwrap();
    consume(&f, id, "partial");
    fs::remove_file(dir(&f, id).join("completed")).unwrap();
    let result = consume(&f, id, "partial");
    assert_eq!(result["state"], "staged_capture_incomplete");
    assert_eq!(result["attempts"].as_array().unwrap().len(), 1);
}
#[test]
fn symlink_and_hardlink_destinations_are_refused() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("ship-alias");
    let (work, _) = f.accepted();
    let p = packet(&f, &work, "aliases");
    let id = p["id"].as_str().unwrap();
    let outside = f.root.join(".exitbind/private-fixture");
    fs::write(&outside, b"preserved").unwrap();
    symlink(&outside, dir(&f, id).join("artifact")).unwrap();
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "aliases", "--apply"],
            b""
        )
        .status
        .success());
    fs::remove_file(dir(&f, id).join("artifact")).unwrap();
    fs::hard_link(&outside, dir(&f, id).join("artifact")).unwrap();
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "aliases", "--apply"],
            b""
        )
        .status
        .success());
    assert_eq!(fs::read(&outside).unwrap(), b"preserved");
}

#[test]
fn bounded_failed_stage_retry_and_lost_intent_never_reopen_work() {
    let f = Fixture::new("ship-infrastructure");
    let (work, accepted) = f.accepted();
    let p = packet(&f, &work, "retry");
    let id = p["id"].as_str().unwrap();
    let ledger = f
        .root
        .join(accepted["reference"]["ledger"].as_str().unwrap());
    let before = fs::read(&ledger).unwrap();
    // Controlled retained failed-stage fixture: effect definitively absent.
    fs::write(dir(&f, id).join("attempt-1"), br#"{"attempt":1}"#).unwrap();
    fs::write(
        dir(&f, id).join("result-1"),
        br#"{"status":"failed","effect":"absent"}"#,
    )
    .unwrap();
    assert_eq!(
        f.json(&["ship", "status", id], b"")["state"],
        "held_infrastructure"
    );
    let result = consume(&f, id, "retry");
    assert_eq!(result["state"], "locally_staged");
    assert_eq!(result["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(fs::read(ledger).unwrap(), before);
    // Missing delivered bytes do not license a second effect.
    fs::remove_file(dir(&f, id).join("artifact")).unwrap();
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "retry", "--apply"],
            b""
        )
        .status
        .success());
}

#[test]
fn exhausted_attempts_and_invalid_observations_hold() {
    let f = Fixture::new("ship-exhausted");
    let (work, _) = f.accepted();
    let p = packet(&f, &work, "exhausted");
    let id = p["id"].as_str().unwrap();
    for n in 1..=3 {
        fs::write(
            dir(&f, id).join(format!("attempt-{n}")),
            br#"{"effect":"unknown"}"#,
        )
        .unwrap();
    }
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "exhausted", "--apply"],
            b""
        )
        .status
        .success());
    assert!(!f
        .call(&["ship", "observe", id, "--apply"], &vec![b'x'; 8193])
        .status
        .success());
    assert!(!dir(&f, id).join("artifact").exists());
}

#[test]
#[cfg(feature = "repository-checks")]
fn repository_provider_adapter_preserves_provider_status() {
    let result = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tooling/repository/release/ship-provider-observation.py"
        ))
        .arg("--self-test")
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
}

#[test]
fn lost_repair_record_cannot_release_a_committed_defect_hold() {
    let f = Fixture::new("ship-repair-loss");
    let (work, _) = f.accepted();
    let p = packet(&f, &work, "held");
    let id = p["id"].as_str().unwrap();
    assert!(
        observe(&f, id, "product-defect", "completed", json!("failure"))
            .status
            .success()
    );
    fs::remove_file(dir(&f, id).join("repair")).unwrap();
    let result = consume(&f, id, "held");
    assert_eq!(result["state"], "held_product_defect");
    assert_eq!(result["repairRecordPending"], true);
    assert!(!dir(&f, id).join("artifact").exists());
    assert!(
        observe(&f, id, "product-defect", "completed", json!("failure"))
            .status
            .success()
    );
    assert!(dir(&f, id).join("repair").is_file());
}

#[test]
fn actual_local_storage_failure_retains_partial_effect_and_prevents_replay() {
    use std::os::unix::process::CommandExt;
    let f = Fixture::new("ship-storage-failure");
    let work = f.begin();
    f.outcome(&work, "scoped");
    let detail = f.detail(&work);
    let mut permit = detail["actionForms"]["beforeEditing"]["command"].clone();
    for arg in permit["argv"].as_array_mut().unwrap() {
        if arg == "<OPERATION>" {
            *arg = json!("controlled local artifact delivery");
        }
    }
    assert!(f.execute(&work, &permit, b"").status.success());
    let detail = f.detail(&work);
    let mut command = detail["actionForms"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == "completed")
        .unwrap()["command"]
        .clone();
    for arg in command["argv"].as_array_mut().unwrap() {
        if arg == "<REASON>" {
            *arg = json!("bounded retained payload for storage-failure qualification");
        }
    }
    assert!(f
        .execute(&work, &command, &vec![b'x'; 16 * 1024])
        .status
        .success());
    let detail = f.detail(&work);
    assert!(f
        .execute(
            &work,
            &detail["actionForms"]["mechanicalAction"]["command"],
            b""
        )
        .status
        .success());
    f.outcome(&work, "approved");
    f.outcome(&work, "accepted");
    let p = packet(&f, &work, "storage");
    let id = p["id"].as_str().unwrap();
    let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    child
        .current_dir(&f.root)
        .args([
            "ship", "consume", id, "--target", "storage", "--apply", "--config",
        ])
        .arg(f.root.join("exitbind.json"));
    // Limit only this owned child, never the host or another process.
    unsafe {
        child.pre_exec(|| {
            let no_core = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &no_core) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let limit = libc::rlimit {
                rlim_cur: 4096,
                rlim_max: 4096,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let failed = child.output().unwrap();
    assert!(!failed.status.success(), "{failed:?}");
    let status = f.json(&["ship", "status", id], b"");
    assert_eq!(status["state"], "held_unknown_effect");
    assert_eq!(status["repairRequest"], Value::Null);
    assert_eq!(
        fs::metadata(dir(&f, id).join("artifact")).unwrap().len(),
        4096
    );
    assert!(!f
        .call(
            &["ship", "consume", id, "--target", "storage", "--apply"],
            b""
        )
        .status
        .success());
    assert_eq!(
        f.json(&["ship", "status", id], b"")["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
