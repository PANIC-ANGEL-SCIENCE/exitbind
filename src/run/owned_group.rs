//! The owned process group of a check observation that ended without a result.
//!
//! A timed-out observation whose capsule could not prove its own cleanup
//! records the group it launched. That group is re-read by its recorded
//! identity: while any of its processes still exists every forward Lead
//! decision stays closed, and once none does the observation's end is
//! established, so the existing environmental re-run route is the honest
//! one. A process list here is a bounded display fact, never signal authority.

use crate::host::codex_exec::{self, ProcessIdentity, ProcessLiveness};
use crate::run::state::check_observation::{OWNED_GROUP_FIELD, OWNED_START_FIELD};
use serde_json::{json, Value};
use std::fs::File;
use std::io::Read;

/// The environmental decision record's optional fact: the owned group was
/// observed ended when the decision was recorded. A later ledger read
/// validates this recorded fact and never reads live process state.
pub(crate) const ENDED_FIELD: &str = "ownedProcessState";
pub(crate) const ENDED: &str = "ended";

/// The most members a view names; the rest are summarized by the group state.
const MAX_NAMED: usize = 3;
/// One named command line is bounded so the action view stays navigable.
const MAX_COMMAND: usize = 80;
const MAX_ENTRIES: usize = 4096;

/// The recorded group as a process identity: the group leader's PID is the
/// group id, and the start marker recorded at failure time (when the capsule
/// could read it) distinguishes a reused group id from the owned group.
pub(crate) fn recorded(facts: &Value) -> Option<ProcessIdentity> {
    let group = facts[OWNED_GROUP_FIELD]
        .as_u64()
        .and_then(|group| u32::try_from(group).ok())
        .filter(|group| *group > 0 && *group <= i32::MAX as u32)?;
    Some(ProcessIdentity {
        pid: group,
        process_group: Some(group as i32),
        start_time_ticks: facts[OWNED_START_FIELD].as_u64().filter(|ticks| *ticks > 0),
    })
}

/// The recorded group's liveness read now, if the failure recorded one.
pub(crate) fn liveness(facts: &Value) -> Option<ProcessLiveness> {
    recorded(facts).map(|identity| liveness_of(&identity))
}

/// Group liveness that never reports a group owned by nobody. A live group id
/// whose leader carries a different start marker than the recorded one was
/// reused by an unrelated process, so it is unverified rather than owned:
/// never ended, never named as an owned member.
fn liveness_of(identity: &ProcessIdentity) -> ProcessLiveness {
    match codex_exec::group_liveness(identity) {
        ProcessLiveness::Alive if reused(identity) => ProcessLiveness::Uncertain,
        // A subreaper that has not drained its orphans keeps the group
        // identifier reachable through zombie members, and `kill(-group, 0)`
        // still succeeds for them. A group whose remaining members have all
        // exited has ended.
        ProcessLiveness::Alive
            if i32::try_from(identity.pid).is_ok_and(|group| live_member(group) == Some(false)) =>
        {
            ProcessLiveness::Ended
        }
        other => other,
    }
}

/// Whether the group still holds a member that is not exiting: `Some(false)`
/// when every `/proc` entry in the group is a zombie or already dead, `None`
/// when the scan cannot be trusted (an unreadable `/proc`, or more entries
/// than the bound), so an unproven end is never claimed.
fn live_member(group: i32) -> Option<bool> {
    if group <= 0 {
        return None;
    }
    let mut entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.by_ref().take(MAX_ENTRIES) {
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
        if group_of(&stat) != Some(group) {
            continue;
        }
        match state_of(&stat) {
            Some(state) if exiting(state) => {}
            // A member whose state cannot be read is not proven exited.
            _ => return Some(true),
        }
    }
    if entries.next().is_some() {
        // More entries than the scan bound: an unproven end is never claimed.
        return None;
    }
    Some(false)
}

fn reused(identity: &ProcessIdentity) -> bool {
    matches!(
        (identity.start_time_ticks, leader_start(identity.pid)),
        (Some(expected), Some(actual)) if expected != actual
    )
}

/// The group leader's start marker in the host's clock ticks, when readable.
#[cfg(target_os = "linux")]
fn leader_start(pid: u32) -> Option<u64> {
    let stat = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
    let text = std::str::from_utf8(&stat).ok()?;
    let (_, fields) = text.rsplit_once(") ")?;
    // starttime is field 22; after the command name the state is field 3.
    fields
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
        .filter(|ticks| *ticks > 0)
}

#[cfg(not(target_os = "linux"))]
fn leader_start(_: u32) -> Option<u64> {
    None
}

/// The timeout class whose execution end the failure facts could not
/// establish: the one class an ended owned group can close.
fn timed_out(observation: &Value, pending: &Value) -> bool {
    observation["state"] == "failed"
        && observation["facts"]["deadlineExceeded"] == true
        && observation["facts"]["termination"] == "unknown"
        && pending["kind"] == "check_observation_failed"
}

/// A timed-out observation that itself recorded no end, whose owned group is
/// now observed with no process left.
pub(crate) fn closed(observation: &Value, pending: &Value) -> bool {
    timed_out(observation, pending)
        && liveness(&observation["facts"]) == Some(ProcessLiveness::Ended)
}

/// The same closure read from the recorded decision, for ledger replay: the
/// event states that the group was observed ended when the decision was
/// admitted, so replay never consults the host.
pub(crate) fn closed_recorded(observation: &Value, pending: &Value, record: &Value) -> bool {
    timed_out(observation, pending) && record[ENDED_FIELD] == ENDED
}

