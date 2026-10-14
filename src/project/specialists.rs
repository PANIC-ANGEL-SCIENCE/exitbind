//! Read-only specialist selection and qualified, agent-owned experience.
//! Existing configuration and memory lifecycle remain the sole writers.
use crate::{config::Loaded, evidence::hash, memory, project::path};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const SOURCE_LIMIT: u64 = 4096;
const SNAPSHOT_LIMIT: usize = 16384;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Experience {
    version: u32,
    id: String,
    revision: String,
    kind: String,
    author: String,
    project_identity: String,
    task_terms: Vec<String>,
    hosts: Vec<String>,
    summary: String,
    preconditions: Vec<String>,
    procedure: Vec<String>,
    verification: Vec<String>,
    required_commands: Vec<String>,
    evidence: Vec<String>,
    guards: Vec<Guard>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    path: String,
    sha256: String,
}

fn text(s: &str, limit: usize) -> bool {
    !s.trim().is_empty() && s.len() <= limit && !s.chars().any(char::is_control)
}
fn list(values: &[String], max: usize, length: usize) -> bool {
    !values.is_empty() && values.len() <= max && values.iter().all(|s| text(s, length))
}
fn identifier(s: &str) -> bool {
    text(s, 64)
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
}
fn read(root: &std::path::Path, name: &str) -> Result<Vec<u8>, String> {
    match path::secure_bytes_observation_bounded(root, name, "specialist resource", SOURCE_LIMIT) {
        path::SecureBytesResult::Bytes(bytes) => Ok(bytes),
        path::SecureBytesResult::Absent(e)
        | path::SecureBytesResult::Unsafe(e)
        | path::SecureBytesResult::Unreadable(e) => Err(e),
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(e) => Err(e),
    }
}

pub(crate) fn route(
    loaded: &Loaded,
    task: &str,
    purpose: &str,
    role: &str,
    host: &str,
    selected: Option<&str>,
) -> Result<Value, String> {
    if !text(task, 1024)
        || !text(purpose, 128)
        || !matches!(role, "worker" | "reviewer" | "adviser")
        || !matches!(host, "codex" | "claude")
    {
        return Err("route requires bounded task/purpose, worker|reviewer|adviser role and codex|claude host".into());
    }
    let mut eligible = Vec::new();
    let mut excluded = Vec::new();
    for (name, agent) in &loaded.agents {
        let reason = if !loaded.roles_for(name).contains(role) {
            Some("role_not_configured")
        } else if !agent
            .purpose
            .to_lowercase()
            .contains(&purpose.to_lowercase())
        {
            Some("purpose_not_matched")
        } else if agent.runtime.host.as_deref().is_some_and(|h| h != host) {
            Some("primary_host_incompatible")
        } else if loaded
            .agents
            .iter()
            .filter(|(other, a)| a.native_name(other) == agent.native_name(name))
            .count()
            != 1
        {
            Some("native_mapping_ambiguous")
        } else {
            None
        };
        if let Some(reason) = reason {
            // Only configured identity and eligibility, never inaccessible resources.
            excluded.push(json!({"agent":name,"reason":reason}));
        } else {
            eligible.push(
                json!({"agent":name,"purpose":agent.purpose,"nativeName":agent.native_name(name),
                "reason":"configured role, literal purpose match and compatible requested host"}),
            );
        }
    }
    if eligible.len() + excluded.len() > 32 {
        return Err(
            "route supports at most 32 configured agents; use an explicit bounded project".into(),
        );
    }
    let chosen = if let Some(name) = selected {
        if !eligible.iter().any(|v| v["agent"] == name) {
            return Err(
                "selected agent is not eligible; no contract or runtime was substituted".into(),
            );
        }
        Some(name)
    } else if eligible.len() == 1 {
        eligible[0]["agent"].as_str()
    } else {
        None
    };
    let snapshot = chosen
        .map(|name| snapshot(loaded, name, task, role, host))
        .transpose()?;
    memory::ensure_config_current(loaded)?;
    let result = json!({"version":1,"effect":"no-change",
        "state":if chosen.is_some() {"selected"} else if eligible.is_empty() {"lead_held"} else {"lead_choice"},
        "task":task,"role":role,"requestedHost":host,"eligible":eligible,"excluded":excluded,
        "selected":chosen,"snapshot":snapshot,
        "meaning":"Lead supplies semantic purpose and observed host availability; literal eligibility grants no rights, invokes no host and changes no active Work. Read the current snapshot at each fresh launch."});
    if serde_json::to_vec(&result)
        .map_err(|e| e.to_string())?
        .len()
        > SNAPSHOT_LIMIT
    {
        return Err("specialist route exceeds 16384 bytes; narrow the selected context".into());
    }
    Ok(result)
}

