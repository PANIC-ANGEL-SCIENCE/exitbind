//! Pending assignment packets derived from validated run state.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn pending(state: &Value) -> Vec<Value> {
    if state["status"] != "running" {
        return Vec::new();
    }
    let stage_num = state["currentStage"].as_u64().unwrap_or(0);
    let stage = state["plan"]["stages"]
        .as_array()
        .and_then(|stages| stages.iter().find(|stage| stage["stage"] == stage_num));
    let Some(stage) = stage else {
        return Vec::new();
    };
    let Some(submissions) = state["submissions"].as_array() else {
        return Vec::new();
    };
    let Some(agents) = stage["agents"].as_array() else {
        return Vec::new();
    };
    let submitted: BTreeSet<&str> = submissions
        .iter()
        .filter(|event| {
            event["stage"] == state["currentStage"] && event["attempt"] == state["attempt"]
        })
        .filter_map(|event| event["agent"].as_str())
        .collect();
    let current_attempt = state["attempt"].as_u64().unwrap_or(0);
    let current_stage = state["currentStage"].as_u64().unwrap_or(0);
    let mut upstream_by_identity = BTreeMap::new();
    for event in submissions {
        let attempt = event["attempt"].as_u64().unwrap_or(0);
        let stage = event["stage"].as_u64().unwrap_or(0);
        // Current preceding stages, the immediately prior attempt's current
        // artifact, and its lead rework are the only dependencies a fresh
        // role needs eagerly. Older attempt artifacts remain reachable from
        // the context snapshot reference.
        let prior_rework_lead = current_attempt > 0
            && attempt.checked_add(1) == Some(current_attempt)
            && event["role"] == "lead"
            && event["outcome"] == "rework";
        let relevant = (attempt == current_attempt && stage < current_stage)
            || (current_attempt > 0
                && attempt.checked_add(1) == Some(current_attempt)
                && stage == current_stage)
            || prior_rework_lead;
        if !relevant {
            continue;
        }
        let key = format!(
            "{}:{}:{}",
            stage,
            event["agent"].as_str().unwrap_or_default(),
            event["role"].as_str().unwrap_or_default()
        );
        upstream_by_identity.insert(
            key,
            json!({
                "stage": event["stage"],
                "attempt": event["attempt"],
                "agent": event["agent"],
                "role": event["role"],
                "root": event["artifact"]["root"].as_str().unwrap_or("product"),
                "path": event["artifact"]["path"],
                "sha256": event["artifact"]["sha256"],
                "attemptStatus": if event["attempt"] == state["attempt"] { "current" } else { "prior" }
            }),
        );
    }
    let mut upstream: Vec<Value> = upstream_by_identity.into_values().collect();
    upstream.sort_by_key(|item| {
        let stage = item["stage"].as_u64().unwrap_or(u64::MAX);
        let priority = if stage < current_stage {
            0
        } else if item["attemptStatus"] == "prior" && item["role"] == "lead" {
            1
        } else {
            2
        };
        (
            priority,
            stage,
            item["agent"].as_str().unwrap_or_default().to_owned(),
        )
    });
    let limit = state["plan"]["maxParallel"]
        .as_u64()
        .unwrap_or(agents.len() as u64)
        .max(1) as usize;
    let mut assignments: Vec<Value> = agents
        .iter()
        .filter(|agent| !submitted.contains(agent["name"].as_str().unwrap_or("")))
        .map(|agent| packet(state, agent, &upstream))
        .take(limit)
        .collect();
    // A reviewer whose primary binding could not execute is re-issued onto its
    // authorized alternate binding: same stage, attempt, role, and reviewer
    // contract — only where it executes is substituted.  At most one
    // substitution per stage+attempt is ever issued.
    for agent in agents {
        let Some(alternate) = agent
            .get("fallbackRuntime")
            .filter(|binding| binding.is_object())
        else {
            continue;
        };
        let name = agent["name"].as_str().unwrap_or("");
        let current: Vec<&Value> = submissions
            .iter()
            .filter(|event| {
                event["stage"] == state["currentStage"]
                    && event["attempt"] == state["attempt"]
                    && event["agent"] == name
            })
            .collect();
        // Exactly one recorded unavailability and nothing else opens the
        // substitution: a verdict has already closed the reviewer, and a second
        // unavailability has already ended the attempt rather than opening a
        // third binding.
        let [unavailable] = current[..] else {
            continue;
        };
        if unavailable["outcome"] != "unavailable" {
            continue;
        }
        assignments.push(substitution(
            state,
            agent,
            alternate,
            unavailable,
            &upstream,
        ));
    }
    assignments
}

