//! Project the Lead's role and current accepted project memory into a new
//! session.
//!
//! A new work item in the same project then starts with the owner's configured
//! role and current rules and permitted findings, without the owner restating
//! them. Only items the existing memory lifecycle accepted for the Lead's read
//! scopes appear; revoked, rejected, expired, changed, and out-of-scope items
//! never do. The projection is context, not a check, approval, or permission,
//! and it is scoped to this initialized project.

use crate::config::Loaded;
use crate::evidence::hash;

const MAX_CONTENT: usize = 6 * 1024;
const MAX_ERROR: usize = 512;

pub(crate) fn session_context(loaded: &Loaded) -> Option<String> {
    let lead = loaded.lead()?;
    crate::memory::policy::get(&loaded.config)?;
    let agent = loaded.agent(lead)?;
    let profile = crate::config::file(&loaded.control_root, &agent.profile)
        .ok()
        .and_then(|path| hash::file(&path).ok())
        .map_or_else(
            || "unreadable".to_owned(),
            |digest| format!("sha256 {digest}"),
        );
    let mut text = format!(
        "Project role for this session: {lead} ({}), profile {} ({profile}).",
        agent.purpose.trim(),
        agent.profile
    );
    let references = match crate::memory::selection::resolve(loaded, lead) {
        Ok(references) if references.is_empty() => return Some(text),
        Ok(references) => references,
        Err(error) => {
            text.push_str(&format!(
                "\nAccepted project memory for the {lead} role could not be projected: {}. Do not assume earlier project rules.",
                error.chars().take(MAX_ERROR).collect::<String>()
            ));
            return Some(text);
        }
    };
    text.push_str(&format!(
        "\nAccepted project memory for the {lead} role: the owner's current project rules and permitted findings. Apply each only within its scope. It is not a check, approval, or permission, and revoked or expired items are excluded."
    ));
    let mut used = 0usize;
    for reference in &references {
        let (Some(scope), Some(path), Some(sha)) = (
            reference["scope"].as_str(),
            reference["sourcePath"].as_str(),
            reference["sourceSha256"].as_str(),
        ) else {
            continue;
        };
        let content = std::fs::read(loaded.product_root.join(path))
            .ok()
            .filter(|bytes| hash::bytes(bytes) == sha)
            .and_then(|bytes| String::from_utf8(bytes).ok());
        match content {
            Some(content) if used + content.len() <= MAX_CONTENT => {
                used += content.len();
                text.push_str(&format!("\n- [{scope}] {path}:\n{}", content.trim_end()));
            }
            _ => text.push_str(&format!("\n- [{scope}] {path} (read this file)")),
        }
    }
    Some(text)
}
