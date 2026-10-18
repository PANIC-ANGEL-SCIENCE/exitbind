//! CLI coverage for the one known-ended quarantine follow-up.
use super::*;
use serde_json::{json, Value};
use std::fs;

fn known_ended_first_verification(c: &Case) -> String {
    c.accept_feature();
    let preview = c.preview(REQUEST);
    assert!(preview.status.success(), "{preview:?}");
    let started = c.apply(&value(&preview));
    assert!(started.status.success(), "{started:?}");
    let work = value(&started)["work"].as_str().unwrap().to_owned();
    c.return_frozen(&work);
    let failed =
        c.f.call(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    assert!(
        !failed.status.success(),
        "expected a nonzero observed check: {failed:?}"
    );
    let events = c.f.events(&work);
    let check = events.last().unwrap();
    assert_eq!(check["action"], "check");
    assert_eq!(check["acquisition"], "observed");
    assert_eq!(check["result"]["code"], 7);
    work
}

fn custody_receipt(c: &Case, exit: i64) -> Value {
    custody_receipt_with_observer(c, exit, digest(OBSERVER))
}

fn custody_receipt_with_observer(c: &Case, exit: i64, observer: String) -> Value {
    let directory = c.f.root.join(".exitbind/private-evidence");
    fs::create_dir_all(&directory).unwrap();
    let job_root =
        c.f.root
            .join("fresh-parent/child-job")
            .to_string_lossy()
            .into_owned();
    let receipt = json!({
        "requestId":"controlled-custody-receipt",
        "uid":FIXTURE_UID,
        "jobRoot":job_root,
        "inside":{"environment":{"PWD":job_root},
            "argv":["controlled-observer","--manifest",
                c.f.root.join("fixture-manifest.json").to_string_lossy(),
                "--manifest-sha256",digest(b"fixture input manifest")]},
        "adapterSha256":observer,
        "rootIdentityPreserved":true,
        "ownedExecutionEnded":true,
        "namespaceReaperWaitedByPidfd":true,
        "remainingNamespaceMembers":[],
        "failure":null,
        "timedOut":false,
        "cancelled":false,
        "launcherExit":exit,
        "argv":["controlled-observer","--manifest",
            c.f.root.join("fixture-manifest.json").to_string_lossy(),
            "--manifest-sha256",digest(b"fixture input manifest")]
    });
    let bytes = serde_json::to_vec(&receipt).unwrap();
    fs::write(directory.join("custody-receipt.json"), &bytes).unwrap();
    json!({"root":"state","path":".exitbind/private-evidence/custody-receipt.json",
        "sha256":digest(&bytes),"bytes":bytes.len()})
}

fn accepted_followup_feature(
    c: &mut Case,
    custody: &Value,
    override_plan: Option<Value>,
) -> String {
    let begin = c.config_call(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Review retained known-ended custody evidence",
            "--check-command",
            "exec true",
            "--review-policy",
            "required",
        ],
        b"",
    );
    assert!(begin.status.success(), "{begin:?}");
    let begun = value(&begin);
    let work = begun["work"].as_str().unwrap().to_owned();
    c.feature = work.clone();
    let lead = begun["next"].clone();
    let scoped = c.config_return(
        &lead,
        "scoped",
        b"Scope the exact failed first verification",
    );
    let worker = scoped["next"].clone();
    let inspection = c.config_call(
        &[
            "run",
            "inspect",
            &format!(".exitbind/runs/work-{}.jsonl", &work[4..]),
            "--json",
        ],
        b"",
    );
    assert!(inspection.status.success(), "{inspection:?}");
    let basis = json!({"version":1,"constraints":["Use the exact retained receipt"],
        "openZones":[],"decisiveCases":["Reject mismatched custody identity"]});
    let plan = override_plan.unwrap_or_else(|| {
        json!({
            "environment":{"kind":"controlled-local-check","executorUid":FIXTURE_UID,
                "observerSha256":digest(OBSERVER),
                "freshParent":c.f.root.join("fresh-parent").to_string_lossy(),
                "inputManifest":c.f.root.join("fixture-manifest.json").to_string_lossy(),
                "inputManifestSha256":digest(b"fixture input manifest")},
            "effectInventory":{"write":"none","scope":"fixture only"},
            "artifactSha256":digest(FROZEN),
            "inputsSha256":value(&inspection)["inputsSha256"],
            "basis":basis,
            "command":"exec sh fail-checker.sh",
            "custodyEvidence":custody
        })
    });
    let report = json!({"kind":"quarantine-verification-plan","protocol":1,"plan":plan});
    c.config_return(&worker, "completed", &serde_json::to_vec(&report).unwrap());
    let feature = work;
    let check = c.config_call(&["work", "check", &feature, "--timeout-ms", "2000"], b"");
    assert!(check.status.success(), "{check:?}");
    let output = c.config_call(&["work", "next", &feature, "--full"], b"");
    assert!(output.status.success(), "{output:?}");
    let reviewer = value(&output)["next"].clone();
    assert_eq!(reviewer["role"], "reviewer");
    let lead =
        c.config_return(&reviewer, "approved", b"Reviewed exact custody plan")["next"].clone();
    c.config_return(&lead, "accepted", b"Accepted exact custody plan");
    feature
}