/// The substitution packet: the primary's own reviewer contract — purpose,
/// profile, profile SHA-256, declared boundary — re-issued against the
/// alternate execution binding, with the operational reason carried so the
/// substitution is legible without upgrading caller-reported identity into
/// observed evidence.
fn substitution(
    state: &Value,
    agent: &Value,
    alternate: &Value,
    unavailable: &Value,
    upstream: &[Value],
) -> Value {
    let mut packet = packet(state, agent, upstream);
    let primary_runtime = binding(&agent["runtime"]);
    // Executing under the alternate binding, with no further fallback: the
    // substitution is bounded to one, so a second operational failure ends the
    // attempt instead of searching for a third binding.
    packet["runtime"] = json!({
        "host": alternate["host"],
        "model": alternate["model"],
        "reasoningEffort": alternate["reasoningEffort"],
        "fallback": "none",
    });
    if let Some(object) = packet.as_object_mut() {
        object.remove("fallbackRuntime");
    }
    packet["substitution"] = json!({
        "reason": unavailable["fallback"]["reason"],
        "primaryRuntime": primary_runtime,
        "runtime": alternate,
        "identitySource": "host-reported",
    });
    packet
}

/// A runtime value reduced to where it executes, dropping the fallback
/// authorization that is not part of any execution identity.
pub(crate) fn binding(runtime: &Value) -> Value {
    json!({
        "host": runtime["host"],
        "model": runtime["model"],
        "reasoningEffort": runtime["reasoningEffort"],
    })
}

fn packet(state: &Value, agent: &Value, upstream: &[Value]) -> Value {
    let mut assignment = json!({
        "stage": state["currentStage"],
        "attempt": state["attempt"],
        "agent": agent["name"],
        "displayName": agent["displayName"],
        "nativeTaskName": agent["nativeTaskName"],
        "role": agent["role"],
        "goal": state["goal"],
        "purpose": agent["purpose"],
        "profile": {"path": agent["profile"], "sha256": agent["profileSha256"]},
        "profilePath": agent["profile"],
        "profileSha256": agent["profileSha256"],
        "runtime": agent["runtime"],
        "declaredBoundary": agent["declaredBoundary"],
        "upstreamArtifacts": upstream
    });
    let run_short = state["runId"]
        .as_str()
        .unwrap_or("run")
        .chars()
        .take(12)
        .collect::<String>();
    assignment["artifactRootHint"] = json!("state");
    let state_namespace = crate::project_layout::state_namespace();
    assignment["artifactPathHint"] = json!(format!(
        "{state_namespace}/artifacts/{run_short}-{}-stage-{}-attempt-{}.md",
        agent["name"].as_str().unwrap_or("agent"),
        state["currentStage"],
        state["attempt"]
    ));
    assignment["upstreamArtifactsImmutable"] = json!(true);
    if let Some(references) = agent.get("memoryReferences") {
        assignment["memoryReferences"] = references.clone();
    }
    if let Some(receipt) = state.get("harnessReceipt") {
        assignment["harnessReceipt"] = receipt.clone();
    }
    if let Some(policy) = state.get("checkPolicy") {
        assignment["checkPolicy"] = policy.clone();
    }
    if let Some(preservation) = state.get("preservation") {
        // Preserve the resolved requirement text and exact checker identity in
        // every role packet.  This is a projection of validated canonical
        // state, not a human-supplied prompt fragment.
        assignment["preservationAssignment"] = json!({
            "route": "FORMAL",
            "quality": "FULL",
            "resolvedBy": "accepted_preservation_requirements",
            "enforcement": "recorded_not_enforced",
            "version": preservation["version"],
            "requirements": preservation["requirements"],
            "nonGoals": state.get("nonGoals").cloned().unwrap_or_else(|| json!([])),
            "source": "canonical_state",
        });
    }
    if let Some(alternate) = agent
        .get("fallbackRuntime")
        .filter(|binding| binding.is_object())
    {
        // The authorized alternate binding travels with the primary packet so
        // the caller can see what a substitution would be allowed to move to
        // before it reports the primary unavailable.
        assignment["fallbackRuntime"] = alternate.clone();
    }
    assignment
}

pub(crate) fn handle(work: &str, assignment: &Value) -> Result<String, String> {
    let stage = assignment["stage"]
        .as_u64()
        .ok_or("assignment stage is invalid")?;
    let attempt = assignment["attempt"]
        .as_u64()
        .ok_or("assignment attempt is invalid")?;
    let agent = assignment["agent"]
        .as_str()
        .ok_or("assignment agent is invalid")?;
    Ok(format!(
        "sma_{}",
        crate::hash::text(&format!("{work}\n{stage}\n{attempt}\n{agent}"))
    ))
}
