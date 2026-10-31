//! Changed after binding: the one typed refusal for "what was bound earlier
//! differs from what is current".
//!
//! Every agent-reachable refusal of this class is built here from a
//! registered [`Class`]: it carries the classification, the bound and the
//! current sha256, the kept copy when one exists and `nextAction` with the
//! exact command, and renders the same facts in text and `--json`. The
//! refusal travels as an error string with [`PREFIX`]; the CLI entry point
//! renders it (`crate::main`). The repository inventory test scans `src/`
//! for the class signature and fails on a hit that is neither built here nor
//! registered there as an internal self-consistency check.

use serde_json::{json, Value};

/// Marks a typed refusal inside an error string, wherever a caller added
/// context before it.
pub(crate) const PREFIX: &str = "EXITBIND_CHANGED:";

/// One registered member of the class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// Lesson events were stamped with a configuration other than the current one.
    MemoryConfiguration,
    /// A memory item's recorded source bytes differ from the file.
    MemorySource,
    /// A lesson's recorded source bytes differ from the file during review.
    LessonSource,
    /// The configuration change under review changes lesson authority.
    LessonAuthority,
    /// The configuration change under review changes the project or its owner.
    LessonOwnership,
    /// A frozen verification checker's bytes or path differ.
    VerificationChecker,
    /// The check command names checker sources other than the frozen ones.
    VerificationCheckerCommand,
    /// A frozen pre-return checker's bytes or path differ.
    PreReturnChecker,
    /// The configuration differs outside the marked advisory fields.
    GuidanceConfiguration,
    /// A marked check observation needs the frozen configuration.
    CheckObservationConfiguration,
    /// A pre-return check needs the frozen configuration.
    PreReturnConfiguration,
    /// A pre-return check needs the tested product inputs.
    PreReturnInputs,
    /// A pre-return recovery no longer matches its bound predecessor.
    PreReturnRecoveryPredecessor,
    /// A pre-return recovery no longer matches its retained evidence.
    PreReturnRecoveryEvidence,
    /// A pre-return recovery decision no longer matches its preview or replay.
    PreReturnRecoveryDecision,
    /// A forward repair needs the frozen configuration.
    RepairConfiguration,
    /// The selected architecture contract bytes differ.
    ArchitectureSource,
    /// The selected architecture contract revision differs.
    ArchitectureRevision,
    /// The architecture selection differs from the assignment's configuration.
    ArchitectureSelection,
    /// A lesson event's actor profile differs from the current profile file.
    MemoryActorProfile,
    /// A saved recovery hint's owner configuration differs from the file.
    RecoveryOwnerConfiguration,
    /// A recovery reference's Work configuration differs from the file.
    RecoveryWorkConfiguration,
    /// A journey member's stamped configuration bytes are not kept.
    JourneyMemberConfiguration,
}

/// What a class says and which route it offers.
pub(crate) struct Member {
    pub(crate) class: Class,
    pub(crate) classification: &'static str,
    pub(crate) message: &'static str,
    pub(crate) meaning: &'static str,
}

