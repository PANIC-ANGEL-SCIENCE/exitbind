//! Product emission and intent credit for Claude's parallel start handlers.

use super::{argv, run, sha, text, Fixture, CHILD, OTHER, SESSION};
use serde_json::{json, Value};
use std::{fs, process::Output};

fn fixture(name: &str) -> Fixture {
    let profile = format!("Required profile.\n{}", "p".repeat(8_000));
    let fixture = Fixture::new_with_material(name, "true", Some("fixture.txt"), &profile, None);
    fs::write(
        fixture.product.join("AGENTS.md"),
        format!("Required rule: {}", "r".repeat(9_000)),
    )
    .unwrap();
    let route = argv(&fixture.next(Some(SESSION))["prepareChild"]);
    let prepared = run(&route, &fixture.product, &fixture.store, Some(SESSION), b"");
    assert!(prepared.status.success(), "{}", text(&prepared));
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert!(
        prepared["transportPreflight"]["parts"].as_u64().unwrap() >= 2,
        "{prepared}"
    );
    fixture
}

fn payload(fixture: &Fixture, session: &str, child: &str) -> Value {
    json!({"hook_event_name":"SubagentStart", "cwd":fixture.product,
        "session_id":session, "agent_id":child, "agent_type":"sonic"})
}

fn invoke(fixture: &Fixture, index: usize, payload: &Value) -> Output {
    let argv = [
        env!("CARGO_BIN_EXE_exitbind").to_owned(),
        "hook-run".into(),
        "--host".into(),
        "claude".into(),
        "--index".into(),
        index.to_string(),
    ];
    run(
        &argv,
        &fixture.product,
        &fixture.store,
        None,
        payload.to_string().as_bytes(),
    )
}

