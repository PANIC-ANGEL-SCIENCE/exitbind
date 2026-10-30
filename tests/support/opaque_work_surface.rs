//! Read-only work packets stay opaque. A bounded mutation result exposes only
//! the exact recorded-event ledger selector in its inspection route.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn assert_opaque_reference(reference: &Value, expected: Value) {
    assert_eq!(reference, &expected);
    let serialized = reference.to_string();
    assert!(!serialized.contains("ledger"));
    assert!(!serialized.contains(".exitbind"));
    assert!(!serialized.contains("runs/"));
}

struct Identity {
    invoked: PathBuf,
    resolved: PathBuf,
}

impl Identity {
    fn new(path: PathBuf) -> Self {
        let resolved = fs::canonicalize(&path).expect("fixture operand identity must resolve");
        Self {
            invoked: path,
            resolved,
        }
    }

    fn exact(&self, text: &str) -> bool {
        self.invoked.to_str() == Some(text) || self.resolved.to_str() == Some(text)
    }

    fn same_file(&self, text: &str) -> bool {
        self.exact(text)
            || (Path::new(text).is_absolute()
                && fs::canonicalize(text).is_ok_and(|path| path == self.resolved))
    }
}

struct FixtureIdentity {
    executable: Identity,
    config: Identity,
    private_roots: Vec<PathBuf>,
}

impl FixtureIdentity {
    fn new(root: &Path) -> Self {
        let root = fs::canonicalize(root).expect("fixture root must resolve");
        let mut private_roots = Vec::new();
        for name in [".exitbind", "state", "runs", "artifacts"] {
            let path = root.join(name);
            if let Ok(resolved) = fs::canonicalize(&path) {
                private_roots.push(resolved);
            }
            private_roots.push(path);
        }
        let identity = Self {
            executable: Identity::new(PathBuf::from(env!("CARGO_BIN_EXE_exitbind"))),
            config: Identity::new(root.join("exitbind.json")),
            private_roots,
        };
        for path in [
            &identity.executable.invoked,
            &identity.executable.resolved,
            &identity.config.invoked,
            &identity.config.resolved,
        ] {
            assert!(
                !identity.private(path),
                "invocation operand resolves into fixture private state"
            );
        }
        identity
    }

    fn private(&self, path: &Path) -> bool {
        self.private_roots.iter().any(|root| path.starts_with(root))
    }

    fn private_alias(&self, text: &str) -> bool {
        let path = Path::new(text);
        path.is_absolute() && self.private_ancestor(path, 0)
    }

    fn private_ancestor(&self, path: &Path, depth: usize) -> bool {
        // A loop or a very deep alias is not evidence that a path is public.
        if depth == 16 {
            return true;
        }
        for ancestor in path.ancestors() {
            if self.private(ancestor) {
                return true;
            }
            if let Ok(resolved) = fs::canonicalize(ancestor) {
                return self.private(&resolved);
            }
            if let Ok(target) = fs::read_link(ancestor) {
                let target = if target.is_absolute() {
                    target
                } else {
                    ancestor.parent().unwrap_or(Path::new("/")).join(target)
                };
                if self.private_ancestor(&target, depth + 1) {
                    return true;
                }
            }
        }
        false
    }
}

/// Object keys and array indices must not be interchangeable in route paths.
#[derive(Clone, Copy)]
enum Step<'a> {
    Key(&'a str),
    Index(usize),
}

