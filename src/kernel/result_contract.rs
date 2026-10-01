//! The role/result vocabulary shared by host-managed and native deliveries.
//!
//! The run reducer remains the authority for transitions.  This contract only
//! defines which role outcomes can be represented at the delivery boundary;
//! stage-specific Lead restrictions are applied by the caller.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Lead,
    Worker,
    Reviewer,
    Adviser,
}

impl Role {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "lead" => Some(Self::Lead),
            "worker" => Some(Self::Worker),
            "reviewer" => Some(Self::Reviewer),
            "adviser" => Some(Self::Adviser),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Worker => "worker",
            Self::Reviewer => "reviewer",
            Self::Adviser => "adviser",
        }
    }

    pub(crate) fn outcomes(self) -> &'static [Outcome] {
        match self {
            Self::Lead => &[
                Outcome::Scoped,
                Outcome::Blocked,
                Outcome::Accepted,
                Outcome::Rework,
                Outcome::Rejected,
                Outcome::Disposition,
            ],
            Self::Worker => &[Outcome::Completed, Outcome::Blocked, Outcome::Contradiction],
            Self::Reviewer => &[
                Outcome::Approved,
                Outcome::Rework,
                Outcome::Blocked,
                Outcome::Unavailable,
            ],
            Self::Adviser => &[Outcome::Completed, Outcome::Blocked],
        }
    }

    pub(crate) fn permits(self, outcome: Outcome) -> bool {
        self.outcomes().contains(&outcome)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Scoped,
    Blocked,
    Accepted,
    Rework,
    Rejected,
    Disposition,
    Completed,
    Contradiction,
    Approved,
    Unavailable,
}

impl Outcome {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "scoped" => Some(Self::Scoped),
            "blocked" => Some(Self::Blocked),
            "accepted" => Some(Self::Accepted),
            "rework" => Some(Self::Rework),
            "rejected" => Some(Self::Rejected),
            "disposition" => Some(Self::Disposition),
            "completed" => Some(Self::Completed),
            "contradiction" => Some(Self::Contradiction),
            "approved" => Some(Self::Approved),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Scoped => "scoped",
            Self::Blocked => "blocked",
            Self::Accepted => "accepted",
            Self::Rework => "rework",
            Self::Rejected => "rejected",
            Self::Disposition => "disposition",
            Self::Completed => "completed",
            Self::Contradiction => "contradiction",
            Self::Approved => "approved",
            Self::Unavailable => "unavailable",
        }
    }
}

pub(crate) fn validate(role: &str, outcome: &str) -> Result<Outcome, String> {
    let role = Role::parse(role).ok_or_else(|| format!("unsupported result role '{role}'"))?;
    let outcome = Outcome::parse(outcome)
        .ok_or_else(|| format!("outcome '{outcome}' is not in the result contract"))?;
    if role.permits(outcome) {
        Ok(outcome)
    } else {
        Err(format!(
            "outcome '{}' is not allowed for role '{}'; permitted outcomes: {}",
            outcome.as_str(),
            role.as_str(),
            permitted_text(role)
        ))
    }
}

pub(crate) fn permitted_text(role: Role) -> String {
    role.outcomes()
        .iter()
        .map(|outcome| outcome.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Return the stable wire spelling used for the on-disk native schema.  The
/// field order is kept compatible with the original schemas while the enum is
/// still sourced from the typed contract above.
pub(crate) fn native_schema_text(role: &str) -> Result<String, String> {
    let role = Role::parse(role).ok_or("native Codex action has an unsupported role")?;
    if role == Role::Lead {
        return Err("native Codex action has an unsupported role".into());
    }
    let outcomes = serde_json::to_string(
        &role
            .outcomes()
            .iter()
            .map(|outcome| outcome.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string())?;
    let common = format!(
        r#"{{"type":"object","additionalProperties":false,"required":["outcome","summary","reason"],"properties":{{"outcome":{{"enum":{outcomes}}},"summary":{{"type":"string","minLength":1,"maxLength":8192}},"reason":{{"type":"string","maxLength":8192}}}}}}"#
    );
    if role == Role::Reviewer {
        Ok(format!(
            r#"{{"type":"object","additionalProperties":false,"required":["outcome","summary","reason","evidenceReferences"],"properties":{{"outcome":{{"enum":{outcomes}}},"summary":{{"type":"string","minLength":1,"maxLength":8192}},"reason":{{"enum":["","review_finding","blocked","provider_quota","rate_limit","provider_unavailable"]}},"evidenceReferences":{{"type":"array","items":{{"type":"string"}}}}}}}}"#
        ))
    } else {
        Ok(common)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_vocabulary_is_disjoint_and_reviewer_excludes_rejected() {
        assert_eq!(validate("worker", "completed"), Ok(Outcome::Completed));
        assert!(validate("reviewer", "rejected").is_err());
        assert_eq!(
            permitted_text(Role::Reviewer),
            "approved, rework, blocked, unavailable"
        );
    }

    #[test]
    fn native_schema_is_strict_and_uses_the_typed_vocabulary() {
        for role in ["worker", "reviewer", "adviser"] {
            let text = native_schema_text(role).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(parsed["additionalProperties"], false);
            let outcomes = parsed["properties"]["outcome"]["enum"].as_array().unwrap();
            assert_eq!(outcomes.len(), Role::parse(role).unwrap().outcomes().len());
        }
    }

    #[test]
    fn native_schema_text_preserves_reviewer_wire_shape() {
        assert_eq!(
            native_schema_text("reviewer").unwrap(),
            r#"{"type":"object","additionalProperties":false,"required":["outcome","summary","reason","evidenceReferences"],"properties":{"outcome":{"enum":["approved","rework","blocked","unavailable"]},"summary":{"type":"string","minLength":1,"maxLength":8192},"reason":{"enum":["","review_finding","blocked","provider_quota","rate_limit","provider_unavailable"]},"evidenceReferences":{"type":"array","items":{"type":"string"}}}}"#
        );
    }
}
