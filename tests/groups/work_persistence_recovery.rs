//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../save_return_contract.rs"]
mod save_return_contract;
#[path = "../save_return_preservation.rs"]
mod save_return_preservation;
#[path = "../save_return_recovery.rs"]
mod save_return_recovery;
#[path = "../work_action_refresh.rs"]
mod work_action_refresh;
#[path = "../work_check_replacement.rs"]
mod work_check_replacement;
#[path = "../work_closeout.rs"]
mod work_closeout;
#[path = "../work_goal_absence.rs"]
mod work_goal_absence;
