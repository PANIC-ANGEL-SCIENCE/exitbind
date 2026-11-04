use super::*;

fn granted(
    label: &str,
) -> (
    crate::test_root::TempRoot,
    Loaded,
    String,
    RunSnapshot,
    Value,
) {
    let (root, loaded, work) = fixture(label, "true");
    scope_current_lead(&loaded, &work);
    let ledger = crate::work::resolve(&loaded, &work).unwrap();
    let snapshot = RunSnapshot::capture(&loaded, &ledger).unwrap();
    let next = crate::work::next_from(&loaded, &work, &snapshot).unwrap();
    crate::work::permit_response::permit(
        &loaded,
        &work,
        next["assignment"].as_str().unwrap(),
        "Edit the fixture",
        Some(label),
    )
    .unwrap();
    let snapshot = RunSnapshot::capture(&loaded, &ledger).unwrap();
    let next = crate::work::next_from(&loaded, &work, &snapshot).unwrap();
    assert!(crate::work::action_forms::current_worker_grant(&next["packet"]).is_some());
    (root, loaded, work, snapshot, next)
}

#[test]
fn held_results_added_after_next_suppress_the_offer_without_changing_binding() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    for count in [1, 2] {
        let label = format!("held-grant-offer-{count}");
        let (_root, loaded, work, snapshot, next) = granted(&label);
        let (before, offer) = read_with_grant_offer(&loaded, &work, &snapshot, &next).unwrap();
        let offer = offer.unwrap();
        assert_eq!(before["actionForms"]["currentGrant"], offer.form);
        let assignment = next["assignment"].as_str().unwrap();
        for index in 0..count {
            crate::work::held::store(
                &loaded,
                &work,
                assignment,
                format!("Retained result {index}").as_bytes(),
            )
            .unwrap();
        }
        let fresh = crate::work::next_from(&loaded, &work, &snapshot).unwrap();
        assert_eq!(fresh["current"]["binding"], next["current"]["binding"]);
        // The caller deliberately supplies its earlier next. Assembly must
        // observe held facts again before selecting and returning the offer.
        let (after, offer) = read_with_grant_offer(&loaded, &work, &snapshot, &next).unwrap();
        assert_eq!(after["binding"], before["binding"]);
        assert!(offer.is_none());
        assert!(after["actionForms"]["currentGrant"].is_null());
        assert!(after["actionForms"]["beforeEditing"].is_null());
        let (_, _, native_offer) =
            crate::work::details::required_assignment_with_check(&loaded, &work, &next).unwrap();
        assert!(native_offer.is_none());
    }
}

#[test]
fn canonical_form_owner_only_offers_current_eligible_unsuppressed_grants() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    let (_root, loaded, work, _snapshot, original) = granted("canonical-grant-offer");
    let binding = original["current"]["binding"].as_str().unwrap();
    let forms = |next: &Value| {
        crate::work::action_forms::full_with_grant_offer(&loaded, &work, next, binding)
    };
    for phase in ["ready", "replan_required", "evidence_required"] {
        let mut next = original.clone();
        next["packet"]["context"]["loop"]["state"] = json!(phase);
        let (forms, offer) = forms(&next);
        assert_eq!(forms["currentGrant"], offer.unwrap().form, "{phase}");
    }
    for change in [
        "run",
        "attempt",
        "subject",
        "event",
        "lineage",
        "state",
        "held",
        "heldResults",
        "reviewer",
        "quarantine",
    ] {
        let mut next = original.clone();
        match change {
            "run" => {
                next["packet"]["context"]["loop"]["currentMutation"]["runId"] = json!("old-run")
            }
            "attempt" => next["packet"]["context"]["loop"]["currentMutation"]["attempt"] = json!(0),
            "subject" => {
                next["packet"]["context"]["loop"]["currentMutation"]["subjectSha256"] =
                    json!("old-subject")
            }
            "event" => next["packet"]["context"]["loop"]["currentGrantEventSha256"] = Value::Null,
            "lineage" => {
                next["packet"]["context"]["loop"]["currentMutation"]["carryLineage"] = json!(false)
            }
            "state" => next["packet"]["context"]["loop"]["state"] = json!("blocked"),
            "held" => next["held"] = json!({"reference":"retained"}),
            "heldResults" => next["heldResults"] = json!([{"reference":"retained"}]),
            "reviewer" => next["role"] = json!("reviewer"),
            "quarantine" => next["packet"]["quarantineVerification"] = json!({}),
            _ => unreachable!(),
        }
        let (forms, offer) = forms(&next);
        assert!(offer.is_none(), "{change}");
        assert!(forms["currentGrant"].is_null(), "{change}");
    }
}
