//! Bounded closing facts appended to the shared Goal system text: how the
//! journey closes and whether project lessons are configured.

use crate::config::Loaded;
use serde_json::Value;

const READ_ROUTE: &str = "`goal status --json .journeyCompletion`";
const MAX_SUFFIX_CHARS: usize = 196;

pub(super) fn append(loaded: &Loaded, record: &Value, text: String) -> String {
    let mut text = text;
    text.reserve(MAX_SUFFIX_CHARS);
    text.push_str(" | Journey: ");
    text.push_str(&journey(loaded, record));
    if crate::memory::policy::get(&loaded.config).is_some_and(|policy| {
        policy
            .protocol_scopes
            .contains(crate::memory::lessons::SCOPE)
    }) {
        text.push_str(" | Lessons: ");
        text.push_str(&lessons(loaded, record));
    }
    text
}

fn journey(loaded: &Loaded, record: &Value) -> String {
    let members = super::journey::state(record)
        .ok()
        .flatten()
        .map(|state| state.members.len())
        .unwrap_or(0);
    let route = if loaded.state_root == loaded.product_root {
        "each member accepted and current"
    } else {
        "one final --journey-integration Work"
    };
    format!("{members} members, closes by {route}; not-yet-current members: {READ_ROUTE}")
}

fn lessons(loaded: &Loaded, record: &Value) -> String {
    let Some(lead) = loaded.lead() else {
        return "0 deliverable (no configured Lead)".into();
    };
    let task = record["goal"].as_str().unwrap_or_default();
    match crate::memory::lessons::delivery(loaded, lead, task) {
        Ok(delivered) => {
            let items = delivered
                .as_ref()
                .and_then(|value| value["items"].as_array())
                .map_or(0, Vec::len);
            let omitted = delivered
                .as_ref()
                .and_then(|value| value["omittedCount"].as_u64())
                .unwrap_or(0) as usize;
            let mut state = format!("{items} deliverable to Lead `{lead}`");
            if omitted > 0 {
                state.push_str(&format!(", {omitted} omitted"));
            }
            if delivered
                .as_ref()
                .is_some_and(|value| value.get("unavailable").is_some())
            {
                state.push_str(", some unavailable");
            }
            state
        }
        Err(_) => "unavailable".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn loaded(root: &str, lead: Option<&str>) -> Loaded {
        Loaded {
            config: json!({}),
            agents: std::collections::BTreeMap::new(),
            lead: lead.map(str::to_owned),
            architecture_contract: None,
            capabilities: Vec::new(),
            workflows: std::collections::BTreeMap::new(),
            verification_preparation: None,
            path: std::path::PathBuf::from(root).join("exitbind.json"),
            control_root: std::path::PathBuf::from(root).join("service"),
            product_root: std::path::PathBuf::from(root).join("product"),
            state_root: std::path::PathBuf::from(root).join("state"),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: String::new(),
        }
    }

    #[test]
    fn prefix_is_preserved_and_absent_lessons_are_not_reported() {
        let prefix = "Goal: IN PROGRESS | Tasks: 0/1 complete | Result readiness: BLOCKED";
        let text = append(
            &loaded("/unused", None),
            &json!({"goal": "g"}),
            prefix.into(),
        );
        assert!(text.starts_with(prefix), "{text}");
        assert!(text.contains("| Journey: 0 members, closes by one final --journey-integration Work; not-yet-current members: `goal status --json .journeyCompletion`"), "{text}");
        assert!(!text.contains("Lessons:"), "{text}");
        assert!(text.chars().count() - prefix.chars().count() <= MAX_SUFFIX_CHARS);
    }

    #[test]
    fn product_state_root_closes_by_member_acceptance() {
        let mut loaded = loaded("/unused", None);
        loaded.state_root = loaded.product_root.clone();
        let text = append(&loaded, &json!({"goal": "g"}), "x".into());
        assert!(
            text.contains("closes by each member accepted and current"),
            "{text}"
        );
    }
}
