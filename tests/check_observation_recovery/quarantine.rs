//! Actual CLI admissions and returns on controlled journals. Only the old
//! unknown failure below is synthetic; it does not attest process termination.
use super::*;
use sha2::{Digest, Sha256};

const FROZEN: &[u8] = b"Frozen verification subject; no implementation mutation.\n";
const REASON: &str = "Preserve the old unknown and permit one separate verification";
const REQUEST: &str = "controlled-quarantine-once";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct Case {
    f: Fixture,
    old: String,
    original_member: String,
    feature: String,
    feature_config: PathBuf,
    owner_config: PathBuf,
    old_bytes: Vec<u8>,
}

impl Case {
    fn new() -> Self {
        Self::with_distinct_owner(true)
    }
    fn with_distinct_owner(distinct: bool) -> Self {
        Self::with_options(distinct, false)
    }
    fn with_options(distinct: bool, descendant: bool) -> Self {
        let mut f = Fixture::new("exit 7\n");
        select_fixture_contract(&f);
        f.native_session = Some(format!(
            "quarantine-{}",
            f.root.file_name().unwrap().to_string_lossy()
        ));
        let owner_root = f.root.join(".exitbind/goal-owner");
        let owner_config = if distinct {
            fs::create_dir(&owner_root).unwrap();
            let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
                .args(["init", "--mode", "portable", "--root"])
                .arg(&owner_root)
                .arg("--skip-skills")
                .output()
                .unwrap();
            assert!(init.status.success(), "{init:?}");
            owner_root.join("exitbind.json")
        } else {
            f.root.join("exitbind.json")
        };
        let incorporated = call_config(
            &f,
            &owner_config,
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "original-quarantine-goal",
                "--goal",
                "Preserve all original outcomes and unknown evidence",
                "--none-applicable",
                "findings,blockers,decisions,externalActions",
                "--json",
            ],
            b"",
        );
        assert!(incorporated.status.success(), "{incorporated:?}");
        let feature_config = f.root.join("feature.json");
        let mut configuration = fs::read(f.root.join("exitbind.json")).unwrap();
        configuration.push(b'\n');
        fs::write(&feature_config, configuration).unwrap();
        let basis = json!({"version":1,"constraints":["Keep old unknown uncredited"],
            "openZones":[],"decisiveCases":["Exact separate verification only"]})
        .to_string();
        let old = if descendant {
            legacy_successor(&f)
        } else {
            f.begin()
        };
        let owner_status = call_config(&f, &owner_config, &["goal", "status", "--json"], b"");
        assert!(owner_status.status.success(), "{owner_status:?}");
        let original_member = value(&owner_status)["journeyMembers"][0]["work"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(old != original_member, descendant);
        // Match legacy journals that already retain earlier observed failures.
        // Only the final unresolved admission is quarantined.
        let prior = f.call(&["work", "check", &old, "--timeout-ms", "2000"], b"");
        assert!(!prior.status.success(), "{prior:?}");
        assert_eq!(
            f.events(&old)
                .iter()
                .filter(|e| e["action"] == "check")
                .count(),
            1
        );
        let lead = f.next(&old);
        let worker = f.give(&old, &lead, "rework")["next"].clone();
        fs::write(f.root.join("checker.sh"), "mv .exitbind/runs .exitbind/runs-retained; printf unavailable > .exitbind/runs; printf captured; exit 0\n").unwrap();
        f.give(&old, &worker, "completed");
        let failed = f.call(&["work", "check", &old, "--timeout-ms", "2000"], b"");
        assert!(!failed.status.success(), "{failed:?}");
        let failure = value(&failed);
        assert_eq!(failure["durableFailureRecorded"], false);
        fs::remove_file(f.root.join(".exitbind/runs")).unwrap();
        fs::rename(
            f.root.join(".exitbind/runs-retained"),
            f.root.join(".exitbind/runs"),
        )
        .unwrap();
        let events = f.events(&old);
        let head = events.last().unwrap();
        assert_eq!(head["action"], "check_observation");
        let mut facts = failure["observation"].clone();
        facts["termination"] = json!("unknown");
        facts["groupEnded"] = json!(false);
        facts["cleanupErrorCount"] = json!(1);
        let mut unknown = json!({"version":8,"kind":"run","producer":head["producer"],
            "action":"check_observation_failed","runId":head["runId"],
            "observation":{"admissionEventSha256":head["eventSha256"],"facts":facts},
            "previousEventSha256":head["eventSha256"],"timestamp":head["timestamp"]});
        rehash(&mut unknown);
        let mut bytes = fs::read(f.ledger(&old)).unwrap();
        bytes.extend(serde_json::to_vec(&unknown).unwrap());
        bytes.push(b'\n');
        fs::write(f.ledger(&old), &bytes).unwrap();
        let lead = f.next(&old);
        f.give(&old, &lead, "blocked");
        let bytes = fs::read(f.ledger(&old)).unwrap();
        let current = f.ok(
            &[
                "run",
                "inspect",
                &format!(".exitbind/runs/work-{}.jsonl", &old[4..]),
                "--json",
            ],
            b"",
        );
        assert_eq!(current["status"], "blocked");
        assert_eq!(current["checkObservation"]["state"], "failed");
        // Prepare the new frozen source before the feature plan is checked.
        fs::write(f.root.join("checker.sh"), "exit 0\n").unwrap();
        fs::write(f.root.join(".exitbind/frozen-subject.txt"), FROZEN).unwrap();
        let mut case = Self {
            f,
            old,
            original_member,
            feature: String::new(),
            feature_config,
            owner_config,
            old_bytes: bytes,
        };
        let begin = case.config_call(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Review the separate immutable verification plan",
                "--check-command",
                "exec sh checker.sh",
                "--review-policy",
                "required",
            ],
            b"",
        );
        assert!(begin.status.success(), "{begin:?}");
        let started = value(&begin);
        case.feature = started["work"].as_str().unwrap().into();
        let worker = case.config_return(&started["next"], "scoped", b"Scoped isolation plan")
            ["next"]
            .clone();
        let inspection = case.config_call(
            &[
                "run",
                "inspect",
                &format!(".exitbind/runs/work-{}.jsonl", &case.feature[4..]),
                "--json",
            ],
            b"",
        );
        assert!(inspection.status.success(), "{inspection:?}");
        let inputs = value(&inspection)["inputsSha256"].clone();
        assert!(inputs.as_str().is_some());
        let report = json!({"kind":"quarantine-verification-plan","protocol":1,
            "plan":{"environment":{"kind":"controlled-local-check"},
                "effectInventory":{"write":"none","scope":"fixture only"},
                "artifactSha256":digest(FROZEN),"inputsSha256":inputs,
                "basis":serde_json::from_str::<Value>(&basis).unwrap(),
                "command":"exec sh checker.sh"}});
        case.config_return(&worker, "completed", &serde_json::to_vec(&report).unwrap());
        case
    }

    fn config_call(&self, args: &[&str], body: &[u8]) -> Output {
        call_config(&self.f, &self.feature_config, args, body)
    }
    fn owner_ok(&self, args: &[&str]) -> Value {
        let output = call_config(&self.f, &self.owner_config, args, b"");
        assert!(output.status.success(), "{output:?}");
        value(&output)
    }

    fn config_return(&self, next: &Value, outcome: &str, body: &[u8]) -> Value {
        let output = self.config_call(
            &[
                "work",
                "return",
                &self.feature,
                next["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            body,
        );
        assert!(output.status.success(), "{output:?}");
        value(&output)
    }
}

fn legacy_successor(f: &Fixture) -> String {
    let original_config = f.root.join("original-source.json");
    let mut bytes = fs::read(f.root.join("exitbind.json")).unwrap();
    bytes.extend(b"\n\n");
    fs::write(&original_config, bytes).unwrap();
    let started = call_config(
        f,
        &original_config,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "repair the checker",
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
        ],
        b"",
    );
    assert!(started.status.success(), "{started:?}");
    let started = value(&started);
    let original = started["work"].as_str().unwrap();
    let scoped = call_config(
        f,
        &original_config,
        &[
            "work",
            "return",
            original,
            started["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        b"Keep original Goal membership",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let scoped = value(&scoped);
    let blocked = call_config(
        f,
        &original_config,
        &[
            "work",
            "return",
            original,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        b"Exact same-goal successor needed",
    );
    assert!(blocked.status.success(), "{blocked:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        digest(f.root.to_str().unwrap().as_bytes())
    );
    let superseded = f.ok(
        &[
            "run",
            "supersede",
            &format!(".exitbind/runs/work-{}.jsonl", &original[4..]),
            "--workflow",
            "change",
            "--goal",
            "repair the checker",
            "--ledger",
            &ledger,
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
            "--json",
        ],
        b"",
    );
    let successor = superseded["work"].as_str().unwrap();
    let lead = f.next(successor);
    let worker = f.give(successor, &lead, "scoped")["next"].clone();
    f.give(successor, &worker, "completed");
    assert!(f.events(successor)[0].get("basis").is_none());
    successor.to_owned()
}

fn call_config(f: &Fixture, config: &std::path::Path, args: &[&str], body: &[u8]) -> Output {
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&f.root)
        .args(args)
        .arg("--config")
        .arg(config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    f.native_environment(&mut command);
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(body).unwrap();
    child.wait_with_output().unwrap()
}

impl Case {
    fn accept_feature(&self) {
        let check = self.config_call(
            &["work", "check", &self.feature, "--timeout-ms", "2000"],
            b"",
        );
        assert!(check.status.success(), "{check:?}");
        let output = self.config_call(&["work", "next", &self.feature, "--full"], b"");
        assert!(output.status.success(), "{output:?}");
        let reviewer = value(&output)["next"].clone();
        assert_eq!(reviewer["role"], "reviewer");
        let lead =
            self.config_return(&reviewer, "approved", b"Reviewed exact plan")["next"].clone();
        self.config_return(&lead, "accepted", b"Accepted exact plan");
    }

    fn preview(&self, request: &str) -> Output {
        self.preview_with_owner(request, true)
    }
    fn preview_with_owner(&self, request: &str, explicit_owner: bool) -> Output {
        let detail = self.f.ok(&["work", "detail", &self.old, "--json"], b"");
        let mut args = vec![
            "work",
            "quarantine",
            &self.old,
            "--feature-config",
            self.feature_config.to_str().unwrap(),
            "--feature-work",
            &self.feature,
            "--artifact-root",
            "state",
            "--artifact",
            ".exitbind/frozen-subject.txt",
            "--request-id",
            request,
            "--reason",
            REASON,
            "--current-binding",
            detail["actionForms"]["binding"].as_str().unwrap(),
            "--json",
        ];
        if explicit_owner {
            args.extend(["--owner-config", self.owner_config.to_str().unwrap()]);
        }
        self.f.call(&args, b"")
    }

    fn apply(&self, preview: &Value) -> Output {
        let argv = if preview["applyArgv"].is_array() {
            &preview["applyArgv"]
        } else {
            &preview["apply"]
        };
        assert!(
            argv.is_array(),
            "producer did not emit apply argv: {preview}"
        );
        self.f.emitted(&json!({"argv":argv}), b"")
    }

    fn return_frozen(&self, work: &str) {
        let lead = self.f.next(work);
        assert_eq!(lead["role"], "lead");
        let worker = self.f.give(work, &lead, "scoped")["next"].clone();
        self.f.ok(
            &[
                "work",
                "return",
                work,
                worker["assignment"].as_str().unwrap(),
                "--outcome",
                "completed",
            ],
            FROZEN,
        );
    }
}

#[test]
fn quarantine_default_owner_remains_supported_and_foreign_owner_refuses() {
    let c = Case::with_distinct_owner(false);
    c.accept_feature();
    let preview = c.preview_with_owner(REQUEST, false);
    assert!(preview.status.success(), "{preview:?}");
    assert!(c.apply(&value(&preview)).status.success());

    let distinct = Case::new();
    distinct.accept_feature();
    assert!(!distinct.preview_with_owner(REQUEST, false).status.success());
    assert_eq!(
        fs::read(distinct.f.ledger(&distinct.old)).unwrap(),
        distinct.old_bytes
    );
}

#[test]
fn quarantine_refuses_active_admission_and_known_ended_failure_without_new_credit() {
    let c = Case::new();
    c.accept_feature();
    let mut events = c.f.events(&c.old);
    let blocked = events.pop().unwrap();
    assert_eq!(blocked["action"], "submit");
    assert_eq!(blocked["outcome"], "blocked");
    let failure = events.pop().unwrap();
    assert_eq!(failure["action"], "check_observation_failed");
    let active = events
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(c.f.ledger(&c.old), &active).unwrap();
    assert!(!c.preview(REQUEST).status.success());
    assert_eq!(fs::read_to_string(c.f.ledger(&c.old)).unwrap(), active);
    let mut ended = failure;
    ended["observation"]["facts"]["termination"] = json!("ended");
    ended["observation"]["facts"]["groupEnded"] = json!(true);
    ended["observation"]["facts"]["captureReadersEnded"] = json!(true);
    ended["observation"]["facts"]["cleanupErrorCount"] = json!(0);
    rehash(&mut ended);
    events.push(ended);
    let ended = events
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(c.f.ledger(&c.old), &ended).unwrap();
    assert!(!c.preview(REQUEST).status.success());
    assert_eq!(fs::read_to_string(c.f.ledger(&c.old)).unwrap(), ended);
    fs::write(c.f.ledger(&c.old), &c.old_bytes).unwrap();
}

#[test]
fn quarantine_requires_accepted_current_plan_and_preview_has_no_start_effect() {
    let c = Case::new();
    let refused = c.preview(REQUEST);
    assert!(!refused.status.success(), "{refused:?}");
    c.accept_feature();
    let before = fs::read_dir(c.f.root.join(".exitbind/runs"))
        .unwrap()
        .count();
    let preview = c.preview(REQUEST);
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(
        fs::read_dir(c.f.root.join(".exitbind/runs"))
            .unwrap()
            .count(),
        before
    );
    assert_eq!(fs::read(c.f.ledger(&c.old)).unwrap(), c.old_bytes);
    fs::write(c.f.root.join("checker.sh"), "exit 7\n").unwrap();
    assert!(!c.preview(REQUEST).status.success());
    assert!(!c.apply(&value(&preview)).status.success());
    assert_eq!(fs::read(c.f.ledger(&c.old)).unwrap(), c.old_bytes);
}

#[test]
fn quarantine_starts_once_preserves_old_unknown_and_requires_fresh_observed_review() {
    exercise_quarantine_lifecycle(Case::new());
    exercise_quarantine_lifecycle(Case::with_options(true, true));
}

#[test]
fn stale_observed_reviewer_recovery_preserves_ordinary_work_protocols() {
    exercise_stale_reviewer_recovery(None, true, true);
    exercise_stale_reviewer_recovery(
        Some(
            r#"{"version":1,"constraints":["preserve history"],"openZones":["worker implementation"],"decisiveCases":["return a contradiction"]}"#,
        ),
        true,
        true,
    );
    exercise_stale_reviewer_recovery(None, true, false);
    exercise_unmarked_stale_check_remains_check_only();
}

fn exercise_stale_reviewer_recovery(basis: Option<&str>, marked: bool, drift: bool) {
    let f = Fixture::new("exit 0\n");
    let work = if marked {
        f.begin_with_optional_basis(basis)
    } else {
        let begun = f.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "repair the checker",
                "--check-command",
                "exec sh checker.sh",
            ],
            b"",
        );
        let work = begun["work"].as_str().unwrap().to_owned();
        let worker = f.give(&work, &begun["next"], "scoped")["next"].clone();
        f.give(&work, &worker, "completed");
        work
    };
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    let reviewer = f.next(&work);
    assert_eq!(reviewer["role"], "reviewer");

    if !drift {
        assert!(reviewer.get("staleEndedCheck").is_none());
        let detail = f.ok(&["work", "detail", &work, "--json"], b"");
        let choices = detail["actionForms"]["choices"].as_array().unwrap();
        assert!(choices.iter().any(|choice| choice["label"] == "approved"));
        let lead = f.give(&work, &reviewer, "approved")["next"].clone();
        f.give(&work, &lead, "accepted");
        assert_eq!(f.next(&work)["status"], "accepted");
        return;
    }

    fs::write(f.root.join("checker.sh"), "printf changed-inputs\\n\n").unwrap();
    let stale = f.next(&work);
    assert_eq!(stale["role"], "reviewer");
    assert_eq!(stale["assignment"], reviewer["assignment"]);
    assert_eq!(
        stale["staleEndedCheck"]["staleReason"],
        "tested_inputs_changed"
    );
    assert_eq!(stale["outcomes"], json!(["rework", "blocked"]));
    let detail = f.ok(&["work", "detail", &work, "--json"], b"");
    let choices = detail["actionForms"]["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 2);
    let before = fs::read(f.ledger(&work)).unwrap();
    for outcome in ["approved", "unavailable"] {
        let refused = f.call(
            &[
                "work",
                "return",
                &work,
                stale["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            outcome.as_bytes(),
        );
        assert!(!refused.status.success(), "{outcome}: {refused:?}");
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    }

    let disposition = f.give(&work, &stale, "rework")["next"].clone();
    let worker = if marked {
        assert_eq!(disposition["role"], "lead");
        assert_eq!(disposition["action"], "lead_decision");
        let detail = f.ok(&["work", "detail", &work, "--json"], b"");
        assert_eq!(detail["actionForms"]["state"], "pending_review_finding");
        assert!(detail["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|choice| choice["label"] == "repair"));
        f.repair(&work)
    } else {
        assert_eq!(disposition["role"], "worker");
        disposition
    };
    assert_eq!(worker["role"], "worker");
    fs::write(f.root.join("checker.sh"), "exit 0\n").unwrap();
    f.give(&work, &worker, "completed");
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    let fresh_reviewer = f.next(&work);
    assert_eq!(fresh_reviewer["role"], "reviewer");
    let lead = f.give(&work, &fresh_reviewer, "approved")["next"].clone();
    assert_eq!(lead["role"], "lead");
    f.give(&work, &lead, "accepted");
    assert_eq!(f.next(&work)["status"], "accepted");
}

fn exercise_unmarked_stale_check_remains_check_only() {
    let f = Fixture::new("exit 0\n");
    let begun = f.ok(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "repair the checker",
            "--check-command",
            "exec sh checker.sh",
        ],
        b"",
    );
    let work = begun["work"].as_str().unwrap().to_owned();
    let worker = f.give(&work, &begun["next"], "scoped")["next"].clone();
    f.give(&work, &worker, "completed");
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    let reviewer = f.next(&work);
    assert_eq!(reviewer["role"], "reviewer");

    fs::write(f.root.join("checker.sh"), "printf changed-inputs\\n\n").unwrap();
    let current = f.next(&work);
    assert_eq!(current["action"], "check");
    assert!(current.get("staleEndedCheck").is_none());
    assert!(current.get("outcomes").is_none());
    let before = fs::read(f.ledger(&work)).unwrap();
    let refused = f.call(
        &[
            "work",
            "return",
            &work,
            reviewer["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        b"attempt approval without an observed-check recovery route",
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
}

fn exercise_quarantine_lifecycle(c: Case) {
    c.accept_feature();
    let preview = c.preview(REQUEST);
    assert!(preview.status.success(), "{preview:?}");
    let preview = value(&preview);
    let applied = c.apply(&preview);
    assert!(applied.status.success(), "{applied:?}");
    let response = value(&applied);
    let work = response["work"].as_str().unwrap();
    assert_ne!(work, c.old);
    let start = c.f.events(work).remove(0);
    assert!(start.get("governor").is_none());
    assert_eq!(start["quarantineVerification"]["oldCredited"], false);
    assert_eq!(start["quarantineVerification"]["oldReset"], false);
    // Admission advances the owner Goal and therefore the old Work action
    // binding. Refresh navigation; the same request must reuse the same Work.
    assert!(!c.apply(&preview).status.success());
    let refreshed = c.preview(REQUEST);
    assert!(refreshed.status.success(), "{refreshed:?}");
    let repeated = c.apply(&value(&refreshed));
    assert!(repeated.status.success(), "{repeated:?}");
    assert_eq!(value(&repeated)["work"], work);
    let conflicting = c.preview("another-request");
    if conflicting.status.success() {
        assert!(!c.apply(&value(&conflicting)).status.success());
    }
    c.return_frozen(work);
    let inspection = c.f.ok(
        &[
            "run",
            "inspect",
            &format!(".exitbind/runs/work-{}.jsonl", &work[4..]),
            "--json",
        ],
        b"",
    );
    let before = fs::read(c.f.ledger(work)).unwrap();
    let target = inspection["events"].as_array().unwrap().last().unwrap()["eventSha256"]
        .as_str()
        .unwrap();
    assert!(!c
        .f
        .call(
            &[
                "run",
                "record-check",
                &format!(".exitbind/runs/work-{}.jsonl", &work[4..]),
                "--target",
                target,
                "--check-command",
                "exec sh checker.sh",
                "--exit-code",
                "0"
            ],
            b""
        )
        .status
        .success());
    assert_eq!(fs::read(c.f.ledger(work)).unwrap(), before);
    c.f.ok(&["work", "check", work, "--timeout-ms", "2000"], b"");
    let reviewer = c.f.next(work);
    assert_eq!(reviewer["role"], "reviewer");
    let lead = c.f.give(work, &reviewer, "approved")["next"].clone();
    c.f.give(work, &lead, "accepted");
    assert_eq!(fs::read(c.f.ledger(&c.old)).unwrap(), c.old_bytes);
    assert_eq!(
        c.f.events(work)
            .iter()
            .filter(|e| e["action"] == "check_observation")
            .count(),
        1
    );
    assert!(!c.f.call(&["work", "check", work], b"").status.success());
    let goal = c.owner_ok(&[
        "goal",
        "quarantine",
        &c.old,
        "--source-config",
        c.f.root.join("exitbind.json").to_str().unwrap(),
        "--verification-work",
        work,
        "--request-id",
        REQUEST,
        "--reason",
        REASON,
        "--json",
    ]);
    let recorded = c.apply(&goal);
    assert!(recorded.status.success(), "{recorded:?}");
    assert_eq!(value(&recorded)["residual"], "quarantined_uncredited");
    let status = c.owner_ok(&["goal", "status", "--json"]);
    assert_eq!(status["journeyMembershipProtocol"], 3);
    assert_eq!(
        status["quarantineResiduals"]["residuals"][0]["old"]["work"],
        c.old
    );
    assert_eq!(
        status["quarantineResiduals"]["residuals"][0]["oldMember"]["work"],
        c.original_member
    );
    if c.old != c.original_member {
        let members = status["journeyMembers"].as_array().unwrap();
        assert!(members
            .iter()
            .any(|member| member["work"] == c.original_member));
        assert!(!members.iter().any(|member| member["work"] == c.old));
    }
    c.owner_ok(&[
        "goal",
        "incorporate",
        "--goal-id",
        "original-quarantine-goal",
        "--goal",
        "Preserve all original outcomes and unknown evidence",
        "--decision",
        "Continue original outcomes after recording the residual",
        "--none-applicable",
        "findings,blockers,externalActions",
        "--json",
    ]);
    let preserved = c.owner_ok(&["goal", "status", "--json"]);
    assert_eq!(
        preserved["quarantineResiduals"],
        status["quarantineResiduals"]
    );
    assert_eq!(preserved["journeyMembershipProtocol"], 3);
    assert_eq!(fs::read(c.f.ledger(&c.old)).unwrap(), c.old_bytes);
}
