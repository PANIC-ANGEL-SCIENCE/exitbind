//! R55 S3: the governor reads a Lead repair as the new information that
//! admits the worker's return.
use super::*;

/// R55 S3 (FR54-11, the R54 case): attempt 1 returns a contradiction, the
/// Lead records a repair, and the attempt 2 permit exhausts the
/// no-information streak. The Lead's recorded repair is the new information:
/// the worker's view offers the return, the attempt 2 result passes its
/// check on the new commit, and no worker re-plan is recorded.
#[test]
fn a_lead_repair_after_a_contradiction_offers_the_return_without_a_replan() {
    let basis = json!({"version": 1, "constraints": ["counter.txt only"],
        "openZones": ["worker implementation"], "decisiveCases": ["exact value"]})
    .to_string();
    // The R54 Work ran its frozen check at return (pre-return).
    let f = Fixture::new_with(&[
        "--basis",
        &basis,
        "--pre-return-command",
        "test \"$(cat counter.txt)\" = correct",
    ]);
    f.edit("environment blocked");
    let assignment = f.detail()["actionForms"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    f.ok(
        &[
            "work",
            "return",
            &f.work,
            &assignment,
            "--outcome",
            "contradiction",
            "--reason",
            "The check environment was unavailable",
        ],
        b"The check environment was unavailable; the result is not comparable.",
    );
    let d = f.detail();
    let repair = Fixture::chosen(&d, "repair");
    let filled: Vec<String> = repair["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| match x.as_str().unwrap() {
            "<REASON>" => "The environment failure is repaired; redo the edit".into(),
            "<REPAIR_BOUNDARY>" => "counter.txt only".into(),
            "<REGRESSION>" => "the exact-value check passes".into(),
            x => x.into(),
        })
        .collect();
    let p = support::git_topology::command(&filled[0])
        .args(&filled[1..])
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(p.status.success(), "{p:?}");
    f.edit("correct");
    let d = f.detail();
    assert_eq!(
        d["actionForms"]["recovery"]["state"],
        Value::Null,
        "{}",
        d["actionForms"]
    );
    let labels: Vec<&str> = d["actionForms"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"completed"),
        "{labels:?}: {}",
        d["actionForms"]
    );
    assert!(!labels.contains(&"replan"), "{labels:?}");
    f.choose("completed", b"attempt 2 on the new commit");
    f.check(true);
    let replans = f
        .events()
        .iter()
        .filter(|e| {
            e["governorEvent"]["action"] == "replan"
                && e["operation"] != "authorized_repair_recovery_v1"
        })
        .count();
    assert_eq!(replans, 0, "no worker re-plan was needed");
}
