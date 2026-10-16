//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../agent_context_delivery.rs"]
mod agent_context_delivery;
#[path = "../agent_protocol.rs"]
mod agent_protocol;
#[path = "../child_capture.rs"]
mod child_capture;
#[path = "../controlled_effects.rs"]
mod controlled_effects;
#[cfg(feature = "repository-checks")]
#[path = "../fixture_host_isolation.rs"]
mod fixture_host_isolation;
#[path = "../harness_receipt.rs"]
mod harness_receipt;
#[path = "../harness_run.rs"]
mod harness_run;
#[path = "../host_bridge.rs"]
mod host_bridge;
#[path = "../host_workflow.rs"]
mod host_workflow;
#[path = "../managed_edits.rs"]
mod managed_edits;
#[path = "../native_continuity.rs"]
mod native_continuity;
#[path = "../native_session_export.rs"]
mod native_session_export;
#[path = "../start_context.rs"]
mod start_context;
#[path = "../state_permissions.rs"]
mod state_permissions;
