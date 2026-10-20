//! R53 Step 3: the R52 slow path, reproduced with a synthetic native session
//! this test owns. A marked successor's predecessor is a journey member of the
//! session Goal, and the Goal also holds the 31 other members R52's leaked test
//! Works left behind, most of whose roots are gone. Reads and mutations of the
//! successor must answer at once, and reads must not change the ledger.
use super::*;
use std::time::{Duration, Instant};

const SESSION: &str = "r53-successor-timing-session";
const READ_BOUND: Duration = Duration::from_secs(5);
const MUTATION_BOUND: Duration = Duration::from_secs(30);

fn timed(
    project: &Project,
    label: &str,
    args: &[&str],
    input: Option<&str>,
    bound: Duration,
) -> Value {
    let started = Instant::now();
    let value = project.value(args, input);
    let elapsed = started.elapsed();
    eprintln!("r53-timing {label}: {:.3}s", elapsed.as_secs_f64());
    assert!(
        elapsed <= bound,
        "{label} took {elapsed:?}, bound {bound:?}"
    );
    value
}

/// Other Works of the same session in their own projects, as R52's leaked
/// test runs left them: a full member list whose roots mostly no longer exist.
fn pollute(project: &Project, count: usize) {
    let (_, store) = project.native_session.as_ref().unwrap();
    for index in 0..count {
        let other = project.root.with_file_name(format!(
            "{}-member-{index}",
            project.root.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&other).unwrap();
        support::git_topology::repository(&other);
        fs::write(other.join("check.sh"), "true\n").unwrap();
        let init = support::run(
            support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
                .args(["init", "--mode", "portable", "--skip-skills", "--root"])
                .arg(&other),
        );
        assert!(init.status.success(), "{init:?}");
        let begin = support::run(
            support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
                .current_dir(&other)
                .args([
                    "work",
                    "begin",
                    "change",
                    "--goal",
                    &format!("leaked test Work {index}"),
                ])
                .args(["--check-command", "sh check.sh", "--config"])
                .arg(other.join("exitbind.json"))
                .env("EXITBIND_NO_UPDATE_CHECK", "1")
                .env("CODEX_THREAD_ID", SESSION)
                .env("EXITBIND_SESSION_GOALS_DIR", store),
        );
        assert!(begin.status.success(), "{begin:?}");
        if index % 6 != 0 {
            fs::remove_dir_all(&other).unwrap();
        }
    }
}

fn remove_pollution(project: &Project, count: usize) {
    for index in 0..count {
        let other = project.root.with_file_name(format!(
            "{}-member-{index}",
            project.root.file_name().unwrap().to_string_lossy()
        ));
        let _ = fs::remove_dir_all(other);
    }
}

#[test]
fn successor_of_a_journey_member_answers_at_once_and_reads_leave_the_ledger_unchanged() {
    let p = Project::new("successor-timing").with_native_session(SESSION);
    p.file("check.sh", "set -eu\ntest -f request.txt\n");
    p.file("request.txt", "the declared check input\n");
    p.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "timing-goal",
            "--goal",
            "Reproduce the R52 successor timing",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
        ],
        None,
    );
    pollute(&p, 31);
    let task_goal = "Check the declared timing requirements";
    let old_work = p.begin_ordinary(task_goal, "sh check.sh", &[]);
    let next = p.value(&["work", "next", &old_work, "--json", "--full"], None)["next"].clone();
    p.return_stage_with_config(&old_work, &next, "scoped", &p.root.join("exitbind.json"));
    let next = p.value(&["work", "next", &old_work, "--json", "--full"], None)["next"].clone();
    assert_eq!(next["role"], "worker");
    p.value(
        &[
            "work",
            "return",
            &old_work,
            next["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some("Blocked fixture worker; the predecessor stays a journey member.\n"),
    );
    assert_eq!(p.status()["journeyMembers"].as_array().unwrap().len(), 32);

    let old_ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        old_work.strip_prefix("smw_").unwrap()
    );
    let new_work = format!("smw_{}", "c".repeat(64));
    let new_ledger = format!(".exitbind/runs/work-{}.jsonl", "c".repeat(64));
    p.value(
        &[
            "run",
            "supersede",
            &old_ledger,
            "--workflow",
            "change",
            "--goal",
            task_goal,
            "--ledger",
            &new_ledger,
            "--review-policy",
            "required",
            "--json",
        ],
        None,
    );
    let ledger = p.root.join(&new_ledger);
    let mut rework_done = false;
    for _ in 0..16 {
        let before = fs::read(&ledger).unwrap();
        timed(
            &p,
            "work detail",
            &["work", "detail", &new_work, "--json"],
            None,
            READ_BOUND,
        );
        timed(
            &p,
            "work detail --view action",
            &["work", "detail", &new_work, "--view", "action", "--json"],
            None,
            READ_BOUND,
        );
        let next = timed(
            &p,
            "work next",
            &["work", "next", &new_work, "--json", "--full"],
            None,
            READ_BOUND,
        )["next"]
            .clone();
        p.value(&["goal", "status", "--json"], None);
        assert_eq!(
            fs::read(&ledger).unwrap(),
            before,
            "a read changed the ledger"
        );
        let assignment = next["assignment"].as_str().unwrap_or_default();
        let returned = |outcome: &str| {
            timed(
                &p,
                &format!("work return {outcome}"),
                &[
                    "work",
                    "return",
                    &new_work,
                    assignment,
                    "--outcome",
                    outcome,
                ],
                Some("Deterministic fixture decision.\n"),
                MUTATION_BOUND,
            );
        };
        match (next["action"].as_str().unwrap(), next["role"].as_str()) {
            ("lead_decision", _) => {
                let outcomes = next["outcomes"].as_array().cloned().unwrap_or_default();
                if outcomes.iter().any(|item| item == "scoped") {
                    returned("scoped");
                } else if outcomes.iter().any(|item| item == "accepted") {
                    returned("accepted");
                } else {
                    timed(
                        &p,
                        "work disposition repair",
                        &[
                            "work",
                            "disposition",
                            &new_work,
                            assignment,
                            "--decision",
                            "repair",
                            "--reason",
                            "fixture repair",
                            "--repair-boundary",
                            "fixture boundary",
                            "--regression",
                            "fixture regression",
                        ],
                        None,
                        MUTATION_BOUND,
                    );
                }
            }
            ("spawn", Some("worker")) => {
                p.value(
                    &[
                        "work",
                        "permit",
                        &new_work,
                        assignment,
                        "--operation",
                        "Fixture edit",
                    ],
                    None,
                );
                returned("completed");
            }
            ("spawn", _) if !rework_done => {
                rework_done = true;
                returned("rework");
            }
            ("spawn", _) => returned("approved"),
            ("check", _) => {
                timed(
                    &p,
                    "work check",
                    &["work", "check", &new_work],
                    None,
                    MUTATION_BOUND,
                );
            }
            ("done", _) => {
                assert!(
                    rework_done,
                    "the reproduction must include the reviewer rework path"
                );
                remove_pollution(&p, 31);
                return;
            }
            (action, _) => panic!("unexpected {action}: {next}"),
        }
    }
    panic!("successor fixture did not finish");
}

