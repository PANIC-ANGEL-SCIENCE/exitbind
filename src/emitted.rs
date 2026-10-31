//! Emitted routes: the one builder for every argv the product prints for a
//! role to run, and the registry of every placeholder such an argv or a
//! printed decision template may carry, with the input that fills it.
//!
//! A printed route is a [`Route`] value: built from its command words here,
//! naming its placeholders through [`slot`] and rendered only by its own
//! renderers; [`fill`] substitutes supplied values so the filled route runs
//! without hand editing. The repository inventory tests scan `src/` and fail
//! on a `<NAME>` literal, an executable lookup or a command argv built outside
//! this module, on a route-carrying field not rendered from a [`Route`], and
//! on a slot name this registry does not carry.

use serde_json::{json, Value};
use std::collections::BTreeMap;

mod route;
pub(crate) use route::{unavailable, Bounded, Route};

/// How a placeholder is filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fill {
    /// The value of this option; the option precedes the placeholder in the
    /// printed argv and is the same option on the fill route.
    Flag(&'static str),
    /// A positional argument of the printed command.
    Positional,
    /// A field of a printed decision template the actor completes.
    Field(&'static str),
}

pub(crate) struct Placeholder {
    pub(crate) name: &'static str,
    pub(crate) fills: &'static [Fill],
    pub(crate) meaning: &'static str,
}

/// Every placeholder an emitted route or decision template may print.
pub(crate) const PLACEHOLDERS: &[Placeholder] = &[
    Placeholder {
        name: "REASON",
        fills: &[Fill::Flag("--reason"), Fill::Field("reason")],
        meaning: "the actor's reason for this decision",
    },
    Placeholder {
        name: "OPERATION",
        fills: &[Fill::Flag("--operation")],
        meaning: "the edit this permit covers",
    },
    Placeholder {
        name: "SANDBOX_MODE",
        fills: &[Fill::Flag("--sandbox-mode")],
        meaning: "the sandbox the human chooses for a relaunch: read-only, workspace-write or danger-full-access, never the recorded one",
    },
    Placeholder {
        name: "NEW_CHECK_COMMAND",
        fills: &[Fill::Flag("--check-command")],
        meaning: "the replacement check command",
    },
    Placeholder {
        name: "NEW_PRE_RETURN_COMMAND",
        fills: &[Fill::Flag("--pre-return-command")],
        meaning: "the replacement pre-return command",
    },
    Placeholder {
        name: "REPAIR_BOUNDARY",
        fills: &[Fill::Flag("--repair-boundary")],
        meaning: "what the repair may change",
    },
    Placeholder {
        name: "REGRESSION",
        fills: &[Fill::Flag("--regression")],
        meaning: "the regression check the repair must pass",
    },
    Placeholder {
        name: "SUCCESSOR_BASIS_JSON",
        fills: &[Fill::Flag("--successor-basis")],
        meaning: "the successor basis as JSON",
    },
    Placeholder {
        name: "HYPOTHESIS",
        fills: &[Fill::Flag("--hypothesis")],
        meaning: "the Lead's re-plan hypothesis",
    },
    Placeholder {
        name: "SCOPE_DECISION",
        fills: &[Fill::Flag("--scope-decision")],
        meaning: "the Lead's re-plan scope decision",
    },
    Placeholder {
        name: "EVIDENCE_REQUEST",
        fills: &[Fill::Flag("--evidence-request")],
        meaning: "the evidence the re-plan asks for",
    },
    Placeholder {
        name: "EFFECTS_REASON",
        fills: &[Fill::Field("effectsReason")],
        meaning: "the configured Lead's assessment of residual effects",
    },
    Placeholder {
        name: "ARTIFACT_ROOT",
        fills: &[Fill::Flag("--artifact-root")],
        meaning: "product or state",
    },
    Placeholder {
        name: "ARTIFACT_PATH",
        fills: &[Fill::Flag("--artifact")],
        meaning: "the evidence file under that root",
    },
    Placeholder {
        name: "SUCCESSOR_LEDGER",
        fills: &[Fill::Flag("--ledger")],
        meaning: "the new successor ledger path",
    },
    Placeholder {
        name: "OWNER_RECOVERY_STATE_ARTIFACT",
        fills: &[Fill::Flag("--owner-recovery")],
        meaning: "the owner's completed recovery decision file",
    },
    Placeholder {
        name: "QUESTION",
        fills: &[Fill::Flag("--question")],
        meaning: "the unresolved question",
    },
    Placeholder {
        name: "REQUEST_ID",
        fills: &[Fill::Flag("--request-id")],
        meaning: "a new request identity",
    },
    Placeholder {
        name: "PREVIOUS_CONFIG",
        fills: &[Fill::Flag("--from-config")],
        meaning: "the configuration the lesson events were stamped with",
    },
    Placeholder {
        name: "ITEM_ID",
        fills: &[Fill::Positional],
        meaning: "the project memory item ID from the listed line",
    },
    Placeholder {
        name: "OLD_LEDGER",
        fills: &[Fill::Positional],
        meaning: "the run ledger to supersede",
    },
    Placeholder {
        name: "WORKFLOW",
        fills: &[Fill::Flag("--workflow")],
        meaning: "the successor run's workflow",
    },
    Placeholder {
        name: "GOAL",
        fills: &[Fill::Flag("--goal")],
        meaning: "the successor run's goal",
    },
    Placeholder {
        name: "OWNER_DECISION",
        fills: &[Fill::Flag("--owner-decision")],
        meaning: "the owner's approved decision file",
    },
    Placeholder {
        name: "NEW_LEDGER",
        fills: &[Fill::Flag("--ledger")],
        meaning: "the new lesson ledger path",
    },
    Placeholder {
        name: "RFC3339_EXPIRY",
        fills: &[Fill::Flag("--expires-at")],
        meaning: "the expiry time",
    },
    Placeholder {
        name: "PROJECT_CHECKER",
        fills: &[Fill::Flag("--artifact")],
        meaning: "a non-ignored project checker file included in tested inputs",
    },
    Placeholder {
        name: "CHECK_COMMAND",
        fills: &[Fill::Flag("--check-command")],
        meaning: "the check command naming that checker",
    },
    Placeholder {
        name: "REVIEW_DECISION",
        fills: &[Fill::Flag("--review-policy")],
        meaning: "the owner's required or omitted review choice",
    },
    Placeholder {
        name: "ACCEPTED_WORK",
        fills: &[Fill::Flag("--result-ref")],
        meaning: "an accepted Work the Lead decides supports the category item",
    },
    Placeholder {
        name: "CURRENT_ACCEPTED_WORK",
        fills: &[Fill::Flag("--result-ref")],
        meaning: "an accepted current Work the Lead decides supports the category item",
    },
    Placeholder {
        name: "NEW_ACCEPTED_WORK",
        fills: &[Fill::Flag("--result-ref")],
        meaning: "the newly accepted Work",
    },
    Placeholder {
        name: "OUTCOME",
        fills: &[Fill::Flag("--outcome")],
        meaning: "the result outcome",
    },
    Placeholder {
        name: "WORK",
        fills: &[Fill::Positional],
        meaning: "the Work the refused command read",
    },
    Placeholder {
        name: "TASK",
        fills: &[Fill::Flag("--task")],
        meaning: "the task the capability applicability is read for",
    },
    Placeholder {
        name: "HOST",
        fills: &[Fill::Flag("--host")],
        meaning: "the host (codex or claude) the applicability is read for",
    },
    Placeholder {
        name: "ASSIGNMENT",
        fills: &[Fill::Positional],
        meaning: "the assignment whose finding the Lead decides",
    },
    Placeholder {
        name: "DISPOSITION_DECISION",
        fills: &[Fill::Flag("--decision")],
        meaning: "repair, defer, reject, supersede or environmental",
    },
    Placeholder {
        name: "FRESH_TOKEN",
        fills: &[Fill::Flag("--context")],
        meaning: "the fresh context token work continuation returns",
    },
    Placeholder {
        name: "NATIVE_SESSION",
        fills: &[Fill::Flag("--session")],
        meaning: "the host's native session identity",
    },
    Placeholder {
        name: "HOST_VERSION",
        fills: &[Fill::Flag("--host-version")],
        meaning: "the host's reported version",
    },
    Placeholder {
        name: "SHORT_ASSIGNMENT",
        fills: &[Fill::Positional],
        meaning: "a short description of the native child's task",
    },
    Placeholder {
        name: "CHILD_ID",
        fills: &[Fill::Flag("--native-child")],
        meaning: "the host-reported native child ID",
    },
    Placeholder {
        name: "SOURCE",
        fills: &[Fill::Positional],
        meaning: "the lesson source file",
    },
    Placeholder {
        name: "HISTORY_INDEX",
        fills: &[Fill::Flag("--history-index")],
        meaning: "the requirement history entry to read, from 0 below historyCount",
    },
    Placeholder {
        name: "SAVED_STEP",
        fills: &[Fill::Flag("--step")],
        meaning: "the step the Lead continues from after a pause",
    },
    Placeholder {
        name: "LEAD_DECISION",
        fills: &[Fill::Flag("--decision")],
        meaning: "a Lead decision taken on an in-flight Work before its Lead act",
    },
    Placeholder {
        name: "COMPLETE_SAVED_STORAGE_FAILURE_RESPONSE",
        fills: &[Fill::Field("response")],
        meaning: "the complete saved storage-failure response",
    },
    Placeholder {
        name: "SHA256_OF_RESPONSE_BYTES",
        fills: &[Fill::Field("responseSha256")],
        meaning: "the sha256 of those response bytes",
    },
    Placeholder {
        name: "OWNER_IDENTITY",
        fills: &[Fill::Field("owner")],
        meaning: "the owner's identity",
    },
    Placeholder {
        name: "OWNER_REASON",
        fills: &[Fill::Field("reason")],
        meaning: "the owner's reason",
    },
    Placeholder {
        name: "OWNER_INVOCATION_EVIDENCE",
        fills: &[Fill::Field("invocation.evidence")],
        meaning: "evidence of the exact invocation",
    },
    Placeholder {
        name: "EFFECT_EVIDENCE_PATH",
        fills: &[Fill::Field("effectsEvidence.path")],
        meaning: "the no-effect evidence file under the state root",
    },
    Placeholder {
        name: "EFFECT_EVIDENCE_SHA256",
        fills: &[Fill::Field("effectsEvidence.sha256")],
        meaning: "the sha256 of that evidence file",
    },
];

impl Placeholder {
    /// Whether `option` fills this placeholder.
    pub(crate) fn filled_by(&self, option: &str) -> bool {
        self.fills
            .iter()
            .any(|fill| matches!(fill, Fill::Flag(flag) if *flag == option))
    }
}

pub(crate) fn lookup(name: &str) -> Option<&'static Placeholder> {
    PLACEHOLDERS
        .iter()
        .find(|placeholder| placeholder.name == name)
}

