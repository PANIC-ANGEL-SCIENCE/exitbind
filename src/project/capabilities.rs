//! Explicitly reviewed project resources for a relevant next native launch.
//! Configuration owns registration; current context owns bounded delivery.
use crate::{config::Loaded, evidence::hash, project::path};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

mod adoption;
pub(crate) use adoption::select;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Selection {
    pub(crate) id: String,
    pub(crate) revision: String,
    pub(crate) descriptor_path: String,
    pub(crate) descriptor_sha256: String,
    pub(crate) content_sha256: String,
    pub(crate) approved_by: String,
    pub(crate) approval_reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Descriptor {
    version: u32,
    id: String,
    revision: String,
    kind: String,
    owner: String,
    project_identity: String,
    path: String,
    roles: Vec<String>,
    task_terms: Vec<String>,
    hosts: Vec<String>,
    expires_at: String,
    #[serde(default)]
    revoked: bool,
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn id(value: &str) -> bool {
    text(value, 64)
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn relative(value: &str) -> bool {
    text(value, 256)
        && std::path::Path::new(value)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}
fn bounded_read(loaded: &Loaded, source: &str) -> Result<Vec<u8>, String> {
    match path::secure_bytes_observation_bounded(
        &loaded.product_root,
        source,
        "capability source",
        4096,
    ) {
        path::SecureBytesResult::Bytes(bytes) => Ok(bytes),
        path::SecureBytesResult::Absent(e)
        | path::SecureBytesResult::Unsafe(e)
        | path::SecureBytesResult::Unreadable(e) => Err(e),
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(e) => Err(e),
    }
}
pub(crate) fn selections(value: &Value) -> Result<Vec<Selection>, String> {
    let entries: Vec<Selection> = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid capability selections: {e}"))?;
    let mut ids = BTreeSet::new();
    if entries.len() > 8
        || entries.iter().any(|s| {
            !id(&s.id)
                || !text(&s.revision, 64)
                || !text(&s.approved_by, 64)
                || !text(&s.approval_reason, 512)
                || !relative(&s.descriptor_path)
                || !digest(&s.descriptor_sha256)
                || !digest(&s.content_sha256)
                || !ids.insert(&s.id)
        })
    {
        return Err("capabilities require at most eight unique ids, confined descriptor paths, bounded revisions and exact SHA-256 digests".into());
    }
    Ok(entries)
}
fn descriptor(loaded: &Loaded, source: &[u8]) -> Result<Descriptor, String> {
    let d: Descriptor = serde_json::from_slice(source)
        .map_err(|e| format!("invalid capability descriptor: {e}"))?;
    if d.version != 1
        || !id(&d.id)
        || !text(&d.revision, 64)
        || !matches!(d.kind.as_str(), "skill" | "lens")
        || Some(d.owner.as_str()) != loaded.lead()
        || d.project_identity != crate::host::assignment_context::project_identity(loaded)?
        || !relative(&d.path)
        || d.roles.is_empty()
        || d.roles.len() > 4
        || d.roles
            .iter()
            .any(|s| !matches!(s.as_str(), "lead" | "worker" | "reviewer" | "adviser"))
        || d.roles.iter().collect::<BTreeSet<_>>().len() != d.roles.len()
        || d.hosts.is_empty()
        || d.hosts.len() > 2
        || d.hosts
            .iter()
            .any(|s| !matches!(s.as_str(), "codex" | "claude"))
        || d.hosts.iter().collect::<BTreeSet<_>>().len() != d.hosts.len()
        || d.task_terms.is_empty()
        || d.task_terms.len() > 4
        || d.task_terms.iter().any(|s| !text(s, 64))
    {
        return Err("invalid capability identity, owner, confined path or declared role/task/host applicability".into());
    }
    chrono::DateTime::parse_from_rfc3339(&d.expires_at)
        .map_err(|_| "capability expiresAt must be an RFC3339 instant")?;
    Ok(d)
}
fn read_descriptor(loaded: &Loaded, s: &Selection) -> Result<Descriptor, String> {
    let bytes = bounded_read(loaded, &s.descriptor_path)?;
    if hash::bytes(&bytes) != s.descriptor_sha256 {
        return Err(
            "capability descriptor changed; register the reviewed revision before a new Work"
                .into(),
        );
    }
    let d = descriptor(loaded, &bytes)?;
    if d.id != s.id || d.revision != s.revision || Some(s.approved_by.as_str()) != loaded.lead() {
        return Err("capability identity differs from its approved registration".into());
    }
    Ok(d)
}
fn active(d: &Descriptor) -> Result<bool, String> {
    let expiry = chrono::DateTime::parse_from_rfc3339(&d.expires_at)
        .map_err(|_| "capability expiry is invalid")?;
    Ok(!d.revoked && expiry > chrono::Utc::now())
}
fn applicable(d: &Descriptor, role: &str, task: &str, host: &str) -> bool {
    d.roles.iter().any(|s| s == role)
        && d.hosts.iter().any(|s| s == host)
        && d.task_terms
            .iter()
            .any(|s| task.to_lowercase().contains(&s.to_lowercase()))
}
fn native_unique(loaded: &Loaded, recipient: &str, host: &str) -> bool {
    let native = loaded.agents[recipient].native_name(recipient);
    loaded
        .agents
        .iter()
        .filter(|(name, agent)| {
            agent.native_name(name) == native
                && !agent.runtime.host.as_deref().is_some_and(|h| h != host)
        })
        .count()
        == 1
}
fn content(loaded: &Loaded, s: &Selection, d: &Descriptor) -> Result<String, String> {
    let bytes = bounded_read(loaded, &d.path)?;
    if hash::bytes(&bytes) != s.content_sha256 {
        return Err(
            "capability content changed; review and register it again before delivery".into(),
        );
    }
    String::from_utf8(bytes).map_err(|_| "capability content must be complete UTF-8".into())
}
pub(crate) fn delivery(
    loaded: &Loaded,
    packet: &Value,
    host: &str,
) -> Result<Option<Value>, String> {
    let Some(agent_id) = packet["agent"].as_str() else {
        return Ok(None);
    };
    let agent = loaded
        .agent(agent_id)
        .ok_or("capability recipient is not configured")?;
    let role = packet["role"].as_str().unwrap_or("");
    let task = packet["goal"].as_str().unwrap_or("");
    let mut items = Vec::new();
    for s in &loaded.capabilities {
        let d = read_descriptor(loaded, s)?;
        if !active(&d)? || !applicable(&d, role, task, host) {
            continue;
        }
        if agent.runtime.host.as_deref().is_some_and(|h| h != host) {
            continue;
        }
        if !native_unique(loaded, agent_id, host) {
            return Err("capability native recipient mapping is ambiguous; resolve the configured mapping before launch".into());
        }
        let presented = crate::host::runtime::redact(&content(loaded, s, &d)?);
        items.push(json!({"id":s.id,"revision":s.revision,"kind":d.kind,
            "descriptorSha256":s.descriptor_sha256,"sourceSha256":s.content_sha256,
            "presentedSha256":hash::text(&presented),"content":presented,"coverage":"complete","transformation":"path-redaction"}));
    }
    if items.is_empty() {
        return Ok(None);
    }
    if items.len() > 3 {
        return Err(
            "more than three applicable capabilities; narrow the approved selection".into(),
        );
    }
    let identity = hash::value(
        &json!({"project":crate::host::assignment_context::project_identity(loaded)?,
        "configuration":hash::text(&loaded.source),"recipient":agent_id,"role":role,"task":task,"host":host,"items":items}),
    );
    let result = json!({"state":"projected","deliveryId":identity,"recipient":agent_id,
        "nativeName":agent.native_name(agent_id),"host":host,"items":items,
        "meaning":"current approved context for the next compatible native launch; no installation or host permission; launch, acknowledgement and observed use remain separate"});
    if serde_json::to_vec(&result)
        .map_err(|e| e.to_string())?
        .len()
        > 6144
    {
        return Err("complete capability delivery exceeds 6144 bytes; narrow the selection".into());
    }
    if std::fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source {
        return Err("capability configuration changed during delivery".into());
    }
    Ok(Some(result))
}
pub(crate) fn inspect(loaded: &Loaded, task: &str, host: &str) -> Result<Value, String> {
    if !matches!(host, "codex" | "claude") {
        return Err("capability host must be codex or claude".into());
    }
    let mut resources = Vec::new();
    for s in &loaded.capabilities {
        let d = read_descriptor(loaded, s)?;
        let state = if d.revoked {
            "revoked"
        } else if !active(&d)? {
            "expired"
        } else {
            "registered"
        };
        let mut recipients = Vec::new();
        if state == "registered" {
            content(loaded, s, &d)?;
            for (name, agent) in &loaded.agents {
                let roles = loaded.roles_for(name);
                if roles.iter().any(|role| applicable(&d, role, task, host))
                    && !agent.runtime.host.as_deref().is_some_and(|h| h != host)
                {
                    recipients.push(json!({"agent":name,"nativeName":agent.native_name(name),"state":if native_unique(loaded,name,host) { "pending next launch" } else { "ambiguous native mapping; no delivery" }}));
                }
            }
        }
        resources
            .push(json!({"id":s.id,"revision":s.revision,"state":state,"recipients":recipients}));
    }
    Ok(
        json!({"version":1,"effect":"no-change","host":host,"resources":resources,
        "liveRefresh":"unsupported; already-running context is retained; prepare the next approved launch",
        "acknowledged":false,"observedUse":false}),
    )
}
