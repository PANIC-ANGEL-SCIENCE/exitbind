//! Bounded delivery of complete project instruction files.
//!
//! The Work detail consumer has a 64 KiB response bound and the native
//! context has a separate 64 KiB bound. Keep rule input bounded before either
//! projection; account for JSON escaping in the ordinary Work detail route.

use crate::{config::Loaded, evidence::hash, project::path};
use serde_json::{json, Value};

const RULES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
pub(crate) const MAX_RULE_BYTES: usize = 32 * 1024;
const MAX_RULES_BYTES: usize = 48 * 1024;
const MAX_RULES_JSON_BYTES: usize = 48 * 1024;

pub(crate) fn current(loaded: &Loaded) -> Result<Vec<(String, String, String)>, String> {
    let mut rules = Vec::new();
    let mut total = 0usize;
    for name in RULES {
        let bytes = match path::secure_bytes_observation_bounded(
            &loaded.product_root,
            name,
            "project rule",
            MAX_RULE_BYTES as u64,
        ) {
            path::SecureBytesResult::Bytes(bytes) => bytes,
            path::SecureBytesResult::Absent(_) => continue,
            path::SecureBytesResult::Unreadable(reason)
                if reason.contains("exceeds its byte bound") =>
            {
                return Err(format!(
                    "project rule {name} exceeds the {MAX_RULE_BYTES}-byte complete-delivery limit; launch refused so instructions are not truncated"
                ));
            }
            path::SecureBytesResult::Unsafe(reason)
            | path::SecureBytesResult::Unreadable(reason) => return Err(reason),
            #[cfg(not(unix))]
            path::SecureBytesResult::Unsupported(reason) => return Err(reason),
        };
        total = total.saturating_add(bytes.len());
        if total > MAX_RULES_BYTES {
            return Err(format!(
                "project rules exceed the {MAX_RULES_BYTES}-byte aggregate complete-delivery limit; launch refused so instructions are not truncated"
            ));
        }
        let content = String::from_utf8(bytes.clone())
            .map_err(|_| format!("project rule {name} is not UTF-8; launch refused"))?;
        rules.push(((*name).to_owned(), hash::bytes(&bytes), content));
    }
    Ok(rules)
}

pub(crate) fn detail_projection(rules: &[(String, String, String)]) -> Result<Vec<Value>, String> {
    let projected = rules
        .iter()
        .map(|(path, sha256, content)| {
            json!({"path":path,"sha256":sha256,"bytes":content.len(),"content":content,
                "complete":true})
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&projected).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_RULES_JSON_BYTES {
        return Err(format!(
            "project rules exceed the {MAX_RULES_JSON_BYTES}-byte encoded detail limit; launch refused so instructions are not truncated"
        ));
    }
    Ok(projected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(content: String) -> (String, String, String) {
        ("AGENTS.md".into(), "digest".into(), content)
    }

    #[test]
    fn complete_reported_rule_fits_and_escaped_overflow_refuses() {
        let reported = vec![rule("x".repeat(18_920))];
        let projected = detail_projection(&reported).unwrap();
        assert_eq!(projected[0]["content"].as_str().unwrap().len(), 18_920);
        assert_eq!(projected[0]["complete"], true);

        let escaped = vec![rule("\u{0001}".repeat(8_200))];
        let error = detail_projection(&escaped).unwrap_err();
        assert!(error.contains("encoded detail limit"));
        assert!(error.contains("not truncated"));
    }
}
