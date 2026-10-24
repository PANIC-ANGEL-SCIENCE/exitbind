//! The owned process group of a check observation that ended without a result.
//!
//! A timed-out observation whose capsule could not prove its own cleanup
//! records the group it launched. That group is re-read by its recorded
//! identity: while any of its processes still exists every forward Lead
//! decision stays closed, and once none does the observation's end is
//! established, so the existing environmental re-run route is the honest
//! one. A process list here is a bounded display fact, never signal authority.

use crate::host::codex_exec::{self, ProcessIdentity, ProcessLiveness};
use crate::run::state::check_observation::OWNED_GROUP_FIELD;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Read;

/// The most members a view names; the rest are summarized by the group state.
const MAX_NAMED: usize = 3;
/// One named command line is bounded so the action view stays navigable.
const MAX_COMMAND: usize = 80;
const MAX_ENTRIES: usize = 4096;

/// The recorded group as a process identity: the group leader's PID is the
/// group id, and no start marker exists for a group.
pub(crate) fn recorded(facts: &Value) -> Option<ProcessIdentity> {
    let group = facts[OWNED_GROUP_FIELD]
        .as_u64()
        .and_then(|group| u32::try_from(group).ok())
        .filter(|group| *group > 0 && *group <= i32::MAX as u32)?;
    Some(ProcessIdentity {
        pid: group,
        process_group: Some(group as i32),
        start_time_ticks: None,
    })
}

/// The recorded group's liveness read now, if the failure recorded one.
pub(crate) fn liveness(facts: &Value) -> Option<ProcessLiveness> {
    recorded(facts).map(|identity| codex_exec::group_liveness(&identity))
}

/// A timed-out observation that itself recorded no end, whose owned group is
/// now observed with no process left. The timeout class is the one whose
/// execution end the failure facts could not establish.
pub(crate) fn closed(observation: &Value, pending: &Value) -> bool {
    observation["state"] == "failed"
        && observation["facts"]["deadlineExceeded"] == true
        && observation["facts"]["termination"] == "unknown"
        && pending["kind"] == "check_observation_failed"
        && liveness(&observation["facts"]) == Some(ProcessLiveness::Ended)
}

/// The Lead-facing state of the recorded group and, while it is alive, at
/// most a few of its members. `unverified` means liveness could not be read;
/// it never means the group ended.
pub(crate) fn view(facts: &Value) -> Option<Value> {
    let identity = recorded(facts)?;
    let (state, processes) = match codex_exec::group_liveness(&identity) {
        ProcessLiveness::Ended => ("ended", Vec::new()),
        ProcessLiveness::Alive => ("alive", members(identity.pid as i32)),
        ProcessLiveness::Uncertain => ("unverified", Vec::new()),
    };
    Some(json!({
        "ownedProcessGroup": identity.pid,
        "ownedProcessState": state,
        "ownedProcesses": processes,
    }))
}

/// A bounded scan of `/proc` for members of `group`. A vanished entry is
/// skipped; the scan never claims to be exhaustive.
fn members(group: i32) -> Vec<Value> {
    let mut named = Vec::new();
    if group <= 0 {
        return named;
    }
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return named;
    };
    for entry in entries.take(MAX_ENTRIES) {
        if named.len() >= MAX_NAMED {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        let Ok(stat) = std::fs::read(format!("/proc/{pid}/stat")) else {
            continue;
        };
        if !group_of(&stat).is_some_and(|candidate| candidate == group) {
            continue;
        }
        named.push(json!({"pid": pid, "command": command(pid)}));
    }
    named
}

/// The process group field of `/proc/<pid>/stat`, read after the parenthesized
/// command name so a name containing spaces or parentheses cannot shift it.
fn group_of(stat: &[u8]) -> Option<i32> {
    let text = std::str::from_utf8(stat).ok()?;
    let (_, fields) = text.rsplit_once(") ")?;
    fields.split_whitespace().nth(2)?.parse().ok()
}

