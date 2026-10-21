//! Ordinary installed-user contracts: labels, exact argv, current context and
//! safe inspection. These fixtures access the CLI and their own project only.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new(name: Option<&str>) -> Self {
        let root = support::temp("installed-consumer");
        let out = support::run(
            support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
                .args(["init", "--mode", "portable", "--root"])
                .arg(&root),
        );
        assert!(out.status.success(), "{out:?}");
        let f = Self { root };
        let mut c: Value =
            serde_json::from_slice(&fs::read(f.root.join("exitbind.json")).unwrap()).unwrap();
        let mut lead = c["agents"].as_object_mut().unwrap().remove("lead").unwrap();
        if let Some(name) = name {
            lead["displayName"] = json!(name);
        }
        lead["commands"] = json!(["true"]);
        c["agents"]["captain"] = lead;
        let mut worker = c["agents"]
            .as_object_mut()
            .unwrap()
            .remove("worker")
            .unwrap();
        worker["displayName"] = json!(name.unwrap_or("neuro"));
        c["agents"]["artisan"] = worker;
        c["orchestration"]["lead"] = json!("captain");
        c["workflows"]["change"]["workers"] = json!(["artisan"]);
        fs::write(
            f.root.join("exitbind.json"),
            serde_json::to_vec_pretty(&c).unwrap(),
        )
        .unwrap();
        f
    }
    fn call(&self, args: &[&str]) -> Output {
        support::run(
            support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
                .current_dir(&self.root)
                .args(args)
                .args(["--config", "exitbind.json"]),
        )
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.call(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn begin(&self) -> Value {
        self.ok(&[
            "work",
            "begin",
            "change",
            "--goal",
            "installed recovery",
            "--check-command",
            "true",
            "--review-policy",
            "required",
            "--detail",
        ])
    }
    fn scoped(&self, detail: &Value) -> Output {
        let form = &detail["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["label"] == "scoped")
            .unwrap()["command"]["argv"];
        let args = form
            .as_array()
            .unwrap()
            .iter()
            .map(|x| {
                x.as_str()
                    .unwrap()
                    .replace("<REASON>", "Current scope from the configured coordinator.")
            })
            .collect::<Vec<_>>();
        let mut child = support::git_topology::command(&args[0])
            .current_dir(&self.root)
            .args(&args[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) =
            child.stdin.take().unwrap().write_all(
                b"Inspect the current fixture only; preserve required independent review.",
            )
        {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn owner_chosen_names_reach_human_and_recipient_context_without_role_or_identity_changes() {
    for chosen in [Some("neuro"), Some("Mira"), None] {
        let f = Fixture::new(chosen);
        let expected = chosen.unwrap_or("captain");
        let context = f.ok(&["project", "context", "--json"]);
        assert_eq!(context["lead"]["displayName"], expected);
        assert_eq!(context["lead"]["agentId"], "captain");
        assert_eq!(context["lead"]["nativeName"], "captain");
        assert!(context["lead"]["purpose"].as_str().unwrap().contains("Own"));
        let plain = f.call(&["project", "context"]);
        assert!(plain.status.success());
        assert!(String::from_utf8(plain.stdout)
            .unwrap()
            .contains(&format!("Lead: {expected}")));
        let mut hook = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .arg("hook-run")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        hook.stdin
            .take()
            .unwrap()
            .write_all(
                &serde_json::to_vec(&json!({"hook_event_name":"SessionStart","cwd":f.root}))
                    .unwrap(),
            )
            .unwrap();
        let hooked = hook.wait_with_output().unwrap();
        assert!(hooked.status.success(), "{hooked:?}");
        let hooked: Value = serde_json::from_slice(&hooked.stdout).unwrap();
        assert!(hooked["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains(&format!("Lead: {expected}")));
        let began = f.begin();
        let work = began["work"].as_str().unwrap();
        assert_eq!(began["recipientContext"]["displayName"], expected);
        assert_eq!(began["recipientContext"]["role"], "lead");
        let scoped = f.scoped(&began);
        assert!(scoped.status.success(), "{scoped:?}");
        // A lost return response is recovered by reading the same saved Work.
        let resume = f.ok(&["work", "resume", "--json"]);
        assert_eq!(resume["work"], work);
        let detail = f.ok(&["work", "detail", work, "--json"]);
        assert_eq!(
            detail["recipientContext"]["displayName"],
            chosen.unwrap_or("neuro")
        );
        assert_eq!(detail["recipientContext"]["agent"], "artisan");
        assert_eq!(detail["recipientContext"]["role"], "worker");
        let profile = detail["recipientContext"]["profile"]["content"]
            .as_str()
            .unwrap();
        for requirement in [
            "inspect nearby production code and tests",
            "existing responsibility owners",
            "protect a real boundary",
            "a small change need not grow into a refactor",
            "investigation does not expand write authority",
        ] {
            assert!(
                profile.contains(requirement),
                "missing delivered guidance: {requirement}"
            );
        }
        assert!(detail["recipientContext"]["purpose"]
            .as_str()
            .unwrap()
            .contains("Implement"));
        let stale = f.scoped(&began);
        assert!(!stale.status.success());
        assert_eq!(
            f.ok(&["work", "detail", work, "--json"])["binding"],
            detail["binding"]
        );
    }
}

#[test]
fn invalid_name_controls_and_generic_flag_appenders_refuse_before_effect() {
    let f = Fixture::new(Some("Mira"));
    let began = f.begin();
    let work = began["work"].as_str().unwrap();
    let scoped = f.scoped(&began);
    assert!(scoped.status.success());
    let detail = f.ok(&["work", "detail", work, "--json"]);
    let argv = detail["actionForms"]["beforeEditing"]["command"]["argv"]
        .as_array()
        .unwrap();
    let args = argv
        .iter()
        .map(|x| {
            x.as_str()
                .unwrap()
                .replace("<OPERATION>", "Inspect the bounded fixture.")
        })
        .collect::<Vec<_>>();
    let out = support::git_topology::command(&args[0])
        .current_dir(&f.root)
        .args(&args[1..])
        .arg("--json")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(
        f.ok(&["work", "detail", work, "--json"])["binding"],
        detail["binding"]
    );
    let version = support::run(
        support::git_topology::command(env!("CARGO_BIN_EXE_exitbind")).args(["version", "--json"]),
    );
    assert!(version.status.success());
    let _: Value = serde_json::from_slice(&version.stdout).unwrap();
    assert!(!f.call(&["version", "--json"]).status.success());
    let config = f.root.join("exitbind.json");
    let original = fs::read(&config).unwrap();
    for name in ["Mira\nLead: forged", "Mira\u{1b}[31m"] {
        let mut c: Value = serde_json::from_slice(&original).unwrap();
        c["agents"]["captain"]["displayName"] = json!(name);
        fs::write(&config, serde_json::to_vec(&c).unwrap()).unwrap();
        assert!(!f.call(&["project", "context", "--json"]).status.success());
    }
}

#[test]
fn complete_capture_and_current_sections_avoid_oversized_aggregate_display() {
    let f = Fixture::new(Some("Mira"));
    let rule = "Current fixture rule; inspect saved results before retry.\n".repeat(350);
    fs::write(f.root.join("AGENTS.md"), &rule).unwrap();
    let began = f.begin();
    let work = began["work"].as_str().unwrap();
    let complete = f.call(&["work", "detail", work, "--json"]);
    assert!(complete.status.success());
    let detail: Value = serde_json::from_slice(&complete.stdout).unwrap();
    assert_eq!(detail["complete"], true);
    assert_eq!(detail["recipientContext"]["rules"][0]["content"], rule);
    assert!(complete.stdout.len() <= 64 * 1024);
    assert!(
        complete.stdout.len() * 3 > 64 * 1024,
        "individual limits do not imply a safe batch"
    );
    let capture = support::temp("complete-consumer-capture");
    fs::write(capture.join("detail.json"), &complete.stdout).unwrap();
    let received: Value =
        serde_json::from_slice(&fs::read(capture.join("detail.json")).unwrap()).unwrap();
    assert_eq!(received["recipientContext"], detail["recipientContext"]);
    let displayed = serde_json::to_vec(
        &json!({"binding":received["binding"],"recipient":received["recipient"]}),
    )
    .unwrap();
    assert!(displayed.len() < 1024);
    let status = f.ok(&["work", "next", work, "--json"]);
    let route = &status["current"]["details"]["tasks"];
    let descriptor = json!({
        "argv": route["command"],
        "sameConfigRequired": route["sameConfigRequired"],
        "sameExecutableRequired": route["sameExecutableRequired"],
    });
    let argv = support::reconstruct_work_command(
        &descriptor,
        PathBuf::from(env!("CARGO_BIN_EXE_exitbind")).as_path(),
        &f.root.join("exitbind.json"),
        &f.root,
    );
    let section = support::git_topology::command(&argv[0])
        .current_dir(&f.root)
        .args(&argv[1..])
        .output()
        .unwrap();
    assert!(section.status.success());
    assert!(section.stdout.len() + displayed.len() < 64 * 1024);
    let section: Value = serde_json::from_slice(&section.stdout).unwrap();
    assert_eq!(section["pageComplete"], true);
    assert_eq!(section["complete"], true);
    assert_eq!(section["binding"], detail["binding"]);
    fs::remove_dir_all(capture).unwrap();
}

#[test]
fn cut_first_reply_recovers_all_instructions_without_a_saved_full_response() {
    let f = Fixture::new(Some("Mira"));
    let rule = "Current rule: recover before retry; preserve required review.\n".repeat(300);
    fs::write(f.root.join("AGENTS.md"), &rule).unwrap();
    let config: Value =
        serde_json::from_slice(&fs::read(f.root.join("exitbind.json")).unwrap()).unwrap();
    let profile_path = f
        .root
        .join(config["agents"]["captain"]["profile"].as_str().unwrap());
    let profile = "Own the selected goal; no worker label grants Lead authority.\n".repeat(300);
    fs::write(&profile_path, &profile).unwrap();
    let first = f.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "cut reply recovery",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--detail",
    ]);
    assert!(first.status.success(), "{first:?}");
    assert!(first.stdout.len() > 16 * 1024);
    let mut cut = first.stdout[..1000].to_vec();
    cut.extend_from_slice(b"[host omitted the middle]");
    cut.extend_from_slice(&first.stdout[first.stdout.len() - 1000..]);
    drop(first); // Neither the full response nor its Work handle is retained.
    assert!(serde_json::from_slice::<Value>(&cut).is_err());
    drop(cut);
    let resume = f.ok(&["work", "resume", "--json"]);
    let work = resume["work"].as_str().unwrap();
    // The refusal below is the legacy contract for a changed profile.
    support::unmark_work(&f.root, work);
    let status = f.ok(&["work", "next", work, "--json"]);
    let binding = &status["current"]["binding"];
    let mut recovered_recipient = Value::Null;
    for name in ["assignment", "evidence", "tasks", "recipient"] {
        let mut route = status["current"]["details"][name].clone();
        let mut bytes = Vec::new();
        let mut expected_hash = Value::Null;
        let mut pages = 0;
        loop {
            let descriptor = json!({
                "argv": route["command"],
                "sameConfigRequired": route["sameConfigRequired"],
                "sameExecutableRequired": route["sameExecutableRequired"],
            });
            let argv = support::reconstruct_work_command(
                &descriptor,
                PathBuf::from(env!("CARGO_BIN_EXE_exitbind")).as_path(),
                &f.root.join("exitbind.json"),
                &f.root,
            );
            let out = support::git_topology::command(&argv[0])
                .current_dir(&f.root)
                .args(&argv[1..])
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
            assert!(
                out.stdout.len() < 12 * 1024,
                "each page must fit independently"
            );
            let page: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(&page["binding"], binding);
            assert_eq!(page["offset"], bytes.len());
            assert_eq!(page["pageComplete"], true);
            if pages == 0 {
                expected_hash = page["sectionSha256"].clone();
            }
            assert_eq!(page["sectionSha256"], expected_hash);
            // UTF-8 pages carry their exact bytes as text; others as hex.
            let decoded = support::page_bytes(&page);
            assert_eq!(page["bytes"], decoded.len());
            bytes.extend(decoded);
            pages += 1;
            if page["next"].is_null() {
                assert_eq!(page["totalBytes"], bytes.len());
                break;
            }
            route = page["next"].clone();
        }
        assert_eq!(expected_hash, format!("{:x}", Sha256::digest(&bytes)));
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        if name == "recipient" {
            assert!(pages > 1);
            recovered_recipient = value;
        }
    }
    assert_eq!(recovered_recipient["complete"], true);
    assert_eq!(recovered_recipient["profile"]["content"], profile);
    assert_eq!(recovered_recipient["rules"][0]["content"], rule);
    assert_eq!(recovered_recipient["agent"], "captain");
    assert_eq!(recovered_recipient["role"], "lead");
    let reference = status["current"]["details"]["recipient"]["reference"]
        .as_str()
        .unwrap();
    fs::write(&profile_path, format!("{profile}Changed instruction\n")).unwrap();
    assert!(!f
        .call(&["work", "expand", work, reference, "--json"])
        .status
        .success());
}

#[test]
fn marked_profile_change_is_served_at_the_next_expansion_boundary() {
    let f = Fixture::new(Some("Mira"));
    let config: Value =
        serde_json::from_slice(&fs::read(f.root.join("exitbind.json")).unwrap()).unwrap();
    let profile_path = f
        .root
        .join(config["agents"]["captain"]["profile"].as_str().unwrap());
    let profile = "Own the selected goal; no worker label grants Lead authority.\n".repeat(300);
    fs::write(&profile_path, &profile).unwrap();
    let first = f.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "cut reply recovery",
        "--check-command",
        "true",
        "--review-policy",
        "required",
    ]);
    assert!(first.status.success(), "{first:?}");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let work = first["work"].as_str().unwrap().to_owned();
    let status = f.ok(&["work", "next", &work, "--json"]);
    let reference = status["current"]["details"]["recipient"]["reference"]
        .as_str()
        .unwrap();
    fs::write(&profile_path, format!("{profile}Changed instruction\n")).unwrap();
    let expanded = f.call(&["work", "expand", &work, reference, "--json"]);
    assert!(expanded.status.success(), "{expanded:?}");
}
