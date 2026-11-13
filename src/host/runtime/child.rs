// Assembles and measures the selected native child delivery.
// Refuses a missing child callback configuration; see `src/host/runtime.rs`.
//! Assemble and measure the same selected child text before and at delivery.

use super::{context, project, MAX_PROFILE};
use crate::{config::Loaded, host::assignment_context::Selection};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

pub(super) fn profile(
    loaded: &Loaded,
    configured: &crate::config::types::AgentConfig,
) -> Option<(PathBuf, String)> {
    let profile = crate::config::file(&loaded.control_root, &configured.profile).ok()?;
    if fs::metadata(&profile).ok()?.len() > MAX_PROFILE {
        return None;
    }
    let source = fs::read_to_string(&profile).ok()?;
    project::contained_existing(&loaded.control_root, &profile).then_some((profile, source))
}

pub(super) fn base(
    loaded: &Loaded,
    agent: &str,
    configured: &crate::config::types::AgentConfig,
    profile: &std::path::Path,
    source: &str,
    selected: &Selection,
    configuration: &str,
) -> Option<String> {
    let mut text = context::format_agent_context(
        &loaded.control_root,
        agent,
        configured,
        profile,
        source,
        selected,
    )?;
    if crate::producer::exitbind_surface() {
        text.push('\n');
        text.push_str(configuration);
    }
    Some(text)
}

/// Preserve the ordinary incomplete-delivery fallback in `text` when the
/// selected complete presentation does not fit.
pub(super) fn render(
    loaded: &Loaded,
    child: &Value,
    selected: &Selection,
    text: &mut String,
    event: &str,
    multipart: bool,
) -> Result<bool, String> {
    text.push_str("\nSelected task perspectives for this assignment:");
    if let Some(specialist) = child.get("specialistExperience") {
        text.push_str("\nCurrent qualified specialist experience (complete; observed use remains separate):\n");
        text.push_str(&serde_json::to_string(specialist).map_err(|e| e.to_string())?);
    }
    if let Some(capabilities) = child.get("capabilities") {
        text.push_str(
            "\nCurrent approved project capabilities (complete; native use remains separate):\n",
        );
        text.push_str(&serde_json::to_string(capabilities).map_err(|e| e.to_string())?);
    }
    for perspective in child["perspectives"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "\nPerspective {} (source SHA-256 {}, presented SHA-256 {}, complete):\n{}",
            perspective["id"].as_str().unwrap_or("?"),
            perspective["sourceSha256"].as_str().unwrap_or("?"),
            perspective["presentedSha256"].as_str().unwrap_or("?"),
            perspective["content"].as_str().unwrap_or("")
        ));
    }
    if let Some(contract) = child["actorResultContract"].as_str() {
        text.push('\n');
        text.push_str(contract);
    }
    if let Some(assignment) = child.get("requiredAssignment") {
        if multipart && child["deliveryHost"] == "claude" {
            *text = context::complete_assignment(loaded, child, selected, assignment, text)?;
            return Ok(true);
        }
        if let Some(complete) =
            context::complete_with_evidence(loaded, child, selected, assignment, text, event)?
        {
            *text = complete;
            return Ok(true);
        }
        text.push_str("\nComplete assignment exceeds the host context envelope; automatic return cannot receive credit. Use the current scoped Work detail route.");
    } else if child["requiredAssignmentUnavailable"].is_string() {
        text.push_str("\nComplete assignment exceeds the host context envelope; use the current scoped Work detail route.");
    }
    Ok(false)
}

