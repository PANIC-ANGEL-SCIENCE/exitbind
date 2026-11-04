mod mutation_history;

use super::*;
use crate::evidence::hash;

fn sections() -> Value {
    let content = format!(
        "{}\nLATE CORRECTION: never edit the protected file",
        "scope\n".repeat(400)
    );
    let sha = hash::text(&content);
    json!({"assignment":{"available":true,
            "assignment":{"agent":"worker","role":"worker","nativeTaskName":"sonic",
                "stage":2,"attempt":1,"goal":"Keep the verified task","purpose":"Implement the task",
                "profile":{"sha256":"profile-a"},"profileSha256":"profile-a",
                "basisSha256":null,"runtime":{"host":"claude"},
                "declaredBoundary":{"write":["allowed.txt"]},"upstreamArtifacts":[],
                "context":{"role":"worker","goal":"Keep the verified task",
                    "scope":{"attempt":1,"currentStage":2,"basisSha256":null,"configSha256":"config-a","freshness":"current","source":"run_ledger","owner":"lead"},
                    "subject":{"sha256":"subject-a"},
                    "run":{"id":"run-a","workflow":"change"},
                    "next":{"action":"spawn","agent":"worker","assignment":"assignment-a","role":"worker","owner":"worker","source":"run_state","freshness":"current"},
                    "loop":{"state":"ready","budget":3,"spent":1,"currentMutation":null,
                        "noInformationStreak":1,"afterReplan":false,"postReplanSpent":0,
                        "defaults":{"noInformationLimit":2,"postReplanLimit":1},
                        "evidence":[],"observations":[],"seenMutations":[]},
                    "recovery":{"conflicts":[],"stale":[]},"obligations":[{"kind":"worker"}],
                    "evidence":[{"kind":"submission","evidenceId":"event-a",
                        "artifact":{"kind":"evidence","bytes":content.len(),"exact":true,"sha256":sha},
                        "rawEventRef":{"kind":"ledger_event","selector":"event-a","id":"ref:source","exact":true,"work":"work-a"}}],
                    "expansions":[]}},
            "projection":{"equalAliases":[],"recoveryAliases":"equal recovery goal/scope/subject/current/missing/loop/next refer to context goal/scope/subject/evidence/obligations/loop/next"},
            "checkPreparation":{"program":"sh","input":{"argument":"verify.sh"}},
            "residual":{"work":"work-a","workflow":"change","historical":false,"next":"spawn","basis":null,
                "remaining":[{"obligation":"implementation"}],"alreadyEstablished":[],"doNotRepeat":[],"stillValid":[],
                "humanHelp":{"ownerDecision":"not_required","whatHappened":"A worker assignment is pending.",
                    "whatRemains":[{"obligation":"implementation"}],
                    "nextAction":{"actor":"worker","summary":"run the pending worker assignment",
                        "command":{"program":"exitbind","args":["work","next","work-a"],"argv":["/bin/exitbind","work","next","work-a"]}}},
                "invalidation":{"basisChangeInvalidates":true,"checkerChangeInvalidates":true,
                    "configChangeInvalidates":true,"sessionRestartInvalidates":false,
                    "subjectChangeInvalidates":true,"testedInputChangeInvalidates":true,
                    "rule":"reuse only while the run is active and its ledger head, exact subject, and tested inputs remain current",
                    "source":"run_ledger","owner":"lead","inputCoverage":"product-root-files-v1","configSha256":"config-a"}}},
            "evidence":{"complete":true,"historical":false,"items":[{"kind":"submission","eventSha256":"event-a",
                "reference":"ref:source","historical":false,"outcome":"scoped","result":null,
                "artifact":{"sha256":sha,"bytes":content.len(),"exact":true,"complete":true,
                    "verified":true,"readable":true,"encoding":"utf-8","content":content}}]},
            "tasks":{"goal":"Keep the verified task","tasks":[],"overall":"in_progress",
                "decomposition":{"available":false,"reason":"native_session_goal_not_requested"},
                "categoryDispositions":{}},
            "currentAction":{"binding":"binding-a","beforeEditing":{"required":true,
                "command":{"argv":["/bin/exitbind","work","permit","work-a","<OPERATION>"]},
                "inputConstraints":{"OPERATION":{"nonBlank":true}}}}})
}

