//! Bounded, owner-checked recovery hints for native host session continuation.
//!
//! The hint is private navigation metadata. The native Goal journal remains
//! authoritative, and no saved route is permission to replay an operation.

mod current_work;
mod hint;
mod milestone;
mod presentation;
mod record;
mod references;
mod show;

pub(crate) use presentation::hook_context;
pub(crate) use record::{attach_graph_result, checkpoint, record};
pub(crate) use references::task_target;
pub(crate) use show::show;
