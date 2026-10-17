//! Project optional protocol markers from the canonical start into reduced state.

use serde_json::Value;

const START_PROTOCOL_FIELDS: &[&str] = &[
    "recoveryProtocol",
    "checkObservationProtocol",
    "verificationPreparationProtocol",
    "verificationCheckerSources",
    "projectContinuation",
];

pub(super) fn copy_fields(start: &Value, state: &mut Value) {
    for field in START_PROTOCOL_FIELDS {
        if let Some(value) = start.get(*field) {
            state[*field] = value.clone();
        }
    }
}
