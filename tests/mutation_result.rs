//! Exact, read-only result lookup for bounded work mutations.

#![cfg(unix)]

mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, path::PathBuf, process::Output};

struct Fixture {
    root: support::TempRoot,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("mutation-result");
        support::git_topology::repository(&root);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        Self { root, config }
    }
    fn call(&self, args: &[&str]) -> (Value, Output) {
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        let value = serde_json::from_slice(&output.stdout).unwrap();
        (value, output)
    }
    fn recorded_failure(&self, args: &[&str]) -> (Value, Output) {
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}: {output:?}");
        let value = serde_json::from_slice(&output.stdout).unwrap();
        (value, output)
    }
    fn ledger(&self, work: &str) -> String {
        format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.root.with_extension("pass"));
        let _ = fs::remove_file(self.root.with_extension("count"));
    }
}

// This is a deliberately narrow shape check for this fixture, not a reader
// for arbitrary actor projections. Unknown fields stay in the 8 KiB check.
fn known_fixture_actor(value: &Value, path: &str) -> bool {
    match value {
        Value::Object(object) => {
            let allowed = match path {
                "" => "assignment binding checkPreparation complete context contextIdentity equalContextAliases evidence instructions kind memoryQuery outcomes reason recipient requiredContentComplete residual tasks work",
                "/assignment" => "basisSha256 declaredBoundary displayName goal guidanceConfigSha256 guidanceContinuityProtocol profile profileSha256 purpose reviewDecisionSha256 runtime upstreamArtifacts upstreamArtifactsImmutable",
                "/assignment/declaredBoundary" => "commands crossContext memoryExpire memoryForget memoryPromote memoryRead memoryReject memoryReview memoryRevoke memoryWrite observe retention skills write",
                "/assignment/profile" => "sha256",
                "/assignment/runtime" => "fallback host model reasoningEffort",
                "/assignment/upstreamArtifacts/*" => "agent attempt attemptStatus reason role sha256 stage",
                "/checkPreparation" => "detail input program state",
                "/checkPreparation/input" => "reason state",
                "/context" => "loop obligations run scope subject version",
                "/context/loop" => "afterReplan budget defaults enabled grantProtocol noInformationStreak postReplanSpent replanCount spent state",
                "/context/loop/defaults" => "budget grantProtocol noInformationLimit postReplanLimit replanBinding version",
                "/context/obligations/*" => "acquisition agent checkEvidenceId checker checkerSha256 freshness kind logStatus origin requirementId requirementText state status strength targetEvidenceId",
                "/context/run" => "id workflow",
                "/context/scope" => "attempt basisSha256 configSha256 currentStage freshness owner planSha256 reviewDecisionSha256 source",
                "/context/subject" => "previousSubjectSha256 transitionSha256 version",
                "/contextIdentity" => "assignmentContentSha256 evidenceContentSha256 recipientSha256 tasksSha256",
                "/evidence" => "checkEvidence complete earlyEvidence historical items preReturn",
                "/evidence/checkEvidence/*" => "acquisition kind result status",
                "/evidence/checkEvidence/*/result" | "/evidence/items/*/result" => "code kind",
                "/evidence/earlyEvidence" => "authority selected state",
                "/evidence/items/*" => "acquisition artifact checker checkerSha256 evidenceId freshness historical kind logStatus outcome result status stderr stdout strength targetEvidenceId",
                "/evidence/items/*/artifact" | "/evidence/items/*/stdout" | "/evidence/items/*/stderr" => "bytes complete content encoding exact kind readable sha256 verified",
                "/evidence/preReturn" => "attempt enabled policy",
                "/instructions" => "authority displayName meaning profile recipientSha256 reuseIfPreviouslyRead rules",
                "/instructions/profile" | "/instructions/rules/*" => "bytes complete content sha256",
                "/recipient" => "actor agent assignment nativeName role",
                "/residual" => "alreadyEstablished basis context doNotRepeat humanHelp invalidation reviewPolicy snapshot stillValid version",
                "/residual/alreadyEstablished/*" => "fact status",
                "/residual/context" => "digest next",
                "/residual/context/next" => "outcomes",
                "/residual/humanHelp" => "nextAction ownerDecision whatHappened whatRemains",
                "/residual/humanHelp/nextAction" => "actor command summary",
                "/residual/humanHelp/nextAction/command" => "args argv program",
                "/residual/humanHelp/whatRemains/*" => "obligation requirementId",
                "/residual/invalidation" => "basisChangeInvalidates checkerChangeInvalidates checkerSha256 configChangeInvalidates inputCoverage owner rule sessionRestartInvalidates source subjectChangeInvalidates testedInputChangeInvalidates",
                "/residual/snapshot" => "inputsSha256",
                "/residual/stillValid/*" => "checkEventSha256 evidence inputs requirementId status subject",
                "/tasks" => "decomposition goal overall resultReadiness systemText tasks",
                "/tasks/decomposition" => "available reason",
                "/tasks/resultReadiness" => "applicable detail reason state",
                _ => return false,
            };
            object.iter().all(|(key, child)| {
                allowed
                    .split_ascii_whitespace()
                    .any(|name| name == key.as_str())
                    && known_fixture_actor(child, &format!("{path}/{key}"))
            })
        }
        Value::Array(items) => items
            .iter()
            .all(|item| known_fixture_actor(item, &format!("{path}/*"))),
        _ => true,
    }
}

