//! Public replacement behavior for an explicitly selected future standalone scope.
use super::*;

const SESSION: &str = "standalone-replacement-session";

struct Case {
    fixture: Fixture,
    store: PathBuf,
    work: String,
    binding: String,
}

/// Own the observer until the controlled checker is released and reaped,
/// including when a fixture assertion unwinds.
struct PendingCheck {
    child: std::process::Child,
    release: PathBuf,
}

impl PendingCheck {
    fn release_and_wait(&mut self) -> std::process::ExitStatus {
        fs::write(&self.release, b"release").expect("release the controlled checker");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll the check observer") {
                return status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "check did not stop after release"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

impl Drop for PendingCheck {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"release");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Case {
    fn new() -> Self {
        let fixture = Fixture::new();
        let (work, _) = fixture.begin_failed_check();
        let store = fixture.root.join(".exitbind/native-test-session-goals");
        fixture.ok_in_session(
            SESSION,
            &store,
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "standalone-owner-goal",
                "--goal",
                "Preserve the owner's independently evolving goal",
                "--none-applicable",
                "obligations,findings,blockers,decisions,externalActions",
            ],
        );
        let binding = fixture.ok_in_session(SESSION, &store, &["work", "next", &work, "--json"])
            ["current"]["binding"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            fixture,
            store,
            work,
            binding,
        }
    }

    fn preview_args(&self) -> Vec<String> {
        let mut args = replacement_args(&self.work, &self.binding);
        let index = args
            .iter()
            .position(|arg| arg == "--check-command")
            .unwrap();
        args[index + 1] = "test -f checker.sh".into();
        args.extend(["--successor-scope".into(), "standalone".into()]);
        args
    }

    fn preview(&self) -> Value {
        self.fixture
            .ok_in_session(SESSION, &self.store, &refs(&self.preview_args()))
    }

    fn apply_args(&self) -> Vec<String> {
        self.preview()["applyCommand"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect()
    }

    fn apply(&self) -> (Value, Vec<String>) {
        let args = self.apply_args();
        let result = self
            .fixture
            .ok_in_session(SESSION, &self.store, &refs(&args));
        (result, args)
    }

    fn goal_path(&self) -> PathBuf {
        native_session_root(&self.store, SESSION).join(".exitbind/session-goal.jsonl")
    }

    fn claim_path(&self) -> PathBuf {
        PathBuf::from(format!(
            "{}.supersede",
            self.fixture.ledger(&self.work).display()
        ))
    }
}

fn assert_refused_without_claim(case: &Case, session: &str, args: &[String]) {
    let before = fs::read(case.fixture.ledger(&case.work)).unwrap();
    let goal = fs::read(case.goal_path()).unwrap();
    let result = case
        .fixture
        .call_in_session(session, &case.store, &refs(args));
    assert!(!result.status.success(), "unexpected success: {result:?}");
    assert_eq!(fs::read(case.fixture.ledger(&case.work)).unwrap(), before);
    assert_eq!(fs::read(case.goal_path()).unwrap(), goal);
    assert!(!case.claim_path().exists());
}

#[test]
fn preview_unbound_requires_verified_current_caller_and_emits_scope_binding() {
    let case = Case::new();
    let before = fs::read(case.fixture.ledger(&case.work)).unwrap();
    let goal = fs::read(case.goal_path()).unwrap();
    let next = case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &["work", "next", &case.work, "--json"],
    );
    assert_eq!(
        next["current"]["actionForm"]["replacementScope"]["state"],
        "validated_current_caller_unbound"
    );
    let preview = case.preview();
    assert_eq!(preview["state"], "preview");
    assert_eq!(preview["decision"]["protocol"], 2);
    assert_eq!(preview["decision"]["scope"]["kind"], "standalone");
    assert_eq!(preview["decision"]["scope"]["currentBinding"], case.binding);
    let digest = preview["decision"]["scopeDecisionSha256"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    let argv = preview["applyCommand"]["argv"].as_array().unwrap();
    assert!(argv
        .windows(2)
        .any(|pair| pair[0] == "--scope-decision-sha256" && pair[1] == digest));
    assert_eq!(fs::read(case.fixture.ledger(&case.work)).unwrap(), before);
    assert_eq!(fs::read(case.goal_path()).unwrap(), goal);
    assert!(!case.claim_path().exists());
}

#[test]
fn omitted_scope_preserves_existing_unavailable_refusal() {
    let case = Case::new();
    let args = replacement_args(&case.work, &case.binding);
    assert_refused_without_claim(&case, SESSION, &args);
    assert_eq!(case.preview()["state"], "preview");
}

#[test]
fn incomplete_invalid_foreign_or_conflicting_contexts_refuse() {
    let case = Case::new();
    let args = case.preview_args();
    assert_refused_without_claim(&case, "another-session", &args);
    let no_selector = case.fixture.call(&refs(&args));
    assert!(!no_selector.status.success());
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    clear_native_selectors(&mut command);
    let conflicting = command
        .current_dir(&case.fixture.root)
        .args(refs(&args))
        .args(["--config", "exitbind.json"])
        .env("EXITBIND_SESSION_GOALS_DIR", &case.store)
        .env("CODEX_THREAD_ID", SESSION)
        .env("CODEX_SESSION_ID", "conflicting-session")
        .output()
        .unwrap();
    assert!(!conflicting.status.success());
    assert!(!case.claim_path().exists());
    let owner = native_session_root(&case.store, SESSION).join("owner.json");
    let saved_owner = fs::read(&owner).unwrap();
    fs::write(&owner, b"invalid owner record").unwrap();
    assert_refused_without_claim(&case, SESSION, &args);
    fs::write(owner, saved_owner).unwrap();

    let legacy = Case::new();
    let journal = legacy.goal_path();
    let mut records = support::goal_journal::records(&journal);
    let last = records.last_mut().unwrap();
    last["journeyMembershipProtocol"] = json!(1);
    last.as_object_mut().unwrap().remove("journeyAdmissions");
    last.as_object_mut().unwrap().remove("eventSha256");
    last["eventSha256"] = json!(digest_value(last));
    support::goal_journal::write(&journal, &records);
    let current = legacy.fixture.ok_in_session(
        SESSION,
        &legacy.store,
        &["work", "next", &legacy.work, "--json"],
    );
    let mut legacy_args = replacement_args(
        &legacy.work,
        current["current"]["binding"].as_str().unwrap(),
    );
    legacy_args.extend(["--successor-scope".into(), "standalone".into()]);
    assert_refused_without_claim(&legacy, SESSION, &legacy_args);
}

#[test]
fn existing_legacy_member_or_pending_admission_refuses_standalone() {
    let legacy = Case::new();
    legacy.fixture.ok(&[
        "goal",
        "incorporate",
        "--project",
        "--goal-id",
        &legacy.work,
        "--goal",
        "The existing project Goal remains bound to this Work",
        "--obligation",
        "Preserve the local Goal relation",
    ]);
    let current = legacy.fixture.ok_in_session(
        SESSION,
        &legacy.store,
        &["work", "next", &legacy.work, "--json"],
    );
    let mut legacy_args = replacement_args(
        &legacy.work,
        current["current"]["binding"].as_str().unwrap(),
    );
    legacy_args.extend(["--successor-scope".into(), "standalone".into()]);
    assert_refused_without_claim(&legacy, SESSION, &legacy_args);

    let fixture = Fixture::new();
    let store = fixture.root.join(".exitbind/native-test-session-goals");
    let (member, binding) = native_failed_work(&fixture, SESSION, &store);
    let mut args = replacement_args(&member, &binding);
    args.extend(["--successor-scope".into(), "standalone".into()]);
    let before = fs::read(fixture.ledger(&member)).unwrap();
    assert!(!fixture
        .call_in_session(SESSION, &store, &refs(&args))
        .status
        .success());
    assert_eq!(fs::read(fixture.ledger(&member)).unwrap(), before);

    let (unbound, _) = fixture.begin_failed_check();
    let journal = native_session_root(&store, SESSION).join(".exitbind/session-goal.jsonl");
    let parent = support::goal_journal::latest(&journal);
    let seed = parent["journeyMembers"].as_array().unwrap()[0].clone();
    let start = fixture.events(&unbound)[0].clone();
    let admission = admission_for_start(&seed, &unbound, &start, &parent);
    append_goal_admission(&native_session_root(&store, SESSION), admission);
    let current = fixture.ok_in_session(SESSION, &store, &["work", "next", &unbound, "--json"]);
    let mut args = replacement_args(&unbound, current["current"]["binding"].as_str().unwrap());
    args.extend(["--successor-scope".into(), "standalone".into()]);
    let before = fs::read(fixture.ledger(&unbound)).unwrap();
    let journal_before = fs::read(&journal).unwrap();
    assert!(!fixture
        .call_in_session(SESSION, &store, &refs(&args))
        .status
        .success());
    assert_eq!(fs::read(fixture.ledger(&unbound)).unwrap(), before);
    assert_eq!(fs::read(journal).unwrap(), journal_before);
}

#[test]
fn changed_caller_goal_head_config_or_scope_refuses_before_claim() {
    let changed_head = Case::new();
    let stale = changed_head.apply_args();
    let mut events = changed_head.fixture.events(&changed_head.work);
    let last = events.last_mut().unwrap();
    last["timestamp"] = json!("2030-01-01T00:00:00Z");
    last.as_object_mut().unwrap().remove("eventSha256");
    last["eventSha256"] = json!(digest_value(last));
    let mut lines = String::new();
    for event in &events {
        lines.push_str(&serde_json::to_string(event).unwrap());
        lines.push('\n');
    }
    fs::write(changed_head.fixture.ledger(&changed_head.work), lines).unwrap();
    assert_refused_without_claim(&changed_head, SESSION, &stale);
    let case = Case::new();
    let args = case.apply_args();
    assert_refused_without_claim(&case, "different-session", &args);
    let mut changed = args.clone();
    let scope = changed
        .iter()
        .position(|arg| arg == "--successor-scope")
        .unwrap();
    changed[scope + 1] = "other".into();
    assert_refused_without_claim(&case, SESSION, &changed);
    case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "standalone-owner-goal",
            "--goal",
            "Preserve the owner's independently evolving goal",
            "--obligation",
            "An unrelated subsequent goal observation",
        ],
    );
    assert_refused_without_claim(&case, SESSION, &args);
    let refreshed = case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &["work", "next", &case.work, "--json"],
    );
    let binding = refreshed["current"]["binding"].as_str().unwrap();
    let mut replacement = replacement_args(&case.work, binding);
    replacement.extend(["--successor-scope".into(), "standalone".into()]);
    let preview = case
        .fixture
        .ok_in_session(SESSION, &case.store, &refs(&replacement));
    let apply: Vec<String> = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let config = case.fixture.root.join("exitbind.json");
    let mut bytes = fs::read(&config).unwrap();
    bytes.push(b'\n');
    fs::write(&config, bytes).unwrap();
    assert_refused_without_claim(&case, SESSION, &apply);
}

