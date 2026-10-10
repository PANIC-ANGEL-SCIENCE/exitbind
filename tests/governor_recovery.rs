//! Owner-bound recovery for one durable duplicate-permit refusal.
mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    work: String,
    goal: String,
    old_ledger: String,
    prior_ledgers: Vec<(String, Vec<u8>)>,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("owner-governor-recovery");
        support::git_topology::repository(&root);
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let goal = "recover one exact duplicate permit without resetting accounting".to_owned();
        let mut fixture = Self {
            root,
            work: String::new(),
            goal,
            old_ledger: String::new(),
            prior_ledgers: Vec::new(),
        };
        let started = fixture.json(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                &fixture.goal,
                "--check-command",
                "true",
                "--review-policy",
                "required",
            ],
            b"",
        );
        fixture.work = started["work"].as_str().unwrap().to_owned();
        fixture.old_ledger = format!(
            ".exitbind/runs/work-{}.jsonl",
            fixture.work.strip_prefix("smw_").unwrap()
        );
        let detail = fixture.json(&["work", "detail", &fixture.work, "--json"], b"");
        let scope_form = Self::choice(&detail["actionForms"]["choices"], "scoped");
        let scope =
            fixture.execute_form(&scope_form, b"owner-approved recovery regression scope\n");
        assert!(scope.status.success(), "{}", display(&scope));

        // The affected CI01 run is itself a protocol-1 same-goal successor.
        // Reproduce that chain before generating the duplicate-permit block.
        let initial_ledger = fixture.old_ledger.clone();
        fixture.prior_ledgers.push((
            initial_ledger.clone(),
            fs::read(fixture.root.join(&initial_ledger)).unwrap(),
        ));
        let carried_ledger = format!(".exitbind/runs/work-{}.jsonl", "c".repeat(64));
        let carried = fixture.json(
            &[
                "run",
                "supersede",
                &initial_ledger,
                "--workflow",
                "change",
                "--goal",
                &fixture.goal,
                "--ledger",
                &carried_ledger,
            ],
            b"",
        );
        fixture.work = carried["work"].as_str().unwrap().to_owned();
        fixture.old_ledger = carried_ledger;
        let carried_detail = fixture.json(&["work", "detail", &fixture.work, "--json"], b"");
        let carried_scope = Self::choice(&carried_detail["actionForms"]["choices"], "scoped");
        let scope = fixture.execute_form(
            &carried_scope,
            b"preserve current recovery behavior through the protocol-1 carry\n",
        );
        assert!(scope.status.success(), "{}", display(&scope));
        fixture
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn choice(choices: &Value, label: &str) -> Value {
        choices
            .as_array()
            .unwrap()
            .iter()
            .find(|choice| choice["label"] == label)
            .unwrap_or_else(|| panic!("missing {label}: {choices}"))
            .clone()
    }

    fn execute_form(&self, form: &Value, input: &[u8]) -> Output {
        self.execute_form_replacing(form, input, "", "")
    }

    fn execute_form_replacing(&self, form: &Value, input: &[u8], from: &str, to: &str) -> Output {
        let default_target = ".exitbind/runs/work-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jsonl";
        let args = form["command"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| match arg.as_str().unwrap() {
                "<SUCCESSOR_LEDGER>" => {
                    if to.is_empty() {
                        default_target.to_owned()
                    } else {
                        to.to_owned()
                    }
                }
                "<OWNER_RECOVERY_STATE_ARTIFACT>" => ".exitbind/artifacts/recovery.json".to_owned(),
                "<REASON>" => "fixture records the exact scoped decision".to_owned(),
                "<ARTIFACT_ROOT>" => "state".to_owned(),
                "<ARTIFACT_PATH>" => ".exitbind/artifacts/fresh.md".to_owned(),
                "<OPERATION>" => "fresh-evidenced-successor-edit".to_owned(),
                value if !from.is_empty() && value == from => to.to_owned(),
                value => value.to_owned(),
            })
            .collect::<Vec<_>>();
        let mut child = Command::new(&args[0])
            .current_dir(&self.root)
            .args(&args[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn json(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {}", display(&output));
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{args:?} did not return JSON: {error}; stdout={}; stderr={}",
                bounded(&output.stdout),
                bounded(&output.stderr)
            )
        })
    }

    fn permit_edit(&self, assignment: &str) -> Output {
        self.call(
            &[
                "work",
                "permit",
                &self.work,
                assignment,
                "--operation",
                "edit",
            ],
            b"",
        )
    }

    fn sensor_request(&self, assignment: &str) -> Value {
        let output = self.call(&["work", "sensor-request", &self.work, assignment], b"");
        assert!(output.status.success(), "{}", display(&output));
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "sensor request did not return JSON: {error}; {}",
                display(&output)
            )
        })
    }

    fn inert_sensor_result(&self, assignment: &str) -> Value {
        let output = self.call(
            &[
                "work",
                "sensor-result",
                &self.work,
                assignment,
                "--assessment",
                "unavailable",
                "--input-digest",
                &"1".repeat(64),
            ],
            b"",
        );
        assert!(output.status.success(), "{}", display(&output));
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "sensor result did not return JSON: {error}; {}",
                display(&output)
            )
        })
    }

    fn assert_prior_ledgers_unchanged(&self) {
        for (ledger, bytes) in &self.prior_ledgers {
            assert_eq!(
                fs::read(self.root.join(ledger)).unwrap(),
                *bytes,
                "{ledger}"
            );
        }
    }

    fn prepare_replanned_mutation(&self, resolve_sensor: Option<bool>) -> String {
        let worker = self.json(&["work", "next", &self.work, "--full"], b"");
        let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
        for _ in 0..2 {
            let permit = self.call(
                &[
                    "work",
                    "permit",
                    &self.work,
                    &assignment,
                    "--operation",
                    "edit",
                ],
                b"",
            );
            assert!(permit.status.success(), "{}", display(&permit));
        }
        let replanned = self.call(
            &[
                "work",
                "replan",
                &self.work,
                &assignment,
                "--hypothesis",
                "the next bounded edit needs a distinct hypothesis",
            ],
            b"",
        );
        assert!(replanned.status.success(), "{}", display(&replanned));
        if let Some(resolve) = resolve_sensor {
            let before_sensor = self.json(&["work", "next", &self.work, "--full"], b"");
            let before_loop = &before_sensor["next"]["packet"]["context"]["loop"];
            let requested = self.sensor_request(&assignment);
            assert_eq!(
                requested["event"]["governorEvent"]["action"],
                "sensor_request"
            );
            let after_request = self.json(&["work", "next", &self.work, "--full"], b"");
            let request_loop = &after_request["next"]["packet"]["context"]["loop"];
            assert_eq!(request_loop["spent"], before_loop["spent"]);
            assert_eq!(
                request_loop["currentMutation"],
                before_loop["currentMutation"]
            );
            assert_eq!(
                request_loop["currentGrantEventSha256"],
                before_loop["currentGrantEventSha256"]
            );
            if resolve {
                let sensed = self.inert_sensor_result(&assignment);
                assert_eq!(sensed["event"]["governorEvent"]["action"], "sensor");
                let after_result = self.json(&["work", "next", &self.work, "--full"], b"");
                let result_loop = &after_result["next"]["packet"]["context"]["loop"];
                assert_eq!(result_loop["spent"], before_loop["spent"]);
                assert_eq!(
                    result_loop["currentMutation"],
                    before_loop["currentMutation"]
                );
                assert_eq!(
                    result_loop["currentGrantEventSha256"],
                    before_loop["currentGrantEventSha256"]
                );
            }
        }
        let granted = self.call(
            &[
                "work",
                "permit",
                &self.work,
                &assignment,
                "--operation",
                "edit",
            ],
            b"",
        );
        assert!(granted.status.success(), "{}", display(&granted));
        assignment
    }

    fn block_duplicate_permit(&self) -> (Vec<u8>, Value) {
        self.block_duplicate_permit_with_sensor(None)
    }

    fn block_duplicate_permit_with_sensor(&self, sensor: Option<bool>) -> (Vec<u8>, Value) {
        let assignment = self.prepare_replanned_mutation(sensor);
        let worker = self.json(&["work", "next", &self.work, "--full"], b"");
        assert_eq!(
            worker["next"]["packet"]["context"]["loop"]["state"],
            "evidence_required"
        );
        let blocked = self.call(
            &[
                "work",
                "permit",
                &self.work,
                &assignment,
                "--operation",
                "edit",
            ],
            b"",
        );
        assert!(
            !blocked.status.success(),
            "duplicate permit was not blocked"
        );
        let detail = self.json(&["work", "detail", &self.work, "--json"], b"");
        assert_eq!(detail["actionForms"]["recovery"]["state"], "blocked");
        assert!(detail["actionForms"].get("beforeEditing").is_none());
        assert!(detail["actionForms"].get("currentGrant").is_none());
        match sensor {
            Some(false) => {
                assert!(detail["actionForms"]["recovery"]
                    .get("ownerRecoveryDraft")
                    .is_none());
                assert!(detail["actionForms"]["recovery"].get("command").is_none());
            }
            Some(true) => {}
            None => {
                assert_eq!(
                    detail["actionForms"]["recovery"]["ownerDecisionRequired"],
                    true
                );
                assert_eq!(
                    detail["actionForms"]["recovery"]["ownerRecoveryDraft"]["approved"],
                    false
                );
                assert_eq!(
                    detail["actionForms"]["recovery"]["effectsInventory"]["productWrites"],
                    "unknown"
                );
                assert!(detail["actionForms"]["recovery"]["command"]["argv"].is_array());
            }
        }
        let bytes = fs::read(self.root.join(&self.old_ledger)).unwrap();
        (bytes, detail)
    }

    fn block_after_newer_unanswered_sensor(&self) -> (Vec<u8>, Value, String) {
        let worker = self.json(&["work", "next", &self.work, "--full"], b"");
        let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
        let input = self.root.join("sensor-input.txt");
        fs::write(&input, b"initial sensor input\n").unwrap();

        let first_grant = self.permit_edit(&assignment);
        assert!(first_grant.status.success(), "{}", display(&first_grant));
        let first_request = self.sensor_request(&assignment);
        let first_digest = first_request["event"]["governorEvent"]["requestDigest"]
            .as_str()
            .unwrap()
            .to_owned();
        let first_result = self.inert_sensor_result(&assignment);
        assert_eq!(
            first_result["event"]["governorEvent"]["assessment"],
            "unavailable"
        );

        fs::write(&input, b"updated sensor input\n").unwrap();
        let second_grant = self.permit_edit(&assignment);
        assert!(second_grant.status.success(), "{}", display(&second_grant));

        let before_replan = self.json(&["work", "next", &self.work, "--full"], b"");
        assert_eq!(
            before_replan["next"]["packet"]["context"]["loop"]["state"],
            "replan_required"
        );
        let replanned = self.call(
            &[
                "work",
                "replan",
                &self.work,
                &assignment,
                "--hypothesis",
                "the newer sensor question set still needs an answer",
            ],
            b"",
        );
        assert!(replanned.status.success(), "{}", display(&replanned));

        let second_request = self.sensor_request(&assignment);
        let second_digest = second_request["event"]["governorEvent"]["requestDigest"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(first_digest, second_digest);
        let final_grant = self.permit_edit(&assignment);
        assert!(final_grant.status.success(), "{}", display(&final_grant));
        let pending = self.json(&["work", "next", &self.work, "--full"], b"");
        assert_eq!(
            pending["next"]["packet"]["context"]["loop"]["state"],
            "evidence_required"
        );
        let blocked = self.permit_edit(&assignment);
        assert!(
            !blocked.status.success(),
            "duplicate permit was not blocked"
        );
        let detail = self.json(&["work", "detail", &self.work, "--json"], b"");
        assert!(detail["actionForms"]["recovery"]
            .get("ownerRecoveryDraft")
            .is_none());
        let bytes = fs::read(self.root.join(&self.old_ledger)).unwrap();
        (bytes, detail, second_digest)
    }

    fn write_recovery_artifacts(&self, draft: &Value) -> String {
        let artifacts = self.root.join(".exitbind/artifacts");
        fs::create_dir_all(&artifacts).unwrap();
        let effects = json!({
            "version": 1,
            "complete": true,
            "inventory": {
                "productWrites": "none",
                "processStarts": "none",
                "networkWrites": "none",
                "otherExternal": "none"
            },
            "ownerAttestation": "fixture owner observed the exact duplicate permit invocation and its bounded effects"
        });
        let effects_bytes = serde_json::to_vec(&effects).unwrap();
        fs::write(artifacts.join("effects.json"), &effects_bytes).unwrap();
        let decision = json!({
            "version": 1,
            "kind": "duplicate_permit_owner_recovery",
            "approved": true,
            "owner": "fixture-owner",
            "reason": "recover only the recorded duplicate-permit refusal",
            "currentConfigSha256": draft["currentConfigSha256"],
            "predecessor": draft["predecessor"],
            "grantEventSha256": draft["grantEventSha256"],
            "grantGovernorEventSha256": draft["grantGovernorEventSha256"],
            "blockedEventSha256": draft["blockedEventSha256"],
            "blockedGovernorEventSha256": draft["blockedGovernorEventSha256"],
            "identity": draft["identity"],
            "productSnapshotSha256": draft["productSnapshotSha256"],
            "invocation": {
                "sameExactInvocation": true,
                "evidence": "the fixture issued the same worker assignment and operation immediately after the recorded grant"
            },
            "effectsEvidence": {
                "root": "state",
                "path": ".exitbind/artifacts/effects.json",
                "sha256": hex(Sha256::digest(&effects_bytes)),
                "bytes": effects_bytes.len()
            }
        });
        assert!(decision["identity"]["operation"].is_string());
        let path = ".exitbind/artifacts/recovery.json";
        fs::write(
            artifacts.join("recovery.json"),
            serde_json::to_vec(&decision).unwrap(),
        )
        .unwrap();
        // Catch accidental mutation of the values cloned from the unapproved
        // read-only draft before handing the artifact to the command.
        assert_eq!(decision["approved"], true);
        path.to_owned()
    }

    fn write_recovery_artifacts_from_inspect(&self, inspected: &Value, detail: &Value) -> String {
        let events = inspected["events"].as_array().unwrap();
        assert!(events.len() >= 3);
        let grant = &events[events.len() - 2];
        let blocked = events.last().unwrap();
        let start = &events[0];
        let effects = json!({
            "version": 1,
            "complete": true,
            "inventory": {
                "productWrites": "none",
                "processStarts": "none",
                "networkWrites": "none",
                "otherExternal": "none"
            },
            "ownerAttestation": "fixture owner observed the exact duplicate permit invocation and its bounded effects"
        });
        let effects_bytes = serde_json::to_vec(&effects).unwrap();
        let artifacts = self.root.join(".exitbind/artifacts");
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(artifacts.join("effects.json"), &effects_bytes).unwrap();
        let decision = json!({
            "version": 1,
            "kind": "duplicate_permit_owner_recovery",
            "approved": true,
            "owner": "fixture-owner",
            "reason": "recover only the recorded duplicate permit refusal",
            "currentConfigSha256": start["configSha256"],
            "predecessor": {
                "ledgerPath": self.old_ledger,
                "ledgerSha256": inspected["ledgerSha256"],
                "runId": inspected["runId"],
                "headEventSha256": blocked["eventSha256"],
                "configSha256": start["configSha256"]
            },
            "grantEventSha256": grant["eventSha256"],
            "grantGovernorEventSha256": grant["governorEvent"]["eventSha256"],
            "blockedEventSha256": blocked["eventSha256"],
            "blockedGovernorEventSha256": blocked["governorEvent"]["eventSha256"],
            "identity": {
                "work": self.work,
                "assignment": detail["recipient"]["assignment"],
                "runId": grant["runId"],
                "stage": grant["stage"],
                "attempt": grant["attempt"],
                "agent": grant["agent"],
                "role": grant["role"],
                "subjectSha256": grant["subjectSha256"],
                "assignmentSha256": grant["assignmentSha256"],
                "inputsSha256": grant["inputsSha256"],
                "operation": grant["operation"]
            },
            "productSnapshotSha256": grant["inputsSha256"],
            "invocation": {
                "sameExactInvocation": true,
                "evidence": "the fixture issued the same worker assignment and operation immediately after the recorded grant"
            },
            "effectsEvidence": {
                "root": "state",
                "path": ".exitbind/artifacts/effects.json",
                "sha256": hex(Sha256::digest(&effects_bytes)),
                "bytes": effects_bytes.len()
            }
        });
        fs::write(
            artifacts.join("recovery.json"),
            serde_json::to_vec(&decision).unwrap(),
        )
        .unwrap();
        ".exitbind/artifacts/recovery.json".to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn sensor_request_and_inert_result_bind_without_spending_or_replacing_mutation() {
    let fixture = Fixture::new();
    fixture.prepare_replanned_mutation(Some(true));
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "evidence_required");
    assert_eq!(inspected["governor"]["spent"], 3);
    let actions = inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["governorEvent"]["action"].as_str())
        .collect::<Vec<_>>();
    assert!(actions.iter().any(|action| *action == "sensor_request"));
    assert!(actions.iter().any(|action| *action == "sensor"));
    let sensor = inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["governorEvent"]["action"] == "sensor")
        .unwrap();
    assert_eq!(sensor["governorEvent"]["assessment"], "unavailable");
}

