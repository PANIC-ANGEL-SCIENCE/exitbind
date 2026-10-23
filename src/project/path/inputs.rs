//! Every path input meets its root here. Each path a command or configuration
//! accepts is registered once with one declared outcome:
//!
//! - `Confined`: judged against the named root by this module; an outside path
//!   is refused with one text naming the input and its route.
//! - `Snapshotted`: an outside file is copied through the plan-card route
//!   (`.exitbind/plan-cards/<goal id>/…`, hidden from Git by the private
//!   writer) and the copy is what the product reads.
//! - `OutsideByHash`: an outside file is allowed and pinned by sha256.
//! - `Selector`: the path selects a root, the configuration or an executable;
//!   it is the place other inputs are judged against, not judged itself.
//! - `ReadOnce`: the bytes are read once wherever they are; the path is never
//!   recorded, re-read or written.
//!
//! `within` is the one prefix decision for a path input; the judges named in
//! the registry call it instead of comparing prefixes themselves.

use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Root {
    Product,
    State,
    Control,
}

impl Root {
    fn name(self) -> &'static str {
        match self {
            Self::Product => "project root",
            Self::State => "state root",
            Self::Control => "control root",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Confined(Root),
    Snapshotted,
    OutsideByHash,
    Selector,
    ReadOnce,
}

/// One registered path input.
pub(crate) struct Input {
    /// The judge's label for this input; refusals are looked up by it.
    pub(crate) label: &'static str,
    /// The options, positionals or configuration fields that carry it.
    pub(crate) options: &'static str,
    pub(crate) commands: &'static str,
    pub(crate) outcome: Outcome,
    /// The module that applies the outcome; the inventory test reads it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) judge: &'static str,
    /// What to do with an outside path.
    pub(crate) route: &'static str,
}

/// Every path input, with its outcome.
pub(crate) const INPUTS: &[Input] = &[
    Input {
        label: "task card",
        options: "--task-card",
        commands: "setup",
        outcome: Outcome::Snapshotted,
        judge: "src/project/setup_card.rs",
        route: "an outside card is snapshotted under .exitbind/plan-cards/<goal id>/TASK.md",
    },
    Input {
        label: "requirement source",
        options: "--artifact",
        commands: "goal require",
        outcome: Outcome::Confined(Root::Product),
        judge: "src/session_goal/requirements.rs",
        route: "pass the outside card to setup --apply as its --task-card value, which snapshots it inside the project, or copy the source into the project",
    },
    Input {
        label: "project checker",
        options: "--artifact",
        commands: "goal prepare, work begin",
        outcome: Outcome::Confined(Root::Product),
        judge: "src/session_goal/requirements.rs",
        route: "name a tested checker inside the project; an outside script is pinned by sha256 when --check-command runs it",
    },
    Input {
        label: "evidence artifact",
        options: "--artifact, --probe-source, --owner-recovery",
        commands: "run submit, work evidence, work challenge, run supersede",
        outcome: Outcome::Confined(Root::Product),
        judge: "src/run/artifact.rs",
        route: "write the artifact under the selected root first (--artifact-root product or state)",
    },
    Input {
        label: "ledger",
        options: "LEDGER, --ledger",
        commands: "run, memory, receipt, away start",
        outcome: Outcome::Confined(Root::State),
        judge: "src/run/ledger.rs, src/memory/ledger.rs",
        route: "name a state-root-relative ledger such as .exitbind/runs/<name>.jsonl",
    },
    Input {
        label: "source",
        options: "SOURCE",
        commands: "memory propose",
        outcome: Outcome::Confined(Root::Product),
        judge: "src/memory/ledger.rs",
        route: "write the source inside the project first",
    },
    Input {
        label: "receipt",
        options: "--receipt, --output, --harness-receipt, RECEIPT",
        commands: "brief, receipt, work closeout, run start, work begin, verify, memory attest-forgotten",
        outcome: Outcome::Confined(Root::State),
        judge: "src/evidence/receipt_path.rs, src/memory/ledger.rs, src/work/closeout.rs",
        route: "use a StateRoot-relative path or an absolute path whose parent resolves inside StateRoot",
    },
    Input {
        label: "harness manifest",
        options: "--harness-manifest",
        commands: "brief",
        outcome: Outcome::Confined(Root::Control),
        judge: "src/evidence/harness.rs",
        route: "keep the manifest at its canonical path under the control root",
    },
    Input {
        label: "actor profile",
        options: "agents.<name>.profile",
        commands: "memory attest-forgotten",
        outcome: Outcome::Confined(Root::Control),
        judge: "src/memory/forgetting.rs",
        route: "keep the profile under the control root",
    },
    Input {
        label: "check script",
        options: "--check-command, --pre-return-command, --preservation-check-command",
        commands: "setup, work begin, goal prepare, run start",
        outcome: Outcome::OutsideByHash,
        judge: "src/run/gate/value.rs",
        route: "an outside script is allowed and pinned by sha256 at Work start",
    },
    Input {
        label: "owner configuration",
        options: "--owner-config",
        commands: "work quarantine",
        outcome: Outcome::OutsideByHash,
        judge: "src/work/quarantine.rs",
        route: "pinned by --owner-config-sha256",
    },
    Input {
        label: "root",
        options: "--root, --control-root, --state-root, --config",
        commands: "every command",
        outcome: Outcome::Selector,
        judge: "src/config/mod.rs, src/project/setup.rs",
        route: "selects the roots other inputs are judged against",
    },
    Input {
        label: "executable",
        options: "--codex-bin",
        commands: "work act, activity codex",
        outcome: Outcome::Selector,
        judge: "src/host/codex_exec.rs",
        route: "selects the host executable to run",
    },
    Input {
        label: "read once",
        options: "--artifact-file, --replan-file, --from-config, --lessons-from, --source-config, --endpoint-configs, --historical-fixtures, --feature-config, --packet, --events, --state, --proposal, --output (value-benchmark)",
        commands: "work fill, work replan, memory revalidate, setup, goal quarantine, goal recover, work quarantine, work validate, context, value-benchmark",
        outcome: Outcome::ReadOnce,
        judge: "the command's own reader",
        route: "read once wherever it is; the path is never recorded or written",
    },
    Input {
        label: "phase guidance",
        options: "agents.<name>.phaseGuidance[].path",
        commands: "work detail, hook-run",
        outcome: Outcome::ReadOnce,
        judge: "src/config/phase_guidance.rs",
        route: "read-only guidance, read as written; it grants nothing",
    },
    Input {
        label: "memory root",
        options: "memory.root",
        commands: "configuration",
        outcome: Outcome::Confined(Root::State),
        judge: "src/memory/policy.rs, src/memory/discovery.rs",
        route: "memory.root must stay inside the project: use a relative path without '..'",
    },
];

/// Value options that carry no path: identifiers, text, numbers, modes and
/// boundary patterns that `config::validate` judges as patterns.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const NOT_PATHS: &[&str] = &[
    "session-key",
    "step",
    "agent",
    "role",
    "artifact-root",
    "choice",
    "first-work",
    "basis",
    "assessment",
    "boundary",
    "classification",
    "source-event",
    "source-kind",
    "current-binding",
    "context",
    "confidence",
    "consider",
    "duration-ms",
    "expected-sha256",
    "event",
    "external-scope",
    "expires-at",
    "exit-code",
    "forbid-term",
    "goal",
    "history-index",
    "hosts",
    "hypothesis",
    "identity-source",
    "index",
    "input-digest",
    "evidence-request",
    "scope-decision",
    "section",
    "view",
    "blocker",
    "mode",
    "model",
    "material-consequence",
    "name",
    "none-applicable",
    "owner-decision",
    "outcome",
    "operation",
    "pre-return-timeout-ms",
    "probe-command",
    "pre-return-capture-bytes",
    "verification-preparation",
    "question",
    "stream",
    "preservation-proof-origin",
    "perspectives",
    "preserve-requirement",
    "proof-origin",
    "promotion-required",
    "previous",
    "purpose",
    "project-id",
    "query",
    "request-id",
    "reason",
    "reasoning-effort",
    "requirement",
    "scope",
    "feature-work",
    "owner-config-sha256",
    "verification-work",
    "observe",
    "write",
    "commands",
    "lead-observe",
    "lead-write",
    "lead-commands",
    "worker-observe",
    "worker-write",
    "worker-commands",
    "reviewer-observe",
    "reviewer-write",
    "reviewer-commands",
    "goal-id",
    "obligation",
    "finding",
    "decision",
    "external-action",
    "result-ref",
    "review-policy",
    "disposition",
    "repair-boundary",
    "regression",
    "category",
    "successor-basis",
    "sandbox-mode",
    "task",
    "target",
    "host",
    "agent-type",
    "session",
    "host-version",
    "native-child",
    "result",
    "timeout-ms",
    "workflow",
];

