//! Machine-owned task choices in the existing state artifact owner. No new rights ledger.
use super::contextual::{self, Choice, Context, Resource, CHOICES, CONTEXT_BYTES};
use crate::{
    config::Loaded,
    evidence::hash,
    project::{managed_files, path},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::PathBuf};

const REVISIONS: usize = 64;
const TASKS: usize = 128;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: u32,
    project_identity: String,
    configuration_sha256: String,
    host: String,
    sequence: usize,
    previous_sha256: String,
    request_sha256: String,
    request_input_sha256: String,
    reason: String,
    context: Context,
}
struct Saved {
    record: Record,
    sha: String,
}

fn root(l: &Loaded) -> Result<PathBuf, String> {
    let policy = super::policy::get(&l.config)
        .and_then(|p| p.contextual)
        .ok_or("memory choice requires explicit contextual version 2 policy")?;
    let expected = format!(
        "{}/artifacts/memory-choices",
        crate::project::layout_types::state_namespace()
    );
    if policy.version != 2 || policy.source_path != expected {
        return Err(
            "memory choice requires version 2 and its fixed StateRoot artifact namespace".into(),
        );
    }
    Ok(l.state_root.join(expected))
}
fn key(l: &Loaded, agent: &str, task: &str) -> Result<String, String> {
    if !contextual::text(task, 1024) || !contextual::text(agent, 64) || l.agent(agent).is_none() {
        return Err(
            "memory choice requires a configured recipient and bounded approved task".into(),
        );
    }
    Ok(hash::value(
        &json!({"project":crate::host::assignment_context::project_identity(l)?,
        "config":hash::text(&l.source),"task":task,"agent":agent}),
    ))
}
fn directory(l: &Loaded, agent: &str, task: &str) -> Result<PathBuf, String> {
    Ok(root(l)?.join(key(l, agent, task)?))
}
fn bytes(l: &Loaded, target: &std::path::Path) -> Result<Vec<u8>, String> {
    let rel = target
        .strip_prefix(&l.state_root)
        .map_err(|_| "choice path escapes StateRoot")?
        .to_str()
        .ok_or("choice path is not UTF-8")?;
    match path::secure_bytes_observation_single_link_bounded(
        &l.state_root,
        rel,
        "task memory choice",
        CONTEXT_BYTES,
    ) {
        path::SecureBytesResult::Bytes(b) => Ok(b),
        _ => Err("task memory choice unavailable, aliased, unsafe or oversized".into()),
    }
}
fn history(l: &Loaded, agent: &str, task: &str) -> Result<Vec<Saved>, String> {
    let dir = directory(l, agent, task)?;
    // Keep lexical components: canonicalizing first would erase an alias.
    let relative = dir
        .strip_prefix(&l.state_root)
        .map_err(|_| "choice directory escapes StateRoot")?;
    let mut ancestor = l.state_root.clone();
    for component in relative.components() {
        ancestor.push(component);
        match fs::symlink_metadata(&ancestor) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => return Err("task memory choice directory is unsafe".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        paths.push(entry.path());
        if paths.len() > REVISIONS {
            return Err("task memory choice history exceeds 64 revisions".into());
        }
    }
    paths.sort();
    let mut saved: Vec<Saved> = vec![];
    for (index, p) in paths.iter().enumerate() {
        if p.file_name().and_then(|s| s.to_str()) != Some(format!("{:04}.json", index + 1).as_str())
        {
            return Err("task memory choice history is incomplete or has an unknown entry".into());
        }
        let b = bytes(l, p)?;
        let r: Record =
            serde_json::from_slice(&b).map_err(|_| "invalid task memory choice record")?;
        let c = &r.context;
        let previous = saved.last().map_or("absent", |s| s.sha.as_str());
        if r.version != 2
            || r.sequence != index + 1
            || r.previous_sha256 != previous
            || r.project_identity != crate::host::assignment_context::project_identity(l)?
            || r.configuration_sha256 != hash::text(&l.source)
            || !matches!(r.host.as_str(), "codex" | "claude")
            || !contextual::digest(&r.request_sha256)
            || !contextual::digest(&r.request_input_sha256)
            || !contextual::text(&r.reason, 512)
            || c.version != 1
            || c.task != task
            || c.revision != format!("r{}", index + 1)
            || Some(c.approved_by.as_str()) != l.lead()
            || c.selections.len() != 1
            || c.selections[0].agent != agent
            || c.selections[0].resources.len() > CHOICES
        {
            return Err(
                "task memory choice ownership, revision or request identity differs".into(),
            );
        }
        let mut ids = BTreeSet::new();
        if c.selections[0].resources.iter().any(|r| {
            !contextual::digest(&r.item_id)
                || !contextual::digest(&r.source_sha256)
                || !contextual::digest(&r.ledger_head_sha256)
                || !contextual::text(&r.reason, 512)
                || !ids.insert(&r.item_id)
        }) {
            return Err("invalid task memory choice resources".into());
        }
        let items: Vec<_> = c.selections[0]
            .resources
            .iter()
            .map(|r| r.item_id.clone())
            .collect();
        let input = request_input(
            l,
            agent,
            task,
            &r.host,
            &r.reason,
            &items,
            &r.previous_sha256,
        )?;
        if input != r.request_input_sha256
            || hash::value(&json!({"input":input,"context":c,"host":r.host})) != r.request_sha256
            || c.selections[0]
                .resources
                .iter()
                .any(|resource| resource.reason != r.reason)
        {
            return Err("task memory choice request does not bind its recorded bytes".into());
        }
        saved.push(Saved {
            record: r,
            sha: hash::bytes(&b),
        });
    }
    super::ensure_config_current(l)?;
    Ok(saved)
}

pub(super) fn context(
    l: &Loaded,
    agent: &str,
    task: &str,
) -> Result<Option<(Context, String)>, String> {
    let mut h = history(l, agent, task)?;
    let Some(s) = h.pop() else {
        return Ok(None);
    };
    let a = l.agent(agent).ok_or("choice recipient unavailable")?;
    if a.runtime
        .host
        .as_deref()
        .is_some_and(|host| host != s.record.host)
    {
        return Err("task memory choice host differs from configured recipient".into());
    }
    Ok(Some((s.record.context, s.sha)))
}
/// Actual consumer host, including host-managed children, must match the prepared choice.
pub(crate) fn assert_host(l: &Loaded, agent: &str, task: &str, host: &str) -> Result<(), String> {
    if !contextual::task_local(l) {
        return Ok(());
    }
    if let Some(saved) = history(l, agent, task)?.last() {
        if saved.record.host != host {
            return Err(
                "task memory choice actual host differs; no selected guidance was delivered".into(),
            );
        }
    }
    Ok(())
}
fn response(
    l: &Loaded,
    agent: &str,
    task: &str,
    saved: &Saved,
    current: &str,
    effect: &str,
) -> Value {
    let binding = contextual::binding(l, agent, task);
    let exact = binding
        .as_ref()
        .is_ok_and(|v| v["contextSha256"] == saved.sha);
    let confirmed = history(l, agent, task)
        .ok()
        .and_then(|h| h.last().map(|s| s.sha.clone()));
    let current = saved.sha == current && confirmed.as_deref() == Some(saved.sha.as_str());
    let usable = current && exact;
    json!({"version":2,"effect":effect,"record":saved.record,"contextSha256":saved.sha,
        "current":current,"currentlyUsable":usable,
        "meaning":"Recorded choice only; current eligibility and each Work's frozen assignment remain mandatory. No adoption, launch, check or permission is replayed."})
}
pub(crate) fn read(
    l: &Loaded,
    agent: &str,
    task: &str,
    request: Option<&str>,
) -> Result<Value, String> {
    let h = history(l, agent, task)?;
    let Some(last) = h.last() else {
        return Ok(json!({"version":2,"effect":"no-change","contextSha256":null,"current":false}));
    };
    let s = if let Some(request) = request {
        h.iter()
            .find(|s| s.record.request_sha256 == request)
            .ok_or("choice request not recorded for this task and recipient")?
    } else {
        last
    };
    Ok(response(l, agent, task, s, &last.sha, "no-change"))
}

// Choice writers serialize only their own key. Existing catalog lifecycle keeps its owner.
#[cfg(unix)]
fn lock_file(l: &Loaded, name: &str) -> Result<fs::File, String> {
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
    let dir = l
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("locks");
    managed_files::ensure_state_directory(&l.state_root, &dir)?;
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(name))
        .map_err(|e| e.to_string())?;
    if !f.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("choice lock is not a regular file".into());
    }
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("task memory choice is being changed; read back before retry".into());
    }
    Ok(f)
}
#[cfg(not(unix))]
fn lock_file(_: &Loaded, _: &str) -> Result<fs::File, String> {
    Err("task memory choice requires a POSIX state host".into())
}