fn snapshot(
    loaded: &Loaded,
    name: &str,
    task: &str,
    role: &str,
    host: &str,
) -> Result<Value, String> {
    let agent = loaded
        .agent(name)
        .ok_or("selected specialist disappeared")?;
    let profile = read(&loaded.control_root, &agent.profile)?;
    let profile_text =
        std::str::from_utf8(&profile).map_err(|_| "specialist profile must be UTF-8")?;
    let references = memory::selection::resolve_for_task(loaded, name, Some(task))?;
    let scope = format!("specialist.{name}.v1");
    let project = crate::host::assignment_context::project_identity(loaded)?;
    let mut resources = Vec::new();
    let mut ids = BTreeSet::new();
    for reference in &references {
        if reference["scope"] != scope {
            continue;
        }
        let source = reference["sourcePath"]
            .as_str()
            .ok_or("invalid specialist source")?;
        let bytes = read(&loaded.product_root, source)?;
        if reference["sourceSha256"] != hash::bytes(&bytes) {
            return Err("specialist experience changed; no snapshot delivered".into());
        }
        let item: Experience = serde_json::from_slice(&bytes)
            .map_err(|_| "invalid specialist experience; inspect its authorized source")?;
        validate(&item, name, &project)?;
        let ledger = memory::snapshot(
            loaded,
            reference["ledgerPath"]
                .as_str()
                .ok_or("invalid specialist ledger")?,
        )?;
        memory::validate_history_current(loaded, &ledger)?;
        let proposed = ledger.events.first().ok_or("empty specialist history")?;
        let reviewer = ledger.events.iter().find(|e| e["action"] == "review");
        let promoter = ledger.events.iter().find(|e| e["action"] == "promote");
        let owned = proposed["actor"] == name && proposed["action"] == "propose";
        let reviewed = reviewer.is_some_and(|e| e["actor"].as_str().is_some_and(|a| a != name));
        let promoted = promoter.is_some_and(|e| e["actor"].as_str() == loaded.lead());
        if !owned || !reviewed || !promoted || proposed["expiresAt"].as_str().is_none() {
            return Err("specialist experience needs attributable author, independent review, Lead promotion and explicit expiry".into());
        }
        // Duplicate accepted revisions are a conflict, including inapplicable ones.
        // Retire the predecessor through the existing lifecycle before adoption.
        if !ids.insert(item.id.clone()) {
            return Err(
                "conflicting accepted specialist revisions; Lead must retire the predecessor"
                    .into(),
            );
        }
        if !item.hosts.iter().any(|h| h == host)
            || !item
                .task_terms
                .iter()
                .any(|term| task.to_lowercase().contains(&term.to_lowercase()))
            || !item
                .required_commands
                .iter()
                .all(|c| agent.commands.contains(c))
        {
            continue;
        }
        let mut current = true;
        for guard in &item.guards {
            let bytes = read(&loaded.product_root, &guard.path)?;
            if hash::bytes(&bytes) != guard.sha256 {
                current = false;
            }
        }
        if !current {
            continue;
        }
        resources.push(json!({"id":item.id,"revision":item.revision,"kind":item.kind,
            "reference":reference,"content":serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())?,
            "coverage":"complete","state":"qualified_snapshot"}));
    }
    if resources.len() > 3 {
        return Err("more than three applicable specialist resources; narrow the policy".into());
    }
    if read(&loaded.control_root, &agent.profile)? != profile
        || memory::selection::resolve_for_task(loaded, name, Some(task))? != references
    {
        return Err("specialist inputs changed during snapshot; refresh before launch".into());
    }
    let value = json!({"projectIdentity":project,"configurationSha256":hash::text(&loaded.source),
        "agent":name,"nativeName":agent.native_name(name),"role":role,"task":task,"host":host,
        "purpose":agent.purpose,"runtime":agent.runtime_value(),"declaredBoundary":agent.boundary_value(),
        "profile":{"path":agent.profile,"sha256":hash::bytes(&profile),"content":profile_text},
        "experience":resources,
        "delivery":"projected; native execution, acknowledgement and observed use require separate evidence"});
    let mut result = value.clone();
    result["snapshotId"] = json!(hash::value(&value));
    Ok(result)
}

fn validate(item: &Experience, name: &str, project: &str) -> Result<(), String> {
    if item.version != 1
        || !identifier(&item.id)
        || !text(&item.revision, 64)
        || !matches!(item.kind.as_str(), "observation" | "skill")
        || item.author != name
        || item.project_identity != project
        || !list(&item.task_terms, 4, 64)
        || !list(&item.hosts, 2, 16)
        || item
            .hosts
            .iter()
            .any(|h| !matches!(h.as_str(), "codex" | "claude"))
        || !text(&item.summary, 512)
        || !list(&item.preconditions, 8, 512)
        || !list(&item.procedure, 12, 512)
        || !list(&item.verification, 8, 512)
        || !list(&item.evidence, 8, 256)
        || item.required_commands.len() > 8
        || item.required_commands.iter().any(|c| !text(c, 256))
        || item.guards.len() > 4
        || item.guards.iter().any(|g| {
            !text(&g.path, 256)
                || g.sha256.len() != 64
                || !g
                    .sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        })
    {
        return Err(
            "specialist experience identity, applicability or bounded procedure is invalid".into(),
        );
    }
    Ok(())
}