pub(crate) fn input(label: &str) -> Option<&'static Input> {
    INPUTS.iter().find(|input| input.label == label)
}

/// The one prefix decision: `path` relative to `root` when it lies beneath
/// it. Both are compared as given; the judge decides what to canonicalize.
pub(crate) fn within<'a>(root: &Path, path: &'a Path) -> Option<&'a Path> {
    path.strip_prefix(root).ok()
}

/// The refusal for an input outside its root: the input, the root and the
/// route. An unregistered label keeps the generic text.
pub(crate) fn refusal(label: &str, given: &str) -> String {
    refusal_in(label, None, given)
}

/// The refusal naming `root` when the command selected it (for example
/// `--artifact-root state`), else the registered root.
pub(crate) fn refusal_in(label: &str, root: Option<Root>, given: &str) -> String {
    match input(label) {
        Some(input) => {
            let root = match (root, input.outcome) {
                (Some(root), _) | (None, Outcome::Confined(root)) => root.name(),
                _ => "project root",
            };
            format!(
                "path escapes the {root}: {} ({} for {}) {given}; {}",
                input.label, input.options, input.commands, input.route
            )
        }
        None => format!("path escapes project root: {label}"),
    }
}

/// Where a given path lies relative to `root`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    /// Root-relative, `/`-separated, UTF-8.
    Inside(String),
    /// The canonical outside path.
    Outside(PathBuf),
}

