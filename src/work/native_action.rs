#[path = "native_action/prompt.rs"]
mod prompt;
#[path = "native_action/result.rs"]
mod result;

use result::{
    bounded_json, projected_result, projection, request_projection, role_schema,
    validate_final_result,
};

use crate::config::Loaded;
use crate::evidence::hash;
use crate::host::codex_exec::{self, Request, TurnStatus};
use crate::project::{managed_files, path as project_path};
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TIMEOUT_MS: u64 = 1_800_000;
const MAX_RESULT_BYTES: usize = 64 * 1024;
const MAX_JOURNAL_BYTES: u64 = 256 * 1024;
const MAX_PROFILE_BYTES: usize = 64 * 1024;
const MAX_PACKET_BYTES: usize = 64 * 1024;

pub(crate) struct Options<'a> {
    pub(crate) codex_bin: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) reasoning_effort: Option<&'a str>,
    pub(crate) sandbox_mode: Option<&'a str>,
    pub(crate) timeout_ms: Option<&'a str>,
    pub(crate) resume: bool,
}

pub(crate) fn execute(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    options: Options<'_>,
) -> Result<Value, String> {
    let identity = assignment_identity(current)?;
    let role = current["role"]
        .as_str()
        .ok_or("current native assignment has no role")?;
    let agent = current["agent"]
        .as_str()
        .ok_or("current native assignment has no agent")?;
    let paths = journal_paths(
        loaded,
        work,
        &identity.assignment,
        current["packet"]["substitution"].is_object(),
    )?;
    let _lock = acquire_lock(&paths.lock)?;
    let existing = read_journal(&paths.journal)?;
    let inherited = if existing.is_none() && options.resume {
        find_resume_source(&paths, role, agent, current["packet"]["attempt"].as_u64())?
    } else {
        None
    };

    if let Some(journal) = existing.as_ref() {
        verify_journal(journal, work, &identity, role, agent)?;
        match journal["status"].as_str() {
            Some("completed") => {
                let bytes = journal_result(journal)?;
                return submit_saved(loaded, work, &identity.assignment, bytes);
            }
            Some("started" | "running") if !options.resume => {
                return Err(format!(
                    "native assignment is already started; use work act {work} --resume after inspecting its private journal"
                ));
            }
            Some("started") if role == "reviewer" => {
                return Err(
                    "reviewer resume is refused; a reviewer must use a fresh native session".into(),
                );
            }
            Some("started") => {}
            Some("running") => {
                return Err(
                    "native assignment journal is in an uncertain running state; safe resume cannot prove the prior provider process ended"
                        .into(),
                );
            }
            Some(_) | None => return Err("native assignment journal has an invalid status".into()),
        }
    } else if options.resume && inherited.is_none() {
        return Err("--resume requires a started native assignment journal".into());
    }

    let assignment = &current["packet"];
    let _canonical_packet = bounded_json(assignment, MAX_PACKET_BYTES, "assignment packet")?;
    let packet = bounded_json(
        &prompt::delivery_packet(assignment),
        MAX_PACKET_BYTES,
        "native delivery packet",
    )?;
    let profile = profile_bytes(loaded, agent, assignment)?;
    let review_evidence = verified_worker_results(loaded, work, role, assignment)?;
    let mut schema: Value =
        serde_json::from_str(role_schema(role)?).map_err(|_| "native role schema is invalid")?;
    if let Some(evidence) = review_evidence.as_ref() {
        schema["properties"]["evidenceReferences"]["items"]["enum"] = json!(evidence
            .iter()
            .map(|item| &item.reference)
            .collect::<Vec<_>>());
    }
    let schema_path = paths.schema.clone();
    ensure_schema(&schema_path, &schema.to_string())?;

    let thread_id = existing
        .as_ref()
        .and_then(|journal| journal["threadId"].as_str())
        .or_else(|| inherited.as_ref().map(|source| source.thread_id.as_str()));
    if options.resume && thread_id.is_none() {
        return Err(
            "native assignment journal has no provider thread ID; safe resume is unavailable"
                .into(),
        );
    }
    let request = build_request(
        loaded,
        options,
        role,
        work,
        assignment,
        packet,
        profile,
        review_evidence.as_ref(),
        &schema_path,
        thread_id,
    )?;
    codex_exec::validate_request(&request).map_err(|error| error.to_string())?;
    if existing.is_none() {
        write_journal(
            &paths.journal,
            &json!({
                "version": 1,
                "status": "started",
                "work": work,
                "assignment": identity.assignment,
                "assignmentSha256": identity.packet_sha256,
                "role": role,
                "agent": agent,
                "stage": current["packet"]["stage"],
                "attempt": current["packet"]["attempt"],
                "threadId": thread_id.map_or(Value::Null, |value| json!(value)),
                "resumedFromAssignment": inherited
                    .as_ref()
                    .map_or(Value::Null, |source| json!(source.assignment)),
                "startedAt": timestamp(),
                "request": request_projection(&request),
            }),
            true,
        )?;
    }

    claim_started(&paths.journal)?;
    let observation = match codex_exec::run(&request) {
        Ok(observation) => observation,
        Err(error) => {
            record_error(&paths.journal, &error.to_string())?;
            return Err(error.to_string());
        }
    };
    let mut observation_value = projection(&observation);
    if let Some(evidence) = review_evidence.as_ref() {
        observation_value["verifiedEvidence"] = json!(evidence
            .iter()
            .map(|item| json!({
                "reference": item.reference,
                "sha256": item.sha256,
                "bytes": item.bytes,
            }))
            .collect::<Vec<_>>());
    }
    update_started(&paths.journal, &observation_value)?;
    let final_result = observation
        .final_result
        .as_ref()
        .ok_or_else(|| format!("native assignment has no typed {role} result"))?;
    let outcome = validate_final_result(role, final_result)?;
    if let Some(evidence) = review_evidence.as_ref() {
        let expected = evidence
            .iter()
            .map(|item| json!(item.reference))
            .collect::<Vec<_>>();
        if final_result["evidenceReferences"] != json!(expected) {
            return Err(
                "native reviewer did not cite every verified worker result reference".into(),
            );
        }
    }
    if !observation.process.success
        || observation.interrupted
        || !matches!(observation.turn, TurnStatus::Completed)
        || !observation.coverage_gap.is_empty()
    {
        return Err(format!(
            "native assignment did not produce a complete accepted observation (outcome {outcome})"
        ));
    }
    let result = projected_result(role, final_result, &observation_value)?;
    let bytes = serde_json::to_vec(&result).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_RESULT_BYTES {
        return Err("native result exceeds the bounded result journal".into());
    }
    write_completed(&paths.journal, &observation_value, &bytes)?;
    submit_saved(loaded, work, &identity.assignment, bytes)
}