#[test]
fn changed_reason_or_policy_refuses_before_claim() {
    let case = Case::new();
    let args = case.apply_args();
    for option in ["--reason", "--check-command", "--scope-decision-sha256"] {
        let mut changed = args.clone();
        let index = changed.iter().position(|arg| arg == option).unwrap();
        changed[index + 1] = if option == "--scope-decision-sha256" {
            "0".repeat(64)
        } else {
            "different explicit value".into()
        };
        assert_refused_without_claim(&case, SESSION, &changed);
    }
}

#[test]
fn exact_apply_replay_and_partial_claim_recovery_preserve_single_successor() {
    let case = Case::new();
    let (applied, args) = case.apply();
    assert_eq!(applied["state"], "successor_created");
    let successor = applied["successor"]["work"].as_str().unwrap();
    let replay = case
        .fixture
        .ok_in_session(SESSION, &case.store, &refs(&args));
    assert_eq!(replay["state"], "existing_successor_recovered");
    let path = case.fixture.ledger(successor);
    let bytes = fs::read(&path).unwrap();
    let claim = fs::read(case.claim_path()).unwrap();
    fs::remove_file(&path).unwrap();
    case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "standalone-owner-goal",
            "--goal",
            "Preserve the owner's independently evolving goal",
            "--obligation",
            "An unrelated later goal observation",
        ],
    );
    let current = case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &["work", "next", &case.work, "--json"],
    );
    let mut recover = args.clone();
    let index = recover
        .iter()
        .position(|arg| arg == "--current-binding")
        .unwrap();
    recover[index + 1] = current["current"]["binding"].as_str().unwrap().to_owned();
    let mut inspect = recover.clone();
    inspect.retain(|arg| arg != "--apply");
    let index = inspect
        .iter()
        .position(|arg| arg == "--scope-decision-sha256")
        .unwrap();
    inspect.drain(index..=index + 1);
    let pending = case
        .fixture
        .ok_in_session(SESSION, &case.store, &refs(&inspect));
    assert_eq!(pending["state"], "partial_successor_claim");
    assert_eq!(pending["effect"], "no-change");
    assert_eq!(
        case.fixture
            .ok_in_session(SESSION, &case.store, &refs(&recover))["state"],
        "partial_successor_claim"
    );
    recover = pending["recoveryApplyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect();
    assert!(recover.iter().any(|arg| arg == "--recover-partial"));
    assert!(!path.exists());
    let recovered = case
        .fixture
        .ok_in_session(SESSION, &case.store, &refs(&recover));
    assert_eq!(recovered["state"], "partial_successor_recovered");
    assert_eq!(recovered["effect"], "committed");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(case.claim_path()).unwrap(), claim);
    assert_eq!(
        case.fixture
            .ok_in_session(SESSION, &case.store, &refs(&recover))["state"],
        "existing_successor_recovered"
    );

    let conflict = Case::new();
    let (applied, mut args) = conflict.apply();
    let successor = applied["successor"]["work"].as_str().unwrap();
    let path = conflict.fixture.ledger(successor);
    fs::remove_file(&path).unwrap();
    let saved_claim = fs::read(conflict.claim_path()).unwrap();
    let seed = conflict.fixture.ok_in_session(
        SESSION,
        &conflict.store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "seed a native member",
            "--check-command",
            "true",
        ],
    );
    assert!(seed["work"].is_string());
    let journal = conflict.goal_path();
    let current_goal = support::goal_journal::latest(&journal);
    let member = current_goal["journeyMembers"].as_array().unwrap()[0].clone();
    let start = conflict.fixture.events(&conflict.work)[0].clone();
    let admission = admission_for_start(&member, &conflict.work, &start, &current_goal);
    append_goal_admission(&native_session_root(&conflict.store, SESSION), admission);
    let goal = fs::read(&journal).unwrap();
    let next = conflict.fixture.ok_in_session(
        SESSION,
        &conflict.store,
        &["work", "next", &conflict.work, "--json"],
    );
    let index = args
        .iter()
        .position(|arg| arg == "--current-binding")
        .unwrap();
    args[index + 1] = next["current"]["binding"].as_str().unwrap().to_owned();
    args.push("--recover-partial".into());
    assert!(!conflict
        .fixture
        .call_in_session(SESSION, &conflict.store, &refs(&args))
        .status
        .success());
    assert!(!path.exists());
    assert_eq!(fs::read(conflict.claim_path()).unwrap(), saved_claim);
    assert_eq!(fs::read(journal).unwrap(), goal);

    let missing = Case::new();
    let (applied, mut args) = missing.apply();
    let successor = applied["successor"]["work"].as_str().unwrap();
    let path = missing.fixture.ledger(successor);
    fs::remove_file(&path).unwrap();
    fs::remove_file(missing.claim_path()).unwrap();
    let next = missing.fixture.ok_in_session(
        SESSION,
        &missing.store,
        &["work", "next", &missing.work, "--json"],
    );
    let index = args
        .iter()
        .position(|arg| arg == "--current-binding")
        .unwrap();
    args[index + 1] = next["current"]["binding"].as_str().unwrap().to_owned();
    args.push("--recover-partial".into());
    assert!(!missing
        .fixture
        .call_in_session(SESSION, &missing.store, &refs(&args))
        .status
        .success());
    assert!(!missing.claim_path().exists());
    assert!(!path.exists());
}

