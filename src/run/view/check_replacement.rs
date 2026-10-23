use super::*;

/// Continue an explicitly defective frozen check contract under the same
/// Work/Goal lineage. The caller must supply the exact predecessor evidence
/// and Lead decision; ordinary supersession never accepts this marker.
#[allow(clippy::too_many_arguments)]
pub fn replace_check_contract(
    loaded: &Loaded,
    old_ledger: &str,
    workflow: &str,
    goal: &str,
    new_ledger: &str,
    check_command: &str,
    proof_origin: Option<&str>,
    reason: &str,
    source_kind: &str,
    source_event_sha256: &str,
    expected_head_event_sha256: &str,
    lead_agent: &str,
    lead_native_task_name: &str,
) -> Result<Value, String> {
    supersede_with_policy_and_recovery(
        loaded,
        old_ledger,
        workflow,
        goal,
        new_ledger,
        None,
        None,
        Some(check_command),
        proof_origin,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(expected_head_event_sha256),
        Some(json!({
            "protocol": 1,
            "classification": "defective_contract",
            "reason": reason,
            "source": {"kind": source_kind, "eventSha256": source_event_sha256},
            "leadOwner": {"agent": lead_agent, "nativeTaskName": lead_native_task_name},
        })),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn replace_pre_return_contract(
    loaded: &Loaded,
    old_ledger: &str,
    workflow: &str,
    goal: &str,
    new_ledger: &str,
    policy: crate::run::gate::value::PreReturnPolicy,
    reason: &str,
    source_event_sha256: &str,
    attempt_id: &str,
    expected_head_event_sha256: &str,
    lead_agent: &str,
    lead_native_task_name: &str,
) -> Result<Value, String> {
    let (_, old_events, _) = load(loaded, old_ledger)?;
    let old_state = reduce_live(loaded, &old_events)?;
    let old_policy = old_state
        .get("preReturnPolicy")
        .ok_or("pre-return replacement predecessor has no frozen policy")?;
    let old_policy = crate::run::gate::value::pre_return_policy_from_value(old_policy, 0)?;
    let old_policy_sha256 = crate::evidence::hash::value(&old_policy.value());
    let new_policy_sha256 = crate::evidence::hash::value(&policy.value());
    supersede_with_policy_and_recovery(
        loaded,
        old_ledger,
        workflow,
        goal,
        new_ledger,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(expected_head_event_sha256),
        Some(json!({
            "protocol": 1,
            "target": "pre_return",
            "classification": "defective_contract",
            "reason": reason,
            "source": {"kind":"blocked_worker","eventSha256":source_event_sha256,"attemptId":attempt_id},
            "leadOwner": {"agent":lead_agent,"nativeTaskName":lead_native_task_name},
            "oldPolicy": {"policySha256":old_policy_sha256},
            "newPolicy": {"policySha256":new_policy_sha256},
        })),
        Some(policy),
    )
}

/// Bind a replacement marker to the predecessor it replaces: the exact
/// failed-check or blocked-worker source, the predecessor head, the changed
/// successor policy (check or pre-return), and the named Lead owner. A
/// pre-return replacement also selects the successor's pre-return policy.
#[allow(clippy::too_many_arguments)]
pub(super) fn bind_marker(
    loaded: &Loaded,
    old: &run_ledger::LedgerPath,
    old_events: &[Value],
    old_state: &Value,
    replacement: &mut Value,
    requested_policy: Option<&crate::run::gate::value::CheckPolicy>,
    pre_return_policy: &mut Option<Value>,
    pre_return_policy_override: Option<&crate::run::gate::value::PreReturnPolicy>,
) -> Result<(), String> {
    if replacement["classification"] != "defective_contract"
        || replacement["reason"]
            .as_str()
            .map_or(true, |value| value.trim().is_empty())
    {
        return Err(
            "check replacement requires an explicit defective-contract classification and reason"
                .into(),
        );
    }
    let source_kind = replacement["source"]["kind"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let source_sha = replacement["source"]["eventSha256"]
        .as_str()
        .unwrap_or_default();
    let source = old_events
        .iter()
        .find(|event| event["eventSha256"] == source_sha)
        .ok_or("check replacement source evidence is not in the predecessor ledger")?;
    let source_valid = match source_kind.as_str() {
        "failed_check" => {
            source["action"] == "check"
                && (source["result"]["signal"].is_number()
                    || source["result"]["code"]
                        .as_i64()
                        .is_some_and(|code| code != 0)
                    || source["exitCode"].as_i64().is_some_and(|code| code != 0))
        }
        "blocked_worker" => {
            source["action"] == "submit"
                && source["role"] == "worker"
                && source["outcome"] == "blocked"
        }
        _ => false,
    };
    if !source_valid {
        return Err(
            "check replacement source must be an exact failed-check or blocked-worker event".into(),
        );
    }
    let target = replacement["target"].as_str().unwrap_or("check").to_owned();
    replacement["predecessorHeadEventSha256"] = old_events
        .last()
        .map(|event| event["eventSha256"].clone())
        .unwrap_or(Value::Null);
    match target.as_str() {
        "check" => {
            if pre_return_policy_override.is_some()
                || old_state.get("checkPolicy").is_none()
                || requested_policy.is_none()
            {
                return Err(
                    "check replacement requires frozen predecessor and successor check policies"
                        .into(),
                );
            }
            let previous =
                crate::run::gate::value::policy_from_value(&old_state["checkPolicy"], 0)?;
            let successor = requested_policy.expect("checked above");
            if previous == *successor {
                return Err("check replacement must identify a changed successor policy".into());
            }
            replacement["oldPolicy"] = json!({
                "commandSha256": previous.command_sha256,
                "origin": previous.origin.as_str(),
            });
            replacement["newPolicy"] = json!({
                "commandSha256": successor.command_sha256,
                "origin": successor.origin.as_str(),
            });
        }
        "pre_return" => {
            if requested_policy.is_some() || source_kind != "blocked_worker" {
                return Err("pre-return replacement requires a blocked Worker source and preserves the final check policy".into());
            }
            let previous = pre_return_policy
                .as_ref()
                .ok_or("pre-return replacement requires a frozen predecessor policy")?;
            let successor = pre_return_policy_override
                .as_ref()
                .ok_or("pre-return replacement policy is missing")?;
            let attempt_id = replacement["source"]["attemptId"]
                .as_str()
                .ok_or("pre-return replacement must identify the diagnostic attempt")?;
            let validated_attempt = crate::run::pre_return::replacement_source_attempt(
                loaded,
                old,
                &old_events[0],
                source,
            )?;
            if validated_attempt != attempt_id {
                return Err("blocked report diagnostic attempt changed after preview".into());
            }
            if previous == &successor.value() {
                return Err(
                    "replacement pre-return policy must differ from the frozen predecessor".into(),
                );
            }
            let old_sha = crate::evidence::hash::value(previous);
            let new_sha = crate::evidence::hash::value(&successor.value());
            if replacement["oldPolicy"]["policySha256"] != old_sha
                || replacement["newPolicy"]["policySha256"] != new_sha
            {
                return Err(
                    "pre-return replacement marker does not match its frozen policies".into(),
                );
            }
            *pre_return_policy = Some(successor.value());
        }
        _ => return Err("check replacement target is invalid".into()),
    }
    for field in ["agent", "nativeTaskName"] {
        if replacement["leadOwner"][field]
            .as_str()
            .map_or(true, |value| value.trim().is_empty())
        {
            return Err("check replacement must name the configured Lead owner".into());
        }
    }
    Ok(())
}
