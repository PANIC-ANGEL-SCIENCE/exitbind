//! Read-only work packets stay opaque. A bounded mutation result exposes only
//! the exact recorded-event ledger selector in its inspection route.

use serde_json::Value;
use sha2::{Digest, Sha256};
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
    if matches!(path, [K("full"), K("command")]) {
        return true;
    }
    if let [K("currentDelivery"), rest @ ..] = path {
        return !matches!(rest, [K("full"), K("command")])
            && (command_array(rest)
                || matches!(
                    rest,
                    [
                        K("sections"),
                        K("assignment"),
                        K("optionalChallenge"),
                        K("command")
                    ] | [
                        K("sections"),
                        K("assignment"),
                        K("residual"),
                        K("humanHelp"),
                        K("nextAction"),
                        K("command"),
                        K("argv")
                    ]
                ));
    }
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
            | [
                K("actionForms" | "effectiveAction"),
                K("fillRoute"),
                K("argv" | "templatePrefix")
            ]
            | [
                K("current"),
                K("actionForm"),
                K("fillRoute"),
                K("argv" | "templatePrefix")
            ]
            | [
                K("next"),
                K("current"),
                K("actionForm"),
                K("fillRoute"),
                K("argv" | "templatePrefix")
            ]
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
    check_full_detail(value, &identity);
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
    let mut checked = value.clone();
    check_current_delivery(&mut checked, value, fixture_root, &identity);
    visit(&checked, &mut Vec::new(), ledger, &identity, false);
}

// The root full descriptor is a current, read-only detail route. It is not
// an exception for similarly named fields inside opaque metadata.
fn check_full_detail(value: &Value, identity: &FixtureIdentity) {
    let Some(full) = value.get("full") else {
        return;
    };
    let Some(command) = full.get("command") else {
        return;
    };
    let argv = command.as_array().expect("full detail command is an array");
    let work = value["work"].as_str().expect("full detail Work");
    let binding = value["current"]["binding"]
        .as_str()
        .expect("full detail current binding");
    assert_eq!(full["readOnly"], true, "full detail must be read-only");
    assert_eq!(full["binding"], binding, "full detail binding differs");
    assert_eq!(argv.len(), 7, "full detail argv length differs");
    assert!(argv[0]
        .as_str()
        .is_some_and(|text| identity.executable.exact(text)));
    for (actual, expected) in argv[1..6]
        .iter()
        .zip(["work", "detail", work, "--json", "--config"])
    {
        assert_eq!(actual.as_str(), Some(expected), "full detail argv differs");
    }
    assert!(argv[6]
        .as_str()
        .is_some_and(|text| identity.config.exact(text)));
    for flag in ["sameExecutableRequired", "sameConfigRequired"] {
        assert_eq!(
            full.get(flag).or_else(|| value["routeDefaults"].get(flag)),
            Some(&Value::Bool(false)),
            "full detail route must carry its own invocation operands"
        );
    }
}