fn followup_preview(c: &Case, work: &str, request: &str) -> Output {
    let detail = c.f.ok(&["work", "detail", work, "--json"], b"");
    c.f.call(
        &[
            "work",
            "quarantine-followup",
            work,
            "--feature-config",
            c.feature_config.to_str().unwrap(),
            "--feature-work",
            &c.feature,
            "--artifact-root",
            "state",
            "--artifact",
            ".exitbind/frozen-subject.txt",
            "--reason",
            REASON,
            "--request-id",
            request,
            "--current-binding",
            detail["actionForms"]["binding"].as_str().unwrap(),
            "--owner-config",
            c.owner_config.to_str().unwrap(),
            "--json",
        ],
        b"",
    )
}

fn legacy_reader_refuses_current_quarantine_action(c: &Case, work: &str) {
    let reader = std::env::var_os("EXITBIND_LEGACY_READER");
    let log = std::env::var_os("EXITBIND_LEGACY_PROBE_LOG");
    if reader.is_none() && log.is_none() {
        return;
    }
    let reader = reader.expect("legacy reader and probe log must be supplied together");
    let log = log.expect("legacy reader and probe log must be supplied together");
    let detail = c.f.ok(&["work", "detail", work, "--view", "action"], b"");
    let forms = detail
        .get("actionForms")
        .unwrap_or(&detail["effectiveAction"]);
    let scoped = forms["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["label"] == "scoped")
        .unwrap_or_else(|| panic!("protocol-2 Work has no current scoped action: {detail}"));
    let argv = scoped["command"]["argv"]
        .as_array()
        .expect("current action form must expose exact argv");
    let mut command = Command::new(&reader);
    command
        .current_dir(&c.f.root)
        .args(argv.iter().skip(1).map(|value| value.as_str().unwrap()))
        .env("EXITBIND_NO_UPDATE_CHECK", "1");
    c.f.native_environment(&mut command);
    let output = command.output().unwrap();
    assert!(
        !output.status.success(),
        "legacy reader accepted protocol-2 action: {output:?}"
    );
    let refusal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    assert!(
        refusal.contains("quarantine")
            && (refusal.contains("invalid") || refusal.contains("unsupported")),
        "legacy refusal was not caused by the protocol-2 marker: {output:?}"
    );
    let exit_code = output
        .status
        .code()
        .expect("legacy refusal must exit normally");
    assert!(exit_code > 0);
    let reader_sha256 = digest(&fs::read(&reader).unwrap());
    let mut evidence = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .unwrap();
    let mut line = serde_json::to_vec(
        &json!({"kind":"quarantine-followup","readerSha256":reader_sha256,"exitCode":exit_code}),
    )
    .unwrap();
    line.push(b'\n');
    evidence.write_all(&line).unwrap();
}

#[test]
fn quarantine_followup_accepts_reviewed_retained_custody_once_and_replays_exact_work() {
    let mut c = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&c);
    let first_bytes = fs::read(c.f.ledger(&first)).unwrap();
    let custody = custody_receipt(&c, 7);
    accepted_followup_feature(&mut c, &custody, None);
    let preview = followup_preview(&c, &first, "followup-once");
    assert!(preview.status.success(), "{preview:?}");
    let applied = c.apply(&value(&preview));
    assert!(applied.status.success(), "{applied:?}");
    let followup = value(&applied)["work"].as_str().unwrap().to_owned();
    legacy_reader_refuses_current_quarantine_action(&c, &followup);
    let start = &c.f.events(&followup)[0];
    assert_eq!(start["quarantineVerification"]["protocol"], 2);
    assert_eq!(
        start["quarantineVerification"]["followUp"]["custodyEvidence"],
        custody
    );
    assert_eq!(fs::read(c.f.ledger(&first)).unwrap(), first_bytes);
    assert_eq!(fs::read(c.f.ledger(&c.old)).unwrap(), c.old_bytes);

    let replay = followup_preview(&c, &first, "followup-once");
    assert!(replay.status.success(), "{replay:?}");
    let repeated = c.apply(&value(&replay));
    assert!(repeated.status.success(), "{repeated:?}");
    assert_eq!(value(&repeated)["work"], followup);

    let changed = followup_preview(&c, &first, "followup-changed-request");
    if changed.status.success() {
        assert!(!c.apply(&value(&changed)).status.success());
    }
}