/// Place `given` against `root`: a relative path must stay beneath it
/// lexically; an absolute path is compared after canonicalizing its parent
/// (the final component is kept, so a missing file can still be placed).
pub(crate) fn place(root: &Path, given: &str) -> Result<Placed, String> {
    if given.trim().is_empty() || given.contains('\0') {
        return Err("path must be a non-empty string".into());
    }
    let path = Path::new(given);
    let relative = if path.is_absolute() {
        let real_root = root
            .canonicalize()
            .map_err(|error| format!("root {} cannot be resolved: {error}", root.display()))?;
        let parent = path
            .parent()
            .ok_or("path names no file")?
            .canonicalize()
            .map_err(|error| format!("{given} cannot be read: {error}"))?;
        let resolved = parent.join(path.file_name().ok_or("path names no file")?);
        match within(&real_root, &resolved) {
            Some(relative) => relative.to_path_buf(),
            None => return Ok(Placed::Outside(resolved)),
        }
    } else {
        path.to_path_buf()
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or("path is not UTF-8")?),
            Component::CurDir => {}
            _ => return Err(format!("path must stay beneath the root: {given}")),
        }
    }
    if parts.is_empty() {
        return Err("path names no file".into());
    }
    Ok(Placed::Inside(parts.join("/")))
}

/// The root-relative form of a confined input, or its registered refusal.
pub(crate) fn confined(root: &Path, label: &str, given: &str) -> Result<String, String> {
    confined_in(root, None, label, given)
}

/// `confined` against the root the command selected, named in the refusal.
pub(crate) fn confined_in(
    path: &Path,
    root: Option<Root>,
    label: &str,
    given: &str,
) -> Result<String, String> {
    match place(path, given)? {
        Placed::Inside(relative) => Ok(relative),
        Placed::Outside(_) => Err(refusal_in(label, root, given)),
    }
}

/// The root-relative form of `given` when it names a path inside `root`; a
/// classifier for text that may name a path, never a refusal.
pub(crate) fn relative(root: &Path, given: &str) -> Option<String> {
    match place(root, given) {
        Ok(Placed::Inside(relative)) => Some(relative),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_confined_input_refuses_an_outside_path_naming_the_input_and_route() {
        let base = crate::test_root::temp("inputs");
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let outside = base.join("outside.md");
        std::fs::write(&outside, "x").unwrap();
        let outside = outside.to_str().unwrap();
        let mut confined_inputs = 0;
        for input in INPUTS {
            assert!(!input.judge.is_empty());
            if let Outcome::Confined(_) = input.outcome {
                confined_inputs += 1;
                let error = confined(&root, input.label, outside).unwrap_err();
                assert!(error.contains(input.label), "{error}");
                assert!(error.contains(input.options), "{error}");
                assert!(error.contains(input.route), "{error}");
                assert!(error.contains(outside), "{error}");
            }
        }
        assert!(confined_inputs >= 8);
        let state =
            confined_in(&root, Some(Root::State), "evidence artifact", outside).unwrap_err();
        assert!(state.starts_with("path escapes the state root"), "{state}");
        assert_eq!(
            confined(&root, "requirement source", "a/./b.md").unwrap(),
            "a/b.md"
        );
        assert!(confined(&root, "requirement source", "../b.md").is_err());
        let inside = root.join("c.md");
        assert_eq!(
            confined(&root, "requirement source", inside.to_str().unwrap()).unwrap(),
            "c.md"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn labels_are_unique_and_no_option_is_both_path_and_not_path() {
        for (index, input) in INPUTS.iter().enumerate() {
            assert!(INPUTS[..index]
                .iter()
                .all(|other| other.label != input.label));
            for option in input.options.split(", ") {
                let name = option.trim_start_matches("--");
                let name = name.split(' ').next().unwrap();
                assert!(!NOT_PATHS.contains(&name), "{name}");
            }
        }
    }
}
