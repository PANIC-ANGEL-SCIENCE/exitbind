// Tests actor delivery of current grants and completed history.
// History cases do not require a current grant; see `src/host/runtime/actor/tests.rs`.
use super::*;

fn offered() -> (Value, GrantOffer) {
    let mut current = sections();
    let mutation = json!({"runId":"run-a", "eventSha256":"b".repeat(64),
        "carryLineage":true, "subjectSha256":"subject-a", "attempt":1,
        "checkpoint":1, "inputSha256":"input-a", "unit":"worker-mutation",
        "operation":"Edit the fixture"});
    current["assignment"]["assignment"]["context"]["loop"]["currentMutation"] = mutation.clone();
    current["assignment"]["assignment"]["context"]["loop"]["currentGrantEventSha256"] =
        mutation["eventSha256"].clone();
    let form = json!({"governorEventSha256":mutation["eventSha256"],
        "operation":mutation["operation"], "state":"current_for_this_worker_assignment",
        "use":"the exact current grant may support the present mutation; it grants no host permissions"});
    current["currentAction"]["currentGrant"] = form.clone();
    (current, GrantOffer { mutation, form })
}

fn project(current: &Value, offer: Option<&GrantOffer>) -> Result<Value, String> {
    project_sections(
        "work-a",
        "binding-a",
        current,
        &["work".into(), "expand".into(), "bound".into()],
        &Value::Null,
        offer,
    )
}

#[test]
fn completed_submission_history_does_not_require_a_current_grant() {
    let (mut current, offer) = offered();
    let packet = &mut current["assignment"]["assignment"];
    packet["attempt"] = json!(2);
    packet["context"]["scope"]["attempt"] = json!(2);
    packet["context"]["loop"]["currentMutation"]["unit"] = json!("worker");
    packet["context"]["loop"]["currentMutation"]["operation"] = json!("completed_submission");
    packet["context"]["loop"]["currentGrantEventSha256"] = Value::Null;
    current["currentAction"]["currentGrant"] = Value::Null;
    let history = current["assignment"]["assignment"]["context"]["loop"]["currentMutation"].clone();
    let actor = project(&current, None).unwrap();
    assert_eq!(actor["governor"]["currentMutation"], history);
    assert!(actor["currentAction"]["beforeEditing"].is_object());
    current["currentAction"]["currentGrant"] = offer.form;
    assert!(project(&current, None)
        .unwrap_err()
        .contains("grant identity differs"));
}

#[test]
fn current_grant_is_preserved_in_each_eligible_governor_phase() {
    let (original, offer) = offered();
    for phase in ["ready", "replan_required", "evidence_required"] {
        let mut current = original.clone();
        current["assignment"]["assignment"]["context"]["loop"]["state"] = json!(phase);
        let actor = project(&current, Some(&offer)).unwrap();
        assert_eq!(actor["currentAction"]["currentGrant"], offer.form);
        assert_eq!(actor["governor"]["currentMutation"], offer.mutation);
    }
}

#[test]
fn expected_grant_cannot_be_missing_or_altered_in_delivery() {
    let (original, offer) = offered();
    for change in ["missing", "operation", "event", "state", "extra"] {
        let mut current = original.clone();
        let form = &mut current["currentAction"]["currentGrant"];
        match change {
            "missing" => *form = Value::Null,
            "operation" => form["operation"] = json!("Different mutation"),
            "event" => form["governorEventSha256"] = json!("c".repeat(64)),
            "state" => form["state"] = json!("different"),
            "extra" => form["extra"] = json!(true),
            _ => unreachable!(),
        }
        assert!(project(&current, Some(&offer)).is_err(), "{change}");
    }
    assert!(project(&original, None).is_err());
}

#[test]
fn stale_history_is_kept_but_cannot_support_a_claimed_current_grant() {
    let (original, offer) = offered();
    for change in [
        "run", "attempt", "subject", "event", "lineage", "state", "reviewer",
    ] {
        let mut current = original.clone();
        let packet = &mut current["assignment"]["assignment"];
        match change {
            "run" => packet["context"]["loop"]["currentMutation"]["runId"] = json!("old-run"),
            "attempt" => packet["context"]["loop"]["currentMutation"]["attempt"] = json!(0),
            "subject" => {
                packet["context"]["loop"]["currentMutation"]["subjectSha256"] = json!("old-subject")
            }
            "event" => packet["context"]["loop"]["currentGrantEventSha256"] = Value::Null,
            "lineage" => {
                packet["context"]["loop"]["currentMutation"]["carryLineage"] = json!(false)
            }
            "state" => packet["context"]["loop"]["state"] = json!("blocked"),
            "reviewer" => {
                packet["role"] = json!("reviewer");
                packet["context"]["role"] = json!("reviewer");
                packet["context"]["next"]["role"] = json!("reviewer");
                packet["context"]["next"]["owner"] = json!("reviewer");
            }
            _ => unreachable!(),
        }
        assert!(project(&current, Some(&offer)).is_err(), "{change}");
        assert!(project(&current, None).is_err(), "forged {change}");
        current["currentAction"]["currentGrant"] = Value::Null;
        let history =
            current["assignment"]["assignment"]["context"]["loop"]["currentMutation"].clone();
        assert_eq!(
            project(&current, None).unwrap()["governor"]["currentMutation"],
            history
        );
    }
}
