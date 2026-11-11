//! Bounded, non-authenticating context for interpreting a local observer PID.
//!
//! Hashes avoid storing raw host identifiers. They are a comparison aid, not
//! proof of machine identity; an unavailable context must never be treated as
//! the legacy absence of a context field.

use serde_json::{json, Value};

#[cfg(target_os = "linux")]
fn bounded_file(path: &str) -> Option<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(257)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() > 256 {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(target_os = "linux")]
fn context_inputs_valid(host: &str, boot: &str, namespace: &str, proc_pid: u32, pid: u32) -> bool {
    let uuid = |text: &str| {
        text.len() == 36
            && text.bytes().enumerate().all(|(i, c)| {
                if [8, 13, 18, 23].contains(&i) {
                    c == b'-'
                } else {
                    c.is_ascii_hexdigit()
                }
            })
            && text.bytes().any(|c| c.is_ascii_hexdigit() && c != b'0')
    };
    host.len() == 32
        && host.bytes().all(|c| c.is_ascii_hexdigit())
        && host.bytes().any(|c| c != b'0')
        && uuid(boot)
        && namespace
            .strip_prefix("pid:[")
            .and_then(|s| s.strip_suffix(']'))
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n > 0)
        && proc_pid > 0
        && proc_pid == pid
}

#[cfg(target_os = "linux")]
pub(crate) fn current() -> Value {
    let parts = (|| {
        let host = bounded_file("/etc/machine-id")?;
        let boot = bounded_file("/proc/sys/kernel/random/boot_id")?;
        let namespace = std::fs::read_link("/proc/self/ns/pid").ok()?;
        let namespace = namespace.to_str()?;
        let proc_self = std::fs::read_link("/proc/self").ok()?;
        let proc_pid = proc_self.to_str()?.parse().ok()?;
        if namespace.len() > 256
            || !context_inputs_valid(&host, &boot, namespace, proc_pid, std::process::id())
        {
            return None;
        }
        let digest = |kind: &str, value: &str| {
            crate::evidence::hash::value(
                &json!({"domain": "observer_context_v1", "kind": kind, "value": value}),
            )
        };
        Some(json!({"version": 1, "state": "available",
            "hostSha256": digest("host", &host), "bootSha256": digest("boot", &boot),
            "pidNamespaceSha256": digest("pid_namespace", namespace)}))
    })();
    parts.unwrap_or_else(|| json!({"version": 1, "state": "unavailable"}))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn current() -> Value {
    json!({"version": 1, "state": "unavailable"})
}

pub(crate) fn valid(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if value["version"] != 1 {
        return false;
    }
    match value["state"].as_str() {
        Some("unavailable") => object.len() == 2,
        Some("available") => {
            object.len() == 5
                && ["hostSha256", "bootSha256", "pidNamespaceSha256"]
                    .iter()
                    .all(|key| {
                        value[*key].as_str().is_some_and(|s| {
                            s.len() == 64
                                && s.bytes()
                                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        })
                    })
        }
        _ => false,
    }
}

pub(crate) fn matching(recorded: &Value, current: &Value) -> bool {
    valid(recorded) && valid(current) && recorded["state"] == "available" && recorded == current
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn automatic_observer_end_requires_matching_available_context() {
        let context = current();
        assert_eq!(context["state"], "available");
        let id = crate::host::codex_exec::observer_identity();
        let ticks = id.start_time_ticks.expect("Linux process start marker");
        let mut observation =
            json!({"observer":{"pid":id.pid,"startTimeTicks":ticks,"context":context}});
        assert!(!crate::run::check_observation::observer_ended(&observation));
        // A different start marker represents PID reuse. This is ended only
        // in the same host, boot and namespace, never across a mismatch.
        observation["observer"]["startTimeTicks"] = json!(ticks + 1);
        assert!(crate::run::check_observation::observer_ended(&observation));
        for field in ["hostSha256", "bootSha256", "pidNamespaceSha256"] {
            let mut mismatch = observation.clone();
            mismatch["observer"]["context"][field] = json!("a".repeat(64));
            assert!(
                !crate::run::check_observation::observer_ended(&mismatch),
                "{field}"
            );
        }
        observation["observer"]["context"] = json!({"version":1,"state":"unavailable"});
        assert!(!crate::run::check_observation::observer_ended(&observation));
        observation["observer"]["startTimeTicks"] = Value::Null;
        assert!(!crate::run::check_observation::observer_ended(&observation));
    }

    #[test]
    fn malformed_context_and_foreign_procfs_are_unavailable() {
        let host = "0123456789abcdef0123456789abcdef";
        let boot = "01234567-89ab-cdef-0123-456789abcdef";
        assert!(context_inputs_valid(host, boot, "pid:[42]", 7, 7));
        assert!(!context_inputs_valid(host, boot, "pid:[42]", 8, 7));
        assert!(!context_inputs_valid(
            "uninitialized",
            boot,
            "pid:[42]",
            7,
            7
        ));
        assert!(!context_inputs_valid(
            &"0".repeat(32),
            boot,
            "pid:[42]",
            7,
            7
        ));
        assert!(!context_inputs_valid(host, "unknown", "pid:[42]", 7, 7));
        assert!(!context_inputs_valid(host, boot, "pid:[0]", 7, 7));
    }
}
