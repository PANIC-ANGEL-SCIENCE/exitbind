use super::*;

fn two_completions() -> Value {
    let mut result = sections();
    let sha = result["evidence"]["items"][0]["artifact"]["sha256"].clone();
    let packet = &mut result["assignment"]["assignment"];
    packet["role"] = json!("reviewer");
    packet["attempt"] = json!(2);
    packet["context"]["role"] = json!("reviewer");
    packet["context"]["scope"]["attempt"] = json!(2);
    packet["context"]["next"]["role"] = json!("reviewer");
    packet["context"]["next"]["owner"] = json!("reviewer");
    let identity = json!({"runId":"run-a", "subjectSha256":"subject-a", "attempt":2,
        "checkpoint":2, "inputSha256":"input-a", "unit":"worker", "operation":"completed_submission"});
    let mut earlier = identity.clone();
    earlier["subjectSha256"] = json!("previous-subject");
    earlier["attempt"] = json!(1);
    earlier["checkpoint"] = json!(1);
    let mut current = identity.clone();
    current["eventSha256"] = json!("c".repeat(64));
    current["carryLineage"] = json!(true);
    packet["context"]["loop"]["currentMutation"] = current;
    packet["context"]["loop"]["seenMutations"] = json!([
        {"digest":"a".repeat(64),"identity":earlier,"evidence":"e".repeat(64)},
        {"digest":"b".repeat(64),"identity":identity,"evidence":sha}]);
    result
}

#[test]
fn earlier_carried_completions_remain_history_without_current_evidence_credit() {
    let original = two_completions();
    let project =
        |value: &Value| project_sections("work-a", "binding-a", value, &[], &Value::Null, None);
    let actor = project(&original).unwrap();
    assert_eq!(
        actor["governor"]["seenMutations"],
        original["assignment"]["assignment"]["context"]["loop"]["seenMutations"]
    );
    assert_eq!(actor["currentAction"], original["currentAction"]);
    assert_eq!(actor["evidence"]["items"].as_array().unwrap().len(), 1);
    let mut missing_current = original.clone();
    missing_current["evidence"]["items"] = json!([]);
    assert!(project(&missing_current).is_err());

    let mut changes = Vec::new();
    for field in ["attempt", "checkpoint"] {
        for value in [json!(2), json!(3), Value::Null, json!("1")] {
            let mut changed = original.clone();
            changed["assignment"]["assignment"]["context"]["loop"]["seenMutations"][0]
                ["identity"][field] = value;
            changes.push(changed);
        }
    }
    for target in ["currentMutation", "previous"] {
        for field in ["runId", "unit", "operation"] {
            let mut changed = original.clone();
            let governor = &mut changed["assignment"]["assignment"]["context"]["loop"];
            let identity = if target == "previous" {
                &mut governor["seenMutations"][0]["identity"]
            } else {
                &mut governor["currentMutation"]
            };
            identity[field] = json!("other");
            changes.push(changed);
        }
    }
    for carry in [json!(false), Value::Null, json!("true")] {
        let mut changed = original.clone();
        changed["assignment"]["assignment"]["context"]["loop"]["currentMutation"]["carryLineage"] =
            carry;
        changes.push(changed);
    }
    let mut missing_carry = original.clone();
    missing_carry["assignment"]["assignment"]["context"]["loop"]["currentMutation"]
        .as_object_mut()
        .unwrap()
        .remove("carryLineage");
    changes.push(missing_carry);
    let mut malformed_anchor = original.clone();
    malformed_anchor["assignment"]["assignment"]["context"]["loop"]["currentMutation"]
        ["eventSha256"] = json!("invalid");
    changes.push(malformed_anchor);
    let mut object = original.clone();
    object["assignment"]["assignment"]["context"]["loop"]["seenMutations"][0]["evidence"] =
        json!({"root":"state", "path":"earlier", "sha256":"e".repeat(64)});
    changes.push(object);
    let mut post_repair = original.clone();
    let packet = &mut post_repair["assignment"]["assignment"];
    packet["attempt"] = json!(3);
    packet["context"]["scope"]["attempt"] = json!(3);
    packet["context"]["loop"]["seenMutations"][1]["evidence"] = json!("f".repeat(64));
    changes.push(post_repair);
    for changed in changes {
        assert!(project(&changed).is_err(), "{changed}");
    }
}

