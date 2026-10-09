//! Current worker recovery forms over the existing governor and held owners.
use super::*;

pub(super) fn compact(result: &mut Value) {
    if let Some(recovery) = result.get_mut("recovery").and_then(Value::as_object_mut) {
        recovery.remove("command");
        recovery.remove("heldResults");
        recovery.insert("requiresDetail".into(), json!(true));
    }
}

pub(super) fn attach(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    binding: &str,
    next: &Value,
    result: &mut Value,
) {
    if next["action"] != "spawn" || next["role"] != "worker" {
        return;
    }
    let held: Vec<&Value> = next
        .get("held")
        .into_iter()
        .chain(next["heldResults"].as_array().into_iter().flatten())
        .collect();
    let state = next["packet"]["context"]["loop"]["state"].as_str();
    let form = match state {
        Some("replan_required") => Some(choice(
            loaded,
            vec![
                "work".into(),
                "replan".into(),
                work.into(),
                assignment.into(),
                "--hypothesis".into(),
                "<HYPOTHESIS>".into(),
                "--scope-decision".into(),
                "<SCOPE_DECISION>".into(),
                "--evidence-request".into(),
                "<EVIDENCE_REQUEST>".into(),
            ],
            "replan",
            &["HYPOTHESIS", "SCOPE_DECISION", "EVIDENCE_REQUEST"],
            no_input(),
        )),
        Some("evidence_required") => Some(choice(
            loaded,
            vec![
                "work".into(),
                "evidence".into(),
                work.into(),
                assignment.into(),
                "--artifact-root".into(),
                "<ARTIFACT_ROOT>".into(),
                "--artifact".into(),
                "<ARTIFACT_PATH>".into(),
            ],
            "evidence",
            &["ARTIFACT_ROOT", "ARTIFACT_PATH"],
            no_input(),
        )),
        _ => None,
    };
    if let Some(form) = form {
        result["recovery"] = json!({"state":state,"command":form["command"],
            "meaning":"supply meaningful current recovery evidence or a Lead re-plan, then refresh detail; spent authority and held bytes remain unchanged"});
        if !held.is_empty() {
            result["recovery"]["heldResults"] =
                json!(held.iter().map(|item| json!({
                "reference": item["reference"], "sha256": item["sha256"], "bytes": item["bytes"]
            })).collect::<Vec<_>>());
            result.as_object_mut().unwrap().remove("currentGrant");
        }
        result["choices"] = json!([form]);
        result["leadChoiceRequired"] = json!(true);
        result.as_object_mut().unwrap().remove("beforeEditing");
        return;
    }
    if state == Some("blocked") {
        result["recovery"] = json!({"state":"blocked","reason":"governor blocked; inspect the owner boundary; no retry or re-plan grants authority"});
        result["choices"] = json!([]);
        result.as_object_mut().unwrap().remove("beforeEditing");
        return;
    }
    if held.is_empty() {
        return;
    }
    let forms: Vec<Value> = held.into_iter().filter_map(|held| {
        let reference = held["reference"].as_str()?;
        Some(json!({"reference":reference,"sha256":held["sha256"],"bytes":held["bytes"],
            "command":command(loaded,vec!["work".into(),"return".into(),work.into(),assignment.into(),
                "--current-binding".into(),binding.into(),"--outcome".into(),"completed".into(),
                "--result-ref".into(),reference.into(),"--reason".into(),"<REASON>".into()]),
            "placeholders":["REASON"],"stdin":no_input()}))
    }).collect();
    result["recovery"] = json!({"state":"retained_result","heldResults":forms,
        "meaning":"return the selected exact retained bytes once, then perform the fresh applicable check and review"});
    result.as_object_mut().unwrap().remove("currentGrant");
    result.as_object_mut().unwrap().remove("beforeEditing");
}
