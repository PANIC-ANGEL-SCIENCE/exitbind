//! Owner-bound recovery for one durable duplicate-permit refusal.
mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    io::Write,
    process::{Output, Stdio},
};

struct Fixture {
    root: support::TempRoot,
    work: String,
    goal: String,
    old_ledger: String,
    prior_ledgers: Vec<(String, Vec<u8>)>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_owned_outputs(false)
    }
    fn with_owned_outputs(preparation: bool) -> Self {
        let root = support::temp("owner-governor-recovery");
        support::git_topology::repository(&root);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        if preparation {
            // Select owned verification outputs so this marked Work's
            // state-aware digest omits the in-product configuration path.
            let path = root.join("exitbind.json");
            let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            config["project"]["verificationPreparation"] = json!({
                "version": 1,
                "pythonBytecode": "disabled",
                "scratch": "taskOwned"
            });
            fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        }
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
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
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
        let mut child = support::git_topology::command(&args[0])
            .current_dir(&self.root)
            .args(&args[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
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

#[path = "governor_recovery/owner_recovery.rs"]
mod owner_recovery;
#[path = "governor_recovery/sensor_resolution.rs"]
mod sensor_resolution;