#[test]
fn mutation_history_is_preserved_and_unknown_or_unbound_evidence_refuses() {
    let route = vec!["work".into(), "expand".into(), "bound".into()];
    let mut original = sections();
    let sha = original["evidence"]["items"][0]["artifact"]["sha256"].clone();
    let packet = &mut original["assignment"]["assignment"];
    packet["role"] = json!("reviewer");
    packet["context"]["role"] = json!("reviewer");
    packet["context"]["next"]["role"] = json!("reviewer");
    packet["context"]["next"]["owner"] = json!("reviewer");
    let current = json!({"runId":"run-a", "eventSha256":"b".repeat(64), "carryLineage":true,
        "subjectSha256":"subject-a", "attempt":1, "checkpoint":1, "inputSha256":"input-a",
        "unit":"worker", "operation":"completed_submission"});
    let identity = json!({"runId":"run-a", "subjectSha256":"subject-a", "attempt":1,
        "checkpoint":1, "inputSha256":"input-a", "unit":"worker", "operation":"completed_submission"});
    packet["context"]["loop"]["currentMutation"] = current.clone();
    packet["context"]["loop"]["seenMutations"] = json!([{
        "digest":"a".repeat(64), "identity":identity, "evidence":sha
    }]);
    let project =
        |value: &Value| project_sections("work-a", "binding-a", value, &route, &Value::Null, None);
    let actor = project(&original).unwrap();
    let governor = &original["assignment"]["assignment"]["context"]["loop"];
    assert_eq!(actor["governor"]["currentMutation"], current);
    assert_eq!(
        actor["governor"]["seenMutations"],
        governor["seenMutations"]
    );

    let mut permitted = original.clone();
    let seen = &mut permitted["assignment"]["assignment"]["context"]["loop"]["seenMutations"][0];
    seen["identity"]["unit"] = json!("worker-mutation");
    seen["identity"]["operation"] = json!("Edit the fixture");
    seen["evidence"] = Value::Null;
    let actor = project(&permitted).unwrap();
    assert_eq!(actor["governor"]["currentMutation"], current);
    assert_eq!(
        actor["governor"]["seenMutations"],
        permitted["assignment"]["assignment"]["context"]["loop"]["seenMutations"]
    );
    let mut legacy = original.clone();
    legacy["assignment"]["assignment"]["context"]["loop"]["currentMutation"]["carryLineage"] =
        Value::Null;
    let actor = project(&legacy).unwrap();
    assert!(actor["governor"]["currentMutation"]["carryLineage"].is_null());

    for change in [
        "unbound-evidence",
        "malformed-digest",
        "unknown-record",
        "unknown-identity",
        "malformed-identity",
        "missing-evidence",
        "unknown-current",
        "malformed-current",
    ] {
        let mut altered = original.clone();
        let governor = &mut altered["assignment"]["assignment"]["context"]["loop"];
        match change {
            "unbound-evidence" => governor["seenMutations"][0]["evidence"] = json!("f".repeat(64)),
            "malformed-digest" => governor["seenMutations"][0]["digest"] = json!("not-a-digest"),
            "unknown-record" => governor["seenMutations"][0]["extra"] = json!("do not omit"),
            "unknown-identity" => {
                governor["seenMutations"][0]["identity"]["extra"] = json!("do not omit")
            }
            "malformed-identity" => {
                governor["seenMutations"][0]["identity"]["checkpoint"] = json!("1")
            }
            "missing-evidence" => {
                governor["seenMutations"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("evidence");
            }
            "unknown-current" => governor["currentMutation"]["extra"] = json!("do not omit"),
            "malformed-current" => governor["currentMutation"]["carryLineage"] = json!("yes"),
            _ => unreachable!(),
        }
        assert!(project(&altered).is_err(), "{change}");
    }
    let mut object = original.clone();
    let governor = &mut object["assignment"]["assignment"]["context"]["loop"];
    let record = json!({"root":"product", "path":"evidence.txt", "sha256":sha});
    governor["evidence"] = json!([record]);
    governor["seenMutations"][0]["evidence"] = record;
    assert!(project(&object).is_ok());
    object["assignment"]["assignment"]["context"]["loop"]["seenMutations"][0]["evidence"]
        ["unknown"] = json!("do not omit");
    assert!(project(&object).is_err());
}