struct Paths {
    journal: PathBuf,
    schema: PathBuf,
    lock: PathBuf,
}

struct Identity {
    assignment: String,
    packet_sha256: String,
}

struct ReviewEvidence {
    reference: String,
    sha256: String,
    bytes: u64,
    content: String,
}

fn verified_worker_results(
    loaded: &Loaded,
    work: &str,
    role: &str,
    assignment: &Value,
) -> Result<Option<Vec<ReviewEvidence>>, String> {
    if role != "reviewer" {
        return Ok(None);
    }
    let ledger = super::super::resolve(loaded, work)?;
    let (_path, events, _source) = crate::run::ledger::load(loaded, &ledger)?;
    let packet_evidence = assignment["context"]["evidence"]
        .as_array()
        .ok_or("reviewer packet has no evidence list")?;
    let worker_events = current_worker_events(&events, assignment);
    if worker_events.is_empty() || worker_events.len() > 8 {
        return Err("reviewer needs one to eight current worker results".into());
    }
    let mut verified = Vec::with_capacity(worker_events.len());
    for event in worker_events {
        let submission = packet_evidence
            .iter()
            .find(|item| {
                item["kind"] == "submission"
                    && item["rawEventRef"]["selector"] == event["eventSha256"]
                    && item["artifact"]["sha256"] == event["artifact"]["sha256"]
            })
            .ok_or("current worker result has no packet-bound submission reference")?;
        let artifact = crate::run::artifact::read(loaded, &event["artifact"], "worker result")?;
        if artifact.bytes > MAX_RESULT_BYTES as u64
            || artifact.preview.len() != artifact.bytes as usize
        {
            return Err("current worker result exceeds the bounded reviewer evidence route".into());
        }
        verified.push(ReviewEvidence {
            reference: submission["rawEventRef"]["id"]
                .as_str()
                .ok_or("current worker result has no packet-bound evidence reference")?
                .to_owned(),
            sha256: artifact.sha256,
            bytes: artifact.bytes,
            content: String::from_utf8(artifact.preview)
                .map_err(|_| "current worker result is not UTF-8 JSON".to_owned())?,
        });
    }
    Ok(Some(verified))
}

