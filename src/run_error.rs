use serde_json::{json, Value};

/// Stable, goal-free diagnostics for a run-start configuration mismatch.
///
/// This type is deliberately small: the ledger protocol owns validation and
/// the CLI decides whether to render the machine representation or the human
/// explanation.
#[derive(Debug, Clone)]
pub struct DriftError {
    pub kind: &'static str,
    pub expected: String,
    pub current: String,
    pub agent: Option<String>,
}

impl DriftError {
    pub fn config(expected: String, current: String) -> Self {
        Self {
            kind: "config",
            expected,
            current,
            agent: None,
        }
    }

    pub fn profile(agent: String, expected: String, current: String) -> Self {
        Self {
            kind: "profile",
            expected,
            current,
            agent: Some(agent),
        }
    }

    pub fn memory(agent: String, expected: String, current: String) -> Self {
        Self {
            kind: "memory",
            expected,
            current,
            agent: Some(agent),
        }
    }

    pub fn boundary(expected: String, current: String) -> Self {
        Self {
            kind: "boundary",
            expected,
            current,
            agent: None,
        }
    }

    pub fn harness_receipt(expected: String, current: String) -> Self {
        Self {
            kind: "harness_receipt",
            expected,
            current,
            agent: None,
        }
    }

    pub fn machine(&self) -> Value {
        if self.kind == "config" {
            json!({
                "error": "configuration drift detected after run start",
                "classification": "config_drift",
                "expectedConfigSha256": self.expected,
                "currentConfigSha256": self.current
            })
        } else if self.kind == "profile" {
            json!({
                "error": "profile drift detected after run start",
                "classification": "profile_drift",
                "agent": self.agent,
                "expectedProfileSha256": self.expected,
                "currentProfileSha256": self.current
            })
        } else if self.kind == "memory" {
            json!({
                "error": "memory drift detected after run start",
                "classification": "memory_drift",
                "agent": self.agent,
                "expectedMemorySetSha256": self.expected,
                "currentMemorySetSha256": self.current
            })
        } else if self.kind == "harness_receipt" {
            json!({
                "error": "harness receipt drift detected after run start",
                "classification": "harness_receipt_drift",
                "expectedHarnessReceiptSha256": self.expected,
                "currentHarnessReceiptSha256": self.current
            })
        } else {
            json!({
                "error": "run boundary manifest drift detected after run start",
                "classification": "boundary_drift",
                "expectedBoundarySha256": self.expected,
                "currentBoundarySha256": self.current
            })
        }
    }
}

impl std::fmt::Display for DriftError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.kind == "boundary" {
            write!(
                formatter,
                "run boundary manifest drift detected after run start"
            )
        } else if self.kind == "harness_receipt" {
            write!(formatter, "harness receipt drift detected after run start")
        } else if self.kind == "memory" {
            write!(
                formatter,
                "memory drift detected after run start for {}",
                self.agent.as_deref().unwrap_or("unknown agent")
            )
        } else if let Some(agent) = &self.agent {
            write!(formatter, "profile drift detected: {agent}")
        } else {
            write!(
                formatter,
                "configuration drift detected after run start: expected config hash {}; current config hash {}",
                self.expected, self.current
            )
        }
    }
}

pub(crate) fn machine_drift(error: DriftError) -> String {
    let machine = serde_json::to_string(&error.machine())
        .unwrap_or_else(|_| "{\"error\":\"drift diagnostic serialization failed\"}".to_owned());
    format!("SOULMATE_DRIFT:{}", machine)
}
