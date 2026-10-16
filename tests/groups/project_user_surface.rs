//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../activation_classify.rs"]
mod activation_classify;
#[path = "../agent_first_control.rs"]
mod agent_first_control;
#[path = "../checked_work_ux.rs"]
mod checked_work_ux;
#[path = "../external_selection.rs"]
mod external_selection;
#[path = "../fresh_user_surface.rs"]
mod fresh_user_surface;
#[path = "../host_help.rs"]
mod host_help;
#[path = "../init_without_skills.rs"]
mod init_without_skills;
#[path = "../project_capabilities.rs"]
mod project_capabilities;
#[path = "../project_context.rs"]
mod project_context;
#[path = "../project_layout.rs"]
mod project_layout;
#[path = "../setup.rs"]
mod setup;
#[path = "../setup_pre_return.rs"]
mod setup_pre_return;
#[path = "../singleton_options.rs"]
mod singleton_options;
#[path = "../specialist_routing.rs"]
mod specialist_routing;
#[path = "../usage_help.rs"]
mod usage_help;