#[test]
fn predecessor_journal_and_accounting_preserved_without_old_credit() {
    let case = Case::new();
    let predecessor = fs::read(case.fixture.ledger(&case.work)).unwrap();
    let goal = fs::read(case.goal_path()).unwrap();
    let predecessor_view = case.fixture.ok(&[
        "run",
        "inspect",
        &format!(".exitbind/runs/work-{}.jsonl", &case.work[4..]),
        "--json",
    ]);
    let (applied, _) = case.apply();
    let successor = applied["successor"]["work"].as_str().unwrap();
    let events = case.fixture.events(successor);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["checkReplacement"]["protocol"], 2);
    let carry = &events[0]["governorCarry"]["accounting"];
    for key in ["spent", "budget", "noInformationStreak", "postReplanSpent"] {
        assert_eq!(carry[key], predecessor_view["governor"][key]);
    }
    assert!(carry["spent"].as_u64().is_some_and(|n| n > 0));
    assert_eq!(
        fs::read(case.fixture.ledger(&case.work)).unwrap(),
        predecessor
    );
    assert_eq!(fs::read(case.goal_path()).unwrap(), goal);
    let detail = case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &["work", "detail", successor, "--json"],
    );
    assert_eq!(
        detail["checkReplacement"]["successorScope"]["kind"],
        "standalone"
    );
    assert_eq!(detail["checkReplacement"]["state"], "verified");
}