fn current_worker_events<'a>(events: &'a [Value], assignment: &Value) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
                && event["attempt"] == assignment["attempt"]
                && event["stage"]
                    .as_u64()
                    .is_some_and(|stage| stage < assignment["stage"].as_u64().unwrap_or(0))
        })
        .collect()
}

fn assignment_identity(current: &Value) -> Result<Identity, String> {
    let assignment = current["assignment"]
        .as_str()
        .ok_or("current native assignment has no assignment identity")?;
    if assignment.len() > 128
        || !assignment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err("current native assignment identity is invalid".into());
    }
    Ok(Identity {
        assignment: assignment.to_owned(),
        packet_sha256: hash::value(&current["packet"]),
    })
}

fn journal_paths(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    substituted: bool,
) -> Result<Paths, String> {
    if !super::super::valid_work_handle(work) {
        return Err("native action work handle is invalid".into());
    }
    let root = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("native-actions")
        .join(work);
    managed_files::ensure_state_directory(&loaded.state_root, &root)?;
    let key = journal_key(assignment, substituted);
    Ok(Paths {
        journal: root.join(format!("{key}.json")),
        schema: root.join(format!("{key}.schema.json")),
        lock: root.join(format!("{key}.lock")),
    })
}

fn journal_key(assignment: &str, substituted: bool) -> String {
    if substituted {
        format!("{assignment}-fallback")
    } else {
        assignment.to_owned()
    }
}

struct ResumeSource {
    assignment: String,
    thread_id: String,
    _lock: JournalLock,
}

fn find_resume_source(
    paths: &Paths,
    role: &str,
    agent: &str,
    current_attempt: Option<u64>,
) -> Result<Option<ResumeSource>, String> {
    if role == "reviewer" {
        return Err(
            "reviewer resume is refused; a reviewer must use a fresh native session".into(),
        );
    }
    let current_attempt =
        current_attempt.ok_or("current native assignment has no attempt for safe resume")?;
    let directory = paths
        .journal
        .parent()
        .ok_or("native assignment journal has no parent")?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path == paths.journal
            || path.extension().and_then(|value| value.to_str()) != Some("json")
            || path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.ends_with(".schema.json"))
        {
            continue;
        }
        let prior_lock = acquire_lock(&path.with_extension("lock"))?;
        let Some(journal) = read_journal(&path)? else {
            continue;
        };
        if journal["role"] != role
            || journal["agent"] != agent
            || !matches!(journal["status"].as_str(), Some("started" | "completed"))
        {
            continue;
        }
        let Some(attempt) = journal["attempt"].as_u64() else {
            continue;
        };
        let Some(thread_id) = journal["threadId"].as_str() else {
            continue;
        };
        let Some(assignment) = journal["assignment"].as_str() else {
            continue;
        };
        if attempt >= current_attempt || thread_id.is_empty() || assignment.is_empty() {
            continue;
        }
        candidates.push((
            attempt,
            assignment.to_owned(),
            thread_id.to_owned(),
            prior_lock,
        ));
    }
    candidates.sort_by_key(|candidate| candidate.0);
    if let Some(newest) = candidates.last().map(|candidate| candidate.0) {
        if candidates
            .iter()
            .filter(|candidate| candidate.0 == newest)
            .count()
            > 1
        {
            return Err("safe native resume is ambiguous across prior assignments".into());
        }
    }
    Ok(candidates
        .pop()
        .map(|(_, assignment, thread_id, lock)| ResumeSource {
            assignment,
            thread_id,
            _lock: lock,
        }))
}

