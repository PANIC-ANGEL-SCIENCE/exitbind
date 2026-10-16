//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../action_forms.rs"]
mod action_forms;
#[path = "../compact_preservation.rs"]
mod compact_preservation;
#[path = "../compact_work.rs"]
mod compact_work;
#[path = "../early_evidence.rs"]
mod early_evidence;
#[path = "../file_effects.rs"]
mod file_effects;
#[path = "../mutation_result.rs"]
mod mutation_result;
#[path = "../run_boundary.rs"]
mod run_boundary;
