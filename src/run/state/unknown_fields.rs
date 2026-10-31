use serde_json::{Map, Value};

pub(super) fn reject_unknown(
    object: &Map<String, Value>,
    action: &str,
    version: u64,
    line: usize,
) -> Result<(), String> {
    let allowed: &[&str] = if action == "start" {
        let mut allowed = vec![
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "workflow",
            "goal",
            "configSha256",
            "plan",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
            "supersedes",
        ];
        if version == 2 {
            allowed.push("harnessReceipt");
        }
        if matches!(version, 3..=8) {
            allowed.push("harnessReceipt");
            allowed.push("checkPolicy");
        }
        if version == 8 {
            allowed.push("preReturnPolicy");
            allowed.push("projectContinuation");
        }
        if version >= 5 {
            allowed.push("subject");
        }
        if version >= 6 {
            allowed.push("preservation");
        }
        if version >= 6 {
            allowed.push("governor");
        }
        if version >= 8 {
            allowed.push("basisProtocol");
            allowed.push("basis");
            allowed.push("reviewPolicy");
            allowed.push("recoveryProtocol");
            allowed.push(super::observation_novelty::FIELD);
            allowed.push("checkObservationProtocol");
            allowed.push("carryProtocol");
            allowed.push("governorCarry");
            allowed.push("governorCarrySha256");
            allowed.push("checkReplacement");
            allowed.push("quarantineVerification");
            crate::run::verification_preparation::append_allowed_fields(&mut allowed);
            crate::run::guidance_continuity::append_allowed_fields(&mut allowed);
        }
        return object
            .keys()
            .find(|key| !allowed.contains(&key.as_str()))
            .map_or(Ok(()), |key| {
                Err(format!(
                    "invalid run ledger line {line}: unknown field '{key}'"
                ))
            });
    } else if action == "pre_return_reconciled" {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "recovery",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if matches!(
        action,
        "check_observation" | "check_observation_failed" | "check_observation_recovered"
    ) {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "observation",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "submit" {
        let mut allowed = vec![
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "stage",
            "attempt",
            "agent",
            "role",
            "outcome",
            "artifact",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
        ];
        if version >= 5 {
            allowed.push("subjectSha256");
        }
        if version >= 6 {
            allowed.push("inputsSha256");
        }
        if version >= 6 {
            allowed.push("assignmentSha256");
        }
        if version >= 7 {
            allowed.push("fallback");
        }
        if version >= 8 {
            allowed.push("basisSha256");
            allowed.push("reviewDecisionSha256");
            allowed.push("disposition");
            allowed.push("quarantineVerificationAck");
            allowed.push(crate::run::owned_group::ENDED_FIELD);
        }
        if version >= crate::run::lead_reason::FIRST_VERSION {
            allowed.push("reason");
        }
        if version >= 6 {
            allowed.push("governorEvent");
        }
        return reject_unknown_fields(object, &allowed, line);
    } else if action == "review_policy" {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "stage",
                "attempt",
                "agent",
                "role",
                "basisProtocol",
                "basisSha256",
                "previousDecisionSha256",
                "reviewPolicy",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "govern" {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "stage",
                "attempt",
                "agent",
                "role",
                "subjectSha256",
                "inputsSha256",
                "assignmentSha256",
                "assignmentPacketSha256",
                "operation",
                "requestId",
                "requestDigest",
                "governorEvent",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "check" {
        if version == 8 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "inputsSha256",
                    "configSha256",
                    "targetEventSha256",
                    "requirementId",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "observationEventSha256",
                    "stdout",
                    "stderr",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version >= 6 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "inputsSha256",
                    "targetEventSha256",
                    "requirementId",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version == 5 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "targetEventSha256",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version == 4 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "targetEventSha256",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        &[
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "targetEventSha256",
            "checkCommand",
            "checkCommandSha256",
            "origin",
            "exitCode",
            "durationMs",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
        ]
    } else {
        if version >= 5 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "inputsSha256",
                    "stage",
                    "attempt",
                    "actor",
                    "role",
                    "attemptedOutcome",
                    "reason",
                    "checkEvidence",
                    "origin",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version == 4 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "stage",
                    "attempt",
                    "actor",
                    "role",
                    "attemptedOutcome",
                    "reason",
                    "checkEvidence",
                    "origin",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        &[
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "stage",
            "attempt",
            "actor",
            "role",
            "attemptedOutcome",
            "reason",
            "checkEvidence",
            "origin",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
        ]
    };
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map_or(Ok(()), |key| {
            Err(format!(
                "invalid run ledger line {line}: unknown field '{key}'"
            ))
        })
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    line: usize,
) -> Result<(), String> {
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map_or(Ok(()), |key| {
            Err(format!(
                "invalid run ledger line {line}: unknown field '{key}'"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::reject_unknown;
    use serde_json::json;

    /// The Lead's `reason` on a `submit` event passes the field inventory at
    /// the current ledger version (8) and at no version below it.
    #[test]
    fn a_submit_reason_passes_at_the_current_version_and_nowhere_below() {
        for version in 1..=8u64 {
            let base = json!({"version": version, "kind": "run", "action": "submit",
                "role": "lead", "outcome": "accepted"});
            let mut with = base.clone();
            with["reason"] = json!("Accepted after the review.");
            assert!(reject_unknown(base.as_object().unwrap(), "submit", version, 1).is_ok());
            let admitted = reject_unknown(with.as_object().unwrap(), "submit", version, 1);
            assert_eq!(
                admitted.is_ok(),
                version == 8,
                "version {version}: {admitted:?}"
            );
        }
    }
}
