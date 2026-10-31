//! Assignment-scoped admission integrity and compatibility.
use super::known_outcomes::Active;
use super::*;

#[test]
fn unrelated_corrupt_admission_does_not_block_a_separate_work() {
    let f = Active::new(
        "pre-return-separate-work",
        "printf x >> .exitbind/foreign-count; exit 8",
        "5000",
        false,
    );
    let begun = successful(command(
        &f.root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "separate work",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--pre-return-command",
            "printf x >> .exitbind/separate-count",
        ],
        None,
    ));
    let work = begun["work"].as_str().unwrap();
    assert_ne!(f.work, work);
    let lead = begun["next"]["assignment"].as_str().unwrap();
    let scoped = command(
        &f.root,
        &["work", "return", work, lead, "--outcome", "scoped"],
        Some(b"scope"),
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let current = successful(command(&f.root, &["work", "detail", work, "--json"], None));
    let worker = current["recipient"]["assignment"].as_str().unwrap();

    assert!(!f.finish(b"failed foreign result").status.success());
    let directory = f.root.join(".exitbind/pre-return");
    let records = fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".attempt.json"))
        .collect::<Vec<_>>();
    let [corrupt] = records.as_slice() else {
        panic!("expected one foreign admission");
    };
    let admission: Value = serde_json::from_slice(&fs::read(corrupt).unwrap()).unwrap();
    assert_eq!(admission["binding"]["work"], f.work);
    fs::write(corrupt, b"{truncated admission").unwrap();
    let returned = command(
        &f.root,
        &["work", "return", work, worker, "--outcome", "completed"],
        Some(b"separate work result"),
    );
    assert!(returned.status.success(), "{returned:?}");
    assert_eq!(
        fs::read(f.root.join(".exitbind/separate-count")).unwrap(),
        b"x"
    );
    assert_eq!(
        fs::read(f.root.join(".exitbind/foreign-count")).unwrap(),
        b"x"
    );
}

