//! Task-local artifacts exercise ordinary preparation and inherited Work/native boundaries.
#![cfg(unix)]
#[path = "contextual_memory/fixture.rs"]
#[allow(dead_code)]
mod fixture;
mod support;
use fixture::{Fixture, FACT, LEDGER, SPECIALIST_GUIDANCE, SPECIALIST_TASK, TASK};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;

fn diagnostic(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}
fn local() -> Fixture {
    let f = Fixture::new(false);
    let mut c = f.config();
    c["agents"]["lead"]["memoryExpire"] = json!(["project-lessons.v1"]);
    c["memory"]["contextual"] =
        json!({"version":2,"sourcePath":".exitbind/artifacts/memory-choices"});
    c["agents"]["worker"]["runtime"] = json!({"host":"codex","fallback":"none"});
    c["agents"]["reviewer"]["runtime"] = json!({"host":"codex","fallback":"none"});
    f.save(&c);
    f
}
fn specialist() -> Fixture {
    let f = Fixture::specialist_mode(false);
    f.write("prior.json", &f.config());
    let mut c = f.config();
    c["memory"]["contextual"] =
        json!({"version":2,"sourcePath":".exitbind/artifacts/memory-choices"});
    f.save(&c);
    f.ok(&[
        "memory",
        "revalidate",
        "lead",
        ".exitbind/memory/specialist.jsonl",
        "--from-config",
        "prior.json",
        "--reason",
        "Lead selects task-local context with unchanged existing specialist rights.",
        "--apply",
        "--json",
    ]);
    f
}
fn preview(
    f: &Fixture,
    agent: &str,
    task: &str,
    items: &[&str],
    previous: &str,
    reason: &str,
) -> Value {
    let mut a = vec!["memory", "choose", agent];
    a.extend(items);
    a.extend([
        "--task",
        task,
        "--host",
        "codex",
        "--reason",
        reason,
        "--previous",
        previous,
        "--json",
    ]);
    f.ok(&a)
}
fn apply(f: &Fixture, p: &Value) -> std::process::Output {
    let a = p["apply"]["command"].as_array().unwrap();
    // Execute the emitted exact product route, without reconstructing identities.
    support::git_topology::command(a[0].as_str().unwrap())
        .current_dir(&f.root)
        .args(a[1..].iter().map(|v| v.as_str().unwrap()))
        .env("EXITBIND_BINDINGS_DIR", f.root.join("fixture-bindings"))
        .output()
        .unwrap()
}
fn choose(f: &Fixture, agent: &str, task: &str, items: &[&str], previous: &str) -> Value {
    let p = preview(
        f,
        agent,
        task,
        items,
        previous,
        "Lead reviewed the selected resources for this exact approved task.",
    );
    let out = apply(f, &p);
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}
fn item(f: &Fixture, agent: &str, task: &str) -> String {
    f.candidates(agent, task)["candidates"][0]["reference"]["itemId"]
        .as_str()
        .unwrap()
        .into()
}
fn packet(f: &Fixture, w: &str) -> Value {
    f.ok(&["work", "next", w, "--full", "--json"])["next"]["packet"].clone()
}
fn stale(f: &Fixture, w: &str) {
    let d = f.ok(&["work", "detail", w, "--json"]);
    assert_eq!(d["recipientContext"]["available"], false, "{d}");
    assert!(!d["recipientContext"].to_string().contains(FACT));
}

