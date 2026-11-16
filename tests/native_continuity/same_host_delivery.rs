//! Verified same-host Goal projection for host callbacks.
use super::{argv, run, text, Fixture, CHILD, OTHER, PROFILE_BODY, SESSION};
use serde_json::{json, Value};
use std::{fs, process::Output};

#[test]
fn selector_free_same_host_delivery_matches_owner_goal_and_returns_after_permit() {
    let fixture = Fixture::new_with_check_and_write(
        "r60-same-host-delivery",
        "true --mode strict",
        Some("fixture.txt"),
    );
    fs::write(
        fixture.product.join("AGENTS.md"),
        "Preserve the fixture flag.\n",
    )
    .unwrap();
    let prepare = argv(&fixture.next(Some(SESSION))["prepareChild"]);
    let prepared = run(
        &prepare,
        &fixture.product,
        &fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(prepared.status.success(), "{}", text(&prepared));
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let capacity = &prepared["transportPreflight"];
    assert_eq!(capacity["state"], "fits", "{capacity}");
    let owner = fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
    let payload = json!({"hook_event_name":"SubagentStart","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic"});
    let argv = [env!("CARGO_BIN_EXE_exitbind").to_owned(), "hook-run".into()];
    let output = run(
        &argv,
        &fixture.product,
        &fixture.store,
        None,
        payload.to_string().as_bytes(),
    );
    assert!(output.status.success(), "{}", text(&output));
    let hook: Value = serde_json::from_slice(&output.stdout).unwrap();
    let start = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(capacity["outputBytes"].as_u64().unwrap() <= 16 * 1024);
    assert!(capacity["contextUtf16Units"].as_u64().unwrap() <= 10_000);
    assert!(
        output.stdout.len() <= 16 * 1024,
        "host bytes: {}",
        output.stdout.len()
    );
    assert!(start.contains("Preserve the fixture flag."), "{start}");
    let packet = start
        .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("complete assignment unavailable, {} bytes: {start}", output.stdout.len()));
    let packet: Value = serde_json::from_str(packet).unwrap();
    assert_eq!(packet["checkCommand"], "true --mode strict");
    assert_eq!(
        packet["checkPreparation"]["workingDirectory"],
        json!(fixture.product)
    );
    assert_eq!(packet["currentAction"]["binding"], owner["binding"]);
    assert_eq!(
        packet["goal"],
        owner["sections"]["assignment"]["assignment"]["goal"]
    );
    assert_eq!(packet["taskGoal"], owner["sections"]["tasks"]["goal"]);
    assert_ne!(packet["goal"], packet["taskGoal"]);
    if owner["sections"]["tasks"]["tasks"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        assert!(packet["tasks"].is_null());
    } else {
        assert_eq!(packet["tasks"], owner["sections"]["tasks"]["tasks"]);
    }
    assert_eq!(
        packet["taskGoal"],
        "A same-host lens needs no cross-host protocol"
    );
    assert_eq!(
        packet["goal"],
        "Spawn sonic with its lenses from the Lead session"
    );
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "presented"
    );
    eprintln!(
        "same-host fixture hook stdout: {} bytes; complete delivery: true",
        output.stdout.len()
    );
    let form = &packet["currentAction"]["beforeEditing"];
    assert_eq!(form["required"], true);
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("advisory for ordinary host workspace writes"));
    let mut public_form = form.clone();
    public_form["enforcement"] = json!("advisory");
    public_form["meaning"] = json!("advisory for ordinary host workspace writes; required:true applies to managed file effects and a selected controlled-effects launch, which require allowed:true. Only a successful controlled launch and protection preflight enforce that file effect. Repeat the exact request ID only to recover its response; this form grants no host permission");
    assert_eq!(public_form, owner["actionForms"]["beforeEditing"]);
    let mut permit = form["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for word in &mut permit {
        if word == "<OPERATION>" {
            *word = "Update fixture".into();
        }
    }
    let granted = run(
        &permit,
        &fixture.product,
        &fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(granted.status.success(), "{}", text(&granted));
    let granted: Value = serde_json::from_slice(&granted.stdout).unwrap();
    assert_eq!(
        granted["childDeliveryContinuity"]["state"], "current",
        "{granted}"
    );
    fs::write(fixture.product.join("fixture.txt"), "permitted edit\n").unwrap();
    fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"completed","summary":"Edited fixture","reason":""}"#}));
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_string(), "{intent}");
    assert!(intent["rawObservation"].is_null(), "{intent}");
}

