//! Render the current role profile and any verified assignment association.
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn format_agent_context(
    project: &Path,
    agent: &str,
    config: &crate::config::types::AgentConfig,
    path: &Path,
    source: &str,
    selection: &crate::host::assignment_context::Selection,
) -> Option<String> {
    let list = |items: &[String]| {
        if items.is_empty() {
            "none".into()
        } else {
            items
                .iter()
                .map(|item| super::safe_inline(item))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let relative = path
        .strip_prefix(project)
        .ok()?
        .to_str()?
        .replace('\\', "/");
    let native = config.native_name(agent);
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    let presented = crate::host::runtime::redact(source);
    let mut lines = vec![
        format!("{product} plan-only context for {agent} (native event reported this agent)."),
        format!("Agent ID: {}", super::safe_inline(agent)),
        format!("Native task name: {}", super::safe_inline(&native)),
        format!("Profile selected/presented: {}", super::safe_inline(&relative)),
        format!("Profile SHA-256: {}", crate::evidence::hash::text(source)),
        format!("Presented profile SHA-256: {} (after path redaction).", crate::evidence::hash::text(&presented)),
        "Evidence is selected/presented bytes; it does not prove a model read or followed the profile.".into(),
        "Declared boundary:".into(),
    ];
    for (key, values) in [
        ("observe", &config.observe),
        ("write", &config.write),
        ("commands", &config.commands),
        ("skills", &config.skills),
        ("memoryRead", &config.memory_read),
        ("memoryWrite", &config.memory_write),
        ("memoryReview", &config.memory_review),
        ("memoryPromote", &config.memory_promote),
        ("memoryReject", &config.memory_reject),
        ("memoryRevoke", &config.memory_revoke),
        ("memoryExpire", &config.memory_expire),
        ("memoryForget", &config.memory_forget),
    ] {
        lines.push(format!("  {key}: {}", list(values)));
    }
    lines.push(format!(
        "  retention: {}",
        super::safe_inline(&config.retention)
    ));
    lines.push(format!(
        "  crossContext: {}",
        super::safe_inline(&config.cross_context)
    ));
    match selection {
        crate::host::assignment_context::Selection::Bound {
            work,
            assignment,
            packet_digest,
            provenance,
            architecture,
            guidance,
            check_preparation,
            ..
        } => {
            lines.push(format!(
                "Current assignment: work {work}, assignment {assignment}, packet digest {packet_digest}."
            ));
            lines.push("Base profile is associated with this current assignment; model use is not inferred.".into());
            if let Some(architecture) = architecture {
                lines.push(format!(
                    "Current architecture contract slice: {architecture}"
                ));
            }
            lines.push(format!(
                "Assignment provenance: project {}, work {}, assignment {}, role {}, configured agent {}, native task {}, profile source SHA-256 {}, packet context SHA-256 {}, project rules projection SHA-256 {}, root scope {}.",
                super::safe_inline(&provenance.project),
                super::safe_inline(&provenance.work),
                super::safe_inline(&provenance.assignment),
                super::safe_inline(&provenance.role),
                super::safe_inline(&provenance.agent),
                super::safe_inline(&provenance.native),
                super::safe_inline(&provenance.profile_sha256),
                super::safe_inline(&provenance.packet_digest),
                super::safe_inline(&provenance.rules_sha256),
                super::safe_inline(&provenance.root_scope),
            ));
            lines.push("Inherited global contract source identity: host-provided and unavailable to this product hook. Forbidden or superseded task scope: unobservable at this host boundary.".into());
            lines.push("Current eligible memory and lessons (complete items, ordered; omitted items have scoped read routes):".into());
            lines.push(serde_json::to_string(guidance).ok()?);
            lines.push("Current check preparation (observation, not a check result):".into());
            lines.push(serde_json::to_string(check_preparation).ok()?);
        }
        _ => lines.push("No current governed assignment was acquired for this profile.".into()),
    }
    lines.push("Profile bytes:".into());
    lines.push(presented);
    Some(lines.join("\n"))
}

pub(super) fn complete_assignment(
    loaded: &crate::config::Loaded,
    child: &Value,
    selected: &crate::host::assignment_context::Selection,
    assignment: &Value,
    context: &str,
) -> Result<String, String> {
    let detail = child["requiredAssignmentBinding"]
        .as_str()
        .and_then(|binding| {
            let reference = format!("{}{binding}", crate::work::readable::PREFIX);
            crate::emitted::Route::new([
                "work",
                "expand",
                child["work"].as_str()?,
                &reference,
                "--json",
            ])
            .config(&loaded.path)
            .full_argv()
            .ok()
        });
    // Omit Lead forms only when their exact current expansion is available.
    let recipient = detail.as_ref().map_or_else(
        || assignment.clone(),
        |route| recipient_assignment(assignment, route),
    );
    let mut complete = without_duplicate_assignment_summary(context, selected, assignment);
    complete.push_str("\nComplete current assignment, evidence and task conditions (verified for this recipient):\n");
    complete.push_str(&serde_json::to_string(&recipient).map_err(|e| e.to_string())?);
    Ok(complete)
}

/// Omit only Lead decision forms from the host presentation. The complete,
/// current detail remains available through the supplied bound route.
pub(super) fn recipient_assignment(assignment: &Value, detail: &[String]) -> Value {
    let mut result = assignment.clone();
    let mut removed_routes = false;
    if let Some(categories) = result
        .pointer_mut("/tasks/categoryDispositions")
        .and_then(Value::as_object_mut)
    {
        for name in [
            "obligations",
            "findings",
            "blockers",
            "decisions",
            "externalActions",
        ] {
            let Some(category) = categories.get_mut(name).and_then(Value::as_object_mut) else {
                continue;
            };
            if category.get("ownerDecisionRequired") == Some(&json!(true)) {
                removed_routes |= category.remove("nextActions").is_some();
                if category.get("validStates") == Some(&json!(["considered", "none_applicable"])) {
                    category.remove("validStates");
                    removed_routes = true;
                }
                if category.get("validItemDispositions")
                    == Some(&json!([
                        "open",
                        "accepted",
                        "outside_scope",
                        "successor",
                        "direct"
                    ]))
                {
                    category.remove("validItemDispositions");
                    removed_routes = true;
                }
            }
            if let Some(items) = category.get_mut("items").and_then(Value::as_array_mut) {
                for item in items {
                    let Some(object) = item.as_object_mut() else {
                        continue;
                    };
                    if object.get("ownerDecisionRequired") == Some(&json!(true)) {
                        removed_routes |= object.remove("nextActions").is_some();
                    }
                }
            }
        }
    }
    let first = result.pointer("/assignment/assignment/preservationAssignment");
    let second = result.pointer("/assignment/residual/humanHelp/preservationAssignment");
    let equal_bytes = match (first, second) {
        (Some(first), Some(second)) if !first.is_null() => matches!(
            (serde_json::to_vec(first), serde_json::to_vec(second)),
            (Ok(left), Ok(right)) if left == right
        ),
        _ => false,
    };
    if let Some(value) = result
        .pointer_mut("/assignment/residual/humanHelp/preservationAssignment")
        .filter(|_| equal_bytes)
    {
        *value = json!({"sameAs":"assignment.assignment.preservationAssignment"});
    }
    if removed_routes {
        result["ownerActionDetail"] = json!(detail);
    }
    result
}

/// Remove a repeated header only when the complete assignment contains the
/// same identity and check observation. The fallback text is untouched.
pub(super) fn without_duplicate_assignment_summary(
    context: &str,
    selection: &crate::host::assignment_context::Selection,
    assignment: &Value,
) -> String {
    let crate::host::assignment_context::Selection::Bound {
        work,
        assignment: assignment_id,
        packet_digest,
        check_preparation,
        ..
    } = selection
    else {
        return context.to_owned();
    };
    let mut result = context.to_owned();
    let packet = &assignment["assignment"]["assignment"];
    if assignment["assignment"]["residual"]["work"] == work.as_str()
        && packet["context"]["next"]["assignment"] == assignment_id.as_str()
        && packet["context"]["digest"] == packet_digest.as_str()
    {
        let duplicate = format!("Current assignment: work {work}, assignment {assignment_id}, packet digest {packet_digest}.\n");
        result = result.replacen(&duplicate, "", 1);
    }
    if !check_preparation.is_null()
        && *check_preparation == assignment["assignment"]["checkPreparation"]
    {
        let duplicate = format!(
            "Current check preparation (observation, not a check result):\n{check_preparation}\n"
        );
        result = result.replacen(&duplicate, "", 1);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_projection_aliases_only_equal_preservation_and_keeps_non_owner_routes() {
        let mut original = json!({"assignment": {
            "assignment": {"preservationAssignment": {"requirements": ["must keep"]}},
            "residual": {"humanHelp": {"preservationAssignment": {"requirements": ["must keep"]}}}
        }, "tasks": {"categoryDispositions": {"obligations": {
            "ownerDecisionRequired": true,
            "nextActions": ["owner form"],
            "items": [
                {"id":"owner", "ownerDecisionRequired":true, "nextActions":["owner choice"]},
                {"id":"recipient", "ownerDecisionRequired":false, "nextActions":["recipient route"]}
            ]
        }}}});
        let route = vec!["work".into(), "expand".into(), "bound".into()];
        let projected = recipient_assignment(&original, &route);
        assert_eq!(projected["ownerActionDetail"], json!(route));
        assert_eq!(
            projected["assignment"]["residual"]["humanHelp"]["preservationAssignment"]["sameAs"],
            "assignment.assignment.preservationAssignment"
        );
        assert_eq!(
            projected["assignment"]["assignment"]["preservationAssignment"]["requirements"][0],
            "must keep"
        );
        assert!(
            projected["tasks"]["categoryDispositions"]["obligations"]["items"][0]
                .get("nextActions")
                .is_none()
        );
        assert_eq!(
            projected["tasks"]["categoryDispositions"]["obligations"]["items"][1]["nextActions"][0],
            "recipient route"
        );

        original["assignment"]["residual"]["humanHelp"]["preservationAssignment"]["requirements"]
            [0] = json!("different");
        let projected = recipient_assignment(&original, &route);
        assert_eq!(
            projected["assignment"]["residual"]["humanHelp"]["preservationAssignment"]
                ["requirements"][0],
            "different"
        );
        assert_eq!(
            projected["assignment"]["assignment"]["preservationAssignment"]["requirements"][0],
            "must keep"
        );
        assert_eq!(recipient_assignment(&json!(null), &route), json!(null));
    }
}
