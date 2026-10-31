//! Real retry controls and explicitly synthetic malformed-result negatives.
use super::*;

pub(super) struct Active {
    pub(super) root: support::TempRoot,
    pub(super) work: String,
    pub(super) assignment: String,
}

impl Active {
    pub(super) fn new(label: &str, checker: &str, timeout_ms: &str, alias: bool) -> Self {
        let root = support::temp(label);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        if alias {
            use std::os::unix::fs::symlink;
            let path = root.join("exitbind.json");
            let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            config["project"]["verificationPreparation"] =
                json!({"version":1,"pythonBytecode":"disabled","scratch":"taskOwned"});
            fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
            symlink(".exitbind/verification", root.join("verification-alias")).unwrap();
        }
        let begun = successful(command(
            &root,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "known pre-return retry",
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--pre-return-command",
                checker,
                "--pre-return-timeout-ms",
                timeout_ms,
            ],
            None,
        ));
        let work = begun["work"].as_str().unwrap().to_owned();
        let lead = begun["next"]["assignment"].as_str().unwrap();
        let scope = command(
            &root,
            &["work", "return", &work, lead, "--outcome", "scoped"],
            Some(b"scope"),
        );
        assert!(scope.status.success(), "{scope:?}");
        let detail = successful(command(&root, &["work", "detail", &work, "--json"], None));
        let assignment = detail["recipient"]["assignment"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            root,
            work,
            assignment,
        }
    }

    pub(super) fn finish(&self, body: &[u8]) -> Output {
        command(
            &self.root,
            &[
                "work",
                "return",
                &self.work,
                &self.assignment,
                "--outcome",
                "completed",
            ],
            Some(body),
        )
    }

    fn detail(&self) -> Value {
        successful(command(
            &self.root,
            &["work", "detail", &self.work, "--json"],
            None,
        ))
    }

    fn finish_with_path(&self, body: &[u8], path: &Path) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args([
                "work",
                "return",
                &self.work,
                &self.assignment,
                "--outcome",
                "completed",
                "--config",
            ])
            .arg(self.root.join("exitbind.json"))
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(body) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        child.wait_with_output().unwrap()
    }
}

