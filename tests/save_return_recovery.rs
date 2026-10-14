//! Focused R18 checks for same-Work native return recovery.
#![cfg(unix)]
include!("save_return_recovery/context.rs");

#[path = "save_return_recovery/journal_replay.rs"]
mod journal_replay;
#[path = "save_return_recovery/native_return_inspection.rs"]
mod native_return_inspection;
#[path = "save_return_recovery/worker_resume.rs"]
mod worker_resume;
