//! Conditional native-host challenges and applicability of reported probes.
//! Reports remain in ordinary immutable Work artifacts. They grant no rights.
use crate::{config::Loaded, evidence::hash, run::RunSnapshot};
use serde_json::{json, Value};

const SCHEMA: &str = "exitbind-early-evidence-v1";

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

fn latest_report(loaded: &Loaded, events: &[Value]) -> Result<Option<(Value, Value)>, String> {
    for event in events
        .iter()
        .rev()
        .filter(|event| event["action"] == "submit")
    {
        if event["artifact"]["root"] != "state" {
            continue;
        }
        let artifact = crate::run::artifact::read(loaded, &event["artifact"], "early evidence")?;
        if artifact.bytes as usize != artifact.preview.len() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&artifact.preview) else {
            continue;
        };
        if let Some(report) = value.get("earlyEvidence") {
            return Ok(Some((event.clone(), report.clone())));
        }
    }
    Ok(None)
}

fn well_formed(report: &Value) -> bool {
    report["schema"] == SCHEMA
        && report["question"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty() && text.len() <= 2048)
        && report["contributions"].as_array().is_some_and(|items| {
            items.len() == 2
                && items[0]["sessionRef"] != items[1]["sessionRef"]
                && items.iter().all(|item| {
                    ["claim", "evidenceRef", "sessionRef", "whatWouldChange"]
                        .iter()
                        .all(|key| {
                            item[*key]
                                .as_str()
                                .is_some_and(|text| !text.trim().is_empty())
                        })
                })
        })
        && matches!(
            report["probe"]["observation"]["outcome"].as_str(),
            Some("discriminated" | "no_new_information" | "inconclusive")
        )
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
    let Some((event, report)) = latest_report(loaded, events)? else {
        return Ok(json!({"selected":false,"state":"not_selected","authority":"none"}));
    };
    let expected = requirement(
        loaded,
        work,
        events.first().ok_or("Work start unavailable")?,
    );
    let mut reasons = Vec::new();
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
    } else {
        reasons.push("probe_command_unavailable");
    }
    let source_current = probe["source"]["path"]
        .as_str()
        .and_then(|path| crate::run::artifact::evidence(loaded, Some("product"), path).ok())
        .is_some_and(|source| source["sha256"] == probe["source"]["sha256"]);
    if !source_current {
        reasons.push("probe_source_changed_or_unavailable");
    }
    let final_check = events.iter().rev().find(|item| {
        item["action"] == "check" && item["targetEventSha256"] == event["eventSha256"]
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
        "finalCheck":final_check.map(|check| json!({"eventSha256":check["eventSha256"],
            "result":check["result"],"inputsSha256":check["inputsSha256"],"distinctFromReportedProbe":true})),
        "leadDisposition":disposition.map(|decision| json!({"eventSha256":decision["eventSha256"],
            "outcome":decision["outcome"],"reason":decision["reason"]})),
        "independence":"Contributor session/evidence references are caller-reported, not host attestation.",
        "meaning":"Applicable reported evidence covers only declared requirement, candidate and probe inputs; it never permits skipping final checks, review or Lead acceptance."}),
    )
}
