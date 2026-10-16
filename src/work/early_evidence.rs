//! Conditional native-host challenges and applicability of reported probes.
//! Reports remain in ordinary immutable Work artifacts. They grant no rights.
use crate::{config::Loaded, evidence::hash, run::RunSnapshot};
use serde_json::{json, Value};

const SCHEMA: &str = "exitbind-early-evidence-v1";
const MAX_REPORT_BYTES: usize = 64 * 1024;

fn requirement(loaded: &Loaded, work: &str, start: &Value) -> Value {
    let goal = crate::session_goal::record_for_work(loaded, work);
    json!({"workStartEventSha256":start["eventSha256"],
        "goalSha256":hash::value(&start["goal"]),"approvedText":start["goal"],
        "basisSha256":start["basisSha256"],"goalRevisionSha256":goal.as_ref().map(hash::value)})
}

/// Read-only selection. It launches no agent, probe, Work or model.
pub(crate) fn packet(loaded: &Loaded, work: &str, question: &str) -> Result<Value, String> {
    if question.trim().is_empty() || question.len() > 2048 || question.chars().any(char::is_control)
    {
        return Err(
            "challenge question must be nonempty single-line text, at most 2048 UTF-8 bytes".into(),
        );
    }
    super::details::ensure_current_config(loaded)?;
    let snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let view = snapshot.inspect_view();
    if view["status"] != "running" {
        return Err(
            "a challenge needs an active Work; historical results grant no new work".into(),
        );
    }
    let start = view["events"]
        .as_array()
        .and_then(|events| events.first())
        .ok_or("Work start unavailable")?;
    let inputs = view["inputsSha256"]
        .as_str()
        .ok_or("current candidate inputs unavailable")?;
    let requirement = requirement(loaded, work, start);
    let template = json!({"schema":SCHEMA,"work":work,"question":question,
        "requirement":requirement,"candidateInputsSha256":inputs,"contributions":[],
        "probe":{"command":"","commandSha256":null,"source":{"path":"","sha256":null},
            "observation":{"outcome":"inconclusive","wrongCandidate":null,"correctAlternative":null}}});
    let result = json!({"kind":"early_evidence_challenge","version":1,"readOnly":true,
        "authority":"none","work":work,"question":question,"requirement":requirement,
        "candidateInputsSha256":inputs,"evidenceTemplate":template,
        "hostAssignment":{"firstPasses":2,"maximumFocusedExchanges":1,
            "firstPassesSeeEachOther":false,
            "commonFacts":"Use the same approved requirement and current candidate facts in separate initial contexts.",
            "stop":"Stop after a relevant discriminator or a reasoned no-new-information/inconclusive result.",
            "permissions":"Existing host rights and bounded task authority only.",
            "return":"Claim, exact evidence reference, counterexample or discriminator, and what would change the conclusion."},
        "delivery":"Put the completed evidenceTemplate in the earlyEvidence field of the ordinary JSON result artifact. Work detail derives applicability and links the actual Lead decision and final check.",
        "validation":{"route":crate::work::compact::continuation_route(&loaded.path,vec!["work".into(),"challenge".into(),"validate".into(),work.into(),"--json".into()]),
            "stdin":"one earlyEvidence report or ordinary result JSON containing earlyEvidence; maximum 65536 bytes",
            "checks":["required fields and bounded values","two distinct contribution sessions","command hash","current Work, requirement and candidate identity","safe current probe source"],
            "limits":"A valid response confirms shape and identity only; it does not authenticate claims or prove probe execution."},
        "limits":"Reported probes are not final observed checks, independence attestation, user requirements, edit authority or acceptance. An unselected routine task needs no challenge."});
    let fresh = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?.inspect_view();
    super::details::ensure_current_config(loaded)?;
    if fresh["ledgerSha256"] != view["ledgerSha256"]
        || fresh["inputsSha256"] != view["inputsSha256"]
    {
        return Err("challenge inputs changed during capture; refresh the current Work".into());
    }
    Ok(result)
}

