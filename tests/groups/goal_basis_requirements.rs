//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../basis_lifecycle.rs"]
mod basis_lifecycle;
#[path = "../goal_requirements.rs"]
mod goal_requirements;
#[path = "../session_goal_usage.rs"]
mod session_goal_usage;
