//! One canonical projection of a recorded preservation policy for role packets.

use serde_json::{json, Value};

pub(crate) fn from_policy(preservation: &Value, non_goals: Option<&Value>) -> Value {
    json!({
        "route": "FORMAL",
        "quality": "FULL",
        "resolvedBy": "accepted_preservation_requirements",
        "enforcement": "recorded_not_enforced",
        "version": preservation["version"],
        "requirements": preservation["requirements"],
        "nonGoals": non_goals.cloned().unwrap_or_else(|| json!([])),
        "source": "canonical_state",
    })
}
