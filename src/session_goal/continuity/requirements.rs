//! Read-only compatibility assessment for named continuation requirements.

use serde_json::{json, Value};

pub(crate) fn assess(start: &Value, requirements: &[Value]) -> Value {
    let check_configured = start["checkPolicy"].is_object();
    let preservation = start["preservation"]["requirements"].as_array();
    let preservation_configured = preservation.is_some_and(|items| !items.is_empty());
    let missing = requirements
        .iter()
        .filter_map(|requirement| {
            let matched = preservation.is_some_and(|items| {
                items.iter().any(|item| {
                    item["id"] == requirement["id"] && item["text"] == requirement["text"]
                })
            });
            if check_configured && matched {
                return None;
            }
            let mut prerequisites = Vec::new();
            if !check_configured {
                prerequisites.push("--check-command");
            }
            if !matched {
                prerequisites.push("--preserve-requirement with the same ID and exact text");
            }
            if !preservation_configured {
                prerequisites.push("--preservation-check-command");
            }
            Some(json!({
                "id": requirement["id"],
                "text": requirement["text"],
                "missingPrerequisites": prerequisites,
            }))
        })
        .collect::<Vec<_>>();
    let compatible = missing.is_empty();
    let next_action = if compatible {
        Value::Null
    } else if requirements.len() > 1 {
        json!("The current CLI accepts one --preserve-requirement per Work and has no supported route for binding several requirement checks to one continuation. Keep this continuation uninitialized and request a focused design decision before retrying.")
    } else {
        json!("Start a new checked Work with the exact --preserve-requirement ID:TEXT and --preservation-check-command; use goal incorporate to attach it before initializing an existing goal.")
    };
    json!({
        "compatible": compatible,
        "requirementCount": requirements.len(),
        "checkConfigured": check_configured,
        "preservationConfigured": preservation_configured,
        "missingRequirements": missing,
        "effect": if compatible {
            "named requirements have frozen checked-work support"
        } else {
            "wholeGoalReady remains false; historical checks are not converted into requirement evidence"
        },
        "nextAction": next_action,
    })
}

pub(crate) fn init_error(assessment: &Value) -> String {
    let ids = assessment["missingRequirements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|requirement| requirement["id"].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let missing_check = assessment["checkConfigured"] != true;
    let missing_preservation = assessment["preservationConfigured"] != true;
    let mut prerequisites = Vec::new();
    if missing_check {
        prerequisites.push("--check-command");
    }
    prerequisites.push("matching --preserve-requirement ID:EXACT_TEXT entries");
    if missing_preservation {
        prerequisites.push("--preservation-check-command");
    }
    if assessment["requirementCount"].as_u64().unwrap_or_default() > 1 {
        prerequisites.push(
            "a supported multi-requirement route (the current CLI accepts one preservation requirement per Work)",
        );
    }
    format!(
        "named requirements unsupported by frozen Work setup ({ids}); missing prerequisite: {}; effect: no canonical goal state was written; next action: {}",
        prerequisites.join(" and "),
        assessment["nextAction"].as_str().unwrap_or("start a new checked Work with matching preservation requirements")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_text_must_match_the_frozen_work_policy_exactly() {
        let start = json!({
            "checkPolicy":{"command":"cargo test"},
            "preservation":{"requirements":[{
                "id":"api",
                "text":"Keep the API compatible.",
            }]},
        });
        let requirements = vec![json!({
            "id":"api",
            "text":"Keep the API stable.",
        })];
        let assessment = assess(&start, &requirements);
        assert_eq!(assessment["compatible"], false);
        assert_eq!(assessment["missingRequirements"][0]["id"], "api");
        assert!(assessment["nextAction"]
            .as_str()
            .unwrap()
            .contains("--preserve-requirement ID:TEXT"));
    }
}
