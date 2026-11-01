//! Render the current role profile and any verified assignment association.
use serde_json::{json, Value};
use std::path::Path;

fn boundary_lines(config: &crate::config::types::AgentConfig) -> Vec<String> {
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
    let mut lines = vec!["Declared boundary:".to_owned()];
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
    lines
}

fn provenance_line(provenance: &crate::host::assignment_context::Provenance) -> String {
    format!(
        "Assignment provenance: project {}, work {}, assignment {}, role {}, configured agent {}, native task {}, profile source SHA-256 {}, packet context SHA-256 {}, project rules projection SHA-256 {}, root scope {}.",
        super::safe_inline(&provenance.project), super::safe_inline(&provenance.work),
        super::safe_inline(&provenance.assignment), super::safe_inline(&provenance.role),
        super::safe_inline(&provenance.agent), super::safe_inline(&provenance.native),
        super::safe_inline(&provenance.profile_sha256), super::safe_inline(&provenance.packet_digest),
        super::safe_inline(&provenance.rules_sha256), super::safe_inline(&provenance.root_scope),
    )
}

pub(super) fn note_delivery(
    loaded: &crate::config::Loaded,
    payload: &serde_json::Map<String, Value>,
    child: Option<&Value>,
) -> Result<(), String> {
    let binding = child.and_then(|value| value["requiredAssignmentBinding"].as_str());
    let permit = child.and_then(|value| {
        value["requiredAssignment"]["currentAction"]
            .get("beforeEditing")
            .filter(|form| form.is_object())
    });
    let facts = child.and_then(|value| {
        Some((
            value["requiredFactsSha256"].as_str()?,
            value["requiredRecipientSha256"].as_str()?,
            value["requiredInstructionsSha256"].as_str()?,
        ))
    });
    crate::session_goal::note_child_delivery(loaded, payload, binding, permit, facts)
}

