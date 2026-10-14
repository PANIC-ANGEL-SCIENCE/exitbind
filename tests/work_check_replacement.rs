//! An explicit Lead decision can preserve a failed Work while binding a new
//! frozen checker to a claimed successor.
#![cfg(unix)]
include!("work_check_replacement/context.rs");

#[path = "work_check_replacement/refusal_and_native_lineage.rs"]
mod refusal_and_native_lineage;
#[path = "work_check_replacement/replacement_lifecycle.rs"]
mod replacement_lifecycle;
#[path = "work_check_replacement/source_and_binding_validation.rs"]
mod source_and_binding_validation;