struct JournalLock(File);

fn acquire_lock(path: &Path) -> Result<JournalLock, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            return Err("native assignment is already claimed by another process".into());
        }
    }
    Ok(JournalLock(file))
}

fn ensure_schema(path: &Path, expected: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("native output schema is not a regular file".into());
            }
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            if bytes != expected.as_bytes() {
                return Err("native output schema does not match the expected role schema".into());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            managed_files::write_state_exclusive(path, expected.as_bytes())
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn read_journal(path: &Path) -> Result<Option<Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("native assignment journal is not a regular file".into());
    }
    if metadata.len() > MAX_JOURNAL_BYTES {
        return Err("native assignment journal exceeds its bound".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("native assignment journal is invalid: {error}"))
}

fn verify_journal(
    journal: &Value,
    work: &str,
    identity: &Identity,
    role: &str,
    agent: &str,
) -> Result<(), String> {
    if journal["version"] != 1
        || journal["work"] != work
        || journal["assignment"] != identity.assignment
        || journal["assignmentSha256"] != identity.packet_sha256
        || journal["role"] != role
        || journal["agent"] != agent
    {
        return Err("native assignment journal does not match the current assignment".into());
    }
    Ok(())
}

fn journal_result(journal: &Value) -> Result<Vec<u8>, String> {
    let encoded = journal["result"]
        .as_str()
        .ok_or("completed native journal has no result")?;
    if encoded.len() > MAX_RESULT_BYTES * 2 {
        return Err("completed native journal result exceeds its bound".into());
    }
    let bytes = hex_decode(encoded)?;
    if bytes.len() > MAX_RESULT_BYTES {
        return Err("completed native journal result exceeds its bound".into());
    }
    Ok(bytes)
}

fn write_journal(path: &Path, value: &Value, exclusive: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_JOURNAL_BYTES as usize {
        return Err("native assignment journal exceeds its bound".into());
    }
    if !exclusive {
        return atomic_replace(path, &bytes);
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("native journal has no parent")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("native journal has an invalid name")?;
    let temporary = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        timestamp()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        Ok::<(), String>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn claim_started(path: &Path) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    match journal["status"].as_str() {
        Some("started") => {}
        Some("running") => return Ok(()),
        _ => return Err("native assignment journal cannot be claimed".into()),
    }
    journal["status"] = json!("running");
    write_journal(path, &journal, false)
}

fn record_error(path: &Path, error: &str) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    journal["status"] = json!("started");
    journal["error"] = json!(error.chars().take(1024).collect::<String>());
    write_journal(path, &journal, false)
}

fn update_started(path: &Path, observation: &Value) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    journal["status"] = json!("started");
    journal["observation"] = observation.clone();
    journal["threadId"] = observation["threadId"].clone();
    write_journal(path, &journal, false)
}

fn write_completed(path: &Path, observation: &Value, result: &[u8]) -> Result<(), String> {
    let mut journal = read_journal(path)?.ok_or("native assignment journal disappeared")?;
    journal["status"] = json!("completed");
    journal["completedAt"] = json!(timestamp());
    journal["observation"] = observation.clone();
    journal["threadId"] = observation["threadId"].clone();
    journal["result"] = json!(hex_encode(result));
    write_journal(path, &journal, false)
}

fn submit_saved(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    bytes: Vec<u8>,
) -> Result<Value, String> {
    let (outcome, reason) = outcome_from_bytes(&bytes)?;
    super::super::return_result_impl::return_result_with(
        loaded,
        work,
        assignment,
        &outcome,
        reason.as_deref(),
        None,
        None,
        Some(bytes),
    )
}

