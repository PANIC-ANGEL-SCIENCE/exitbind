//! The conversational surface: what Exitbind says, when it says it, and when it
//! stays quiet. Every line here is derived from canonical run state.
#![cfg(unix)]
include!("presentation/context.rs");

#[path = "presentation/interactive_closure.rs"]
mod interactive_closure;
#[path = "presentation/session_goal_closure.rs"]
mod session_goal_closure;
#[path = "presentation/status_reporting.rs"]
mod status_reporting;
#[path = "presentation/terminal_decisions.rs"]
mod terminal_decisions;