#[test]
fn unresolved_sensor_blocks_direct_owner_recovery_before_claim_or_start() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit_with_sensor(Some(false));
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert!(inspected["governor"]["currentSensorRequest"].is_object());
    assert!(inspected["governor"]["seenSensors"]
        .as_array()
        .unwrap()
        .is_empty());
    let recovery_path = fixture.write_recovery_artifacts_from_inspect(&inspected, &detail);
    let successor = ".exitbind/runs/work-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
            "--owner-recovery",
            &recovery_path,
        ],
        b"",
    );
    assert!(
        !refused.status.success(),
        "unresolved sensor recovery succeeded"
    );
    assert!(
        display(&refused).contains("unresolved governor sensor request"),
        "wrong refusal boundary: {}",
        display(&refused)
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    assert!(!fixture.root.join(successor).exists());
    assert!(!fixture.root.join(claim).exists());
    fixture.assert_prior_ledgers_unchanged();
}

#[test]
fn newer_unanswered_sensor_is_not_resolved_by_earlier_inert_result() {
    let fixture = Fixture::new();
    let (old_bytes, detail, newer_digest) = fixture.block_after_newer_unanswered_sensor();
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert_eq!(
        inspected["governor"]["seenSensors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        inspected["governor"]["currentSensorRequest"]["requestDigest"],
        newer_digest
    );
    let latest_request = inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|event| event["governorEvent"]["action"] == "sensor_request")
        .unwrap();
    assert_eq!(
        latest_request["governorEvent"]["requestDigest"],
        newer_digest
    );
    let recovery_path = fixture.write_recovery_artifacts_from_inspect(&inspected, &detail);
    let successor = ".exitbind/runs/work-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
            "--owner-recovery",
            &recovery_path,
        ],
        b"",
    );
    assert!(!refused.status.success());
    assert!(
        display(&refused).contains("supersession refused unresolved governor sensor request"),
        "wrong refusal boundary: {}",
        display(&refused)
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    assert!(!fixture.root.join(successor).exists());
    assert!(!fixture.root.join(claim).exists());
}