pub(crate) fn with_probe(
    loaded: &Loaded,
    mut packet: Value,
    command: Option<&str>,
    source: Option<&str>,
) -> Result<Value, String> {
    match (command, source) {
        (None, None) => return Ok(packet),
        (Some(command), Some(source))
            if !command.trim().is_empty() && command.len() <= 8192 && !command.contains('\0') => {
            let evidence = crate::run::artifact::evidence(loaded, Some("product"), source)?;
            packet["evidenceTemplate"]["probe"]["command"] = json!(command);
            packet["evidenceTemplate"]["probe"]["commandSha256"] = json!(hash::text(command));
            packet["evidenceTemplate"]["probe"]["source"] = json!({"path":source,"sha256":evidence["sha256"]});
        }
        _ => return Err("declare both --probe-command and --probe-source; command must be bounded nonempty text".into()),
    }
    packet["probeExecution"] =
        json!("not executed; observe this permitted probe before reporting an outcome");
    Ok(packet)
}

#[derive(Debug)]
enum LatestReport {
    Found(Value, Value),
    None,
    Unknown { event: Value, reason: &'static str },
}

fn latest_report(loaded: &Loaded, events: &[Value]) -> Result<LatestReport, String> {
    let Some(candidate_index) = events.iter().rposition(|event| {
        event["action"] == "submit" && event["role"] == "worker" && event["outcome"] == "completed"
    }) else {
        return Ok(LatestReport::None);
    };
    let event = &events[candidate_index];
    if event["artifact"]["root"] != "state" {
        return Ok(LatestReport::Unknown {
            event: event.clone(),
            reason: "latest_candidate_artifact_unavailable",
        });
    }
    let artifact = match crate::run::artifact::read(loaded, &event["artifact"], "early evidence") {
        Ok(artifact) => artifact,
        Err(error) if crate::run::artifact::is_missing(&error) => {
            return Ok(LatestReport::Unknown {
                event: event.clone(),
                reason: "latest_candidate_artifact_missing",
            })
        }
        Err(error) if error.ends_with("artifact bytes changed") => {
            return Ok(LatestReport::Unknown {
                event: event.clone(),
                reason: "latest_candidate_artifact_changed",
            })
        }
        Err(error) => return Err(error),
    };
    if artifact.bytes as usize != artifact.preview.len() {
        return Ok(LatestReport::Unknown {
            event: event.clone(),
            reason: "latest_candidate_artifact_truncated",
        });
    }
    // Result artifacts are allowed to be ordinary UTF-8 text. Only an
    // explicit JSON earlyEvidence member opts into report validation.
    let Ok(value) = serde_json::from_slice::<Value>(&artifact.preview) else {
        return historical_report(loaded, events, candidate_index);
    };
    if let Some(report) = value.get("earlyEvidence") {
        return Ok(LatestReport::Found(event.clone(), report.clone()));
    }
    // Preserve the latest earlier counterexample as history when this result
    // has no report. It cannot be promoted to current evidence.
    historical_report(loaded, events, candidate_index)
}

fn historical_report(
    loaded: &Loaded,
    events: &[Value],
    candidate_index: usize,
) -> Result<LatestReport, String> {
    for prior in events[..candidate_index]
        .iter()
        .rev()
        .filter(|prior| prior["action"] == "submit" && prior["artifact"]["root"] == "state")
    {
        let Ok(artifact) =
            crate::run::artifact::read(loaded, &prior["artifact"], "historical early evidence")
        else {
            continue;
        };
        if artifact.bytes as usize != artifact.preview.len() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&artifact.preview) else {
            continue;
        };
        if let Some(report) = value.get("earlyEvidence") {
            return Ok(LatestReport::Found(prior.clone(), report.clone()));
        }
    }
    Ok(LatestReport::None)
}

