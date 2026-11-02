//! The complete current context of one prepared named child: base profile,
//! frozen perspectives and the assignment's deliveries, rechecked now.

use super::{adviser, binding_of, load, perspective, selected_matches_current, Frozen};
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

/// Current, complete, bounded bytes for one prepared named child. A saved
/// selector is never enough: binding, assignment and source hashes are rechecked.
pub(crate) fn context(loaded: &Loaded, work: &str, intent_id: &str) -> Result<Value, String> {
    context_inner(loaded, work, intent_id, false, false)
}

/// Internal host assembly is checked against the actual serialized host
/// envelope after duplicate fields are removed. The public context reader
/// keeps its existing independent 16 KiB result bound.
pub(super) fn host_context(loaded: &Loaded, work: &str, intent_id: &str) -> Result<Value, String> {
    context_inner(loaded, work, intent_id, true, true)
}

/// Verify the existing complete identity and recipient context without
/// requiring the newer selected-presentation proof. Used both to associate a
/// claimed child for observation and to recover a legacy presentation.
pub(super) fn core_host_context(
    loaded: &Loaded,
    work: &str,
    intent_id: &str,
) -> Result<Value, String> {
    context_inner(loaded, work, intent_id, true, false)
}

fn context_inner(
    loaded: &Loaded,
    work: &str,
    intent_id: &str,
    host_delivery: bool,
    selected_actor: bool,
) -> Result<Value, String> {
    let intent = load(loaded)?
        .into_iter()
        .find(|item| item["id"] == intent_id && item["work"] == work)
        .ok_or("prepared child intent is unknown")?;
    let pending_host_return = host_delivery
        && intent["state"] == "recorded"
        && intent["directAssignment"] == true
        && intent["pendingReturn"] == true;
    if !matches!(intent["state"].as_str(), Some("prepared" | "claimed")) && !pending_host_return {
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
    let current = crate::work::next(loaded, work)
        .map_err(|_| "child context expired after assignment or profile change")?;
    if !selected_matches_current(loaded, work, selected, &current["next"]) {
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
    let mut assignment_binding = None;
    if intent["directAssignment"] == true {
        let (delivery, check_command) = if host_delivery && selected_actor && host == "claude" {
            let (delivery, command) = crate::work::details::required_assignment_with_check(
                loaded,
                work,
                &current["next"],
            )?;
            (delivery, Some(command))
        } else {
            (
                crate::work::details::required_assignment(loaded, work, &current["next"])?,
                None,
            )
        };
        super::reviewer_retry::ensure_fence(
            loaded,
            work,
            &intent,
            &delivery["binding"],
            &delivery["sections"],
        )?;
        result["hostResultContract"] =
            crate::work::action::host_result_contract(loaded, work, &current["next"])?;
        assignment_binding = Some(delivery["binding"].clone());
        let mut complete = result.clone();
        complete["requiredAssignment"] = delivery["sections"].clone();
        complete["requiredAssignmentBinding"] = delivery["binding"].clone();
        if host_delivery {
            if selected_actor {
                complete["deliveryHost"] = json!(host);
                // The installed Codex hook advertises a context limit whose
                // units and delivery behavior have not been verified here.
                // New selected automatic credit is therefore Claude-only.
                if host == "claude" {
                    super::actor_checks::exact_instructions(&result)?;
                    let packet = &current["next"]["packet"];
                    let role = current["next"]["role"]
                        .as_str()
                        .ok_or("native actor role is unavailable")?;
                    let attempt = packet["attempt"]
                        .as_u64()
                        .ok_or("native actor attempt is unavailable")?;
                    crate::project::agent_context::require_selected_phase_guidance(
                        loaded,
                        agent_id,
                        role,
                        attempt,
                        &result["guidance"],
                    )?;
                    let binding = delivery["binding"]
                        .as_str()
                        .ok_or("native actor binding is unavailable")?;
                    let check_command = check_command
                        .as_ref()
                        .ok_or("native actor check command is unavailable")?;
                    let selected = crate::host::runtime::selected_actor_projection(
                        loaded,
                        work,
                        binding,
                        &delivery["sections"],
                        check_command,
                    )?;
                    let (contract, _) =
                        crate::work::action::actor_contract(role, &result["hostResultContract"])?;
                    complete["actorFactsSha256"] = json!(hash::value(&selected));
                    complete["actorAssignment"] = selected;
                    complete["actorContractSha256"] = json!(hash::text(&contract));
                    complete["actorResultContract"] = json!(contract);
                }
            }
            complete["requiredFactsSha256"] = delivery["requiredFactsSha256"].clone();
            complete["requiredRecipientSha256"] = delivery["recipientSha256"].clone();
            complete["requiredRules"] = delivery["recipientRules"].clone();
            complete["requiredInstructionsSha256"] = json!(hash::value(&json!({
                "profile":result["profile"], "perspectives":result["perspectives"],
                "guidance":result["guidance"], "architectureContract":result["architectureContract"],
                "capabilities":result["capabilities"], "specialistExperience":result["specialistExperience"],
                "rules":delivery["recipientRules"],
            })));
        }
        if host_delivery
            || serde_json::to_vec(&complete)
                .map_err(|_| "child context serialization failed")?
                .len()
                <= 16 * 1024
        {
            result = complete;
        } else {
            result["requiredAssignmentUnavailable"] = json!("complete assignment exceeds the host context envelope; use the current scoped Work detail route");
        }
    }
    let fresh = crate::work::next(loaded, work)?;
    crate::memory::contextual::assert_assignment(loaded, &fresh["next"]["packet"])?;
    let (_, fresh_binding) = binding_of(loaded, work, &intent)?;
    if fresh["next"]["assignment"] != selected["assignment"]
        || assignment_binding
            .as_ref()
            .is_some_and(|binding| fresh["next"]["current"]["binding"] != *binding)
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
    if !host_delivery
        && serde_json::to_vec(&result)
            .map_err(|_| "child context serialization failed")?
            .len()
            > 16 * 1024
    {
        return Err("child context exceeds the complete 16 KiB envelope".into());
    }
    Ok(result)
}