#[test]
fn legacy_exact_delivery_remains_eligible_after_its_matching_permit() {
    let fixture = Fixture::new_with_check_and_write(
        "r60-same-host-legacy-delivery",
        "true",
        Some("fixture.txt"),
    );
    fixture.prepare_sonic();
    let start = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    let packet = start
        .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next()).unwrap();
    let packet: Value = serde_json::from_str(packet).unwrap();
    let path = fixture.product.join(".exitbind/child-intents.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    // Model the old producer's record, which predates all three fields.
    assert!(saved["intents"][0]
        .as_object_mut()
        .unwrap()
        .remove("selectedDeliveryProtocol")
        .is_some());
    let delivery = saved["intents"][0]["assignmentDelivery"]
        .as_object_mut()
        .unwrap();
    assert!(delivery.remove("actorFactsSha256").is_some());
    assert!(delivery.remove("actorContractSha256").is_some());
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut permit = packet["currentAction"]["beforeEditing"]["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for word in &mut permit {
        if word == "<OPERATION>" {
            *word = "Update fixture".into();
        }
    }
    let granted = run(
        &permit,
        &fixture.product,
        &fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(granted.status.success(), "{}", text(&granted));
    let granted: Value = serde_json::from_slice(&granted.stdout).unwrap();
    assert_eq!(granted["childDeliveryContinuity"]["state"], "current");
    fs::write(fixture.product.join("fixture.txt"), "permitted edit\n").unwrap();
    fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"completed","summary":"Edited fixture","reason":""}"#}));
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_string(), "{intent}");
    assert!(intent["rawObservation"].is_null(), "{intent}");
}

// Stop the actual hook while its completed presentation is blocked on stdout.
// This puts each mutation after rendering and before note_delivery without a
// timing-based race or a test-only product hook.
#[cfg(target_os = "linux")]
pub(super) struct BlockedStart(Option<std::process::Child>);

// File observations are best effort: unsupported NOWAIT reads remain unknown.
// In particular, a lock's recorded PID is not proof that it is still held.
#[cfg(target_os = "linux")]
fn read_nowait(path: &std::path::Path) -> Result<Vec<u8>, i32> {
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};

    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| error.raw_os_error().unwrap_or(-1))?;
    let mut bytes = vec![0u8; 1025];
    let iov = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: bytes.len(),
    };
    let offset: libc::c_ulong = 0;
    let count = unsafe {
        libc::syscall(
            libc::SYS_preadv2,
            file.as_raw_fd(),
            &iov as *const libc::iovec,
            1_i32,
            offset,
            offset,
            libc::RWF_NOWAIT,
        )
    };
    if count < 0 {
        return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1));
    }
    if count as usize > 1024 {
        return Err(libc::EOVERFLOW);
    }
    bytes.truncate(count as usize);
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn lock_record(bytes: &[u8]) -> Value {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(value) if value["pid"].as_u64().is_some() => {
            json!({"state":"recorded", "pid":value["pid"]})
        }
        _ => json!({"state":"unknown", "reason":"invalid_lock"}),
    }
}

#[cfg(target_os = "linux")]
fn recorded_lock(path: &std::path::Path) -> Value {
    match read_nowait(path) {
        Ok(bytes) => lock_record(&bytes),
        Err(errno) => json!({"state":"unknown", "errno":errno}),
    }
}

