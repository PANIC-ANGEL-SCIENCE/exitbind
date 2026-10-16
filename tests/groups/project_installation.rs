//! Grouped integration target. Source files remain at their original paths.

// Original test sources keep module-local support state; this grouped target intentionally loads those modules repeatedly.
#![allow(clippy::duplicate_mod)]

#[path = "../installed_consumer.rs"]
mod installed_consumer;
#[path = "../installer_platform.rs"]
mod installer_platform;
#[path = "../layout_migration.rs"]
mod layout_migration;
#[path = "../legacy_installer_bridge.rs"]
mod legacy_installer_bridge;
#[path = "../path_compatibility.rs"]
mod path_compatibility;
#[path = "../portability.rs"]
mod portability;
#[path = "../provider_fallback.rs"]
mod provider_fallback;
#[path = "../skill_diagnostics.rs"]
mod skill_diagnostics;
#[path = "../skill_refresh.rs"]
mod skill_refresh;
#[path = "../update.rs"]
mod update;