/// Every agent-reachable refusal of the class.
pub(crate) const MEMBERS: &[Member] = &[
    Member {
        class: Class::MemoryConfiguration,
        classification: "memory_configuration_changed",
        message: "configuration changed since memory event",
        meaning: "review the change against the configuration the lesson events were stamped with and record it with your reason; a change to lesson authority still refuses",
    },
    Member {
        class: Class::MemorySource,
        classification: "memory_source_changed",
        message: "memory source changed since proposal",
        meaning: "restore the proposed bytes, or retire this item and propose a new immutable source",
    },
    Member {
        class: Class::LessonSource,
        classification: "lesson_source_changed",
        message: "lesson source changed since proposal",
        meaning: "restore the proposed bytes, or retire this lesson and propose a new immutable source",
    },
    Member {
        class: Class::LessonAuthority,
        classification: "lesson_authority_changed",
        message: "incompatible lesson authority/configuration change; only additive recipient read rights and fields that carry no lesson authority (profile, nativeName, displayName, phaseGuidance) are supported",
        meaning: "restore the bound lesson authority, or retire this lesson and carry it again under the current configuration",
    },
    Member {
        class: Class::LessonOwnership,
        classification: "lesson_ownership_changed",
        message: "lesson revalidation project or ownership changed",
        meaning: "restore the bound project roots and Lead, or retire this lesson and carry it again under the current configuration",
    },
    Member {
        class: Class::VerificationChecker,
        classification: "verification_checker_changed",
        message: "verification checker source changed; use authorized supersession to bind the changed checker",
        meaning: "restore the checker's bound bytes, or bind the changed checker in a successor run",
    },
    Member {
        class: Class::VerificationCheckerCommand,
        classification: "verification_checker_command_changed",
        message: "verification checker source path or contents changed since Work start; no check was launched",
        meaning: "restore the checker the Work started with, or bind the changed checker in a successor run",
    },
    Member {
        class: Class::PreReturnChecker,
        classification: "pre_return_checker_changed",
        message: "pre-return checker source changed; use authorized supersession to bind the changed checker",
        meaning: "restore the checker's bound bytes, or bind the changed checker in a successor run",
    },
    Member {
        class: Class::GuidanceConfiguration,
        classification: "guidance_configuration_changed",
        message: "project configuration changed outside the marked advisory fields",
        meaning: "restore the bound configuration, or bind the changed configuration in a successor run",
    },
    Member {
        class: Class::CheckObservationConfiguration,
        classification: "check_configuration_changed",
        message: "check observation requires the frozen current configuration; inspect the Work and use authorized supersession for a changed configuration",
        meaning: "restore the bound configuration, or bind the changed configuration in a successor run",
    },
    Member {
        class: Class::PreReturnConfiguration,
        classification: "pre_return_configuration_changed",
        message: "pre-return check requires the frozen current configuration; inspect and use authorized supersession for a changed configuration",
        meaning: "restore the bound configuration, or bind the changed configuration in a successor run",
    },
    Member {
        class: Class::PreReturnInputs,
        classification: "pre_return_inputs_changed",
        message: "pre-return check requires current product inputs; use authorized supersession after input drift",
        meaning: "restore the tested inputs, or bind the changed inputs in a successor run",
    },
    Member {
        class: Class::PreReturnRecoveryPredecessor,
        classification: "pre_return_recovery_predecessor_changed",
        message: "pre-return recovery requires its exact bound predecessor and subject",
        meaning: "inspect the current Work and preserve the retained admission; do not retry the interrupted execution",
    },
    Member {
        class: Class::PreReturnRecoveryEvidence,
        classification: "pre_return_recovery_evidence_changed",
        message: "pre-return recovery requires its exact retained evidence",
        meaning: "inspect the current Work and preserve the bound evidence before any reconciliation or retry",
    },
    Member {
        class: Class::PreReturnRecoveryDecision,
        classification: "pre_return_recovery_decision_changed",
        message: "pre-return recovery decision does not match its bound state",
        meaning: "inspect the current Work and follow its current recovery action; do not repeat the interrupted execution",
    },
    Member {
        class: Class::RepairConfiguration,
        classification: "repair_configuration_changed",
        message: "forward repair refuses changed configuration; use authorized supersession",
        meaning: "restore the bound configuration, or bind the changed configuration in a successor run",
    },
    Member {
        class: Class::ArchitectureSource,
        classification: "architecture_source_changed",
        message: "architecture contract source changed; restore the selected bytes or use the existing owner approval/configuration and run supersede paths",
        meaning: "restore the selected contract bytes, or bind the changed contract in a successor run",
    },
    Member {
        class: Class::ArchitectureRevision,
        classification: "architecture_revision_changed",
        message: "architecture contract revision differs from project configuration",
        meaning: "restore the selected revision, or bind the changed contract in a successor run",
    },
    Member {
        class: Class::ArchitectureSelection,
        classification: "architecture_selection_changed",
        message: "architecture selection differs from the assignment configuration; use the existing run supersede path",
        meaning: "restore the assignment's configuration, or bind the changed selection in a successor run",
    },
    Member {
        class: Class::MemoryActorProfile,
        classification: "memory_actor_profile_changed",
        message: "memory actor profile changed",
        meaning: "restore the actor profile the lesson events were stamped with, or retire this lesson and carry it again under the current profile",
    },
    Member {
        class: Class::RecoveryOwnerConfiguration,
        classification: "recovery_owner_configuration_changed",
        message: "saved native owner configuration changed",
        meaning: "read the current recovery record; record a new hint only with the verified current owner configuration",
    },
    Member {
        class: Class::RecoveryWorkConfiguration,
        classification: "recovery_work_configuration_changed",
        message: "session recovery Work configuration changed",
        meaning: "read the current recovery record and the Work's current detail; a saved reference is navigation, never authority",
    },
    Member {
        class: Class::JourneyMemberConfiguration,
        classification: "journey_member_configuration_changed",
        message: "journey member belongs to another or changed project/session configuration",
        meaning: "supply the configuration these journey members were stamped with; it is kept under its hash and admits them when the change moves no authority or read right",
    },
];

