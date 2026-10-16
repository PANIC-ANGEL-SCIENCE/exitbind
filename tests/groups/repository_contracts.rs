//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[cfg(feature = "repository-checks")]
#[path = "../architecture_adoption.rs"]
mod architecture_adoption;
#[cfg(feature = "repository-checks")]
#[path = "../architecture_contract.rs"]
mod architecture_contract;
#[cfg(feature = "repository-checks")]
#[path = "../artifact_bytes.rs"]
mod artifact_bytes;
#[cfg(feature = "repository-checks")]
#[path = "../build_identity.rs"]
mod build_identity;
#[cfg(feature = "repository-checks")]
#[path = "../ci_policy.rs"]
mod ci_policy;
#[cfg(feature = "repository-checks")]
#[path = "../ci_test_execution.rs"]
mod ci_test_execution;
#[cfg(feature = "repository-checks")]
#[path = "../compatibility_matrix.rs"]
mod compatibility_matrix;
#[cfg(feature = "repository-checks")]
#[path = "../exit_transition_conformance.rs"]
mod exit_transition_conformance;
#[cfg(feature = "repository-checks")]
#[path = "../public_positioning.rs"]
mod public_positioning;
#[cfg(feature = "repository-checks")]
#[path = "../r13_check.rs"]
mod r13_check;
#[cfg(feature = "repository-checks")]
#[path = "../rust_contract.rs"]
mod rust_contract;
#[cfg(feature = "repository-checks")]
#[path = "../rust_parity.rs"]
mod rust_parity;