#[test]
fn current_lead_repair_is_delivered_exactly_to_worker_and_reviewer() {
    let route = vec!["work".into(), "expand".into(), "bound".into()];
    for role in ["worker", "reviewer"] {
        let mut current = sections();
        let packet = &mut current["assignment"]["assignment"];
        packet["role"] = json!(role);
        packet["context"]["role"] = json!(role);
        packet["context"]["next"]["role"] = json!(role);
        packet["context"]["next"]["owner"] = json!(role);
        let repair = json!({"authority":"lead", "decision":"repair",
            "reason":"The inherited blank-row skip violates the row-shape contract",
            "repairBoundary":"Only report.py; preserve helper, verifier and modes",
            "decisiveRegression":"Reject zero-field records before and after valid rows",
            "findingSha256s":["a".repeat(64)], "dispositionSha256":"b".repeat(64),
            "basisSha256":null});
        packet["leadRepair"] = repair.clone();
        let actor =
            project_sections("work-a", "binding-a", &current, &route, &Value::Null, None).unwrap();
        assert_eq!(actor["leadRepair"], repair);
        assert_eq!(actor["boundary"], json!({"write":["allowed.txt"]}));

        current["assignment"]["assignment"]["leadRepair"]["newInstruction"] =
            json!("Do not silently discard future repair terms");
        assert!(
            project_sections("work-a", "binding-a", &current, &route, &Value::Null, None)
                .unwrap_err()
                .contains("unclassified")
        );
    }
}

#[test]
fn selected_actor_keeps_exact_late_facts_and_the_canonical_action() {
    let original = sections();
    let route = vec![
        "/bin/exitbind".into(),
        "work".into(),
        "expand".into(),
        "bound".into(),
    ];
    let actor = project_sections(
        "work-a",
        "binding-a",
        &original,
        &route,
        &json!("sh verify.sh --mode strict"),
        None,
    )
    .unwrap();
    assert_eq!(actor["ownerActionDetail"], json!(route));
    assert_eq!(actor["checkCommand"], "sh verify.sh --mode strict");
    assert_eq!(actor["currentAction"], original["currentAction"]);
    assert_eq!(
        actor["checkPreparation"],
        original["assignment"]["checkPreparation"]
    );
    assert_eq!(
        actor["boundary"],
        original["assignment"]["assignment"]["declaredBoundary"]
    );
    assert_eq!(
        actor["evidence"]["items"][0]["artifact"],
        original["evidence"]["items"][0]["artifact"]["content"]
    );
    assert_eq!(actor["evidence"]["items"][0]["reference"], "ref:source");
    assert!(actor
        .to_string()
        .contains("LATE CORRECTION: never edit the protected file"));
    assert_eq!(
        actor["obligations"]["remaining"][0]["obligation"],
        "implementation"
    );
    assert_eq!(actor["governor"]["noInformationStreak"], 1);
    assert_eq!(actor["governor"]["noInformationLimit"], 2);
    assert!(actor.get("artifactPathHint").is_none());
    assert!(actor.get("currentStatus").is_none());
    for args in [
        json!(["work", "next", "another-work"]),
        json!(["work", "next", "work-a", "--json"]),
        json!(["work", "check", "work-a"]),
        json!(["work", "next", null]),
    ] {
        let mut different_route = original.clone();
        different_route["assignment"]["residual"]["humanHelp"]["nextAction"]["command"]["args"] =
            args;
        let actor = project_sections(
            "work-a",
            "binding-a",
            &different_route,
            &route,
            &json!("sh verify.sh --mode strict"),
            None,
        )
        .unwrap();
        assert!(actor["currentStatus"].is_object());
    }
}

#[test]
fn check_evidence_alias_must_match_canonical_evidence() {
    let route = vec!["work".into(), "expand".into(), "bound".into()];
    let checks = json!([{"status":"passed","result":{"kind":"exit","code":0}}]);
    let mut current = sections();
    current["assignment"]["assignment"]["checkEvidence"] = checks.clone();
    current["evidence"]["checkEvidence"] = checks.clone();
    let actor = project_sections(
        "work-a",
        "binding-a",
        &current,
        &route,
        &json!("sh verify.sh --mode strict"),
        None,
    )
    .unwrap();
    assert_eq!(actor["evidence"]["checkEvidence"], checks);

    for change in [
        "different",
        "missing-alias",
        "missing-canonical",
        "null-alias",
    ] {
        let mut altered = current.clone();
        match change {
            "different" => {
                altered["assignment"]["assignment"]["checkEvidence"][0]["status"] = json!("failed");
            }
            "missing-alias" => {
                altered["assignment"]["assignment"]
                    .as_object_mut()
                    .unwrap()
                    .remove("checkEvidence");
            }
            "missing-canonical" => {
                altered["evidence"]
                    .as_object_mut()
                    .unwrap()
                    .remove("checkEvidence");
            }
            "null-alias" => altered["assignment"]["assignment"]["checkEvidence"] = Value::Null,
            _ => unreachable!(),
        }
        let error = project_sections(
            "work-a",
            "binding-a",
            &altered,
            &route,
            &json!("sh verify.sh --mode strict"),
            None,
        )
        .unwrap_err();
        assert!(error.contains("check evidence"), "{change}: {error}");
    }
    current["assignment"]["assignment"]["newInstruction"] = json!("never drop this");
    let error = project_sections(
        "work-a",
        "binding-a",
        &current,
        &route,
        &json!("sh verify.sh --mode strict"),
        None,
    )
    .unwrap_err();
    assert!(error.contains("unclassified"), "{error}");
}

