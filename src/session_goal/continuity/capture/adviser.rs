use crate::config::Loaded;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn validate_requested_binding(
    loaded: &Loaded,
    packet: &Value,
    receiving_host: &str,
) -> Result<(), String> {
    if packet["guidanceContinuityProtocol"].as_u64()
        != Some(crate::run::guidance_continuity::PROTOCOL)
        || packet["role"] != "adviser"
    {
        return Ok(());
    }
    let agent_id = packet["agent"]
        .as_str()
        .ok_or("marked adviser assignment has no configured agent")?;
    if !loaded.roles_for(agent_id).contains("adviser") {
        return Err("marked adviser is no longer configured for the adviser role".into());
    }
    let configured = loaded
        .agent(agent_id)
        .ok_or("marked adviser is no longer configured")?;
    let host = configured
        .runtime
        .host
        .as_deref()
        .filter(|value| matches!(*value, "codex" | "claude"))
        .ok_or("native adviser requested host is unresolved")?;
    if host != receiving_host {
        return Err("native adviser requested host differs from the bound receiving host".into());
    }
    let model = configured
        .runtime
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or("native adviser requested model is unresolved")?;
    let effort = configured
        .runtime
        .reasoning_effort
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or("native adviser requested reasoning effort is unresolved")?;
    let floor = &loaded.config["project"]["adviserFloor"];
    let required = floor["bindings"]
        .as_array()
        .and_then(|bindings| {
            bindings.iter().find(|binding| {
                binding["host"] == host
                    && binding["model"] == model
                    && binding["reasoningEffort"] == effort
            })
        })
        .and_then(|binding| binding["requiredCapabilities"].as_array())
        .ok_or("native adviser requested binding is outside the approved floor")?;
    let available = if required.is_empty() {
        BTreeSet::new()
    } else {
        crate::project::capabilities::delivery(loaded, packet, host)?
            .and_then(|value| value["items"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| item["id"].as_str().map(str::to_owned))
            .collect()
    };
    crate::work::action::validate_adviser_candidate(floor, host, model, effort, &available)
}