#[test]
fn resolved_sensor_allows_owner_recovery_to_start_grantless() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit_with_sensor(Some(true));
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert!(inspected["governor"]["currentSensorRequest"].is_object());
    assert_eq!(
        inspected["governor"]["seenSensors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let recovery_draft = detail["actionForms"]["recovery"].get("ownerRecoveryDraft");
    let recovery_path = fixture.write_recovery_artifacts_from_inspect(&inspected, &detail);
    let successor = ".exitbind/runs/work-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    let recovered = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
            "--owner-recovery",
            &recovery_path,
        ],
        b"",
    );
    assert!(
        recovered.status.success(),
        "resolved sensor should enable the valid owner recovery; draft={recovery_draft:?}; {}",
        display(&recovered)
    );
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["work"], format!("smw_{}", "e".repeat(64)));
    assert_eq!(recovery_draft.unwrap()["approved"], false);
    let successor_view = fixture.json(&["run", "inspect", successor], b"");
    assert_eq!(successor_view["governor"]["state"], "evidence_required");
    assert_eq!(successor_view["governor"]["spent"], 3);
    assert!(successor_view["governor"]["currentMutation"].is_null());
    assert!(successor_view["governor"]["consumedGrants"]
        .as_array()
        .unwrap()
        .is_empty());
    let start: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(start["governorCarry"]["accounting"]["state"], "blocked");
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    assert!(fixture.root.join(claim).exists());
    fixture.assert_prior_ledgers_unchanged();
}

