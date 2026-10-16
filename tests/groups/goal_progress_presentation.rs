//! Grouped integration target. Source files remain at their original paths.

// Original nested test contexts retain module-local support state; this grouped target intentionally loads the same support modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../presentation.rs"]
mod presentation;
#[path = "../subject_progress.rs"]
mod subject_progress;
