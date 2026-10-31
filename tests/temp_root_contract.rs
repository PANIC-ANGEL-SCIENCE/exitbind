//! Test fixture ownership and cleanup are independent of repository file size.

#[path = "inventory/rust_source.rs"]
mod rust_source;
#[path = "support/temp_root.rs"]
mod temp_root;
#[path = "temp_root_contract/temp_root_guard.rs"]
mod temp_root_guard;
#[path = "temp_root_contract/temp_roots.rs"]
mod temp_roots;
