//! Bounded, owner-checked recovery hints for native host session continuation.
//!
//! The hint is private navigation metadata. The native Goal journal remains
//! authoritative, and no saved route is permission to replay an operation.

mod checkout;
mod current_work;
mod derived_text;
mod hint;
mod milestone;
mod owner_config;
mod pause;
mod presentation;
mod record;
mod references;
mod show;
mod spend;

pub(crate) use owner_config::{goal_owner, is_owner, GoalOwner};
pub(crate) use pause::{
    decision_line, for_view as pause_routes, record as record_pause, SAVED_DECISIONS_MAX_BYTES,
};
pub(crate) use presentation::hook_context;
pub(crate) use record::{attach_graph_result, checkpoint, record};
pub(crate) use references::task_target;
pub(crate) use show::show;
pub(crate) use spend::with_saved_decisions;
