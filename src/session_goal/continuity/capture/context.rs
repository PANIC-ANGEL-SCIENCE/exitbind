//! The complete current context of one prepared named child: base profile,
//! frozen perspectives and the assignment's deliveries, rechecked now.

use super::{adviser, binding_of, load, perspective, selected_is_current, Frozen};
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

/// Current, complete, bounded bytes for one prepared named child. A saved
/// selector is never enough: binding, assignment and source hashes are rechecked.
pub(crate) fn context(loaded: &Loaded, work: &str, intent_id: &str) -> Result<Value, String> {
    let intent = load(loaded)?
        .into_iter()
        .find(|item| item["id"] == intent_id && item["work"] == work)
        .ok_or("prepared child intent is unknown")?;
    if !matches!(intent["state"].as_str(), Some("prepared" | "claimed")) {
        return Err("child context expired with its prepared assignment".into());
    }
    let (_, binding) = binding_of(loaded, work, &intent)?;
    if binding["session"] != intent["session"] || binding["revision"] != intent["bindingRevision"] {
        return Err("child context expired after binding change".into());
    }
    let selected = &intent["selected"];
    if selected.is_null() {
        return Err("no named assignment was frozen for this child".into());
    }
    if !selected_is_current(loaded, work, selected) {
        return Err("child context expired after assignment or profile change".into());
    }
    let agent_id = selected["agent"].as_str().ok_or("invalid selected agent")?;
    let agent = loaded
        .agent(agent_id)
        .ok_or("selected agent is unavailable")?;
    let bytes =
        crate::project::path::secure_bytes(&loaded.control_root, &agent.profile, "agent profile")?;
    if hash::bytes(&bytes) != selected["profileSha256"] {
        return Err("selected profile source changed".into());
    }
    let source = String::from_utf8(bytes).map_err(|_| "agent profile is not UTF-8")?;
    let presented = crate::host::runtime::redact(&source);
    let sources: Vec<Frozen> = serde_json::from_value(intent["perspectives"].clone())
        .map_err(|_| "prepared perspectives are malformed")?;
    let perspectives = perspective::current(loaded, &sources)?;
    let current = crate::work::next(loaded, work)?;
    crate::memory::contextual::assert_assignment(loaded, &current["next"]["packet"])?;
    adviser::validate_requested_binding(
        loaded,
        &current["next"]["packet"],
        binding["host"].as_str().unwrap_or(""),
    )?;
    let packet = &current["next"]["packet"];
    let host = binding["host"].as_str().unwrap_or("");
    let guidance = if crate::run::guidance_continuity::enabled(packet) {
        crate::project::agent_context::guidance_for_assignment(
            loaded,
            agent_id,
            packet["goal"].as_str(),
            host,
            3 * 1024,
            packet,
        )?
    } else {
        crate::project::agent_context::guidance(
            loaded,
            agent_id,
            packet["goal"].as_str(),
            host,
            3 * 1024,
        )?
    };
    let architecture = crate::project::architecture::delivery(loaded, &current["next"]["packet"])?;
    let capabilities = crate::project::capabilities::delivery(
        loaded,
        &current["next"]["packet"],
        binding["host"].as_str().unwrap_or(""),
    )?;
    let specialist = crate::project::specialists::delivery(
        loaded,
        agent_id,
        current["next"]["role"].as_str().unwrap_or(""),
        current["next"]["packet"]["goal"].as_str().unwrap_or(""),
        binding["host"].as_str().unwrap_or(""),
    )?;
    if (specialist.is_some() || !intent["specialistBinding"].is_null())
        && intent["specialistBinding"] != hash::value(&json!(specialist))
    {
        return Err(
            "specialist experience changed or expired; prepare the current approved next launch"
                .into(),
        );
    }
    if !intent["capabilityBinding"].is_null()
        && intent["capabilityBinding"] != hash::value(&json!(capabilities))
    {
        return Err("approved child capabilities changed or expired; prepare the current approved next launch".into());
    }
    let mut result = json!({
        "work":work,"intent":intent_id,"assignment":selected["assignment"],
        "configuredAgent":agent_id,"nativeTaskName":selected["nativeTaskName"],
        "bindingRevision":intent["bindingRevision"],
        "profile":{"path":agent.profile,"sourceSha256":selected["profileSha256"],
            "presentedSha256":hash::text(&presented),"content":presented,
            "transformation":"path-redaction","coverage":"complete"},
        "perspectives":perspectives,
        "evidence":"product-presented-for-current-assignment; native launch and behavior separate",
    });
    if let Some(architecture) = &architecture {
        result["architectureContract"] = architecture.clone();
    }
    if let Some(capabilities) = &capabilities {
        result["capabilities"] = capabilities.clone();
    }
    if let Some(specialist) = &specialist {
        result["specialistExperience"] = specialist.clone();
    }
    result["guidance"] = guidance;
    let fresh = crate::work::next(loaded, work)?;
    crate::memory::contextual::assert_assignment(loaded, &fresh["next"]["packet"])?;
    let (_, fresh_binding) = binding_of(loaded, work, &intent)?;
    if fresh["next"]["assignment"] != selected["assignment"]
        || fresh_binding != binding
        || crate::project::architecture::delivery(loaded, &fresh["next"]["packet"])? != architecture
        || crate::project::capabilities::delivery(
            loaded,
            &fresh["next"]["packet"],
            fresh_binding["host"].as_str().unwrap_or(""),
        )? != capabilities
        || crate::project::specialists::delivery(
            loaded,
            agent_id,
            fresh["next"]["role"].as_str().unwrap_or(""),
            fresh["next"]["packet"]["goal"].as_str().unwrap_or(""),
            fresh_binding["host"].as_str().unwrap_or(""),
        )? != specialist
    {
        return Err("child context changed during delivery; refresh the current assignment".into());
    }
    if serde_json::to_vec(&result)
        .map_err(|_| "child context serialization failed")?
        .len()
        > 16 * 1024
    {
        return Err("child context exceeds the complete 16 KiB envelope".into());
    }
    Ok(result)
}
