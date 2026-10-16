//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../continuation_conditions.rs"]
mod continuation_conditions;
#[path = "../current_details.rs"]
mod current_details;
#[path = "../held_delivery.rs"]
mod held_delivery;
#[path = "../lead_disposition.rs"]
mod lead_disposition;
#[path = "../pre_return.rs"]
mod pre_return;
#[path = "../pre_return_native.rs"]
mod pre_return_native;
#[path = "../receiving_entry.rs"]
mod receiving_entry;
#[path = "../replan_input.rs"]
mod replan_input;
#[path = "../resume_scope.rs"]
mod resume_scope;
