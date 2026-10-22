//! R55 F4a: a recipient's name is not lesson authority in the owner policy
//! correction either; it reads the same list as `memory revalidate`.
use super::{Fixture, LEDGER};
use serde_json::{json, Value};
use std::fs;

#[test]
fn identity_fields_ride_along_an_owner_policy_correction_as_recorded_differences() {
    let f = Fixture::new();
    let before = f.bytes();
    f.corrected();
    let mut c = f.config();
    let old_profile = c["agents"]["worker"]["profile"].clone();
    fs::write(f.root.join("sonic.md"), b"Sonic implements.\n").unwrap();
    c["agents"]["worker"]["profile"] = json!("sonic.md");
    c["agents"]["worker"]["nativeName"] = json!("sonic");
    f.save(&c);
    let template = f.decision();
    let differences = template["differences"].as_array().unwrap();
    for expected in [
        json!({"agent":"worker","field":"profile","before":old_profile,"after":"sonic.md"}),
        json!({"agent":"worker","field":"nativeName","before":null,"after":"sonic"}),
        json!({"agent":"lead","field":"crossContext","before":"none","after":"protocol-only"}),
    ] {
        assert!(differences.contains(&expected), "{expected}: {template}");
    }
    let mut runtime = c.clone();
    runtime["agents"]["worker"]["runtime"] = json!({"model": "other-model"});
    f.save(&runtime);
    assert!(!f.correction(false, None).status.success());
    f.save(&c);
    f.approve();
    let out = f.correction(true, Some("decision.json"));
    assert!(out.status.success(), "{out:?}");
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["status"], "recorded");
    assert!(f.bytes().starts_with(&before));
    f.ok(&["memory", "inspect", LEDGER, "--json"]);
}
