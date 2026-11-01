//! Native-only successor admission, compatibility, and interrupted recovery.

#[path = "native_supersede/reason.rs"]
mod reason;

use crate::fixture::*;
use serde_json::{json, Value};
use std::{
    ffi::CString,
    fs,
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::PathBuf,
    process::Stdio,
};

const PARENT: &str = "native-successor-parent";
const ADOPTER: &str = "native-successor-adopter";

struct Case {
    project: Project,
    predecessor: String,
    successor: String,
}

impl Case {
    fn new(label: &str) -> Self {
        Self::new_with_boundary(label, false)
    }

    fn new_with_boundary(label: &str, narrowed: bool) -> Self {
        let project = Project::new(label);
        if narrowed {
            fs::create_dir_all(project.root.join("src")).unwrap();
            fs::write(project.root.join("src/a.txt"), "allowed\n").unwrap();
            fs::write(project.root.join("src/b.txt"), "outside\n").unwrap();
            fs::create_dir_all(project.root.join(".agents/boundaries")).unwrap();
            fs::write(
                project.root.join(".agents/boundaries/task.json"),
                json!({"version":1,
                "agents":{"worker":{"observe":["src/a.txt"],"write":["src/a.txt"]}}})
                .to_string(),
            )
            .unwrap();
            let path = project.root.join("exitbind.json");
            let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            config["agents"]["worker"]["observe"] = json!(["src/**"]);
            config["agents"]["worker"]["write"] = json!(["src/**"]);
            fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        }
        let store = &project.session_store;
        project.value_in_session(
            "codex",
            PARENT,
            store,
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "native-successor",
                "--goal",
                "Complete the same conversation task",
                "--obligation",
                "Complete the task",
            ],
        );
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            "Carry the task forward",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ];
        if narrowed {
            args.extend(["--boundary", ".agents/boundaries/task.json"]);
        }
        let begin = project.value_in_session("codex", PARENT, store, &args);
        let predecessor = begin["work"].as_str().unwrap().to_owned();
        let lead = begin["next"]["assignment"].as_str().unwrap();
        project.value_in_session(
            "codex",
            PARENT,
            store,
            &["work", "return", &predecessor, lead, "--outcome", "scoped"],
        );
        let next = project.value_in_session(
            "codex",
            PARENT,
            store,
            &["work", "next", &predecessor, "--json", "--full"],
        );
        let worker = next["next"]["assignment"].as_str().unwrap();
        project.value_in_session(
            "codex",
            PARENT,
            store,
            &[
                "work",
                "permit",
                &predecessor,
                worker,
                "--operation",
                "Perform the scoped edit",
            ],
        );
        let parent_key = digest_value(&json!({"host":"codex", "identity":PARENT}));
        project.value_in_session(
            "codex",
            ADOPTER,
            store,
            &["goal", "adopt", "--session-key", &parent_key, "--json"],
        );
        let successor = String::new();
        Self {
            project,
            predecessor,
            successor,
        }
    }

    fn call(&self, args: &[&str]) -> std::process::Output {
        self.project
            .call_in_session("codex", ADOPTER, &self.project.session_store, args)
    }

    fn supersede(&mut self) -> Value {
        let value = self.project.value_in_session(
            "codex",
            ADOPTER,
            &self.project.session_store,
            &[
                "work",
                "supersede",
                &self.predecessor,
                "--reason=--json",
                "--json",
            ],
        );
        self.successor = value["work"].as_str().unwrap().to_owned();
        value
    }

    fn ledger(&self, work: &str) -> PathBuf {
        self.project
            .root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..]))
    }

    fn companion(&self) -> PathBuf {
        self.project.root.join(format!(
            ".exitbind/runs/work-{}.session-goal.json",
            &self.successor[4..]
        ))
    }

    fn journal(&self) -> PathBuf {
        native_goal_journal(&self.project, PARENT)
    }

    fn rewind_to_pending(&self, keep_claim: bool, keep_start: bool) {
        let records = super::support::goal_journal::records(&self.journal());
        let admitted = records
            .iter()
            .position(|record| {
                record["journeyAdmissions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["work"] == self.successor)
            })
            .expect("saved exact admission");
        super::support::goal_journal::write(&self.journal(), &records[..=admitted]);
        fs::remove_file(self.companion()).unwrap();
        if !keep_start {
            fs::remove_file(self.ledger(&self.successor)).unwrap();
        }
        if !keep_claim {
            fs::remove_file(format!(
                "{}.supersede",
                self.ledger(&self.predecessor).display()
            ))
            .unwrap();
        }
    }
}

