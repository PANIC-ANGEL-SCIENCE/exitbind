//! R55 F7a: a check that failed for an environmental cause (a lock, not a
//! test) is re-run on the same result after the Lead's attestation, with the
//! failed observation kept and no worker re-return.
use super::*;
use serde_json::json;

struct EnvironmentalWork {
    fixture: Fixture,
    lock: PathBuf,
    work: String,
}

impl Drop for EnvironmentalWork {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock);
    }
}

/// A marked Work whose frozen check exits 101 while a lock outside the
/// project exists, returned once by its worker and checked once under the lock.
fn failed_under_lock(label: &str) -> EnvironmentalWork {
    let fixture = Fixture::new();
    let lock = fixture.root.with_extension(label);
    fs::write(&lock, b"held by a concurrent cargo").unwrap();
    let check = format!(
        "if test -e '{}'; then echo 'Blocking waiting for file lock on build directory' >&2; exit 101; fi; printf ok",
        lock.display()
    );
    let started = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "environmental check failure",
        "--check-command",
        &check,
        "--review-policy",
        "required",
        "--standalone",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    fixture.value(&[
        "work",
        "return",
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = fixture.value(&["work", "next", &work]);
    fixture.value(&[
        "work",
        "return",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    let failed = fixture.failed_value(&["work", "check", &work]);
    assert_eq!(failed["next"]["progress"]["reason"]["code"], "check_failed");
    EnvironmentalWork {
        fixture,
        lock,
        work,
    }
}

fn action_view(fixture: &Fixture, work: &str) -> Value {
    fixture.value(&["work", "detail", work, "--view", "action", "--json"])
}

fn choice<'a>(view: &'a Value, label: &str) -> Option<&'a Value> {
    view["effectiveAction"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["label"] == label)
}

fn check_obligation(view: &Value) -> Value {
    view["decision"]["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "check")
        .cloned()
        .unwrap()
}

/// Record the attestation through the fill route of the Lead's printed form.
fn attest(fixture: &Fixture, work: &str, reason: &str) -> Output {
    let view = action_view(fixture, work);
    let form = choice(&view, "environmental").expect("the Lead form offers environmental");
    assert_eq!(form["fill"]["options"], json!({"REASON": "--reason"}));
    assert_eq!(form["stdin"]["required"], false);
    let binding = view["effectiveAction"]["binding"].as_str().unwrap();
    fixture.call(&[
        "work",
        "fill",
        work,
        "--current-binding",
        binding,
        "--choice",
        "environmental",
        "--reason",
        reason,
    ])
}

#[test]
fn environmental_failure_reruns_the_same_target_without_a_worker_return() {
    let case = failed_under_lock("env-lock");
    let (fixture, work) = (&case.fixture, case.work.as_str());
    let ledger = fixture.ledger(work);
    let failed = observed_checks(&ledger).pop().unwrap();
    assert_eq!(failed["result"]["code"], 101);

    // Without the attestation the same target only replays its failure.
    fs::remove_file(&case.lock).unwrap();
    fixture.failed_value(&["work", "check", work]);
    assert_eq!(observed_checks(&ledger).len(), 1);
    let before = action_view(fixture, work);
    assert_eq!(check_obligation(&before)["status"], "failed");

    let attested = attest(fixture, work, "a concurrent cargo held the build lock");
    assert!(attested.status.success(), "{attested:?}");
    let disposition = events(&ledger)
        .into_iter()
        .find(|event| event["outcome"] == "disposition")
        .unwrap();
    assert_eq!(disposition["role"], "lead");
    assert_eq!(disposition["disposition"]["decision"], "environmental");
    assert_eq!(
        disposition["disposition"]["reason"],
        "a concurrent cargo held the build lock"
    );
    assert_eq!(
        disposition["disposition"]["findingSha256s"],
        json!([failed["eventSha256"]])
    );

    let pending = fixture.value(&["work", "next", work]);
    assert_eq!(pending["next"]["action"], "check");
    let obligation = check_obligation(&action_view(fixture, work));
    assert_eq!(obligation["status"], "missing");
    assert_eq!(obligation["targetEventSha256"], failed["targetEventSha256"]);
    assert!(pending["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap()
        .contains("attested the earlier functional check failure environmental"));
    // A second attestation without a new failed observation refuses.
    let again = fixture.call(&[
        "work",
        "disposition",
        work,
        before["effectiveAction"]["assignment"].as_str().unwrap(),
        "--decision",
        "environmental",
        "--reason",
        "again",
    ]);
    assert!(!again.status.success(), "{again:?}");
    // The Lead decision is no longer pending: the target waits for a check.
    assert!(
        String::from_utf8_lossy(&again.stderr)
            .contains("assignment is not the current pending Lead decision"),
        "{again:?}"
    );

    let rerun = fixture.value(&["work", "check", work]);
    let checks = observed_checks(&ledger);
    assert_eq!(checks.len(), 2, "both observations stay in the ledger");
    assert_eq!(checks[0], failed);
    assert_eq!(checks[1]["result"]["code"], 0);
    for field in ["targetEventSha256", "subjectSha256", "inputsSha256"] {
        assert_eq!(checks[1][field], failed[field], "{field}");
    }
    assert_eq!(rerun["next"]["action"], "spawn");
    assert_eq!(rerun["next"]["role"], "reviewer");
    let review = action_view(fixture, work);
    assert!(choice(&review, "approved").is_some(), "{review}");
    let all = events(&ledger);
    let worker_returns = all
        .iter()
        .filter(|event| event["role"] == "worker" && event["action"] == "submit")
        .count();
    assert_eq!(worker_returns, 1, "no worker re-return");
    assert!(
        all.iter()
            .all(|event| event["governorEvent"]["action"] != "replan"),
        "no replan"
    );
}

#[test]
fn a_seeded_second_attestation_without_a_new_observation_refuses() {
    let case = failed_under_lock("env-lock-seeded");
    let (fixture, work) = (&case.fixture, case.work.as_str());
    let attested = attest(fixture, work, "the build lock was held");
    assert!(attested.status.success(), "{attested:?}");
    let ledger = fixture.ledger(work);
    let mut all = events(&ledger);
    let mut second = all.last().unwrap().clone();
    assert_eq!(second["disposition"]["decision"], "environmental");
    second["previousEventSha256"] = all.last().unwrap()["eventSha256"].clone();
    second.as_object_mut().unwrap().remove("eventSha256");
    second["eventSha256"] = json!(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&second).unwrap())
    ));
    all.push(second);
    let text = all
        .iter()
        .map(|event| serde_json::to_string(event).unwrap() + "\n")
        .collect::<String>();
    fs::write(&ledger, text).unwrap();
    let refused = fixture.call(&["work", "next", work, "--json"]);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(String::from_utf8_lossy(&refused.stdout).contains("corrupt_ledger"));
    let relative = ledger
        .strip_prefix(&fixture.root)
        .unwrap()
        .to_str()
        .unwrap();
    let refused = fixture.call(&["run", "inspect", relative, "--json"]);
    assert!(!refused.status.success(), "{refused:?}");
    let stdout = String::from_utf8_lossy(&refused.stdout);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        format!("{stdout}{stderr}")
            .contains("environmental attestation needs a new failed observation"),
        "{refused:?}"
    );
}