// The default step can include a complete typed delivery. Compare its current
// instruction and checker identities to a direct current read, then exempt
// only the validated typed content/path slots from the outer opaque scan.
fn check_current_delivery(
    checked: &mut Value,
    original: &Value,
    fixture_root: &Path,
    identity: &FixtureIdentity,
) {
    let Some(delivery) = checked.get_mut("currentDelivery") else {
        return;
    };
    if delivery["complete"] == false {
        assert!(
            delivery["reason"].is_string(),
            "incomplete delivery has no reason"
        );
        return;
    }
    assert_eq!(delivery["kind"], "work_detail");
    assert_eq!(delivery["complete"], true);
    assert_eq!(delivery["work"], original["work"]);
    assert_eq!(delivery["binding"], original["current"]["binding"]);
    assert_eq!(delivery["recipient"], original["current"]["recipient"]);
    assert_eq!(delivery["actionForms"]["binding"], delivery["binding"]);
    assert_eq!(delivery["recipientContext"]["complete"], true);

    let work = delivery["work"].as_str().expect("delivery Work handle");
    let direct = super::support::git_topology::command(&identity.executable.invoked)
        .current_dir(fixture_root)
        .args(["work", "detail", work, "--json", "--config"])
        .arg(&identity.config.invoked)
        .output()
        .expect("read current detail for typed delivery identity");
    assert!(direct.status.success(), "direct current detail: {direct:?}");
    let direct: Value = serde_json::from_slice(&direct.stdout).unwrap();
    assert_eq!(direct["complete"], true);
    assert_eq!(delivery["binding"], direct["binding"]);
    assert_eq!(delivery["recipient"], direct["recipient"]);
    assert_eq!(delivery["transport"], direct["transport"]);
    assert_eq!(delivery["transport"]["encoding"], "utf-8");
    assert_eq!(delivery["transport"]["exact"], true);
    delivery["transport"]
        .as_object_mut()
        .unwrap()
        .remove("exact");
    assert_eq!(
        delivery["sections"]["assignment"]["checkPreparation"],
        direct["sections"]["assignment"]["checkPreparation"],
        "typed checker observation changed"
    );

    let actor = delivery["recipient"]["actor"].clone();
    let recipient = &mut delivery["recipientContext"];
    let expected_recipient = &direct["recipientContext"];
    assert_eq!(recipient["agent"], expected_recipient["agent"]);
    assert_eq!(recipient["role"], expected_recipient["role"]);
    if recipient["available"] == true {
        assert_eq!(recipient["profile"], expected_recipient["profile"]);
        check_instruction_file(&mut recipient["profile"], fixture_root, identity);
        assert_eq!(recipient["rules"], expected_recipient["rules"]);
        for rule in recipient["rules"].as_array_mut().expect("typed rules") {
            check_instruction_file(rule, fixture_root, identity);
        }
    } else {
        assert_eq!(recipient["available"], false);
        assert_eq!(actor, "exitbind");
    }
    for pointer in [
        "/guidance/items",
        "/guidance/overflow",
        "/projectLessons/items",
        "/specialistExperience/experience",
    ] {
        let Some(items) = recipient.pointer_mut(pointer).and_then(Value::as_array_mut) else {
            continue;
        };
        for (index, item) in items.iter_mut().enumerate() {
            if item["content"].is_string() {
                let expected = &expected_recipient.pointer(pointer).unwrap()[index];
                assert_eq!(item["content"], expected["content"]);
                assert_eq!(item["reference"]["itemId"], expected["reference"]["itemId"]);
                assert_eq!(
                    item["reference"]["sourceSha256"],
                    expected["reference"]["sourceSha256"]
                );
                item["content"] = Value::Null;
            }
        }
    }

    let checker = &mut delivery["sections"]["assignment"]["checkPreparation"];
    if let Some(object) = checker.as_object_mut() {
        if let Some(directory) = object
            .remove("workingDirectory")
            .and_then(|directory| directory.as_str().map(str::to_owned))
        {
            assert_eq!(
                fs::canonicalize(directory).unwrap(),
                fs::canonicalize(fixture_root).unwrap(),
                "checker working directory differs from the fixture"
            );
        }
        if let Some(path) = object
            .remove("path")
            .and_then(|path| path.as_str().map(str::to_owned))
        {
            assert!(
                !identity.private_alias(&path),
                "checker path names private state"
            );
        }
        if let Some(input) = object.get_mut("input").and_then(Value::as_object_mut) {
            if let Some(path) = input
                .remove("path")
                .and_then(|path| path.as_str().map(str::to_owned))
            {
                assert!(
                    !identity.private_alias(&path),
                    "checker input names private state"
                );
            }
        }
    }
    let evidence = &mut delivery["sections"]["evidence"];
    let expected_evidence = &direct["sections"]["evidence"];
    for group in ["items", "checkEvidence"] {
        let Some(items) = evidence[group].as_array_mut() else {
            continue;
        };
        for (index, item) in items.iter_mut().enumerate() {
            for field in ["artifact", "stdout", "stderr"] {
                let Some(artifact) = item.get_mut(field).filter(|artifact| artifact.is_object())
                else {
                    continue;
                };
                assert_eq!(*artifact, expected_evidence[group][index][field]);
                if artifact["exact"] == true {
                    assert_eq!(artifact["verified"], true);
                    artifact.as_object_mut().unwrap().remove("exact");
                }
                if artifact["content"].is_string() {
                    let content = artifact["content"].as_str().unwrap().as_bytes();
                    assert_eq!(artifact["bytes"].as_u64(), Some(content.len() as u64));
                    assert_eq!(artifact["sha256"], format!("{:x}", Sha256::digest(content)));
                    artifact["content"] = Value::Null;
                }
            }
        }
    }
}

fn check_instruction_file(value: &mut Value, fixture_root: &Path, identity: &FixtureIdentity) {
    assert_eq!(value["complete"], true);
    let path = value["path"].as_str().expect("typed instruction path");
    let resolved = fixture_root
        .join(path)
        .canonicalize()
        .expect("instruction path resolves");
    assert!(
        !identity.private(&resolved),
        "instruction path names private state"
    );
    let content = fs::read(&resolved).expect("typed instruction bytes");
    assert_eq!(value["content"].as_str().unwrap().as_bytes(), content);
    assert_eq!(value["bytes"].as_u64(), Some(content.len() as u64));
    assert_eq!(value["sha256"], format!("{:x}", Sha256::digest(&content)));
    value.as_object_mut().unwrap().remove("path");
    value["content"] = Value::Null;
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
