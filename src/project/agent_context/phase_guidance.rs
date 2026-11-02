//! Phase guidance bodies declared on one agent, delivered whole with the
//! assignment that enters the phase, or as an exact role-scoped read route.
use crate::{
    config::{phase_guidance as declared, Loaded},
    evidence::hash,
};
use serde_json::{json, Value};

pub(super) const KIND: &str = "phase_guidance";
type Items = (Vec<Value>, Vec<Value>, Vec<Value>);

pub(super) fn is_phase_item(entry: &Value) -> bool {
    entry["reference"]["kind"] == KIND
}

/// Current complete bodies for `agent` in `phases`: (items, overflow,
/// unavailable). Items larger than `max_item` overflow to their read route.
pub(super) fn items(
    loaded: &Loaded,
    agent: &str,
    phases: &[&str],
    max_item: usize,
) -> Result<Items, String> {
    let (mut items, mut overflow, mut unavailable) = (Vec::new(), Vec::new(), Vec::new());
    let Some(configured) = loaded.agent(agent) else {
        return Ok((items, overflow, unavailable));
    };
    for item in configured
        .phase_guidance
        .iter()
        .filter(|item| phases.contains(&item.phase.as_str()))
    {
        let item_id = item_id(&item.id);
        match read(loaded, item) {
            Some((reference, content)) if content.len() <= max_item => {
                items.push(json!({"reference":reference,"content":content}));
            }
            Some((reference, content)) => {
                overflow.push(overflow_entry(
                    loaded,
                    agent,
                    reference,
                    content.len(),
                    "item_size_limit",
                )?);
            }
            None => unavailable
                .push(json!({"itemId":item_id,"reason":"source_unavailable_or_not_utf8"})),
        }
    }
    Ok((items, overflow, unavailable))
}

/// Selected automatic delivery may claim complete instructions only when
/// every applicable declared phase body is actually present and current.
/// Optional memory overflow remains a separate visible notice.
pub(crate) fn require_selected(
    loaded: &Loaded,
    agent: &str,
    role: &str,
    attempt: u64,
    guidance: &Value,
) -> Result<(), String> {
    let configured = loaded
        .agent(agent)
        .ok_or("phase recipient is unavailable")?;
    let phases = declared::phases_for(role, attempt);
    for item in configured
        .phase_guidance
        .iter()
        .filter(|item| phases.contains(&item.phase.as_str()))
    {
        let (reference, content) =
            read(loaded, item).ok_or("required phase guidance source is unavailable")?;
        let selected = guidance["items"]
            .as_array()
            .ok_or("selected phase guidance is unavailable")?;
        if selected
            .iter()
            .filter(|entry| entry["reference"] == reference && entry["content"] == content)
            .count()
            != 1
        {
            return Err("required phase guidance was not delivered complete and current".into());
        }
    }
    Ok(())
}

/// Whether a delivered reference still names this agent's declared item with
/// the same current bytes.
pub(super) fn still_current(loaded: &Loaded, agent: &str, reference: &Value) -> bool {
    declared_item(
        loaded,
        agent,
        reference["itemId"].as_str().unwrap_or_default(),
    )
    .and_then(|item| read(loaded, item))
    .is_some_and(|(current, _)| &current == reference)
}

pub(super) fn overflow_entry(
    loaded: &Loaded,
    agent: &str,
    reference: Value,
    bytes: usize,
    reason: &str,
) -> Result<Value, String> {
    let command = crate::emitted::Route::new([
        "project",
        "context",
        "guidance",
        reference["itemId"].as_str().unwrap_or_default(),
        "--agent",
        agent,
        "--expected-sha256",
        reference["sourceSha256"].as_str().unwrap_or_default(),
        "--json",
    ])
    .config(&loaded.path)
    .within(4096);
    Ok(json!({"reference":reference,"bytes":bytes,"reason":reason,
        "readRoute":{"command":command.argv_value(),"readOnly":true,
            "sameConfigRequired":command.needs_config(),
            "sameExecutableRequired":command.needs_executable()}}))
}

