//! Canonical event motifs, projection privacy and actual finding application.
use super::*;

pub(super) fn world(f: &Fixture) -> Value {
    let output = f.call(&["work", "world", &f.work, "--json"]);
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

pub(super) fn motif_active(world: &Value, name: &str) -> bool {
    world["motifs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|motif| motif["name"] == name)
        .unwrap()["active"]
        == true
}

#[test]
fn world_needs_receiving_execution_and_export_excludes_private_markers() {
    let f = Fixture::new("world-receiving");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"private-session-one","hostVersion":"1"}),
    );
    let initial = world(&f);
    assert_eq!(initial["motifs"].as_array().unwrap().len(), 6);
    assert!(!motif_active(&initial, "distant_payphone"));
    assert!(!motif_active(&initial, "same_door"));
    assert!(!motif_active(&initial, "exit_sign"));
    f.ok(
        json!({"action":"bind","expectedRevision":2,"expectedBindingRevision":1,
        "host":"claude","session":"private-session-two","hostVersion":"2"}),
    );
    assert!(!motif_active(&world(&f), "distant_payphone"));
    let text = "host-delivered exact result";
    f.ok(json!({"action":"child","expectedRevision":3,"bindingRevision":2,
        "assignment":"continue","nativeChild":"private-child","resultText":text,"resultSha256":sha(text)}));
    let before = f.history();
    let resumed = world(&f);
    assert!(motif_active(&resumed, "distant_payphone"));
    assert!(motif_active(&resumed, "same_door"));
    assert!(!motif_active(&resumed, "passing_black_cat"));
    assert!(!motif_active(&resumed, "exit_sign"));
    assert_eq!(world(&f), resumed);
    assert_eq!(f.history(), before);
    let exported = f.call(&["work", "world", &f.work, "--export", "--json"]);
    assert!(exported.status.success());
    let text = String::from_utf8(exported.stdout).unwrap();
    assert!(!text.contains(&f.work));
    assert!(!text.contains("private"));
    assert!(!text.contains("host-delivered"));
    let plain = f.call(&["work", "world", &f.work, "--plain", "--reduced-motion"]);
    assert!(plain.status.success());
    assert!(String::from_utf8(plain.stdout)
        .unwrap()
        .contains("[+] A distant payphone"));
    assert_eq!(f.history(), before);
}

#[test]
fn finding_retrieval_is_not_reuse_and_changed_inputs_suppress_signpost() {
    let f = Fixture::new("world-reuse");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"1"}),
    );
    f.return_stage("scoped", "Scoped regression");
    f.return_stage("completed", "Completed regression");
    let checked = f.call(&["work", "check", &f.work]);
    assert!(checked.status.success(), "{checked:?}");
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let finding = checked["event"]["eventSha256"].clone();
    let current = f.view();
    f.ok(
        json!({"action":"intent","expectedRevision":2,"bindingRevision":1,
        "id":"repair","parametersSha256":sha("repair"),"description":"apply retained finding"}),
    );
    f.ok(
        json!({"action":"diagnose","expectedRevision":3,"bindingRevision":1,
        "operationId":"repair","class":"diagnosed_cause","text":"Retained cause",
        "evidenceEventSha256":finding,"conditionsSha256":current["currentConditionsSha256"],
        "invalidateWhen":"tested inputs change"}),
    );
    assert!(!motif_active(&world(&f), "poster_becomes_signpost"));
    let before = f.history();
    let rejected = f.record(
        json!({"action":"reuse","expectedRevision":4,"bindingRevision":1,
        "operationId":"repair","diagnosisRevision":4,"resultEventSha256":finding,
        "conditionsSha256":current["currentConditionsSha256"]}),
    );
    assert!(!rejected.status.success());
    assert_eq!(f.history(), before);
    let later = f.call(&["work", "check", &f.work]);
    assert!(later.status.success(), "{later:?}");
    let later: Value = serde_json::from_slice(&later.stdout).unwrap();
    f.ok(json!({"action":"reuse","expectedRevision":4,"bindingRevision":1,
        "operationId":"repair","diagnosisRevision":4,"resultEventSha256":later["event"]["eventSha256"],
        "conditionsSha256":current["currentConditionsSha256"]}));
    assert_eq!(f.view()["reuses"][0]["applicableCurrent"], true);
    assert!(motif_active(&world(&f), "poster_becomes_signpost"));
    assert!(!motif_active(&world(&f), "exit_sign"));
    fs::write(f.root.join("changed-input.txt"), "new input").unwrap();
    assert_eq!(f.view()["reuses"][0]["applicableCurrent"], false);
    assert!(!motif_active(&world(&f), "poster_becomes_signpost"));
}