#[test]
fn resolved_sensor_allows_ordinary_same_goal_carry() {
    let fixture = Fixture::new();
    let worker = fixture.json(&["work", "next", &fixture.work, "--full"], b"");
    let assignment = worker["next"]["assignment"].as_str().unwrap();
    let grant = fixture.permit_edit(assignment);
    assert!(grant.status.success(), "{}", display(&grant));
    let request = fixture.sensor_request(assignment);
    assert_eq!(
        request["event"]["governorEvent"]["action"],
        "sensor_request"
    );
    let result = fixture.inert_sensor_result(assignment);
    assert_eq!(
        result["event"]["governorEvent"]["assessment"],
        "unavailable"
    );
    let old_bytes = fs::read(fixture.root.join(&fixture.old_ledger)).unwrap();
    let successor = ".exitbind/runs/work-9999999999999999999999999999999999999999999999999999999999999999.jsonl";
    let carried = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
        ],
        b"",
    );
    assert!(carried.status.success(), "{}", display(&carried));
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    let successor_view = fixture.json(&["run", "inspect", successor], b"");
    assert_eq!(successor_view["governor"]["state"], "ready");
    assert_eq!(successor_view["governor"]["spent"], 1);
    assert!(successor_view["governor"]["currentMutation"].is_null());
    assert!(successor_view["governor"]["currentGrantEventSha256"].is_null());
}

