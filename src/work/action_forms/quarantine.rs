//! Verification-only forms cannot advertise an ordinary mutation or successor.
use super::*;

pub(super) fn project(loaded: &Loaded, work: &str, next: &Value, binding: &str) -> Value {
    let marker = &next["packet"]["quarantineVerification"];
    let choices: Vec<_> = next["assignment"]
        .as_str()
        .map_or_else(Vec::new, |assignment| {
            allowed_outcomes(next)
                .into_iter()
                .filter(|outcome| {
                    *outcome != "disposition" && !(*outcome == "rework" && next["role"] == "lead")
                })
                .map(|outcome| return_choice(loaded, work, assignment, binding, outcome, outcome))
                .collect()
        });
    json!({
        "version":1,"state":"quarantine_verification_only","binding":binding,
        "work":work,"assignment":next["assignment"],"actor":next["resolvedActor"],
        "leadChoiceRequired":next["role"]=="lead","choices":choices,
        "mechanicalAction":if next["action"]=="check" {
            json!({"command":command(loaded,vec!["work".into(),"check".into(),work.into()]),
                "meaning":"Execute the separately authorized frozen verification once; an admitted execution is never retried.",
                "executionOptions":{"timeoutMs":{"option":"--timeout-ms","default":1_800_000,
                    "minimum":1,"maximum":crate::run::check_observation::MAX_TIMEOUT_MS}}})
        } else {Value::Null},
        "meaning":"Return the exact frozen artifact without code mutation. Fresh check, required review and distinct Lead acceptance remain mandatory.",
        "quarantine":{"predecessor":marker["predecessor"]["work"],
            "observationEventSha256":marker["predecessor"]["observationEventSha256"],
            "oldCredited":false,"oldReset":false,"allowance":marker["allowance"],
            "artifactSha256":marker["verification"]["artifactSha256"],
            "environmentSha256":marker["verification"]["environmentSha256"]}
    })
}