fn failure(output: &Output) -> Value {
    assert!(!output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {output:?}"))
}

#[test]
fn known_ended_timeout_replays_same_result_but_rechecks_a_new_result() {
    let f = Active::new("pre-return-known-timeout", "if [ -f .exitbind/known-seen ]; then printf x >> .exitbind/known-count; else : > .exitbind/known-seen; printf x >> .exitbind/known-count; sleep 2; fi", "250", false);
    let first = f.finish(b"first result");
    let failed = failure(&first);
    assert_eq!(failed["failureKind"], "timeout");
    assert_eq!(failed["diagnostic"]["termination"], "ended");
    assert_eq!(failed["diagnostic"]["groupEnded"], true);
    assert_eq!(failed["diagnostic"]["captureReadersEnded"], true);
    assert!(failed["nextActions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str().is_some_and(|text| text.contains("repair"))));
    assert_eq!(f.detail()["actionForms"]["state"], "pending_assignment");
    let same = failure(&f.finish(b"first result"));
    assert_eq!(same["replayed"], true);
    assert_eq!(
        fs::read(f.root.join(".exitbind/known-count")).unwrap(),
        b"x"
    );
    let second = f.finish(b"different result");
    assert!(second.status.success(), "{second:?}");
    assert_eq!(
        fs::read(f.root.join(".exitbind/known-count")).unwrap(),
        b"xx"
    );
    let detail = f.detail();
    assert_eq!(detail["actionForms"]["state"], "check");
    assert_eq!(
        detail["sections"]["evidence"]["preReturn"]["attempt"]["result"]["state"],
        "passed"
    );
}

#[test]
fn launch_failure_did_not_start_and_changed_result_can_retry() {
    let f = Active::new(
        "pre-return-known-launch",
        "printf x >> .exitbind/launch-count",
        "5000",
        false,
    );
    let no_shell = support::git_topology::git_only_path(&f.root.join(".exitbind"));
    let first = f.finish_with_path(b"first result", &no_shell);
    let failed = failure(&first);
    assert_eq!(failed["failureKind"], "launch_failure");
    assert_eq!(failed["diagnostic"]["processStarted"], false);
    assert_eq!(failed["diagnostic"]["termination"], "ended");
    assert!(!f.root.join(".exitbind/launch-count").exists());
    let second = f.finish(b"different result");
    assert!(second.status.success(), "{second:?}");
    assert_eq!(
        fs::read(f.root.join(".exitbind/launch-count")).unwrap(),
        b"x"
    );
    assert_eq!(f.detail()["actionForms"]["state"], "check");
}

#[test]
fn changed_result_rechecks_the_preparation_and_still_refuses_the_alias() {
    let f = Active::new(
        "pre-return-known-alias",
        "touch verification-alias/should-not-run",
        "5000",
        true,
    );
    let first = failure(&f.finish(b"first result"));
    assert_eq!(first["failureKind"], "preparation_refused");
    let guidance = first["nextActions"].to_string();
    assert!(guidance.contains("preparation"), "{first}");
    assert!(!guidance.contains("repair within"), "{first}");
    assert!(first["effectMeaning"].as_str().unwrap().contains("audit"));
    assert_eq!(first["execution"]["started"], false);
    assert_eq!(first["execution"]["certainty"], "known_not_started");
    let second = failure(&f.finish(b"different result"));
    assert_eq!(second["failureKind"], "preparation_refused");
    assert_eq!(second["execution"]["started"], false);
    assert_eq!(second["execution"]["certainty"], "known_not_started");
    assert_ne!(first["attemptId"], second["attemptId"]);
    assert!(!f.root.join("verification-alias/should-not-run").exists());
}

fn synthetic_result(f: &Fixture) -> Value {
    let admission: Value = serde_json::from_slice(&f.admission).unwrap();
    let binding = &admission["binding"];
    json!({"version":1,"kind":"pre_return_result","attemptId":f.attempt,
        "state":"uncertain","failureKind":"execution_uncertain",
        "bindingSha256":digest(&serde_json::to_vec(binding).unwrap()),
        "policySha256":digest(&serde_json::to_vec(&admission["policy"]).unwrap()),
        "producer":{"executableSha256":binding["checkerExecutableSha256"],"producer":binding["producer"]},
        "completedAt":"2026-01-01T00:00:00Z"})
}

fn save_synthetic(f: &Fixture, mut result: Value) {
    result["recordSha256"] = json!(digest(&serde_json::to_vec(&result).unwrap()));
    fs::write(f.result_path(), serde_json::to_vec(&result).unwrap()).unwrap();
    fs::set_permissions(f.result_path(), fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn synthetic_malformed_uncertain_facts_cannot_authorize_another_attempt() {
    let f = Fixture::new("pre-return-malformed-facts", false);
    f.release();
    let before = f.ledger();
    let mut cases = Vec::new();
    let mut foreign = synthetic_result(&f);
    foreign["producer"]["executableSha256"] = json!("0".repeat(64));
    foreign["execution"] = json!({"started":false,"certainty":"known_not_started"});
    cases.push((foreign, "pre-return result identity mismatch"));
    let mut bad_type = synthetic_result(&f);
    bad_type["execution"] = json!({"started":"false","certainty":"known_not_started"});
    cases.push((bad_type, "uncertain outcome"));
    let mut contradiction = synthetic_result(&f);
    contradiction["execution"] = json!({"started":false,"certainty":"known_not_started"});
    contradiction["diagnostic"] = json!({"processStarted":true,"termination":"ended","groupEnded":true,"captureReadersEnded":true});
    cases.push((contradiction, "uncertain outcome"));
    let mut conflicting_end = synthetic_result(&f);
    conflicting_end["execution"] = json!({"started":true,"knownEnded":true});
    conflicting_end["diagnostic"] = json!({"processStarted":true,"termination":"unknown","groupEnded":false,"captureReadersEnded":false});
    cases.push((conflicting_end, "uncertain outcome"));
    let mut incomplete = synthetic_result(&f);
    incomplete["diagnostic"] =
        json!({"processStarted":true,"termination":"ended","groupEnded":true});
    cases.push((incomplete, "uncertain outcome"));
    let mut label_only = synthetic_result(&f);
    label_only["failureKind"] = json!("launch_failure");
    cases.push((label_only, "uncertain outcome"));
    let mut unsupported = synthetic_result(&f);
    unsupported["state"] = json!("unrecognized");
    unsupported["execution"] = json!({"started":false,"certainty":"known_not_started"});
    cases.push((unsupported, "uncertain outcome"));
    let mut no_kind = synthetic_result(&f);
    no_kind["failureKind"] = Value::Null;
    no_kind["execution"] = json!({"started":false,"certainty":"known_not_started"});
    cases.push((no_kind, "pre-return result identity mismatch"));
    for (index, (result, expected)) in cases.into_iter().enumerate() {
        save_synthetic(&f, result);
        let output = f.call(
            &[
                "work",
                "return",
                &f.work,
                &f.assignment,
                "--outcome",
                "completed",
            ],
            Some(b"different result"),
        );
        assert!(!output.status.success(), "case {index}: {output:?}");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(text.contains(expected), "case {index}: {text}");
        assert_eq!(f.ledger(), before);
        assert_eq!(f.count(), b"x");
    }
}

#[test]
fn synthetic_unknown_saved_result_offers_inspection_and_blocked_not_repair() {
    let f = Fixture::new("pre-return-unknown-guidance", false);
    f.release();
    let mut unknown = synthetic_result(&f);
    unknown["diagnostic"] = json!({"processStarted":true,"termination":"unknown","groupEnded":false,"captureReadersEnded":false});
    save_synthetic(&f, unknown);
    let before = f.ledger();
    let refusal = f.call(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
        ],
        None,
    );
    assert!(!refusal.status.success(), "{refusal:?}");
    assert!(
        String::from_utf8_lossy(&refusal.stderr)
            .contains("has an uncertain outcome; inspect it and do not replay"),
        "{refusal:?}"
    );
    assert_eq!(f.count(), b"x");
    let detail = f.detail();
    assert_eq!(
        detail["actionForms"]["state"],
        "pre_return_uncertain_result"
    );
    assert!(detail["actionForms"]["diagnostic"]["command"]["argv"].is_array());
    let choices = detail["actionForms"]["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0]["label"], "blocked");
    let action = successful(f.call(
        &["work", "detail", &f.work, "--view", "action", "--json"],
        None,
    ));
    assert_eq!(
        action["effectiveAction"]["formState"],
        "pre_return_uncertain_result"
    );
    assert_eq!(action["effectiveAction"]["kind"], "spawn");
    assert!(action["effectiveAction"]["diagnostic"]["command"]["argv"].is_array());
    let choices = action["effectiveAction"]["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0]["label"], "blocked");
    let mut route = choices[0]["command"].clone();
    for word in route["argv"].as_array_mut().unwrap() {
        if word == "<REASON>" {
            *word = json!("Outcome remains unknown");
        }
    }
    assert_eq!(f.ledger(), before);
    let blocked = rendered(
        &f.root,
        &route,
        Some(b"Cannot establish the checker outcome."),
    );
    assert!(blocked.status.success(), "{blocked:?}");
    assert_eq!(f.count(), b"x");
}
