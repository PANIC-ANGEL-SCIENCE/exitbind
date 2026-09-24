mod support;

use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("evidence-expansion-v022");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        Self { root, config }
    }

    fn call(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&self.root).args(args);
        if args.first() == Some(&"work") && matches!(args.get(1), Some(&"next" | &"resume")) {
            command.arg("--full");
        }
        command.arg("--config").arg(&self.config).output().unwrap()
    }

    fn call_with_env(&self, args: &[&str], key: &str, value: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .env(key, value)
            .output()
            .unwrap()
    }

    #[cfg(unix)]
    fn call_with_file_limit(&self, args: &[&str], limit: u64) -> Output {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config);
        unsafe {
            command.pre_exec(move || {
                if libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR {
                    return Err(std::io::Error::last_os_error());
                }
                let limit = libc::rlim_t::try_from(limit).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "file limit overflow")
                })?;
                let resource = libc::rlimit {
                    rlim_cur: limit,
                    rlim_max: limit,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &resource) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ledger_path(&self, work: &str) -> PathBuf {
        self.root.join(ledger_reference(work))
    }

    fn artifact_path(&self, work: &str, stream: &str) -> PathBuf {
        let source = fs::read_to_string(self.ledger_path(work)).unwrap();
        let event: Value = serde_json::from_str(source.lines().last().unwrap()).unwrap();
        self.root.join(event[stream]["path"].as_str().unwrap())
    }

    fn return_body(&self, work: &str, assignment: &str, outcome: &str, body: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(["work", "return", work, assignment, "--outcome", outcome])
            .arg("--config")
            .arg(&self.config)
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }

    fn start_worker(&self, command: &str) -> (Value, String, String, String) {
        let started = self.json(&[
            "work",
            "begin",
            "change",
            "--goal",
            "v0.22 exact evidence",
            "--check-command",
            command,
        ]);
        let work = started["work"].as_str().unwrap().to_owned();
        let lead = self.return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        );
        assert!(lead.status.success(), "{lead:?}");
        let worker = self.json(&["work", "next", &work]);
        let completed = self.return_body(
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "completed",
            b"worker\n",
        );
        assert!(completed.status.success(), "{completed:?}");
        let ledger = ledger_reference(&work);
        let source = fs::read_to_string(self.root.join(&ledger)).unwrap();
        let target = serde_json::from_str::<Value>(source.lines().last().unwrap()).unwrap()
            ["eventSha256"]
            .as_str()
            .unwrap()
            .to_owned();
        (started, work, ledger, target)
    }
}

fn ledger_reference(work: &str) -> String {
    format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    )
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn assert_no_capture_artifacts(fixture: &Fixture) {
    let artifacts = fixture.root.join(".exitbind/artifacts");
    let leftovers = fs::read_dir(artifacts)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.contains("check-log") && (name.ends_with(".raw") || name.ends_with(".tmp"))
        })
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "leftover capture artifacts: {leftovers:?}"
    );
}

fn assert_no_ledger_stage_artifacts(fixture: &Fixture) {
    let runs = fixture.root.join(".exitbind/runs");
    let leftovers = fs::read_dir(runs)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".append-") && name.ends_with(".tmp"))
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "leftover ledger stage artifacts: {leftovers:?}"
    );
}

