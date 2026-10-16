use super::*;
#[cfg(target_os = "linux")]
use std::{
    fs::File,
    io::{self, Read},
};
#[cfg(target_os = "linux")]
#[path = "../../src/run/capture_supervisor/linux/inventory.rs"]
mod fixture_inventory;
#[cfg(target_os = "linux")]
fn fault(_: &str) -> bool {
    false
}

#[cfg(target_os = "linux")]
struct ExclusiveFixture;
#[cfg(target_os = "linux")]
impl Drop for ExclusiveFixture {
    fn drop(&mut self) {
        // Only an exact isolated test owns these children. All ordinary Child
        // owners have returned/dropped before this final bounded teardown.
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if let Ok(candidates) = fixture_inventory::candidates(deadline) {
                for pid in candidates {
                    signal_owned_fixture(pid);
                }
            }
            let waited = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
            if waited == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if std::thread::panicking() {
            eprintln!("exclusive fixture teardown unresolved");
        } else {
            panic!("exclusive fixture teardown unresolved");
        }
    }
}
#[cfg(target_os = "linux")]
fn signal_owned_fixture(pid: i32) {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    if unsafe {
        libc::waitid(
            libc::P_PID,
            pid as u32,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } == 0
        && unsafe { info.si_pid() } == 0
    {
        // No competing waiter or intervening reap; exited children stay reserved.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}
#[cfg(target_os = "linux")]
fn bounded_check(f: &Fixture, args: &[&str]) -> Output {
    let stdout = f.root.join(".exitbind/bounded-check.stdout");
    let stderr = f.root.join(".exitbind/bounded-check.stderr");
    let create = |path| {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
    };
    let mut child = f
        .command(args)
        .stdin(Stdio::null())
        .stdout(create(&stdout))
        .stderr(create(&stderr))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if Instant::now() >= deadline {
            signal_owned_fixture(child.id() as i32);
            let cleanup = Instant::now() + Duration::from_millis(500);
            while Instant::now() < cleanup {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("actual CLI watchdog expired; exclusive fixture retains owned teardown");
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let read = |path| {
        let mut bytes = Vec::new();
        File::open(path)
            .unwrap()
            .take(65537)
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(bytes.len() <= 65536);
        bytes
    };
    Output {
        status,
        stdout: read(&stdout),
        stderr: read(&stderr),
    }
}

#[cfg(target_os = "linux")]
fn isolated(test: &str) -> bool {
    if std::env::var("EXITBIND_OBSERVATION_REAPER_FIXTURE").as_deref() == Ok(test) {
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0
        );
        return false;
    }
    let root = support::temp("observation-reaper-controller");
    let mut child = support::git_topology::command(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env("EXITBIND_OBSERVATION_REAPER_FIXTURE", test)
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
            panic!("isolated deadline; retained {}", root.display());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = fs::read_to_string(root.join("stdout")).unwrap();
    let stderr = fs::read_to_string(root.join("stderr")).unwrap();
    assert!(
        status.success(),
        "{status}; {stdout}; {stderr}; retained {}",
        root.display()
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "isolated exact test did not run: {stdout}"
    );
    fs::remove_dir_all(root).unwrap();
    true
}

#[cfg(target_os = "linux")]
struct OwnedDescendant(i32);
#[cfg(target_os = "linux")]
impl Drop for OwnedDescendant {
    fn drop(&mut self) {
        // This exclusive fixture adopted this exact shell after capsule death.
        // waitid owns the unreaped identity through any cleanup signal.
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                self.0 as u32,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } == 0
        {
            if unsafe { info.si_pid() } == 0 {
                unsafe {
                    libc::kill(self.0, libc::SIGKILL);
                }
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                if unsafe { libc::waitpid(self.0, std::ptr::null_mut(), libc::WNOHANG) } == self.0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("exclusive fixture descendant cleanup unresolved");
        }
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn legacy_unknown_diagnostic_preserves_blocked_bytes_and_refuses_all_apply_credit() {
    const TEST: &str = "check_observation_recovery::process_containment::legacy_unknown_diagnostic_preserves_blocked_bytes_and_refuses_all_apply_credit";
    if isolated(TEST) {
        return;
    }
    // Deliberately kill this new observation's exclusive capsule. This creates
    // a genuine missing-proof failure; it does not alter historical records.
    let f = Fixture::new("printf '%s' $$ > .exitbind/descendant; kill -KILL $PPID; exit 0\n");
    select_fixture_contract(&f);
    let work = f.begin();
    let output = f.call(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    assert!(!output.status.success(), "{output:?}");
    let pid: i32 = fs::read_to_string(f.root.join(".exitbind/descendant"))
        .unwrap()
        .parse()
        .unwrap();
    let descendant = OwnedDescendant(pid);
    let failure = value(&output);
    let facts = &failure["observationFailure"]["facts"];
    assert!(facts["process"].is_null());
    assert_eq!(facts["groupEnded"], false);
    assert_eq!(facts["termination"], "unknown");
    let lead = f.next(&work);
    assert_eq!(lead["outcomes"], json!(["blocked", "rejected"]));
    assert!(!f.call(&["work", "check", &work], b"").status.success());
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
    f.give(&work, &lead, "blocked");
    let relative = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let successor = f.call(
        &[
            "run",
            "supersede",
            &relative,
            "--workflow",
            "change",
            "--goal",
            "unsafe successor",
            "--ledger",
            ".exitbind/runs/unknown-successor.jsonl",
        ],
        b"",
    );
    assert!(!successor.status.success());
    assert!(!f
        .root
        .join(".exitbind/runs/unknown-successor.jsonl")
        .exists());
    let retained = fs::read(f.ledger(&work)).unwrap();
    let diagnostic = f.ok(&["work", "recover-check", &work, "--json"], b"");
    assert_eq!(diagnostic["effect"], "no-change");
    assert_eq!(diagnostic["status"], "historical_unknown_termination");
    assert_eq!(diagnostic["recoverable"], false);
    assert_eq!(diagnostic["checkRecorded"], false);
    assert_eq!(diagnostic["originalOutcome"], "blocked");
    assert_eq!(diagnostic["observation"]["termination"], "unknown");
    for key in ["apply", "ownerDecision", "next", "partialCaptures"] {
        assert!(diagnostic.get(key).is_none());
    }
    assert!(diagnostic["observation"].get("partialCaptures").is_none());
    assert_eq!(
        diagnostic["reference"]["headEventSha256"],
        f.events(&work).last().unwrap()["eventSha256"]
    );
    fs::write(f.root.join("checker.sh"), "exit 42\n").unwrap();
    let changed = f.ok(&["work", "recover-check", &work, "--json"], b"");
    assert_eq!(changed["currentInputs"]["state"], "changed");
    let refused = f.call(
        &[
            "work",
            "recover-check",
            &work,
            "--apply",
            "--current-binding",
            &"a".repeat(64),
            "--json",
        ],
        b"{}",
    );
    assert!(!refused.status.success());
    assert!(value(&refused)["error"]
        .as_str()
        .unwrap()
        .contains("inspection-only"));
    let original = fs::read_to_string(f.root.join("exitbind.json")).unwrap();
    for (name, contents) in [
        ("other.json", format!("{original}\n")),
        ("corrupt.json", "{".into()),
    ] {
        fs::write(f.root.join(name), contents).unwrap();
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&f.root)
            .args(["work", "recover-check", &work, "--json", "--config", name])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "foreign or corrupt configuration: {output:?}"
        );
    }
    let missing = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args([
            "work",
            "recover-check",
            &work,
            "--json",
            "--config",
            "missing.json",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let invalid = format!("smw_{}", "a".repeat(64));
    assert!(!f
        .call(&["work", "recover-check", &invalid, "--json"], b"")
        .status
        .success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), retained);
    // Rehashing an internally impossible ended claim is still rejected.
    let mut events = f.events(&work);
    let event = events
        .iter_mut()
        .find(|e| e["action"] == "check_observation_failed")
        .unwrap();
    event["observation"]["facts"]["termination"] = json!("ended");
    rehash(event);
    fs::write(
        f.root.join(".exitbind/runs/invalid-termination.jsonl"),
        events
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    assert!(!f
        .call(
            &[
                "run",
                "inspect",
                ".exitbind/runs/invalid-termination.jsonl",
                "--json"
            ],
            b""
        )
        .status
        .success());
    drop(descendant);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

#[cfg(target_os = "linux")]
#[test]
fn production_capsule_reaps_escaped_pipe_and_closed_stream_children_after_deadline() {
    const TEST: &str = "check_observation_recovery::process_containment::production_capsule_reaps_escaped_pipe_and_closed_stream_children_after_deadline";
    if isolated(TEST) {
        return;
    }
    let _owner = ExclusiveFixture;
    for closed in [false, true] {
        let f = Fixture::new("exec python3 escaped.py\n");
        fs::write(
            f.root.join("escaped.py"),
            format!(
                r#"import os, signal, time
from pathlib import Path
capsule = os.getppid()
status = Path('/proc/%d/status' % capsule).read_text()
assert next(line for line in status.splitlines() if line.startswith('Threads:')).split()[1] == '1'
assert os.getpgid(capsule) == capsule and os.getpgid(0) != capsule
read, write = os.pipe()
pid = os.fork()
if pid == 0:
    os.close(read)
    os.setsid()
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    Path('.exitbind/escaped.pid').write_text(str(os.getpid()))
    if {closed}:
        os.close(1); os.close(2)
    os.write(write, b'r'); os.close(write)
    time.sleep(30)
    os._exit(0)
os.close(write)
assert os.read(read, 1) == b'r'
os.close(read)
os._exit(0)
"#,
                closed = if closed { "True" } else { "False" }
            ),
        )
        .unwrap();
        select_fixture_contract(&f);
        let work = f.begin();
        let started = Instant::now();
        let output = bounded_check(&f, &["work", "check", &work, "--timeout-ms", "500"]);
        assert!(!output.status.success(), "{output:?}");
        eprintln!("bounded check CLI elapsed: {:?}", started.elapsed());
        let failure = value(&output);
        let facts = &failure["observationFailure"]["facts"];
        assert!(facts["durationMs"].as_u64().unwrap() < 1500, "{facts}");
        assert_eq!(facts["process"]["code"], 0);
        assert_eq!(facts["groupEnded"], true);
        assert_eq!(facts["captureReadersEnded"], true);
        assert_eq!(facts["termination"], "ended");
        assert_eq!(facts["deadlineExceeded"], true);
        let pid: i32 = fs::read_to_string(f.root.join(".exitbind/escaped.pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "owned escaped child survived or was not reaped"
        );
        assert_eq!(
            f.events(&work)
                .iter()
                .filter(|e| e["action"] == "check")
                .count(),
            0
        );
        assert_eq!(f.repair(&work)["role"], "worker");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn production_capsule_preserves_exact_orphan_leader_results_and_raw_streams() {
    for (tail, expected) in [
        ("os._exit(0)", json!({"kind":"exit","code":0})),
        ("os._exit(101)", json!({"kind":"exit","code":101})),
        (
            "os.kill(os.getpid(), signal.SIGTERM)",
            json!({"kind":"signal","signal":libc::SIGTERM}),
        ),
    ] {
        let f = Fixture::new("exec python3 orphan.py\n");
        fs::write(
            f.root.join("orphan.py"),
            format!(
                r#"import os, signal, time
from pathlib import Path
capsule = os.getppid()
status = Path('/proc/%d/status' % capsule).read_text()
assert next(line for line in status.splitlines() if line.startswith('Threads:')).split()[1] == '1'
assert os.getpgid(capsule) == capsule and os.getpgid(0) != capsule
read, write = os.pipe()
pid = os.fork()
if pid == 0:
    os.close(read)
    grandchild = os.fork()
    if grandchild == 0:
        os.setsid()
        os.write(write, b'r'); os.close(write)
        time.sleep(.02)
        os._exit(0)
    os._exit(0)
os.close(write)
assert os.read(read, 1) == b'r'
os.close(read)
os.write(1, b'raw\x00out')
os.write(2, b'raw\xfferr')
{tail}
"#
            ),
        )
        .unwrap();
        let work = f.begin();
        let output = f.call(&["work", "check", &work, "--timeout-ms", "2000"], b"");
        let result = value(&output);
        assert_eq!(result["result"], expected, "{output:?}");
        let event = f
            .events(&work)
            .into_iter()
            .find(|event| event["action"] == "check")
            .unwrap();
        for (stream, expected) in [("stdout", b"raw\0out" as &[u8]), ("stderr", b"raw\xfferr")] {
            assert_eq!(
                fs::read(f.root.join(event[stream]["path"].as_str().unwrap())).unwrap(),
                expected
            );
        }
        assert_eq!(output.status.success(), expected["code"] == 0);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn explicit_prelaunch_failure_has_no_checker_status_or_check_credit() {
    let f = Fixture::new("printf launched > .exitbind/launched\n");
    let work = f.begin();
    let bin_root = support::temp("capsule-no-shell");
    let no_shell = support::git_topology::git_only_path(&bin_root);
    let output = f
        .command(&["work", "check", &work, "--timeout-ms", "1000"])
        .env("PATH", no_shell)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let response = value(&output);
    let facts = &response["observationFailure"]["facts"];
    assert!(facts["process"].is_null());
    assert_eq!(facts["processStarted"], false);
    assert_eq!(facts["termination"], "ended");
    assert_eq!(facts["deadlineExceeded"], false);
    assert!(facts["diagnostic"]
        .as_str()
        .unwrap()
        .contains("could not be launched"));
    assert!(!f.root.join(".exitbind/launched").exists());
    let actions = f
        .events(&work)
        .into_iter()
        .map(|e| e["action"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(actions.contains(&"check_observation".into()));
    assert!(actions.contains(&"check_observation_failed".into()));
    assert!(!actions.contains(&"check".into()));
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(f.repair(&work)["role"], "worker");
    fs::remove_dir_all(bin_root).unwrap();
}