#[cfg(target_os = "linux")]
fn child_state(child: &mut std::process::Child) -> Value {
    use std::os::unix::process::ExitStatusExt;

    match child.try_wait() {
        Ok(None) => json!({"state":"running"}),
        Ok(Some(status)) => {
            json!({"state":"exited", "code":status.code(), "signal":status.signal()})
        }
        Err(error) => json!({"state":"unknown", "errno":error.raw_os_error()}),
    }
}

#[cfg(target_os = "linux")]
fn bounded_stderr(bytes: &[u8]) -> Value {
    use std::fmt::Write;

    let retained = &bytes[..bytes.len().min(1024)];
    let mut hex = String::with_capacity(retained.len() * 2);
    for byte in retained {
        let _ = write!(hex, "{byte:02x}");
    }
    // One nonblocking read cannot prove that the rest of the pipe is empty.
    let truncated = (bytes.len() > 1024).then_some(true);
    json!({"state":"captured", "bytes":retained.len(), "truncated":truncated,
        "hex":hex, "text":String::from_utf8_lossy(retained)})
}

#[cfg(target_os = "linux")]
fn nonblocking_stderr(child: &mut std::process::Child) -> Value {
    use std::os::unix::io::AsRawFd;

    let Some(stderr) = child.stderr.as_ref() else {
        return json!({"state":"unknown", "reason":"stderr_unavailable"});
    };
    let fd = stderr.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return json!({"state":"unknown", "errno":std::io::Error::last_os_error().raw_os_error()});
    }
    let mut bytes = [0u8; 1025];
    let count = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
    if count < 0 {
        return json!({"state":"unknown", "errno":std::io::Error::last_os_error().raw_os_error()});
    }
    if count == 0 {
        return json!({"state":"eof"});
    }
    bounded_stderr(&bytes[..count as usize])
}

#[cfg(target_os = "linux")]
fn proc_start(pid: u32) -> Option<String> {
    let bytes = read_nowait(&std::path::PathBuf::from(format!("/proc/{pid}/stat"))).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    text.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
}

#[cfg(target_os = "linux")]
fn timeout_diagnostics(
    child: &mut std::process::Child,
    product: &std::path::Path,
    work: &str,
    elapsed: std::time::Duration,
) -> Value {
    let pid = child.id();
    let before = child_state(child);
    let stderr = nonblocking_stderr(child);
    let intent_lock = recorded_lock(&product.join(".exitbind/child-intents.json.lock"));
    let work_lock = work.strip_prefix("smw_").map_or_else(
        || json!({"state":"unknown", "reason":"invalid_work"}),
        |token| {
            let relative = format!(".exitbind/runs/work-{token}.jsonl");
            let name = format!("run-v1-{}.lock", super::sha(relative.as_bytes()));
            recorded_lock(&product.join(".exitbind/locks").join(name))
        },
    );
    let (first_start, channel, second_start) = if before["state"] == "running" {
        let first = proc_start(pid);
        let channel = read_nowait(&std::path::PathBuf::from(format!("/proc/{pid}/wchan"))).ok();
        (first, channel, proc_start(pid))
    } else {
        (None, None, None)
    };
    let after = child_state(child);
    let wait = match (first_start, channel, second_start) {
        (Some(first), Some(channel), Some(second))
            if before["state"] == "running" && after["state"] == "running" && first == second =>
        {
            json!({"state":"observed", "startTicks":first,
                "wchan":String::from_utf8_lossy(&channel).trim()})
        }
        _ => json!({"state":"unknown"}),
    };
    json!({"childPid":pid, "elapsedMillis":elapsed.as_millis(), "tryWaitBefore":before,
        "tryWaitAfter":after, "stderr":stderr, "linuxWait":wait,
        "recordedIntentLock":intent_lock, "recordedWorkLock":work_lock})
}