/// The Lead-facing state of the recorded group and, while it is alive, at
/// most a few of its members. `unverified` means liveness could not be read or
/// the recorded identity no longer owns the group; it never means the group
/// ended.
pub(crate) fn view(facts: &Value) -> Option<Value> {
    let identity = recorded(facts)?;
    let (state, processes) = match liveness_of(&identity) {
        ProcessLiveness::Ended => (ENDED, Vec::new()),
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
        if state_of(&stat).is_some_and(exiting) {
            // A zombie or already-dead entry is not a live member to name.
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

/// The state field of `/proc/<pid>/stat`, read from the first field after the
/// parenthesized command name.
fn state_of(stat: &[u8]) -> Option<u8> {
    let text = std::str::from_utf8(stat).ok()?;
    let (_, fields) = text.rsplit_once(") ")?;
    fields.split_whitespace().next()?.bytes().next()
}

/// Whether a process state names one that has exited: `Z` waits to be reaped
/// and `X` is already gone.
fn exiting(state: u8) -> bool {
    matches!(state, b'Z' | b'X')
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
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};

    /// The test name libtest runs in the subreaper child.
    const ZOMBIE_GROUP_TEST: &str = "run::owned_group::tests::a_zombie_only_group_reads_ended";

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
        let mut marked = recorded_facts(4321);
        marked[OWNED_START_FIELD] = json!(77);
        assert_eq!(recorded(&marked).unwrap().start_time_ticks, Some(77));
        marked[OWNED_START_FIELD] = json!(0);
        assert_eq!(recorded(&marked).unwrap().start_time_ticks, None);
    }

    #[test]
    fn a_reused_group_id_is_never_named_as_owned() {
        // A live group this test owns: its leader start marker is readable.
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 5"])
            .process_group(0)
            .spawn()
            .unwrap();
        let group = u64::from(child.id());
        let actual = leader_start(child.id()).expect("the live leader has a start marker");
        let mut facts = recorded_facts(group);
        facts[OWNED_START_FIELD] = json!(actual);
        assert_eq!(liveness(&facts), Some(ProcessLiveness::Alive));
        let owned = view(&facts).unwrap();
        assert_eq!(owned["ownedProcessState"], "alive");
        assert!(!owned["ownedProcesses"].as_array().unwrap().is_empty());
        // The same group id carrying another process's start marker was
        // reused: it is unverified and no unrelated process is named.
        facts[OWNED_START_FIELD] = json!(actual + 1);
        assert_eq!(liveness(&facts), Some(ProcessLiveness::Uncertain));
        let reused_view = view(&facts).unwrap();
        assert_eq!(reused_view["ownedProcessState"], "unverified");
        assert!(reused_view["ownedProcesses"].as_array().unwrap().is_empty());
        let _ = child.kill();
        let _ = child.wait();
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
        // The recorded verdict is the only one replay reads: it holds even
        // while the same recorded group id belongs to a live process.
        let own = u64::try_from(unsafe { libc::getpgrp() }).unwrap();
        assert!(!closed(&observation(own), &pending));
        assert!(closed_recorded(
            &observation(own),
            &pending,
            &json!({ENDED_FIELD: ENDED})
        ));
        assert!(!closed_recorded(&observation(own), &pending, &json!({})));
        assert!(!closed_recorded(
            &observation(2),
            &pending,
            &json!({ENDED_FIELD: "alive"})
        ));
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

    /// The observation capsule is a child subreaper that drains its orphans
    /// only after its leader ends, so a killed group can keep its identifier
    /// through zombie members that `kill(-group, 0)` still reaches. Such a
    /// group has ended and names no live member.
    #[test]
    fn a_zombie_only_group_reads_ended() {
        if isolated(ZOMBIE_GROUP_TEST) {
            return;
        }
        // A short-lived leader in its own group starts a sleeper and exits.
        // This subreaper adopts the orphan and never reaps it.
        let mut leader = std::process::Command::new("sh")
            .args(["-c", "sleep 60 & echo $!; exit 0"])
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let group = u64::from(leader.id());
        let mut line = String::new();
        BufReader::new(leader.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let sleeper = line.trim().parse::<i32>().unwrap();
        leader.wait().unwrap();
        assert_eq!(unsafe { libc::kill(sleeper, libc::SIGKILL) }, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let stat = std::fs::read(format!("/proc/{sleeper}/stat")).unwrap_or_default();
            if state_of(&stat) == Some(b'Z') {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the orphan never became a zombie: {}",
                String::from_utf8_lossy(&stat)
            );
            std::thread::sleep(Duration::from_millis(5));
        }

        let facts = recorded_facts(group);
        let identity = recorded(&facts).unwrap();
        assert_eq!(
            codex_exec::group_liveness(&identity),
            ProcessLiveness::Alive,
            "a zombie member keeps the group id reachable"
        );
        assert_eq!(liveness(&facts), Some(ProcessLiveness::Ended));
        let pending = json!({"kind": "check_observation_failed"});
        assert!(closed(&observation(group), &pending));
        let ended = view(&facts).unwrap();
        assert_eq!(ended["ownedProcessState"], ENDED);
        assert_eq!(ended["ownedProcesses"], json!([]));

        // Reap the adopted zombie so no fixture entry outlives this test.
        let _ = unsafe { libc::waitpid(sleeper, std::ptr::null_mut(), 0) };
    }

    /// Run the named test in a child process that is a child subreaper, as the
    /// observation capsule is. The child runs the body; this process only
    /// relays its result.
    fn isolated(test: &str) -> bool {
        const FIXTURE: &str = "EXITBIND_OWNED_GROUP_SUBREAPER_FIXTURE";
        if std::env::var(FIXTURE).as_deref() == Ok(test) {
            assert_eq!(
                unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
                0
            );
            return false;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(FIXTURE, test)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("1 passed"), "{stdout}\n{stderr}");
        true
    }
}
