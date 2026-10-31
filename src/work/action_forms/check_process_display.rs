//! Read-only presentation of owned check-process state.

use crate::config::Loaded;
use serde_json::{json, Value};

/// The recorded owned group of a timed-out observation: its state now and, if
/// it still has processes, a few of them by pid and command. The form says so
/// plainly; the recorded group supplies no forward route while it is alive.
/// A running admission whose recorder ended is closed by a retry only when it
/// retains no capture or owned verification effect; the form says why.
pub(super) fn attach(loaded: &Loaded, work: &str, result: &mut Value, next: &Value) {
    let observation = &next["packet"]["checkObservation"];
    if observation["state"] == "running" {
        let retained = crate::run::check_observation::observer_ended(observation)
            && super::super::resolve(loaded, work)
                .and_then(|ledger| {
                    crate::run::check_observation::retained_effect(loaded, &ledger, observation)
                })
                .unwrap_or(false);
        if retained {
            result["meaning"] = json!(
                "The process observing this check ended without recording its result and the admission retains a partial capture or an owned verification effect. A retry neither closes nor appends over it: recover a known-ended storage failure with work recover-check, or block this Work."
            );
        }
        return;
    }
    let Some(view) = crate::run::owned_group::view(&observation["facts"]) else {
        return;
    };
    let group = view["ownedProcessGroup"].as_u64().unwrap_or_default();
    let meaning = match view["ownedProcessState"].as_str() {
        Some("ended") => format!(
            "The timed-out check's owned process group {group} has no remaining processes; the environmental re-run route is available."
        ),
        Some("alive") => format!(
            "The timed-out check's owned process group {group} still has processes: {}; wait for them to end, or stop this Work. No forward route is available while they run.",
            owned_process_names(&view)
        ),
        _ => format!(
            "The timed-out check's owned process group {group} could not be read on this host; wait and read this Work again. No forward route is available until it is observed ended."
        ),
    };
    if let Some(object) = view.as_object() {
        for (key, value) in object {
            result[key] = value.clone();
        }
    }
    result["meaning"] = json!(meaning);
}

fn owned_process_names(view: &Value) -> String {
    let named = view["ownedProcesses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|process| {
            Some(format!(
                "pid {} ({})",
                process["pid"].as_u64()?,
                process["command"].as_str()?
            ))
        })
        .collect::<Vec<_>>()
        .join(", ");
    if named.is_empty() {
        "unnamed processes".to_owned()
    } else {
        named
    }
}