#[cfg(target_os = "linux")]
fn assert_blocked_poll(
    result: libc::c_int,
    child: &mut std::process::Child,
    product: &std::path::Path,
    work: &str,
    elapsed: std::time::Duration,
    report: impl FnOnce(&Value),
) {
    if result == 0 {
        report(&timeout_diagnostics(child, product, work, elapsed));
    }
    assert_eq!(result, 1);
}

#[cfg(target_os = "linux")]
impl BlockedStart {
    fn new(fixture: &Fixture) -> Self {
        Self::for_child(fixture, CHILD, "sonic")
    }

    pub(super) fn for_child(fixture: &Fixture, child_id: &str, agent_type: &str) -> Self {
        Self::for_hook(fixture, child_id, agent_type, None)
    }

    pub(super) fn for_slot(
        fixture: &Fixture,
        child_id: &str,
        agent_type: &str,
        slot: usize,
    ) -> Self {
        Self::for_hook(fixture, child_id, agent_type, Some(slot))
    }

    fn for_hook(fixture: &Fixture, child_id: &str, agent_type: &str, slot: Option<usize>) -> Self {
        use std::os::unix::io::AsRawFd;

        let mut command =
            super::super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command.arg("hook-run");
        if let Some(slot) = slot {
            assert!((1..=4).contains(&slot));
            command
                .args(["--host", "claude", "--index"])
                .arg(slot.to_string());
        }
        command
            .current_dir(&fixture.product)
            .env("EXITBIND_SESSION_GOALS_DIR", &fixture.store)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut owned = Self(Some(command.spawn().unwrap()));
        let child = owned.0.as_mut().unwrap();
        let fd = child.stdout.as_ref().unwrap().as_raw_fd();
        // The child is still waiting for its payload while the pipe is sized.
        let capacity = unsafe { libc::fcntl(fd, libc::F_SETPIPE_SZ, 4096) };
        assert_eq!(capacity, 4096, "could not establish bounded output barrier");
        let payload = json!({"hook_event_name":"SubagentStart","cwd":fixture.product,
            "session_id":SESSION,"agent_id":child_id,"agent_type":agent_type});
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let started = std::time::Instant::now();
        let result = unsafe { libc::poll(&mut poll, 1, 15_000) };
        assert_blocked_poll(
            result,
            child,
            &fixture.product,
            &fixture.work,
            started.elapsed(),
            |facts| {
                eprintln!("BlockedStart timeout diagnostics: {facts}");
            },
        );
        let mut queued: libc::c_int = 0;
        assert_eq!(unsafe { libc::ioctl(fd, libc::FIONREAD, &mut queued) }, 0);
        assert_eq!(
            queued, capacity,
            "presentation did not fill the output barrier"
        );
        owned
    }

    pub(super) fn finish(mut self) -> Output {
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}

#[cfg(target_os = "linux")]
impl Drop for BlockedStart {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn blocked_start_timeout_diagnostics_preserve_failure_and_reap_owned_child() {
    use std::{os::unix::io::AsRawFd, process::Stdio, time::Instant};