#[test]
fn v8_context_refs_expand_exactly_and_refuse_stale_cross_work_and_traversal() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "history expansion",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let history = started["next"]["packet"]["context"]["expansions"][0].clone();
    assert_eq!(started["next"]["packet"]["context"]["version"], 3);
    assert!(history.get("root").is_none());
    assert!(history.get("path").is_none());
    assert_eq!(history["sha256"].as_str().unwrap().len(), 64);
    let history_arg = history["id"].as_str().unwrap().to_owned();
    let expanded = fixture.call(&["work", "expand", &work, &history_arg]);
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    let source = fs::read(fixture.ledger_path(&work)).unwrap();
    assert_eq!(expanded["encoding"], "hex");
    assert_eq!(expanded["bytes"], source.len());
    assert_eq!(expanded["contentHex"], hex(&source));

    let lead = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(lead.status.success(), "{lead:?}");
    let current = fixture.json(&["work", "next", &work]);
    let event_ref = current["next"]["packet"]["context"]["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("rawEventRef"))
        .cloned()
        .unwrap();
    let event_arg = event_ref["id"].as_str().unwrap().to_owned();
    let event = fixture.call(&["work", "expand", &work, &event_arg]);
    assert!(event.status.success(), "{event:?}");
    let event: Value = serde_json::from_slice(&event.stdout).unwrap();
    assert_eq!(event["kind"], "event");
    assert_eq!(event["encoding"], "hex");
    assert_eq!(
        event["contentHex"].as_str().unwrap().len(),
        event["bytes"].as_u64().unwrap() as usize * 2
    );

    let object_ref = format!(r#"{{"id":"{event_arg}"}}"#);
    assert!(
        !fixture
            .call(&["work", "expand", &work, &object_ref])
            .status
            .success(),
        "caller-supplied JSON reference was accepted"
    );
    assert!(
        !fixture
            .call(&["work", "expand", &work, &ledger_reference(&work)])
            .status
            .success(),
        "path reference was accepted"
    );
    assert!(
        !fixture
            .call(&[
                "work",
                "expand",
                &work,
                "ref:0000000000000000000000000000000000000000000000000000000000000000",
            ])
            .status
            .success(),
        "wrong-kind or forged reference was accepted"
    );

    let ledger_path = fixture.ledger_path(&work);
    let original = fs::read(&ledger_path).unwrap();
    let mut tampered = original.clone();
    tampered[0] ^= 1;
    fs::write(&ledger_path, tampered).unwrap();
    assert!(
        !fixture
            .call(&["work", "expand", &work, &event_arg])
            .status
            .success(),
        "tampered ledger expanded"
    );
    fs::write(&ledger_path, original).unwrap();

    let stale = fixture.call(&["work", "expand", &work, &history_arg]);
    assert!(!stale.status.success(), "stale history reference expanded");
    let traversal = fixture.call(&["work", "expand", &work, "../outside"]);
    assert!(!traversal.status.success(), "traversal reference expanded");
    let other = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "different work",
        "--check-command",
        "true",
    ]);
    let other_work = other["work"].as_str().unwrap();
    let cross = fixture.call(&["work", "expand", other_work, &event_arg]);
    assert!(!cross.status.success(), "cross-work reference expanded");
}

#[cfg(unix)]
#[test]
fn symlinked_ledger_is_refused_even_when_target_bytes_match() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "symlink ledger",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap();
    let reference = started["next"]["packet"]["context"]["expansions"][0].clone();
    assert!(reference.get("root").is_none());
    assert!(reference.get("path").is_none());
    let path = fixture.ledger_path(work);
    let backup = path.with_extension("jsonl.backup");
    fs::rename(&path, &backup).unwrap();
    symlink(&backup, &path).unwrap();
    let refused = fixture.call(&["work", "expand", work, reference["id"].as_str().unwrap()]);
    assert!(!refused.status.success(), "symlinked ledger expanded");
}