#[test]
fn standalone_scope_survives_full_lifecycle_without_goal_membership() {
    let case = Case::new();
    let (applied, _) = case.apply();
    let successor = applied["successor"]["work"].as_str().unwrap().to_owned();
    case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "standalone-owner-goal",
            "--goal",
            "Preserve the owner's independently evolving goal",
            "--obligation",
            "An unrelated later goal revision",
        ],
    );
    let goal = fs::read(case.goal_path()).unwrap();
    for outcome in ["scoped", "completed"] {
        let next = case.fixture.ok_in_session(
            SESSION,
            &case.store,
            &["work", "next", &successor, "--json", "--full"],
        );
        case.fixture.ok_in_session(
            SESSION,
            &case.store,
            &[
                "work",
                "return",
                &successor,
                next["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
        );
    }
    let check = case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &["work", "check", &successor, "--timeout-ms", "2000"],
    );
    assert_eq!(check["result"]["code"], 0);
    for outcome in ["approved", "accepted"] {
        let next = case.fixture.ok_in_session(
            SESSION,
            &case.store,
            &["work", "next", &successor, "--json", "--full"],
        );
        case.fixture.ok_in_session(
            SESSION,
            &case.store,
            &[
                "work",
                "return",
                &successor,
                next["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
        );
    }
    assert_eq!(
        case.fixture.ok_in_session(
            SESSION,
            &case.store,
            &["work", "next", &successor, "--json"]
        )["next"]["action"],
        "done"
    );
    assert_eq!(fs::read(case.goal_path()).unwrap(), goal);
    let latest = support::goal_journal::latest(&case.goal_path());
    assert!(!latest["journeyMembers"].to_string().contains(&successor));
}

#[test]
fn repeated_replacement_requires_a_new_standalone_decision() {
    let case = Case::new();
    let mut preview_args = case.preview_args();
    let index = preview_args
        .iter()
        .position(|arg| arg == "--check-command")
        .unwrap();
    preview_args[index + 1] = "exit 9".into();
    let preview = case
        .fixture
        .ok_in_session(SESSION, &case.store, &refs(&preview_args));
    let first_decision = preview["decision"]["scopeDecisionSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let args: Vec<String> = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect();
    let applied = case
        .fixture
        .ok_in_session(SESSION, &case.store, &refs(&args));
    let successor = applied["successor"]["work"].as_str().unwrap().to_owned();
    for outcome in ["scoped", "completed"] {
        let next = case.fixture.ok_in_session(
            SESSION,
            &case.store,
            &["work", "next", &successor, "--json", "--full"],
        );
        case.fixture.ok_in_session(
            SESSION,
            &case.store,
            &[
                "work",
                "return",
                &successor,
                next["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
        );
    }
    assert!(!case
        .fixture
        .call_in_session(
            SESSION,
            &case.store,
            &["work", "check", &successor, "--timeout-ms", "2000"]
        )
        .status
        .success());
    let current = case.fixture.ok_in_session(
        SESSION,
        &case.store,
        &["work", "next", &successor, "--json"],
    );
    let binding = current["current"]["binding"].as_str().unwrap();
    let mut replacement = replacement_args(&successor, binding);
    assert!(!case
        .fixture
        .call_in_session(SESSION, &case.store, &refs(&replacement))
        .status
        .success());
    replacement.extend(["--successor-scope".into(), "standalone".into()]);
    let second = case
        .fixture
        .ok_in_session(SESSION, &case.store, &refs(&replacement));
    assert_eq!(second["state"], "preview");
    assert_ne!(second["decision"]["scopeDecisionSha256"], first_decision);
}

#[test]
fn protocol_one_unchanged_protocol_two_exact_shape_and_pairing() {
    let plain = Fixture::new();
    let (old, _) = plain.begin_failed_check();
    let binding = plain.ok(&["work", "next", &old, "--json"])["current"]["binding"]
        .as_str()
        .unwrap()
        .to_owned();
    let preview = plain.ok(&refs(&replacement_args(&old, &binding)));
    assert_eq!(preview["decision"]["protocol"], 1);
    assert!(preview["decision"].get("scope").is_none());
    let case = Case::new();
    let (applied, apply_args) = case.apply();
    let successor = applied["successor"]["work"].as_str().unwrap();
    let path = case.fixture.ledger(successor);
    let original = fs::read(&path).unwrap();
    let mut event = case.fixture.events(successor)[0].clone();
    let claim: Value = serde_json::from_slice(&fs::read(case.claim_path()).unwrap()).unwrap();
    assert_eq!(claim["checkReplacement"], event["checkReplacement"]);
    for changed_shape in [true, false] {
        if changed_shape {
            event["checkReplacement"]["scope"]["unexpected"] = json!(true);
        } else {
            event["checkReplacement"]["scope"]
                .as_object_mut()
                .unwrap()
                .remove("unexpected");
            event["checkReplacement"]["scopeDecisionSha256"] = json!("0".repeat(64));
        }
        event.as_object_mut().unwrap().remove("eventSha256");
        event["eventSha256"] = json!(digest_value(&event));
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&event).unwrap()),
        )
        .unwrap();
        assert!(!case
            .fixture
            .call_in_session(
                SESSION,
                &case.store,
                &["work", "detail", successor, "--json"]
            )
            .status
            .success());
    }
    let mut unpaired = case.fixture.events(successor)[0].clone();
    unpaired["checkReplacement"] = claim["checkReplacement"].clone();
    unpaired["supersedes"]["headEventSha256"] = json!("0".repeat(64));
    unpaired.as_object_mut().unwrap().remove("eventSha256");
    unpaired["eventSha256"] = json!(digest_value(&unpaired));
    fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&unpaired).unwrap()),
    )
    .unwrap();
    assert!(!case
        .fixture
        .call_in_session(SESSION, &case.store, &refs(&apply_args))
        .status
        .success());
    fs::write(path, original).unwrap();
}

