//! Bounded planning data. Source text never supplies execution authority.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, Path};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Spec {
    pub(crate) version: u64,
    pub(crate) project_id: String,
    pub(crate) mandate: Mandate,
    pub(crate) sources: Vec<Source>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Mandate {
    pub(crate) id: String,
    pub(crate) revision: u64,
    pub(crate) outcome: String,
    pub(crate) mode: Mode,
    pub(crate) allowed_sources: Vec<String>,
    pub(crate) write: Vec<String>,
    pub(crate) commands: Vec<String>,
    pub(crate) roles: Vec<String>,
    pub(crate) preserve: Vec<String>,
    pub(crate) priority_policy: String,
    pub(crate) expires_at: String,
    pub(crate) max_works: u64,
    pub(crate) max_actions: u64,
    pub(crate) max_parallel: u64,
    pub(crate) max_elapsed_seconds: u64,
    pub(crate) external_publication: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Mode {
    Propose,
    Cycle,
    Continue,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) id: String,
    pub(crate) kind: SourceKind,
    #[serde(deserialize_with = "present_option")]
    pub(crate) repository: Option<String>,
    pub(crate) revision: String,
    pub(crate) path: String,
    #[serde(deserialize_with = "present_option")]
    pub(crate) section: Option<String>,
    pub(crate) sha256: String,
    pub(crate) acquired_at: String,
    pub(crate) currentness: String,
    pub(crate) trust: Trust,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SourceKind {
    Git,
    Export,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Trust {
    OwnerDirection,
    Reference,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Plan {
    pub(crate) version: u64,
    pub(crate) id: String,
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) rationale: String,
    pub(crate) outcome: String,
    pub(crate) acceptance: Vec<String>,
    pub(crate) preservation: Vec<String>,
    pub(crate) source_ids: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) workflow: String,
    pub(crate) write: Vec<String>,
    pub(crate) check_command: String,
    pub(crate) preservation_check_command: String,
    pub(crate) estimated_actions: u64,
    pub(crate) overlap_reason: String,
    pub(crate) goal_id: String,
    pub(crate) requirements: Vec<String>,
    pub(crate) checker: String,
    pub(crate) integration: bool,
}

fn text(value: &str, max: usize, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!(
            "{label} must contain nonblank text of at most {max} bytes without control characters"
        ));
    }
    Ok(())
}

pub(super) fn bounded(value: &str, label: &str, max: usize) -> Result<(), String> {
    text(value, max, label)
}

// Optional provenance values are explicit nulls, never silently missing
// fields. This keeps the public form's complete versioned shape fail-closed.
fn present_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn identifier(value: &str, label: &str) -> Result<(), String> {
    text(value, 64, label)?;
    if !value
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
    {
        return Err(format!(
            "{label} must use lowercase ASCII letters, digits, underscore or hyphen"
        ));
    }
    Ok(())
}

fn list(
    values: &[String],
    min: usize,
    max: usize,
    bytes: usize,
    label: &str,
) -> Result<(), String> {
    if !(min..=max).contains(&values.len()) {
        return Err(format!("{label} must contain {min}..={max} entries"));
    }
    let mut seen = BTreeSet::new();
    for value in values {
        text(value, bytes, label)?;
        if !seen.insert(value) {
            return Err(format!("{label} contains a duplicate entry"));
        }
    }
    Ok(())
}

pub(super) fn validate_path(value: &str) -> Result<(), String> {
    text(value, 256, "project path")?;
    if value != value.trim()
        || value.contains('\\')
        || value.contains(':')
        || value.chars().any(|c| "*?[]".contains(c))
        || Path::new(value).is_absolute()
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || !Path::new(value)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        || value.split('/').any(|part| {
            matches!(
                part,
                ".git" | ".exitbind" | ".soulmate" | ".ssh" | ".aws" | ".gnupg"
            ) || part == ".env"
                || part.starts_with(".env.")
        })
    {
        return Err("project path must be confined, normalized and free of wildcards, protected state or credential paths".into());
    }
    Ok(())
}