#[test]
fn observed_v8_logs_round_trip_and_failure_paths_leave_no_logs() {
    let fixture = Fixture::new();
    let (_started, work, _ledger, _target) =
        fixture.start_worker("printf '\\001\\000X'; printf '\\377\\n' >&2; exit 7");
    let checked = fixture.json(&["work", "check", &work]);
    let context = &checked["next"]["packet"]["context"];
    let reference = context["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("checkLogRef"))
        .cloned()
        .unwrap();
    let argument = reference["id"].as_str().unwrap().to_owned();
    let expanded = fixture.call(&["work", "expand", &work, &argument]);
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["event"]["result"]["code"], 7);
    assert_eq!(expanded["stdout"]["contentHex"], "010058");
    assert_eq!(expanded["stderr"]["contentHex"], "ff0a");
    let check = context["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "check")
        .unwrap();
    assert_eq!(check["logStatus"], "available");
    assert_ne!(check["targetEventSha256"], check["checkEventSha256"]);
    assert!(reference["stdout"].get("path").is_none());
    assert!(reference["stderr"].get("path").is_none());
    assert!(reference["stdout"].get("id").is_none());
    assert!(reference["stderr"].get("id").is_none());
    let check_event_ref = check["checkEventRef"]["id"].as_str().unwrap();
    let check_event = fixture.call(&["work", "expand", &work, check_event_ref]);
    assert!(check_event.status.success(), "{check_event:?}");
    let check_event: Value = serde_json::from_slice(&check_event.stdout).unwrap();
    assert_eq!(
        check_event["event"]["eventSha256"],
        check["checkEventSha256"]
    );
    let stdout_path = fixture.artifact_path(&work, "stdout");
    let stdout = fs::read(&stdout_path).unwrap();
    let mut changed = stdout.clone();
    changed[0] ^= 1;
    fs::write(&stdout_path, changed).unwrap();
    let refused = fixture.call(&["work", "expand", &work, &argument]);
    assert!(!refused.status.success(), "mutated log artifact expanded");
    fs::write(&stdout_path, stdout).unwrap();
    fs::remove_file(stdout_path).unwrap();
    let refused = fixture.call(&["work", "expand", &work, &argument]);
    assert!(!refused.status.success(), "deleted log artifact expanded");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let symlink_fixture = Fixture::new();
        let (_started, symlink_work, _ledger, _target) = symlink_fixture.start_worker("printf ok");
        let checked = symlink_fixture.json(&["work", "check", &symlink_work]);
        let reference = checked["next"]["packet"]["context"]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|item| item.get("checkLogRef"))
            .unwrap()
            .clone();
        assert!(reference["stdout"].get("path").is_none());
        let stdout = symlink_fixture.artifact_path(&symlink_work, "stdout");
        let target = symlink_fixture.root.join("symlink-target.raw");
        fs::write(&target, b"ok").unwrap();
        fs::remove_file(&stdout).unwrap();
        symlink(&target, &stdout).unwrap();
        let refused = symlink_fixture.call(&[
            "work",
            "expand",
            &symlink_work,
            reference["id"].as_str().unwrap(),
        ]);
        assert!(!refused.status.success(), "symlinked log artifact expanded");
    }

    let signal = Fixture::new();
    let (_started, signal_work, _ledger, _target) = signal.start_worker("kill -TERM $$");
    let checked = signal.call(&["work", "check", &signal_work]);
    assert!(checked.status.success(), "{checked:?}");
    let signal_next: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let signal_ref = signal_next["next"]["packet"]["context"]["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("checkLogRef"))
        .unwrap();
    let signal_expanded = signal.call(&[
        "work",
        "expand",
        &signal_work,
        signal_ref["id"].as_str().unwrap(),
    ]);
    assert!(signal_expanded.status.success(), "{signal_expanded:?}");
    let signal_expanded: Value = serde_json::from_slice(&signal_expanded.stdout).unwrap();
    assert_eq!(signal_expanded["event"]["result"]["signal"], 15);

    let failure = Fixture::new();
    let (_started, _failure_work, failure_ledger, failure_target) =
        failure.start_worker("head -c 8388609 /dev/zero");
    let before = fs::read_to_string(failure.root.join(&failure_ledger)).unwrap();
    let output = failure.call(&[
        "run",
        "observe-check",
        &failure_ledger,
        "--target",
        &failure_target,
        "--timeout-ms",
        "1000",
    ]);
    assert!(!output.status.success(), "overflow unexpectedly recorded");
    assert_eq!(
        fs::read_to_string(failure.root.join(&failure_ledger)).unwrap(),
        before
    );
    let artifacts = failure.root.join(".exitbind/artifacts");
    let leftovers = fs::read_dir(artifacts)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.contains("check-log") && (name.ends_with(".raw") || name.ends_with(".tmp"))
        })
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "leftover capture artifacts: {leftovers:?}"
    );

    let drift = Fixture::new();
    let (_started, _drift_work, drift_ledger, drift_target) = drift.start_worker("sleep 1");
    let before_drift = fs::read_to_string(drift.root.join(&drift_ledger)).unwrap();
    let mut observing = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    observing
        .current_dir(&drift.root)
        .args([
            "run",
            "observe-check",
            &drift_ledger,
            "--target",
            &drift_target,
            "--timeout-ms",
            "2000",
        ])
        .arg("--config")
        .arg(&drift.config);
    let observing = observing.spawn().unwrap();
    thread::sleep(Duration::from_millis(100));
    OpenOptions::new()
        .append(true)
        .open(drift.root.join(&drift_ledger))
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let observed = observing.wait_with_output().unwrap();
    assert!(
        !observed.status.success(),
        "ledger drift unexpectedly recorded"
    );
    let after_drift = fs::read_to_string(drift.root.join(&drift_ledger)).unwrap();
    let before_events = before_drift
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let after_events = after_drift
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(before_events, after_events);
    let drift_artifacts = drift.root.join(".exitbind/artifacts");
    let drift_leftovers = fs::read_dir(drift_artifacts)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.contains("check-log") && (name.ends_with(".raw") || name.ends_with(".tmp"))
        })
        .collect::<Vec<_>>();
    assert!(
        drift_leftovers.is_empty(),
        "ledger drift left capture artifacts: {drift_leftovers:?}"
    );

    let timeout = Fixture::new();
    let (_started, timeout_work, timeout_ledger, timeout_target) = timeout.start_worker("sleep 1");
    let before_timeout = fs::read_to_string(timeout.root.join(&timeout_ledger)).unwrap();
    let output = timeout.call(&[
        "run",
        "observe-check",
        &timeout_ledger,
        "--target",
        &timeout_target,
        "--timeout-ms",
        "10",
    ]);
    assert!(!output.status.success(), "timeout unexpectedly recorded");
    assert_eq!(
        fs::read_to_string(timeout.root.join(&timeout_ledger)).unwrap(),
        before_timeout
    );
    assert!(timeout_work.starts_with("smw_"));

    let launch = Fixture::new();
    let (_started, _launch_work, launch_ledger, launch_target) = launch.start_worker("true");
    let before_launch = fs::read(launch.root.join(&launch_ledger)).unwrap();
    let output = launch.call_with_env(
        &[
            "run",
            "observe-check",
            &launch_ledger,
            "--target",
            &launch_target,
        ],
        "PATH",
        "/definitely-missing-exitbind-path",
    );
    assert!(
        !output.status.success(),
        "capture launch unexpectedly succeeded"
    );
    assert_eq!(
        fs::read(launch.root.join(&launch_ledger)).unwrap(),
        before_launch
    );
    assert_no_capture_artifacts(&launch);

    let read = Fixture::new();
    let (_started, _read_work, read_ledger, read_target) =
        read.start_worker("find .exitbind/artifacts -maxdepth 1 -name '*.tmp' -delete");
    let before_read = fs::read(read.root.join(&read_ledger)).unwrap();
    let output = read.call(&[
        "run",
        "observe-check",
        &read_ledger,
        "--target",
        &read_target,
    ]);
    assert!(
        !output.status.success(),
        "capture read unexpectedly succeeded"
    );
    assert_eq!(fs::read(read.root.join(&read_ledger)).unwrap(), before_read);
    assert_no_capture_artifacts(&read);

    #[cfg(unix)]
    {
        let write = Fixture::new();
        let (_started, write_work, write_ledger, _write_target) = write.start_worker("true");
        let before_write = fs::read(write.root.join(&write_ledger)).unwrap();
        let output =
            write.call_with_file_limit(&["work", "check", &write_work], before_write.len() as u64);
        assert!(
            !output.status.success(),
            "ledger stage write unexpectedly succeeded"
        );
        assert_eq!(
            fs::read(write.root.join(&write_ledger)).unwrap(),
            before_write
        );
        assert_no_capture_artifacts(&write);
        assert_no_ledger_stage_artifacts(&write);

        // The old direct-child-to-file capture let a child hit RLIMIT_FSIZE,
        // return success, and append a truncated log. The bounded pipe drain
        // makes the write failure observable to Exitbind instead.
        let capture_write = Fixture::new();
        let (_started, capture_work, capture_ledger, _capture_target) =
            capture_write.start_worker("trap '' TERM; head -c 4096 /dev/zero; exit 7");
        let before_capture_write = fs::read(capture_write.root.join(&capture_ledger)).unwrap();
        let output = capture_write.call_with_file_limit(&["work", "check", &capture_work], 1024);
        assert!(
            !output.status.success(),
            "capture writer failure unexpectedly succeeded: {output:?}"
        );
        let capture_text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            capture_text.contains("status=ExitStatus(unix_wait_status(1792))"),
            "the child exit status observed during cleanup was lost: {capture_text}"
        );
        assert!(capture_text.contains("capture=failed"), "{capture_text}");
        assert_eq!(
            fs::read(capture_write.root.join(&capture_ledger)).unwrap(),
            before_capture_write
        );
        assert_no_capture_artifacts(&capture_write);
        assert_no_ledger_stage_artifacts(&capture_write);

        // The check can finish with a real nonzero result before ledger
        // storage fails. Preserve that observed outcome while leaving the
        // append-only ledger unchanged and removing the owned captures.
        let append_failure = Fixture::new();
        let (_started, _append_work, append_ledger, append_target) =
            append_failure.start_worker("chmod u-w .exitbind/runs; printf captured; exit 7");
        let before_append = fs::read(append_failure.root.join(&append_ledger)).unwrap();
        let output = append_failure.call(&[
            "run",
            "observe-check",
            &append_ledger,
            "--target",
            &append_target,
        ]);
        assert!(
            !output.status.success(),
            "ledger append unexpectedly succeeded"
        );
        let failure_text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            failure_text.contains("status={\"code\":7,\"kind\":\"exit\"}")
                || failure_text.contains("status={\"kind\":\"exit\",\"code\":7}"),
            "{failure_text}"
        );
        assert!(failure_text.contains("durationMs="), "{failure_text}");
        assert!(
            failure_text.contains("capture=complete(stdout=8, stderr=0)"),
            "{failure_text}"
        );
        assert_eq!(
            fs::read(append_failure.root.join(&append_ledger)).unwrap(),
            before_append
        );
        fs::set_permissions(
            append_failure.root.join(".exitbind/runs"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert_no_capture_artifacts(&append_failure);
        assert_no_ledger_stage_artifacts(&append_failure);

        // A signal result is semantic check evidence, not capture
        // infrastructure failure. It must remain recorded with both log
        // artifacts present (the files are intentionally empty here).
        let signal = Fixture::new();
        let (_started, signal_work, signal_ledger, _signal_target) =
            signal.start_worker("kill -XFSZ $$");
        let checked = signal.call(&["work", "check", &signal_work]);
        assert!(checked.status.success(), "{checked:?}");
        let signal_source = fs::read_to_string(signal.root.join(signal_ledger)).unwrap();
        let signal_event: Value =
            serde_json::from_str(signal_source.lines().last().unwrap()).unwrap();
        assert_eq!(signal_event["result"]["kind"], "signal");
        assert_eq!(signal_event["result"]["signal"], 25);
        for stream in ["stdout", "stderr"] {
            let path = signal
                .root
                .join(signal_event[stream]["path"].as_str().unwrap());
            assert!(path.is_file(), "missing {stream} signal artifact");
        }

        // Both readers drain concurrently, and streams above the legacy 1 MiB
        // limit remain complete while staying within the new bounded cap.
        let exact = Fixture::new();
        let (_started, exact_work, exact_ledger, _exact_target) =
            exact.start_worker("head -c 2097152 /dev/zero; head -c 2097152 /dev/zero >&2; exit 0");
        let checked = exact.call(&["work", "check", &exact_work]);
        assert!(checked.status.success(), "{checked:?}");
        let exact_source = fs::read_to_string(exact.root.join(exact_ledger)).unwrap();
        let exact_event: Value =
            serde_json::from_str(exact_source.lines().last().unwrap()).unwrap();
        for stream in ["stdout", "stderr"] {
            assert_eq!(exact_event[stream]["bytes"], 2_097_152);
            assert_eq!(
                exact_event[stream]["sha256"],
                "5647f05ec18958947d32874eeb788fa396a05d0bab7c1b71f112ceb7e9b31eee"
            );
        }
        let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
        let log_ref = checked["next"]["packet"]["context"]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|item| item.get("checkLogRef"))
            .unwrap();
        let expanded = exact.call(&[
            "work",
            "expand",
            &exact_work,
            log_ref["id"].as_str().unwrap(),
        ]);
        assert!(expanded.status.success(), "{expanded:?}");
        let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
        for stream in ["stdout", "stderr"] {
            assert_eq!(expanded[stream]["bytes"], 2_097_152);
            assert_eq!(expanded[stream]["displayedBytes"], 64 * 1024);
            assert_eq!(
                expanded[stream]["contentHex"].as_str().unwrap().len(),
                128 * 1024
            );
            assert_eq!(expanded[stream]["truncated"], true);
        }

        // stderr reaches EOF before stdout. Counts must follow the stream
        // that produced them, rather than whichever reader reports first.
        let asymmetric = Fixture::new();
        let (_started, asymmetric_work, asymmetric_ledger, _asymmetric_target) = asymmetric
            .start_worker("printf e >&2; exec 2>&-; sleep 0.05; printf stdout-later; exit 7");
        let checked = asymmetric.call(&["work", "check", &asymmetric_work]);
        assert!(checked.status.success(), "{checked:?}");
        let asymmetric_source =
            fs::read_to_string(asymmetric.root.join(asymmetric_ledger)).unwrap();
        let asymmetric_event: Value =
            serde_json::from_str(asymmetric_source.lines().last().unwrap()).unwrap();
        assert_eq!(asymmetric_event["result"]["code"], 7);
        assert_eq!(asymmetric_event["stdout"]["bytes"], 12);
        assert_eq!(asymmetric_event["stderr"]["bytes"], 1);

        // A successful shell leader is not enough: a background descendant
        // may still hold a pipe open. Capture must terminate the process group
        // at the deadline instead of blocking on an unbounded recv/join.
        let holder = Fixture::new();
        let (_started, _holder_work, holder_ledger, holder_target) =
            holder.start_worker("sleep 60 & exit 0");
        let before_holder = fs::read(holder.root.join(&holder_ledger)).unwrap();
        let output = holder.call(&[
            "run",
            "observe-check",
            &holder_ledger,
            "--target",
            &holder_target,
            "--timeout-ms",
            "100",
        ]);
        assert!(
            !output.status.success(),
            "pipe holder unexpectedly recorded"
        );
        assert_eq!(
            fs::read(holder.root.join(&holder_ledger)).unwrap(),
            before_holder
        );
        assert_no_capture_artifacts(&holder);
        assert_no_ledger_stage_artifacts(&holder);
    }
}

#[test]
fn reported_checks_and_historical_governance_do_not_claim_logs() {
    let fixture = Fixture::new();
    let (_started, work, ledger, target) = fixture.start_worker("true");
    let reported = fixture.call(&[
        "run",
        "record-check",
        &ledger,
        "--target",
        &target,
        "--check-command",
        "true",
        "--exit-code",
        "0",
    ]);
    assert!(reported.status.success(), "{reported:?}");
    let next = fixture.json(&["work", "next", &work]);
    let evidence = next["next"]["packet"]["context"]["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "check")
        .unwrap();
    assert_eq!(evidence["acquisition"], "reported");
    assert_eq!(evidence["logStatus"], "unavailable_reported");
    assert!(evidence.get("checkLogRef").is_none());
    assert_eq!(next["next"]["packet"]["context"]["loop"]["state"], "ready");

    let state = fs::read_to_string(fixture.root.join(&ledger)).unwrap();
    assert!(state.lines().all(|line| !line.contains("\"stdout\"")));
}