#[test]
fn v1_interleaving_retains_its_conservative_contract() {
    let f = Fixture::new(true);
    f.accept_lesson();
    f.select(TASK);
    let a = f.worker_unmarked(TASK);
    assert_eq!(f.resolve(TASK).as_array().unwrap().len(), 1);
    f.context("Independent task B", "r2", vec![]);
    let _b = f.worker_unmarked("Independent task B");
    assert_eq!(f.resolve(TASK), json!([]));
    stale(&f, &a);
}
#[test]
fn independent_tasks_and_recipients_keep_exact_choices_without_fingerprint_changes() {
    let f = local();
    f.accept_lesson();
    let id = item(&f, "worker", TASK);
    let chosen = choose(&f, "worker", TASK, &[&id], "absent");
    let a = f.worker_unmarked(TASK);
    let before = packet(&f, &a);
    let btask = "Format unrelated text quoting release as data";
    let bchoice = choose(&f, "worker", btask, &[], "absent");
    let b = f.worker_unmarked(btask);
    assert_eq!(f.resolve(btask), json!([]));
    assert_eq!(
        f.resolve(TASK)[0]["contextSelection"]["contextSha256"],
        chosen["contextSha256"]
    );
    choose(
        &f,
        "worker",
        btask,
        &[&id],
        bchoice["contextSha256"].as_str().unwrap(),
    );
    choose(&f, "reviewer", btask, &[], "absent");
    let after = packet(&f, &a);
    assert_eq!(
        before["memoryContextSelection"],
        after["memoryContextSelection"]
    );
    assert_eq!(before["memoryReferences"], after["memoryReferences"]);
    assert_eq!(before["context"]["digest"], after["context"]["digest"]);
    assert_eq!(
        f.ok(&["work", "detail", &a, "--json"])["recipientContext"]["available"],
        true
    );
    stale(&f, &b); // B changed its own frozen choice, which remains an explicit transition.
    let wrong = preview(
        &f,
        "reviewer",
        TASK,
        &[],
        "absent",
        "Reviewer's separate empty choice.",
    );
    assert!(apply(&f, &wrong).status.success());
    assert_eq!(
        f.ok(&["memory", "resolve", "reviewer", "--task", TASK, "--json"])["references"],
        json!([])
    );
}
#[test]
fn empty_choice_is_frozen_and_same_text_works_deliberately_share_currentness() {
    let f = local();
    f.accept_lesson();
    let empty = choose(&f, "worker", TASK, &[], "absent");
    let a = f.worker_unmarked(TASK);
    let a2 = f.worker_unmarked(TASK);
    assert_eq!(
        packet(&f, &a)["memoryContextSelection"]["contextSha256"],
        empty["contextSha256"]
    );
    assert_eq!(
        packet(&f, &a)["memoryContextSelection"],
        packet(&f, &a2)["memoryContextSelection"]
    );
    let id = item(&f, "worker", TASK);
    choose(
        &f,
        "worker",
        TASK,
        &[&id],
        empty["contextSha256"].as_str().unwrap(),
    );
    stale(&f, &a);
    stale(&f, &a2);
}
#[test]
fn exact_replay_recovers_original_after_later_revision_and_conflicts_never_append() {
    let f = local();
    let p = preview(
        &f,
        "worker",
        TASK,
        &[],
        "absent",
        "First no-adoption decision.",
    );
    let other = preview(&f, "worker", TASK, &[], "absent", "Competing decision.");
    let one = apply(&f, &p);
    assert!(one.status.success());
    let one: Value = serde_json::from_slice(&one.stdout).unwrap();
    let conflict = apply(&f, &other);
    assert!(!conflict.status.success());
    assert!(diagnostic(&conflict).contains("expected-version conflict"));
    let two = choose(
        &f,
        "worker",
        TASK,
        &[],
        one["contextSha256"].as_str().unwrap(),
    );
    let replay = apply(&f, &p);
    assert!(replay.status.success(), "{replay:?}");
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["contextSha256"], one["contextSha256"]);
    assert_eq!(replay["effect"], "replayed");
    assert_eq!(replay["current"], false);
    assert_eq!(replay["currentlyUsable"], false);
    let recovered = f.ok(&[
        "memory",
        "choice",
        "worker",
        "--task",
        TASK,
        "--request-id",
        p["expectedSha256"].as_str().unwrap(),
        "--json",
    ]);
    assert_eq!(recovered["record"], one["record"]);
    assert_eq!(
        f.ok(&["memory", "choice", "worker", "--task", TASK, "--json"])["contextSha256"],
        two["contextSha256"]
    );
}
#[test]
fn stale_preview_and_cross_recipient_or_host_selection_have_no_effect() {
    let f = local();
    f.accept_lesson();
    let id = item(&f, "worker", TASK);
    let p = preview(&f, "worker", TASK, &[&id], "absent", "Reviewed paraphrase.");
    f.ok(&["memory", "revoke", "lead", LEDGER]);
    let out = apply(&f, &p);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        f.ok(&["memory", "choice", "worker", "--task", TASK, "--json"])["contextSha256"].is_null()
    );
    let out = f.call(&[
        "memory",
        "choose",
        "reviewer",
        &id,
        "--task",
        TASK,
        "--host",
        "codex",
        "--reason",
        "Not this recipient.",
        "--json",
    ]);
    assert!(!out.status.success());
    let out = f.call(&[
        "memory",
        "choose",
        "worker",
        "--task",
        TASK,
        "--host",
        "claude",
        "--reason",
        "Wrong primary host.",
        "--json",
    ]);
    assert!(!out.status.success());
}
#[test]
fn selected_source_guard_revocation_and_profile_drift_still_refuse() {
    for change in ["source", "guard", "revoke", "profile"] {
        let f = local();
        f.accept_lesson();
        let id = item(&f, "worker", TASK);
        choose(&f, "worker", TASK, &[&id], "absent");
        let a = f.worker_unmarked(TASK);
        match change {
            "source" => fs::write(f.root.join("lesson.json"), b"{}").unwrap(),
            "guard" => fs::write(f.root.join("guard"), b"changed").unwrap(),
            "revoke" => {
                f.ok(&["memory", "revoke", "lead", LEDGER]);
            }
            _ => {
                let profile = f.config()["agents"]["worker"]["profile"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                fs::write(f.root.join(profile), b"Changed profile").unwrap();
            }
        }
        stale(&f, &a);
    }
}
#[test]
fn corrupt_missing_or_aliased_choice_never_hot_swaps() {
    for mode in ["missing", "corrupt", "symlink", "hardlink"] {
        let f = local();
        let p = preview(&f, "worker", TASK, &[], "absent", "Explicit empty choice.");
        assert!(apply(&f, &p).status.success());
        let a = f.worker(TASK);
        let path = f.root.join(p["artifact"].as_str().unwrap());
        match mode {
            "missing" => fs::remove_file(&path).unwrap(),
            "corrupt" => fs::write(&path, b"{}").unwrap(),
            "symlink" => {
                fs::remove_file(&path).unwrap();
                std::os::unix::fs::symlink(f.root.join("current-task.json"), &path).unwrap();
            }
            _ => {
                fs::hard_link(&path, f.root.join("choice-alias")).unwrap();
            }
        }
        stale(&f, &a);
    }
}
#[test]
fn unselected_owned_draft_does_not_replace_the_adopted_choice() {
    let f = local();
    f.accept_lesson();
    let id = item(&f, "worker", TASK);
    choose(&f, "worker", TASK, &[&id], "absent");
    let a = f.worker(TASK);
    let before = packet(&f, &a);
    let mut draft = f.lesson();
    draft["id"] = json!("unselected-draft");
    f.write(".exitbind/artifacts/draft.json", &draft);
    f.ok(&[
        "memory",
        "propose",
        "lead",
        ".exitbind/artifacts/draft.json",
        "--scope",
        "project-lessons.v1",
        "--ledger",
        ".exitbind/memory/draft.jsonl",
        "--expires-at",
        "2100-01-01T00:00:00Z",
    ]);
    let after = packet(&f, &a);
    assert_eq!(before["memoryReferences"], after["memoryReferences"]);
    assert_eq!(before["context"]["digest"], after["context"]["digest"]);
}
#[test]
fn specialist_route_and_fresh_child_keep_a_after_b_preparation() {
    let f = specialist();
    let id = item(&f, "record-specialist", SPECIALIST_TASK);
    choose(&f, "record-specialist", SPECIALIST_TASK, &[&id], "absent");
    let a = f.worker(SPECIALIST_TASK);
    f.bind_parent(&a);
    let btask = "Independent quoted aggregation formatting";
    choose(&f, "record-specialist", btask, &[], "absent");
    let _b = f.worker(btask);
    let route = f.ok(&[
        "project",
        "route",
        "--task",
        SPECIALIST_TASK,
        "--purpose",
        f.config()["agents"]["record-specialist"]["purpose"]
            .as_str()
            .unwrap(),
        "--role",
        "worker",
        "--host",
        "codex",
        "--agent",
        "record-specialist",
        "--json",
    ]);
    assert!(route.to_string().contains(SPECIALIST_GUIDANCE), "{route}");
    f.ok(&["work", "focus", &a]);
    let child = f.prepare_child(&a);
    assert!(child.status.success(), "{child:?}");
    let child: Value = serde_json::from_slice(&child.stdout).unwrap();
    let context = f.ok(&[
        "work",
        "child",
        "context",
        &a,
        child["intent"].as_str().unwrap(),
    ]);
    assert!(
        context.to_string().contains(SPECIALIST_GUIDANCE),
        "{context}"
    );
    assert!(!f
        .ok(&[
            "project",
            "route",
            "--task",
            btask,
            "--purpose",
            f.config()["agents"]["record-specialist"]["purpose"]
                .as_str()
                .unwrap(),
            "--role",
            "worker",
            "--host",
            "codex",
            "--agent",
            "record-specialist",
            "--json"
        ])
        .to_string()
        .contains(SPECIALIST_GUIDANCE));
}

#[test]
fn wrong_host_child_preparation_refuses_before_intent_and_stale_claim_is_inert() {
    let f = specialist();
    let id = item(&f, "record-specialist", SPECIALIST_TASK);
    choose(&f, "record-specialist", SPECIALIST_TASK, &[&id], "absent");
    let a = f.worker(SPECIALIST_TASK);
    f.bind_parent(&a);
    let prepared = f.prepare_child(&a);
    assert!(prepared.status.success());
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let token = f.child_token(&a);
    f.ok(&[
        "work",
        "bind",
        &a,
        "--context",
        &token,
        "--host",
        "claude",
        "--session",
        "other-host",
        "--host-version",
        "fixture",
    ]);
    let intents = f.root.join(".exitbind/child-intents.json");
    let before = fs::read(&intents).unwrap();
    let bad = f.prepare_child(&a);
    assert!(!bad.status.success(), "{bad:?}");
    assert!(diagnostic(&bad).contains("actual host differs"), "{bad:?}");
    assert_eq!(fs::read(&intents).unwrap(), before);
    let old = f.call(&[
        "work",
        "child",
        "context",
        &a,
        prepared["intent"].as_str().unwrap(),
    ]);
    assert!(!old.status.success());
    assert!(!diagnostic(&old).contains(SPECIALIST_GUIDANCE));
    let hook = f.child_start("stale-host-child");
    assert!(!hook.to_string().contains(SPECIALIST_GUIDANCE));
}
#[test]
fn copied_record_wrong_task_or_project_and_invalid_policy_refuse() {
    let f = local();
    let p = preview(&f, "worker", TASK, &[], "absent", "No adoption.");
    assert!(apply(&f, &p).status.success());
    let g = local();
    let target = preview(&g, "worker", TASK, &[], "absent", "No adoption.");
    let dst = g.root.join(target["artifact"].as_str().unwrap());
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    fs::copy(f.root.join(p["artifact"].as_str().unwrap()), &dst).unwrap();
    let bad = g.call(&["memory", "choice", "worker", "--task", TASK, "--json"]);
    assert!(!bad.status.success());
    assert!(diagnostic(&bad).contains("ownership"));
    let other = preview(
        &f,
        "worker",
        "Different approved task",
        &[],
        "absent",
        "Another choice.",
    );
    let dst = f.root.join(other["artifact"].as_str().unwrap());
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    fs::copy(f.root.join(p["artifact"].as_str().unwrap()), &dst).unwrap();
    assert!(!f
        .call(&[
            "memory",
            "choice",
            "worker",
            "--task",
            "Different approved task",
            "--json"
        ])
        .status
        .success());
    let mut c = f.config();
    c["memory"]["contextual"]["sourcePath"] = json!("unowned/choices");
    f.save(&c);
    assert!(!f
        .call(&["memory", "choice", "worker", "--task", TASK, "--json"])
        .status
        .success());
}
#[test]
fn selected_expiry_is_current_even_with_an_unchanged_frozen_record() {
    let f = local();
    f.write("lesson.json", &f.lesson());
    let expiry = chrono::Utc::now() + chrono::Duration::seconds(8);
    f.ok(&[
        "memory",
        "propose",
        "lead",
        "lesson.json",
        "--scope",
        "project-lessons.v1",
        "--ledger",
        LEDGER,
        "--expires-at",
        &expiry.to_rfc3339(),
    ]);
    f.ok(&["memory", "review", "lead", LEDGER]);
    f.ok(&["memory", "promote", "lead", LEDGER]);
    let id = item(&f, "worker", TASK);
    let chosen = choose(&f, "worker", TASK, &[&id], "absent");
    let a = f.worker_unmarked(TASK);
    assert!(
        chrono::Utc::now() < expiry,
        "fixture must freeze before real expiry"
    );
    while chrono::Utc::now() <= expiry {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    stale(&f, &a);
    let read = f.ok(&["memory", "choice", "worker", "--task", TASK, "--json"]);
    assert_eq!(read["contextSha256"], chosen["contextSha256"]);
    assert_eq!(read["currentlyUsable"], false);
}
#[test]
fn valid_record_aliases_and_aliased_ancestors_are_refused_before_read() {
    for parent in [false, true] {
        let f = local();
        let p = preview(&f, "worker", TASK, &[], "absent", "Explicit empty.");
        assert!(apply(&f, &p).status.success());
        let a = f.worker(TASK);
        let path = f.root.join(p["artifact"].as_str().unwrap());
        if parent {
            let dir = path.parent().unwrap();
            let aside = dir.with_extension("aside");
            fs::rename(dir, &aside).unwrap();
            std::os::unix::fs::symlink(&aside, dir).unwrap();
        } else {
            let aside = f.root.join(".exitbind/artifacts/valid-choice.json");
            fs::rename(&path, &aside).unwrap();
            std::os::unix::fs::symlink(&aside, &path).unwrap();
        }
        stale(&f, &a);
    }
}

#[test]
fn simultaneous_same_choice_apply_has_one_record_and_exact_recovery() {
    let f = local();
    let a = preview(
        &f,
        "worker",
        TASK,
        &[],
        "absent",
        "First simultaneous decision.",
    );
    let b = preview(
        &f,
        "worker",
        TASK,
        &[],
        "absent",
        "Second simultaneous decision.",
    );
    let outcomes = std::thread::scope(|s| {
        let x = s.spawn(|| apply(&f, &a));
        let y = s.spawn(|| apply(&f, &b));
        vec![x.join().unwrap(), y.join().unwrap()]
    });
    assert_eq!(
        outcomes.iter().filter(|o| o.status.success()).count(),
        1,
        "{outcomes:?}"
    );
    let current = f.ok(&["memory", "choice", "worker", "--task", TASK, "--json"]);
    assert_eq!(current["record"]["sequence"], 1);
    let winner = if outcomes[0].status.success() { &a } else { &b };
    let replay = apply(&f, winner);
    assert!(replay.status.success());
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["contextSha256"], current["contextSha256"]);
}
#[test]
fn namespace_capacity_serializes_competing_distinct_keys_and_history_is_bounded() {
    let f = specialist();
    let a = preview(
        &f,
        "record-specialist",
        TASK,
        &[],
        "absent",
        "Capacity race A.",
    );
    let b = preview(
        &f,
        "record-specialist",
        "Capacity task B",
        &[],
        "absent",
        "Capacity race B.",
    );
    let root = f.root.join(".exitbind/artifacts/memory-choices");
    fs::create_dir_all(&root).unwrap();
    for i in 0..127 {
        fs::create_dir(root.join(format!("{i:064x}"))).unwrap();
    }
    // Hold the exact allocation owner independently of the conditional lessons lock.
    // Removing allocation serialization would incorrectly create a record here.
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
    let allocation = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(f.root.join(".exitbind/locks/memory-choice-allocation.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(allocation.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let denied = apply(&f, &a);
    assert!(!denied.status.success(), "{denied:?}");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 127);
    drop(allocation);
    let outcomes = std::thread::scope(|s| {
        let x = s.spawn(|| apply(&f, &a));
        let y = s.spawn(|| apply(&f, &b));
        vec![x.join().unwrap(), y.join().unwrap()]
    });
    assert_eq!(fs::read_dir(&root).unwrap().count(), 128, "{outcomes:?}");
    assert_eq!(
        outcomes.iter().filter(|o| o.status.success()).count(),
        1,
        "{outcomes:?}"
    );
    let g = local();
    let first = preview(&g, "worker", TASK, &[], "absent", "Initial bounded choice.");
    assert!(apply(&g, &first).status.success());
    let path = g.root.join(first["artifact"].as_str().unwrap());
    // Unsupported oversized histories fail before any record is parsed or effect occurs.
    for i in 2..=65 {
        fs::write(path.parent().unwrap().join(format!("{i:04}.json")), b"{}").unwrap();
    }
    let out = g.call(&["memory", "choice", "worker", "--task", TASK, "--json"]);
    assert!(!out.status.success());
    assert!(diagnostic(&out).contains("exceeds 64 revisions"));
}
#[test]
fn changed_valid_record_cannot_replay_an_old_request_token() {
    let f = local();
    let p = preview(&f, "worker", TASK, &[], "absent", "Original reason.");
    assert!(apply(&f, &p).status.success());
    let path = f.root.join(p["artifact"].as_str().unwrap());
    let mut v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    v["reason"] = json!("Altered valid reason.");
    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let out = apply(&f, &p);
    assert!(!out.status.success());
    assert!(diagnostic(&out).contains("request does not bind"));
}

#[test]
fn marked_source_guard_revocation_and_profile_changes_keep_the_work_readable() {
    for change in ["source", "guard", "revoke", "profile"] {
        let f = local();
        f.accept_lesson();
        let id = item(&f, "worker", TASK);
        choose(&f, "worker", TASK, &[&id], "absent");
        let a = f.worker(TASK);
        match change {
            "source" => fs::write(f.root.join("lesson.json"), b"{}").unwrap(),
            "guard" => fs::write(f.root.join("guard"), b"changed").unwrap(),
            "revoke" => {
                f.ok(&["memory", "revoke", "lead", LEDGER]);
            }
            _ => {
                let profile = f.config()["agents"]["worker"]["profile"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                fs::write(f.root.join(profile), b"Changed profile").unwrap();
            }
        }
        let detail = f.ok(&["work", "detail", &a, "--json"]);
        // Advisory changes never invalidate the Work or its current action.
        assert_eq!(detail["valid"], true, "{change}: {detail}");
        assert!(
            !detail["effectiveAction"]["kind"].is_null(),
            "{change}: {detail}"
        );
        assert!(
            !detail["recipientContext"].to_string().contains(FACT),
            "{change}: {detail}"
        );
        if matches!(change, "source" | "guard") {
            assert_eq!(
                detail["recipientContext"]["available"], true,
                "{change}: {detail}"
            );
        }
    }
}

#[test]
fn marked_selected_expiry_keeps_the_work_valid_and_omits_the_expired_item() {
    let f = local();
    f.write("lesson.json", &f.lesson());
    let expiry = chrono::Utc::now() + chrono::Duration::seconds(8);
    f.ok(&[
        "memory",
        "propose",
        "lead",
        "lesson.json",
        "--scope",
        "project-lessons.v1",
        "--ledger",
        LEDGER,
        "--expires-at",
        &expiry.to_rfc3339(),
    ]);
    f.ok(&["memory", "review", "lead", LEDGER]);
    f.ok(&["memory", "promote", "lead", LEDGER]);
    let id = item(&f, "worker", TASK);
    let chosen = choose(&f, "worker", TASK, &[&id], "absent");
    let a = f.worker(TASK);
    assert!(
        chrono::Utc::now() < expiry,
        "fixture must freeze before real expiry"
    );
    while chrono::Utc::now() <= expiry {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let detail = f.ok(&["work", "detail", &a, "--json"]);
    assert_eq!(detail["recipientContext"]["available"], true, "{detail}");
    assert!(!detail["recipientContext"].to_string().contains(FACT));
    let read = f.ok(&["memory", "choice", "worker", "--task", TASK, "--json"]);
    assert_eq!(read["contextSha256"], chosen["contextSha256"]);
    assert_eq!(read["currentlyUsable"], false);
}
