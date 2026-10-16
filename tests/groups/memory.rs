//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../contextual_memory.rs"]
mod contextual_memory;
#[path = "../lesson_input.rs"]
mod lesson_input;
#[path = "../lesson_revalidation.rs"]
mod lesson_revalidation;
#[path = "../memory_policy_correction.rs"]
mod memory_policy_correction;
#[path = "../memory_recall.rs"]
mod memory_recall;
#[path = "../project_memory_projection.rs"]
mod project_memory_projection;
#[path = "../task_local_memory.rs"]
mod task_local_memory;
