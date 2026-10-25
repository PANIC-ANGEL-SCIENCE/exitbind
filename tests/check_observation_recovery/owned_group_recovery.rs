//! A timed-out observation whose recorded owned process group outlives the
//! deadline: while any member is alive the Lead form names it and offers no
//! forward route; once the group is observed ended the existing environmental
//! re-run route re-pends the same check. Never an automatic retry.
#![cfg(target_os = "linux")]
use super::*;
use std::os::unix::process::CommandExt;

/// The checker records its own owned group, then waits (bounded) until the
/// fixture releases it. The group survives the deadline because the fixture
/// joins a helper to it from outside the capsule's child inventory.
const OWNED_GROUP_SCRIPT: &str = "\
if [ -f .exitbind/rerun ]; then printf 'ok\\n'; exit 0; fi
printf '%s' $$ > .exitbind/owned-group
i=0
while [ ! -f .exitbind/release ] && [ \"$i\" -lt 160 ]; do sleep 0.05; i=$((i + 1)); done
exit 0
";

fn group_present(group: i32) -> bool {
    (unsafe { libc::kill(-group, 0) }) == 0
}

fn process_group_of(pid: u32) -> Option<i32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields.split_whitespace().nth(2)?.parse().ok()
}

/// The helper joined to the checker's group, with the fixture's release file.
/// Stopping is idempotent and bounded so a failed assertion cannot leave the
/// group alive.
struct JoinedGroup {
    root: PathBuf,
    group: i32,
    helper: Option<std::process::Child>,
}