fn outcome_from_bytes(bytes: &[u8]) -> Result<(String, Option<String>), String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "native result is not JSON")?;
    let outcome = value["outcome"]
        .as_str()
        .filter(|outcome| !outcome.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "native result has no outcome".to_owned())?;
    let reason = value
        .get("reason")
        .map(|reason| {
            reason
                .as_str()
                .filter(|reason| reason.len() <= 8192 && !reason.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or("native result reason is invalid")
        })
        .transpose()?;
    Ok((outcome, reason))
}

fn build_request(
    loaded: &Loaded,
    options: Options<'_>,
    role: &str,
    work: &str,
    assignment: &Value,
    packet: String,
    profile: String,
    review_evidence: Option<&Vec<ReviewEvidence>>,
    schema_path: &Path,
    thread_id: Option<&str>,
) -> Result<Request, String> {
    if assignment["runtime"]["host"]
        .as_str()
        .is_some_and(|host| host != "codex")
    {
        return Err("native Codex action requires runtime.host=codex".into());
    }
    let model = options
        .model
        .map(str::to_owned)
        .or_else(|| assignment["runtime"]["model"].as_str().map(str::to_owned));
    let effort = options.reasoning_effort.map(str::to_owned).or_else(|| {
        assignment["runtime"]["reasoningEffort"]
            .as_str()
            .map(str::to_owned)
    });
    let timeout = options
        .timeout_ms
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "timeout must be an integer".to_owned())
        })
        .transpose()?
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    if timeout == 0 {
        return Err("timeout must be positive".into());
    }
    let executable = codex_exec::resolve_codex(options.codex_bin.map(Path::new))
        .map_err(|error| error.to_string())?;
    if thread_id.is_some() && options.sandbox_mode.is_some() {
        return Err(
            "native resume cannot apply --sandbox; omit --sandbox for the persisted session or start a fresh assignment"
                .into(),
        );
    }
    let evidence_route = if let Some(evidence) = review_evidence {
        let mut route = format!("Use every verified current worker result below. The evidenceReferences array must contain every listed ref token in order; put file lines and check observations in summary. Use exitbind work expand {work} REFERENCE for surrounding ledger events. Do not reconstruct upstream artifacts from git or summaries. For unavailable, reason must be provider_quota, rate_limit, or provider_unavailable; for rework use review_finding; for blocked use blocked; for approved use an empty reason.\n");
        for item in evidence {
            route.push_str(&format!(
                "\nVERIFIED WORKER RESULT (reference {}, sha256 {}):\n{}\n",
                item.reference, item.sha256, item.content
            ));
        }
        route
    } else if role == "reviewer" {
        return Err("reviewer evidence route is unavailable".into());
    } else {
        format!(
            "Use the packet's declared evidence routes when checking the assignment. Do not copy upstream artifacts into the prompt or reconstruct them from git."
        )
    };
    let prompt = format!(
        "You are the native Codex {role} for one governed Exitbind assignment. Follow the supplied profile and verified assignment. Work only within the declared boundary. This packet is already bound; a continuation lookup is unnecessary. {evidence_route} Return only the JSON object required by the output schema; do not include markdown or commentary.\n\nPROFILE BYTES:\n{profile}\n\nCURRENT ASSIGNMENT DELIVERY PROJECTION (canonical packet SHA-256 {canonical_sha}; context.digest belongs to the full canonical context; omitted recovery goal/scope/subject/current/missing/loop/next fields equal context goal/scope/subject/evidence/obligations/loop/next respectively):\n{packet}\n",
        canonical_sha = hash::value(assignment),
    );
    let sandbox = if thread_id.is_some() {
        None
    } else {
        Some(
            options
                .sandbox_mode
                .map(str::to_owned)
                .unwrap_or_else(|| match role {
                    "worker" => "workspace-write".to_owned(),
                    _ => "read-only".to_owned(),
                }),
        )
    };
    Ok(Request {
        executable,
        cwd: loaded.product_root.clone(),
        prompt,
        model,
        effort,
        sandbox,
        output_schema: Some(schema_path.to_path_buf()),
        resume_thread_id: thread_id.map(str::to_owned),
        persist_session: true,
        timeout: Duration::from_millis(timeout),
    })
}