fn command_array(path: &[Step<'_>]) -> bool {
    use Step::{Index as I, Key as K};
    matches!(
        path,
        [K("effectiveAction"), K("detail"), K("command")]
            | [K("effectiveAction"), K("command"), K("argv")]
            | [K("nextAction"), K("command")]
            | [K("nextAction"), K("command"), K("argv")]
            | [K("continuation"), K("command")]
            | [K("fullCommand")]
            | [K("works" | "unreadable"), I(_), K("command")]
            | [K("warnings"), I(_), K("nextAction"), K("command")]
            | [
                K("next"),
                K("warnings"),
                I(_),
                K("nextAction"),
                K("command")
            ]
            | [
                K("effectiveAction"),
                K("warnings"),
                I(_),
                K("nextAction"),
                K("command")
            ]
            | [
                K("current"),
                K("details"),
                K("grouped" | "assignment" | "evidence" | "recipient" | "tasks"),
                K("command")
            ]
            | [
                K("next"),
                K("current"),
                K("details"),
                K("grouped" | "assignment" | "evidence" | "recipient" | "tasks"),
                K("command")
            ]
            | [K("humanHelp"), K("nextAction"), K("command"), K("argv")]
            | [
                K("residual" | "packet"),
                K("humanHelp"),
                K("nextAction"),
                K("command"),
                K("argv")
            ]
            | [
                K("actionForms" | "effectiveAction"),
                K("beforeEditing" | "mechanicalAction"),
                K("command"),
                K("argv")
            ]
            | [
                K("actionForms" | "effectiveAction"),
                K("choices"),
                I(_),
                K("command"),
                K("argv")
            ]
            | [K("actionForms"), K("fillRoute"), K("argv")]
            | [
                K("current"),
                K("actionForm"),
                K("beforeEditing" | "mechanicalAction"),
                K("command"),
                K("argv")
            ]
            | [
                K("next"),
                K("current"),
                K("actionForm"),
                K("beforeEditing" | "mechanicalAction"),
                K("command"),
                K("argv")
            ]
            | [
                K("current"),
                K("actionForm"),
                K("choices"),
                I(_),
                K("command"),
                K("argv")
            ]
            | [
                K("next"),
                K("current"),
                K("actionForm"),
                K("choices"),
                I(_),
                K("command"),
                K("argv")
            ]
    )
}

fn display(path: &[Step<'_>]) -> String {
    path.iter().fold(String::new(), |mut text, step| {
        text.push('/');
        match step {
            Step::Key(key) => text.push_str(key),
            Step::Index(index) => text.push_str(&index.to_string()),
        }
        text
    })
}

pub(super) fn assert_opaque_envelope(value: &Value, fixture_root: &Path) {
    let identity = FixtureIdentity::new(fixture_root);
    let mutation = value["compact"] == true && value["effect"].is_string();
    let ledger = if mutation {
        value["reference"]["ledger"].as_str()
    } else {
        None
    };
    if let Some(ledger) = ledger {
        assert!(ledger.starts_with(".exitbind/runs/work-") && ledger.ends_with(".jsonl"));
        assert_eq!(value["nextAction"]["safe"], true);
        assert!(matches!(
            value["nextAction"]["type"].as_str(),
            Some("inspect" | "inspect_held")
        ));
    }
    visit(value, &mut Vec::new(), ledger, &identity, false);
}

fn visit<'a>(
    value: &'a Value,
    path: &mut Vec<Step<'a>>,
    ledger: Option<&str>,
    identity: &FixtureIdentity,
    public_operand: bool,
) {
    match value {
        Value::String(text) => {
            let location = display(path);
            assert!(
                !identity.private_alias(text),
                "opaque envelope exposed private path or unresolved alias at {location}"
            );
            let public_identity =
                identity.executable.same_file(text) || identity.config.same_file(text);
            assert!(
                !public_identity || public_operand,
                "opaque envelope misplaced invocation operand at {location}"
            );
            // Retain the pre-existing ledger-selector exception unchanged.
            let allowed_selector = ledger.is_some_and(|ledger| {
                text == ledger
                    && (location == "/reference/ledger"
                        || location.starts_with("/nextAction/command/"))
            });
            if !allowed_selector && !public_operand {
                for marker in [".exitbind", "runs/", "artifacts/", "state/"] {
                    assert!(
                        !text.contains(marker),
                        "opaque envelope leaked {marker} at {location}"
                    );
                }
            }
        }
        Value::Array(items) => {
            let command = command_array(path);
            if command {
                if let Some(executable) = items.first().and_then(Value::as_str) {
                    if Path::new(executable).is_absolute() {
                        assert!(
                            identity.executable.exact(executable),
                            "command executable differs from fixture at {}",
                            display(path)
                        );
                    }
                }
            }
            for (index, item) in items.iter().enumerate() {
                let config_slot = command && index > 0 && items[index - 1] == "--config";
                if config_slot {
                    assert!(
                        index + 1 == items.len()
                            && item
                                .as_str()
                                .is_some_and(|text| identity.config.exact(text)),
                        "command config differs from fixture at {}",
                        display(path)
                    );
                }
                let public = command
                    && item.as_str().is_some_and(|text| {
                        (index == 0 && identity.executable.exact(text))
                            || (config_slot && identity.config.exact(text))
                    });
                path.push(Step::Index(index));
                visit(item, path, ledger, identity, public);
                path.pop();
            }
        }
        Value::Object(object) => {
            for key in [
                "path",
                "root",
                "ledgerPath",
                "profilePath",
                "artifactPathHint",
                "sourcePath",
            ] {
                assert!(
                    !object.contains_key(key),
                    "opaque envelope leaked {key} at {}",
                    display(path)
                );
            }
            if object.get("exact") == Some(&Value::Bool(true)) {
                assert!(matches!(
                    object.get("kind").and_then(Value::as_str),
                    Some(
                        "ledger_history"
                            | "ledger_event"
                            | "check_log"
                            | "evidence"
                            | "stdout"
                            | "stderr"
                    )
                ));
            }
            for (key, child) in object {
                path.push(Step::Key(key));
                visit(child, path, ledger, identity, false);
                path.pop();
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
