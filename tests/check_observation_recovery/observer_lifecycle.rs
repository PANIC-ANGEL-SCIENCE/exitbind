use super::*;
use std::{
    fs,
    process::Stdio,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
#[test]
fn a_never_run_checker_offers_the_environmental_route_and_re_pends_the_same_check() {
    // The action view must hold its 8 KiB bound while the executable,
    // project and configuration paths are long, not just in a short temp root.
    let long_parent = support::temp("long-project-path");
    let mut parent = long_parent.to_path_buf();
    for index in 0..2 {
        parent.push(format!(
            "padded-project-root-component-{index}-{}",
            "x".repeat(30)
        ));
    }
    fs::create_dir_all(&parent).unwrap();
    let f = Fixture::new_in(Some(&parent), "printf launched > .exitbind/launched\n");
    assert!(
        f.root.to_str().unwrap().len() >= 90,
        "fixture project path is not long: {}",
        f.root.display()
    );
    let config_path = f.root.join("exitbind.json");
    assert!(
        config_path.to_str().unwrap().len() >= 100,
        "fixture configuration path is not long: {}",
        config_path.display()
    );
    let work = f.begin();
    let bin_root = support::temp("capsule-no-shell-environmental");
    let no_shell = support::git_topology::git_only_path(&bin_root);
    let output = f
        .command(&["work", "check", &work, "--timeout-ms", "1000"])
        .env("PATH", no_shell)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(
        value(&output)["observationFailure"]["facts"]["processStarted"],
        false
    );

    // The Lead's form offers the never-ran environmental attestation; a
    // checker that never started is not labelled a defective contract.
    let output = f.call(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    assert!(output.status.success(), "{output:?}");
    let view = value(&output);
    // The navigation part (delivered guidance keeps its own budget) holds the
    // compact response bound that this long path would otherwise exceed.
    let mut navigation = view.clone();
    navigation.as_object_mut().unwrap().remove("guidance");
    let navigation_bytes = serde_json::to_vec(&navigation).unwrap().len() + 1;
    assert!(
        navigation_bytes <= 8192,
        "action-view navigation is {navigation_bytes} bytes under {}",
        f.root.display()
    );
    let choices = view["effectiveAction"]["choices"].as_array().unwrap();
    let environmental = choices
        .iter()
        .find(|choice| choice["label"] == "environmental")
        .unwrap_or_else(|| panic!("no environmental route: {view}"));
    assert!(environmental["fill"]["options"].get("REASON").is_some());
    assert!(!choices
        .iter()
        .any(|choice| choice["label"] == "defective-contract"));
    let binding = view["effectiveAction"]["binding"]
        .as_str()
        .unwrap()
        .to_owned();
    let worker_returns = f
        .events(&work)
        .iter()
        .filter(|event| event["action"] == "submit" && event["role"] == "worker")
        .count();

    let attested = f.call(
        &[
            "work",
            "fill",
            &work,
            "--current-binding",
            &binding,
            "--choice",
            "environmental",
            "--reason",
            "the checker could not be launched in this capsule",
        ],
        b"",
    );
    assert!(attested.status.success(), "{attested:?}");
    let events = f.events(&work);
    assert!(events.iter().any(|event| event["action"] == "submit"
        && event["role"] == "lead"
        && event["outcome"] == "disposition"
        && event["disposition"]["decision"] == "environmental"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "submit" && event["role"] == "worker")
            .count(),
        worker_returns,
        "the attestation needs no second worker return"
    );

    // The same check target is pending again: a new check runs the checker.
    assert!(!f.root.join(".exitbind/launched").exists());
    let checked = f.call(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    assert!(checked.status.success(), "{checked:?}");
    assert!(f.root.join(".exitbind/launched").exists());
    let events = f.events(&work);
    assert!(events
        .iter()
        .any(|event| event["action"] == "check_observation_failed"));
    assert!(events.iter().any(|event| event["action"] == "check"));
    fs::remove_dir_all(bin_root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn a_dead_observer_closes_its_admission_and_a_live_observer_is_never_closed() {
    let f = Fixture::new("printf started > .exitbind/checker-started; sleep 5; printf ok\n");
    let work = f.begin();
    let mut observer = f
        .command(&["work", "check", &work, "--timeout-ms", "60000"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let admission = loop {
        let started = f.root.join(".exitbind/checker-started").exists();
        let admission = fs::read_to_string(f.ledger(&work)).ok().and_then(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find(|event| event["action"] == "check_observation")
        });
        if started {
            if let Some(admission) = admission {
                break admission;
            }
        }
        assert!(Instant::now() < deadline, "the check never admitted");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        admission["observation"]["observer"]["pid"]
            .as_u64()
            .unwrap(),
        u64::from(observer.id())
    );

    // A live observer is never closed: the second check is refused and the
    // ledger keeps the running admission.
    let refused = f.call(&["work", "check", &work], b"");
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("admitted, no end recorded yet"),
        "{refused:?}"
    );
    assert!(!f
        .events(&work)
        .iter()
        .any(|event| event["action"] == "check_observation_failed"));

    // A killed observer is found ended by its recorded start marker; the next
    // check closes the admission as failed/unknown without a check result.
    observer.kill().unwrap();
    observer.wait().unwrap();
    let closed = f.call(&["work", "check", &work, "--timeout-ms", "1000"], b"");
    assert!(!closed.status.success(), "{closed:?}");
    let failure = value(&closed);
    let facts = &failure["observationFailure"]["facts"];
    assert!(facts["process"].is_null());
    assert_eq!(facts["processStarted"], true);
    assert_eq!(facts["groupEnded"], false);
    assert_eq!(facts["termination"], "unknown");
    let events = f.events(&work);
    assert!(events
        .iter()
        .any(|event| event["action"] == "check_observation_failed"
            && event["observation"]["admissionEventSha256"] == admission["eventSha256"]));
    assert!(!events.iter().any(|event| event["action"] == "check"
        && event["observationEventSha256"] == admission["eventSha256"]));

    // The existing quarantine route owns the unknown termination.
    let lead = f.next(&work);
    f.give(&work, &lead, "blocked");
    let detail = f.ok(&["work", "detail", &work, "--json"], b"");
    assert_eq!(
        detail["actionForms"]["state"],
        "blocked_unresolved_check_observation"
    );
    let diagnostic = f.ok(&["work", "recover-check", &work, "--json"], b"");
    assert_eq!(diagnostic["status"], "historical_unknown_termination");
    assert_eq!(diagnostic["observation"]["termination"], "unknown");
    assert_eq!(diagnostic["checkRecorded"], false);
}