/// The role-scoped read behind an overflow route: only the named agent's own
/// declared item, and only while its bytes still match the expected hash.
pub(crate) fn content_for_agent(
    loaded: &Loaded,
    item_id: &str,
    agent: &str,
    expected_sha256: &str,
) -> Result<Value, String> {
    let item = declared_item(loaded, agent, item_id)
        .ok_or("phase guidance item is not declared for this agent")?;
    let (reference, content) =
        read(loaded, item).ok_or("phase guidance source is unavailable or not UTF-8")?;
    if reference["sourceSha256"] != expected_sha256 {
        return Err("phase guidance source changed; refresh the current assignment detail".into());
    }
    Ok(
        json!({"reference":reference,"content":content,"agent":agent,
        "authority":"read-only guidance; grants no permission"}),
    )
}

fn declared_item<'a>(
    loaded: &'a Loaded,
    agent: &str,
    item_id: &str,
) -> Option<&'a declared::PhaseGuidance> {
    loaded
        .agent(agent)?
        .phase_guidance
        .iter()
        .find(|item| self::item_id(&item.id) == item_id)
}

fn item_id(id: &str) -> String {
    format!("phase:{id}")
}

fn read(loaded: &Loaded, item: &declared::PhaseGuidance) -> Option<(Value, String)> {
    let source = item.source(&loaded.control_root);
    let bytes = std::fs::read(&source).ok()?;
    let content = String::from_utf8(bytes).ok()?;
    let reference = json!({"kind":KIND,"itemId":item_id(&item.id),"id":item.id,
        "phase":item.phase,"sourcePath":source.to_str()?,"sourceSha256":hash::text(&content),
        "byteLength":content.len()});
    Some((reference, content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types;

    #[test]
    fn selected_delivery_requires_each_applicable_phase_body_but_not_optional_memory() {
        let root = crate::test_root::temp("selected-phase-guidance");
        let source = root.join("phase.md");
        std::fs::write(&source, "required phase instruction\n").unwrap();
        let config = json!({"agents":{"worker":{"profile":"profile.md",
            "purpose":"implement","retention":"task","crossContext":"none",
            "phaseGuidance":[{"id":"implementation","phase":"implementation","path":"phase.md"}]}}});
        let loaded = Loaded {
            agents: types::from_validated_agents(&config["agents"]).unwrap(),
            workflows: Default::default(),
            config,
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            verification_preparation: None,
            path: root.join("exitbind.json"),
            control_root: root.to_path_buf(),
            product_root: root.to_path_buf(),
            state_root: root.to_path_buf(),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: String::new(),
        };
        let phases = declared::phases_for("worker", 1);
        let (selected, overflow, unavailable) =
            items(&loaded, "worker", &phases, 16 * 1024).unwrap();
        let mut guidance = json!({"items":selected,"overflow":overflow,"unavailable":unavailable});
        guidance["overflow"] = json!([{"reference":{"kind":"optional_memory"}}]);
        assert!(require_selected(&loaded, "worker", "worker", 1, &guidance).is_ok());

        let (selected, overflow, unavailable) = items(&loaded, "worker", &phases, 8).unwrap();
        let over_budget = json!({"items":selected,"overflow":overflow,"unavailable":unavailable});
        assert!(!over_budget["overflow"].as_array().unwrap().is_empty());
        assert!(require_selected(&loaded, "worker", "worker", 1, &over_budget).is_err());
        guidance["items"][0]["content"] = json!("changed instruction");
        assert!(require_selected(&loaded, "worker", "worker", 1, &guidance).is_err());
        std::fs::remove_file(source).unwrap();
        let (selected, overflow, unavailable) =
            items(&loaded, "worker", &phases, 16 * 1024).unwrap();
        let unavailable = json!({"items":selected,"overflow":overflow,"unavailable":unavailable});
        assert!(!unavailable["unavailable"].as_array().unwrap().is_empty());
        assert!(require_selected(&loaded, "worker", "worker", 1, &unavailable).is_err());
        assert!(require_selected(&loaded, "worker", "reviewer", 1, &unavailable).is_ok());
    }
}