fn required_text<'a>(value: &'a Value, field: &str, max: usize) -> Result<&'a str, String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("earlyEvidence.{field} is missing or must be text"))?;
    if text.trim().is_empty() || text.len() > max || text.contains('\0') {
        return Err(format!(
            "earlyEvidence.{field} must be nonempty text of at most {max} UTF-8 bytes"
        ));
    }
    Ok(text)
}

fn required_hash(value: &Value, field: &str) -> Result<(), String> {
    let text = required_text(value, field, 64)?;
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "earlyEvidence.{field} must be a 64-character SHA-256 value"
        ));
    }
    Ok(())
}

/// Validate the caller-reported packet before a completed result is appended.
/// This checks shape and binding only; it does not verify execution or truth.
fn validate_report(report: &Value) -> Result<(), String> {
    if report["schema"] != SCHEMA {
        return Err("earlyEvidence.schema is missing or unsupported".into());
    }
    required_text(&report["work"], "work", 128)?;
    required_text(&report["question"], "question", 2048)?;
    required_hash(&report["candidateInputsSha256"], "candidateInputsSha256")?;
    let requirement = &report["requirement"];
    if !requirement.is_object() {
        return Err("earlyEvidence.requirement is missing or malformed".into());
    }
    required_hash(
        &requirement["workStartEventSha256"],
        "requirement.workStartEventSha256",
    )?;
    required_hash(&requirement["goalSha256"], "requirement.goalSha256")?;
    required_text(
        &requirement["approvedText"],
        "requirement.approvedText",
        65_536,
    )?;
    if !requirement["basisSha256"].is_null() {
        required_hash(&requirement["basisSha256"], "requirement.basisSha256")?;
    }
    if !requirement["goalRevisionSha256"].is_null() {
        required_hash(
            &requirement["goalRevisionSha256"],
            "requirement.goalRevisionSha256",
        )?;
    }
    let contributions = report["contributions"]
        .as_array()
        .ok_or("earlyEvidence.contributions is missing or must be an array")?;
    if contributions.len() != 2 {
        return Err("earlyEvidence.contributions must contain exactly two observations".into());
    }
    for (index, contribution) in contributions.iter().enumerate() {
        for field in ["claim", "evidenceRef", "sessionRef", "whatWouldChange"] {
            required_text(
                &contribution[field],
                &format!("contributions[{index}].{field}"),
                4096,
            )?;
        }
    }
    if contributions[0]["sessionRef"] == contributions[1]["sessionRef"] {
        return Err("earlyEvidence.contributions must identify two distinct sessions".into());
    }
    let probe = &report["probe"];
    let observation = &probe["observation"];
    let outcome = required_text(&observation["outcome"], "probe.observation.outcome", 32)?;
    if !matches!(
        outcome,
        "discriminated" | "no_new_information" | "inconclusive"
    ) {
        return Err("earlyEvidence.probe.observation.outcome must be discriminated, no_new_information, or inconclusive".into());
    }
    let command = probe["command"].as_str().unwrap_or_default();
    if command.trim().is_empty() {
        if outcome == "discriminated" {
            return Err("earlyEvidence.probe.command is required for a discriminator".into());
        }
        if !probe["commandSha256"].is_null()
            || !probe["source"]["path"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
            || !probe["source"]["sha256"].is_null()
        {
            return Err(
                "earlyEvidence.probe command and source identity must be supplied together".into(),
            );
        }
    } else {
        required_text(&probe["command"], "probe.command", 8192)?;
        let command_hash = required_text(&probe["commandSha256"], "probe.commandSha256", 64)?;
        if command_hash.len() != 64 || !command_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(
                "earlyEvidence.probe.commandSha256 must be a 64-character SHA-256 value".into(),
            );
        }
        if command_hash != hash::text(command) {
            return Err("earlyEvidence.probe.commandSha256 does not match probe.command".into());
        }
        required_text(&probe["source"]["path"], "probe.source.path", 4096)?;
        required_hash(&probe["source"]["sha256"], "probe.source.sha256")?;
    }
    if outcome == "discriminated"
        && (observation["wrongCandidate"].is_null() || observation["correctAlternative"].is_null())
    {
        return Err("earlyEvidence.probe.observation requires wrongCandidate and correctAlternative for a discriminator".into());
    }
    Ok(())
}

