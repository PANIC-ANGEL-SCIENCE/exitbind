//! R53: an assignment receives the owner-declared guidance body for its phase
//! whole, with a count, or an exact role-scoped read route; never another
//! role's body. The Lead's detail names delegation for an open assignment.
use super::*;

#[path = "action_view.rs"]
mod action_view;

const TRIAGE: &str = "# Failing test triage\n\nReproduce only the failed tests, with the target's features.\nUnset host session variables first: \"EXITBIND_NATIVE_SESSION_ID\".\nKlassifiziere eine Ursache — before editing.\n";
const REVIEW_NOTES: &str = "# Reviewer-only notes\n\nPRIVATE-REVIEWER-BODY\n";

fn worker_assignment(fixture: &Fixture) -> (String, String) {
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Deliver phase guidance",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap().to_owned();
    let lead = started["next"]["assignment"].as_str().unwrap();
    let returned = fixture.call(
        &["work", "return", &work, lead, "--outcome", "scoped"],
        b"scope",
    );
    assert!(returned.status.success(), "{}", text(&returned));
    let next = fixture.value(&["work", "next", &work, "--json", "--full"], b"");
    assert_eq!(next["next"]["role"], "worker");
    let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
    (work, assignment)
}

fn declare(fixture: &Fixture, agent: &str, items: Value) {
    let path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["agents"][agent]["phaseGuidance"] = items;
    fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

#[test]
fn worker_detail_delivers_its_phase_body_whole_with_a_count_and_the_lead_sees_delegation() {
    let fixture = Fixture::new("phase-guidance-worker");
    let skill = fixture
        .root
        .join("owner-skills/failing-test-triage/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, TRIAGE).unwrap();
    fs::write(fixture.root.join("review-notes.md"), REVIEW_NOTES).unwrap();
    declare(
        &fixture,
        "worker",
        json!([{"id":"failing-test-triage","phase":"test","path":skill.to_str().unwrap()}]),
    );
    declare(
        &fixture,
        "reviewer",
        json!([{"id":"review-notes","phase":"review","path":"review-notes.md"}]),
    );
    let (work, assignment) = worker_assignment(&fixture);

    let detail = fixture.value(&["work", "detail", &work, "--json"], b"");
    let guidance = &detail["recipientContext"]["guidance"];
    let item = &guidance["items"][0];
    assert_eq!(
        item["content"], TRIAGE,
        "whole body, byte for byte: {guidance}"
    );
    assert_eq!(item["reference"]["kind"], "phase_guidance");
    assert_eq!(item["reference"]["phase"], "test");
    assert_eq!(item["reference"]["byteLength"], TRIAGE.len());
    assert_eq!(guidance["phaseGuidance"]["selectedCount"], 1);
    assert_eq!(guidance["phaseGuidance"]["overflowCount"], 0);
    assert_eq!(
        guidance["phaseGuidance"]["text"],
        "1 phase guidance body delivered whole, 0 by read route"
    );
    assert_eq!(
        guidance["phaseGuidance"]["phases"],
        json!(["implementation", "test"])
    );
    let raw = serde_json::to_string(&detail).unwrap();
    assert!(
        !raw.contains("PRIVATE-REVIEWER-BODY"),
        "another role's body leaked"
    );

    // The Lead reading the same detail is told to delegate, by name.
    let expected =
        format!("next action: delegate to worker agent 'worker' (assignment {assignment})");
    assert_eq!(
        detail["effectiveAction"]["delegation"]["text"],
        expected.as_str()
    );
    assert_eq!(
        detail["effectiveAction"]["delegation"]["nextAction"],
        "delegate"
    );
    assert_eq!(
        detail["effectiveAction"]["state"], "action",
        "a sign, not a refusal"
    );
    let action = fixture.call(&["work", "detail", &work, "--view", "action"], b"");
    assert!(action.status.success(), "{}", text(&action));
    let readable = String::from_utf8(action.stdout).unwrap();
    assert!(readable.contains(&expected), "{readable}");
    assert!(
        !readable.trim_start().starts_with('{'),
        "plain text, not JSON: {readable}"
    );
}

#[test]
fn an_over_budget_phase_body_has_an_exact_role_scoped_read_route() {
    let fixture = Fixture::new("phase-guidance-overflow");
    let large = format!(
        "# Large body\n{}",
        "complete line of guidance\n".repeat(480)
    );
    assert!(large.len() > 10 * 1024 && large.len() < 16 * 1024);
    fs::write(fixture.root.join("large.md"), &large).unwrap();
    declare(
        &fixture,
        "worker",
        json!([{"id":"large-body","phase":"implementation","path":"large.md"}]),
    );
    let (work, _) = worker_assignment(&fixture);
    let detail = fixture.value(&["work", "detail", &work, "--json"], b"");
    let guidance = &detail["recipientContext"]["guidance"];
    assert_eq!(guidance["phaseGuidance"]["selectedCount"], 0);
    assert_eq!(guidance["phaseGuidance"]["overflowCount"], 1);
    let overflow = &guidance["overflow"][0];
    assert_eq!(overflow["reason"], "recipient_budget");
    assert!(!serde_json::to_string(guidance)
        .unwrap()
        .contains("complete line of guidance\\ncomplete"));
    let command = overflow["readRoute"]["command"].as_array().unwrap();
    let argv: Vec<&str> = command.iter().map(|item| item.as_str().unwrap()).collect();
    let read = super::support::git_topology::command(argv[0])
        .args(&argv[1..])
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert!(read.status.success(), "{}", text(&read));
    let read: Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(
        read["content"],
        large.as_str(),
        "the route returns the whole body"
    );

    let agent = argv.iter().position(|item| *item == "--agent").unwrap() + 1;
    let mut other = argv.clone();
    other[agent] = "reviewer";
    let refused = super::support::git_topology::command(other[0])
        .args(&other[1..])
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert!(
        !refused.status.success(),
        "another role must not read this body"
    );
    assert!(!String::from_utf8_lossy(&refused.stdout).contains("complete line of guidance"));
}

#[test]
fn an_agent_without_phase_guidance_keeps_the_previous_guidance_shape() {
    let fixture = Fixture::new("phase-guidance-legacy");
    let (work, _) = worker_assignment(&fixture);
    let detail = fixture.value(&["work", "detail", &work, "--json"], b"");
    assert!(detail["recipientContext"]["guidance"]
        .get("phaseGuidance")
        .is_none());
}