/// A short, control-free command line for one process; its kernel command name
/// stands in when the command line is empty.
fn command(pid: u32) -> String {
    let mut bytes = Vec::new();
    let read = File::open(format!("/proc/{pid}/cmdline"))
        .and_then(|file| file.take(1024).read_to_end(&mut bytes));
    if read.is_err() || bytes.is_empty() {
        bytes.clear();
        let _ = File::open(format!("/proc/{pid}/comm"))
            .and_then(|file| file.take(1024).read_to_end(&mut bytes));
    }
    let text = String::from_utf8_lossy(&bytes);
    let joined = text
        .split(['\0', '\n'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let bounded: String = joined
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_COMMAND)
        .collect();
    if bounded.trim().is_empty() {
        "unnamed process".to_owned()
    } else {
        bounded
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    fn recorded_facts(group: u64) -> Value {
        let mut facts = json!({});
        facts[OWNED_GROUP_FIELD] = json!(group);
        facts
    }

    fn observation(group: u64) -> Value {
        let mut facts = json!({"deadlineExceeded": true, "termination": "unknown"});
        facts[OWNED_GROUP_FIELD] = json!(group);
        json!({"state": "failed", "facts": facts})
    }

    #[test]
    fn only_a_positive_recorded_group_is_an_identity() {
        assert!(recorded(&json!({})).is_none());
        assert!(recorded(&recorded_facts(0)).is_none());
        assert!(recorded(&recorded_facts(i32::MAX as u64 + 1)).is_none());
        let mut negative = json!({});
        negative[OWNED_GROUP_FIELD] = json!(-4);
        assert!(recorded(&negative).is_none());
        let identity = recorded(&recorded_facts(4321)).unwrap();
        assert_eq!(identity.pid, 4321);
        assert_eq!(identity.process_group, Some(4321));
        assert_eq!(identity.start_time_ticks, None);
    }

    #[test]
    fn a_recorded_group_that_still_exists_is_never_closed() {
        // The test's own process group exists while this test runs.
        let own = u64::try_from(unsafe { libc::getpgrp() }).unwrap();
        let facts = recorded_facts(own);
        let pending = json!({"kind": "check_observation_failed"});
        assert_eq!(liveness(&facts), Some(ProcessLiveness::Alive));
        assert!(!closed(&observation(own), &pending));
        let view = view(&facts).unwrap();
        assert_eq!(view["ownedProcessState"], "alive");
        assert_eq!(view["ownedProcessGroup"], own);
        assert!(!view["ownedProcesses"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_recorded_group_field_of_a_stat_line_is_read_after_the_command_name() {
        assert_eq!(
            group_of(b"17 (a ) nested (name)) S 23 4321 0 0").unwrap(),
            4321
        );
        assert!(group_of(b"17 missing comm").is_none());
    }

    #[test]
    fn only_the_timeout_class_with_an_ended_group_closes() {
        let pending = json!({"kind": "check_observation_failed"});
        let mut not_timed_out = observation(2);
        not_timed_out["facts"]["deadlineExceeded"] = json!(false);
        assert!(!closed(&not_timed_out, &pending));
        let mut already_ended = observation(2);
        already_ended["facts"]["termination"] = json!("ended");
        assert!(!closed(&already_ended, &pending));
        let mut other = observation(2);
        other["state"] = json!("running");
        assert!(!closed(&other, &pending));
        assert!(!closed(&observation(2), &json!({"kind": "review_rework"})));
        // A group that has already ended, read from a child this test owned.
        let mut child = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .process_group(0)
            .spawn()
            .unwrap();
        let gone = u64::from(child.id());
        child.wait().unwrap();
        if liveness(&recorded_facts(gone)) == Some(ProcessLiveness::Ended) {
            assert!(closed(&observation(gone), &pending));
            assert_eq!(
                view(&recorded_facts(gone)).unwrap()["ownedProcessState"],
                "ended"
            );
        }
    }
}