#[test]
fn quarantine_followup_preserves_the_configured_leads_blocked_return_after_failure() {
    let mut c = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&c);
    let check = c.f.events(&first).last().unwrap().clone();
    let lead = c.f.next(&first);
    assert_eq!(lead["role"], "lead");
    let returned = c.f.give(&first, &lead, "blocked");
    assert_eq!(returned["event"]["action"], "submit");
    assert_eq!(returned["event"]["role"], "lead");
    assert_eq!(returned["event"]["agent"], "lead");
    assert_eq!(returned["event"]["outcome"], "blocked");
    assert_eq!(
        returned["event"]["previousEventSha256"],
        check["eventSha256"]
    );
    let blocked_state = c.f.ok(
        &[
            "run",
            "inspect",
            &format!(".exitbind/runs/work-{}.jsonl", &first[4..]),
            "--json",
        ],
        b"",
    );
    assert_eq!(blocked_state["status"], "blocked");
    assert_eq!(
        blocked_state["currentHeadEventSha256"],
        returned["eventSha256"]
    );
    let first_bytes = fs::read(c.f.ledger(&first)).unwrap();
    let custody = custody_receipt(&c, 7);
    accepted_followup_feature(&mut c, &custody, None);
    let preview = followup_preview(&c, &first, "followup-after-lead-blocked");
    assert!(preview.status.success(), "{preview:?}");
    let applied = c.apply(&value(&preview));
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(fs::read(c.f.ledger(&first)).unwrap(), first_bytes);
}

#[test]
fn quarantine_followup_refuses_missing_or_changed_receipt_bytes_before_start() {
    let mut c = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&c);
    let missing = json!({"root":"state","path":".exitbind/private-evidence/missing-custody.json",
        "sha256":digest(b"missing"),"bytes":7});
    accepted_followup_feature(&mut c, &missing, None);
    let before = fs::read(c.f.ledger(&first)).unwrap();
    assert!(!followup_preview(&c, &first, "missing-receipt")
        .status
        .success());
    assert_eq!(fs::read(c.f.ledger(&first)).unwrap(), before);

    let mut changed = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&changed);
    let custody = custody_receipt(&changed, 7);
    accepted_followup_feature(&mut changed, &custody, None);
    let preview = followup_preview(&changed, &first, "changed-receipt");
    assert!(preview.status.success(), "{preview:?}");
    let first_bytes = fs::read(changed.f.ledger(&first)).unwrap();
    fs::write(
        changed.f.root.join(custody["path"].as_str().unwrap()),
        b"altered receipt",
    )
    .unwrap();
    assert!(!changed.apply(&value(&preview)).status.success());
    assert_eq!(fs::read(changed.f.ledger(&first)).unwrap(), first_bytes);
}