#[test]
fn sensor_event_rejects_mutation_retry_identity_on_read() {
    let fixture = Fixture::new();
    let worker = fixture.json(&["work", "next", &fixture.work, "--full"], b"");
    let assignment = worker["next"]["assignment"].as_str().unwrap();
    let grant = fixture.permit_edit(assignment);
    assert!(grant.status.success(), "{}", display(&grant));
    let request = fixture.sensor_request(assignment);
    assert_eq!(
        request["event"]["governorEvent"]["action"],
        "sensor_request"
    );

    let path = fixture.root.join(&fixture.old_ledger);
    let source = fs::read_to_string(&path).unwrap();
    let mut lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut event: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    event["governorEvent"]["requestId"] = json!("forbidden-sensor-retry-id");
    rehash_event(&mut event["governorEvent"]);
    rehash_event(&mut event);
    *lines.last_mut().unwrap() = serde_json::to_string(&event).unwrap();
    fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

    let refused = fixture.call(&["run", "inspect", &fixture.old_ledger], b"");
    assert!(
        !refused.status.success(),
        "sensor retry identity was accepted"
    );
    assert!(
        display(&refused).contains("sensor event cannot use mutation retry identity"),
        "wrong refusal boundary: {}",
        display(&refused)
    );
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&object[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn value_sha256(value: &Value) -> String {
    hex(Sha256::digest(canonical(value).as_bytes()))
}

fn rehash_event(event: &mut Value) {
    event.as_object_mut().unwrap().remove("eventSha256");
    let digest = value_sha256(event);
    event["eventSha256"] = json!(digest);
}

fn display(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn bounded(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(1200).collect()
}

#[test]
fn owner_recovery_preserves_old_ledger_accounting_and_requires_new_evidence() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    let _decision_path = fixture.write_recovery_artifacts(draft);
    let successor = format!(".exitbind/runs/work-{}.jsonl", "a".repeat(64));
    let recovered = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
    assert!(recovered.status.success(), "{}", display(&recovered));
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();
    assert_eq!(recovered["work"], format!("smw_{}", "a".repeat(64)));
    let start: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(start["carryProtocol"], 2);
    assert_eq!(start["governorCarry"]["accounting"]["state"], "blocked");
    let carry_sha = value_sha256(&start["governorCarry"]);
    assert_eq!(start["governorCarrySha256"], carry_sha);
    let claim_path = format!("{}.supersede", fixture.old_ledger);
    let claim_bytes = fs::read(fixture.root.join(&claim_path)).unwrap();
    let claim: Value = serde_json::from_slice(&claim_bytes).unwrap();
    assert_eq!(claim["carryProtocol"], 2);
    assert_eq!(claim["governorCarrySha256"], carry_sha);
    let successor_work = format!("smw_{}", "a".repeat(64));
    let inspected = fixture.json(&["run", "inspect", &successor], b"");
    assert_eq!(inspected["governor"]["state"], "evidence_required");
    assert_eq!(inspected["governor"]["spent"], 3);
    assert_eq!(inspected["governor"]["currentMutation"], Value::Null);
    assert_eq!(
        inspected["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        inspected["events"][0]["reviewPolicy"]["decision"],
        "required"
    );
    assert!(inspected["events"][0]["checkPolicy"].is_object());
    assert_eq!(inspected["governorRecovery"]["protocol"], 2);
    assert_eq!(inspected["governorRecovery"]["state"], "evidence_required");
    let status = fixture.json(&["run", "status", &successor, "--json"], b"");
    assert_eq!(status["governorRecovery"]["state"], "evidence_required");

    // The frozen predecessor reader must still read the old Work and ledger
    // after the protocol-2 successor claim and start have been written.
    if let Ok(old_binary) = std::env::var("EXITBIND_OWNER_RECOVERY_OLD_READER") {
        for args in [
            vec![
                "work",
                "detail",
                fixture.work.as_str(),
                "--json",
                "--config",
                "exitbind.json",
            ],
            vec![
                "run",
                "inspect",
                fixture.old_ledger.as_str(),
                "--config",
                "exitbind.json",
            ],
        ] {
            let output = Command::new(&old_binary)
                .current_dir(&fixture.root)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", display(&output));
        }
    }
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();

    // The exact same successor request replays the existing claim/start.
    let replay = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
    assert!(replay.status.success(), "{}", display(&replay));
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["work"], recovered["work"]);
    let replayed = fixture.json(&["run", "inspect", &successor], b"");
    assert_eq!(replayed["governor"]["spent"], 3);
    assert_eq!(replayed["governor"]["state"], "evidence_required");

    let other_target = ".exitbind/runs/work-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.jsonl";
    let refused_other_target = fixture.execute_form_replacing(
        &detail["actionForms"]["recovery"],
        b"",
        ".exitbind/runs/work-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jsonl",
        other_target,
    );
    assert!(
        !refused_other_target.status.success(),
        "different target replay was accepted"
    );
    assert!(!fixture.root.join(other_target).exists());

    let initial_recipient = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(initial_recipient["next"]["role"], "lead");
    let lead_detail = fixture.json(&["work", "detail", &successor_work, "--json"], b"");
    assert!(lead_detail["actionForms"].get("beforeEditing").is_none());
    let scope_form = Fixture::choice(&lead_detail["actionForms"]["choices"], "scoped");
    let scope = fixture.execute_form(&scope_form, b"fresh scope for the recovered successor\n");
    assert!(scope.status.success(), "{}", display(&scope));

    let worker_detail = fixture.json(&["work", "detail", &successor_work, "--json"], b"");
    let pending = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(
        pending["next"]["packet"]["context"]["loop"]["state"],
        "evidence_required"
    );
    let evidence_path = fixture.root.join(".exitbind/artifacts/fresh.md");
    fs::write(evidence_path, b"new exact evidence\n").unwrap();
    let evidence_form = Fixture::choice(&worker_detail["actionForms"]["choices"], "evidence");
    let evidence = fixture.execute_form(&evidence_form, b"");
    assert!(evidence.status.success(), "{}", display(&evidence));
    let after_evidence: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        after_evidence["governorCarry"]["accounting"]["state"],
        "blocked"
    );
    assert_eq!(after_evidence["governorCarrySha256"], carry_sha);
    assert_eq!(
        fs::read(fixture.root.join(&claim_path)).unwrap(),
        claim_bytes
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    let ready = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(ready["next"]["packet"]["context"]["loop"]["state"], "ready");
    assert!(ready["next"]["packet"]["context"]["loop"]["spent"] == 3);
    let ready_detail = fixture.json(&["work", "detail", &successor_work, "--json"], b"");
    let permit_form = &ready_detail["actionForms"]["beforeEditing"];
    assert!(permit_form.is_object());
    let argv = permit_form["command"]["argv"].as_array().unwrap();
    let request_index = argv
        .iter()
        .position(|argument| argument == "--request-id")
        .unwrap();
    assert_eq!(
        argv[request_index + 1],
        format!(
            "permit-{}",
            ready_detail["actionForms"]["binding"].as_str().unwrap()
        )
    );
    let permit = fixture.execute_form(permit_form, b"");
    assert!(permit.status.success(), "{}", display(&permit));
    let grant: Value = serde_json::from_slice(&permit.stdout).unwrap();
    assert_eq!(grant["allowed"], true);
    assert!(grant["currentGrant"].is_object());
    let granted = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(granted["next"]["packet"]["context"]["loop"]["spent"], 4);
    assert_eq!(
        granted["next"]["packet"]["context"]["loop"]["currentMutation"]["runId"],
        granted["next"]["packet"]["context"]["run"]["id"]
    );
    let granted_bytes = fs::read(fixture.root.join(&successor)).unwrap();
    let replay = fixture.execute_form(permit_form, b"");
    assert!(replay.status.success(), "{}", display(&replay));
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["idempotent"], true);
    assert_eq!(
        fs::read(fixture.root.join(&successor)).unwrap(),
        granted_bytes
    );
    let replayed = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(replayed["next"]["packet"]["context"]["loop"]["spent"], 4);
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();

    // Altered protocol-2 marker evidence is refused on successor read.
    let successor_bytes = fs::read(fixture.root.join(&successor)).unwrap();
    let mut first: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    first.as_object_mut().unwrap().remove("governorCarry");
    first.as_object_mut().unwrap().remove("carryProtocol");
    first.as_object_mut().unwrap().remove("governorCarrySha256");
    rehash_event(&mut first);
    fs::write(
        fixture.root.join(&successor),
        format!("{}\n", serde_json::to_string(&first).unwrap()),
    )
    .unwrap();
    let stripped = fixture.call(&["run", "inspect", &successor], b"");
    assert!(
        !stripped.status.success(),
        "stripped recovery marker was accepted"
    );
    fs::write(fixture.root.join(&successor), &successor_bytes).unwrap();
    assert!(fixture
        .call(&["run", "inspect", &successor], b"")
        .status
        .success());

    // Even a correctly rehashed successor event cannot change the accounting
    // that the predecessor claim committed to.
    let mut changed: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    changed["governorCarry"]["accounting"]["spent"] = json!(2);
    let carry_sha = value_sha256(&changed["governorCarry"]);
    changed["governorCarrySha256"] = json!(carry_sha);
    rehash_event(&mut changed);
    fs::write(
        fixture.root.join(&successor),
        format!("{}\n", serde_json::to_string(&changed).unwrap()),
    )
    .unwrap();
    let altered_accounting = fixture.call(&["run", "inspect", &successor], b"");
    assert!(
        !altered_accounting.status.success(),
        "rehash allowed carried accounting to diverge from the predecessor claim"
    );
    fs::write(fixture.root.join(&successor), &successor_bytes).unwrap();
}

#[test]
fn duplicate_blocked_protocol1_predecessor_refuses_direct_evidence_without_appending() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let artifact = fixture.root.join(".exitbind/artifacts/blocked.md");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(&artifact, b"evidence cannot reopen a blocked predecessor\n").unwrap();
    let assignment = detail["recipient"]["assignment"].as_str().unwrap();
    let attempt = fixture.call(
        &[
            "work",
            "evidence",
            &fixture.work,
            assignment,
            "--artifact-root",
            "state",
            "--artifact",
            ".exitbind/artifacts/blocked.md",
        ],
        b"",
    );
    assert!(
        !attempt.status.success(),
        "direct evidence reopened a blocked protocol-1 predecessor: {}",
        display(&attempt)
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
}

#[test]
fn owner_recovery_rejects_changed_owner_decision_bindings_before_claim() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    let successor = ".exitbind/runs/work-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    for case in [
        "unapproved",
        "malformed",
        "config",
        "predecessor",
        "assignment",
        "operation",
        "effect_digest",
    ] {
        fixture.write_recovery_artifacts(draft);
        let decision_path = fixture.root.join(".exitbind/artifacts/recovery.json");
        let mut decision: Value =
            serde_json::from_slice(&fs::read(&decision_path).unwrap()).unwrap();
        match case {
            "unapproved" => decision["approved"] = json!(false),
            "malformed" => {
                decision.as_object_mut().unwrap().remove("invocation");
            }
            "config" => decision["currentConfigSha256"] = json!("0".repeat(64)),
            "predecessor" => {
                decision["predecessor"]["headEventSha256"] = json!("0".repeat(64));
            }
            "assignment" => {
                decision["identity"]["assignment"] = json!(format!("smw_{}", "0".repeat(64)));
            }
            "operation" => decision["identity"]["operation"] = json!("different operation"),
            "effect_digest" => {
                decision["effectsEvidence"]["sha256"] = json!("0".repeat(64));
            }
            _ => unreachable!(),
        }
        fs::write(&decision_path, serde_json::to_vec(&decision).unwrap()).unwrap();
        let refused = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
        assert!(!refused.status.success(), "{case} decision was accepted");
        assert!(
            !fixture.root.join(successor).exists(),
            "{case} created a successor"
        );
        assert!(
            !fixture.root.join(&claim).exists(),
            "{case} created a predecessor claim"
        );
        assert_eq!(
            fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
            old_bytes
        );
        fixture.assert_prior_ledgers_unchanged();
    }
}