pub(crate) struct Request<'a> {
    pub agent: &'a str,
    pub task: &'a str,
    pub host: &'a str,
    pub reason: &'a str,
    pub items: &'a [String],
    pub previous: &'a str,
    pub apply: bool,
    pub expected: Option<&'a str>,
}
fn request_input(
    l: &Loaded,
    agent: &str,
    task: &str,
    host: &str,
    reason: &str,
    items: &[String],
    previous: &str,
) -> Result<String, String> {
    Ok(hash::value(
        &json!({"key":key(l,agent,task)?,"host":host,"reason":reason,"items":items,"previous":previous}),
    ))
}
pub(crate) fn choose(l: &Loaded, q: Request<'_>) -> Result<Value, String> {
    let Request {
        agent,
        task,
        host,
        reason,
        items,
        previous,
        apply,
        expected,
    } = q;
    root(l)?;
    key(l, agent, task)?;
    if !contextual::text(reason, 512)
        || !matches!(host, "codex" | "claude")
        || (previous != "absent" && !contextual::digest(previous))
        || items.len() > CHOICES
        || items.iter().collect::<BTreeSet<_>>().len() != items.len()
    {
        return Err("memory choose requires a bounded Lead reason, host, unique eligible items and previous digest|absent".into());
    }
    let _lock = if apply {
        Some(lock_file(
            l,
            &format!("memory-choice-{}.lock", key(l, agent, task)?),
        )?)
    } else {
        None
    };
    let _catalog = if apply {
        super::lessons::mutation_lock(l)?
    } else {
        None
    };
    let h = history(l, agent, task)?;
    let current = h.last().map_or("absent", |s| s.sha.as_str());
    let input = request_input(l, agent, task, host, reason, items, previous)?;
    if apply {
        let token = expected.ok_or("apply requires the exact preview --expected-sha256")?;
        if let Some(saved) = h.iter().find(|s| s.record.request_sha256 == token) {
            if saved.record.request_input_sha256 != input {
                return Err("choice replay intent differs from recorded request".into());
            }
            return Ok(response(l, agent, task, saved, current, "replayed"));
        }
    }
    if current != previous {
        return Err("task memory choice expected-version conflict; read current choice before preparing another revision".into());
    }
    if h.len() >= REVISIONS {
        return Err("task memory choice history is full; preserve its records".into());
    }
    let candidate = contextual::candidates(l, agent, task, host, None)?;
    let a = l.agent(agent).ok_or("choice recipient unavailable")?;
    if a.runtime.host.as_deref() != Some(host) {
        return Err(
            "version 2 choice requires the exact explicit configured recipient host".into(),
        );
    }
    let mut choice: Choice =
        serde_json::from_value(candidate["choiceTemplate"].clone()).map_err(|e| e.to_string())?;
    for id in items {
        let r = candidate["candidates"]
            .as_array()
            .ok_or("candidate entries unavailable")?
            .iter()
            .map(|c| &c["reference"])
            .find(|r| r["itemId"] == *id)
            .ok_or("selected item is not eligible for the task recipient and host")?;
        choice.resources.push(Resource {
            item_id: id.clone(),
            source_sha256: r["sourceSha256"]
                .as_str()
                .ok_or("source digest unavailable")?
                .into(),
            ledger_head_sha256: r["ledgerHeadSha256"]
                .as_str()
                .ok_or("ledger digest unavailable")?
                .into(),
            reason: reason.into(),
        });
    }
    let context = Context {
        version: 1,
        task: task.into(),
        revision: format!("r{}", h.len() + 1),
        approved_by: l.lead().ok_or("configured Lead unavailable")?.into(),
        selections: vec![choice],
    };
    let prepared = json!({"input":input,"context":context,"host":host});
    let token = hash::value(&prepared);
    let record = Record {
        version: 2,
        project_identity: crate::host::assignment_context::project_identity(l)?,
        configuration_sha256: hash::text(&l.source),
        host: host.into(),
        sequence: h.len() + 1,
        previous_sha256: previous.into(),
        request_sha256: token.clone(),
        request_input_sha256: input,
        reason: reason.into(),
        context,
    };
    let b = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    if b.len() > CONTEXT_BYTES as usize {
        return Err("task memory choice exceeds 8192 bytes".into());
    }
    let dir = directory(l, agent, task)?;
    let target = dir.join(format!("{:04}.json", h.len() + 1));
    let rel = target
        .strip_prefix(&l.state_root)
        .map_err(|e| e.to_string())?
        .to_str()
        .ok_or("choice path is not UTF-8")?;
    let mut argv = vec![
        crate::emitted::executable()?,
        "memory".into(),
        "choose".into(),
        agent.into(),
    ];
    argv.extend(items.iter().cloned());
    argv.extend([
        "--task".into(),
        task.into(),
        "--host".into(),
        host.into(),
        "--reason".into(),
        reason.into(),
        "--previous".into(),
        previous.into(),
        "--apply".into(),
        "--expected-sha256".into(),
        token.clone(),
        "--json".into(),
        "--config".into(),
        l.path.to_str().ok_or("config is not UTF-8")?.into(),
    ]);
    if !apply {
        return Ok(
            json!({"version":2,"effect":"no-change","expectedSha256":token,"record":record,"artifact":rel,"apply":{"command":argv},"authority":"Configured Lead applicability choice, not a new memory approval or host permission."}),
        );
    }
    if expected != Some(token.as_str()) {
        return Err("choice preview inputs changed; prepare again before apply".into());
    }
    super::ensure_config_current(l)?;
    // Refresh the candidate inventory before the one create-only effect.
    if contextual::candidates(l, agent, task, host, None)? != candidate {
        return Err("choice candidates changed during preparation".into());
    }
    let _allocation = lock_file(l, "memory-choice-allocation.lock")?;
    let namespace = root(l)?;
    managed_files::ensure_state_directory(&l.state_root, &namespace)?;
    if fs::read_dir(&namespace)
        .map_err(|e| e.to_string())?
        .take(TASKS + 1)
        .count()
        >= TASKS
        && !dir.exists()
    {
        return Err("task memory choice namespace exceeds 128 task/recipient identities".into());
    }
    managed_files::ensure_state_directory(&l.state_root, &dir)?;
    crate::project::git_preflight::refuse_tracked_targets(&l.state_root, &[target.as_path()])?;
    let mut file = path::secure_create_new(&l.state_root, rel, "task memory choice")?;
    use std::io::Write;
    file.write_all(&b)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    let stored = history(l, agent, task)?;
    let last = stored.last().ok_or("choice effect requires readback")?;
    if last.sha != hash::bytes(&b) {
        return Err("choice effect readback differs; inspect without retrying".into());
    }
    Ok(response(l, agent, task, last, &last.sha, "recorded"))
}