impl JoinedGroup {
    fn stop(&mut self) {
        fs::write(self.root.join(".exitbind/release"), b"release").unwrap();
        if let Some(mut helper) = self.helper.take() {
            let _ = helper.kill();
            let _ = helper.wait();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while group_present(self.group) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if group_present(self.group) {
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while group_present(self.group) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

impl Drop for JoinedGroup {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn a_timed_out_owned_group_closes_onto_the_environmental_rerun_once_it_ends() {
    let f = Fixture::new(OWNED_GROUP_SCRIPT);
    let work = f.begin();

    // The check outlives its short deadline; a helper joins the recorded group
    // from outside the capsule's own child inventory, so cleanup cannot reach it.
    let check = f
        .command(&["work", "check", &work, "--timeout-ms", "2500"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let marker = f.root.join(".exitbind/owned-group");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "the checker never recorded its group"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let group = fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let helper = Command::new("sh")
        .args(["-c", "sleep 60", "owned-group-helper"])
        .process_group(group)
        .spawn()
        .unwrap();
    let helper_pid = helper.id();
    let joined = Instant::now() + Duration::from_secs(2);
    while process_group_of(helper_pid) != Some(group) {
        assert!(Instant::now() < joined, "the helper never joined the group");
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut owned = JoinedGroup {
        root: f.root.to_path_buf(),
        group,
        helper: Some(helper),
    };

    let failed = check.wait_with_output().unwrap();
    assert!(!failed.status.success(), "{failed:?}");
    let failure = value(&failed);
    let facts = &failure["observationFailure"]["facts"];
    assert_eq!(facts["processStarted"], true);
    assert_eq!(facts["deadlineExceeded"], true, "{failure}");
    assert_eq!(facts["groupEnded"], false, "{failure}");
    assert_eq!(facts["termination"], "unknown", "{failure}");
    assert_eq!(facts["ownedProcessGroup"], json!(group), "{failure}");
    // The capsule records the owned leader's start marker with the group, so a
    // reused group id is never mistaken for this one.
    assert!(
        facts["ownedProcessStart"]
            .as_u64()
            .is_some_and(|ticks| ticks > 0),
        "{failure}"
    );

    // No automatic retry: the failed observation still owns the check, so the
    // same admitted execution limit is refused rather than launched again.
    let refused = f.call(&["work", "check", &work, "--timeout-ms", "2500"], b"");
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("failed without a check result"),
        "{refused:?}"
    );

    // While the group lives the Lead form names its processes and closes every
    // forward route: the environmental choice is not offered.
    let view = f.ok(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    let form = &view["effectiveAction"];
    assert_eq!(form["formState"], "unresolved_check_observation", "{view}");
    assert_eq!(form["ownedProcessGroup"], json!(group), "{view}");
    assert_eq!(form["ownedProcessState"], "alive", "{view}");
    let named = form["ownedProcesses"].as_array().unwrap();
    assert!(
        named
            .iter()
            .any(|process| process["pid"] == json!(helper_pid)
                && process["command"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("owned-group-helper")),
        "{view}"
    );
    let meaning = form["meaning"].as_str().unwrap_or_default();
    assert!(meaning.contains(&helper_pid.to_string()), "{view}");
    assert!(meaning.contains("No forward route is available"), "{view}");
    let labels = form["choices"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|choice| choice["label"].as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"blocked") && labels.contains(&"rejected"),
        "{view}"
    );
    assert!(!labels.contains(&"environmental"), "{view}");
    assert!(!f
        .events(&work)
        .iter()
        .any(|event| event["action"] == "check" && event["observationEventSha256"].is_string()));

    // The closure holds at the reducer, not only in the form: the same
    // environmental decision is refused while the group lives and records
    // nothing.
    let refused_decision = f.call(
        &[
            "work",
            "disposition",
            &work,
            form["assignment"].as_str().unwrap(),
            "--current-binding",
            form["binding"].as_str().unwrap(),
            "--decision",
            "environmental",
            "--reason",
            "premature attestation",
        ],
        b"",
    );
    assert!(!refused_decision.status.success(), "{refused_decision:?}");
    assert!(!f
        .events(&work)
        .iter()
        .any(|event| event["action"] == "submit"
            && event["role"] == "lead"
            && event["outcome"] == "disposition"));

    // The owned group is observed ended; the same read now offers the existing
    // environmental re-run route on this Work.
    owned.stop();
    assert!(!group_present(group), "the owned group is still alive");
    let view = f.ok(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    let form = &view["effectiveAction"];
    assert_eq!(form["formState"], "unresolved_check_observation", "{view}");
    assert_eq!(form["ownedProcessGroup"], json!(group), "{view}");
    assert_eq!(form["ownedProcessState"], "ended", "{view}");
    assert_eq!(form["ownedProcesses"], json!([]), "{view}");
    let environmental = form["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["label"] == "environmental")
        .unwrap_or_else(|| panic!("no environmental route once the group ended: {view}"));
    assert!(environmental["fill"]["options"].get("REASON").is_some());
    assert!(form["meaning"]
        .as_str()
        .unwrap_or_default()
        .contains("environmental re-run route is available"));
    let binding = form["binding"].as_str().unwrap().to_owned();
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
            "the timed-out checker's owned processes have all ended",
        ],
        b"",
    );
    assert!(attested.status.success(), "{attested:?}");
    let events = f.events(&work);
    let disposition = events
        .iter()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "lead"
                && event["outcome"] == "disposition"
                && event["disposition"]["decision"] == "environmental"
        })
        .unwrap_or_else(|| panic!("no environmental submit event: {events:?}"));
    // The decision records the state it observed, so replay validates the
    // fact instead of the host's process table.
    assert_eq!(disposition["ownedProcessState"], "ended", "{disposition}");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "submit" && event["role"] == "worker")
            .count(),
        worker_returns,
        "the attestation needs no second worker return"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "check_observation_failed")
            .count(),
        1
    );

    // The same frozen check is pending again and its re-run admits it.
    fs::write(f.root.join(".exitbind/rerun"), b"rerun").unwrap();
    let checked = f.call(&["work", "check", &work, "--timeout-ms", "3000"], b"");
    assert!(checked.status.success(), "{checked:?}");
    let events = f.events(&work);
    let admissions = events
        .iter()
        .filter(|event| event["action"] == "check_observation")
        .collect::<Vec<_>>();
    assert_eq!(admissions.len(), 2, "{events:?}");
    assert!(events.iter().any(|event| event["action"] == "check"
        && event["observationEventSha256"] == admissions[1]["eventSha256"]));
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "check_observation_failed")
            .count(),
        1
    );
}