#[test]
fn owner_recovery_requires_a_complete_effect_inventory_and_exact_same_goal() {
    let fixture = Fixture::new();
    let (_old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    let decision_path = fixture.write_recovery_artifacts(draft);
    let effects = fixture.root.join(".exitbind/artifacts/effects.json");
    let mut inventory: Value = serde_json::from_slice(&fs::read(&effects).unwrap()).unwrap();
    inventory["inventory"]["processStarts"] = json!("unknown");
    let effects_bytes = serde_json::to_vec(&inventory).unwrap();
    fs::write(&effects, &effects_bytes).unwrap();
    let decision_file = fixture.root.join(".exitbind/artifacts/recovery.json");
    let mut decision: Value = serde_json::from_slice(&fs::read(&decision_file).unwrap()).unwrap();
    decision["effectsEvidence"]["sha256"] = json!(hex(Sha256::digest(&effects_bytes)));
    decision["effectsEvidence"]["bytes"] = json!(effects_bytes.len());
    fs::write(&decision_file, serde_json::to_vec(&decision).unwrap()).unwrap();
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            ".exitbind/runs/refused-no-effects.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !refused.status.success(),
        "unknown effect inventory was accepted"
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/refused-no-effects.jsonl")
        .exists());

    let decision_path = fixture.write_recovery_artifacts(draft);
    fs::write(
        fixture.root.join("late-product-change.md"),
        b"changed after the draft\n",
    )
    .unwrap();
    let stale_inputs = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            ".exitbind/runs/refused-stale-inputs.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !stale_inputs.status.success(),
        "stale product snapshot was accepted"
    );
    fs::remove_file(fixture.root.join("late-product-change.md")).unwrap();

    let different_goal = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            "a changed goal must not use duplicate-permit recovery",
            "--ledger",
            ".exitbind/runs/refused-different-goal.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !different_goal.status.success(),
        "changed goal was accepted"
    );
}

