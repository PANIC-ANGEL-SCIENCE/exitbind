//! Bounded, owner-checked recovery hints for native host session continuation.
//!
//! The hint is private navigation metadata. The native Goal journal remains
//! authoritative, and no saved route is permission to replay an operation.

mod hint;
mod presentation;
mod record;
mod references;

pub(crate) use presentation::{hook_context, show};
pub(crate) use record::{attach_graph_result, checkpoint, record};
pub(crate) use references::task_target;