fn profile_bytes(loaded: &Loaded, agent: &str, assignment: &Value) -> Result<String, String> {
    let configured = loaded
        .agent(agent)
        .ok_or_else(|| format!("configured agent '{agent}' is unavailable"))?;
    let bytes = project_path::secure_bytes(&loaded.control_root, &configured.profile, "profile")?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err("native profile exceeds its bound".into());
    }
    let expected = assignment["profile"]["sha256"]
        .as_str()
        .ok_or("native assignment profile hash is unavailable")?;
    let current = hash::bytes(&bytes);
    if current != expected {
        return Err("native assignment profile drifted before launch".into());
    }
    String::from_utf8(bytes).map_err(|_| "native profile is not UTF-8".into())
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("completed native journal result is not hex".into());
    }
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text =
                std::str::from_utf8(pair).map_err(|_| "invalid journal result".to_owned())?;
            u8::from_str_radix(text, 16).map_err(|_| "invalid journal result".to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_outcomes_are_strictly_bounded() {
        assert!(result::allowed_outcome("worker", "completed").is_ok());
        assert!(result::allowed_outcome("reviewer", "completed").is_err());
        assert!(result::allowed_outcome("adviser", "approved").is_err());
    }

    #[test]
    fn native_role_schemas_require_every_declared_property() {
        for role in ["worker", "reviewer", "adviser"] {
            let schema: Value = serde_json::from_str(role_schema(role).unwrap()).unwrap();
            let properties = schema["properties"].as_object().unwrap();
            let required = schema["required"].as_array().unwrap();
            assert_eq!(required.len(), properties.len(), "{role}");
            for property in properties.keys() {
                assert!(
                    required.iter().any(|name| name == property),
                    "{role}: {property}"
                );
            }
        }
    }

    #[test]
    fn journal_result_round_trip_is_bounded_and_content_preserving() {
        let value = b"{\"outcome\":\"blocked\"}";
        let encoded = hex_encode(value);
        assert_eq!(hex_decode(&encoded).unwrap(), value);
        assert!(hex_decode("xyz").is_err());
    }

    #[test]
    fn current_reviewer_requires_all_completed_workers_in_its_attempt() {
        let assignment = json!({"attempt": 2, "stage": 3});
        let events = vec![
            json!({"action":"submit","role":"worker","outcome":"completed","attempt":1,"stage":2,"agent":"old"}),
            json!({"action":"submit","role":"worker","outcome":"completed","attempt":2,"stage":2,"agent":"first"}),
            json!({"action":"submit","role":"worker","outcome":"completed","attempt":2,"stage":2,"agent":"second"}),
            json!({"action":"submit","role":"reviewer","outcome":"approved","attempt":2,"stage":3,"agent":"reviewer"}),
        ];
        let selected = current_worker_events(&events, &assignment);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0]["agent"], "first");
        assert_eq!(selected[1]["agent"], "second");
    }

    #[test]
    fn fallback_journal_is_distinct_from_primary_assignment() {
        assert_eq!(journal_key("sma_123", false), "sma_123");
        assert_eq!(journal_key("sma_123", true), "sma_123-fallback");
    }

    #[test]
    fn unavailable_reason_must_be_an_allowed_operational_code() {
        let valid = json!({"outcome":"unavailable","summary":"provider unavailable","reason":"rate_limit","evidenceReferences":["ref:abc123"]});
        assert!(validate_final_result("reviewer", &valid).is_ok());
        let invalid = json!({"outcome":"unavailable","summary":"provider unavailable","reason":"some prose","evidenceReferences":["ref:abc123"]});
        assert!(validate_final_result("reviewer", &invalid).is_err());
        let missing = json!({"outcome":"unavailable","summary":"provider unavailable","evidenceReferences":["ref:abc123"]});
        assert!(validate_final_result("reviewer", &missing).is_err());
    }
}
