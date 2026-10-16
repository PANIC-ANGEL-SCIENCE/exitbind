//! Bounded, context-only Work membership on the existing canonical Goal.

use serde_json::Value;

pub(crate) const PROTOCOL: u64 = 1;
const MAX_ASSOCIATIONS: usize = 128;
const COMPANION_KIND: &str = "native_session_goal_context";
const VERIFIED_CONTEXT_FIELD: &str = "__verifiedContextOnlyWork";

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    goal_id: String,
    revision: u64,
    event_sha256: String,
    session_key: String,
    host: &'static str,
    owner_project_id: Option<String>,
    owner_identity_sha256: String,
}

#[derive(Clone, Debug)]
pub(crate) enum WorkContext {
    NotRequested,
    Local,
    Owner(Value),
    Session(Value),
    Unavailable,
    Invalid,
}

/// The owner journal root is returned only when the association was verified
/// against the current native session and its fixed owner record.
pub(crate) struct WorkContextResolution {
    pub(crate) context: WorkContext,
    pub(crate) owner_root: Option<std::path::PathBuf>,
}

mod admission;
mod companion;
mod context;
mod record;

#[cfg(test)]
#[path = "association/tests.rs"]
mod tests;

pub(crate) use admission::{admit, link_session, prepare};
pub(super) use companion::validate_companion;
pub(crate) use companion::{
    read_private_companion, write_private_companion, PrivateCompanionWrite,
};
pub(crate) use context::{context_for_work, context_for_work_with_root};
pub(super) use record::{
    entries, for_work, preserve_same_goal, public_record, valid_sha, valid_work, validate_record,
};
