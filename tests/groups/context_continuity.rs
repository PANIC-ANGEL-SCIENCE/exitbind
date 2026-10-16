//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../context_v022.rs"]
mod context_v022;
#[path = "../cross_host_continuity.rs"]
mod cross_host_continuity;
#[path = "../current_work_focus.rs"]
mod current_work_focus;
#[path = "../governor_recovery.rs"]
mod governor_recovery;