fn part(output: &Output) -> Option<String> {
    assert!(output.status.success(), "{}", text(output));
    if output.stdout.is_empty() {
        return None;
    }
    assert!(output.stdout.len() <= 16 * 1024);
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    let context = response["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.encode_utf16().count() <= 10_000);
    Some(context.to_owned())
}

fn reconstructed(parts: &[Option<String>]) -> String {
    let actual = parts.iter().flatten().collect::<Vec<_>>();
    assert!(actual.len() >= 2);
    let mut complete = String::new();
    let mut digest = None;
    for (index, text) in actual.iter().enumerate() {
        let (header, payload) = text.split_once("\n\n").unwrap();
        let observed = header
            .split("SHA-256 ")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        if let Some(digest) = digest {
            assert_eq!(digest, observed);
        } else {
            digest = Some(observed);
        }
        assert!(header.contains(&format!("part {} of {}", index + 1, actual.len())));
        complete.push_str(&payload.replace("&lt;", "<").replace("&amp;", "&"));
    }
    assert_eq!(sha(complete.as_bytes()), digest.unwrap());
    assert!(complete.contains("Required profile."));
    assert!(complete.contains("Required rule:"));
    assert!(complete.contains("Automatic Work return. ENTIRE final message"));
    complete
}

fn assert_presented(fixture: &Fixture) {
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(
        intent["assignmentDelivery"]["state"], "presented",
        "{intent}"
    );
    assert_eq!(intent["selectedDeliveryProtocol"], 1);
    for field in [
        "requiredFactsSha256",
        "recipientSha256",
        "instructionsSha256",
        "actorFactsSha256",
        "actorContractSha256",
    ] {
        assert_eq!(
            intent["assignmentDelivery"][field].as_str().unwrap().len(),
            64,
            "{field}"
        );
    }
    assert_eq!(intent["multipartDelivery"]["state"], "complete");
    assert!(intent["multipartDelivery"].get("parts").is_none());
}

#[test]
fn reversed_slots_reconstruct_and_only_the_complete_set_promotes() {
    let fixture = fixture("r62-multipart-reversed");
    let input = payload(&fixture, SESSION, CHILD);
    let mut parts = vec![None; 4];
    let mut emitted = vec![None; 4];
    for index in (1..=4).rev() {
        let output = invoke(&fixture, index, &input);
        if !output.stdout.is_empty() {
            emitted[index - 1] = Some(sha(&output.stdout));
        }
        parts[index - 1] = part(&output);
        let intent = &fixture.intents().unwrap()[0];
        if index > 1 {
            assert_ne!(
                intent["assignmentDelivery"]["state"], "presented",
                "{intent}"
            );
        }
    }
    reconstructed(&parts);
    assert_presented(&fixture);
    let intent = &fixture.intents().unwrap()[0];
    let receipts = intent["multipartDelivery"]["receipts"].as_array().unwrap();
    for (index, receipt) in receipts.iter().enumerate() {
        assert_eq!(receipt.as_str(), emitted[index].as_deref());
    }
}

#[test]
fn concurrent_slots_use_one_bundle() {
    let fixture = fixture("r62-multipart-concurrent");
    let input = payload(&fixture, SESSION, CHILD);
    let parts = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for index in 1..=4 {
            let fixture = &fixture;
            let input = &input;
            handles.push(scope.spawn(move || {
                let started = std::time::Instant::now();
                let output = invoke(fixture, index, input);
                eprintln!(
                    "Claude multipart slot {index}: {:.3}s",
                    started.elapsed().as_secs_f64()
                );
                part(&output)
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    reconstructed(&parts);
    assert_presented(&fixture);
}

#[test]
fn duplicate_missing_and_foreign_callbacks_cannot_supply_a_piece() {
    let fixture = fixture("r62-multipart-missing");
    let input = payload(&fixture, SESSION, CHILD);
    let first = part(&invoke(&fixture, 1, &input));
    assert_eq!(first, part(&invoke(&fixture, 1, &input)));
    let before = fixture.intents().unwrap()[0]["multipartDelivery"].clone();
    assert!(part(&invoke(&fixture, 2, &payload(&fixture, OTHER, CHILD))).is_none());
    assert!(part(&invoke(
        &fixture,
        2,
        &payload(&fixture, SESSION, "other-child")
    ))
    .is_none());
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(intent["multipartDelivery"], before);
    assert_ne!(intent["assignmentDelivery"]["state"], "presented");
    for index in 2..=4 {
        part(&invoke(&fixture, index, &input));
    }
    assert_presented(&fixture);
}

#[test]
fn changed_recipient_instructions_cannot_complete_an_existing_bundle() {
    let fixture = fixture("r62-multipart-stale");
    let input = payload(&fixture, SESSION, CHILD);
    assert!(part(&invoke(&fixture, 1, &input)).is_some());
    fs::write(
        fixture.product.join("AGENTS.md"),
        "a different required rule",
    )
    .unwrap();
    for index in 2..=4 {
        let _ = invoke(&fixture, index, &input);
    }
    let intent = &fixture.intents().unwrap()[0];
    assert_ne!(
        intent["assignmentDelivery"]["state"], "presented",
        "{intent}"
    );
    assert_eq!(intent["multipartDelivery"]["state"], "invalid", "{intent}");
}

#[test]
fn a_conflicting_saved_receipt_cannot_complete_the_bundle() {
    let fixture = fixture("r62-multipart-conflict");
    let input = payload(&fixture, SESSION, CHILD);
    assert!(part(&invoke(&fixture, 1, &input)).is_some());
    let path = fixture.product.join(".exitbind/child-intents.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["intents"][0]["multipartDelivery"]["receipts"][0] = json!("conflicting");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    for index in 2..=4 {
        let _ = invoke(&fixture, index, &input);
    }
    let intent = &fixture.intents().unwrap()[0];
    assert_ne!(
        intent["assignmentDelivery"]["state"], "presented",
        "{intent}"
    );
    assert_eq!(intent["multipartDelivery"]["state"], "invalid", "{intent}");
}

#[test]
fn legacy_one_part_hook_keeps_its_selected_delivery_proof() {
    let fixture = Fixture::new("r62-multipart-legacy");
    fixture.prepare_sonic();
    let output = fixture.hook(payload(&fixture, SESSION, CHILD)).unwrap();
    assert!(output.contains("Automatic Work return. ENTIRE final message"));
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(
        intent["assignmentDelivery"]["state"], "presented",
        "{intent}"
    );
    assert_eq!(intent["selectedDeliveryProtocol"], 1);
    assert!(intent.get("multipartDelivery").is_none());
}

#[test]
fn unprepared_profile_only_context_is_emitted_by_one_slot() {
    let fixture = Fixture::new("r62-multipart-unprepared");
    let input = payload(&fixture, SESSION, CHILD);
    let first = part(&invoke(&fixture, 1, &input)).unwrap();
    assert!(first.contains("Profile bytes:"));
    for index in 2..=4 {
        assert!(part(&invoke(&fixture, index, &input)).is_none());
    }
    assert!(fixture.intents().map_or(true, |intents| intents.is_empty()));
}

#[cfg(unix)]
#[test]
fn broken_stdout_never_records_the_missing_piece() {
    use std::io::Write;
    use std::process::Stdio;
    let fixture = fixture("r62-multipart-broken-output");
    let input = payload(&fixture, SESSION, CHILD);
    let mut command = super::super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .args(["hook-run", "--host", "claude", "--index", "1"])
        .current_dir(&fixture.product)
        .env("EXITBIND_SESSION_GOALS_DIR", &fixture.store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().unwrap();
    drop(child.stdout.take());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    assert!(!child.wait().unwrap().success());
    for index in 2..=4 {
        part(&invoke(&fixture, index, &input));
    }
    let intent = &fixture.intents().unwrap()[0];
    assert_ne!(
        intent["assignmentDelivery"]["state"], "presented",
        "{intent}"
    );
    assert!(
        intent["multipartDelivery"]["receipts"][0].is_null(),
        "{intent}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn killed_writer_before_receipt_cannot_promote() {
    use std::{
        io::Write,
        os::unix::io::AsRawFd,
        process::{Child, Stdio},
    };
    struct Process(Option<Child>);
    impl Drop for Process {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let fixture = fixture("r62-multipart-killed-writer");
    let input = payload(&fixture, SESSION, CHILD);
    let mut command = super::super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .args(["hook-run", "--host", "claude", "--index", "1"])
        .current_dir(&fixture.product)
        .env("EXITBIND_SESSION_GOALS_DIR", &fixture.store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut process = Process(Some(command.spawn().unwrap()));
    let child = process.0.as_mut().unwrap();
    let fd = child.stdout.as_ref().unwrap().as_raw_fd();
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETPIPE_SZ, 4096) }, 4096);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(unsafe { libc::poll(&mut poll, 1, 15_000) }, 1);
    let mut queued: libc::c_int = 0;
    assert_eq!(unsafe { libc::ioctl(fd, libc::FIONREAD, &mut queued) }, 0);
    assert_eq!(queued, 4096);
    child.kill().unwrap();
    child.wait().unwrap();
    process.0 = None;
    for index in 2..=4 {
        part(&invoke(&fixture, index, &input));
    }
    let intent = &fixture.intents().unwrap()[0];
    assert_ne!(
        intent["assignmentDelivery"]["state"], "presented",
        "{intent}"
    );
    assert!(
        intent["multipartDelivery"]["receipts"][0].is_null(),
        "{intent}"
    );
}
