//! Focused correction-boundary tests.

use super::*;
use crate::evidence::hash;

fn goal() -> Value {
    let text = "A recorded requirement.";
    let source =
        json!({"ref":"request.txt","text":text,"sha256":hash::text(text),"coverageConfirmed":true});
    json!({"goalId":"g","closure":{"closed":false},"categories":{},
            "obligations":[],"findings":[],"blockers":[],"decisions":[],"externalActions":[],
            "continuation":{"kind":"goal_requirements","version":2,"projectIdentity":"a".repeat(64),
                "source":source,"requirements":[{"id":"R1","revision":1,"text":text,
                    "source":source,"history":[]}],"mappings":[]}})
}

#[test]
fn every_historical_association_and_pending_admission_refuses_correction() {
    let base = goal();
    assert!(empty_goal(&[base.clone()], "g").is_ok());
    for field in [
        "journeyMembers",
        "journeyIntegrations",
        "journeyAdmissions",
        "workAssociations",
        "sessionAdoptions",
    ] {
        let mut old = base.clone();
        old[field] = json!([{}]);
        assert!(empty_goal(&[old, base.clone()], "g").is_err(), "{field}");
    }
    for field in ["nativeGoalRecovery", "quarantineResiduals"] {
        let mut old = base.clone();
        old[field] = json!({});
        assert!(empty_goal(&[old, base.clone()], "g").is_err(), "{field}");
    }
    let mut old = base.clone();
    old["continuation"]["mappings"] = json!([{"work":"smw_fixture", "active":false,
            "startEventSha256":"b".repeat(64),"requirement":{"id":"R1","revision":1,
                "text":"A recorded requirement.","sourceSha256":hash::text("A recorded requirement.")}}]);
    assert!(empty_goal(&[old, base.clone()], "g").is_err());
    let mut old = base.clone();
    old["continuation"] = json!({"version":1,"work":"smw_fixture"});
    assert!(empty_goal(&[old, base.clone()], "g").is_err());
    let mut old = base.clone();
    old["obligations"] = json!([{"disposition":"accepted","resultRefs":["work:fixture"]}]);
    assert!(empty_goal(&[old, base.clone()], "g").is_err());
    old = base.clone();
    old["decisions"] = json!([{"disposition":"direct","resultRefs":[]}]);
    assert!(empty_goal(&[old, base], "g").is_err());
}
