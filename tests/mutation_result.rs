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

fn assert_fixture_step_budget(
    fixture: &Fixture,
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
    let mut metadata = value.clone();
    let mut actor_bytes = 0;
    if value["currentDelivery"]["kind"] == "work_actor_context" {
        let work = value["work"].as_str().unwrap();
        let (full, _) = fixture.call(&["work", "detail", work, "--json"]);
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
        metadata.as_object_mut().unwrap().remove("currentDelivery");
    }
    let metadata_bytes = serde_json::to_vec(&metadata).unwrap().len() + 1;
    assert!(
        metadata_bytes <= 8 * 1024,
        "metadata={metadata_bytes}, total={}, actor={actor_bytes}",
        output.stdout.len()
    );
    eprintln!("step_total_bytes={} verified_actor_value_bytes={actor_bytes} metadata_bytes={metadata_bytes}", output.stdout.len());
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
    assert_fixture_step_budget(&fixture, &scope, &scope_output, &mut read_recipients);
    let (scope_full, _) = fixture.call(&["work", "detail", work, "--json"]);
    assert!(verified_fixture_actor(
        &scope,
        &scope_full,
        &read_recipients
    ));
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
    }
    let mut oversized_metadata = scope.clone();
    oversized_metadata["unrecognizedMetadata"] = Value::String("x".repeat(8192));
    assert!(verified_fixture_actor(
        &oversized_metadata,
        &scope_full,
        &read_recipients
    ));
    oversized_metadata
        .as_object_mut()
        .unwrap()
        .remove("currentDelivery");
    assert!(serde_json::to_vec(&oversized_metadata).unwrap().len() + 1 > 8192);
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
    assert_fixture_step_budget(&fixture, &returned, &return_output, &mut read_recipients);
    eprintln!("worker_return_bytes={}", return_output.stdout.len());
    assert_eq!(returned["outcome"], "completed");
    assert_eq!(returned["assignment"], worker["next"]["assignment"]);
    let (failed, failed_output) = fixture.recorded_failure(&["work", "check", work]);
    assert_fixture_step_budget(&fixture, &failed, &failed_output, &mut read_recipients);
    eprintln!("failed_check_bytes={}", failed_output.stdout.len());
    assert_eq!(failed["effect"], "recorded");
    assert_eq!(failed["result"]["code"], 1);
    let first_sha = failed["eventSha256"].as_str().unwrap();
    let ledger = fixture.ledger(work);
    fs::write(&marker, b"pass").unwrap();
    let (passed, passed_output) = fixture.call(&["work", "check", work, "--json"]);
    assert_fixture_step_budget(&fixture, &passed, &passed_output, &mut read_recipients);
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
