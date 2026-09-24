//! Read-only integrity status for artifacts named by a selected ledger event.

use super::artifact::{self, SubmissionEvidence};
use crate::config::Loaded;
use serde_json::{json, Value};

pub(super) fn inspect(loaded: &Loaded, event: &Value) -> Result<Value, String> {
    let mut evidence = Vec::new();
    if event["action"] == "submit" && event["artifact"].is_object() {
        let status = match artifact::verify_submission(loaded, event)? {
            SubmissionEvidence::Verified => "verified",
            SubmissionEvidence::Missing => "missing",
            SubmissionEvidence::Changed => "changed",
        };
        evidence.push(json!({"kind": "submission", "status": status}));
    }
    if event["action"] == "check" {
        for stream in ["stdout", "stderr"] {
            if let Some(reference) = event.get(stream) {
                let status = match artifact::read(loaded, reference, "check log") {
                    Ok(_) => "verified",
                    Err(error) if artifact::is_missing(&error) => "missing",
                    Err(error) => {
                        return Err(format!(
                            "selected event {stream} evidence is invalid: {error}"
                        ))
                    }
                };
                evidence.push(json!({"kind": stream, "status": status}));
            }
        }
    }
    Ok(Value::Array(evidence))
}