#[test]
fn adopted_native_successor_carries_spend_links_and_replays_without_old_credit() {
    let mut case = Case::new("native-successor-adopted");
    let predecessor = fs::read(case.ledger(&case.predecessor)).unwrap();
    let result = case.supersede();
    assert_eq!(result["effect"], "started");
    assert_eq!(result["sessionLink"]["state"], "linked");
    assert!(result["recovery"]["requiresExactOriginalReason"]
        .as_bool()
        .unwrap());
    assert!(result["recovery"]["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .any(|word| word == "--reason=--json"));
    assert_eq!(
        fs::read(case.ledger(&case.predecessor)).unwrap(),
        predecessor
    );
    let events = fs::read_to_string(case.ledger(&case.successor)).unwrap();
    let start: Value = serde_json::from_str(events.lines().next().unwrap()).unwrap();
    assert_eq!(start["governorCarry"]["accounting"]["spent"], 1);
    assert!(start.get("currentGrant").is_none());
    assert!(start.get("checkResult").is_none());
    let detail = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "next", &case.successor, "--json", "--full"],
    );
    assert_eq!(detail["next"]["role"], "lead");
    assert_eq!(
        detail["next"]["goalProgress"]["goal"],
        "Complete the same conversation task"
    );
    let again = case.supersede();
    assert_eq!(again["effect"], "started");
    assert_eq!(
        fs::read_to_string(case.ledger(&case.successor)).unwrap(),
        events
    );
    let lead = detail["next"]["assignment"].as_str().unwrap();
    case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &[
            "work",
            "return",
            &case.successor,
            lead,
            "--outcome",
            "scoped",
        ],
    );
    let worker = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "next", &case.successor, "--json", "--full"],
    );
    let route = worker["next"]["prepareChild"]["command"]
        .as_array()
        .expect("the linked successor emits its named-child preparation");
    let argv = route
        .iter()
        .map(|word| word.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(&argv[1..4], &["work", "child", "prepare"]);
    let mut command = super::support::git_topology::command(argv[0]);
    command
        .current_dir(&case.project.root)
        .args(&argv[1..])
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CODEX_THREAD_ID", ADOPTER)
        .env("EXITBIND_SESSION_GOALS_DIR", &case.project.session_store);
    let prepared = command.output().unwrap();
    assert!(
        prepared.status.success(),
        "the exact named-child route must work: {prepared:?}"
    );
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["effect"], "prepared");
}

#[test]
fn successor_preserves_narrowed_recipient_boundary_and_refuses_manifest_drift() {
    let mut case = Case::new_with_boundary("native-successor-narrow-boundary", true);
    let old = fs::read_to_string(case.ledger(&case.predecessor)).unwrap();
    let old: Value = serde_json::from_str(old.lines().next().unwrap()).unwrap();
    case.supersede();
    let new = fs::read_to_string(case.ledger(&case.successor)).unwrap();
    let new: Value = serde_json::from_str(new.lines().next().unwrap()).unwrap();
    assert_eq!(
        new["plan"], old["plan"],
        "the native route must retain the exact frozen plan"
    );
    let worker = new["plan"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|stage| stage["agents"].as_array().unwrap())
        .find(|agent| agent["name"] == "worker")
        .unwrap();
    assert_eq!(worker["declaredBoundary"]["observe"], json!(["src/a.txt"]));
    assert_eq!(worker["declaredBoundary"]["write"], json!(["src/a.txt"]));
    let lead = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "next", &case.successor, "--json", "--full"],
    );
    let lead_assignment = lead["next"]["assignment"].as_str().unwrap();
    case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &[
            "work",
            "return",
            &case.successor,
            lead_assignment,
            "--outcome",
            "scoped",
        ],
    );
    let worker = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "next", &case.successor, "--json", "--full"],
    );
    let assignment = worker["next"]["assignment"].as_str().unwrap();
    let prepared = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "file", "prepare", &case.successor, assignment],
    );
    let session = prepared["session"].as_str().unwrap();
    let allowed = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "file", "read", session, "src/a.txt"],
    );
    assert_eq!(allowed["content"], "allowed\n");
    let outside = case.call(&["work", "file", "read", session, "src/b.txt"]);
    assert!(
        !outside.status.success(),
        "an otherwise configured path outside the frozen boundary must refuse"
    );
    assert!(String::from_utf8_lossy(&outside.stderr)
        .contains("outside the current worker read boundary"));
    assert_eq!(
        fs::read_to_string(case.project.root.join("src/b.txt")).unwrap(),
        "outside\n"
    );

    let stale = Case::new_with_boundary("native-successor-stale-boundary", true);
    fs::write(stale.project.root.join(".agents/boundaries/task.json"), json!({"version":1,
        "agents":{"worker":{"observe":["src/a.txt", "src/b.txt"],"write":["src/a.txt", "src/b.txt"]}}}).to_string()).unwrap();
    let refused = stale.call(&["work", "supersede", &stale.predecessor, "--reason=--json"]);
    assert!(
        !refused.status.success(),
        "changed boundary evidence must refuse: {refused:?}"
    );
    assert!(!PathBuf::from(format!(
        "{}.supersede",
        stale.ledger(&stale.predecessor).display()
    ))
    .exists());
}