/// The printed `<NAME>` token for a registered placeholder.
pub(crate) fn slot(name: &str) -> String {
    debug_assert!(lookup(name).is_some(), "unregistered placeholder {name}");
    format!("<{name}>")
}

/// The registered placeholder a printed token stands for, if any.
pub(crate) fn placeholder_in(token: &str) -> Option<&'static Placeholder> {
    let name = token.strip_prefix('<')?.strip_suffix('>')?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
    .then(|| lookup(name))
    .flatten()
}

/// The raw path of the executable this process runs as, for routes printed
/// as display text that must render a non-UTF-8 path lossily.
pub(crate) fn executable_path() -> Result<std::path::PathBuf, String> {
    std::env::current_exe()
        .map_err(|error| format!("invoking executable cannot be resolved: {error}"))
}

/// One printed command line in prose: the executable this process runs as,
/// quoted as one shell word, then `rest`. A path that cannot be resolved is
/// named as unavailable rather than replaced by a command name another
/// executable may answer.
fn command_text(rest: &str) -> String {
    format!("{} {rest}", executable_word())
}

/// The executable this process runs as, quoted as one shell word, or a note
/// that it is unavailable; for a renderer that quotes the rest itself.
pub(crate) fn executable_word() -> String {
    match executable_path() {
        Ok(path) => word(&path.display().to_string()),
        Err(_) => "(executable unavailable)".to_owned(),
    }
}

