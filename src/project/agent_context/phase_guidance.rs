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
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let command = vec![
        executable
            .to_str()
            .ok_or("executable path is not UTF-8")?
            .to_owned(),
        "project".into(),
        "context".into(),
        "guidance".into(),
        reference["itemId"].as_str().unwrap_or_default().to_owned(),
        "--agent".into(),
        agent.to_owned(),
        "--expected-sha256".into(),
        reference["sourceSha256"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        "--json".into(),
        "--config".into(),
        loaded
            .path
            .to_str()
            .ok_or("configuration path is not UTF-8")?
            .to_owned(),
    ];
    Ok(json!({"reference":reference,"bytes":bytes,"reason":reason,
        "readRoute":{"command":command,"readOnly":true,"sameConfigRequired":true,
            "sameExecutableRequired":true}}))
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