#[test]
fn exact_request_recovers_admission_claim_and_written_start_without_duplicates() {
    for (claim, start) in [(false, false), (true, false), (true, true)] {
        let mut case = Case::new("native-successor-recovery");
        case.supersede();
        let successor = case.successor.clone();
        case.rewind_to_pending(claim, start);
        let before = fs::read(case.journal()).unwrap();
        let changed = case.call(&[
            "work",
            "supersede",
            &case.predecessor,
            "--reason",
            "different",
        ]);
        assert!(
            !changed.status.success(),
            "different request must not reuse admission: {changed:?}"
        );
        assert_eq!(fs::read(case.journal()).unwrap(), before);
        let result = case.supersede();
        assert_eq!(case.successor, successor);
        assert_eq!(result["sessionLink"]["state"], "linked");
        assert_eq!(
            fs::read_to_string(case.ledger(&successor))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}

#[test]
fn already_started_successor_without_strong_prior_admission_is_not_backfilled() {
    let mut case = Case::new("native-successor-no-backfill");
    case.supersede();
    let records = super::support::goal_journal::records(&case.journal());
    let admitted = records
        .iter()
        .position(|record| {
            record["journeyAdmissions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["work"] == case.successor)
        })
        .unwrap();
    super::support::goal_journal::write(&case.journal(), &records[..admitted]);
    fs::remove_file(case.companion()).unwrap();
    let before_goal = fs::read(case.journal()).unwrap();
    let before_start = fs::read(case.ledger(&case.successor)).unwrap();
    let refused = case.call(&["work", "supersede", &case.predecessor, "--reason=--json"]);
    assert!(
        !refused.status.success(),
        "a started legacy successor cannot gain a new admission"
    );
    assert_eq!(fs::read(case.journal()).unwrap(), before_goal);
    assert_eq!(
        fs::read(case.ledger(&case.successor)).unwrap(),
        before_start
    );
    assert!(!case.companion().exists());
}

#[test]
fn standalone_wrong_session_and_changed_goal_do_not_gain_admission() {
    let mut case = Case::new("native-successor-refusal");
    let standalone = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Independent",
            "--check-command",
            "true",
            "--standalone",
        ],
    );
    let independent = standalone["work"].as_str().unwrap();
    assert!(!case
        .call(&["work", "supersede", independent, "--reason=--json"])
        .status
        .success());
    let wrong = case.project.call_in_session(
        "codex",
        "unrelated-session",
        &case.project.session_store,
        &["work", "supersede", &case.predecessor, "--reason=--json"],
    );
    assert!(!wrong.status.success());
    case.supersede();
    case.rewind_to_pending(false, false);
    let mut records = super::support::goal_journal::records(&case.journal());
    let previous = records.last().unwrap();
    let mut changed = previous.clone();
    changed["goal"] = json!("A materially changed Goal");
    changed["revision"] = json!(previous["revision"].as_u64().unwrap() + 1);
    changed["predecessor"] =
        json!({"goalId": previous["goalId"], "revision": previous["revision"]});
    changed["previousEventSha256"] = previous["eventSha256"].clone();
    changed.as_object_mut().unwrap().remove("eventSha256");
    changed["eventSha256"] = json!(digest_value(&changed));
    records.push(changed);
    super::support::goal_journal::write(&case.journal(), &records);
    let before = fs::read(case.journal()).unwrap();
    let refused = case.call(&["work", "supersede", &case.predecessor, "--reason=--json"]);
    assert!(
        !refused.status.success(),
        "changed Goal must refuse: {refused:?}"
    );
    assert_eq!(fs::read(case.journal()).unwrap(), before);
    assert!(!case.ledger(&case.successor).exists());
}

// Reader fixture matching the public 0.31.24 admission fields and its
// deny-unknown-fields rule. This intentionally knows nothing about the new
// domain commitment and checks both pending and linked legacy representations.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyAdmission {
    work: String,
    start_intent_sha256: String,
    parent_goal_id: String,
    parent_observed_goal_event_sha256: String,
    work_goal_sha256: String,
    host: String,
    session_key: String,
    owner_identity_sha256: String,
    owner_project_id: Option<String>,
    project_identity_sha256: String,
    local_identity_sha256: String,
    config_path: String,
    config_sha256: String,
    product_root: String,
    state_root: String,
}