/// F7a review D1: re-run with the lock still held. The same failure adds no
/// governor observation; the new failed observation makes the attestation
/// available again, naming only the new check.
#[test]
fn a_rerun_under_the_same_lock_adds_no_governor_observation_and_offers_only_the_new_failure() {
    let case = failed_under_lock("env-lock-held");
    let (fixture, work) = (&case.fixture, case.work.as_str());
    let ledger = fixture.ledger(work);
    let relative = ledger
        .strip_prefix(&fixture.root)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let governed = || {
        let inspected = fixture.value(&["run", "inspect", &relative, "--json"]);
        inspected["governor"]["observations"]
            .as_array()
            .unwrap_or_else(|| panic!("{inspected}"))
            .len()
    };
    let observed = governed();
    let attested = attest(fixture, work, "the build lock was held");
    assert!(attested.status.success(), "{attested:?}");

    let failed = fixture.failed_value(&["work", "check", work]);
    assert_eq!(failed["next"]["progress"]["reason"]["code"], "check_failed");
    let checks = observed_checks(&ledger);
    assert_eq!(checks.len(), 2, "both failed observations are kept");
    assert_eq!(checks[1]["result"]["code"], 101);
    assert_eq!(
        governed(),
        observed,
        "the same failure is no new governor observation"
    );

    let view = action_view(fixture, work);
    assert!(choice(&view, "environmental").is_some(), "{view}");
    let again = attest(fixture, work, "the build lock is still held");
    assert!(again.status.success(), "{again:?}");
    let dispositions: Vec<Value> = events(&ledger)
        .into_iter()
        .filter(|event| event["outcome"] == "disposition")
        .collect();
    assert_eq!(dispositions.len(), 2);
    assert_eq!(
        dispositions[1]["disposition"]["findingSha256s"],
        json!([checks[1]["eventSha256"]]),
        "the second attestation names only the new failed check"
    );
}
