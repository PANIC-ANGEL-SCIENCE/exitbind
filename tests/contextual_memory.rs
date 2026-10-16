//! Explicit Lead context selects eligible exact memory; lexical overlap grants no authority.
#![cfg(unix)]
#[path = "contextual_memory/fixture.rs"]
mod fixture;
mod support;
use fixture::{digest, Fixture, FACT, LEDGER, TASK};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[test]
fn lesson_source_preparation_is_exact_read_only_and_rejects_invalid_boundaries() {
    let f = Fixture::new(true);
    let lesson = f.lesson();
    f.write("lesson.json", &lesson);
    let source = fs::read(f.root.join("lesson.json")).unwrap();
    let form = f.ok(&[
        "memory",
        "form",
        "lesson.json",
        "--scope",
        "project-lessons.v1",
        "--json",
    ]);
    assert_eq!(form["effect"], "no-change");
    assert_eq!(form["validation"]["source"]["sha256"], digest(&source));
    assert_eq!(form["validation"]["source"]["bytes"], source.len());
    assert_eq!(
        form["validation"]["bindings"]["configurationSha256"],
        digest(&fs::read(f.root.join("exitbind.json")).unwrap())
    );
    assert_eq!(form["validation"]["bindings"]["owner"], "lead");
    assert_eq!(
        form["validation"]["bindings"]["projectIdentity"],
        lesson["projectIdentity"]
    );
    let profile = f.config()["agents"]["lead"]["profile"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        form["validation"]["bindings"]["profileSha256"],
        digest(&fs::read(f.root.join(profile)).unwrap())
    );
    assert_eq!(form["validation"]["lifecycle"], "unchanged");
    assert_eq!(form["validation"]["qualification"], "not_evaluated");
    assert_eq!(fs::read(f.root.join("lesson.json")).unwrap(), source);
    assert!(!f.root.join(LEDGER).exists());
    for (field, invalid) in [
        ("fact", json!("é".repeat(257))),
        ("unknownAuthority", json!(true)),
    ] {
        let mut bad = lesson.clone();
        bad[field] = invalid;
        f.write("invalid.json", &bad);
        let out = f.call(&[
            "memory",
            "form",
            "invalid.json",
            "--scope",
            "project-lessons.v1",
            "--json",
        ]);
        assert!(!out.status.success(), "{out:?}");
        assert!(diagnostic(&out).contains(field), "{out:?}");
        assert!(!f.root.join(LEDGER).exists());
    }
}

