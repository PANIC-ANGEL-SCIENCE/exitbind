use super::*;

#[cfg(target_os = "linux")]
#[test]
fn live_descendant_with_closed_streams_remains_blocked_and_is_owned_by_fixture() {
    if std::env::var("EXITBIND_OBSERVATION_REAPER_FIXTURE").as_deref() != Ok("1") {
        let root = support::temp("observation-reaper-controller");
        let stdout = fs::File::create(root.join("stdout")).unwrap();
        let stderr = fs::File::create(root.join("stderr")).unwrap();
        let mut command = support::git_topology::command(std::env::current_exe().unwrap());
        let mut child = command
            .args([
                "--exact",
                "live_descendant_with_closed_streams_remains_blocked_and_is_owned_by_fixture",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("EXITBIND_OBSERVATION_REAPER_FIXTURE", "1")
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!(
                    "isolated fixture deadline; captures retained at {}",
                    root.display()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let diagnostic = fs::read_to_string(root.join("stderr")).unwrap();
        assert!(
            status.success(),
            "{status}; {diagnostic}; captures retained at {}",
            root.display()
        );
        assert!(fs::metadata(root.join("stdout")).unwrap().len() < 65536);
        assert!(diagnostic.len() < 65536);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    struct OwnedDescendant(i32);
    impl Drop for OwnedDescendant {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                let waited = unsafe { libc::waitpid(self.0, std::ptr::null_mut(), libc::WNOHANG) };
                if waited == self.0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            eprintln!("owned descendant cleanup unresolved: {}", self.0);
        }
    }
    let f =
        Fixture::new("sleep 10 >/dev/null 2>&1 & printf '%s' $! > .exitbind/descendant; exit 0\n");
    select_fixture_contract(&f);
    let work = f.begin();
    let output = f.call(&["work", "check", &work], b"");
    let pid: i32 = fs::read_to_string(f.root.join(".exitbind/descendant"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(pid > 0);
    let descendant = OwnedDescendant(pid);
    assert!(!output.status.success(), "{output:?}");
    let output = value(&output);
    let facts = &output["observationFailure"]["facts"];
    assert_eq!(facts["process"]["code"], 0);
    assert_eq!(facts["capture"], "complete");
    assert_eq!(facts["groupEnded"], false);
    assert_eq!(facts["termination"], "unknown");
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    let lead = f.next(&work);
    assert_eq!(lead["outcomes"], json!(["blocked", "rejected"]));
    assert!(!f
        .call(
            &[
                "work",
                "disposition",
                &work,
                lead["assignment"].as_str().unwrap(),
                "--decision",
                "repair",
                "--reason",
                "unsafe",
                "--repair-boundary",
                "checker.sh",
                "--regression",
                "retry"
            ],
            b""
        )
        .status
        .success());
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    drop(descendant);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

#[cfg(target_os = "linux")]
#[test]
fn escaped_descendant_holding_capture_streams_keeps_termination_unknown() {
    if std::env::var("EXITBIND_ESCAPED_CAPTURE_FIXTURE").as_deref() != Ok("1") {
        let root = support::temp("escaped-capture-controller");
        let mut child = support::git_topology::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "escaped_descendant_holding_capture_streams_keeps_termination_unknown",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("EXITBIND_ESCAPED_CAPTURE_FIXTURE", "1")
            .stdout(fs::File::create(root.join("stdout")).unwrap())
            .stderr(fs::File::create(root.join("stderr")).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!(
                    "isolated escaped capture deadline; captures retained at {}",
                    root.display()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let diagnostic = fs::read_to_string(root.join("stderr")).unwrap();
        assert!(
            status.success(),
            "{status}; {diagnostic}; captures at {}",
            root.display()
        );
        assert!(fs::metadata(root.join("stdout")).unwrap().len() < 65536);
        assert!(diagnostic.len() < 65536);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    // Only this isolated test process adopts descendants; the Cargo test
    // runner and other fixtures retain their original process topology.
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    struct OwnedDescendant(i32);
    impl Drop for OwnedDescendant {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                if unsafe { libc::waitpid(self.0, std::ptr::null_mut(), libc::WNOHANG) } == self.0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            eprintln!("exact escaped descendant cleanup unresolved: {}", self.0);
        }
    }
    let f = Fixture::new("exec python3 escaped.py\n");
    fs::write(f.root.join("escaped.py"), "import os, time\nfrom pathlib import Path\npid=os.fork()\nif pid == 0:\n    os.setsid()\n    Path('.exitbind/escaped.pid').write_text(str(os.getpid()))\n    time.sleep(10)\n    os._exit(0)\nfor _ in range(200):\n    if Path('.exitbind/escaped.pid').exists():\n        os._exit(0)\n    time.sleep(.005)\nos._exit(1)\n").unwrap();
    select_fixture_contract(&f);
    let work = f.begin();
    // The deadline includes Python startup and fork readiness. Keep it below
    // the escaped child's ten-second lifetime so unfinished readers still
    // exercise unknown termination after the command itself exits successfully.
    let result = f.call(&["work", "check", &work, "--timeout-ms", "1000"], b"");
    let pid: i32 = fs::read_to_string(f.root.join(".exitbind/escaped.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(pid > 0);
    let descendant = OwnedDescendant(pid);
    assert!(!result.status.success(), "{result:?}");
    let failure = value(&result);
    let facts = &failure["observationFailure"]["facts"];
    assert_eq!(facts["process"]["code"], 0);
    assert_eq!(facts["groupEnded"], true);
    assert_eq!(facts["captureReadersEnded"], false);
    assert_eq!(facts["capture"], "incomplete");
    assert_eq!(facts["termination"], "unknown");
    assert_eq!(facts["deadlineExceeded"], true);
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    let lead = f.next(&work);
    assert_eq!(lead["outcomes"], json!(["blocked", "rejected"]));
    let before = fs::read(f.ledger(&work)).unwrap();
    let repair = f.call(
        &[
            "work",
            "disposition",
            &work,
            lead["assignment"].as_str().unwrap(),
            "--decision",
            "repair",
            "--reason",
            "unsafe detached reader",
            "--repair-boundary",
            "checker.sh",
            "--regression",
            "retry",
        ],
        b"",
    );
    assert!(!repair.status.success(), "{repair:?}");
    let relative = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let superseded = f.call(
        &[
            "run",
            "supersede",
            &relative,
            "--workflow",
            "change",
            "--goal",
            "unsafe successor",
            "--ledger",
            ".exitbind/runs/escaped-successor.jsonl",
        ],
        b"",
    );
    assert!(!superseded.status.success(), "{superseded:?}");
    assert!(!f
        .root
        .join(".exitbind/runs/escaped-successor.jsonl")
        .exists());
    assert!(!f
        .call(&["work", "check", &work, "--timeout-ms", "50"], b"")
        .status
        .success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    // Rehashed persisted facts cannot turn unfinished readers into safe
    // termination and thus manufacture repair authority.
    let events = f.events(&work);
    let mut invalid = events.last().unwrap().clone();
    assert_eq!(invalid["action"], "check_observation_failed");
    invalid["observation"]["facts"]["termination"] = json!("ended");
    rehash(&mut invalid);
    let mut prefix = events[..events.len() - 1].to_vec();
    prefix.push(invalid);
    let invalid_ledger = ".exitbind/runs/invalid-reader-termination.jsonl";
    fs::write(
        f.root.join(invalid_ledger),
        prefix
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let invalid = f.call(&["run", "inspect", invalid_ledger, "--json"], b"");
    assert!(!invalid.status.success(), "{invalid:?}");
    drop(descendant);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}