#[cfg(unix)]
#[test]
fn owner_recovery_refuses_symlinked_effect_evidence_before_claiming_successor() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let (_old_bytes, detail) = fixture.block_duplicate_permit();
    let decision_path =
        fixture.write_recovery_artifacts(&detail["actionForms"]["recovery"]["ownerRecoveryDraft"]);
    let artifacts = fixture.root.join(".exitbind/artifacts");
    fs::rename(
        artifacts.join("effects.json"),
        artifacts.join("effects-target.json"),
    )
    .unwrap();
    symlink("effects-target.json", artifacts.join("effects.json")).unwrap();
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            ".exitbind/runs/refused-symlink.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !refused.status.success(),
        "symlinked effects evidence was accepted"
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/refused-symlink.jsonl")
        .exists());
}

#[cfg(unix)]
#[test]
fn held_completion_blocks_owner_successor_without_changing_old_ledger_or_bytes() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    fixture.write_recovery_artifacts(draft);
    let assignment = fixture.json(&["work", "next", &fixture.work, "--full"], b"")["next"]
        ["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ordinary_result = fixture.call(
        &[
            "work",
            "return",
            &fixture.work,
            &assignment,
            "--outcome",
            "completed",
        ],
        b"a completion after durable governor refusal",
    );
    assert!(
        !ordinary_result.status.success(),
        "blocked completion unexpectedly succeeded"
    );
    let held_dir = fixture.root.join(".exitbind/held");
    assert!(!held_dir.exists() || fs::read_dir(&held_dir).unwrap().next().is_none());

    let result_bytes = b"valid fault-injected held bytes for the exact assignment";
    let work_part = fixture.work.strip_prefix("smw_").unwrap();
    let assignment_sha = hex(Sha256::digest(assignment.as_bytes()));
    let bytes_sha = hex(Sha256::digest(result_bytes));
    let reference = format!("hld_{work_part}_{assignment_sha}_{bytes_sha}");
    fs::create_dir_all(&held_dir).unwrap();
    fs::set_permissions(&held_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let held_path = held_dir.join(format!("{reference}.bin"));
    fs::write(&held_path, result_bytes).unwrap();
    fs::set_permissions(&held_path, fs::Permissions::from_mode(0o600)).unwrap();

    // The previously emitted command is stale with respect to held ownership
    // and must refuse before claim/start, even though its evidence was valid.
    let refused = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
    assert!(
        !refused.status.success(),
        "held result did not block successor admission"
    );
    let successor = ".exitbind/runs/work-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jsonl";
    assert!(!fixture.root.join(successor).exists());
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();

    let after = fixture.json(&["work", "detail", &fixture.work, "--json"], b"");
    assert!(after["actionForms"].get("beforeEditing").is_none());
    let current = fixture.json(&["work", "next", &fixture.work, "--full"], b"");
    assert_eq!(current["next"]["held"]["sha256"], bytes_sha);
    assert_eq!(current["next"]["held"]["bytes"], result_bytes.len());
    assert_eq!(current["next"]["held"]["reference"], reference);
    assert_eq!(
        fs::read(held_dir.join(format!("{reference}.bin"))).unwrap(),
        result_bytes
    );
}

#[test]
fn frozen_old_reader_can_read_the_exact_blocked_work_detail_when_configured() {
    let Ok(old_binary) = std::env::var("EXITBIND_OWNER_RECOVERY_OLD_READER") else {
        eprintln!("COVERAGE UNAVAILABLE: set EXITBIND_OWNER_RECOVERY_OLD_READER to exercise the frozen 9098 executable against the blocked predecessor");
        return;
    };
    let fixture = Fixture::new();
    let (old_bytes, _) = fixture.block_duplicate_permit();
    let output = Command::new(old_binary)
        .current_dir(&fixture.root)
        .args([
            "work",
            "detail",
            &fixture.work,
            "--json",
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", display(&output));
    let detail: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(detail["work"], fixture.work);
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
}