    let root = super::super::support::temp("blocked-start-timeout-diagnostic");
    let product = root.path().join("product");
    fs::create_dir_all(product.join(".exitbind/locks")).unwrap();
    let work = format!("smw_{}", "a".repeat(64));
    let relative = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let work_lock = product
        .join(".exitbind/locks")
        .join(format!("run-v1-{}.lock", super::sha(relative.as_bytes())));
    let mut child_pid = None;
    let mut facts = None;
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut command = super::super::support::git_topology::command("sh");
        command
            .args([
                "-c",
                "printf '%s' \"$BLOCKED_START_TEST_STDERR\" >&2; IFS= read -r line",
            ])
            .env("BLOCKED_START_TEST_STDERR", "x".repeat(2048))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Keep the guard inside the unwinding scope, as in the constructor.
        let mut owned = BlockedStart(Some(command.spawn().unwrap()));
        let child = owned.0.as_mut().unwrap();
        child_pid = Some(child.id());
        let lock = json!({"pid":child.id()}).to_string();
        assert_eq!(lock_record(lock.as_bytes())["pid"], child.id());
        assert_eq!(
            lock_record(br#"{"pid":"not-a-number"}"#)["state"],
            "unknown"
        );
        fs::write(product.join(".exitbind/child-intents.json.lock"), &lock).unwrap();
        fs::write(&work_lock, &lock).unwrap();
        let mut stdout = libc::pollfd {
            fd: child.stdout.as_ref().unwrap().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let started = Instant::now();
        let result = unsafe { libc::poll(&mut stdout, 1, 0) };
        assert_eq!(result, 0);
        assert_blocked_poll(result, child, &product, &work, started.elapsed(), |value| {
            facts = Some(value.clone());
        });
    }));
    assert!(
        failed.is_err(),
        "the original poll assertion must still fail"
    );
    let facts = facts.expect("the timeout path reported diagnostics before unwinding");
    let pid = child_pid.expect("the controlled child was spawned");
    assert_eq!(facts["childPid"], pid);
    assert_eq!(facts["tryWaitBefore"]["state"], "running");
    assert_eq!(facts["tryWaitAfter"]["state"], "running");
    assert!(facts["elapsedMillis"].is_number());
    if facts["stderr"]["state"] == "captured" {
        assert!(facts["stderr"]["bytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 1024));
    } else {
        assert_eq!(facts["stderr"]["state"], "unknown");
    }
    for lock in [&facts["recordedIntentLock"], &facts["recordedWorkLock"]] {
        if lock["state"] == "recorded" {
            assert_eq!(lock["pid"], pid);
        } else {
            assert_eq!(lock["state"], "unknown");
        }
    }
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn blocked_start_stderr_diagnostics_are_bounded_after_child_exit() {
    use std::process::Stdio;

    let root = super::super::support::temp("blocked-start-exited-diagnostic");
    let mut command = super::super::support::git_topology::command("sh");
    command
        .args(["-c", "printf '%s' \"$BLOCKED_START_TEST_STDERR\" >&2"])
        .env("BLOCKED_START_TEST_STDERR", "x".repeat(2048))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut owned = BlockedStart(Some(command.spawn().unwrap()));
    let child = owned.0.as_mut().unwrap();
    assert!(child.wait().unwrap().success());
    let facts = timeout_diagnostics(child, root.path(), "smw_fixture", std::time::Duration::ZERO);
    assert_eq!(facts["tryWaitBefore"]["state"], "exited");
    assert_eq!(facts["tryWaitAfter"]["state"], "exited");
    assert_eq!(facts["linuxWait"]["state"], "unknown");
    let stderr = &facts["stderr"];
    assert_eq!(stderr["state"], "captured");
    assert_eq!(stderr["bytes"], 1024);
    assert_eq!(stderr["truncated"], true);
    assert_eq!(stderr["text"].as_str().unwrap().len(), 1024);
    assert_eq!(stderr["hex"].as_str().unwrap(), "78".repeat(1024));
}

#[cfg(target_os = "linux")]
#[test]
fn sent_context_with_failed_delivery_record_is_reported_without_return_credit() {
    let profile = PROFILE_BODY.repeat(40);
    let fixture = Fixture::new_with_material(
        "r61-delivery-record-failure",
        "true --mode strict",
        Some("fixture.txt"),
        &profile,
        None,
    );
    fixture.prepare_sonic();
    let blocked = BlockedStart::new(&fixture);
    let path = fixture.product.join(".exitbind/child-intents.json");
    let held = fixture.product.join(".exitbind/child-intents-held.json");
    fs::rename(&path, &held).unwrap();
    fs::create_dir(&path).unwrap();
    let output = blocked.finish();
    assert!(output.status.success(), "{}", text(&output));
    let hook: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("Automatic Work return. ENTIRE final message"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("wrote hook output but could not record")
    );
    fs::remove_dir(&path).unwrap();
    fs::rename(&held, &path).unwrap();
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"completed","summary":"Fixture","reason":""}"#}),
    );
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_null(), "{intent}");
    assert!(intent["rawObservation"]["reason"]
        .as_str()
        .unwrap()
        .contains("delivery proof is unavailable"));
}

#[cfg(target_os = "linux")]
#[test]
fn postrender_changes_refuse_delivery_credit_before_note_delivery() {
    let profile = PROFILE_BODY.repeat(40);
    for change in ["rules", "profile", "config", "goal", "focus", "permit"] {
        let fixture = Fixture::new_with_material(
            &format!("r60-postrender-{change}"),
            "true --mode strict",
            Some("fixture.txt"),
            &profile,
            None,
        );
        fs::write(
            fixture.product.join("AGENTS.md"),
            "Original required rule.\n",
        )
        .unwrap();
        let other = if change == "focus" {
            let other = fixture.value(
                Some(SESSION),
                &[
                    "work",
                    "begin",
                    "change",
                    "--goal",
                    "Independent fixture work",
                    "--check-command",
                    "true",
                    "--standalone",
                ],
            )["work"]
                .as_str()
                .unwrap()
                .to_owned();
            fixture.value(Some(SESSION), &["work", "focus", &fixture.work]);
            Some(other)
        } else {
            None
        };
        fixture.prepare_sonic();
        let detail = fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
        let mut permit = detail["actionForms"]["beforeEditing"]["command"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        for word in &mut permit {
            if word == "<OPERATION>" {
                *word = "Update fixture".into();
            }
        }
        let blocked = BlockedStart::new(&fixture);
        match change {
            "rules" => fs::write(
                fixture.product.join("AGENTS.md"),
                "Changed required rule.\n",
            )
            .unwrap(),
            "profile" => {
                fs::write(fixture.product.join(super::PROFILE), "Changed profile.\n").unwrap()
            }
            "config" => {
                let path = fixture.product.join("exitbind.json");
                let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                config["agents"]["worker"]["write"] = json!(["different.txt"]);
                fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
            }
            "goal" => {
                fixture.value(
                    Some(SESSION),
                    &[
                        "goal",
                        "incorporate",
                        "--goal-id",
                        "f5b-goal",
                        "--goal",
                        "A same-host lens needs no cross-host protocol",
                        "--obligation",
                        "A later independent obligation",
                    ],
                );
            }
            "focus" => {
                fixture.value(Some(SESSION), &["work", "focus", other.as_deref().unwrap()]);
            }
            "permit" => {
                let output = run(
                    &permit,
                    &fixture.product,
                    &fixture.store,
                    Some(SESSION),
                    b"",
                );
                assert!(output.status.success(), "{}", text(&output));
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["allowed"], true);
            }
            _ => unreachable!(),
        }
        let output = blocked.finish();
        assert!(output.status.success(), "{change}: {}", text(&output));
        assert!(
            output.stdout.len() > 4096,
            "{change}: output did not exceed the barrier"
        );
        let hook: Value = serde_json::from_slice(&output.stdout).unwrap();
        let presented = hook["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(
            presented.contains("Original required rule."),
            "{change}: {presented}"
        );
        assert!(
            presented.contains("Automatic Work return. ENTIRE final message"),
            "{change}: {presented}"
        );
        let intent = &fixture.intents().unwrap()[0];
        assert_eq!(
            intent["assignmentDelivery"]["state"], "unavailable",
            "{change}: {intent}"
        );
        fixture.hook(
            json!({"hook_event_name":"SubagentStop","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic",
            "last_assistant_message":r#"{"outcome":"completed","summary":"Fixture","reason":""}"#}),
        );
        let intent = &fixture.intents().unwrap()[0];
        assert!(intent["returnEventSha256"].is_null(), "{change}: {intent}");
        assert!(intent["rawObservation"].is_object(), "{change}: {intent}");
        assert!(intent.get("pendingReturn").is_none(), "{change}: {intent}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn postrender_ancestor_changes_cannot_credit_the_successor() {
    let profile = PROFILE_BODY.repeat(40);
    for change in ["ledger", "claim"] {
        let mut fixture = Fixture::new_with_material(
            &format!("r60-postrender-ancestor-{change}"),
            "true --mode strict",
            Some("fixture.txt"),
            &profile,
            None,
        );
        let ancestor = fixture.work.clone();
        let successor = fixture.value(
            Some(SESSION),
            &[
                "work",
                "supersede",
                &ancestor,
                "--reason=fixture successor",
                "--json",
            ],
        );
        assert_eq!(successor["effect"], "started");
        fixture.work = successor["work"].as_str().unwrap().to_owned();
        let lead = fixture.next(Some(SESSION));
        assert_eq!(lead["role"], "lead");
        let scope = fixture.call(
            Some(SESSION),
            &[
                "work",
                "return",
                &fixture.work,
                lead["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            b"scope",
            true,
        );
        assert!(scope.status.success(), "{}", text(&scope));
        fs::write(
            fixture.product.join("AGENTS.md"),
            "Original required rule.\n",
        )
        .unwrap();
        fixture.prepare_sonic();
        let ancestor_path = fixture
            .product
            .join(format!(".exitbind/runs/work-{}.jsonl", &ancestor[4..]));
        let changed_path = if change == "claim" {
            let mut path = ancestor_path.into_os_string();
            path.push(".supersede");
            std::path::PathBuf::from(path)
        } else {
            ancestor_path
        };
        let current_path = fixture
            .product
            .join(format!(".exitbind/runs/work-{}.jsonl", &fixture.work[4..]));
        let before = fs::read(&current_path).unwrap();
        let blocked = BlockedStart::new(&fixture);
        fs::write(&changed_path, b"invalid fixture evidence\n").unwrap();
        let output = blocked.finish();
        assert!(output.status.success(), "{change}: {}", text(&output));
        let intent = &fixture.intents().unwrap()[0];
        assert_eq!(
            intent["assignmentDelivery"]["state"], "unavailable",
            "{intent}"
        );
        fixture.hook(
            json!({"hook_event_name":"SubagentStop","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic",
            "last_assistant_message":r#"{"outcome":"completed","summary":"Fixture","reason":""}"#}),
        );
        let intent = &fixture.intents().unwrap()[0];
        assert!(intent["returnEventSha256"].is_null(), "{change}: {intent}");
        assert!(intent.get("pendingReturn").is_none(), "{change}: {intent}");
        assert_eq!(fs::read(current_path).unwrap(), before, "{change}");
    }
}

#[test]
fn selected_delivery_refuses_transformed_or_incomplete_required_instructions() {
    let long_phase = "required phase instruction\n".repeat(200);
    let redacted_profile = format!(
        "Never edit {}\n",
        ["/", "home", "/", "account", "/project"].concat()
    );
    let scenarios = [
        (
            "phase-complete",
            PROFILE_BODY,
            Some("phase.md"),
            Some("retain this phase instruction\n"),
            true,
        ),
        (
            "phase-overflow",
            PROFILE_BODY,
            Some("phase.md"),
            Some(long_phase.as_str()),
            false,
        ),
        (
            "phase-unavailable",
            PROFILE_BODY,
            Some("missing-phase.md"),
            None,
            false,
        ),
        (
            "profile-redacted",
            redacted_profile.as_str(),
            None,
            None,
            false,
        ),
    ];
    for (name, profile, path, body, expected) in scenarios {
        let phase = path.map(|path| (path, body));
        let fixture = Fixture::new_with_material(name, "true", None, profile, phase);
        let prepare = argv(&fixture.next(Some(SESSION))["prepareChild"]);
        let prepared = run(
            &prepare,
            &fixture.product,
            &fixture.store,
            Some(SESSION),
            b"",
        );
        if !expected {
            assert!(!prepared.status.success(), "{name}: {}", text(&prepared));
            assert!(text(&prepared).contains("no child was prepared"));
            assert!(fixture.intents().map_or(true, |intents| intents.is_empty()));
            continue;
        }
        assert!(prepared.status.success(), "{name}: {}", text(&prepared));
        let start = fixture
            .hook(
                json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
            )
            .unwrap();
        let intent = &fixture.intents().unwrap()[0];
        assert_eq!(
            intent["assignmentDelivery"]["state"] == "presented",
            expected,
            "{name}: {start}"
        );
        assert!(start.contains("retain this phase instruction"), "{start}");
    }
}

#[test]
fn duplicate_start_cannot_upgrade_an_unavailable_delivery_even_when_renderable() {
    let fixture = Fixture::new("r60-unavailable-duplicate");
    fixture.prepare_sonic();
    let first = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    assert!(
        first.contains("Automatic Work return. ENTIRE final message"),
        "{first}"
    );
    let path = fixture.product.join(".exitbind/child-intents.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        saved["intents"][0]["assignmentDelivery"]["state"],
        "presented"
    );
    // Model an earlier unavailable observation while all current actor facts
    // remain renderable; the second callback cannot create delivery credit.
    saved["intents"][0]["assignmentDelivery"] = json!({"state":"unavailable","binding":null});
    let unavailable = saved["intents"][0]["assignmentDelivery"].clone();
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let payload = json!({"hook_event_name":"SubagentStart","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic"});
    let output = hook_with_selector(&fixture, payload, "CLAUDE_CODE_SESSION_ID", SESSION);
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("cannot receive credit"), "{context}");
    assert!(!context.contains("Automatic Work return. ENTIRE final message"));
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"],
        unavailable
    );
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"completed","summary":"uncredited","reason":""}"#}),
    );
    let retained = &fixture.intents().unwrap()[0];
    assert!(retained["rawObservation"].is_object(), "{retained}");
    assert!(retained["returnEventSha256"].is_null(), "{retained}");
    assert!(retained.get("pendingReturn").is_none(), "{retained}");
}

fn hook_with_selector(fixture: &Fixture, payload: Value, selector: &str, identity: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut command = super::super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&fixture.product)
        .arg("hook-run")
        .env("EXITBIND_SESSION_GOALS_DIR", &fixture.store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env(selector, identity)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn same_host_callback_rejects_conflicting_native_selectors_without_credit() {
    let fixture = Fixture::new("r60-same-host-selector-conflict");
    fixture.prepare_sonic();
    fixture.hook(json!({"hook_event_name":"SubagentStart","session_id":OTHER,
        "agent_id":CHILD,"agent_type":"sonic"}));
    assert_eq!(fixture.intents().unwrap()[0]["state"], "prepared");
    let start = json!({"hook_event_name":"SubagentStart","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic"});
    let wrong_host = hook_with_selector(&fixture, start, "CODEX_THREAD_ID", SESSION);
    assert!(wrong_host.status.success(), "{}", text(&wrong_host));
    assert!(text(&wrong_host).contains("assignment context is unavailable"));
    assert_eq!(fixture.intents().unwrap()[0]["state"], "prepared");
    fixture.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "presented"
    );
    let valid = r#"{"outcome":"completed","summary":"Fixture","reason":""}"#;
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"foreign-child","agent_type":"sonic","last_assistant_message":valid}),
    );
    assert_eq!(fixture.intents().unwrap()[0]["state"], "claimed");
    let stop = json!({"hook_event_name":"SubagentStop","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid});
    let wrong_session = hook_with_selector(&fixture, stop, "CLAUDE_CODE_SESSION_ID", OTHER);
    assert!(wrong_session.status.success(), "{}", text(&wrong_session));
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_null(), "{intent}");
    assert!(
        intent["rawObservation"]["reason"]
            .as_str()
            .unwrap()
            .contains("conflicts"),
        "{intent}"
    );
    assert!(intent.get("pendingReturn").is_none(), "{intent}");
}
