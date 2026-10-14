//! Direct-only session-goal closure stays separate from governed run evidence.
#![cfg(unix)]

#[path = "session_goal_direct/closure_events.rs"]
mod closure_events;
#[path = "session_goal_direct/direct_scope.rs"]
mod direct_scope;
#[path = "session_goal_direct/fixture.rs"]
mod fixture;
#[path = "session_goal_direct/native_context.rs"]
mod native_context;
mod support;