#[test]
fn legacy_reader_sees_pending_but_only_links_ordinary_intent_and_reads_linked_companion() {
    let mut case = Case::new("native-successor-legacy-reader");
    case.supersede();
    let history = super::support::goal_journal::records(&case.journal());
    let pending = history
        .iter()
        .find_map(|record| {
            record["journeyAdmissions"]
                .as_array()?
                .iter()
                .find(|item| item["work"] == case.successor)
        })
        .expect("historical pending admission");
    let old: LegacyAdmission = serde_json::from_value(pending.clone()).unwrap();
    assert_eq!(serde_json::to_value(&old).unwrap(), *pending);
    let source = fs::read_to_string(case.ledger(&case.successor)).unwrap();
    let start: Value = serde_json::from_str(source.lines().next().unwrap()).unwrap();
    assert_ne!(
        old.start_intent_sha256,
        start_intent_sha256(&start),
        "the old exact-intent link must refuse the domain-separated pending admission"
    );
    let companion: Value = serde_json::from_slice(&fs::read(case.companion()).unwrap()).unwrap();
    assert_eq!(companion.as_object().unwrap().len(), 11);
    assert_eq!(companion["kind"], "native_session_goal_context");
    assert_eq!(companion["protocol"], 1);
    assert_eq!(companion["startEventSha256"], start["eventSha256"]);
    let mut unsigned = companion.clone();
    let saved = unsigned.as_object_mut().unwrap().remove("sha256").unwrap();
    assert_eq!(saved, digest_value(&unsigned));
    let latest = history.last().unwrap();
    assert!(latest["journeyAdmissions"].as_array().unwrap().is_empty());
    assert!(latest["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|member| member["work"] == case.successor
            && member["startEventSha256"] == start["eventSha256"]));
}

#[test]
fn managed_journal_wins_or_successor_claim_wins_without_a_duplicate_launch() {
    let mut case = Case::new("native-successor-managed-fence");
    let view = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &["work", "next", &case.predecessor, "--json", "--full"],
    );
    let next = &view["next"];
    let assignment = next["assignment"].as_str().unwrap();
    let directory = case
        .project
        .root
        .join(".exitbind/native-actions")
        .join(&case.predecessor);
    fs::create_dir_all(&directory).unwrap();
    let journal_path = directory.join(format!("{assignment}.json"));
    let mut journal = json!({"version":1,"work":case.predecessor,"assignment":assignment,
        "status":"running","role":next["role"],"agent":next["agent"],
        "stage":next["packet"]["stage"],"attempt":next["packet"]["attempt"],
        "request":{"executable":"fixture-provider"}});
    for status in ["running", "started"] {
        journal["status"] = json!(status);
        fs::write(&journal_path, journal.to_string()).unwrap();
        let refused = case.call(&["work", "supersede", &case.predecessor, "--reason=--json"]);
        assert!(
            !refused.status.success(),
            "{status} without a known end must refuse: {refused:?}"
        );
        assert!(!PathBuf::from(format!(
            "{}.supersede",
            case.ledger(&case.predecessor).display()
        ))
        .exists());
    }
    journal["observation"] = json!({"interrupted":false,
        "process":{"success":false,"code":1,"signal":null,"timedOut":false}});
    journal["provisionalFinalResult"] = json!({"summary":"held"});
    fs::write(&journal_path, journal.to_string()).unwrap();
    assert!(!case
        .call(&["work", "supersede", &case.predecessor, "--reason=--json"])
        .status
        .success());
    fs::remove_file(&journal_path).unwrap();

    case.supersede();
    let sentinel = directory.join("provider-invoked");
    let fake = directory.join("fake-codex");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\nprintf invoked > {}\nexit 1\n",
            shell_quote(&sentinel.display().to_string())
        ),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let refused = case.call(&[
        "work",
        "act",
        &case.predecessor,
        "--codex-bin",
        fake.to_str().unwrap(),
    ]);
    assert!(
        !refused.status.success(),
        "retired Work must refuse a fresh launch: {refused:?}"
    );
    assert!(
        !sentinel.exists(),
        "provider must never be invoked after the claim"
    );
    assert!(
        !journal_path.exists(),
        "retirement must not admit a replacement journal"
    );
}