#[test]
fn unresolved_check_or_nonlead_or_invalid_source_refuses() {
    let case = Case::new();
    let mut args = case.preview_args();
    let class = args
        .iter()
        .position(|arg| arg == "--classification")
        .unwrap();
    args[class + 1] = "ordinary-failure".into();
    assert_refused_without_claim(&case, SESSION, &args);
    let other = &case.fixture;
    let begin = other.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "no failed source",
        "--check-command",
        "true",
    ]);
    let work = begin["work"].as_str().unwrap();
    let lead = other.ok_in_session(SESSION, &case.store, &["work", "next", work, "--json"]);
    let mut no_source = replacement_args(work, lead["current"]["binding"].as_str().unwrap());
    no_source.extend(["--successor-scope".into(), "standalone".into()]);
    let missing = other.call_in_session(SESSION, &case.store, &refs(&no_source));
    assert!(!missing.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&missing.stdout),
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(
        diagnostic.contains("no exact failed-check or blocked-worker source evidence"),
        "{diagnostic}"
    );
    other.ok(&[
        "work",
        "return",
        work,
        begin["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = other.ok_in_session(SESSION, &case.store, &["work", "next", work, "--json"]);
    assert_eq!(worker["next"]["role"], "worker");
    let mut args = replacement_args(work, worker["current"]["binding"].as_str().unwrap());
    args.extend(["--successor-scope".into(), "standalone".into()]);
    let before = fs::read(other.ledger(work)).unwrap();
    assert!(!other
        .call_in_session(SESSION, &case.store, &refs(&args))
        .status
        .success());
    assert_eq!(fs::read(other.ledger(work)).unwrap(), before);

    fs::write(
        other.root.join("waiting-check.sh"),
        "printf ready > .exitbind/scope-check-started || exit 2\n\
         i=0\n\
         while [ ! -f .exitbind/scope-check-release ] && [ \"$i\" -lt 1000 ]; do\n\
           sleep 0.05\n\
           i=$((i + 1))\n\
         done\n\
         [ -f .exitbind/scope-check-release ] || exit 2\n\
         exit 1\n",
    )
    .unwrap();
    let running_work = other.begin_completed_worker(
        "exec sh waiting-check.sh",
        "a running observation is not replaceable",
    );
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    clear_native_selectors(&mut command);
    let child = command
        .current_dir(&other.root)
        .args([
            "work",
            "check",
            &running_work,
            "--timeout-ms",
            "60000",
            "--config",
            "exitbind.json",
        ])
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut observer = PendingCheck {
        child,
        release: other.root.join(".exitbind/scope-check-release"),
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !other.root.join(".exitbind/scope-check-started").exists() {
        assert!(
            observer.child.try_wait().unwrap().is_none(),
            "observer ended before ready"
        );
        assert!(std::time::Instant::now() < deadline, "check did not start");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let events = other.events(&running_work);
    let admission = events
        .iter()
        .find(|event| event["action"] == "check_observation")
        .expect("a running check must have a durable admission");
    assert_eq!(admission["observation"]["state"], "running");
    assert_eq!(
        admission["observation"]["observer"]["pid"].as_u64(),
        Some(u64::from(observer.child.id()))
    );
    assert!(
        observer.child.try_wait().unwrap().is_none(),
        "observer ended before the refusal"
    );
    let next = other.ok_in_session(
        SESSION,
        &case.store,
        &["work", "next", &running_work, "--json"],
    );
    let mut unresolved =
        replacement_args(&running_work, next["current"]["binding"].as_str().unwrap());
    unresolved.extend(["--successor-scope".into(), "standalone".into()]);
    let refused = other.call_in_session(SESSION, &case.store, &refs(&unresolved));
    assert!(!refused.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        diagnostic.contains("unresolved check observation"),
        "{diagnostic}"
    );
    assert!(
        observer.child.try_wait().unwrap().is_none(),
        "observer ended before release"
    );
    assert!(
        !observer.release_and_wait().success(),
        "the released checker must fail"
    );
    let events = other.events(&running_work);
    assert!(events
        .iter()
        .any(|event| event["action"] == "check" && event["result"]["code"] == 1));
}

#[test]
#[ignore = "requires the pinned previous 3cca executable; run only in the mixed-version Box fixture"]
fn old_reader_refuses_protocol_two_protected_effects() {
    let old = std::env::var_os("EXITBIND_OLD_BIN").expect("pinned previous executable");
    let old_bytes = fs::read(&old).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(old_bytes)),
        "8f68e62f55aeecdac637760ff2dfe18bbb53affc8ee077f3260cca4fba7130fb"
    );
    let case = Case::new();
    let (applied, _) = case.apply();
    let successor = applied["successor"]["work"].as_str().unwrap();
    let before = fs::read(case.fixture.ledger(successor)).unwrap();
    let goal_before = fs::read(case.goal_path()).unwrap();
    let current =
        case.fixture
            .ok_in_session(SESSION, &case.store, &["work", "next", successor, "--json"]);
    let assignment = current["next"]["assignment"].as_str().unwrap();
    for command in [
        vec!["work", "detail", successor, "--json"],
        vec!["work", "check", successor],
        vec![
            "work",
            "return",
            successor,
            assignment,
            "--outcome",
            "scoped",
        ],
    ] {
        let mut process = support::git_topology::command(&old);
        clear_native_selectors(&mut process);
        let output = process
            .current_dir(&case.fixture.root)
            .args(command)
            .args(["--config", "exitbind.json"])
            .env("EXITBIND_SESSION_GOALS_DIR", &case.store)
            .env("CODEX_THREAD_ID", SESSION)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "old reader accepted protocol 2: {output:?}"
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(text.contains("invalid check replacement"), "{text}");
        assert_eq!(fs::read(case.fixture.ledger(successor)).unwrap(), before);
        assert_eq!(fs::read(case.goal_path()).unwrap(), goal_before);
    }
}