/// One printed command line in prose built from words: the executable, each
/// word, then `--config CONFIG` when the route reads a configuration. Every
/// word is quoted with [`word`], so a path with spaces survives copy-paste.
pub(crate) fn command_line(words: &[&str], config: Option<&std::path::Path>) -> String {
    let route = Route::new(words.iter().copied());
    match config {
        Some(config) => route.config(config).shell_text(),
        None => route.shell_text(),
    }
}

/// The follow-up a refusal names when its reader must read the current Work
/// again: `refresh with` the full `work next WORK --json` route.
pub(crate) fn refresh(work: &str, config: &std::path::Path) -> String {
    format!(
        "refresh with {}",
        command_line(&["work", "next", work, "--json"], Some(config))
    )
}

/// The status a Goal recovery or quarantine response points at: the full
/// `goal status` route with the owner's configuration.
pub(crate) fn goal_status(config: &std::path::Path) -> String {
    format!(
        "recompute with {}",
        command_line(&["goal", "status"], Some(config))
    )
}

/// One shell word as every printed command line renders it: plain when it
/// has only safe characters, otherwise single-quoted.
pub(crate) fn word(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:=<>@%+,".contains(&byte));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The executable a printed route names: the one this process runs as.
pub(crate) fn executable() -> Result<String, String> {
    executable_path()?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "invoking executable path is not valid UTF-8".to_owned())
}