fn matching_fields(projected: &Value, full: &Value) -> bool {
    projected.as_object().is_some_and(|fields| {
        fields
            .iter()
            .all(|(key, value)| full.get(key) == Some(value))
    })
}

fn verified_body(body: &Value, full: &Value) -> bool {
    let Some(content) = body["content"].as_str() else {
        return false;
    };
    body["complete"] == true
        && ["content", "sha256", "bytes", "complete"]
            .iter()
            .all(|key| body[*key] == full[*key])
        && body["bytes"].as_u64() == Some(content.len() as u64)
        && body["sha256"] == format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn verified_fixture_actor(
    response: &Value,
    full: &Value,
    read_recipients: &HashSet<String>,
) -> bool {
    let actor = &response["currentDelivery"];
    if actor["kind"] != "work_actor_context"
        || actor["complete"] != false
        || !known_fixture_actor(actor, "")
        || actor["work"] != response["work"]
        || actor["work"] != full["work"]
        || actor["binding"] != response["current"]["binding"]
        || actor["binding"] != full["binding"]
        || actor["recipient"] != response["current"]["recipient"]
        || actor["recipient"] != full["recipient"]
        || actor["contextIdentity"] != full["contextIdentity"]
        || full["kind"] != "work_detail"
        || full["complete"] != true
        || full["recipientContext"]["complete"] != true
        || full["sections"]["evidence"]["complete"] != true
        || actor["evidence"]["complete"] != true
    {
        return false;
    }
    for field in [
        "recipientSha256",
        "assignmentContentSha256",
        "evidenceContentSha256",
        "tasksSha256",
    ] {
        if !actor["contextIdentity"][field]
            .as_str()
            .is_some_and(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
        {
            return false;
        }
    }
    let instructions = &actor["instructions"];
    if actor["requiredContentComplete"] == true {
        if !instructions.as_object().is_some_and(|fields| {
            fields.len() == 4
                && ["authority", "displayName", "profile", "rules"]
                    .iter().all(|key| fields.contains_key(*key))
        }) {
            return false;
        }
        if actor["reason"] != "current actor instructions and evidence are included; original full detail remains available" {
            return false;
        }
        let (Some(rules), Some(full_rules)) = (instructions["rules"].as_array(), full["recipientContext"]["rules"].as_array()) else {
            return false;
        };
        if !verified_body(&instructions["profile"], &full["recipientContext"]["profile"])
            || instructions["authority"] != full["recipientContext"]["authority"]
            || instructions["displayName"] != full["recipientContext"]["displayName"]
            || rules.len() != full_rules.len()
            || !rules.iter().zip(full_rules).all(|(rule, original)| verified_body(rule, original))
        {
            return false;
        }
    } else if actor["requiredContentComplete"] != false
        || instructions["reuseIfPreviouslyRead"] != true
        || !instructions.as_object().is_some_and(|fields| {
            fields.len() == 3
                && ["meaning", "recipientSha256", "reuseIfPreviouslyRead"]
                    .iter().all(|key| fields.contains_key(*key))
        })
        || actor["reason"] != "assignment and evidence are included; compare the recipient identity or read full detail"
        || instructions["meaning"] != "compare this recipient identity with the complete instructions already read; otherwise use full detail"
        || instructions["recipientSha256"] != actor["contextIdentity"]["recipientSha256"]
        || !instructions["recipientSha256"].as_str().is_some_and(|sha| read_recipients.contains(sha))
    {
        return false;
    }
    let section = &full["sections"]["assignment"];
    let assignment = &section["assignment"];
    if !matching_fields(&actor["assignment"], assignment)
        || ["goal", "declaredBoundary", "purpose", "upstreamArtifacts"]
            .iter()
            .any(|key| actor["assignment"].get(*key).is_none())
        || actor["memoryQuery"] != section["memoryQuery"]
        || actor["outcomes"] != section["outcomes"]
    {
        return false;
    }
    let context = &assignment["context"];
    for key in ["loop", "run", "scope", "subject"] {
        if !actor["context"]
            .get(key)
            .is_some_and(|value| matching_fields(value, &context[key]))
        {
            return false;
        }
    }
    if actor["context"]["version"] != context["version"] {
        return false;
    }
    let (Some(obligations), Some(full_obligations)) = (
        actor["context"]["obligations"].as_array(),
        context["obligations"].as_array(),
    ) else {
        return false;
    };
    if obligations.len() != full_obligations.len()
        || !obligations
            .iter()
            .zip(full_obligations)
            .all(|(item, original)| {
                item.as_object().is_some_and(|fields| {
                    fields.iter().all(|(key, value)| {
                        let source = match key.as_str() {
                            "checkEvidenceId" => "checkEventSha256",
                            "targetEvidenceId" => "targetEventSha256",
                            other => other,
                        };
                        original.get(source) == Some(value)
                    })
                })
            })
    {
        return false;
    }
    let residual = &actor["residual"];
    let original = &section["residual"];
    if [
        "alreadyEstablished",
        "basis",
        "doNotRepeat",
        "humanHelp",
        "invalidation",
        "reviewPolicy",
        "snapshot",
        "stillValid",
        "version",
    ]
    .iter()
    .any(|key| residual.get(*key).is_none())
    {
        return false;
    }
    let Some(fields) = residual.as_object() else {
        return false;
    };
    for (key, value) in fields {
        let same = match key.as_str() {
            "context" => value.as_object().is_some_and(|context| {
                context.iter().all(|(key, value)| {
                    if key == "next" {
                        matching_fields(value, &original["context"]["next"])
                    } else {
                        original["context"].get(key) == Some(value)
                    }
                })
            }),
            "humanHelp" | "invalidation" | "snapshot" => matching_fields(value, &original[key]),
            _ => original.get(key) == Some(value),
        };
        if !same {
            return false;
        }
    }
    let Some(aliases) = actor["equalContextAliases"].as_array() else {
        return false;
    };
    if !aliases.iter().all(|alias| {
        alias.as_str().is_some_and(|key| {
            context.get(key).is_some() && context.get(key) == original["context"].get(key)
        })
    }) {
        return false;
    }
    let preparation = &actor["checkPreparation"];
    let full_preparation = &section["checkPreparation"];
    if !preparation.as_object().is_some_and(|fields| {
        fields.iter().all(|(key, value)| {
            if key == "input" {
                matching_fields(value, &full_preparation["input"])
            } else {
                full_preparation.get(key) == Some(value)
            }
        })
    }) || actor
        .get("tasks")
        .is_some_and(|tasks| tasks != &full["sections"]["tasks"])
    {
        return false;
    }
    let evidence = &actor["evidence"];
    let full_evidence = &full["sections"]["evidence"];
    if !evidence.as_object().is_some_and(|fields| {
        fields
            .iter()
            .all(|(key, value)| key == "items" || full_evidence.get(key) == Some(value))
    }) {
        return false;
    }
    let (Some(items), Some(full_items)) = (
        evidence["items"].as_array(),
        full_evidence["items"].as_array(),
    ) else {
        return false;
    };
    if items.len() != full_items.len() {
        return false;
    }
    for item in items {
        let Some(id) = item["evidenceId"].as_str() else {
            return false;
        };
        let matches = full_items
            .iter()
            .filter(|candidate| candidate["eventSha256"] == id)
            .collect::<Vec<_>>();
        let [original] = matches.as_slice() else {
            return false;
        };
        let obligation = full_obligations
            .iter()
            .find(|candidate| candidate["checkEventSha256"] == id);
        let Some(fields) = item.as_object() else {
            return false;
        };
        for (key, value) in fields {
            if key == "evidenceId" {
                continue;
            }
            if matches!(key.as_str(), "artifact" | "stdout" | "stderr") {
                let expected_kind = if key == "artifact" {
                    "evidence"
                } else {
                    key.as_str()
                };
                if value["kind"] != expected_kind
                    || !verified_body(value, &original[key.as_str()])
                    || ["encoding", "exact", "readable", "verified"]
                        .iter()
                        .any(|field| value[*field] != original[key.as_str()][*field])
                {
                    return false;
                }
            } else if original.get(key) != Some(value) {
                let source = if key == "targetEvidenceId" {
                    "targetEventSha256"
                } else {
                    key.as_str()
                };
                if obligation.and_then(|entry| entry.get(source)) != Some(value) {
                    return false;
                }
            }
        }
    }
    true
}

// Normalize only complete, fixture-bound routes. Unknown data stays in the
// measurement, while an unrecognized or mismatched route is rejected.
fn normalized_fixture_metadata(
    fixture: &Fixture,
    value: &Value,
    full: &Value,
    work: &str,
    seen: &HashSet<String>,
) -> Option<Value> {
    let exe = env!("CARGO_BIN_EXE_exitbind");
    let config = fixture.config.to_str()?;
    let binding = full["binding"].as_str()?;
    let assignment = full["recipient"]["assignment"].as_str();
    let event =
        full["sections"]["assignment"]["residual"]["snapshot"]["headEventSha256"].as_str()?;
    if full["kind"] != "work_detail"
        || full["complete"] != true
        || full["valid"] != true
        || full["work"] != work
        || value["work"] != work
        || value["current"]["work"] != work
        || value["current"]["binding"] != binding
        || value["current"]["recipient"] != full["recipient"]
        || value["eventSha256"] != event
    {
        return None;
    }
    fn route_flags(route: &Value) -> bool {
        ["sameExecutableRequired", "sameConfigRequired"]
            .iter()
            .all(|key| route.get(*key).map_or(true, |flag| flag == false))
    }
    fn route(argv: &mut Value, exe: &str, config: &str, tail: &[&str]) -> Option<()> {
        let args = argv.as_array_mut()?;
        let expected: Vec<&str> = std::iter::once(exe)
            .chain(tail.iter().copied())
            .chain(["--config", config])
            .collect();
        if args.len() != expected.len()
            || !args
                .iter()
                .zip(&expected)
                .all(|(arg, want)| arg.as_str() == Some(*want))
        {
            return None;
        }
        args[0] = Value::String("/fixture/bin/exitbind".into());
        let last = args.len() - 1;
        args[last] = Value::String("/fixture/project/exitbind.json".into());
        Some(())
    }
    let mut metadata = value.clone();
    if value
        .get("currentDelivery")
        .is_some_and(|actor| !actor.is_null())
    {
        if value["currentDelivery"]["kind"] != "work_actor_context"
            || !verified_fixture_actor(value, full, seen)
        {
            return None;
        }
        metadata.as_object_mut()?.remove("currentDelivery");
    }
    if metadata["full"]["binding"] != binding
        || metadata["full"]["readOnly"] != true
        || !route_flags(&metadata["full"])
        || metadata["continuation"]["readOnly"] != true
        || !route_flags(&metadata["continuation"])
        || metadata["nextAction"]["safe"] != true
        || metadata["nextAction"]["type"] != "inspect"
        || !route_flags(&metadata["nextAction"])
    {
        return None;
    }
    route(
        &mut metadata["full"]["command"],
        exe,
        config,
        &["work", "detail", work, "--json"],
    )?;
    route(
        &mut metadata["continuation"]["command"],
        exe,
        config,
        &["work", "next", work],
    )?;
    let ledger = fixture.ledger(work);
    route(
        &mut metadata["nextAction"]["command"],
        exe,
        config,
        &["run", "inspect", &ledger, "--event", event, "--json"],
    )?;

    let action = &mut metadata["effectiveAction"];
    if action["binding"] != binding
        || (!action["work"].is_null() && action["work"] != work)
        || (!action["assignment"].is_null() && action["assignment"].as_str() != assignment)
        || (!action["actor"].is_null() && action["actor"] != full["recipient"]["actor"])
    {
        return None;
    }
    if !action["beforeEditing"].is_null() {
        if !route_flags(&action["beforeEditing"]["command"]) {
            return None;
        }
        let request = format!("permit-{binding}");
        route(
            &mut action["beforeEditing"]["command"]["argv"],
            exe,
            config,
            &[
                "work",
                "permit",
                work,
                assignment?,
                "--operation",
                "<OPERATION>",
                "--request-id",
                &request,
            ],
        )?;
    }
    if !action["command"].is_null() {
        if !route_flags(&action["command"]) {
            return None;
        }
        route(
            &mut action["command"]["argv"],
            exe,
            config,
            &["work", "check", work],
        )?;
    }
    let mut labels = HashSet::new();
    let choices = match action.get_mut("choices") {
        None | Some(Value::Null) => &mut [] as &mut [Value],
        Some(Value::Array(choices)) => choices.as_mut_slice(),
        _ => return None,
    };
    for choice in choices {
        let label = choice["label"].as_str()?.to_owned();
        if !labels.insert(label.clone()) || !route_flags(&choice["command"]) {
            return None;
        }
        let expected = match label.as_str() {
            "return" => vec![
                "work",
                "return",
                work,
                assignment?,
                "--current-binding",
                binding,
                "--outcome",
                "<OUTCOME>",
                "--reason",
                "<REASON>",
            ],
            "environmental" => vec![
                "work",
                "disposition",
                work,
                assignment?,
                "--current-binding",
                binding,
                "--decision",
                "environmental",
                "--reason",
                "<REASON>",
            ],
            "approved" | "rework" | "blocked" | "unavailable"
                if full["recipient"]["actor"] == "reviewer" =>
            {
                vec![
                    "work",
                    "return",
                    work,
                    assignment?,
                    "--current-binding",
                    binding,
                    "--outcome",
                    label.as_str(),
                    "--reason",
                    "<REASON>",
                ]
            }
            "preview replacement of an explicitly defective frozen check" => vec![
                "work",
                "replace-check",
                work,
                "--check-command",
                "<NEW_CHECK_COMMAND>",
                "--reason",
                "<REASON>",
                "--classification",
                "defective-contract",
                "--current-binding",
                binding,
            ],
            _ => return None,
        };
        route(&mut choice["command"]["argv"], exe, config, &expected)?;
    }
    Some(metadata)
}

fn emitted_read(fixture: &Fixture, argv: &Value) -> Value {
    let argv = argv.as_array().unwrap();
    let output = support::git_topology::command(argv[0].as_str().unwrap())
        .current_dir(&fixture.root)
        .args(argv[1..].iter().map(|arg| arg.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_fixture_step_budget(
    fixture: &Fixture,
    work: &str,
    value: &Value,
    output: &Output,
    seen: &mut HashSet<String>,
) {
    assert!(output.stdout.len() <= 256 * 1024);
    let mut exact_json = serde_json::to_vec(value).unwrap();
    exact_json.push(b'\n');
    assert_eq!(
        output.stdout, exact_json,
        "fixture byte accounting requires the actual JSON bytes"
    );
    let (full, _) = fixture.call(&["work", "detail", work, "--json"]);
    let mut actor_bytes = 0;
    if value["currentDelivery"]["kind"] == "work_actor_context" {
        assert!(
            verified_fixture_actor(value, &full, seen),
            "current actor body was not independently verified"
        );
        if value["currentDelivery"]["requiredContentComplete"] == true {
            seen.insert(
                value["currentDelivery"]["contextIdentity"]["recipientSha256"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        actor_bytes = serde_json::to_vec(&value["currentDelivery"]).unwrap().len();
    }
    let mut raw_metadata = value.clone();
    if actor_bytes > 0 {
        raw_metadata
            .as_object_mut()
            .unwrap()
            .remove("currentDelivery");
    }
    let actual_metadata = serde_json::to_vec(&raw_metadata).unwrap().len() + 1;
    let metadata = normalized_fixture_metadata(fixture, value, &full, work, seen)
        .expect("fixture route must match independently read current Work");
    let ledger = fixture.root.join(fixture.ledger(work));
    let ledger_before = fs::read(&ledger).unwrap();
    let count = fixture.root.with_extension("count");
    let count_before = fs::read(&count).ok();
    let emitted_full = emitted_read(fixture, &value["full"]["command"]);
    assert_eq!(emitted_full["work"], work);
    assert_eq!(emitted_full["binding"], full["binding"]);
    assert_eq!(emitted_full["complete"], true);
    assert_eq!(emitted_full["valid"], true);
    let inspected = emitted_read(fixture, &value["nextAction"]["command"]);
    assert_eq!(inspected["eventSha256"], value["eventSha256"]);
    assert_eq!(inspected["authorizesMutation"], false);
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    assert_eq!(fs::read(&count).ok(), count_before);
    let metadata_bytes = serde_json::to_vec(&metadata).unwrap().len() + 1;
    assert!(
        metadata_bytes <= 8 * 1024,
        "metadata={metadata_bytes}, total={}, actor={actor_bytes}",
        output.stdout.len()
    );
    eprintln!("step_total_bytes={} verified_actor_value_bytes={actor_bytes} actual_metadata_bytes={actual_metadata} normalized_metadata_bytes={metadata_bytes}", output.stdout.len());
}

#[test]
fn recorded_result_survives_later_head_and_inspection_never_rechecks() {
    let fixture = Fixture::new();
    let marker = fixture.root.with_extension("pass");
    let count = fixture.root.with_extension("count");
    let checker = format!(
        "printf x >> '{}'; if test -f '{}'; then exit 0; else exit 1; fi",
        count.display(),
        marker.display()
    );
    let (started, _) = fixture.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "exact result lookup",
        "--check-command",
        &checker,
    ]);
    let work = started["work"].as_str().unwrap();
    let (initial_full, _) = fixture.call(&["work", "detail", work, "--json"]);
    let mut read_recipients = HashSet::new();
    assert!(verified_fixture_actor(
        &started,
        &initial_full,
        &read_recipients
    ));
    read_recipients.insert(
        started["currentDelivery"]["contextIdentity"]["recipientSha256"]
            .as_str()
            .unwrap()
            .to_owned(),
    );
    let (scope, scope_output) = fixture.call(&[
        "work",
        "return",
        work,
        started["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
        "--json",
    ]);
    assert_fixture_step_budget(&fixture, work, &scope, &scope_output, &mut read_recipients);
    let (scope_full, _) = fixture.call(&["work", "detail", work, "--json"]);
    assert!(verified_fixture_actor(
        &scope,
        &scope_full,
        &read_recipients
    ));
    for pointer in [
        "/full/command/0", // executable
        "/full/command/6", // configuration
        "/full/command/3", // Work
        "/full/binding",
        "/nextAction/command/5",                         // recorded event
        "/effectiveAction/choices/0/command/argv/4",     // assignment
        "/effectiveAction/choices/0/command/argv/6",     // binding
        "/effectiveAction/beforeEditing/command/argv/8", // bound request
        "/effectiveAction/choices/0/label",              // unknown route
    ] {
        let mut forged = scope.clone();
        *forged.pointer_mut(pointer).unwrap() = Value::String("forged".into());
        assert!(
            normalized_fixture_metadata(&fixture, &forged, &scope_full, work, &read_recipients)
                .is_none(),
            "accepted {pointer}"
        );
        eprintln!("normalizer_rejected={pointer}");
    }
    let mut forged_event = scope.clone();
    forged_event["eventSha256"] = Value::String("forged".into());
    forged_event["nextAction"]["command"][5] = Value::String("forged".into());
    assert!(normalized_fixture_metadata(
        &fixture,
        &forged_event,
        &scope_full,
        work,
        &read_recipients
    )
    .is_none());
    eprintln!("normalizer_rejected=forged_event_and_route_against_independent_head");
    for position in [0, 1] {
        let mut duplicate = scope.clone();
        let argv = duplicate["full"]["command"].as_array_mut().unwrap();
        if position == 0 {
            argv.insert(0, Value::String(env!("CARGO_BIN_EXE_exitbind").into()));
        } else {
            argv.push(Value::String("--config".into()));
            argv.push(Value::String(fixture.config.to_str().unwrap().into()));
        }
        assert!(normalized_fixture_metadata(
            &fixture,
            &duplicate,
            &scope_full,
            work,
            &read_recipients
        )
        .is_none());
        eprintln!("normalizer_rejected=duplicate_executable_or_config_{position}");
    }
    for (pointer, values) in [
        ("/full/command", vec![work.to_owned()]),
        (
            "/effectiveAction/choices/0/command/argv",
            vec![
                "--current-binding".into(),
                scope_full["binding"].as_str().unwrap().into(),
            ],
        ),
        (
            "/nextAction/command",
            vec![
                "--event".into(),
                scope["eventSha256"].as_str().unwrap().into(),
            ],
        ),
    ] {
        let mut duplicate = scope.clone();
        duplicate
            .pointer_mut(pointer)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .extend(values.into_iter().map(Value::String));
        assert!(normalized_fixture_metadata(
            &fixture,
            &duplicate,
            &scope_full,
            work,
            &read_recipients
        )
        .is_none());
        eprintln!("normalizer_rejected=duplicate_{pointer}");
    }
    let mut duplicate_choice = scope.clone();
    let choices = duplicate_choice["effectiveAction"]["choices"]
        .as_array_mut()
        .unwrap();
    choices.push(choices[0].clone());
    assert!(normalized_fixture_metadata(
        &fixture,
        &duplicate_choice,
        &scope_full,
        work,
        &read_recipients
    )
    .is_none());
    eprintln!("normalizer_rejected=duplicate_choice");
    let mut unknown_actor = scope.clone();
    unknown_actor["currentDelivery"]["kind"] = Value::String("unknown_actor".into());
    assert!(normalized_fixture_metadata(
        &fixture,
        &unknown_actor,
        &scope_full,
        work,
        &read_recipients
    )
    .is_none());
    eprintln!("normalizer_rejected=unknown_actor");
    let mut malformed_route = scope.clone();
    malformed_route["full"]["command"] = Value::String("not argv".into());
    assert!(normalized_fixture_metadata(
        &fixture,
        &malformed_route,
        &scope_full,
        work,
        &read_recipients
    )
    .is_none());
    eprintln!("normalizer_rejected=malformed_route");
    let mut unsafe_route = scope.clone();
    unsafe_route["full"]["readOnly"] = Value::Bool(false);
    assert!(normalized_fixture_metadata(
        &fixture,
        &unsafe_route,
        &scope_full,
        work,
        &read_recipients
    )
    .is_none());
    eprintln!("normalizer_rejected=non_readonly_route");
    for pointer in [
        "/kind",
        "/work",
        "/binding",
        "/recipient/actor",
        "/contextIdentity/recipientSha256",
        "/instructions/profile/content",
        "/assignment/goal",
        "/assignment/declaredBoundary/crossContext",
        "/context/scope/configSha256",
        "/residual/invalidation/rule",
        "/evidence/items/0/artifact/content",
    ] {
        let mut forged = scope.clone();
        *forged["currentDelivery"].pointer_mut(pointer).unwrap() = Value::String("forged".into());
        assert!(
            !verified_fixture_actor(&forged, &scope_full, &read_recipients),
            "accepted forged {pointer}"
        );
        assert!(normalized_fixture_metadata(
            &fixture,
            &forged,
            &scope_full,
            work,
            &read_recipients
        )
        .is_none());
        eprintln!("normalizer_rejected=forged_actor{pointer}");
    }
    for pointer in ["/unknown", "/instructions/profile/unknown"] {
        let mut forged = scope.clone();
        let parent = pointer.rsplit_once('/').unwrap();
        let target = forged["currentDelivery"].pointer_mut(parent.0).unwrap();
        target
            .as_object_mut()
            .unwrap()
            .insert(parent.1.into(), Value::String("x".repeat(8192)));
        assert!(
            !verified_fixture_actor(&forged, &scope_full, &read_recipients),
            "accepted unknown {pointer}"
        );
        assert!(normalized_fixture_metadata(
            &fixture,
            &forged,
            &scope_full,
            work,
            &read_recipients
        )
        .is_none());
        eprintln!("normalizer_rejected=unknown_actor_field{pointer}");
    }
    let mut oversized_metadata = scope.clone();
    let unknown = format!("{}{}", fixture.config.display(), "x".repeat(8192));
    oversized_metadata["unrecognizedMetadata"] = Value::String(unknown.clone());
    assert!(verified_fixture_actor(
        &oversized_metadata,
        &scope_full,
        &read_recipients
    ));
    let oversized_metadata = normalized_fixture_metadata(
        &fixture,
        &oversized_metadata,
        &scope_full,
        work,
        &read_recipients,
    )
    .unwrap();
    assert_eq!(oversized_metadata["unrecognizedMetadata"], unknown);
    assert!(serde_json::to_vec(&oversized_metadata).unwrap().len() + 1 > 8192);
    eprintln!("normalizer_preserved_and_counted=unknown_metadata");
    eprintln!("scope_return_bytes={}", scope_output.stdout.len());
    assert_eq!(scope["effect"], "recorded");
    assert_eq!(scope["outcome"], "scoped");
    assert!(scope["eventSha256"].as_str().is_some());
    let (worker, _) = fixture.call(&["work", "next", work]);
    let (returned, return_output) = fixture.call(&[
        "work",
        "return",
        work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    assert_fixture_step_budget(
        &fixture,
        work,
        &returned,
        &return_output,
        &mut read_recipients,
    );
    eprintln!("worker_return_bytes={}", return_output.stdout.len());
    assert_eq!(returned["outcome"], "completed");
    assert_eq!(returned["assignment"], worker["next"]["assignment"]);
    let (failed, failed_output) = fixture.recorded_failure(&["work", "check", work]);
    assert_fixture_step_budget(
        &fixture,
        work,
        &failed,
        &failed_output,
        &mut read_recipients,
    );
    eprintln!("failed_check_bytes={}", failed_output.stdout.len());
    assert_eq!(failed["effect"], "recorded");
    assert_eq!(failed["result"]["code"], 1);
    let first_sha = failed["eventSha256"].as_str().unwrap();
    let ledger = fixture.ledger(work);
    fs::write(&marker, b"pass").unwrap();
    let (passed, passed_output) = fixture.call(&["work", "check", work, "--json"]);
    assert_fixture_step_budget(
        &fixture,
        work,
        &passed,
        &passed_output,
        &mut read_recipients,
    );
    eprintln!("passed_check_bytes={}", passed_output.stdout.len());
    assert_eq!(passed["result"]["code"], 0);
    assert_ne!(passed["eventSha256"], failed["eventSha256"]);
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let (historical, _) =
        fixture.call(&["run", "inspect", &ledger, "--event", first_sha, "--json"]);
    let (again, _) = fixture.call(&["run", "inspect", &ledger, "--event", first_sha, "--json"]);
    assert_eq!(historical, again);
    assert_eq!(historical["eventSha256"], first_sha);
    assert_eq!(historical["event"]["result"]["code"], 1);
    assert_eq!(historical["historical"], true);
    assert_eq!(historical["authorizesMutation"], false);
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
    assert_eq!(fs::read(&count).unwrap(), b"xx");
}

#[test]
fn return_help_advertises_json_and_unknown_options_fail_directly() {
    let fixture = Fixture::new();
    let help = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["work", "return", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success(), "{help:?}");
    assert!(String::from_utf8_lossy(&help.stdout).contains("[--json]"));
    let invalid = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["work", "return", "--unrecognized", "--config"])
        .arg(&fixture.config)
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("--unrecognized"));
}
