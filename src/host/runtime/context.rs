//! Render the current role profile and any verified assignment association.
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
        }
        _ => lines.push("No current governed assignment was acquired for this profile.".into()),
    }
    lines.push("Profile bytes:".into());
    lines.push(presented);
    Some(lines.join("\n"))
}