/// The canonical executable path, for routes that record it.
pub(crate) fn canonical_executable() -> Result<String, String> {
    let path = std::fs::canonicalize(executable()?)
        .map_err(|error| format!("invoking executable cannot be canonicalized: {error}"))?;
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "invoking executable path is not valid UTF-8".to_owned())
}

/// The option that fills each placeholder in this argv, as `{NAME: option}`.
pub(crate) fn fill_flags(argv: &[String]) -> Value {
    let mut map = serde_json::Map::new();
    for (index, token) in argv.iter().enumerate() {
        if let Some(placeholder) = placeholder_in(token) {
            let option = index
                .checked_sub(1)
                .and_then(|before| argv.get(before))
                .filter(|before| placeholder.filled_by(before));
            if let Some(option) = option {
                map.insert(placeholder.name.into(), json!(option));
            }
        }
    }
    Value::Object(map)
}

/// Substitute every placeholder of a printed route from `values`, keyed by
/// the option that fills it. Every placeholder must be filled, and every
/// supplied value must fill one; the route is otherwise unchanged.
pub(crate) fn fill(printed: &Route, values: &BTreeMap<String, String>) -> Result<Route, String> {
    let argv = printed.full_argv()?;
    let mut used = std::collections::BTreeSet::new();
    let mut filled = Vec::with_capacity(argv.len());
    for (index, token) in argv.iter().enumerate() {
        let Some(placeholder) = placeholder_in(token) else {
            filled.push(token.clone());
            continue;
        };
        let option = index
            .checked_sub(1)
            .map(|before| argv[before].as_str())
            .filter(|before| placeholder.filled_by(before))
            .ok_or_else(|| {
                format!(
                    "placeholder {} has no fill option in this route",
                    placeholder.name
                )
            })?;
        let value = values.get(option).ok_or_else(|| {
            format!(
                "fill {} ({}) with {option} VALUE",
                placeholder.name, placeholder.meaning
            )
        })?;
        used.insert(option.to_owned());
        filled.push(value.clone());
    }
    if let Some(unused) = values.keys().find(|option| !used.contains(option.as_str())) {
        return Err(format!("{unused} fills no placeholder of this route"));
    }
    Route::printed(&filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn fill_replaces_every_placeholder_by_its_option_and_refuses_gaps() {
        let printed = argv(&[
            "x",
            "work",
            "return",
            "--reason",
            &slot("REASON"),
            "--config",
            "c",
        ]);
        assert_eq!(fill_flags(&printed), json!({"REASON": "--reason"}));
        let values = BTreeMap::from([("--reason".to_owned(), "two words".to_owned())]);
        let route = Route::printed(&printed).unwrap();
        assert_eq!(
            fill(&route, &values).unwrap().full_argv().unwrap(),
            argv(&[
                "x",
                "work",
                "return",
                "--reason",
                "two words",
                "--config",
                "c"
            ])
        );
        assert!(fill(&route, &BTreeMap::new())
            .unwrap_err()
            .contains("--reason"));
        let extra = BTreeMap::from([
            ("--reason".to_owned(), "r".to_owned()),
            ("--question".to_owned(), "q".to_owned()),
        ]);
        assert!(fill(&route, &extra).unwrap_err().contains("--question"));
        assert!(placeholder_in("<NOT_REGISTERED>").is_none());
    }

    #[test]
    fn every_registered_placeholder_has_a_fill_and_a_unique_name() {
        let mut names = std::collections::BTreeSet::new();
        for placeholder in PLACEHOLDERS {
            assert!(names.insert(placeholder.name), "{}", placeholder.name);
            assert!(!placeholder.fills.is_empty(), "{}", placeholder.name);
            assert!(!placeholder.meaning.is_empty(), "{}", placeholder.name);
            assert_eq!(
                placeholder_in(&slot(placeholder.name)).map(|p| p.name),
                Some(placeholder.name)
            );
        }
    }
}