#[test]
fn unequal_facts_survive_and_unknown_or_wrong_subject_refuses() {
    let mut original = sections();
    original["assignment"]["assignment"]["context"]["goal"] = json!("different current goal");
    original["tasks"]["goal"] = json!("different task goal");
    original["assignment"]["residual"]["goal"] = json!("different residual goal");
    original["assignment"]["residual"]["currentSubject"] = json!({"sha256":"different subject"});
    original["assignment"]["assignment"]["preservationAssignment"] = json!({"rule":"keep alpha"});
    original["assignment"]["residual"]["humanHelp"]["preservationAssignment"] =
        json!({"rule":"keep beta"});
    let route = vec!["work".into(), "expand".into(), "bound".into()];
    let actor = project_sections(
        "work-a",
        "binding-a",
        &original,
        &route,
        &json!("sh verify.sh --mode strict"),
        None,
    )
    .unwrap();
    assert_eq!(actor["contextGoal"], "different current goal");
    assert_eq!(actor["taskGoal"], "different task goal");
    assert_eq!(actor["residualGoal"], "different residual goal");
    assert_eq!(actor["residualSubject"]["sha256"], "different subject");
    assert_eq!(actor["preservationAssignment"]["rule"], "keep alpha");
    assert_eq!(
        actor["additionalPreservationAssignment"]["rule"],
        "keep beta"
    );
    let original_hash = hash::value(&actor);
    let changed = "new late correction";
    original["evidence"]["items"][0]["artifact"]["content"] = json!(changed);
    original["evidence"]["items"][0]["artifact"]["bytes"] = json!(changed.len());
    original["evidence"]["items"][0]["artifact"]["sha256"] = json!(hash::text(changed));
    original["assignment"]["assignment"]["context"]["evidence"][0]["artifact"]["bytes"] =
        json!(changed.len());
    original["assignment"]["assignment"]["context"]["evidence"][0]["artifact"]["sha256"] =
        json!(hash::text(changed));
    assert_ne!(
        original_hash,
        hash::value(
            &project_sections(
                "work-a",
                "binding-a",
                &original,
                &route,
                &json!("sh verify.sh --mode strict"),
                None
            )
            .unwrap()
        )
    );
    original["assignment"]["assignment"]["context"]["loop"]["lateProhibition"] =
        json!("never bypass this rule");
    assert!(project_sections(
        "work-a",
        "binding-a",
        &original,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .unwrap_err()
    .contains("unclassified"));
    original["assignment"]["assignment"]["context"]["loop"]
        .as_object_mut()
        .unwrap()
        .remove("lateProhibition");
    original["assignment"]["assignment"]["context"]["evidence"][0]["rawEventRef"]["work"] =
        json!("other-work");
    assert!(project_sections(
        "work-a",
        "binding-a",
        &original,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .is_err());
    original["assignment"]["assignment"]["context"]["evidence"][0]["rawEventRef"]["work"] =
        json!("work-a");
    original["assignment"]["assignment"]["context"]["evidence"][0]["rawEventRef"]["exact"] =
        json!(false);
    assert!(project_sections(
        "work-a",
        "binding-a",
        &original,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .unwrap_err()
    .contains("malformed"));
    assert!(project_sections(
        "work-a",
        "wrong-binding",
        &sections(),
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .is_err());
    original = sections();
    original["assignment"]["assignment"]["profile"]["sha256"] = json!("different");
    assert!(project_sections(
        "work-a",
        "binding-a",
        &original,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .is_err());
}

#[test]
fn current_status_and_unclassified_nested_instructions_cannot_disappear() {
    let route = vec!["work".into(), "expand".into(), "bound".into()];
    let mut current = sections();
    current["assignment"]["residual"]["humanHelp"]["preservationAssignment"] =
        json!({"rule":"preserve the only original"});
    current["evidence"]["items"][0]["artifact"]["reason"] = json!("verify before using");
    let actor = project_sections(
        "work-a",
        "binding-a",
        &current,
        &route,
        &json!("sh verify.sh --mode strict"),
        None,
    )
    .unwrap();
    assert_eq!(
        actor["preservationAssignment"]["rule"],
        "preserve the only original"
    );
    assert_eq!(
        actor["evidence"]["items"][0]["artifactStatus"]["reason"],
        "verify before using"
    );
    let mut replanned = current.clone();
    let governor = &mut replanned["assignment"]["assignment"]["context"]["loop"];
    governor["afterReplan"] = json!(true);
    governor["postReplanSpent"] = json!(1);
    let actor = project_sections(
        "work-a",
        "binding-a",
        &replanned,
        &route,
        &json!("sh verify.sh --mode strict"),
        None,
    )
    .unwrap();
    assert_eq!(actor["governor"]["postReplanSpent"], 1);
    assert_eq!(actor["governor"]["postReplanLimit"], 1);
    replanned["assignment"]["assignment"]["context"]["loop"]["defaults"]
        .as_object_mut()
        .unwrap()
        .remove("postReplanLimit");
    assert!(project_sections(
        "work-a",
        "binding-a",
        &replanned,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .is_err());

    let mut mismatched_source = current.clone();
    let source = &mut mismatched_source["assignment"]["assignment"]["context"]["evidence"][0];
    source["status"] = json!("current");
    source["evidenceId"] = json!("different-event");
    assert!(project_sections(
        "work-a",
        "binding-a",
        &mismatched_source,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .is_err());

    for path in [
        "/assignment/checkPreparation/input/newInstruction",
        "/currentAction/beforeEditing/command/newInstruction",
        "/currentAction/beforeEditing/inputConstraints/OPERATION/newInstruction",
    ] {
        let mut changed = current.clone();
        let (parent, key) = path.rsplit_once('/').unwrap();
        changed
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), json!("do not proceed"));
        assert!(project_sections(
            "work-a",
            "binding-a",
            &changed,
            &route,
            &json!("sh verify.sh --mode strict"),
            None
        )
        .unwrap_err()
        .contains("unclassified"));
    }

    let mut changed = current.clone();
    changed["assignment"]["residual"]["snapshot"] = json!({"newInstruction":"do not proceed"});
    assert!(project_sections(
        "work-a",
        "binding-a",
        &changed,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .unwrap_err()
    .contains("unclassified"));
    let mut changed = current.clone();
    changed["tasks"]["categoryDispositions"]["blockers"] = json!({
        "state":"considered", "itemsOmitted":0,
        "items":[{"id":"blocked", "disposition":"open", "valid":true,
                  "newInstruction":"do not proceed"}]
    });
    assert!(project_sections(
        "work-a",
        "binding-a",
        &changed,
        &route,
        &json!("sh verify.sh --mode strict"),
        None
    )
    .unwrap_err()
    .contains("unclassified"));
}

#[test]
fn human_help_instructions_survive_default_worker_and_reviewer_presentation() {
    let instruction = json!({"mode":"execute_and_record_once",
        "summary":"do not separately run the frozen command", "futureDetail":"retain"});
    for reviewer in [false, true] {
        for include in [1, 2, 3] {
            let mut current = sections();
            let help = &mut current["assignment"]["residual"]["humanHelp"];
            if reviewer {
                help["whatHappened"] = json!("A reviewer assignment is pending.");
                help["nextAction"]["actor"] = json!("reviewer");
                help["nextAction"]["summary"] = json!("run the pending reviewer assignment");
            }
            if include & 1 != 0 {
                help["checkInstruction"] = instruction.clone();
            }
            if include & 2 != 0 {
                help["preservationEvidence"] = json!("command_observed");
            }
            let expected = help.clone();
            let actor =
                project_sections("work-a", "binding-a", &current, &[], &Value::Null, None).unwrap();
            for field in ["checkInstruction", "preservationEvidence"] {
                assert_eq!(actor["currentStatus"][field], expected[field]);
            }
            assert_eq!(
                actor["currentStatus"].get("whatHappened").is_some(),
                reviewer
            );
            for pointer in [
                "/assignment/residual/humanHelp",
                "/assignment/residual/humanHelp/nextAction",
                "/assignment/residual/humanHelp/nextAction/command",
            ] {
                let mut unknown = current.clone();
                unknown.pointer_mut(pointer).unwrap()["futureInstruction"] =
                    json!("must not vanish");
                assert!(
                    project_sections("work-a", "binding-a", &unknown, &[], &Value::Null, None)
                        .unwrap_err()
                        .contains("unclassified")
                );
            }
        }
    }
}

mod grants;