/// Measure only an owner-resolved Claude callback for this exact current
/// candidate. Other hosts or an unverified callback project remain unknown.
pub(crate) fn preflight(
    loaded: &Loaded,
    work: &str,
    intent: &Value,
    child: &Value,
) -> Result<Value, String> {
    let host = intent["host"].as_str().unwrap_or("");
    if let Some(unknown) = unknown_capacity(host) {
        return Ok(unknown);
    }
    let session = intent["session"]
        .as_str()
        .ok_or("child session is unavailable")?;
    let native = intent["selected"]["nativeTaskName"]
        .as_str()
        .ok_or("child native task name is unavailable")?;
    let cwd = loaded
        .product_root
        .to_str()
        .ok_or("product root is not valid UTF-8")?;
    let payload = json!({"session_id":session,"agent_type":native,"cwd":cwd});
    let object = payload
        .as_object()
        .ok_or("child preflight payload is unavailable")?;
    let (hook_loaded, config_path) = match project::resolve(object) {
        project::Resolution::Project {
            loaded: hook_loaded,
            config_path,
            source: project::Source::GoalOwner,
            ..
        } => (hook_loaded, config_path),
        project::Resolution::Project { .. } => {
            return Ok(json!({"host":host,"state":"unknown",
            "reason":"callback configuration depends on the host working directory"}))
        }
        _ => {
            return Err("child callback configuration is unavailable; no child was prepared".into())
        }
    };
    if hook_loaded.path != loaded.path
        || hook_loaded.source != loaded.source
        || hook_loaded.product_root != loaded.product_root
        || hook_loaded.control_root != loaded.control_root
        || hook_loaded.state_root != loaded.state_root
        || hook_loaded.project_id != loaded.project_id
    {
        return Err(
            "child callback configuration differs from the current Work; no child was prepared"
                .into(),
        );
    }
    let agent = super::exact_agent(object, &loaded.agents)
        .filter(|agent| child["configuredAgent"].as_str() == Some(agent.as_str()))
        .ok_or("child callback agent identity is unavailable or ambiguous")?;
    let configured = loaded.agent(&agent).ok_or("child agent is unavailable")?;
    let (profile, source) = profile(loaded, configured).ok_or("child profile is unavailable")?;
    let selected = crate::host::assignment_context::selection(
        loaded,
        &agent,
        &crate::evidence::hash::text(&source),
    );
    if !matches!(&selected, Selection::Bound { work: selected_work, assignment, guidance, .. }
        if selected_work.as_str() == work && child["assignment"] == assignment.as_str()
            && child["guidance"] == *guidance)
    {
        return Err("child callback selection changed; no child was prepared".into());
    }
    let configuration = project::fact_line(&config_path, project::Source::GoalOwner);
    let mut text = base(
        loaded,
        &agent,
        configured,
        &profile,
        &source,
        &selected,
        &configuration,
    )
    .ok_or("child profile context is unavailable")?;
    if !render(loaded, child, &selected, &mut text, "SubagentStart", true)? {
        let detail =
            crate::emitted::command_line(&["work", "detail", work, "--json"], Some(&loaded.path));
        return Err(format!("complete child assignment exceeds the native host context capacity; no child was prepared. Inspect `{detail}` and narrow optional material before preparing again"));
    }
    let bundle = super::multipart::bundle(&text).map_err(|error| {
        format!("{error}; no child was prepared. Narrow optional material before preparing again")
    })?;
    let sizes = bundle
        .parts
        .iter()
        .map(|part| crate::host::assignment_evidence::claude_size("SubagentStart", part))
        .collect::<Result<Vec<_>, _>>()?;
    let multipart_bytes = sizes.iter().map(|size| size.0).max().unwrap_or(0);
    let multipart_units = sizes.iter().map(|size| size.1).max().unwrap_or(0);
    Ok(
        json!({"host":host,"state":"fits","outputBytes":multipart_bytes,
        "contextUtf16Units":multipart_units,"parts":bundle.parts.len(),
        "meaning":"each current complete host envelope fits; delivery is rechecked at launch and does not prove model understanding or execution"}),
    )
}

fn unknown_capacity(host: &str) -> Option<Value> {
    (host != "claude").then(|| {
        json!({"host":host,"state":"unknown",
        "reason":"native delivery capacity for this host is not verified"})
    })
}

#[cfg(test)]
mod tests {
    use super::unknown_capacity;

    #[test]
    fn unverified_native_hosts_remain_unknown() {
        assert!(unknown_capacity("claude").is_none());
        for host in ["codex", "other", ""] {
            let result = unknown_capacity(host).unwrap();
            assert_eq!(result["host"], host);
            assert_eq!(result["state"], "unknown");
        }
    }
}
