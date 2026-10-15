use super::*;

#[cfg(unix)]
#[test]
fn timeout_cleans_descendants_appends_nothing_and_allows_recovery() {
    #[cfg(target_os = "linux")]
    if std::env::var("EXITBIND_LEGACY_OBSERVE_REAPER").as_deref() != Ok("1") {
        let captures = support::temp("legacy-observe-reaper");
        let mut child = support::git_topology::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "timeout_cleans_descendants_appends_nothing_and_allows_recovery",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("EXITBIND_LEGACY_OBSERVE_REAPER", "1")
            .stdout(fs::File::create(captures.join("stdout")).unwrap())
            .stderr(fs::File::create(captures.join("stderr")).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(25);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("legacy fixture deadline; captures {}", captures.display());
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert!(
            status.success(),
            "legacy fixture failed; captures {}: {}",
            captures.display(),
            fs::read_to_string(captures.join("stderr")).unwrap()
        );
        fs::remove_dir_all(captures).unwrap();
        return;
    }
    #[cfg(target_os = "linux")]
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    let command = "if [ -f allow-check ]; then exit 0; fi; trap '' TERM; sh -c 'trap \"\" TERM; echo $$ > child.pid; exec sleep 30' & wait";
    let (root, ledger, target) = checked_worker("observe-timeout", command);
    let before = fs::read(root.join(&ledger)).unwrap();
    #[cfg(target_os = "linux")]
    let reaper = {
        let pid_file = root.join("child.pid");
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            // Shell redirection creates the file before echo publishes its contents.
            let pid = wait_for_positive_pid(&pid_file, deadline).unwrap();
            while Instant::now() < deadline {
                let mut status = 0;
                let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
                let wait_error = std::io::Error::last_os_error().raw_os_error();
                if waited == pid
                    || (waited == -1 && wait_error == Some(libc::ECHILD) && !process_exists(pid))
                {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            for _ in 0..100 {
                let mut status = 0;
                if unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } == pid {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
            panic!("owned legacy descendant not reaped");
        })
    };

    let started = Instant::now();
    let timed_out = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--timeout-ms",
            "100",
            "--config",
            "exitbind.json",
        ],
    );
    #[cfg(target_os = "linux")]
    reaper.join().unwrap();
    assert!(!timed_out.status.success(), "{}", text(&timed_out));
    assert!(text(&timed_out).contains("timed out after 100 ms"));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);

    let child_pid: i32 = fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(!process_exists(child_pid), "timed-out descendant survived");
    let status = run(
        &root,
        &[
            "run",
            "status",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(status["status"], "running");
    assert_eq!(status["checks"]["status"], "not_observed");
    assert_eq!(status["acceptance"]["status"], "absent");
    let human = call(
        &root,
        &["run", "status", &ledger, "--config", "exitbind.json"],
    );
    assert!(text(&human).contains("local observe-check is available"));

    fs::write(root.join("allow-check"), "ready\n").unwrap();
    let recovered = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--timeout-ms",
            "1000",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(recovered.status.success(), "{}", text(&recovered));
    assert_eq!(json(&recovered)["event"]["result"]["code"], 0);

    for timeout in ["0", "invalid"] {
        let rejected = call(
            &root,
            &[
                "run",
                "observe-check",
                &ledger,
                "--target",
                &target,
                "--timeout-ms",
                timeout,
                "--config",
                "exitbind.json",
            ],
        );
        assert!(!rejected.status.success());
        assert!(text(&rejected).contains("--timeout-ms must be a positive integer"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn concurrent_mutation_is_not_locked_out_and_stale_observation_is_refused() {
    let command = "printf started > observe-started; while [ ! -f observe-continue ]; do sleep 0.01; done; printf observed > observe-effect.txt; exit 7";
    let (root, ledger, target) = checked_worker("observe-race", command);
    let observation = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--timeout-ms",
            "2000",
            "--config",
            "exitbind.json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&root.join("observe-started"));

    let reviewer = call(
        &root,
        &[
            "run",
            "submit",
            "reviewer",
            &ledger,
            "--outcome",
            "approved",
            "--artifact",
            ".exitbind/artifacts/observe-race.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(reviewer.status.success(), "{}", text(&reviewer));
    fs::write(root.join("observe-continue"), "continue\n").unwrap();
    let observed = observation.wait_with_output().unwrap();
    assert!(!observed.status.success());
    assert!(root.join("observe-effect.txt").is_file());
    let observed_text = text(&observed);
    assert!(observed_text.contains("ledger changed while observing"));
    assert!(
        observed_text.contains("this check result was not recorded"),
        "{observed_text}"
    );
    assert!(
        !observed_text.contains("no mutation was made"),
        "{observed_text}"
    );
    let inspected = run(
        &root,
        &[
            "run",
            "inspect",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["action"] != "check"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn launch_failure_and_caller_overrides_do_not_append() {
    let (root, ledger, target) = checked_worker("observe-launch", "printf launched > launched.txt");
    let before = fs::read(root.join(&ledger)).unwrap();
    let bin = root.join("no-sh");
    fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink("/usr/bin/git", bin.join("git")).unwrap();
    let failed = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .env("PATH", &bin)
        .args([
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(text(&failed).contains("could not be launched"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    assert!(!root.join("launched.txt").exists());

    for extra in [
        vec!["--check-command", "exit 0"],
        vec!["--exit-code", "0"],
        vec!["--duration-ms", "0"],
        vec!["--env", "X=1"],
        vec!["--cwd", "."],
    ] {
        let mut args = vec![
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ];
        args.extend(extra);
        let rejected = call(&root, &args);
        assert!(!rejected.status.success(), "{}", text(&rejected));
        assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
        assert!(!root.join("launched.txt").exists());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_target_after_supersede_and_post_execution_drift_do_not_append() {
    let (root, ledger, target) = checked_worker("observe-stale", "printf launched > launched.txt");
    let successor = ".exitbind/runs/observe-successor.jsonl";
    let supersede = call(
        &root,
        &[
            "run",
            "supersede",
            &ledger,
            "--workflow",
            "change",
            "--goal",
            "new",
            "--ledger",
            successor,
            "--check-command",
            "printf launched > launched.txt",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(supersede.status.success(), "{}", text(&supersede));
    let rejected = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!rejected.status.success());
    assert!(!root.join("launched.txt").exists());

    let (root, ledger, target) = checked_worker("observe-drift", "printf changed > exitbind.json");
    let drifted = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(drifted.status.success(), "{}", text(&drifted));
    let observed: Value = serde_json::from_slice(&drifted.stdout).unwrap();
    assert_eq!(observed["event"]["result"]["code"], 0);
    assert_eq!(observed["warnings"][0]["classification"], "input_drift");
    fs::remove_dir_all(root).unwrap();

    let (root, ledger, target) = checked_worker(
        "observe-artifact-drift",
        "printf changed > .exitbind/artifacts/observe-artifact-drift.md",
    );
    let before = fs::read(root.join(&ledger)).unwrap();
    let drifted = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!drifted.status.success());
    assert!(text(&drifted).contains("artifact"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn input_drift_during_observed_check_keeps_recorded_identity_and_result() {
    let (root, ledger, target) = checked_exitbind_worker(
        "observe-input-drift",
        "printf changed > observed-input.txt; sleep 0.05; exit 7",
    );
    let before = call_exitbind(
        &root,
        &[
            "run",
            "inspect",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(before.status.success(), "{}", text(&before));
    let expected_inputs = json(&before)["inputsSha256"].clone();
    assert!(expected_inputs.is_string());
    let observed = call_exitbind(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(observed.status.success(), "{}", text(&observed));
    let observed = json(&observed);
    assert_eq!(observed["event"]["result"]["code"], 7);
    assert_eq!(observed["event"]["inputsSha256"], expected_inputs);
    assert_eq!(observed["warnings"][0]["classification"], "input_drift");
    assert_eq!(
        observed["warnings"][0]["expectedInputsSha256"],
        expected_inputs
    );
    assert_ne!(
        observed["warnings"][0]["currentInputsSha256"],
        expected_inputs
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reviewer_rework_makes_old_target_stale_before_observation_launch() {
    let (root, ledger, old_target) = checked_worker(
        "observe-reviewer-rework",
        "printf launched > reviewer-rework-launched.txt",
    );
    let rework_artifact = ".exitbind/artifacts/observe-reviewer-rework-request.md";
    fs::write(root.join(rework_artifact), "please rework\n").unwrap();
    let reviewer = call(
        &root,
        &[
            "run",
            "submit",
            "reviewer",
            &ledger,
            "--outcome",
            "rework",
            "--artifact",
            rework_artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(reviewer.status.success(), "{}", text(&reviewer));
    let fresh_artifact = ".exitbind/artifacts/observe-reviewer-rework-fresh.md";
    fs::write(root.join(fresh_artifact), "fresh completion\n").unwrap();
    let worker = call(
        &root,
        &[
            "run",
            "submit",
            "worker",
            &ledger,
            "--outcome",
            "completed",
            "--artifact",
            fresh_artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(worker.status.success(), "{}", text(&worker));
    let before = fs::read(root.join(&ledger)).unwrap();
    let rejected = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &old_target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert!(text(&rejected).contains("current worker completion"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    assert!(!root.join("reviewer-rework-launched.txt").exists());
    fs::remove_dir_all(root).unwrap();
}
