//! Approved capabilities remain project-bound and current at the next launch.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
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
    fn new() -> Self {
        let root = support::temp("approved-capability");
        support::git_topology::repository(&root);
        let out = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let f = Self { root };
        let mut c: Value =
            serde_json::from_slice(&fs::read(f.root.join("exitbind.json")).unwrap()).unwrap();
        let mut agent = c["agents"]
            .as_object_mut()
            .unwrap()
            .remove("worker")
            .unwrap();
        agent["nativeName"] = json!("parcel_auditor");
        agent["runtime"]["host"] = json!("codex");
        c["agents"]["parcel-checker"] = agent;
        c["workflows"]["change"]["workers"] = json!(["parcel-checker"]);
        fs::write(f.root.join("exitbind.json"), c.to_string()).unwrap();
        fs::write(f.root.join("custom-agent.md"), "Keep this custom file.\n").unwrap();
        fs::write(
            f.root.join("lens.md"),
            "Report unavailable values separately from measured zero.\n",
        )
        .unwrap();
        let identity =
            f.ok(&["project", "context", "--json"], b"")["project"]["memoryIdentity"].clone();
        let d = json!({"version":1,"id":"availability","revision":"r1","kind":"lens","owner":"lead","projectIdentity":identity,
            "path":"lens.md","roles":["worker"],"taskTerms":["parcel"],"hosts":["codex"],"expiresAt":"2100-01-01T00:00:00Z","revoked":false});
        fs::write(f.root.join("lens.json"), d.to_string()).unwrap();
        f
    }
    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], input: &[u8]) -> Value {
        let out = self.call(args, input);
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn register(&self) -> Value {
        let p = self.ok(
            &[
                "project",
                "capabilities",
                "select",
                "lens.json",
                "--decision",
                "reviewed",
                "--reason",
                "Owner approved parcel availability lens",
            ],
            b"",
        );
        self.ok(
            &[
                "project",
                "capabilities",
                "select",
                "lens.json",
                "--decision",
                "reviewed",
                "--reason",
                "Owner approved parcel availability lens",
                "--current-binding",
                p["currentBinding"].as_str().unwrap(),
                "--apply",
            ],
            b"",
        )
    }
    fn prepare(&self) -> (String, String) {
        let b = self.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Check parcel availability",
                "--check-command",
                "true",
                "--preservation-check-command",
                "true",
                "--preserve-requirement",
                "availability:Check parcel availability",
                "--review-policy",
                "required",
            ],
            b"",
        );
        let w = b["work"].as_str().unwrap();
        let next = self.ok(&["work", "next", w, "--full"], b"");
        self.ok(
            &[
                "work",
                "return",
                w,
                next["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            b"Check parcel values\n",
        );
        self.ok(&["work","record",w],json!({"action":"init","sourceRef":"owner:request","sourceText":"Check parcel availability","requirements":[{"id":"availability","text":"Check parcel availability"}]}).to_string().as_bytes());
        let t = self.ok(&["work", "continuation", w], b"")["mutationContext"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
        self.ok(
            &[
                "work",
                "bind",
                w,
                "--context",
                &t,
                "--host",
                "codex",
                "--session",
                "parent",
                "--host-version",
                "0.162.1",
            ],
            b"",
        );
        let t = self.ok(&["work", "continuation", w], b"")["mutationContext"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let wrong = self.call(
            &[
                "work",
                "child",
                "prepare",
                w,
                "wrong-child",
                "--context",
                &t,
                "--agent-type",
                "reviewer",
            ],
            b"",
        );
        assert!(!wrong.status.success());
        let prepared = self.ok(
            &[
                "work",
                "child",
                "prepare",
                w,
                "parcel-task",
                "--context",
                &t,
                "--agent-type",
                "parcel_auditor",
            ],
            b"",
        );
        (
            w.to_owned(),
            prepared["intent"].as_str().unwrap().to_owned(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn user_owned_profiles_and_lens_use_confined_sources_without_migration() {
    let f = Fixture::new();
    let old_lead = fs::read(f.root.join("exitbind/agents/lead.md")).unwrap();
    let old_worker = fs::read(f.root.join("exitbind/agents/worker.md")).unwrap();
    let profile = "your.soulmate/agents/owned-checker/profile.md";
    let lead = "your.soulmate/agents/my-lead/profile.md";
    let lens = "your.soulmate/lenses/availability.md";
    for path in [profile, lead, lens] {
        fs::create_dir_all(f.root.join(path).parent().unwrap()).unwrap();
    }
    fs::write(
        f.root.join(profile),
        "Owner checker: preserve unknown values.\n",
    )
    .unwrap();
    fs::write(
        f.root.join(lead),
        "Owner lead: own the acceptance decision.\n",
    )
    .unwrap();
    fs::write(
        f.root.join(lens),
        "Distinguish missing values from measured zero.\n",
    )
    .unwrap();
    let mut config: Value =
        serde_json::from_slice(&fs::read(f.root.join("exitbind.json")).unwrap()).unwrap();
    config["agents"]["parcel-checker"]["profile"] = json!(profile);
    config["agents"]["lead"]["profile"] = json!(lead);
    config["agents"]["lead"]["displayName"] = json!("Owner lead");
    fs::write(f.root.join("exitbind.json"), config.to_string()).unwrap();
    let mut descriptor: Value =
        serde_json::from_slice(&fs::read(f.root.join("lens.json")).unwrap()).unwrap();
    descriptor["path"] = json!(lens);
    fs::write(f.root.join("lens.json"), descriptor.to_string()).unwrap();
    assert_eq!(f.register()["effect"], "applied");
    let context = f.ok(&["project", "context", "--json"], b"");
    assert_eq!(context["lead"]["agentId"], "lead");
    assert_eq!(context["lead"]["displayName"], "Owner lead");
    assert_eq!(context["lead"]["profilePath"], lead);
    let (work, intent) = f.prepare();
    let detail = f.ok(&["work", "detail", &work, "--json"], b"");
    assert_eq!(detail["recipientContext"]["profile"]["path"], profile);
    assert_eq!(
        detail["recipientContext"]["profile"]["content"],
        "Owner checker: preserve unknown values.\n"
    );
    let delivery = f.ok(&["work", "child", "context", &work, &intent], b"");
    assert_eq!(
        delivery["capabilities"]["items"][0]["content"],
        "Distinguish missing values from measured zero.\n"
    );
    assert_eq!(
        fs::read(f.root.join("exitbind/agents/lead.md")).unwrap(),
        old_lead
    );
    assert_eq!(
        fs::read(f.root.join("exitbind/agents/worker.md")).unwrap(),
        old_worker
    );
    assert_eq!(
        fs::read_to_string(f.root.join("custom-agent.md")).unwrap(),
        "Keep this custom file.\n"
    );

    fs::write(f.root.join(profile), "Changed owner source.\n").unwrap();
    assert!(!f
        .call(
            &[
                "work",
                "expand",
                &work,
                detail["reference"].as_str().unwrap()
            ],
            b""
        )
        .status
        .success());
    fs::write(f.root.join(lens), "Changed without renewed approval.\n").unwrap();
    assert!(!f
        .call(&["work", "child", "context", &work, &intent], b"")
        .status
        .success());
    fs::remove_file(f.root.join(profile)).unwrap();
    std::os::unix::fs::symlink(f.root.join("custom-agent.md"), f.root.join(profile)).unwrap();
    assert!(!f.call(&["work", "detail", &work], b"").status.success());
}

#[test]
fn reviewed_lens_selects_arbitrary_recipient_and_recovers_delivery_without_another_effect() {
    let f = Fixture::new();
    let old = fs::read(f.root.join("exitbind.json")).unwrap();
    assert!(!f
        .call(
            &["project", "capabilities", "select", "lens.json", "--apply"],
            b""
        )
        .status
        .success());
    assert_eq!(fs::read(f.root.join("exitbind.json")).unwrap(), old);
    assert_eq!(f.register()["effect"], "applied");
    let registered = fs::read(f.root.join("exitbind.json")).unwrap();
    assert_eq!(f.register()["state"], "unchanged");
    assert_eq!(fs::read(f.root.join("exitbind.json")).unwrap(), registered);
    let s = f.ok(
        &[
            "project",
            "capabilities",
            "--task",
            "Check parcel availability",
            "--host",
            "codex",
        ],
        b"",
    );
    assert_eq!(
        s["resources"][0]["recipients"],
        json!([{"agent":"parcel-checker","nativeName":"parcel_auditor","state":"pending next launch"}])
    );
    for args in [
        [
            "project",
            "capabilities",
            "--task",
            "unrelated",
            "--host",
            "codex",
        ],
        [
            "project",
            "capabilities",
            "--task",
            "parcel",
            "--host",
            "claude",
        ],
    ] {
        assert_eq!(f.ok(&args, b"")["resources"][0]["recipients"], json!([]));
    }
    assert_eq!(s["acknowledged"], false);
    assert_eq!(s["observedUse"], false);
    let (w, i) = f.prepare();
    let one = f.ok(&["work", "child", "context", &w, &i], b"");
    let two = f.ok(&["work", "child", "context", &w, &i], b"");
    assert_eq!(one["capabilities"], two["capabilities"]);
    assert_eq!(one["capabilities"]["recipient"], "parcel-checker");
    assert!(one["capabilities"]["items"][0]["content"]
        .as_str()
        .unwrap()
        .contains("measured zero"));
    assert_eq!(
        fs::read_to_string(f.root.join("custom-agent.md")).unwrap(),
        "Keep this custom file.\n"
    );
    fs::write(f.root.join("lens.md"), "Changed without review\n").unwrap();
    assert!(!f
        .call(&["work", "child", "context", &w, &i], b"")
        .status
        .success());
}

#[test]
fn expired_revoked_and_other_project_resources_never_deliver() {
    for (key, value, state) in [
        ("revoked", json!(true), "revoked"),
        ("expiresAt", json!("2000-01-01T00:00:00Z"), "expired"),
    ] {
        let f = Fixture::new();
        let p = f.root.join("lens.json");
        let mut d: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        d[key] = value;
        fs::write(p, d.to_string()).unwrap();
        f.register();
        let status = f.ok(&["project", "capabilities", "--task", "parcel"], b"");
        assert_eq!(status["resources"][0]["state"], state);
        assert_eq!(status["resources"][0]["recipients"], json!([]));
    }
    let a = Fixture::new();
    let b = Fixture::new();
    fs::copy(a.root.join("lens.json"), b.root.join("lens.json")).unwrap();
    assert!(!b
        .call(
            &[
                "project",
                "capabilities",
                "select",
                "lens.json",
                "--decision",
                "reviewed",
                "--reason",
                "Owner approved"
            ],
            b""
        )
        .status
        .success());
}