pub(super) fn format_agent_context(
    project: &Path,
    agent: &str,
    config: &crate::config::types::AgentConfig,
    path: &Path,
    source: &str,
    selection: &crate::host::assignment_context::Selection,
) -> Option<String> {
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
    ];
    lines.extend(boundary_lines(config));
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
            lines.push(provenance_line(provenance));
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
    let config = child["configuredAgent"]
        .as_str()
        .and_then(|agent| loaded.agent(agent));
    let mut complete = without_duplicate_assignment_summary(context, selected, assignment, config);
    if child["requiredRules"]
        .as_array()
        .is_some_and(|rules| !rules.is_empty())
    {
        complete.push_str("\nCurrent project rules (complete verified bytes):\n");
        complete
            .push_str(&serde_json::to_string(&child["requiredRules"]).map_err(|e| e.to_string())?);
    }
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
        // The same empty/status category can appear several times. Preserve
        // the complete value once and use an explicit alias only for an
        // identical serialized value.
        let mut seen: Vec<(String, Vec<u8>)> = Vec::new();
        for name in [
            "obligations",
            "findings",
            "blockers",
            "decisions",
            "externalActions",
        ] {
            let Some(value) = categories.get(name) else {
                continue;
            };
            let Ok(bytes) = serde_json::to_vec(value) else {
                continue;
            };
            if let Some((first, _)) = seen.iter().find(|(_, original)| *original == bytes) {
                categories.insert(
                    name.into(),
                    json!({"sameAs":format!("tasks.categoryDispositions.{first}")}),
                );
            } else {
                seen.push((name.into(), bytes));
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

/// Keep both profile identities while presenting identical digest bytes once.
fn alias_equal_profile_hashes(context: &str, source_digest: &str) -> String {
    let Some(profile_start) = context.find("\nProfile bytes:\n") else {
        return context.to_owned();
    };
    let (header, profile) = context.split_at(profile_start);
    let repeated = format!(
        "Profile SHA-256: {source_digest}\nPresented profile SHA-256: {source_digest} (after path redaction)."
    );
    let alias = format!(
        "Profile SHA-256: {source_digest}\nPresented profile SHA-256: same as Profile SHA-256 (after path redaction)."
    );
    format!("{}{profile}", header.replacen(&repeated, &alias, 1))
}

/// Remove a repeated header only when the complete assignment contains the
/// same identity and check observation. The fallback text is untouched.
pub(super) fn without_duplicate_assignment_summary(
    context: &str,
    selection: &crate::host::assignment_context::Selection,
    assignment: &Value,
    config: Option<&crate::config::types::AgentConfig>,
) -> String {
    let crate::host::assignment_context::Selection::Bound {
        work,
        assignment: assignment_id,
        packet_digest,
        provenance,
        check_preparation,
        ..
    } = selection
    else {
        return context.to_owned();
    };
    let mut result = alias_equal_profile_hashes(context, &provenance.profile_sha256);
    let packet = &assignment["assignment"]["assignment"];
    if assignment["assignment"]["residual"]["work"] == work.as_str()
        && packet["context"]["next"]["assignment"] == assignment_id.as_str()
        && packet["context"]["digest"] == packet_digest.as_str()
    {
        let duplicate = format!("Current assignment: work {work}, assignment {assignment_id}, packet digest {packet_digest}.\n");
        result = result.replacen(&duplicate, "", 1);
    }
    if let Some(config) = config {
        let boundary = json!({"observe":config.observe,"write":config.write,
            "commands":config.commands,"skills":config.skills,
            "memoryRead":config.memory_read,"memoryWrite":config.memory_write,
            "memoryReview":config.memory_review,"memoryPromote":config.memory_promote,
            "memoryReject":config.memory_reject,"memoryRevoke":config.memory_revoke,
            "memoryExpire":config.memory_expire,"memoryForget":config.memory_forget,
            "retention":config.retention,"crossContext":config.cross_context});
        if packet["declaredBoundary"] == boundary {
            result = result.replacen(&format!("{}\n", boundary_lines(config).join("\n")), "", 1);
        }
    }
    let root_scope = packet["context"]["run"]["id"]
        .as_str()
        .or_else(|| packet["context"]["scope"]["planSha256"].as_str())
        .unwrap_or("unavailable");
    if assignment["assignment"]["residual"]["work"] == provenance.work.as_str()
        && packet["context"]["next"]["assignment"] == provenance.assignment.as_str()
        && packet["role"] == provenance.role.as_str()
        && packet["agent"] == provenance.agent.as_str()
        && packet["nativeTaskName"] == provenance.native.as_str()
        && packet["profileSha256"] == provenance.profile_sha256.as_str()
        && packet["context"]["digest"] == provenance.packet_digest.as_str()
        && root_scope == provenance.root_scope.as_str()
    {
        let compact = format!(
            "Project {}. Rules SHA-256 {}. Identity is in the assignment.",
            super::safe_inline(&provenance.project),
            super::safe_inline(&provenance.rules_sha256)
        );
        result = result.replacen(&provenance_line(provenance), &compact, 1);
        result = result.replacen(
            "Base profile is associated with this current assignment; model use is not inferred.\n",
            "",
            1,
        );
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
    fn equal_profile_hashes_share_one_explicitly_resolvable_value() {
        let source = "a".repeat(64);
        let presented = "b".repeat(64);
        let equal = format!(
            "Profile SHA-256: {source}\nPresented profile SHA-256: {source} (after path redaction).\nProfile bytes:\nrequired instructions"
        );
        let aliased = alias_equal_profile_hashes(&equal, &source);
        assert_eq!(
            aliased,
            format!(
                "Profile SHA-256: {source}\nPresented profile SHA-256: same as Profile SHA-256 (after path redaction).\nProfile bytes:\nrequired instructions"
            )
        );
        let unequal = format!(
            "Profile SHA-256: {source}\nPresented profile SHA-256: {presented} (after path redaction).\nProfile bytes:\nrequired instructions"
        );
        assert_eq!(alias_equal_profile_hashes(&unequal, &source), unequal);
        assert_eq!(alias_equal_profile_hashes(&equal, &presented), equal);
        let within_profile = format!("{unequal}\n{equal}");
        assert_eq!(
            alias_equal_profile_hashes(&within_profile, &source),
            within_profile
        );
    }

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
