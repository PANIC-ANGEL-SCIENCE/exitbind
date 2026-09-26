use super::*;

fn fresh() -> Value {
    let mut packet = json!({
        "version": 2, "work": "w", "workflow": "change", "goal": "g", "historical": false,
        "snapshot": {"eventCount": 3, "headEventSha256": "h", "inputsSha256": "i"},
        "currentSubject": {"sha256": "s"}, "basis": null, "reviewPolicy": null,
        "alreadyEstablished": [],
        "stillValid": [{"evidence": "current_check"}], "remaining": [{"obligation": "review"}],
        "next": "spawn", "doNotRepeat": ["passed_check"], "invalidation": {"rule": "r"}
    });
    packet["humanHelp"] = json!({"whatHappened": "x"});
    packet
}

#[test]
fn judge_refuses_every_non_current_packet_and_accepts_the_matching_one() {
    let fresh = fresh();
    assert_eq!(judge("w", &fresh, &fresh).0, "usable");
    type Mutation = fn(&mut Value);
    let cases: [(&str, Mutation, &str, &str); 13] = [
        (
            "w",
            |p| p["version"] = json!(1),
            CANNOT_ESTABLISH,
            "unsupported_packet_version",
        ),
        (
            "w",
            |p| p["requires"] = json!(["future"]),
            CANNOT_ESTABLISH,
            "unsupported_required_semantics",
        ),
        (
            "w",
            |p| p["stillValid"] = json!("x"),
            CANNOT_ESTABLISH,
            "malformed_packet",
        ),
        (
            "w",
            |p| {
                p.as_object_mut().unwrap().remove("remaining");
            },
            CANNOT_ESTABLISH,
            "malformed_packet",
        ),
        ("other", |_| {}, CANNOT_ESTABLISH, "different_work"),
        (
            "w",
            |p| p["snapshot"]["eventCount"] = json!(2),
            "refresh_required",
            "ledger_advanced",
        ),
        (
            "w",
            |p| p["snapshot"]["inputsSha256"] = json!("old"),
            "refresh_required",
            "tested_inputs_changed",
        ),
        (
            "w",
            |p| p["doNotRepeat"] = json!(["passed_check", "review"]),
            "refresh_required",
            "claims_disagree_with_state",
        ),
        (
            "w",
            |p| p["goal"] = json!("weaker goal"),
            "refresh_required",
            "claims_disagree_with_state",
        ),
        (
            "w",
            |p| p["currentSubject"] = json!({"sha256": "other"}),
            "refresh_required",
            "claims_disagree_with_state",
        ),
        (
            "w",
            |p| p["remaining"] = json!([]),
            "refresh_required",
            "claims_disagree_with_state",
        ),
        (
            "w",
            |p| p["next"] = json!("lead_decision"),
            "refresh_required",
            "claims_disagree_with_state",
        ),
        (
            "w",
            |p| p["historical"] = json!(true),
            "refresh_required",
            "claims_disagree_with_state",
        ),
    ];
    for (work, mutate, result, reason) in cases {
        let mut packet = fresh.clone();
        mutate(&mut packet);
        let judged = judge(work, &packet, &fresh);
        assert_eq!((judged.0, judged.1), (result, reason), "{reason}");
    }
    let mut unbound = fresh.clone();
    unbound["snapshot"]["inputsSha256"] = Value::Null;
    assert_eq!(judge("w", &unbound, &unbound).1, "tested_inputs_not_bound");
    let mut terminal = fresh.clone();
    terminal["historical"] = json!(true);
    assert_eq!(judge("w", &terminal, &terminal).0, "not_continuable");
    let mut display = fresh.clone();
    display["humanHelp"] = json!({"whatHappened": "edited display text"});
    display["futureHint"] = json!(true);
    assert_eq!(judge("w", &display, &fresh).0, "usable");
}

const CANNOT_ESTABLISH: &str = "cannot_establish_applicability";