#[test]
fn owned_missing_misfiled_and_forged_admissions_refuse_without_reexecution() {
    let f = Fixture::new("pre-return-owned-corruption", false);
    let before = f.ledger();
    let original = f.admission_path();
    let moved = original.with_extension("misfiled");

    for case in ["missing", "misfiled", "truncated", "forged"] {
        match case {
            "missing" => fs::remove_file(&original).unwrap(),
            "misfiled" => fs::rename(&original, &moved).unwrap(),
            "truncated" => fs::write(&original, b"{truncated").unwrap(),
            "forged" => {
                let mut record: Value = serde_json::from_slice(&f.admission).unwrap();
                record["binding"]["agent"] = json!("another-worker");
                record.as_object_mut().unwrap().remove("recordSha256");
                record["recordSha256"] = json!(digest(&serde_json::to_vec(&record).unwrap()));
                fs::write(&original, serde_json::to_vec(&record).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        let output = f.call(
            &[
                "work",
                "return",
                &f.work,
                &f.assignment,
                "--outcome",
                "completed",
                "--result-ref",
                &f.held,
            ],
            None,
        );
        assert!(!output.status.success(), "{case}: {output:?}");
        assert_eq!(f.ledger(), before, "{case}");
        assert_eq!(f.count(), b"x", "{case}");
        assert_eq!(fs::read(f.held_path()).unwrap(), f.body, "{case}");
        if case == "misfiled" {
            fs::rename(&moved, &original).unwrap();
        } else {
            fs::write(&original, &f.admission).unwrap();
            fs::set_permissions(&original, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

#[test]
fn forged_ledger_ownership_refuses_even_when_the_event_is_rehashed() {
    let f = Fixture::new("pre-return-forged-ledger", false);
    let before = f.ledger();
    let path = f.root.join(f.ledger_path());
    for field in ["ledger", "agent", "repair"] {
        let mut events = std::str::from_utf8(&before)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let event = events.last_mut().unwrap();
        assert_eq!(event["action"], "pre_return_admitted");
        if field == "repair" {
            event["admission"]["ownRepairEventSha256"] = event["previousEventSha256"].clone();
        } else {
            event["admission"]["assignment"][field] = json!("forged-owner");
        }
        event.as_object_mut().unwrap().remove("eventSha256");
        event["eventSha256"] = json!(digest(&serde_json::to_vec(event).unwrap()));
        let mut source = String::new();
        for event in &events {
            source.push_str(&serde_json::to_string(event).unwrap());
            source.push('\n');
        }
        fs::write(&path, source).unwrap();
        let output = f.call(&["run", "inspect", &f.ledger_path(), "--json"], None);
        assert!(!output.status.success(), "{field}: {output:?}");
        assert_eq!(f.count(), b"x", "{field}");
        fs::write(&path, &before).unwrap();
    }
}

#[test]
fn unresolved_attempt_refuses_a_different_producer() {
    let f = Fixture::new("pre-return-cross-producer", false);
    let before = f.ledger();
    let alternate = f.root.join(".exitbind/alternate-exitbind");
    fs::copy(env!("CARGO_BIN_EXE_exitbind"), &alternate).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&alternate)
        .unwrap()
        .write_all(b"different producer identity")
        .unwrap();
    fs::set_permissions(&alternate, fs::Permissions::from_mode(0o700)).unwrap();
    let output = support::git_topology::command(&alternate)
        .current_dir(&f.root)
        .args([
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
            "--config",
        ])
        .arg(f.root.join("exitbind.json"))
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("uncertain outcome"),
        "{output:?}"
    );
    assert_eq!(f.ledger(), before);
    assert_eq!(f.count(), b"x");
}

#[test]
fn own_admission_preserves_an_exact_current_binding() {
    let f = Active::new(
        "pre-return-current-binding",
        "printf x >> .exitbind/bound-count",
        "5000",
        false,
    );
    let next = successful(command(&f.root, &["work", "next", &f.work, "--json"], None));
    let binding = next["current"]["binding"].as_str().unwrap();
    let output = command(
        &f.root,
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--current-binding",
            binding,
        ],
        Some(b"bound result"),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(f.root.join(".exitbind/bound-count")).unwrap(),
        b"x"
    );
}

#[test]
fn a_foreign_event_during_the_check_stales_the_original_binding() {
    struct FinishGuard<'a> {
        child: Option<Child>,
        root: &'a Path,
    }
    impl Drop for FinishGuard<'_> {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = fs::write(self.root.join(".exitbind/release-check"), b"cleanup");
                let _ = child.wait();
            }
        }
    }

    let f = Active::new(
        "pre-return-foreign-event",
        "printf x >> .exitbind/bound-count; : > .exitbind/check-started; while test ! -f .exitbind/release-check; do sleep 0.05; done",
        "5000",
        false,
    );
    let next = successful(command(&f.root, &["work", "next", &f.work, "--json"], None));
    let binding = next["current"]["binding"].as_str().unwrap();
    let child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args([
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--current-binding",
            binding,
            "--config",
        ])
        .arg(f.root.join("exitbind.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut caller = FinishGuard {
        child: Some(child),
        root: &f.root,
    };
    caller
        .child
        .as_mut()
        .unwrap()
        .stdin
        .take()
        .unwrap()
        .write_all(b"bound result")
        .unwrap();
    wait_until(|| f.root.join(".exitbind/check-started").is_file());
    fs::write(
        f.root.join(".exitbind/artifacts/foreign-evidence"),
        b"independent evidence",
    )
    .unwrap();
    let evidence = command(
        &f.root,
        &[
            "work",
            "evidence",
            &f.work,
            &f.assignment,
            "--artifact-root",
            "state",
            "--artifact",
            ".exitbind/artifacts/foreign-evidence",
        ],
        None,
    );
    assert!(evidence.status.success(), "{evidence:?}");
    fs::write(f.root.join(".exitbind/release-check"), b"finish").unwrap();
    let returned = caller.child.take().unwrap().wait_with_output().unwrap();
    assert!(!returned.status.success(), "{returned:?}");
    assert!(
        String::from_utf8_lossy(&returned.stderr).contains("binding is stale"),
        "{returned:?}"
    );
    assert_eq!(
        fs::read(f.root.join(".exitbind/bound-count")).unwrap(),
        b"x"
    );
    let ledger = f
        .root
        .join(format!(".exitbind/runs/work-{}.jsonl", &f.work[4..]));
    let events = fs::read_to_string(ledger).unwrap();
    assert!(!events
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .any(|event| { event["action"] == "submit" && event["role"] == "worker" }));
}

#[test]
fn older_reader_refuses_the_new_start_marker() {
    let Ok(old) = std::env::var("EXITBIND_PRE_RETURN_OLD_READER") else {
        eprintln!("COVERAGE UNAVAILABLE: no earlier reader supplied");
        return;
    };
    let f = Active::new("pre-return-old-reader", "true", "5000", false);
    let inspect = || {
        support::git_topology::command(&old)
            .current_dir(&f.root)
            .args(["work", "detail", &f.work, "--json", "--config"])
            .arg(f.root.join("exitbind.json"))
            .output()
            .unwrap()
    };
    let before = inspect();
    assert!(!before.status.success(), "{before:?}");
    let refusal: Value = serde_json::from_slice(&before.stdout).unwrap();
    assert!(
        refusal["error"]
            .as_str()
            .is_some_and(|error| error.contains("unknown field 'preReturnAdmissionProtocol'")),
        "{before:?}"
    );
    assert!(f.finish(b"current reader result").status.success());
    let after = inspect();
    assert!(!after.status.success(), "{after:?}");
    let refusal: Value = serde_json::from_slice(&after.stdout).unwrap();
    assert!(
        refusal["error"]
            .as_str()
            .is_some_and(|error| error.contains("unknown field 'preReturnAdmissionProtocol'")),
        "{after:?}"
    );
}

#[test]
fn unreadable_legacy_admission_with_unknown_owner_still_refuses() {
    let Ok(old) = std::env::var("EXITBIND_PRE_RETURN_OLD_READER") else {
        eprintln!("COVERAGE UNAVAILABLE: no earlier reader supplied");
        return;
    };
    let root = support::temp("pre-return-legacy-unowned");
    let init = support::git_topology::command(&old)
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let old_call = |args: &[&str], input: Option<&[u8]>| {
        let mut child = support::git_topology::command(&old)
            .current_dir(&root)
            .args(args)
            .arg("--config")
            .arg(root.join("exitbind.json"))
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(bytes) = input {
            if let Err(error) = child.stdin.take().unwrap().write_all(bytes) {
                assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
            }
        }
        child.wait_with_output().unwrap()
    };
    let start = |label: &str, checker: &str| {
        let begun = successful(old_call(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                label,
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--pre-return-command",
                checker,
            ],
            None,
        ));
        let work = begun["work"].as_str().unwrap().to_owned();
        let lead = begun["next"]["assignment"].as_str().unwrap();
        assert!(old_call(
            &["work", "return", &work, lead, "--outcome", "scoped"],
            Some(b"scope")
        )
        .status
        .success());
        let detail = successful(old_call(&["work", "detail", &work, "--json"], None));
        (
            work,
            detail["recipient"]["assignment"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    };
    let (a, aw) = start("legacy source", "exit 1");
    let (b, bw) = start("legacy current", "printf x >> .exitbind/legacy-count");
    let ledger = root.join(format!(".exitbind/runs/work-{}.jsonl", &b[4..]));
    let before = fs::read(&ledger).unwrap();
    let start_event: Value = serde_json::from_str(
        std::str::from_utf8(&before)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert!(start_event.get("preReturnAdmissionProtocol").is_none());
    assert!(!old_call(
        &["work", "return", &a, &aw, "--outcome", "completed"],
        Some(b"first")
    )
    .status
    .success());
    let admission = fs::read_dir(root.join(".exitbind/pre-return"))
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .find(|path| path.to_string_lossy().ends_with(".attempt.json"))
        .unwrap();
    fs::write(admission, b"{truncated legacy admission").unwrap();
    let refused = command(
        &root,
        &["work", "return", &b, &bw, "--outcome", "completed"],
        Some(b"second"),
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("pre-return record is invalid"),
        "{refused:?}"
    );
    assert_eq!(fs::read(ledger).unwrap(), before);
    assert!(!root.join(".exitbind/legacy-count").exists());
}