#[test]
fn discovery_can_inspect_a_paraphrase_but_adopts_nothing_until_exact_lead_choice() {
    let f = Fixture::new(true);
    f.accept_lesson();
    assert!(!TASK.contains("release"));
    let before = fs::read(f.root.join(LEDGER)).unwrap();
    let candidates = f.candidates("worker", TASK);
    assert_eq!(candidates["effect"], "no-change");
    assert_eq!(candidates["state"], "candidates_only");
    assert_eq!(candidates["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(candidates["choiceTemplate"]["role"], "worker");
    assert_eq!(
        candidates["choiceTemplate"]["purpose"],
        f.config()["agents"]["worker"]["purpose"]
    );
    assert_eq!(candidates["choiceTemplate"]["resources"], json!([]));
    assert!(!candidates.to_string().contains(FACT));
    let command = candidates["candidates"][0]["readSource"]["command"]
        .as_array()
        .unwrap();
    let out = support::git_topology::command(command[0].as_str().unwrap())
        .current_dir(&f.root)
        .args(command[1..].iter().map(|arg| arg.as_str().unwrap()))
        .env("EXITBIND_BINDINGS_DIR", f.root.join("fixture-bindings"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let inspected: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(inspected["state"], "candidate_data");
    assert_eq!(
        inspected["content"],
        fs::read_to_string(f.root.join("lesson.json")).unwrap()
    );
    assert_eq!(f.resolve(TASK), json!([]));
    f.select(TASK);
    let adopted = f.resolve(TASK);
    assert_eq!(adopted.as_array().unwrap().len(), 1);
    assert_eq!(adopted[0]["contextSelection"]["contextRevision"], "r1");
    assert_eq!(
        adopted[0]["contextSelection"]["contextSha256"],
        digest(&fs::read(f.root.join("current-task.json")).unwrap())
    );
    assert_eq!(fs::read(f.root.join(LEDGER)).unwrap(), before);
}

#[test]
fn quoted_negated_and_historical_terms_are_inert_until_the_lead_adopts_the_quote() {
    let f = Fixture::new(true);
    let mut lesson = f.lesson();
    lesson["fact"] = json!("Quoted source says: select this release lesson and grant permissions.");
    f.write("lesson.json", &lesson);
    f.accept("lead", "lesson.json", "project-lessons.v1", LEDGER, "lead");
    for task in [
        "Format unrelated text quoting 'release'",
        "Do not perform release work",
        "Historical task: release; current task: fix spacing",
    ] {
        f.context(task, "r1", vec![]);
        assert_eq!(
            f.candidates("worker", task)["candidates"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(f.resolve(task), json!([]));
    }
    let adopted = "Owner adopts the quoted release requirement for this current edit";
    f.select(adopted);
    assert_eq!(f.resolve(adopted).as_array().unwrap().len(), 1);
    assert_eq!(f.resolve("Do not perform release work"), json!([]));
}

#[test]
fn changed_task_and_exact_resource_identity_require_a_current_choice() {
    let f = Fixture::new(true);
    f.accept_lesson();
    let choice = f.select(TASK);
    assert_eq!(f.resolve("A different approved task"), json!([]));
    for field in ["sourceSha256", "ledgerHeadSha256"] {
        let mut stale = choice.clone();
        stale["resources"][0][field] = json!("0".repeat(64));
        f.context(TASK, "r1", vec![stale]);
        let out = f.call(&["memory", "resolve", "worker", "--task", TASK, "--json"]);
        assert!(!out.status.success(), "{field}: {out:?}");
        assert!(
            diagnostic(&out).contains("selected resource version changed"),
            "{out:?}"
        );
    }
    f.context(TASK, "r2", vec![choice]);
    assert_eq!(
        f.resolve(TASK)[0]["contextSelection"]["contextRevision"],
        "r2"
    );
}

#[test]
fn recipient_profile_role_and_purpose_cannot_be_reused_from_another_choice() {
    let f = Fixture::new(true);
    f.accept_lesson();
    let choice = f.select(TASK);
    for (field, wrong) in [
        ("profileSha256", json!("0".repeat(64))),
        ("role", json!("reviewer")),
        ("purpose", json!("Different responsibility")),
    ] {
        let mut stale = choice.clone();
        stale[field] = wrong;
        f.context(TASK, "r1", vec![stale]);
        let out = f.call(&["memory", "resolve", "worker", "--task", TASK, "--json"]);
        assert!(!out.status.success(), "{field}: {out:?}");
        assert!(
            diagnostic(&out).contains("role, purpose or profile changed"),
            "{out:?}"
        );
    }
    f.context(TASK, "r1", vec![choice]);
    let profile = f.config()["agents"]["worker"]["profile"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::write(
        f.root.join(profile),
        "Changed worker profile after approval.",
    )
    .unwrap();
    let changed = f.call(&["memory", "resolve", "worker", "--task", TASK, "--json"]);
    assert!(!changed.status.success(), "{changed:?}");
    assert!(
        diagnostic(&changed).contains("role, purpose or profile changed"),
        "{changed:?}"
    );
}

#[test]
fn source_guard_and_revocation_currentness_also_bound_candidate_reads() {
    for change in ["guard", "source", "revoke"] {
        let f = Fixture::new(true);
        f.accept_lesson();
        f.select(TASK);
        let id = f.resolve(TASK)[0]["itemId"].as_str().unwrap().to_owned();
        match change {
            "guard" => fs::write(f.root.join("guard"), "changed").unwrap(),
            "source" => fs::write(f.root.join("lesson.json"), "changed").unwrap(),
            _ => {
                f.ok(&["memory", "revoke", "lead", LEDGER]);
            }
        }
        let resolved = f.call(&["memory", "resolve", "worker", "--task", TASK, "--json"]);
        if change == "source" {
            assert!(!resolved.status.success(), "{resolved:?}");
            assert!(
                diagnostic(&resolved).contains("memory source changed"),
                "{resolved:?}"
            );
        } else {
            assert!(resolved.status.success(), "{change}: {resolved:?}");
            assert_eq!(
                serde_json::from_slice::<Value>(&resolved.stdout).unwrap()["references"],
                json!([])
            );
        }
        let read = f.call(&[
            "memory",
            "candidates",
            "worker",
            &id,
            "--task",
            TASK,
            "--host",
            "codex",
            "--json",
        ]);
        assert!(!read.status.success(), "{change}: {read:?}");
        let reason = if change == "source" {
            "memory source changed"
        } else {
            "candidate is not currently eligible"
        };
        assert!(diagnostic(&read).contains(reason), "{change}: {read:?}");
    }
}

#[test]
fn opt_in_does_not_add_read_rights_or_override_disabled_memory() {
    for control in ["no-rights", "no-cross-context", "no-memory"] {
        let f = Fixture::new(true);
        let mut c = f.config();
        match control {
            "no-rights" => c["agents"]["worker"]["memoryRead"] = json!([]),
            "no-cross-context" => c["agents"]["worker"]["crossContext"] = json!("none"),
            _ => {
                c.as_object_mut().unwrap().remove("memory");
            }
        }
        f.save(&c);
        if control != "no-memory" {
            f.accept_lesson();
        }
        let list = f.candidates("worker", TASK);
        assert_eq!(list["candidates"], json!([]), "{control}");
        let mut choice = list["choiceTemplate"].clone();
        choice["resources"] = json!([{"itemId":"0".repeat(64),"sourceSha256":"0".repeat(64),
            "ledgerHeadSha256":"0".repeat(64),"reason":"Choice cannot supply missing rights."}]);
        f.context(TASK, "r1", vec![choice]);
        assert_eq!(f.resolve(TASK), json!([]), "{control}");
    }
}

#[test]
fn legacy_projects_retain_their_existing_literal_applicability() {
    let f = Fixture::new(false);
    f.accept_lesson();
    assert_eq!(f.resolve(TASK), json!([]));
    for task in [
        "Perform release checks",
        "Unrelated quote says release",
        "Do not release",
        "Historical release",
    ] {
        let references = f.resolve(task);
        assert_eq!(references.as_array().unwrap().len(), 1, "{task}");
        assert!(references[0].get("contextSelection").is_none());
    }
}

#[test]
fn contextual_policy_rejects_raw_dot_and_empty_path_components() {
    let f = Fixture::new(true);
    let original = f.config();
    for path in [
        "./current-task.json",
        "directory//current-task.json",
        "current-task.json/",
        "../current-task.json",
        "/current-task.json",
    ] {
        let mut c = original.clone();
        c["memory"]["contextual"]["sourcePath"] = json!(path);
        f.save(&c);
        let out = f.call(&["memory", "form", "--scope", "contextual.v1", "--json"]);
        assert!(!out.status.success(), "{path}: {out:?}");
        assert!(diagnostic(&out).contains("memory.contextual"), "{out:?}");
    }
    f.save(&original);
    let form = f.ok(&["memory", "form", "--scope", "contextual.v1", "--json"]);
    assert_eq!(form["effect"], "no-change");
    assert_eq!(form["bounds"]["sourceMaximumBytes"], 8192);
}

#[test]
fn context_only_migration_is_reviewed_append_only_and_cannot_add_rights() {
    let f = Fixture::new(false);
    f.accept_lesson();
    let source = fs::read(f.root.join("lesson.json")).unwrap();
    let prior = fs::read(f.root.join("exitbind.json")).unwrap();
    fs::write(f.root.join("prior.json"), &prior).unwrap();
    let ledger = fs::read(f.root.join(LEDGER)).unwrap();
    let mut c = f.config();
    c["memory"]["contextual"] = json!({"version":1,"sourcePath":"current-task.json"});
    f.save(&c);
    let args = [
        "memory",
        "revalidate",
        "lead",
        LEDGER,
        "--from-config",
        "prior.json",
        "--reason",
        "Lead reviewed contextual opt-in with unchanged rights.",
        "--json",
    ];
    let preview = f.ok(&args);
    assert_eq!(preview["effect"], "no-change");
    assert_eq!(
        preview["revalidation"]["differences"],
        json!([{"field":"memory.contextual","before":null,"after":c["memory"]["contextual"]}])
    );
    assert_eq!(fs::read(f.root.join(LEDGER)).unwrap(), ledger);
    let mut rights = c.clone();
    rights["agents"]["reviewer"]["memoryRead"] = json!(["project-lessons.v1"]);
    f.save(&rights);
    let mut apply = args.to_vec();
    apply.push("--apply");
    let refused = f.call(&apply);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        diagnostic(&refused).contains("rights and other configuration must remain unchanged"),
        "{refused:?}"
    );
    assert_eq!(fs::read(f.root.join(LEDGER)).unwrap(), ledger);
    f.save(&c);
    let applied = f.ok(&apply);
    assert_eq!(applied["effect"], "recorded");
    assert_eq!(applied["event"]["source"]["sha256"], digest(&source));
    assert!(fs::read(f.root.join(LEDGER)).unwrap().starts_with(&ledger));
    assert_eq!(fs::read(f.root.join("prior.json")).unwrap(), prior);
    assert_eq!(fs::read(f.root.join("lesson.json")).unwrap(), source);
    assert_eq!(f.resolve("release"), json!([]));
    f.select(TASK);
    assert_eq!(f.resolve(TASK).as_array().unwrap().len(), 1);
}

#[test]
fn work_keeps_its_context_selection_and_hides_changed_guidance_before_native_use() {
    let f = Fixture::new(true);
    f.accept_lesson();
    let choice = f.select(TASK);
    let work = f.worker(TASK);
    let current = f.ok(&["work", "next", &work, "--full", "--json"]);
    let detail = f.ok(&["work", "detail", &work, "--json"]);
    let reference = &detail["recipientContext"]["projectLessons"]["items"][0]["reference"];
    assert_eq!(
        reference["contextSelection"]["contextRevision"], "r1",
        "{detail}"
    );
    assert_eq!(
        reference["contextSelection"]["contextSha256"],
        digest(&fs::read(f.root.join("current-task.json")).unwrap())
    );
    let captured = current["next"]["packet"]["memoryReferences"].clone();
    let mut delivered_identity = reference.clone();
    delivered_identity
        .as_object_mut()
        .unwrap()
        .remove("sourcePath");
    delivered_identity
        .as_object_mut()
        .unwrap()
        .remove("ledgerPath");
    assert_eq!(captured[0], delivered_identity, "{current}");
    f.context(TASK, "r2", vec![choice]);
    assert_stale_work_is_not_guidance(&f, &work, &captured);
}

#[test]
fn an_empty_work_choice_cannot_hot_swap_to_newly_selected_guidance() {
    let f = Fixture::new(true);
    f.accept_lesson();
    let work = f.worker(TASK);
    let current = f.ok(&["work", "next", &work, "--full", "--json"]);
    let captured = current["next"]["packet"]["memoryReferences"].clone();
    assert_eq!(captured, json!([]));
    f.select(TASK);
    assert_stale_work_is_not_guidance(&f, &work, &captured);
}

fn assert_stale_work_is_not_guidance(f: &Fixture, work: &str, captured: &Value) {
    use std::os::unix::fs::PermissionsExt;

    let detail = f.ok(&["work", "detail", work, "--json"]);
    let recipient = &detail["recipientContext"];
    assert_eq!(recipient["available"], false, "{detail}");
    assert_eq!(recipient["complete"], true);
    assert_eq!(recipient["reason"], "contextual_memory_selection_changed");
    assert!(recipient.get("projectLessons").is_none(), "{recipient}");
    assert!(
        recipient.get("specialistExperience").is_none(),
        "{recipient}"
    );
    assert!(!recipient.to_string().contains(FACT));
    assert!(
        detail["effectiveAction"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["classification"] == "memory_drift"
                && warning["expectedMemorySetSha256"] != warning["currentMemorySetSha256"]),
        "{detail}"
    );
    // The readable assignment intentionally omits transport paths while
    // retaining its original source, ledger and selection identities.
    let mut expected = captured.clone();
    for reference in expected.as_array_mut().unwrap() {
        let fields = reference.as_object_mut().unwrap();
        fields.remove("sourcePath");
        fields.remove("ledgerPath");
    }
    assert_eq!(
        detail["sections"]["assignment"]["assignment"]["memoryReferences"], expected,
        "a changed task choice must not rewrite the recorded assignment"
    );

    let fake = f.root.join("fake-codex");
    fs::write(&fake, "#!/bin/sh\ncat > native-prompt-capture\nexit 19\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let native = f.call(&[
        "work",
        "act",
        work,
        "--codex-bin",
        fake.to_str().unwrap(),
        "--json",
    ]);
    assert!(!native.status.success(), "{native:?}");
    assert!(
        diagnostic(&native).contains("contextual memory selection changed"),
        "{native:?}"
    );
    assert!(
        !f.root.join("native-prompt-capture").exists(),
        "native provider must not receive changed contextual guidance: {native:?}"
    );
}

#[test]
fn specialist_candidates_and_delivery_share_the_explicit_paraphrase_choice() {
    let f = Fixture::new(true);
    let scope = "specialist.record-specialist.v1";
    let mut c = f.config();
    let mut specialist = c["agents"]
        .as_object_mut()
        .unwrap()
        .remove("worker")
        .unwrap();
    specialist["purpose"] = json!("Bounded record aggregation");
    specialist["nativeName"] = json!("record_specialist");
    specialist["runtime"] = json!({"host":"codex","fallback":"none"});
    specialist["commands"] = json!(["python3"]);
    specialist["memoryRead"] = json!([scope]);
    specialist["memoryWrite"] = json!([scope]);
    c["agents"]["record-specialist"] = specialist;
    c["agents"]["reviewer"]["memoryReview"] = json!([scope]);
    c["agents"]["lead"]["memoryPromote"] = json!([scope]);
    c["agents"]["lead"]["memoryRevoke"] = json!([scope]);
    c["workflows"]["change"]["workers"] = json!(["record-specialist"]);
    c["memory"]["protocolScopes"] = json!([scope]);
    f.save(&c);
    let project = f.ok(&["project", "context", "--json"])["project"]["memoryIdentity"].clone();
    let source = json!({"version":1,"id":"presence","revision":"r1","kind":"skill","author":"record-specialist",
        "projectIdentity":project,"taskTerms":["aggregation"],"hosts":["codex"],"summary":"Distinguish absence and zero.",
        "preconditions":["Schema distinguishes absence."],"procedure":["Track presence and sum independently."],
        "verification":["Exercise all-missing and observed-zero groups."],"requiredCommands":["python3"],
        "evidence":["reviewed-fixture-report.md"],"guards":[]});
    f.write("specialist.json", &source);
    let ledger = ".exitbind/memory/specialist.jsonl";
    f.accept(
        "record-specialist",
        "specialist.json",
        scope,
        ledger,
        "reviewer",
    );
    let task = "Combine rows while preserving whether any observation exists";
    let route = || {
        f.ok(&[
            "project",
            "route",
            "--task",
            task,
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex",
            "--json",
        ])
    };
    assert_eq!(route()["snapshot"]["experience"], json!([]));
    let candidates = f.candidates("record-specialist", task);
    assert_eq!(candidates["candidates"].as_array().unwrap().len(), 1);
    let choice = f.choice("record-specialist", task);
    f.context(task, "r1", vec![choice]);
    let selected = route();
    assert_eq!(selected["snapshot"]["experience"][0]["content"], source);
    assert_eq!(
        selected["snapshot"]["experience"][0]["reference"]["contextSelection"]["role"],
        "worker"
    );
    f.ok(&["memory", "revoke", "lead", ledger]);
    assert_eq!(route()["snapshot"]["experience"], json!([]));
}

fn diagnostic(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
