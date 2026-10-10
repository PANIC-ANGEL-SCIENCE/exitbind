//! Human labels never select a role, native identity or authority.
use crate::config::Loaded;

pub(crate) fn lead(loaded: &Loaded) -> String {
    let Some(id) = loaded.lead() else {
        return "unavailable".into();
    };
    let name = loaded
        .agent(id)
        .and_then(|agent| agent.display_name.as_deref())
        .unwrap_or(id);
    name.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
