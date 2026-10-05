//! Consumers execute product-issued forms, without reconstructing ledger paths.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        support::git_topology::repository(&root);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Self { root }
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn json(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn detail(&self, work: &str) -> Value {
        self.json(&["work", "detail", work, "--json"], b"")
    }

    fn execute(&self, command: &Value, input: &[u8]) -> Output {
        assert_eq!(command["sameExecutableRequired"], false);
        assert_eq!(command["sameConfigRequired"], false);
        let argv = command["argv"].as_array().unwrap();
        let mut child = support::git_topology::command(argv[0].as_str().unwrap())
            .current_dir(&self.root)
            .args(argv[1..].iter().map(|a| a.as_str().unwrap()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn outcome(&self, work: &str, outcome: &str) -> Value {
        let detail = self.detail(work);
        let choice = detail["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["label"] == outcome)
            .unwrap();
        let mut command = choice["command"].clone();
        for arg in command["argv"].as_array_mut().unwrap() {
            if arg == "<REASON>" {
                *arg = Value::String("the exact evidence satisfies this role".into());
            }
        }
        let output = self.execute(
            &command,
            format!("complete {outcome} role result").as_bytes(),
        );
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn begin(&self) -> String {
        self.json(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "close the exact bounded result",
                "--check-command",
                "true",
                "--review-policy",
                "required",
                "--proof-origin",
                "synthetic",
            ],
            b"",
        )["work"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn accepted(&self) -> (String, Value) {
        let work = self.begin();
        self.outcome(&work, "scoped");
        let worker = self.detail(&work);
        let mut permit = worker["actionForms"]["beforeEditing"]["command"].clone();
        for arg in permit["argv"].as_array_mut().unwrap() {
            if arg == "<OPERATION>" {
                *arg = Value::String("exercise closeout consumer".into());
            }
        }
        let result = self.execute(&permit, b"");
        assert!(result.status.success(), "{result:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&result.stdout).unwrap()["allowed"],
            true
        );
        self.outcome(&work, "completed");
        let check = self.detail(&work);
        let result = self.execute(&check["actionForms"]["mechanicalAction"]["command"], b"");
        assert!(result.status.success(), "{result:?}");
        self.outcome(&work, "approved");
        let result = self.outcome(&work, "accepted");
        (work, result)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn terminal_export_form_is_separate_optional_verified_and_replayable() {
    let fixture = Fixture::new("closeout-emitted-argv");
    let (work, accepted) = fixture.accepted();
    assert_eq!(accepted["effectiveAction"]["exists"], false);
    assert_eq!(
        accepted["current"]["completion"]["workAcceptance"]["state"],
        "accepted"
    );
    let detail = fixture.detail(&work);
    assert_eq!(detail["effectiveAction"]["exists"], false);
    assert_eq!(detail["completion"]["receipt"]["state"], "not_requested");
    let next = fixture.json(&["work", "next", &work], b"");
    assert_eq!(next["current"]["completion"], detail["completion"]);
    let export = &detail["completion"]["receipt"]["export"]["command"];
    let read = fixture.json(&["work", "closeout", &work], b"");
    assert_eq!(read["receipt"]["state"], "not_requested");
    assert_eq!(
        fs::read_dir(fixture.root.join(".exitbind/receipts"))
            .unwrap()
            .count(),
        0
    );
    let output = fixture.execute(export, b"");
    assert!(output.status.success(), "{output:?}");
    let created: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(created["receipt"]["state"], "created_verified");
    assert_eq!(created["effectiveAction"]["exists"], false);
    let path = created["receipt"]["path"].as_str().unwrap();
    let before = fs::read(fixture.root.join(path)).unwrap();
    let ledger = accepted["reference"]["ledger"].as_str().unwrap();
    let ledger_before = fs::read(fixture.root.join(ledger)).unwrap();
    let output = fixture.execute(export, b"");
    assert!(output.status.success(), "{output:?}");
    let repeated: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(repeated["receipt"]["state"], "existing_verified");
    assert_eq!(repeated["receipt"]["sha256"], created["receipt"]["sha256"]);
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), before);
    let verified = fixture.json(&["work", "closeout", &work, "--receipt", path], b"");
    assert_eq!(verified["receipt"]["state"], "existing_verified");
    assert_eq!(verified["effectiveAction"]["state"], "done");
    assert_eq!(fs::read(fixture.root.join(ledger)).unwrap(), ledger_before);
}

#[test]
fn unaccepted_custom_corrupt_wrong_work_and_stale_evidence_never_export_as_verified() {
    let fixture = Fixture::new("closeout-negative");
    let pending = fixture.begin();
    assert_eq!(
        fixture.json(&["work", "closeout", &pending], b"")["receipt"]["state"],
        "unavailable"
    );
    assert!(!fixture
        .call(&["work", "closeout", &pending, "--export"], b"")
        .status
        .success());
    let (work, _) = fixture.accepted();
    fs::write(fixture.root.join("custom.json"), b"owner custom file").unwrap();
    assert!(!fixture
        .call(&["work", "closeout", &work, "--output", "custom.json"], b"")
        .status
        .success());
    assert_eq!(
        fs::read(fixture.root.join("custom.json")).unwrap(),
        b"owner custom file"
    );
    // Git-covered custom bytes must exist before the second accepted check.
    let (work, _) = fixture.accepted();
    assert!(!fixture
        .call(&["work", "closeout", &work, "--output", "custom.json"], b"")
        .status
        .success());
    let created = fixture.json(&["work", "closeout", &work, "--export"], b"");
    let path = created["receipt"]["path"].as_str().unwrap();
    let bytes = fs::read(fixture.root.join(path)).unwrap();
    let (other, _) = fixture.accepted();
    assert!(!fixture
        .call(&["work", "closeout", &other, "--receipt", path], b"")
        .status
        .success());
    assert!(!fixture
        .call(&["work", "closeout", &other, "--output", path], b"")
        .status
        .success());
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), bytes);
    fs::write(fixture.root.join(path), b"{ corrupt").unwrap();
    assert!(!fixture
        .call(&["work", "closeout", &work, "--receipt", path], b"")
        .status
        .success());
    assert!(!fixture
        .call(&["work", "closeout", &work, "--export"], b"")
        .status
        .success());
    fs::write(fixture.root.join(path), bytes).unwrap();
    fs::write(
        fixture.root.join("changed-product.txt"),
        b"changed after acceptance",
    )
    .unwrap();
    let stale = fixture.json(&["work", "closeout", &work], b"");
    assert_eq!(stale["workAcceptance"]["state"], "accepted");
    assert_eq!(stale["receipt"]["state"], "unavailable");
    assert!(!fixture
        .call(&["work", "closeout", &work, "--receipt", path], b"")
        .status
        .success());
    assert!(!fixture
        .call(&["work", "closeout", &work, "--output", "new.json"], b"")
        .status
        .success());
    assert!(!fixture.root.join("new.json").exists());
    // The pre-existing ledger-oriented owner keeps historical receipt meaning;
    // the additive current Work facade deliberately refuses stale applicability.
    assert!(fixture.call(&["verify", path], b"").status.success());
}

#[test]
fn export_refuses_symlink_parents_leaf_aliases_and_outside_paths() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("closeout-path");
    let outside = support::temp("closeout-outside");
    fs::create_dir(fixture.root.join("receipts")).unwrap();
    symlink(
        &outside,
        fixture.root.join(".exitbind/receipts/outside-link"),
    )
    .unwrap();
    symlink(
        fixture.root.join("receipts"),
        fixture.root.join(".exitbind/receipts/inside-link"),
    )
    .unwrap();
    let (work, _) = fixture.accepted();
    for destination in [
        ".exitbind/receipts/outside-link/proof.json",
        ".exitbind/receipts/inside-link/proof.json",
        "../escape.json",
        "missing/proof.json",
    ] {
        assert!(
            !fixture
                .call(&["work", "closeout", &work, "--output", destination], b"")
                .status
                .success(),
            "{destination}"
        );
    }
    assert!(!fixture
        .call(
            &[
                "work",
                "closeout",
                &work,
                "--output",
                outside.join("proof.json").to_str().unwrap()
            ],
            b""
        )
        .status
        .success());
    assert!(!outside.join("proof.json").exists());
    let created = fixture.json(&["work", "closeout", &work, "--export"], b"");
    let path = created["receipt"]["path"].as_str().unwrap();
    let alias = fixture.root.join(".exitbind/receipts/alias.json");
    symlink(fixture.root.join(path), &alias).unwrap();
    assert!(!fixture
        .call(
            &[
                "work",
                "closeout",
                &work,
                "--receipt",
                ".exitbind/receipts/alias.json"
            ],
            b""
        )
        .status
        .success());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(fixture.root.join(path), &alias).unwrap();
    assert!(!fixture
        .call(
            &[
                "work",
                "closeout",
                &work,
                "--receipt",
                ".exitbind/receipts/alias.json"
            ],
            b""
        )
        .status
        .success());
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn cross_project_missing_and_invalid_export_options_refuse_without_writes() {
    let fixture = Fixture::new("closeout-cross-project");
    let (work, _) = fixture.accepted();
    let created = fixture.json(&["work", "closeout", &work, "--export"], b"");
    let path = created["receipt"]["path"].as_str().unwrap();
    let other = Fixture::new("closeout-other-project");
    let (other_work, _) = other.accepted();
    fs::copy(
        fixture.root.join(path),
        other.root.join(".exitbind/receipts/foreign.json"),
    )
    .unwrap();
    for option in ["--output", "--receipt"] {
        assert!(!other
            .call(
                &[
                    "work",
                    "closeout",
                    &other_work,
                    option,
                    ".exitbind/receipts/foreign.json"
                ],
                b""
            )
            .status
            .success());
    }
    assert!(!other
        .call(&["work", "closeout", &work, "--export"], b"")
        .status
        .success());
    assert!(!other
        .call(
            &["work", "closeout", &other_work, "--receipt", "missing.json"],
            b""
        )
        .status
        .success());
    assert!(!other
        .call(
            &[
                "work",
                "closeout",
                &other_work,
                "--receipt",
                path,
                "--export"
            ],
            b""
        )
        .status
        .success());
    assert!(!other
        .call(
            &[
                "work",
                "closeout",
                &other_work,
                "--receipt",
                path,
                "--output",
                "new.json"
            ],
            b""
        )
        .status
        .success());
    assert!(!other.root.join("new.json").exists());
    // An output inside covered product inputs would invalidate the result it
    // purports to export. Refuse before creating it in a co-located project.
    assert!(!other
        .call(
            &[
                "work",
                "closeout",
                &other_work,
                "--output",
                "product-proof.json"
            ],
            b""
        )
        .status
        .success());
    assert!(!other.root.join("product-proof.json").exists());
    let selected = other.json(
        &[
            "work",
            "closeout",
            &other_work,
            "--output",
            ".exitbind/receipts/selected.json",
        ],
        b"",
    );
    assert_eq!(selected["receipt"]["state"], "created_verified");
}

#[test]
fn unrepresentable_output_refuses_before_write_and_representable_long_output_replays() {
    use std::{
        ffi::CString,
        os::unix::io::{AsRawFd, FromRawFd},
    };

    let fixture = Fixture::new("closeout-output-reply-bound");
    let (work, accepted) = fixture.accepted();
    let ledger = fixture
        .root
        .join(accepted["reference"]["ledger"].as_str().unwrap());
    let ledger_before = fs::read(&ledger).unwrap();
    let mut directory = fs::File::open(fixture.root.join(".exitbind/receipts")).unwrap();
    let mut destination = String::from(".exitbind/receipts");
    // Build owned real directories one component at a time. A single OS path
    // lookup cannot reach the negative case, while the product's secure
    // descriptor traversal supports it and must refuse before creating a leaf.
    for number in 0..80 {
        let segment = format!("segment{number:03}{}", "x".repeat(110));
        let name = CString::new(segment.as_str()).unwrap();
        let made = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
        assert_eq!(made, 0, "{}", std::io::Error::last_os_error());
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        assert!(descriptor >= 0, "{}", std::io::Error::last_os_error());
        directory = unsafe { fs::File::from_raw_fd(descriptor) };
        destination.push('/');
        destination.push_str(&segment);
        if number == 19 {
            let supported = format!("{destination}/supported.json");
            assert!(supported.len() > 2400);
            for (option, expected) in [
                ("--output", "created_verified"),
                ("--output", "existing_verified"),
                ("--receipt", "existing_verified"),
            ] {
                let output = fixture.call(&["work", "closeout", &work, option, &supported], b"");
                assert!(output.status.success(), "{output:?}");
                assert!(output.stdout.len() <= 8 * 1024);
                let result: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(result["receipt"]["state"], expected);
                assert_eq!(result["receipt"]["pathOmitted"], true);
                assert_eq!(result["effectiveAction"]["exists"], false);
            }
        }
    }
    destination.push_str("/proof.json");
    assert!(destination.len() > 8 * 1024);
    for _ in 0..2 {
        let output = fixture.call(&["work", "closeout", &work, "--output", &destination], b"");
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds its bounded channel"));
        let leaf = CString::new("proof.json").unwrap();
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor >= 0 {
            drop(unsafe { fs::File::from_raw_fd(descriptor) });
            panic!("unrepresentable reply created a receipt");
        }
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::NotFound
        );
    }
    let missing = fixture.call(&["work", "closeout", &work, "--receipt", &destination], b"");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("requested Work receipt is missing"));
    assert_eq!(fs::read(ledger).unwrap(), ledger_before);
}

#[test]
fn long_config_routes_require_exact_arguments_and_terminal_output_stays_bounded() {
    let parent = support::temp("closeout-long-path");
    let mut root = parent.clone();
    // Five components keep macOS file paths practical while the exact
    // executable/config argv still exceeds closeout's 768-byte budget.
    for _ in 0..5 {
        root.push("segment".repeat(18));
    }
    fs::create_dir_all(&root).unwrap();
    support::git_topology::repository(&root);
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let fixture = Fixture { root };
    let (work, _) = fixture.accepted();
    let detail = fixture.detail(&work);
    let route = &detail["completion"]["receipt"]["export"]["command"];
    assert_eq!(route["sameConfigRequired"], true);
    let mut argv = route["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(route["sameExecutableRequired"], false);
    assert_eq!(argv.last().unwrap(), "--config");
    argv.push(fixture.root.join("exitbind.json").to_str().unwrap().into());
    let output = support::git_topology::command(&argv[0])
        .args(&argv[1..])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.len() <= 8 * 1024);
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["receipt"]["state"], "created_verified");
    let compact = fixture.call(&["work", "next", &work], b"");
    assert!(compact.status.success(), "{compact:?}");
    assert!(compact.stdout.len() <= 8 * 1024);
    let compact: Value = serde_json::from_slice(&compact.stdout).unwrap();
    assert_eq!(compact["effectiveAction"]["exists"], false);
    assert_eq!(
        compact["current"]["completion"]["receipt"]["available"],
        true
    );
    assert_eq!(
        compact["current"]["completion"]["receipt"]["export"]["command"]["sameConfigRequired"],
        true
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn known_and_revoked_detail_routes_keep_canonical_goal_and_export_bounds() {
    let fixture = Fixture::new("closeout-detail-goal");
    fixture.json(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "external-closeout",
            "--goal",
            "deliver and publish",
            "--obligation",
            "owner still controls publication",
        ],
        b"",
    );
    let work = fixture.begin();
    let known = fixture.detail(&work);
    assert_eq!(known["complete"], true);
    fixture.outcome(&work, "scoped");
    assert!(!fixture
        .call(
            &[
                "work",
                "expand",
                &work,
                known["reference"].as_str().unwrap()
            ],
            b""
        )
        .status
        .success());
    assert_eq!(fixture.detail(&work)["recipient"]["role"], "worker");
    let (accepted, _) = fixture.accepted();
    let result = fixture.json(&["work", "closeout", &accepted], b"");
    assert_ne!(result["goalProgress"]["overall"], "completed");
    assert_eq!(result["workAcceptance"]["state"], "accepted");
    assert!(serde_json::to_vec(&result).unwrap().len() < 8 * 1024);
    let compact = fixture.json(&["work", "next", &accepted], b"");
    assert_eq!(
        compact["current"]["completion"]["goalProgress"]["overall"],
        result["goalProgress"]["overall"]
    );
}
