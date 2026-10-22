//! Read-only specialist selection and qualified, agent-owned experience.
//! Existing configuration and memory lifecycle remain the sole writers.
use crate::{config::Loaded, evidence::hash, memory, project::path};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

mod aliases;
mod delivery;
mod form;

pub(crate) use aliases::reserved_source;
pub(crate) use delivery::{delivery, enabled, is_scope};
pub(crate) use form::{input_form, next_action};

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
        | path::SecureBytesResult::Unreadable(e) => {
            let _ = e;
            Err("specialist resource unavailable, unsafe or exceeds 4096 bytes".into())
        }
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(e) => Err(e),
    }
}

fn profile_bytes(loaded: &Loaded, name: &str) -> Result<Vec<u8>, String> {
    match path::secure_bytes_observation_bounded(
        &loaded.control_root,
        name,
        "specialist profile",
        64 * 1024,
    ) {
        path::SecureBytesResult::Bytes(bytes) => Ok(bytes),
        _ => Err("specialist profile unavailable, unsafe or exceeds 65536 bytes".into()),
    }
}

// Inspect lifecycle metadata before reading any experience content. Generic
// recall deliberately serves all of an agent's scopes; this consumer serves
// only its own reserved scope and must not read other eligible source bytes.
fn references(loaded: &Loaded, name: &str) -> Result<Vec<Value>, String> {
    let agent = loaded.agent(name).ok_or("specialist is not configured")?;
    let scope = format!("specialist.{name}.v1");
    let Some(policy) = memory::policy::get(&loaded.config) else {
        return Ok(Vec::new());
    };
    let allowed = agent.memory_read.contains(&scope)
        && match agent.cross_context.as_str() {
            "same-scope" => true,
            "protocol-only" => policy.protocol_scopes.contains(&scope),
            "synthetic-only" => policy.synthetic_scopes.contains(&scope),
            _ => false,
        };
    if !allowed {
        return Ok(Vec::new());
    }
    let ledgers = memory::discovery::discover(loaded)
        .map_err(|_| {
            "specialist lifecycle metadata unavailable; inspect the authorized memory store"
        })?
        .unwrap_or_default();
    let retired = ledgers
        .iter()
        .flat_map(|ledger| ledger.snapshot.items.values())
        .filter(|item| item["scope"] == scope && item["state"] == "revoked")
        .filter_map(|item| item["source"]["sha256"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut result = Vec::new();
    for ledger in ledgers {
        for item in ledger.snapshot.items.values() {
            if item["state"] != "accepted" || item["scope"] != scope {
                continue;
            }
            if item["source"]["sha256"]
                .as_str()
                .is_some_and(|digest| retired.contains(digest))
            {
                return Err(
                    "revoked specialist source cannot be reintroduced through another ledger"
                        .into(),
                );
            }
            let expires = item["expiresAt"]
                .as_str()
                .ok_or("specialist experience requires explicit expiry")?;
            if chrono::DateTime::parse_from_rfc3339(expires)
                .map_err(|_| "invalid specialist expiry")?
                <= chrono::Utc::now().fixed_offset()
            {
                continue;
            }
            memory::validate_history_current(loaded, &ledger.snapshot)
                .map_err(|_| "specialist recorded policy or actor profile changed")?;
            result.push(json!({"itemId":item["itemId"],"scope":scope,
                "sourcePath":item["source"]["path"],"sourceSha256":item["source"]["sha256"],
                "ledgerPath":ledger.path,"ledgerHeadSha256":ledger.snapshot.last_event_sha256,
                "state":"accepted"}));
            if result.len() > policy.max_items {
                return Err("specialist experience exceeds the configured recall budget".into());
            }
        }
    }
    Ok(result)
}

/// Candidate metadata under the same recipient/host/lifecycle eligibility, without activation.
pub(crate) fn candidate_references(
    loaded: &Loaded,
    name: &str,
    role: &str,
    task: &str,
    host: &str,
) -> Result<Vec<Value>, String> {
    if !enabled(loaded, name) || delivery::exclusion(loaded, name, role, host).is_some() {
        return Ok(Vec::new());
    }
    let value = delivery::select_resources(loaded, name, task, role, host, true, || Ok(()))?;
    Ok(value["experience"]
        .as_array()
        .ok_or("invalid specialist candidates")?
        .iter()
        .map(|r| r["reference"].clone())
        .collect())
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
        let reason = if !agent
            .purpose
            .to_lowercase()
            .contains(&purpose.to_lowercase())
        {
            Some("purpose_not_matched")
        } else {
            delivery::exclusion(loaded, name, role, host)
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
    snapshot_after_source(loaded, name, task, role, host, || Ok(()))
}

fn snapshot_after_source(
    loaded: &Loaded,
    name: &str,
    task: &str,
    role: &str,
    host: &str,
    after_source: impl FnMut() -> Result<(), String>,
) -> Result<Value, String> {
    let agent = loaded
        .agent(name)
        .ok_or("selected specialist disappeared")?;
    let profile = profile_bytes(loaded, &agent.profile)?;
    let profile_text =
        std::str::from_utf8(&profile).map_err(|_| "specialist profile must be UTF-8")?;
    let selected = delivery::select_after_source(loaded, name, task, role, host, after_source)?;
    if profile_bytes(loaded, &agent.profile)? != profile {
        return Err("specialist profile changed during snapshot; refresh before launch".into());
    }
    let profile_access = if profile.len() <= 4096 {
        Value::Null
    } else {
        json!({"command":[crate::emitted::executable()?,"profile",name,"--config",loaded.path.to_str().ok_or("configuration path is not UTF-8")?],
            "sameConfigRequired":true,"sameExecutableRequired":true,"readOnly":true})
    };
    let value = json!({"projectIdentity": selected["projectIdentity"],"configurationSha256":hash::text(&loaded.source),
        "agent":name,"nativeName":agent.native_name(name),"role":role,"task":task,"host":host,
        "purpose":agent.purpose,"runtime":agent.runtime_value(),"declaredBoundary":agent.boundary_value(),
        "profile":{"path":agent.profile,"sha256":hash::bytes(&profile),
            "content":if profile.len() <= 4096 { Some(profile_text) } else { None },
            "complete":profile.len() <= 4096,
            "access":profile_access},
        "experience":selected["experience"],
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
                || std::path::Path::new(&g.path).is_absolute()
                || !std::path::Path::new(&g.path).components().all(|part| {
                    matches!(
                        part,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                })
                || !std::path::Path::new(&g.path)
                    .components()
                    .any(|part| matches!(part, std::path::Component::Normal(_)))
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

fn parse(loaded: &Loaded, bytes: &[u8], name: &str, project: &str) -> Result<Experience, String> {
    if bytes.len() > SOURCE_LIMIT as usize {
        return Err("specialist source exceeds 4096 UTF-8 bytes".into());
    }
    let item: Experience = serde_json::from_slice(bytes).map_err(|_| {
        let scope = format!("specialist.{name}.v1");
        let form = crate::emitted::command_line(
            &["memory", "form", "--scope", &scope],
            Some(&loaded.path),
        );
        format!("invalid specialist source; use {form}: guards are file path/sha256 objects; prose restrictions belong in preconditions")
    })?;
    validate(&item, name, project)?;
    Ok(item)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn excluded_item_late_ledger_and_history_failures_do_not_disclose_private_names() {
        for mutate_ledger in [true, false] {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "specialist-late-read-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            assert!(Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["init", "-q"])
                .status()
                .unwrap()
                .success());
            let scope = "specialist.specialist.v1";
            let mut agents = json!({});
            for name in ["lead", "specialist", "reviewer"] {
                fs::write(
                    root.join(format!("{name}.md")),
                    "Bounded selected profile.\n",
                )
                .unwrap();
                agents[name] = json!({"profile":format!("{name}.md"),"purpose":"record aggregation",
                    "observe":[],"write":[],"commands":["python3"],"skills":[],"memoryForget":[],
                    "memoryRead":[],"memoryWrite":[],"retention":"explicit-expiry","crossContext":"protocol-only"});
            }
            agents["specialist"]["memoryRead"] = json!([scope]);
            agents["specialist"]["memoryWrite"] = json!([scope]);
            agents["reviewer"]["memoryReview"] = json!([scope]);
            agents["lead"]["memoryPromote"] = json!([scope]);
            let c = json!({"version":1,"project":{"root":"."},"agents":agents,
                "orchestration":{"lead":"lead","maxParallel":1},
                "workflows":{"change":{"workers":["specialist"],"reviewers":["reviewer"]}},
                "memory":{"root":".exitbind/memory","maxItems":4,"maxBytes":8192,"protocolScopes":[scope],"syntheticScopes":[]}});
            let config = root.join("exitbind.json");
            fs::write(&config, c.to_string()).unwrap();
            let loaded = crate::config::load(config.to_str()).unwrap();
            let source = json!({"version":1,"id":"past-method","revision":"r1","kind":"skill",
                "author":"specialist","projectIdentity":crate::host::assignment_context::project_identity(&loaded).unwrap(),
                "taskTerms":["different task"],"hosts":["codex"],"summary":"Prior qualified method.",
                "preconditions":["Applicable schema"],"procedure":["Use presence accounting"],
                "verification":["Distinct input"],"requiredCommands":["python3"],"evidence":["past-task.md"],"guards":[]});
            fs::write(root.join("private-source-name.json"), source.to_string()).unwrap();
            let ledger = ".exitbind/memory/private-ledger-name.jsonl";
            fs::create_dir_all(root.join(".exitbind/memory")).unwrap();
            memory::action(
                &loaded,
                "specialist",
                "propose",
                Some("private-source-name.json"),
                Some(scope),
                ledger,
                Some("2100-01-01T00:00:00Z"),
            )
            .unwrap();
            memory::action(&loaded, "reviewer", "review", None, None, ledger, None).unwrap();
            memory::action(&loaded, "lead", "promote", None, None, ledger, None).unwrap();
            let result = snapshot_after_source(
                &loaded,
                "specialist",
                "current task",
                "worker",
                "codex",
                || {
                    if mutate_ledger {
                        fs::remove_file(root.join(ledger)).unwrap();
                        std::os::unix::fs::symlink("absent", root.join(ledger)).unwrap();
                    } else {
                        fs::write(
                            root.join("reviewer.md"),
                            "Changed after initial metadata verification.\n",
                        )
                        .unwrap();
                    }
                    Ok(())
                },
            );
            let error = result.unwrap_err();
            assert!(!error.contains("private-source-name"));
            assert!(!error.contains("private-ledger-name"));
            assert!(!error.contains(root.to_str().unwrap()));
            assert!(error.contains("specialist"));
            fs::remove_dir_all(root).unwrap();
        }
    }
}
