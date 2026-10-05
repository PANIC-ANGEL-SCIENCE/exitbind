//! Native role schemas, verdict validation, and bounded observation projection.

use crate::host::codex_exec::{CoverageGap, DiagnosticStatus, Observation, Request};
use serde_json::{json, Value};
use std::sync::OnceLock;

pub(super) fn role_schema(role: &str) -> Result<&'static str, String> {
    static WORKER: OnceLock<String> = OnceLock::new();
    static REVIEWER: OnceLock<String> = OnceLock::new();
    static ADVISER: OnceLock<String> = OnceLock::new();
    let schema = match role {
        "worker" => WORKER.get_or_init(|| schema_text("worker")),
        "reviewer" => REVIEWER.get_or_init(|| schema_text("reviewer")),
        "adviser" => ADVISER.get_or_init(|| schema_text("adviser")),
        _ => return Err("native Codex action has an unsupported role".into()),
    };
    Ok(schema.as_str())
}

fn schema_text(role: &str) -> String {
    crate::kernel::result_contract::native_schema_text(role)
        .expect("known native role must have a schema")
}

pub(super) fn allowed_outcome<'a>(role: &str, outcome: &'a str) -> Result<&'a str, String> {
    if !crate::kernel::result_contract::Role::parse(role)
        .is_some_and(|role| role != crate::kernel::result_contract::Role::Lead)
    {
        return Err(format!("native {role} result has an invalid outcome"));
    }
    crate::kernel::result_contract::validate(role, outcome)
        .map(|_| outcome)
        .map_err(|_| format!("native {role} result has an invalid outcome"))
}

pub(super) fn validate_final_result<'a>(role: &str, result: &'a Value) -> Result<&'a str, String> {
    let object = result
        .as_object()
        .ok_or_else(|| format!("native {role} result must be an object"))?;
    if object.keys().any(|key| {
        !(matches!(key.as_str(), "outcome" | "summary" | "reason")
            || (role == "reviewer" && key == "evidenceReferences"))
    }) {
        return Err(format!("native {role} result contains an undeclared field"));
    }
    if role == "reviewer" {
        let references = object
            .get("evidenceReferences")
            .and_then(Value::as_array)
            .filter(|references| !references.is_empty() && references.len() <= 8)
            .ok_or("native reviewer result needs bounded evidenceReferences")?;
        let mut seen = Vec::new();
        for reference in references {
            let reference = reference
                .as_str()
                .filter(|reference| reference.len() >= 6 && reference.len() <= 80)
                .ok_or("native reviewer evidence reference is invalid")?;
            if !reference.starts_with("ref:")
                || !reference
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"ref:._:-".contains(&byte))
            {
                return Err("native reviewer evidence reference is invalid".into());
            }
            if seen.contains(&reference) {
                return Err("native reviewer evidence references must be unique".into());
            }
            seen.push(reference);
        }
    }
    let summary = object
        .get("summary")
        .and_then(Value::as_str)
        .filter(|summary| !summary.is_empty() && summary.len() <= 8192)
        .ok_or_else(|| format!("native {role} result has no bounded summary"))?;
    if summary.chars().any(char::is_control) {
        return Err(format!(
            "native {role} result summary contains control text"
        ));
    }
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .filter(|reason| reason.len() <= 8192)
        .ok_or_else(|| format!("native {role} result has no bounded reason"))?;
    if reason.chars().any(char::is_control) {
        return Err(format!("native {role} result reason contains control text"));
    }
    if role == "reviewer" {
        let valid = match object["outcome"].as_str() {
            Some("approved") => reason.is_empty(),
            Some("rework") => reason == "review_finding",
            Some("blocked") => reason == "blocked",
            Some("unavailable") => crate::run::state::FALLBACK_REASONS.contains(&reason),
            _ => false,
        };
        if !valid {
            return Err("native reviewer reason does not match its outcome".into());
        }
    }
    allowed_outcome(role, object["outcome"].as_str().unwrap_or(""))
}

pub(super) fn projected_result(
    role: &str,
    final_result: &Value,
    observation: &Value,
) -> Result<Value, String> {
    let outcome = allowed_outcome(role, final_result["outcome"].as_str().unwrap_or(""))?;
    let mut result = final_result.clone();
    result["outcome"] = json!(outcome);
    result["nativeObservation"] = observation.clone();
    Ok(result)
}

pub(super) fn projection(observation: &Observation) -> Value {
    json!({
        "ephemeral": observation.ephemeral,
        "process": {
            "code": observation.process.code,
            "signal": observation.process.signal,
            "success": observation.process.success,
            "timedOut": observation.process.timed_out,
        },
        "turn": observation.turn.as_str(),
        "commands": observation.command_outcomes.iter().map(|command| json!({
            "status": command.status,
            "exitCode": command.exit_code,
            "outputBytes": command.output_bytes,
            "invocationSha256": command.invocation_sha256,
            "hostItemId": command.host_item_id,
        })).collect::<Vec<_>>(),
        "unobservedItemCount": observation.unobserved_item_count,
        "usage": observation.usage.as_ref().map(|usage| json!({
            "source": usage.source,
            "inputTokens": usage.input_tokens,
            "cachedInputTokens": usage.cached_input_tokens,
            "outputTokens": usage.output_tokens,
        })),
        "outcomeAccount": crate::host::codex_exec::usage_account(observation),
        "threadId": observation.thread_id,
        "diagnostic": diagnostic_projection(observation),
        "coverageGaps": observation.coverage_gap.iter().map(CoverageGap::as_str).collect::<Vec<_>>(),
        "interrupted": observation.interrupted,
    })
}

fn diagnostic_projection(observation: &Observation) -> Value {
    let diagnostic = &observation.diagnostic;
    json!({
        "status": match diagnostic.status {
            DiagnosticStatus::Observed => "observed",
        },
        "codes": diagnostic.codes.iter().map(|code| code.as_str()).collect::<Vec<_>>(),
        "stdoutBytes": diagnostic.stdout_bytes,
        "stderrBytes": diagnostic.stderr_bytes,
    })
}

pub(super) fn request_projection(request: &Request) -> Value {
    json!({
        "executable": request.executable,
        "model": request.model,
        "reasoningEffort": request.effort,
        "sandbox": request.sandbox,
        "timeoutMs": request.timeout.as_millis() as u64,
        "persistSession": request.persist_session,
        "resumed": request.resume_thread_id.is_some(),
        "controlledEffects": request.controlled_tool.is_some(),
        "controlledSession": request.controlled_tool.as_ref().map(|tool| &tool.session),
    })
}

pub(super) fn bounded_json(value: &Value, limit: usize, label: &str) -> Result<String, String> {
    let text = serde_json::to_string(value).map_err(|error| error.to_string())?;
    if text.len() > limit {
        return Err(format!("{label} exceeds its bound"));
    }
    Ok(text)
}