/// The command a refusal offers next.
pub(crate) enum Route<'a> {
    /// `memory revalidate`: review a configuration change for a lesson ledger.
    Revalidate {
        lead: &'a str,
        ledgers: &'a [String],
        kept_copy: Option<&'a str>,
        config: &'a std::path::Path,
    },
    /// `memory inspect`: read the lesson before restoring or retiring it.
    Inspect {
        ledger: &'a str,
        config: &'a std::path::Path,
    },
    /// `memory resolve`: read the accepted items the agent receives now.
    Resolve {
        agent: &'a str,
        config: &'a std::path::Path,
    },
    /// `session recovery show`: read the current recovery record.
    RecoveryShow,
    /// `work detail`: inspect the current action without changing it.
    WorkDetail {
        work: &'a str,
        config: &'a std::path::Path,
    },
    /// `run supersede`: bind the changed input in a successor run.
    Supersede {
        ledger: Option<&'a str>,
        config: Option<&'a std::path::Path>,
    },
}

fn member(class: Class) -> &'static Member {
    MEMBERS
        .iter()
        .find(|member| member.class == class)
        .expect("every class is registered")
}

impl Route<'_> {
    /// The emitted route this follow-up names, and whether it only reads.
    fn route(&self) -> (crate::emitted::Route, bool) {
        use crate::emitted::Route as Emitted;
        match self {
            Route::Revalidate {
                lead,
                ledgers,
                kept_copy,
                config,
            } => {
                let from = kept_copy
                    .map_or_else(|| crate::emitted::slot("PREVIOUS_CONFIG"), str::to_owned);
                let route = Emitted::new(["memory", "revalidate", *lead])
                    .words(ledgers.iter().cloned())
                    .words([
                        "--from-config",
                        from.as_str(),
                        "--reason",
                        crate::emitted::slot("REASON").as_str(),
                        "--apply",
                    ])
                    .config(config);
                (route, false)
            }
            Route::Inspect { ledger, config } => (
                Emitted::new(["memory", "inspect", *ledger]).config(config),
                true,
            ),
            Route::Resolve { agent, config } => (
                Emitted::new(["memory", "resolve", *agent, "--json"]).config(config),
                true,
            ),
            Route::RecoveryShow => (
                Emitted::new(["session", "recovery", "show", "--json"]),
                true,
            ),
            Route::WorkDetail { work, config } => (
                Emitted::new(["work", "detail", *work, "--json"]).config(config),
                true,
            ),
            Route::Supersede { ledger, config } => {
                let old = ledger.map_or_else(|| crate::emitted::slot("OLD_LEDGER"), str::to_owned);
                let route = Emitted::new([
                    "run",
                    "supersede",
                    old.as_str(),
                    "--workflow",
                    crate::emitted::slot("WORKFLOW").as_str(),
                    "--goal",
                    crate::emitted::slot("GOAL").as_str(),
                    "--ledger",
                    crate::emitted::slot("SUCCESSOR_LEDGER").as_str(),
                ]);
                let route = match config {
                    Some(config) => route.config(config),
                    None => route,
                };
                (route, false)
            }
        }
    }
}

/// Build the typed refusal for `class`: bound and current sha256 (current is
/// `None` when the bound input is unavailable), the kept copy when one
/// exists, and the route.
pub(crate) fn refusal(
    class: Class,
    bound: &str,
    current: Option<&str>,
    kept_copy: Option<&str>,
    route: Route<'_>,
) -> String {
    format!(
        "{PREFIX}{}",
        machine(class, bound, current, kept_copy, &route)
    )
}

/// The `nextAction` object for `route`, with the meaning the caller gives.
pub(crate) fn next_action(route: &Route<'_>, meaning: &str) -> Value {
    let (route, read_only) = route.route();
    let command = route.shown_argv();
    json!({
        "command": command,
        "fill": crate::emitted::fill_flags(&command),
        "readOnly": read_only,
        "meaning": meaning,
    })
}

fn machine(
    class: Class,
    bound: &str,
    current: Option<&str>,
    kept_copy: Option<&str>,
    route: &Route<'_>,
) -> Value {
    let member = member(class);
    json!({
        "error": member.message,
        "classification": member.classification,
        "boundSha256": bound,
        "currentSha256": current,
        "keptCopy": kept_copy,
        "nextAction": next_action(route, member.meaning),
    })
}

/// The typed refusal inside `error`, with any context a caller put before it.
pub(crate) fn split(error: &str) -> Option<(&str, Value)> {
    let at = error.find(PREFIX)?;
    let value = serde_json::from_str(&error[at + PREFIX.len()..]).ok()?;
    Some((&error[..at], value))
}

