//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../behavioral_evaluation.rs"]
mod behavioral_evaluation;
#[path = "../evidence_expansion_v022.rs"]
mod evidence_expansion_v022;
#[path = "../value_benchmark.rs"]
mod value_benchmark;
#[path = "../value_proof.rs"]
mod value_proof;
#[path = "../value_receipt.rs"]
mod value_receipt;
#[path = "../value_reconstruction.rs"]
mod value_reconstruction;
#[path = "../value_replay.rs"]
mod value_replay;
