//! Opt-in choices in the existing approved task context, never another authority ledger.
use crate::{config::Loaded, evidence::hash, project::path};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) const CONTEXT_BYTES: u64 = 8192;
pub(super) const CHOICES: usize = 8;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Policy {
    pub(crate) version: u32,
    pub(crate) source_path: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Context {
    pub(super) version: u32,
    pub(super) task: String,
    pub(super) revision: String,
    pub(super) approved_by: String,
    pub(super) selections: Vec<Choice>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Choice {
    pub(super) agent: String,
    pub(super) role: String,
    pub(super) purpose: String,
    pub(super) profile_sha256: String,
    pub(super) resources: Vec<Resource>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Resource {
    pub(super) item_id: String,
    pub(super) source_sha256: String,
    pub(super) ledger_head_sha256: String,
    pub(super) reason: String,
}
pub(super) fn text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
pub(super) fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub(crate) fn valid_policy(value: &Value) -> bool {
    serde_json::from_value::<Policy>(value.clone()).is_ok_and(|p| {
        matches!(p.version, 1 | 2)
            && (p.version == 1
                || matches!(
                    p.source_path.as_str(),
                    ".exitbind/artifacts/memory-choices" | ".soulmate/artifacts/memory-choices"
                ))
            && p.source_path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && text(&p.source_path, 256)
            && !std::path::Path::new(&p.source_path).is_absolute()
            && std::path::Path::new(&p.source_path)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
    })
}
pub(crate) fn enabled(l: &Loaded) -> bool {
    super::policy::get(&l.config).is_some_and(|p| p.contextual.is_some())
}
fn bounded(l: &Loaded, source: &str, max: u64) -> Result<Vec<u8>, String> {
    match path::secure_bytes_observation_bounded(
        &l.product_root,
        source,
        "contextual memory source",
        max,
    ) {
        path::SecureBytesResult::Bytes(b) => Ok(b),
        _ => Err("contextual memory source unavailable, unsafe or exceeds its byte bound".into()),
    }
}
fn context(
    l: &Loaded,
    agent: &str,
    task: Option<&str>,
) -> Result<Option<(Context, String)>, String> {
    let Some(p) = super::policy::get(&l.config).and_then(|p| p.contextual) else {
        return Ok(None);
    };
    if p.version == 2 {
        return task
            .map(|task| super::choices::context(l, agent, task))
            .transpose()
            .map(Option::flatten);
    }
    let bytes = bounded(l, &p.source_path, CONTEXT_BYTES)?;
    let c: Context = serde_json::from_slice(&bytes)
        .map_err(|_| "invalid contextual memory task context shape")?;
    let mut agents = BTreeSet::new();
    if c.version != 1
        || !text(&c.task, 1024)
        || !text(&c.revision, 64)
        || Some(c.approved_by.as_str()) != l.lead()
        || c.selections.len() > CHOICES
    {
        return Err(
            "invalid contextual memory task, revision, configured Lead or selection count".into(),
        );
    }
    for choice in &c.selections {
        let mut ids = BTreeSet::new();
        if !text(&choice.agent, 64)
            || !text(&choice.purpose, 1024)
            || !matches!(
                choice.role.as_str(),
                "lead" | "worker" | "reviewer" | "adviser"
            )
            || !digest(&choice.profile_sha256)
            || !agents.insert(&choice.agent)
            || choice.resources.len() > CHOICES
            || choice.resources.iter().any(|r| {
                !digest(&r.item_id)
                    || !digest(&r.source_sha256)
                    || !digest(&r.ledger_head_sha256)
                    || !text(&r.reason, 512)
                    || !ids.insert(&r.item_id)
            })
        {
            return Err("invalid contextual memory recipient or bounded resource identity".into());
        }
    }
    super::ensure_config_current(l)?;
    if bounded(l, &p.source_path, CONTEXT_BYTES)? != bytes {
        return Err("approved memory context changed during observation".into());
    }
    Ok(Some((c, hash::bytes(&bytes))))
}
pub(crate) fn unique_role(l: &Loaded, agent: &str) -> Result<String, String> {
    let roles = l.roles_for(agent);
    if roles.len() != 1 {
        return Err("contextual memory requires one unambiguous configured recipient role".into());
    }
    Ok((*roles.iter().next().ok_or("recipient role unavailable")?).to_owned())
}

/// Freeze only the selected approved-context identity in the existing assignment.
pub(crate) fn binding(l: &Loaded, agent: &str, task: &str) -> Result<Value, String> {
    let Some((c, sha)) = context(l, agent, Some(task))? else {
        return Ok(Value::Null);
    };
    let choice = c
        .selections
        .iter()
        .find(|s| s.agent == agent && (task_local(l) || !s.resources.is_empty()));
    if c.task != task || choice.is_none() {
        return Ok(Value::Null);
    }
    let role = unique_role(l, agent)?;
    let configured = l
        .agent(agent)
        .ok_or("contextual recipient is unavailable")?;
    validate_choice(l, choice.ok_or("contextual choice unavailable")?)?;
    let mut eligible = super::selection::candidate_references(l, agent, Some(task))?;
    // A Work may precede its actual receiving host. An unspecified primary is
    // no default host: freeze eligible identities across supported hosts and
    // leave the actual-host restriction to the existing delivery consumer.
    let hosts = configured
        .runtime
        .host
        .as_deref()
        .map_or_else(|| vec!["codex", "claude"], |host| vec![host]);
    for host in hosts {
        for reference in
            crate::project::specialists::candidate_references(l, agent, &role, task, host)?
        {
            if !eligible.iter().any(|current| current == &reference) {
                eligible.push(reference);
            }
        }
    }
    for resource in &choice.ok_or("contextual choice unavailable")?.resources {
        let reference = eligible
            .iter()
            .find(|r| r["itemId"] == resource.item_id)
            .ok_or("contextual memory selected resource is no longer eligible")?;
        identity(l, agent, Some(task), reference)?
            .ok_or("contextual memory selected resource has no current choice")?;
    }
    if context(l, agent, Some(task))?
        .map(|(_, current)| current)
        .as_deref()
        != Some(sha.as_str())
    {
        return Err("contextual memory selection changed during binding".into());
    }
    Ok(
        json!({"version":if task_local(l) {2} else {1},"contextSha256":sha,"contextRevision":c.revision,
        "agent":agent,"role":role}),
    )
}

/// Mutation and check consumers keep their existing APIs; only opted-in memory
/// requires its frozen choice and eligible resource version to remain current.
pub(crate) fn assert_plan(l: &Loaded, state: &Value) -> Result<(), String> {
    if !enabled(l) {
        return Ok(());
    }
    for stage in state["plan"]["stages"]
        .as_array()
        .ok_or("contextual memory plan stages unavailable")?
    {
        for selected in stage["agents"]
            .as_array()
            .ok_or("contextual memory plan recipients unavailable")?
        {
            let mut packet = json!({"agent":selected["name"],"goal":state["goal"],
                "memoryContextSelection":selected["memoryContextSelection"],
                "memoryReferences":selected["memoryReferences"]});
            if crate::run::guidance_continuity::enabled(state) {
                packet["guidanceContinuityProtocol"] = state["guidanceContinuityProtocol"].clone();
                packet["frozenMemoryRead"] = selected["declaredBoundary"]["memoryRead"].clone();
                packet["frozenCrossContext"] = selected["declaredBoundary"]["crossContext"].clone();
            }
            assert_assignment(l, &packet)?;
        }
    }
    Ok(())
}

pub(crate) fn assert_assignment(l: &Loaded, packet: &Value) -> Result<(), String> {
    if crate::run::guidance_continuity::enabled(packet) {
        let agent = packet["agent"]
            .as_str()
            .ok_or("contextual assignment recipient unavailable")?;
        let task = packet["goal"]
            .as_str()
            .ok_or("contextual assignment task unavailable")?;
        let ceiling = packet["frozenMemoryRead"]
            .as_array()
            .ok_or("marked assignment has no frozen memory-read ceiling")?
            .iter()
            .map(|scope| {
                scope
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("marked assignment memory-read ceiling is malformed")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cross_context = packet["frozenCrossContext"]
            .as_str()
            .ok_or("marked assignment cross-context ceiling is malformed")?;
        // A changed valid choice is advisory, but a frozen choice whose
        // approved artifact is missing, corrupt or aliased is not.
        if enabled(l)
            && packet
                .get("memoryContextSelection")
                .is_some_and(|value| !value.is_null())
            && context(l, agent, Some(task))?.is_none()
        {
            return Err(
                "contextual memory choice is unavailable; preserve the recorded Work".into(),
            );
        }
        // The selector still validates current ledgers, current read rights,
        // contextual applicability, and every source hash. Only optional
        // source absence/change is omitted. The frozen scope ceiling prevents
        // later policy widening from adding a new memory class to this Work.
        super::selection::resolve_for_task_optional_with_ceiling(
            l,
            agent,
            Some(task),
            &ceiling,
            cross_context,
        )?;
        return Ok(());
    }
    if !enabled(l) {
        return Ok(());
    }
    let agent = packet["agent"]
        .as_str()
        .ok_or("contextual assignment recipient unavailable")?;
    let task = packet["goal"]
        .as_str()
        .ok_or("contextual assignment task unavailable")?;
    let expected = packet
        .get("memoryContextSelection")
        .ok_or("contextual assignment has no frozen selection identity")?;
    if expected != &binding(l, agent, task)? {
        return Err("contextual memory selection changed; preserve the recorded Work and use its supported successor before dependent use".into());
    }
    let current = super::selection::resolve_for_task(l, agent, Some(task))?;
    let stable_references = |references: &Value| {
        let mut references = references.clone();
        if let Some(items) = references.as_array_mut() {
            for item in items {
                if let Some(object) = item.as_object_mut() {
                    object.remove("sourcePath");
                    object.remove("ledgerPath");
                }
            }
        }
        references
    };
    if packet.get("memoryReferences").map(stable_references)
        != Some(stable_references(&json!(current)))
    {
        return Err("contextual memory eligibility changed; preserve the recorded Work before dependent use".into());
    }
    Ok(())
}
/// Eligibility remains owned by the existing consumer. This only filters its eligible exact version.
/// None means no adopted guidance; it never means a missing access right was granted.
pub(crate) fn identity(
    l: &Loaded,
    agent: &str,
    task: Option<&str>,
    reference: &Value,
) -> Result<Option<Value>, String> {
    let Some((c, sha)) = context(l, agent, task)? else {
        return Ok(None);
    };
    if task != Some(c.task.as_str()) {
        return Ok(None);
    }
    let Some(choice) = c.selections.iter().find(|s| s.agent == agent) else {
        return Ok(None);
    };
    validate_choice(l, choice)?;
    let role = unique_role(l, agent)?;
    let Some(r) = choice
        .resources
        .iter()
        .find(|r| reference["itemId"] == r.item_id)
    else {
        return Ok(None);
    };
    if reference["sourceSha256"] != r.source_sha256
        || reference["ledgerHeadSha256"] != r.ledger_head_sha256
    {
        return Err("contextual memory selected resource version changed; review its current eligible version".into());
    }
    Ok(Some(
        json!({"version":if task_local(l) {2} else {1},"contextSha256":sha,"contextRevision":c.revision,
        "agent":agent,"role":role,"reason":r.reason}),
    ))
}
fn validate_choice(l: &Loaded, choice: &Choice) -> Result<(), String> {
    let agent = &choice.agent;
    let a = l
        .agent(agent)
        .ok_or("contextual memory recipient is not configured")?;
    let role = unique_role(l, agent)?;
    let profile = path::secure_bytes(&l.control_root, &a.profile, "contextual recipient profile")?;
    if choice.role != role
        || choice.purpose != a.purpose
        || choice.profile_sha256 != hash::bytes(&profile)
    {
        return Err("contextual memory recipient role, purpose or profile changed; review the current task choice".into());
    }
    Ok(())
}
pub(crate) fn task_local(l: &Loaded) -> bool {
    super::policy::get(&l.config)
        .and_then(|p| p.contextual)
        .is_some_and(|p| p.version == 2)
}
pub(crate) fn valid_identity(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.len() == 6)
        && matches!(v["version"].as_u64(), Some(1 | 2))
        && v["contextSha256"].as_str().is_some_and(digest)
        && v["contextRevision"].as_str().is_some_and(|s| text(s, 64))
        && v["agent"].as_str().is_some_and(|s| text(s, 64))
        && v["role"]
            .as_str()
            .is_some_and(|s| matches!(s, "lead" | "worker" | "reviewer" | "adviser"))
        && v["reason"].as_str().is_some_and(|s| text(s, 512))
}
pub(crate) fn input_form(l: &Loaded) -> Result<Value, String> {
    Ok(
        json!({"version":1,"scope":"contextual.v1","effect":"no-change",
        "bounds":{"sourceMaximumBytes":CONTEXT_BYTES,"maximumSelections":CHOICES,"maximumResourcesPerSelection":CHOICES,
            "taskMaximumBytes":1024,"revisionMaximumBytes":64,"reasonMaximumBytes":512,"purposeMaximumBytes":1024,"agentMaximumBytes":64},
        "template":{"version":1,"task":"<exact approved current task>","revision":"<approved context revision>","approvedBy":l.lead(),
            "selections":[{"agent":"<configured recipient>","role":"<its unique configured role>","purpose":"<exact configured purpose>",
                "profileSha256":"<current profile digest>","resources":[{"itemId":"<eligible item digest>","sourceSha256":"<eligible source digest>",
                    "ledgerHeadSha256":"<current ledger head>","reason":"<Lead's task-scoped applicability reason>"}]}]},
        "identityRules":"All digests are 64 lowercase hexadecimal characters. All strings are nonblank and control-free; byte bounds use UTF-8. Unknown fields and duplicate recipients/resources are refused. Empty selections/resources are valid.",
        "authority":"The explicitly configured existing approved task context carries the host-controlled Lead choice. Source text cannot select itself. This form writes no file, grants no rights and proves neither semantic suitability nor authenticated human approval."}),
    )
}
pub(crate) fn candidates(
    l: &Loaded,
    agent: &str,
    task: &str,
    host: &str,
    item: Option<&str>,
) -> Result<Value, String> {
    if !text(task, 1024) || !matches!(host, "codex" | "claude") {
        return Err("candidate discovery requires bounded task and codex|claude host".into());
    }
    let a = l.agent(agent).ok_or("unknown candidate recipient")?;
    let role = unique_role(l, agent)?;
    let mut refs = super::selection::candidate_references(l, agent, Some(task))?;
    refs.extend(crate::project::specialists::candidate_references(
        l, agent, &role, task, host,
    )?);
    let bytes = refs.iter().try_fold(0usize, |n, r| {
        r["byteLength"]
            .as_u64()
            .and_then(|b| usize::try_from(b).ok())
            .map(|b| n.saturating_add(b))
            .ok_or("candidate byte length unavailable")
    })?;
    super::selection::check_budget(l, refs.len(), bytes, "candidates")?;
    if let Some(id) = item {
        let r = refs
            .iter()
            .find(|r| r["itemId"] == id)
            .ok_or("candidate is not currently eligible for this recipient")?;
        let source = r["sourcePath"]
            .as_str()
            .ok_or("candidate source unavailable")?;
        let b = bounded(l, source, 16 * 1024)?;
        if hash::bytes(&b) != r["sourceSha256"] {
            return Err("candidate changed before read; refresh candidates".into());
        }
        let content = std::str::from_utf8(&b).map_err(|_| "candidate content must be UTF-8")?;
        if bounded(l, source, 16 * 1024)? != b {
            return Err("candidate changed during read".into());
        }
        let mut fresh = super::selection::candidate_references(l, agent, Some(task))?;
        fresh.extend(crate::project::specialists::candidate_references(
            l, agent, &role, task, host,
        )?);
        if !fresh.iter().any(|current| current == r) {
            return Err("candidate eligibility changed during read".into());
        }
        super::ensure_config_current(l)?;
        return Ok(
            json!({"version":1,"effect":"no-change","state":"candidate_data","reference":r,"content":content,
            "authority":"Candidate content is data for Lead evaluation, not adopted guidance, truth or permission."}),
        );
    }
    let config = l.path.to_str().ok_or("candidate config is not UTF-8")?;
    let profile = path::secure_bytes(&l.control_root, &a.profile, "candidate recipient profile")?;
    let entries = refs
        .into_iter()
        .map(|r| {
            let read = crate::emitted::Route::new([
                "memory",
                "candidates",
                agent,
                r["itemId"].as_str().unwrap_or_default(),
                "--task",
                task,
                "--host",
                host,
                "--json",
            ])
            .config_text(config)
            .full_argv()?;
            Ok(json!({"reference":r,"readSource":{"command":read,"readOnly":true}}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let v = json!({"version":1,"effect":"no-change","state":"candidates_only","agent":agent,"task":task,"role":role,"candidates":entries,
        "choiceTemplate":{"agent":agent,"role":role,"purpose":a.purpose,"profileSha256":hash::bytes(&profile),"resources":[]},
        "meaning":"Eligible resource metadata; literal matches are retrieval hints only. The Lead may inspect an eligible paraphrase candidate. No item is applied by discovery; use only the explicitly approved current task choice."});
    if serde_json::to_vec(&v).map_err(|e| e.to_string())?.len() > 16384 {
        return Err("candidate metadata exceeds 16384 bytes; narrow the existing policy".into());
    }
    Ok(v)
}