/// The machine form main prints for `--json`: the caller's context, if any,
/// stays in front of the message.
pub(crate) fn machine_with_context(context: &str, mut value: Value) -> Value {
    if !context.is_empty() {
        value["error"] = json!(format!(
            "{context}{}",
            value["error"].as_str().unwrap_or_default()
        ));
    }
    value
}

/// The human form of a typed refusal: the same classification, both hashes,
/// the kept copy and the route as the machine form.
pub(crate) fn human(value: &Value) -> String {
    let hash = |field: &str| value[field].as_str().unwrap_or("unavailable").to_owned();
    let mut text = format!(
        "{} [{}]: bound sha256 {}, current sha256 {}",
        value["error"].as_str().unwrap_or("changed after binding"),
        value["classification"].as_str().unwrap_or("unknown"),
        hash("boundSha256"),
        hash("currentSha256"),
    );
    if let Some(copy) = value["keptCopy"].as_str() {
        text.push_str(&format!("; kept copy {copy}"));
    }
    let next = &value["nextAction"];
    let argv: Vec<String> = next["command"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let command = crate::emitted::Route::printed(&argv)
        .map(|route| route.shell_text())
        .unwrap_or_default();
    if !command.is_empty() {
        text.push_str(&format!(
            ". Next: {command} ({})",
            next["meaning"].as_str().unwrap_or("")
        ));
        let fills = next["fill"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, option)| format!("{name} with {}", option.as_str().unwrap_or("?")))
            .collect::<Vec<_>>();
        if !fills.is_empty() {
            text.push_str(&format!("; fill {}", fills.join(", ")));
        }
    }
    text
}

/// A plain sentence for a typed refusal embedded in recorded diagnostics, so
/// a persisted record never carries the internal channel.
pub(crate) fn plain(error: &str) -> String {
    match split(error) {
        Some((context, value)) => format!("{context}{}", human(&value)),
        None => error.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_member_renders_classification_hashes_and_route_in_text_and_json() {
        let config = std::path::Path::new("/project dir/exitbind.json");
        let ledgers = [".exitbind/memory/l.jsonl".to_owned()];
        let mut names = std::collections::BTreeSet::new();
        for member in MEMBERS {
            assert!(
                names.insert(member.classification),
                "{}",
                member.classification
            );
            for route in [
                Route::Revalidate {
                    lead: "lead",
                    ledgers: &ledgers,
                    kept_copy: Some("/kept/copy.json"),
                    config,
                },
                Route::Inspect {
                    ledger: ".exitbind/memory/l.jsonl",
                    config,
                },
                Route::Resolve {
                    agent: "worker",
                    config,
                },
                Route::RecoveryShow,
                Route::Supersede {
                    ledger: None,
                    config: Some(config),
                },
            ] {
                let error = format!(
                    "context: {}",
                    refusal(
                        member.class,
                        "aaaa",
                        Some("bbbb"),
                        Some("/kept/copy.json"),
                        route
                    )
                );
                let (context, value) = split(&error).unwrap();
                assert_eq!(context, "context: ");
                assert_eq!(value["classification"], member.classification);
                assert_eq!(value["boundSha256"], "aaaa");
                assert_eq!(value["currentSha256"], "bbbb");
                let command = value["nextAction"]["command"].as_array().unwrap();
                assert!(command.len() > 3, "{value}");
                let text = plain(&error);
                for expected in [
                    member.message,
                    member.classification,
                    "aaaa",
                    "bbbb",
                    "Next: ",
                ] {
                    assert!(text.contains(expected), "{expected}: {text}");
                }
                if command.iter().any(|word| word == "--config") {
                    assert!(text.contains("'/project dir/exitbind.json'"), "{text}");
                }
                assert!(!text.contains(PREFIX), "{text}");
            }
        }
        assert_eq!(names.len(), MEMBERS.len());
    }

    #[test]
    fn an_unavailable_current_input_and_a_missing_copy_read_as_such() {
        let config = std::path::Path::new("/p/exitbind.json");
        let error = refusal(
            Class::MemoryConfiguration,
            "aaaa",
            None,
            None,
            Route::Revalidate {
                lead: "lead",
                ledgers: &["l.jsonl".to_owned()],
                kept_copy: None,
                config,
            },
        );
        let (_, value) = split(&error).unwrap();
        assert!(value["currentSha256"].is_null());
        assert_eq!(
            value["nextAction"]["fill"],
            json!({"PREVIOUS_CONFIG": "--from-config", "REASON": "--reason"})
        );
        let text = human(&value);
        assert!(text.contains("current sha256 unavailable"), "{text}");
        assert!(
            text.contains("fill PREVIOUS_CONFIG with --from-config"),
            "{text}"
        );
    }
}