/// The per-process ledger memo and the bounded lineage walk must not weaken
/// acceptance: changed product bytes are found even when size and mtime match.
#[test]
fn acceptance_rehashes_changed_bytes_under_an_unchanged_mtime_and_size() {
    let p = Project::new("acceptance-rehash");
    p.file("check.sh", "set -eu\ngrep -q . input.txt\n");
    p.file("input.txt", "alpha\n");
    let work = p.begin_ordinary("Accept only the checked bytes", "sh check.sh", &[]);
    let config = p.root.join("exitbind.json");
    for _ in 0..12 {
        let next = p.value(&["work", "next", &work, "--json", "--full"], None)["next"].clone();
        let outcomes = next["outcomes"].as_array().cloned().unwrap_or_default();
        match next["action"].as_str().unwrap() {
            "lead_decision" if outcomes.iter().any(|item| item == "scoped") => {
                p.return_stage_with_config(&work, &next, "scoped", &config)
            }
            "lead_decision" => {
                assert!(outcomes.iter().any(|item| item == "accepted"), "{next}");
                let input = p.root.join("input.txt");
                let metadata = fs::metadata(&input).unwrap();
                fs::write(&input, "omega\n").unwrap();
                fs::File::options()
                    .write(true)
                    .open(&input)
                    .unwrap()
                    .set_modified(metadata.modified().unwrap())
                    .unwrap();
                let changed = fs::metadata(&input).unwrap();
                assert_eq!(
                    (changed.len(), changed.modified().unwrap()),
                    (metadata.len(), metadata.modified().unwrap())
                );
                let refused = p.call(
                    &[
                        "work",
                        "return",
                        &work,
                        next["assignment"].as_str().unwrap(),
                        "--outcome",
                        "accepted",
                    ],
                    Some("Accept.\n"),
                );
                assert!(
                    !refused.status.success(),
                    "acceptance used stale checked bytes"
                );
                let current =
                    p.value(&["work", "next", &work, "--json", "--full"], None)["next"].clone();
                assert!(
                    !current["outcomes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item == "accepted"),
                    "{current}"
                );
                return;
            }
            "spawn" if next["role"] == "worker" => {
                p.value(
                    &[
                        "work",
                        "permit",
                        &work,
                        next["assignment"].as_str().unwrap(),
                        "--operation",
                        "Edit",
                    ],
                    None,
                );
                p.return_stage_with_config(&work, &next, "completed", &config);
            }
            "spawn" => p.return_stage_with_config(&work, &next, "approved", &config),
            "check" => {
                let output = p.call(&["work", "check", &work], None);
                assert!(output.status.success(), "{output:?}");
            }
            action => panic!("unexpected {action}: {next}"),
        }
    }
    panic!("acceptance fixture did not reach the Lead decision");
}

/// Every observation response names the build and host class that produced it.
#[test]
fn observation_json_carries_the_producer_envelope() {
    let p = Project::new("observation-envelope").with_native_session("r53-envelope-session");
    p.file("check.sh", "true\n");
    incorporate_native_generic_goal(&p, "envelope-goal", "Observe the producer envelope");
    let work = p.begin_ordinary("Observe the producer envelope", "sh check.sh", &[]);
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let observations = [
        p.value(&["work", "detail", &work, "--json"], None),
        p.value(
            &["work", "detail", &work, "--view", "action", "--json"],
            None,
        ),
        p.value(&["work", "next", &work, "--json"], None),
        p.value(&["work", "resume", "--json"], None),
        p.value(&["goal", "status", "--json"], None),
        p.value(&["run", "status", &ledger, "--json"], None),
        p.value(&["session", "recovery", "show", "--json"], None),
    ];
    let host_class = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let executable = {
        use sha2::Digest;
        format!(
            "{:x}",
            sha2::Sha256::digest(fs::read(env!("CARGO_BIN_EXE_exitbind")).unwrap())
        )
    };
    for observation in observations {
        let producer = &observation["producer"];
        assert_eq!(
            producer["version"],
            env!("CARGO_PKG_VERSION"),
            "{observation}"
        );
        assert!(
            producer["commit"].is_null()
                || producer["commit"]
                    .as_str()
                    .is_some_and(|commit| commit.len() == 40)
        );
        assert_eq!(producer["executableSha256"], executable.as_str());
        assert_eq!(producer["hostClass"], host_class.as_str());
        assert_eq!(producer.as_object().unwrap().len(), 4);
    }
}