fn well_formed(report: &Value) -> bool {
    validate_report(report).is_ok()
}

/// Reject malformed or stale optional evidence before recording a worker return.
/// An omitted report remains an ordinary valid result.
pub(crate) fn validate_submission(
    _loaded: &Loaded,
    _work: &str,
    _events: &[Value],
    artifact: &[u8],
) -> Result<(), String> {
    let value: Value = match serde_json::from_slice(artifact) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    let Some(report) = value.get("earlyEvidence") else {
        return Ok(());
    };
    if serde_json::to_vec(report)
        .map_err(|_| "earlyEvidence report could not be encoded")?
        .len()
        > MAX_REPORT_BYTES
    {
        return Err("earlyEvidence report exceeds the 65536-byte report bound".into());
    }
    validate_report(report)?;
    // Currentness is a separate projection. A structurally complete report may
    // be historical, which preserves counterexamples and their original hashes.
    Ok(())
}

/// Validate a prepared report against the current machine identity before a
/// worker returns it. Reported claims and observations remain unauthenticated.
pub(crate) fn validate_current(loaded: &Loaded, work: &str, bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_REPORT_BYTES {
        return Err("earlyEvidence validation input exceeds the result artifact bound".into());
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| "earlyEvidence validation input is not valid JSON")?;
    let report = value.get("earlyEvidence").unwrap_or(&value);
    validate_report(report)?;
    let snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let view = snapshot.inspect_view();
    if view["status"] != "running" {
        return Err("earlyEvidence validation requires an active Work".into());
    }
    let start = view["events"]
        .as_array()
        .and_then(|events| events.first())
        .ok_or("earlyEvidence current Work start is unavailable")?;
    if report["work"] != work {
        return Err("earlyEvidence.work does not match the current Work".into());
    }
    let expected = requirement(loaded, work, start);
    if report["requirement"] != expected {
        return Err(
            "earlyEvidence.requirement does not match the current approved requirement".into(),
        );
    }
    if report["candidateInputsSha256"] != view["inputsSha256"] {
        return Err(
            "earlyEvidence.candidateInputsSha256 does not match the current candidate inputs"
                .into(),
        );
    }
    let probe = &report["probe"];
    let probe_source_sha256 = if !probe["command"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        let source_path = probe["source"]["path"].as_str().expect("validated path");
        let source = crate::run::artifact::evidence(loaded, Some("product"), source_path)
            .map_err(|_| "earlyEvidence.probe.source.path is unsafe or unavailable")?;
        if source["sha256"] != probe["source"]["sha256"] {
            return Err(
                "earlyEvidence.probe.source.sha256 does not match the current source".into(),
            );
        }
        source.get("sha256").cloned()
    } else {
        None
    };
    Ok(json!({"valid":true,"authority":"none","work":work,
        "candidateInputsSha256":view["inputsSha256"],
        "probeSourceSha256":probe_source_sha256,
        "meaning":"shape and current machine identity validated; claims, independence and probe execution remain caller-reported"}))
}

fn unknown_projection(event: &Value, reason: &str) -> Value {
    json!({"selected":true,"state":"unknown","authority":"none",
        "reasons":[reason],"reportEventSha256":event["eventSha256"],
        "artifactSha256":event["artifact"]["sha256"],
        "candidateSubjectSha256":event["subjectSha256"]})
}

