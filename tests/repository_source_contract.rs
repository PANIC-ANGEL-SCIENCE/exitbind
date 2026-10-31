//! Repository-wide source contracts kept outside the Goal behavior target.
#![cfg(unix)]

use std::{fs, path::Path};

#[path = "repository_source_contract/action_view.rs"]
mod action_view;
#[path = "repository_source_contract/attested_check_consumers.rs"]
mod attested_check_consumers;
#[path = "repository_source_contract/changed_after_binding_inventory.rs"]
mod changed_after_binding_inventory;
#[path = "repository_source_contract/child_block_refusals.rs"]
mod child_block_refusals;
#[path = "inventory/cli_tables.rs"]
mod cli_tables;
#[path = "repository_source_contract/config_writer_keep.rs"]
mod config_writer_keep;
#[path = "repository_source_contract/emitted_routes.rs"]
mod emitted_routes;
#[path = "repository_source_contract/follow_up_routes.rs"]
mod follow_up_routes;
/// This target uses action names; the Goal runtime target uses the full API.
#[allow(dead_code)]
#[path = "../src/cli/goal_action.rs"]
mod goal_action;
#[path = "repository_source_contract/journey_config_change.rs"]
mod journey_config_change;
#[path = "repository_source_contract/lead_reason.rs"]
mod lead_reason;
#[path = "goal_requirements/lead_reason_forms.rs"]
mod lead_reason_forms;
#[path = "repository_source_contract/root_inputs.rs"]
mod root_inputs;
#[path = "repository_source_contract/route_values.rs"]
mod route_values;
#[path = "inventory/rust_source.rs"]
mod rust_source;
#[path = "repository_source_contract/session_adoption.rs"]
mod session_adoption;
#[path = "repository_source_contract/session_record_lock.rs"]
mod session_record_lock;
#[path = "repository_source_contract/source_scanner.rs"]
mod source_scanner;
#[path = "support/mod.rs"]
mod support;
#[path = "repository_source_contract/typed_routes.rs"]
mod typed_routes;
#[path = "repository_source_contract/view_refusals.rs"]
mod view_refusals;
