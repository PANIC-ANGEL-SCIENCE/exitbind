//! Static, read-only world projection. No artwork cache, provider call, raw
//! task prose or new authority is needed to make canonical events visible.

use crate::config::Loaded;
use serde_json::{json, Value};

pub(crate) fn project(loaded: &Loaded, work: &str, response: &Value) -> Value {
    let continuation = crate::session_goal::continuation_view(loaded, work).ok();
    let goal = crate::session_goal::read(&loaded.state_root).ok().flatten();
    let ready = goal
        .as_ref()
        .filter(|goal| goal["goalId"] == work)
        .and_then(|goal| crate::session_goal::presentation_for_loaded(loaded, Some(goal)).ok())
        .is_some_and(|projection| projection["explicitLeadClosure"] == true);
    let c = continuation.as_ref().unwrap_or(&Value::Null);
    let binding = &c["binding"];
    let received = c["children"].as_array().is_some_and(|children| {
        children.iter().any(|child| {
            child["origin"]["revision"] == binding["revision"]
                && child["origin"]["session"] == binding["session"]
                && child["sourceClass"] == "host_reported_native_child"
        })
    }) || c["preparedChildren"].as_array().is_some_and(|children| {
        children.iter().any(|child| {
            matches!(child["state"].as_str(), Some("claimed" | "recorded"))
                && child["bindingRevision"] == binding["revision"]
                && child["session"] == binding["session"]
                && child["host"] == binding["host"]
        })
    });
    let returned = received && binding["previousRevision"].as_u64().is_some_and(|r| r > 0)
        || matches!(
            response["recoveryEvent"]["kind"].as_str(),
            Some("native_saved_return" | "native_return_replay")
        ) && response["recoveryEvent"]["work"] == work;
    let replay = response["recoveryEvent"]["kind"] == "native_return_replay"
        && response["recoveryEvent"]["work"] == work
        && response["recoveryEvent"]["providerExecuted"] == false
        && response["recoveryEvent"]["eventSha256"].is_string();
    let next = &response["residual"];
    let unresolved = matches!(
        response["presentation"]["exitState"].as_str(),
        Some("BLOCKED" | "REFUSED")
    ) || c["operations"].as_array().is_some_and(|ops| {
        ops.iter()
            .any(|op| matches!(op["status"].as_str(), Some("uncertain" | "failed")))
    }) || next["humanHelp"]["ownerDecision"] == "required";
    let reused = c["reuses"].as_array().is_some_and(|relations| {
        relations
            .iter()
            .any(|relation| relation["applicableCurrent"] == true)
    });
    let state = response["presentation"]["exitState"]
        .as_str()
        .filter(|s| matches!(*s, "READY" | "IN_PROGRESS" | "BLOCKED" | "REFUSED"))
        .unwrap_or("UNKNOWN");
    json!({"version":1,"renderer":"static_world_v1","readOnly":true,"authority":"none",
        "work":work,"goalRevision":c["goalRevision"],"status":state,"animation":"none",
        "motifs":[
            motif("same_door",returned,"same_work_return"),
            motif("passing_black_cat",replay,"recorded_result_replay"),
            motif("distant_payphone",received,"receiving_native_child_started"),
            motif("low_resolution_poster",unresolved,"canonical_unresolved_disposition"),
            motif("poster_becomes_signpost",reused,"finding_applied_with_current_observed_check"),
            motif("exit_sign",ready,"named_goal_current_readiness")
        ],"continuation":if continuation.is_some(){"current"}else{"unavailable"}})
}

fn motif(name: &str, active: bool, basis: &str) -> Value {
    json!({"name":name,"active":active,"basis":basis})
}

/// Export only allowlisted renderer state. Private work markers, event IDs,
/// paths, logs and text never become pixels or metadata in this artifact.
pub(crate) fn export(world: &Value) -> Value {
    json!({"version":1,"renderer":"static_world_v1","status":world["status"],
        "animation":"none","motifs":world["motifs"]})
}

pub(crate) fn render(world: &Value, plain: bool) -> String {
    let labels = [
        "The same door",
        "A passing black cat",
        "A distant payphone",
        "A low-resolution poster",
        "An old poster becomes a signpost",
        "The exit sign",
    ];
    let mut text = if plain {
        String::new()
    } else {
        "+----------------------------+\n|       THE QUIET HALL       |\n+----------------------------+\n".to_owned()
    };
    text.push_str(&format!(
        "Status: {}\n",
        world["status"].as_str().unwrap_or("UNKNOWN")
    ));
    for (label, motif) in labels
        .iter()
        .zip(world["motifs"].as_array().into_iter().flatten())
    {
        text.push_str(&format!(
            "{} {}\n",
            if motif["active"] == true {
                "[+]"
            } else {
                "[ ]"
            },
            label
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artwork_export_drops_private_identity_and_plain_has_identical_status() {
        let world = json!({"work":"private-marker","goalRevision":19,"status":"BLOCKED",
            "secret":"private-data","motifs":[motif("same_door",true,"same_work_return")]});
        let artifact = export(&world);
        let rendered = artifact.to_string();
        assert!(!rendered.contains("private"));
        assert_eq!(artifact["motifs"], world["motifs"]);
        assert!(render(&artifact, true).contains("Status: BLOCKED"));
        assert!(render(&artifact, false).contains("Status: BLOCKED"));
    }
}
