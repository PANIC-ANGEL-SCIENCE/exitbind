//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../check_observation_recovery.rs"]
mod check_observation_recovery;
#[path = "../check_retry.rs"]
mod check_retry;
#[path = "../event_detail_integrity.rs"]
mod event_detail_integrity;
#[path = "../observe_check.rs"]
mod observe_check;
#[path = "../preservation_assignment_stages.rs"]
mod preservation_assignment_stages;
#[path = "../preservation_independence.rs"]
mod preservation_independence;
#[path = "../recovery_config.rs"]
mod recovery_config;
#[cfg(feature = "repository-checks")]
#[path = "../recurrence_repairs.rs"]
mod recurrence_repairs;
#[path = "../repeated_repair.rs"]
mod repeated_repair;
#[path = "../review_currentness.rs"]
mod review_currentness;
#[path = "../review_policy.rs"]
mod review_policy;
#[path = "../reviewer_evidence_handoff.rs"]
mod reviewer_evidence_handoff;
#[cfg(feature = "repository-checks")]
#[path = "../verification_context.rs"]
mod verification_context;
