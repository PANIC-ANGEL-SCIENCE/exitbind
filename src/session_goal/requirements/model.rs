//! Bounded requirement, history and declared Work-coverage representation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) const MAX_SOURCE: usize = 32 * 1024;
pub(super) const MAX_REQUIREMENTS: usize = 32;
pub(super) const MAX_MAPPINGS: usize = 128;
pub(super) const PREFIX: &str = "exitbind.goal-support.v1:";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Source {
    #[serde(rename = "ref")]
    pub reference: String,
    pub text: String,
    pub sha256: String,
    pub coverage_confirmed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RequirementRef {
    pub id: String,
    pub revision: u64,
    pub text: String,
    pub source_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Requirement {
    pub id: String,
    pub revision: u64,
    pub text: String,
    pub source: Source,
    pub history: Vec<RequirementRef>,
}

impl Requirement {
    pub fn reference(&self) -> RequirementRef {
        RequirementRef {
            id: self.id.clone(),
            revision: self.revision,
            text: self.text.clone(),
            source_sha256: self.source.sha256.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Mapping {
    pub work: String,
    pub requirement: RequirementRef,
    pub start_event_sha256: String,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Goal {
    pub kind: String,
    pub version: u64,
    pub project_identity: String,
    pub source: Source,
    pub requirements: Vec<Requirement>,
    pub mappings: Vec<Mapping>,
}

impl Goal {
    pub fn read(record: &Value) -> Result<Self, String> {
        let goal: Self = serde_json::from_value(record["continuation"].clone())
            .map_err(|_| "named goal record is malformed")?;
        if goal.kind != "goal_requirements"
            || goal.version != 2
            || goal.requirements.is_empty()
            || goal.requirements.len() > MAX_REQUIREMENTS
            || goal.mappings.len() > MAX_MAPPINGS
        {
            return Err("named goal version or bounds are invalid".into());
        }
        validate_source(&goal.source)?;
        let mut ids = std::collections::BTreeSet::new();
        for requirement in &goal.requirements {
            validate_text(&requirement.id, 64)?;
            validate_text(&requirement.text, 1024)?;
            validate_source(&requirement.source)?;
            if !ids.insert(&requirement.id)
                || requirement.revision == 0
                || requirement.history.len() > MAX_REQUIREMENTS
                || !requirement.source.text.contains(&requirement.text)
            {
                return Err("requirement identity, revision or source is invalid".into());
            }
        }
        Ok(goal)
    }
}

pub(super) fn validate_text(text: &str, max: usize) -> Result<(), String> {
    if text.trim().is_empty() || text.len() > max || text.contains('\0') {
        return Err("requirement text/identity is empty, oversized or contains NUL".into());
    }
    Ok(())
}

fn validate_source(source: &Source) -> Result<(), String> {
    validate_text(&source.reference, 256)?;
    validate_text(&source.text, MAX_SOURCE)?;
    if source.sha256 != crate::evidence::hash::text(&source.text) {
        return Err("requirement source hash mismatch".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CheckerInput {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Contract {
    pub goal_id: String,
    pub project_identity: String,
    pub integration: bool,
    pub requirements: Vec<BoundRequirement>,
    pub checker_input: CheckerInput,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BoundRequirement {
    pub id: String,
    pub revision: u64,
    pub text_sha256: String,
    pub source_sha256: String,
}

impl From<&RequirementRef> for BoundRequirement {
    fn from(value: &RequirementRef) -> Self {
        Self {
            id: value.id.clone(),
            revision: value.revision,
            text_sha256: crate::evidence::hash::text(&value.text),
            source_sha256: value.source_sha256.clone(),
        }
    }
}

impl Contract {
    pub fn covers(&self, reference: &RequirementRef) -> bool {
        self.requirements
            .contains(&BoundRequirement::from(reference))
    }
}
