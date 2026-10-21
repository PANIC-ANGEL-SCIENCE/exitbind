//! `setup --lessons-from PREVIOUS_CONFIG`: bring the previous project's
//! accepted, unexpired project lessons into the new project. Setup owns the
//! configuration change (the `memory` section and lesson rights, in the same
//! shape a hand-configured project uses); the memory lesson module owns the
//! classification, the new immutable sources and the Lead lifecycle.

use crate::{config, host::settings, memory::lessons};
use serde_json::{json, Value};
use std::{fs, path::Path};

const LEAD_RIGHTS: [&str; 6] = [
    "memoryRead",
    "memoryWrite",
    "memoryReview",
    "memoryPromote",
    "memoryRevoke",
    "memoryExpire",
];

fn previous(from: &str) -> Result<config::Loaded, String> {
    config::load(Some(from)).map_err(|error| format!("--lessons-from: {error}"))
}

/// Read-only preview: the same lists `--apply` would produce; nothing changes.
pub(super) fn preview(from: Option<&str>, root: &Path) -> Result<Option<Value>, String> {
    let Some(from) = from else {
        return Ok(None);
    };
    let previous = previous(from)?;
    if previous.product_root == root {
        return Err(
            "--lessons-from names this project; give the previous project's configuration".into(),
        );
    }
    let mut value = lessons::carry::plan(&previous, root)?.preview();
    value["from"] = json!(previous.path);
    value["applied"] = json!(false);
    Ok(Some(value))
}

/// Carry the planned lessons after the new configuration exists. Returns the
/// (possibly reloaded) configuration and whether setup changed it.
pub(super) fn apply(
    loaded: config::Loaded,
    from: Option<&str>,
    report: &mut Value,
) -> Result<(config::Loaded, bool), String> {
    let Some(from) = from else {
        return Ok((loaded, false));
    };
    let previous = previous(from)?;
    if previous.product_root == loaded.product_root || previous.path == loaded.path {
        return Err(
            "--lessons-from names this project; give the previous project's configuration".into(),
        );
    }
    let plan = lessons::carry::plan(&previous, &loaded.product_root)?;
    if plan.is_empty() {
        let mut value = plan.preview();
        value["from"] = json!(previous.path);
        value["applied"] = json!(true);
        report["lessons"] = value;
        return Ok((loaded, false));
    }
    let mut next: Value =
        serde_json::from_str(&loaded.source).map_err(|error| error.to_string())?;
    let lead = loaded
        .lead()
        .ok_or("lesson carry-over requires a configured Lead")?
        .to_owned();
    let mut recipients = plan.recipients();
    recipients.retain(|agent| loaded.agent(agent).is_some());
    let edited = memory_section(&mut next, &lead, &recipients);
    let loaded = if edited {
        let mut source = serde_json::to_string_pretty(&next).map_err(|error| error.to_string())?;
        source.push('\n');
        let root = fs::canonicalize(&loaded.control_root).map_err(|error| error.to_string())?;
        settings::atomic_write(&loaded.path, &source, None, Some(&loaded.source), &root)
            .map_err(|error| format!("setup partially applied: {error}"))?;
        let path = loaded
            .path
            .to_str()
            .ok_or("configuration path is not valid UTF-8")?;
        config::load(Some(path)).map_err(|error| format!("setup partially applied: {error}"))?
    } else {
        loaded
    };
    let mut value = lessons::carry::apply(&loaded, plan)
        .map_err(|error| format!("setup partially applied: lesson carry-over: {error}"))?;
    // The carried events are stamped with these exact bytes; keep them so a
    // later configuration change has a real file to review against.
    let copy = crate::memory::stamped_config::keep(&loaded)
        .map_err(|error| format!("setup partially applied: stamped configuration copy: {error}"))?;
    value["stampedConfig"] = json!(copy);
    value["from"] = json!(previous.path);
    value["applied"] = json!(true);
    report["lessons"] = value;
    Ok((loaded, edited))
}

/// Opt the project in to `project-lessons.v1`, give the Lead its lesson rights
/// and each recipient `memoryRead`. Existing rights and scopes are kept.
fn memory_section(
    config: &mut Value,
    lead: &str,
    recipients: &std::collections::BTreeSet<String>,
) -> bool {
    let before = config.clone();
    if !config["memory"].is_object() {
        config["memory"] = json!({"root": format!("{}/memory", crate::project::layout_types::state_namespace()),
            "maxItems": 8, "maxBytes": 16384, "protocolScopes": [], "syntheticScopes": []});
    }
    add(&mut config["memory"]["protocolScopes"], lessons::SCOPE);
    let mut grant = |agent: &str, rights: &[&str]| {
        let entry = &mut config["agents"][agent];
        for right in rights {
            add(&mut entry[*right], lessons::SCOPE);
        }
        if entry["crossContext"]
            .as_str()
            .map_or(true, |value| value == "none")
        {
            entry["crossContext"] = json!("same-scope");
        }
    };
    grant(lead, &LEAD_RIGHTS);
    for agent in recipients {
        grant(agent, &["memoryRead"]);
    }
    *config != before
}

fn add(list: &mut Value, scope: &str) {
    if !list.is_array() {
        *list = json!([]);
    }
    let items = list.as_array_mut().expect("array was just ensured");
    if !items.iter().any(|item| item == scope) {
        items.push(json!(scope));
    }
}

pub(super) fn print(lessons: &Value) {
    let Some(carried) = lessons["carried"].as_array() else {
        return;
    };
    let verb = if lessons["applied"] == true {
        "carried"
    } else {
        "would carry"
    };
    let ids = carried
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect::<Vec<_>>();
    println!(
        "Lessons {verb} from {}: {}",
        lessons["from"].as_str().unwrap_or("the previous project"),
        if ids.is_empty() {
            "none".to_owned()
        } else {
            ids.join(", ")
        }
    );
    for item in lessons["notCarried"].as_array().into_iter().flatten() {
        println!(
            "  not carried: {} ({}{})",
            item["id"]
                .as_str()
                .or(item["previousItemId"].as_str())
                .unwrap_or("unknown"),
            item["reason"].as_str().unwrap_or("unknown"),
            item["path"]
                .as_str()
                .map(|path| format!(": {path}"))
                .unwrap_or_default()
        );
    }
}