#[test]
fn quarantine_followup_refuses_receipt_for_another_observer() {
    let mut c = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&c);
    let custody = custody_receipt_with_observer(&c, 7, digest(b"other observer"));
    accepted_followup_feature(&mut c, &custody, None);
    let first_bytes = fs::read(c.f.ledger(&first)).unwrap();
    assert!(!followup_preview(&c, &first, "foreign-observer")
        .status
        .success());
    assert_eq!(fs::read(c.f.ledger(&first)).unwrap(), first_bytes);
}

#[test]
fn quarantine_followup_refuses_receipt_for_another_input_manifest() {
    let mut c = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&c);
    let mut custody = custody_receipt(&c, 7);
    let path = c.f.root.join(custody["path"].as_str().unwrap());
    let mut receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt["argv"][2] = json!("/another/manifest.json");
    receipt["argv"][4] = json!(digest(b"another input manifest"));
    let argv = receipt["argv"].clone();
    receipt["inside"]["argv"] = argv;
    let bytes = serde_json::to_vec(&receipt).unwrap();
    fs::write(&path, &bytes).unwrap();
    custody["sha256"] = json!(digest(&bytes));
    custody["bytes"] = json!(bytes.len());
    accepted_followup_feature(&mut c, &custody, None);
    let before = fs::read(c.f.ledger(&first)).unwrap();
    assert!(!followup_preview(&c, &first, "foreign-manifest")
        .status
        .success());
    assert_eq!(fs::read(c.f.ledger(&first)).unwrap(), before);
}

#[test]
fn quarantine_followup_refuses_receipt_with_a_different_launcher_exit() {
    let mut c = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&c);
    let custody = custody_receipt(&c, 9);
    accepted_followup_feature(&mut c, &custody, None);
    let before = fs::read(c.f.ledger(&first)).unwrap();
    assert!(!followup_preview(&c, &first, "wrong-launcher-exit")
        .status
        .success());
    assert_eq!(fs::read(c.f.ledger(&first)).unwrap(), before);
}

#[test]
fn quarantine_followup_refuses_successful_first_check() {
    let mut c = Case::new();
    c.accept_feature();
    let preview = c.preview(REQUEST);
    assert!(preview.status.success(), "{preview:?}");
    let started = c.apply(&value(&preview));
    assert!(started.status.success(), "{started:?}");
    let first = value(&started)["work"].as_str().unwrap().to_owned();
    c.return_frozen(&first);
    let check =
        c.f.ok(&["work", "check", &first, "--timeout-ms", "2000"], b"");
    assert_eq!(check["exitCode"], 0);
    assert!(!followup_preview(&c, &first, "successful-first-check")
        .status
        .success());
}

#[test]
fn quarantine_followup_refuses_followup_of_followup() {
    let mut c = Case::with_verification_command(true, false, "exec sh fail-checker.sh");
    let first = known_ended_first_verification(&c);
    let custody = custody_receipt(&c, 7);
    accepted_followup_feature(&mut c, &custody, None);
    let preview = followup_preview(&c, &first, "first-followup");
    assert!(preview.status.success(), "{preview:?}");
    let applied = c.apply(&value(&preview));
    assert!(applied.status.success(), "{applied:?}");
    let second = value(&applied)["work"].as_str().unwrap().to_owned();
    c.return_frozen(&second);
    assert!(!c
        .f
        .call(&["work", "check", &second, "--timeout-ms", "2000"], b"")
        .status
        .success());
    assert!(!followup_preview(&c, &second, "second-followup")
        .status
        .success());
}
