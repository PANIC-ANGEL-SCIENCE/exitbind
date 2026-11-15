use crate::fixture::Project;
use serde_json::Value;

const GOAL: &str = "Deliver the owner's work";
const QUESTION: &str = "Which direction should we take?";

fn open(project: &Project) -> Value {
    project.value(&[
        "goal",
        "incorporate",
        "--project",
        "--goal-id",
        "bridge",
        "--goal",
        GOAL,
        "--decision",
        QUESTION,
    ])
}

fn resolve(
    project: &Project,
    goal_id: &str,
    goal: &str,
    question: &str,
    expected: &str,
) -> std::process::Output {
    project.call(&[
        "goal",
        "incorporate",
        "--project",
        "--goal-id",
        goal_id,
        "--goal",
        goal,
        "--decision",
        question,
        "--disposition",
        "accepted",
        "--result-ref",
        "reply-result",
        "--expected-event-sha256",
        expected,
    ])
}

#[test]
fn expected_open_decision_is_resolved_once_under_the_goal_lock() {
    let project = Project::new("guarded-owner-decision");
    let first = open(&project);
    let hash = first["eventSha256"].as_str().unwrap();
    for (goal_id, goal, question, expected) in [
        ("different", GOAL, QUESTION, hash),
        ("bridge", "Different goal", QUESTION, hash),
        ("bridge", GOAL, "Another question?", hash),
        ("bridge", GOAL, QUESTION, "invalid"),
    ] {
        assert!(!resolve(&project, goal_id, goal, question, expected)
            .status
            .success());
    }
    let accepted = resolve(&project, "bridge", GOAL, QUESTION, hash);
    assert!(accepted.status.success(), "{:?}", accepted);
    let accepted: Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(
        accepted["revision"].as_u64(),
        Some(first["revision"].as_u64().unwrap() + 1)
    );
    assert_eq!(accepted["previousEventSha256"], first["eventSha256"]);
    assert_eq!(accepted["decisions"][0]["disposition"], "accepted");
    assert_eq!(accepted["decisions"][0]["resultRefs"][0], "reply-result");
    let accepted_hash = accepted["eventSha256"].as_str().unwrap();
    assert!(!resolve(&project, "bridge", GOAL, QUESTION, hash)
        .status
        .success());
    // Even the latest head cannot turn an already-resolved decision into a second result.
    assert!(!resolve(&project, "bridge", GOAL, QUESTION, accepted_hash)
        .status
        .success());
}

#[test]
fn intervening_goal_event_and_replaced_goal_refuse_stale_answers() {
    let project = Project::new("guarded-owner-stale");
    let first = open(&project);
    let first_hash = first["eventSha256"].as_str().unwrap();
    let changed = project.value(&[
        "goal",
        "incorporate",
        "--project",
        "--goal-id",
        "bridge",
        "--goal",
        GOAL,
        "--finding",
        "An independent observation",
    ]);
    assert!(!resolve(&project, "bridge", GOAL, QUESTION, first_hash)
        .status
        .success());
    let successor = project.value(&[
        "goal",
        "incorporate",
        "--project",
        "--goal-id",
        "successor",
        "--goal",
        GOAL,
    ]);
    assert_eq!(
        successor["revision"].as_u64(),
        Some(changed["revision"].as_u64().unwrap() + 1)
    );
    assert!(!resolve(
        &project,
        "bridge",
        GOAL,
        QUESTION,
        successor["eventSha256"].as_str().unwrap()
    )
    .status
    .success());
}

#[test]
fn competing_answers_cannot_consume_one_goal_event_twice() {
    let project = Project::new("guarded-owner-concurrent");
    let first = open(&project);
    let expected = first["eventSha256"].as_str().unwrap();
    let barrier = std::sync::Barrier::new(3);
    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(|| {
            barrier.wait();
            resolve(&project, "bridge", GOAL, QUESTION, expected)
        });
        let right = scope.spawn(|| {
            barrier.wait();
            resolve(&project, "bridge", GOAL, QUESTION, expected)
        });
        barrier.wait();
        (left.join().unwrap(), right.join().unwrap())
    });
    assert_ne!(left.status.success(), right.status.success());
    let (accepted, refused) = if left.status.success() {
        (left, right)
    } else {
        (right, left)
    };
    let refusal = String::from_utf8_lossy(&refused.stderr);
    assert!(refusal.contains("no longer current") || refusal.contains("ledger is busy"));
    assert!(!resolve(&project, "bridge", GOAL, QUESTION, expected)
        .status
        .success());
    let accepted: Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(
        accepted["revision"].as_u64(),
        Some(first["revision"].as_u64().unwrap() + 1)
    );
    assert_eq!(accepted["previousEventSha256"], first["eventSha256"]);
}
