// Validates typed and sealed Goal ownership transitions.
// Refuses invalid correction decisions; see `src/session_goal/owner_correction.rs`.
//! Typed, sealed ownership decisions and their transition validation.

use crate::{evidence::hash, session_goal::recovery::ConfigBinding};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(super) const FIELD: &str = "nativeGoalOwnerCorrections";
const MAX_CORRECTIONS: usize = 16;
const MAX_SOURCES: usize = 33;

/// A committed correction keeps its selected configuration and project
/// identity, while later edits may change configuration bytes. Full binding
/// equality still protects each preview and apply.
pub(crate) fn same_owner_binding(left: &ConfigBinding, right: &ConfigBinding) -> bool {
    left.path == right.path
        && left.product_root == right.product_root
        && left.state_root == right.state_root
        && left.project_id == right.project_id
        && left.project_identity_sha256 == right.project_identity_sha256
        && left.local_identity_sha256 == right.local_identity_sha256
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceRelocation {
    pub(crate) original_ref: String,
    pub(crate) recorded_sha256: String,
    pub(crate) target_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SavedRequest {
    pub(super) owner_config: String,
    pub(super) target_config: String,
    pub(super) relocations: String,
    pub(super) request_id: String,
    pub(super) reason: String,
    pub(super) old_writers_quiesced: bool,
}

impl SavedRequest {
    pub(super) fn digest(&self) -> String {
        hash::value(
            &json!({"ownerConfig":self.owner_config,"targetConfig":self.target_config,
            "relocations":self.relocations,"requestId":self.request_id,"reason":self.reason,
            "oldWritersQuiesced":self.old_writers_quiesced}),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnerCorrectionDecision {
    pub(crate) version: u64,
    pub(crate) goal_id: String,
    pub(crate) host: String,
    pub(crate) session_key: String,
    pub(crate) old_owner_record_sha256: String,
    pub(crate) previous_event_sha256: String,
    pub(crate) previous_revision: u64,
    pub(crate) old_config: ConfigBinding,
    pub(crate) target_config: ConfigBinding,
    pub(crate) reason: String,
    pub(crate) relocations: Vec<SourceRelocation>,
    pub(crate) request_id: String,
    pub(crate) request_sha256: String,
    pub(crate) preview_sha256: String,
    pub(crate) old_writers_quiesced: bool,
    pub(crate) decision_sha256: String,
}

fn valid_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

fn binding_valid(binding: &ConfigBinding) -> bool {
    [&binding.path, &binding.product_root, &binding.state_root]
        .iter()
        .all(|path| valid_text(path, 4096) && std::path::Path::new(path).is_absolute())
        && [
            &binding.config_sha256,
            &binding.project_identity_sha256,
            &binding.local_identity_sha256,
        ]
        .iter()
        .all(|digest| super::super::native::valid_hash(digest))
        && binding
            .project_id
            .as_ref()
            .map_or(true, |id| valid_text(id, 128))
}

impl OwnerCorrectionDecision {
    pub(super) fn preview_digest(&self) -> Result<String, String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let object = value
            .as_object_mut()
            .ok_or("owner correction decision malformed")?;
        object.remove("previewSha256");
        object.remove("decisionSha256");
        Ok(hash::value(&value))
    }

    pub(super) fn digest(&self) -> Result<String, String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        value
            .as_object_mut()
            .ok_or("owner correction decision malformed")?
            .remove("decisionSha256");
        Ok(hash::value(&value))
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        let mut keys = std::collections::BTreeSet::new();
        if self.version != 1
            || !valid_text(&self.goal_id, 128)
            || !matches!(self.host.as_str(), "codex" | "claude")
            || !super::super::native::valid_hash(&self.session_key)
            || !super::super::native::valid_hash(&self.old_owner_record_sha256)
            || !super::super::native::valid_hash(&self.previous_event_sha256)
            || self.previous_revision == 0
            || !binding_valid(&self.old_config)
            || !binding_valid(&self.target_config)
            || !valid_text(&self.reason, 1024)
            || !valid_text(&self.request_id, 128)
            || !super::super::native::valid_hash(&self.request_sha256)
            || !self.old_writers_quiesced
            || self.relocations.is_empty()
            || self.relocations.len() > MAX_SOURCES
            || self.relocations.iter().any(|source| {
                !valid_text(&source.original_ref, 256)
                    || !valid_text(&source.target_path, 4096)
                    || !super::super::native::valid_hash(&source.recorded_sha256)
                    || !keys.insert((&source.original_ref, &source.recorded_sha256))
            })
            || self.preview_digest()? != self.preview_sha256
            || self.digest()? != self.decision_sha256
        {
            return Err("native Goal owner correction decision is invalid".into());
        }
        Ok(())
    }
}

pub(super) fn decisions(record: &Value) -> Result<Vec<OwnerCorrectionDecision>, String> {
    let Some(value) = record.get(FIELD) else {
        return Ok(Vec::new());
    };
    let items: Vec<OwnerCorrectionDecision> = serde_json::from_value(value.clone())
        .map_err(|_| "native Goal owner correction history is malformed")?;
    if items.is_empty() || items.len() > MAX_CORRECTIONS {
        return Err("native Goal owner correction history bound is invalid".into());
    }
    for item in &items {
        item.validate()?;
    }
    Ok(items)
}

pub(in crate::session_goal) fn validate_transition(
    record: &Value,
    previous: Option<&Value>,
    line: usize,
) -> Result<(), String> {
    let current = decisions(record)?;
    let prior = previous.map(decisions).transpose()?.unwrap_or_default();
    let same_goal = previous.is_some_and(|old| old["goalId"] == record["goalId"]);
    if !same_goal {
        if !current.is_empty() {
            return Err(format!("invalid canonical session goal line {line}: owner correction cannot cross Goal identity"));
        }
        return Ok(());
    }
    if current == prior {
        return Ok(());
    }
    if current.len() != prior.len() + 1 || current[..prior.len()] != prior {
        return Err(format!(
            "invalid canonical session goal line {line}: owner correction history was altered"
        ));
    }
    let old = previous.ok_or("owner correction has no predecessor")?;
    let decision = current.last().ok_or("owner correction is missing")?;
    if decision.goal_id != record["goalId"].as_str().unwrap_or_default()
        || Some(decision.previous_revision) != old["revision"].as_u64()
        || Some(decision.previous_event_sha256.as_str()) != old["eventSha256"].as_str()
        || prior
            .last()
            .is_some_and(|prior| !same_owner_binding(&prior.target_config, &decision.old_config))
    {
        return Err(format!(
            "invalid canonical session goal line {line}: owner correction predecessor mismatch"
        ));
    }
    let mut expected = old.clone();
    expected["revision"] = record["revision"].clone();
    expected["predecessor"] = json!({"goalId":old["goalId"],"revision":old["revision"]});
    expected["successorOf"] = Value::Null;
    expected["previousEventSha256"] = record["previousEventSha256"].clone();
    expected["eventSha256"] = record["eventSha256"].clone();
    expected[FIELD] = record[FIELD].clone();
    if &expected != record {
        return Err(format!(
            "invalid canonical session goal line {line}: owner correction changed Goal content"
        ));
    }
    Ok(())
}

pub(in crate::session_goal) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] == next["goalId"]
        && previous.get(FIELD).is_some()
        && next.get(FIELD).is_none()
    {
        next[FIELD] = previous[FIELD].clone();
    }
}

#[cfg(test)]
mod tests {
    use super::{same_owner_binding, ConfigBinding};

    #[test]
    fn committed_binding_ignores_only_config_bytes() {
        let binding = ConfigBinding {
            path: "/project/exitbind.json".into(),
            config_sha256: "config-a".into(),
            product_root: "/project".into(),
            state_root: "/state".into(),
            project_id: Some("project".into()),
            project_identity_sha256: "project-identity".into(),
            local_identity_sha256: "local-identity".into(),
        };
        let mut changed = binding.clone();
        changed.config_sha256 = "config-b".into();
        assert!(same_owner_binding(&binding, &changed));
        for changed in [
            ConfigBinding {
                path: "/other/config".into(),
                ..binding.clone()
            },
            ConfigBinding {
                product_root: "/other".into(),
                ..binding.clone()
            },
            ConfigBinding {
                state_root: "/other-state".into(),
                ..binding.clone()
            },
            ConfigBinding {
                project_id: Some("other".into()),
                ..binding.clone()
            },
            ConfigBinding {
                project_identity_sha256: "other".into(),
                ..binding.clone()
            },
            ConfigBinding {
                local_identity_sha256: "other".into(),
                ..binding.clone()
            },
        ] {
            assert!(!same_owner_binding(&binding, &changed));
        }
    }
}