pub(crate) fn projection(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
) -> Result<Value, String> {
    let view = snapshot.inspect_view();
    let events = view["events"]
        .as_array()
        .ok_or("current events unavailable")?;
    let (event, report) = match latest_report(loaded, events)? {
        LatestReport::None => {
            return Ok(json!({"selected":false,"state":"not_selected","authority":"none"}));
        }
        LatestReport::Unknown { event, reason } => {
            return Ok(unknown_projection(&event, reason));
        }
        LatestReport::Found(event, report) => (event, report),
    };
    let expected = requirement(
        loaded,
        work,
        events.first().ok_or("Work start unavailable")?,
    );
    let mut reasons = Vec::new();
    let latest_candidate = events.iter().rev().find(|item| {
        item["action"] == "submit" && item["role"] == "worker" && item["outcome"] == "completed"
    });
    if latest_candidate.is_some_and(|latest| latest["eventSha256"] != event["eventSha256"]) {
        reasons.push("newer_candidate_result_without_report");
    }
    let valid = well_formed(&report);
    if !valid {
        reasons.push("incomplete_or_invalid_report");
    }
    if report["work"] != work || report["requirement"] != expected {
        reasons.push("requirement_or_goal_revision_changed");
    }
    let inputs = crate::run::inputs::fingerprint(loaded).ok();
    if inputs.is_none() {
        reasons.push("current_inputs_unavailable");
    } else if report["candidateInputsSha256"].as_str() != inputs.as_deref() {
        reasons.push("candidate_or_relevant_inputs_changed");
    }
    let probe = &report["probe"];
    if let Some(command) = probe["command"]
        .as_str()
        .filter(|command| !command.is_empty())
    {
        if probe["commandSha256"] != hash::text(command) {
            reasons.push("probe_command_identity_mismatch");
        }
    }
    if probe["command"]
        .as_str()
        .is_some_and(|command| !command.trim().is_empty())
    {
        let source_current = probe["source"]["path"]
            .as_str()
            .and_then(|path| crate::run::artifact::evidence(loaded, Some("product"), path).ok())
            .is_some_and(|source| source["sha256"] == probe["source"]["sha256"]);
        if !source_current {
            reasons.push("probe_source_changed_or_unavailable");
        }
    }
    let final_check = latest_candidate.and_then(|candidate| {
        events.iter().rev().find(|item| {
            item["action"] == "check" && item["targetEventSha256"] == candidate["eventSha256"]
        })
    });
    let disposition = events.iter().rev().find(|item| {
        item["action"] == "submit"
            && item["role"] == "lead"
            && item["attempt"] == event["attempt"]
            && item["stage"].as_u64() > event["stage"].as_u64()
    });
    let state = if !valid {
        "unknown"
    } else if reasons.is_empty() {
        "reported_current"
    } else {
        "historical"
    };
    Ok(
        json!({"selected":true,"state":state,"authority":"none","reasons":reasons,
        "requirement":expected,"report":report,"artifactSha256":event["artifact"]["sha256"],
        "reportEventSha256":event["eventSha256"],"candidateSubjectSha256":event["subjectSha256"],
        "latestCandidateEventSha256":latest_candidate.map(|candidate|candidate["eventSha256"].clone()),
        "finalCheck":final_check.map(|check| json!({"eventSha256":check["eventSha256"],
            "result":check["result"],"inputsSha256":check["inputsSha256"],"distinctFromReportedProbe":true})),
        "leadDisposition":disposition.map(|decision| json!({"eventSha256":decision["eventSha256"],
            "outcome":decision["outcome"],"reason":decision["reason"]})),
        "independence":"Contributor session/evidence references are caller-reported, not host attestation.",
        "meaning":"Applicable reported evidence covers only declared requirement, candidate and probe inputs; it never permits skipping final checks, review or Lead acceptance."}),
    )
}

#[cfg(test)]
#[path = "early_evidence/unit_tests.rs"]
mod unit_tests;