pub(super) fn contains_path(scope: &str, path: &str) -> bool {
    scope == path || path.starts_with(&format!("{scope}/"))
}

fn timestamp(value: &str, label: &str) -> Result<(), String> {
    text(value, 64, label)?;
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| format!("{label} must be an RFC3339 timestamp with an explicit offset"))
}

impl Spec {
    /// Structural validation preserves expired history. Admission checks the
    /// actual clock separately; parsing a document never renews its mandate.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported project continuation specification version".into());
        }
        identifier(&self.project_id, "projectId")?;
        let m = &self.mandate;
        identifier(&m.id, "mandate id")?;
        if m.revision == 0 {
            return Err("mandate revision must be positive".into());
        }
        text(&m.outcome, 4096, "mandate outcome")?;
        text(&m.priority_policy, 2048, "priority policy")?;
        timestamp(&m.expires_at, "mandate expiry")?;
        if !(1..=64).contains(&m.max_works)
            || !(1..=65536).contains(&m.max_actions)
            || m.max_parallel == 0
            || m.max_parallel > m.max_works
            || !(1..=31_536_000).contains(&m.max_elapsed_seconds)
        {
            return Err("mandate needs finite nonzero ceilings: at most 64 Works, 65536 actions, one year and maxParallel <= maxWorks".into());
        }
        list(&m.allowed_sources, 1, 8, 64, "allowed sources")?;
        list(&m.write, 0, 32, 256, "mandate write paths")?;
        for path in &m.write {
            validate_path(path)?;
        }
        list(&m.commands, 1, 32, 4096, "mandate commands")?;
        list(&m.roles, 1, 4, 16, "mandate roles")?;
        if m.roles
            .iter()
            .any(|r| !matches!(r.as_str(), "lead" | "worker" | "reviewer" | "adviser"))
        {
            return Err("mandate roles must name lead, worker, reviewer or adviser".into());
        }
        list(&m.preserve, 1, 16, 1024, "mandate preservation")?;
        if self.sources.is_empty() || self.sources.len() > 8 {
            return Err("specification requires one to eight approved sources".into());
        }
        let mut ids = BTreeSet::new();
        let mut direction = false;
        for source in &self.sources {
            identifier(&source.id, "source id")?;
            if !ids.insert(&source.id) || !m.allowed_sources.contains(&source.id) {
                return Err(
                    "source ids must be unique and explicitly allowed by the mandate".into(),
                );
            }
            validate_path(&source.path)?;
            text(&source.revision, 128, "source revision")?;
            timestamp(&source.acquired_at, "source acquisition")?;
            if let Some(repository) = &source.repository {
                text(repository, 1024, "source repository")?;
                if repository.contains("://")
                    && repository
                        .split("://")
                        .nth(1)
                        .unwrap_or("")
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .contains('@')
                {
                    return Err("source repository must not contain URL credentials".into());
                }
            }
            if let Some(section) = &source.section {
                text(section, 256, "source section locator")?;
            }
            if source.sha256.len() != 64 || !lower_hex(&source.sha256) {
                return Err(
                    "source SHA256 must contain 64 lowercase hexadecimal characters".into(),
                );
            }
            match source.kind {
                SourceKind::Git if source.repository.is_some()
                    && matches!(source.revision.len(), 40 | 64)
                    && lower_hex(&source.revision)
                    && source.currentness == "checkout_head" => {}
                SourceKind::Export if source.currentness == "declared_export" => {}
                _ => return Err("Git sources require an exact commit/repository and checkout_head; exports require declared_export".into()),
            }
            direction |= source.trust == Trust::OwnerDirection;
        }
        if !direction || m.allowed_sources.iter().any(|id| !ids.contains(id)) {
            return Err(
                "approved sources must include ownerDirection and resolve every allowed source id"
                    .into(),
            );
        }
        Ok(())
    }
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Plan {
    pub(crate) fn validate(&self, spec: &Spec) -> Result<(), String> {
        spec.validate()?;
        if self.version != 1 {
            return Err("unsupported project continuation plan version".into());
        }
        for (value, label) in [
            (&self.id, "plan id"),
            (&self.task_id, "task id"),
            (&self.workflow, "workflow"),
            (&self.goal_id, "Goal id"),
        ] {
            identifier(value, label)?;
        }
        text(&self.title, 1024, "plan title")?;
        text(&self.rationale, 4096, "plan rationale")?;
        text(&self.outcome, 4096, "plan outcome")?;
        text(&self.overlap_reason, 2048, "overlap judgment")?;
        list(&self.acceptance, 1, 16, 1024, "plan acceptance")?;
        list(&self.preservation, 1, 16, 1024, "plan preservation")?;
        if spec
            .mandate
            .preserve
            .iter()
            .any(|p| !self.preservation.contains(p))
        {
            return Err("plan must retain every mandate preservation condition".into());
        }
        list(&self.source_ids, 1, 8, 64, "plan sources")?;
        if self
            .source_ids
            .iter()
            .any(|id| !spec.mandate.allowed_sources.contains(id))
            || !spec
                .sources
                .iter()
                .any(|s| s.trust == Trust::OwnerDirection && self.source_ids.contains(&s.id))
        {
            return Err("plan must cite approved sources including ownerDirection; reference data grants no authority".into());
        }
        list(&self.dependencies, 0, 64, 64, "plan dependencies")?;
        for dependency in &self.dependencies {
            identifier(dependency, "dependency task id")?;
            if dependency == &self.task_id {
                return Err("a plan task cannot depend on itself".into());
            }
        }
        list(&self.write, 0, 32, 256, "plan write paths")?;
        for path in &self.write {
            validate_path(path)?;
            if !spec
                .mandate
                .write
                .iter()
                .any(|scope| contains_path(scope, path))
            {
                return Err("plan write path exceeds the mandate".into());
            }
        }
        for command in [&self.check_command, &self.preservation_check_command] {
            text(command, 4096, "plan check command")?;
            if !spec.mandate.commands.contains(command) {
                return Err("plan check commands must exactly match mandate commands".into());
            }
        }
        if self.estimated_actions == 0 || self.estimated_actions > spec.mandate.max_actions {
            return Err("plan action estimate must be positive and within the mandate".into());
        }
        list(&self.requirements, 1, 32, 64, "Goal requirements")?;
        for requirement in &self.requirements {
            identifier(requirement, "Goal requirement id")?;
        }
        validate_path(&self.checker)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn spec() -> Spec {
        serde_json::from_value(json!({"version":1,"projectId":"example","mandate":{
            "id":"advance","revision":1,"outcome":"Make the project useful.","mode":"continue",
            "allowedSources":["brief"],"write":["src"],"commands":["python3 checks.py"],
            "roles":["worker","reviewer"],"preserve":["Keep existing records."],"priorityPolicy":"Use current evidence.",
            "expiresAt":"2020-01-01T00:00:00Z","maxWorks":2,"maxActions":12,"maxParallel":1,
            "maxElapsedSeconds":600,"externalPublication":false},"sources":[{
            "id":"brief","kind":"export","repository":null,"revision":"export-1","path":"docs/brief.md",
            "section":null,"sha256":"a".repeat(64),"acquiredAt":"2019-01-01T00:00:00Z",
            "currentness":"declared_export","trust":"ownerDirection"}]})).unwrap()
    }

    fn plan() -> Plan {
        serde_json::from_value(json!({"version":1,"id":"selected","taskId":"parse-records",
            "title":"Preserve records when parsing","rationale":"Current behavior loses useful records.",
            "outcome":"Make the project useful.","acceptance":["Valid records survive parsing."],
            "preservation":["Keep existing records."],"sourceIds":["brief"],"dependencies":[],"workflow":"change",
            "write":["src/parser.py"],"checkCommand":"python3 checks.py","preservationCheckCommand":"python3 checks.py",
            "estimatedActions":4,"overlapReason":"No running task owns parsing.","goalId":"example-goal",
            "requirements":["records"],"checker":"checks.py","integration":false})).unwrap()
    }

    #[test]
    fn confined_paths_reject_protected_credentials_and_wildcards() {
        for path in [
            ".",
            "../brief.md",
            "/brief.md",
            "docs/../brief.md",
            "docs//brief.md",
            "src/**",
            ".git/config",
            ".exitbind/memory/a",
            "x/.ssh/id_rsa",
            ".env.local",
            "C:/brief.md",
        ] {
            assert!(validate_path(path).is_err(), "{path}");
        }
        for path in ["brief.md", "src/parser.rs", "docs/project direction.md"] {
            validate_path(path).unwrap();
        }
    }

    #[test]
    fn mode_and_trust_are_exact_and_unknown_fields_fail_closed() {
        assert_eq!(
            serde_json::from_str::<Mode>("\"continue\"").unwrap(),
            Mode::Continue
        );
        assert_eq!(
            serde_json::from_str::<Trust>("\"ownerDirection\"").unwrap(),
            Trust::OwnerDirection
        );
        assert!(serde_json::from_str::<Mode>("\"unlimited\"").is_err());
        assert!(serde_json::from_str::<Source>(r#"{"id":"brief","kind":"export","repository":null,"revision":"v1","path":"brief.md","section":null,"sha256":"a","acquiredAt":"2026-01-01T00:00:00Z","currentness":"declared_export","trust":"reference","grant":"all"}"#).is_err());
        let mut missing: Value = serde_json::to_value(&spec().sources[0]).unwrap();
        missing.as_object_mut().unwrap().remove("repository");
        assert!(serde_json::from_value::<Source>(missing).is_err());
    }

    #[test]
    fn structural_validation_keeps_expired_history_but_never_unbounded_or_reference_authority() {
        let mut approved = spec();
        approved.validate().unwrap();
        approved.sources[0].trust = Trust::Reference;
        assert!(approved.validate().unwrap_err().contains("ownerDirection"));
        approved = spec();
        approved.mandate.max_actions = 0;
        assert!(approved.validate().is_err());
        approved = spec();
        approved.mandate.max_parallel = 3;
        assert!(approved.validate().is_err());
        approved = spec();
        approved.sources[0].currentness = "latest".into();
        assert!(approved.validate().is_err());
        approved = spec();
        approved.mandate.expires_at = "tomorrow".into();
        assert!(approved.validate().is_err());
    }

    #[test]
    fn plan_cannot_expand_paths_commands_sources_or_weaken_preservation() {
        let approved = spec();
        plan().validate(&approved).unwrap();
        let mut candidate = plan();
        candidate.write = vec!["src-other/parser.py".into()];
        assert!(candidate
            .validate(&approved)
            .unwrap_err()
            .contains("write path"));
        candidate = plan();
        candidate.check_command = "python3 checks.py && publish".into();
        assert!(candidate
            .validate(&approved)
            .unwrap_err()
            .contains("exactly"));
        candidate = plan();
        candidate.preservation = vec!["A weaker condition.".into()];
        assert!(candidate
            .validate(&approved)
            .unwrap_err()
            .contains("retain"));
        candidate = plan();
        candidate.source_ids = vec!["imported-comment".into()];
        assert!(candidate
            .validate(&approved)
            .unwrap_err()
            .contains("approved sources"));
        candidate = plan();
        candidate.dependencies = vec![candidate.task_id.clone()];
        assert!(candidate
            .validate(&approved)
            .unwrap_err()
            .contains("itself"));
    }
}
