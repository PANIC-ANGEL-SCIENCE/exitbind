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
    policy: crate::run_value::PreReturnPolicy,
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
    let old_policy = crate::run_value::pre_return_policy_from_value(old_policy, 0)?;
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