#[test]
fn managed_launch_persists_before_process_and_blocks_concurrent_retirement() {
    let case = Case::new("native-successor-managed-live-fence");
    let directory = case
        .project
        .root
        .join(".exitbind/native-actions")
        .join(&case.predecessor);
    fs::create_dir_all(&directory).unwrap();
    let ready_path = directory.join("ready.fifo");
    let release_path = directory.join("release.fifo");
    for path in [&ready_path, &release_path] {
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    }
    let open_fifo = |path: &PathBuf| {
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .unwrap()
    };
    let ready = open_fifo(&ready_path);
    let mut release = open_fifo(&release_path);
    let sentinel = directory.join("launch-count");
    let fake = directory.join("fake-codex");
    fs::write(&fake, format!("#!/bin/sh\ncat >/dev/null\nprintf x >> {}\nprintf ready > {}\nread unused < {}\nexit 1\n",
        shell_quote(&sentinel.display().to_string()), shell_quote(&ready_path.display().to_string()),
        shell_quote(&release_path.display().to_string()))).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&case.project.root)
        .args([
            "work",
            "act",
            &case.predecessor,
            "--codex-bin",
            fake.to_str().unwrap(),
            "--timeout-ms",
            "20000",
        ])
        .arg("--config")
        .arg(case.project.root.join("exitbind.json"))
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CODEX_THREAD_ID", ADOPTER)
        .env("EXITBIND_SESSION_GOALS_DIR", &case.project.session_store)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let worker = command.spawn().unwrap();
    let mut poll = libc::pollfd {
        fd: ready.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let received = unsafe { libc::poll(&mut poll, 1, 15_000) } == 1;
    let refusal =
        received.then(|| case.call(&["work", "supersede", &case.predecessor, "--reason=--json"]));
    let claim_exists = PathBuf::from(format!(
        "{}.supersede",
        case.ledger(&case.predecessor).display()
    ))
    .exists();
    release.write_all(b"go\n").unwrap();
    let output = worker.wait_with_output().unwrap();
    assert!(
        received,
        "fake provider did not reach the owned barrier: {output:?}"
    );
    let refusal = refusal.unwrap();
    assert!(
        !refusal.status.success(),
        "an active owned native launch must block retirement: {refusal:?}"
    );
    assert!(
        !claim_exists,
        "no successor claim may be created behind an active provider"
    );
    assert_eq!(
        fs::read(sentinel).unwrap(),
        b"x",
        "no duplicate provider launched"
    );
}

#[test]
fn prepared_child_blocks_successor_and_old_preparation_cannot_claim_after_retirement() {
    let mut case = Case::new("native-successor-child-fence");
    let prepared = case.project.value_in_session(
        "codex",
        ADOPTER,
        &case.project.session_store,
        &[
            "work",
            "child",
            "prepare",
            &case.predecessor,
            "fixture child",
        ],
    );
    assert_eq!(prepared["effect"], "prepared");
    let intents = case.project.root.join(".exitbind/child-intents.json");
    let retained = fs::read(&intents).unwrap();
    let refused = case.call(&["work", "supersede", &case.predecessor, "--reason=--json"]);
    assert!(
        !refused.status.success(),
        "a prepared child must block retirement: {refused:?}"
    );
    assert!(!PathBuf::from(format!(
        "{}.supersede",
        case.ledger(&case.predecessor).display()
    ))
    .exists());

    // Construct the race edge of a prepared record created by an earlier
    // compatible snapshot, then let the successor claim win first.
    fs::remove_file(&intents).unwrap();
    case.supersede();
    let later = case.call(&["work", "child", "prepare", &case.predecessor, "new child"]);
    assert!(
        !later.status.success(),
        "retired Work must refuse a new preparation"
    );
    fs::write(&intents, &retained).unwrap();
    let payload = json!({"hook_event_name":"SubagentStart","session_id":ADOPTER,
        "agent_id":"fixture-child","cwd":case.project.root.path()});
    let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&case.project.root)
        .arg("hook-run")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CODEX_THREAD_ID", ADOPTER)
        .env("EXITBIND_SESSION_GOALS_DIR", &case.project.session_store)
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
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "host hook remains fail-open: {output:?}"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("no assignment context was presented"),
        "{text}"
    );
    assert_eq!(
        fs::read(&intents).unwrap(),
        retained,
        "the old intent gained no child authority"
    );
}
