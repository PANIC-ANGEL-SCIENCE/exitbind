use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The validated, typed representation of one configured agent.
///
/// Unknown fields are intentionally ignored here. `config::validate` is the
/// sole compatibility authority; this type is derived only after it accepts
/// the raw value and never broadens or narrows the public configuration shape.
#[derive(Clone, Debug, Deserialize)]
pub struct AgentConfig {
    pub profile: String,
    pub purpose: String,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(rename = "nativeName", default)]
    pub native_name: Option<String>,
    #[serde(default)]
    pub observe: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(rename = "memoryRead", default)]
    pub memory_read: Vec<String>,
    #[serde(rename = "memoryWrite", default)]
    pub memory_write: Vec<String>,
    #[serde(rename = "memoryReview", default)]
    pub memory_review: Vec<String>,
    #[serde(rename = "memoryPromote", default)]
    pub memory_promote: Vec<String>,
    #[serde(rename = "memoryReject", default)]
    pub memory_reject: Vec<String>,
    #[serde(rename = "memoryRevoke", default)]
    pub memory_revoke: Vec<String>,
    #[serde(rename = "memoryExpire", default)]
    pub memory_expire: Vec<String>,
    #[serde(rename = "memoryForget", default)]
    pub memory_forget: Vec<String>,
    pub retention: String,
    #[serde(rename = "crossContext")]
    pub cross_context: String,
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// Requested host runtime bindings for an agent.
#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(rename = "reasoningEffort", default)]
    pub reasoning_effort: Option<String>,
    /// "none" declines fallback; otherwise an alternate execution binding for
    /// this same agent.
    #[serde(default = "default_fallback")]
    pub fallback: Fallback,
}

/// An alternate execution binding authorized for an agent whose primary binding
/// cannot run.
///
/// A fallback never names another agent. It therefore carries no profile,
/// purpose, or declared boundary, and substituting it cannot exchange the
/// reviewer contract for a second, possibly weaker, one.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Fallback {
    /// The literal "none": no substitution is authorized.
    Declined(String),
    Runtime(RuntimeBinding),
}

/// Where a role executes: host, model, and reasoning effort, and nothing that
/// could carry authority.
#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeBinding {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(rename = "reasoningEffort", default)]
    pub reasoning_effort: Option<String>,
}

impl Fallback {
    pub fn runtime(&self) -> Option<&RuntimeBinding> {
        match self {
            Self::Runtime(binding) => Some(binding),
            Self::Declined(_) => None,
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Declined(declined) => json!(declined),
            Self::Runtime(binding) => binding.value(),
        }
    }
}

impl RuntimeBinding {
    pub fn value(&self) -> Value {
        json!({
            "host": self.host,
            "model": self.model,
            "reasoningEffort": self.reasoning_effort,
        })
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            host: None,
            model: None,
            reasoning_effort: None,
            fallback: default_fallback(),
        }
    }
}

fn default_fallback() -> Fallback {
    Fallback::Declined("none".to_owned())
}

pub fn from_validated_agents(
    value: &Value,
) -> Result<BTreeMap<String, AgentConfig>, serde_json::Error> {
    serde_json::from_value(value.clone())
}

impl AgentConfig {
    pub fn native_name(&self, configured_name: &str) -> String {
        self.native_name
            .clone()
            .unwrap_or_else(|| configured_name.to_lowercase().replace('-', "_"))
    }

    pub fn runtime_value(&self) -> Value {
        json!({
            "host": self.runtime.host,
            "model": self.runtime.model,
            "reasoningEffort": self.runtime.reasoning_effort,
            "fallback": self.runtime.fallback.value(),
        })
    }

    pub fn boundary_value(&self) -> Value {
        json!({
            "observe": self.observe,
            "write": self.write,
            "commands": self.commands,
            "skills": self.skills,
            "memoryRead": self.memory_read,
            "memoryWrite": self.memory_write,
            "memoryReview": self.memory_review,
            "memoryPromote": self.memory_promote,
            "memoryReject": self.memory_reject,
            "memoryRevoke": self.memory_revoke,
            "memoryExpire": self.memory_expire,
            "memoryForget": self.memory_forget,
            "retention": self.retention,
            "crossContext": self.cross_context,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_name_prefers_explicit_value_and_derives_the_default() {
        let mut agent: AgentConfig = serde_json::from_value(json!({
            "profile":"profile.md","purpose":"test","retention":"task","crossContext":"none"
        }))
        .unwrap();
        assert_eq!(agent.native_name("Build-Worker"), "build_worker");
        agent.native_name = Some("exact_name".into());
        assert_eq!(agent.native_name("ignored"), "exact_name");
    }
}
