mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn sha(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn local_call(
    product: &Path,
    config: &Path,
    bindings: &Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(product)
        .env("EXITBIND_BINDINGS_DIR", bindings)
        .args(args)
        .args(["--config", config.to_str().unwrap()]);
    if let Some(bytes) = input {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        child.wait_with_output().unwrap()
    } else {
        command.output().unwrap()
    }
}

#[test]
fn observed_support_uses_acquired_configuration_not_later_claimed_conditions() {
    let base = support::temp("w2ch-acquired-config");
    let product = base.join("product");
    let control = base.join("control");
    let state = base.join("state");
    let bindings = base.join("bindings");
    for path in [&product, &control, &state] {
        fs::create_dir(path).unwrap();
    }
    fs::write(product.join("requirements.txt"), "First requirement.\n").unwrap();
    let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .env("EXITBIND_BINDINGS_DIR", &bindings)
        .args([
            "init",
            "--mode",
            "local",
            "--project-id",
            "w2ch_config",
            "--root",
            product.to_str().unwrap(),
            "--control-root",
            control.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = control.join("exitbind.json");
    let original = fs::read_to_string(&config).unwrap();
    let started = local_call(
        &product,
        &config,
        &bindings,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "First requirement.",
            "--check-command",
            "true",
            "--preserve-requirement",
            "first:First requirement.",
            "--preservation-check-command",
            "true",
            "--review-policy",
            "required",
        ],
        None,
    );
    assert!(started.status.success(), "{started:?}");
    let started: Value = serde_json::from_slice(&started.stdout).unwrap();
    let work = started["work"].as_str().unwrap();
    for input in [
        json!({"action":"init","sourceRef":"requirements.txt",
            "sourceText":"First requirement.",
            "requirements":[{"id":"first","text":"First requirement."}]}),
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
            "host":"codex","session":"config-a","hostVersion":"0.156.1"}),
    ] {
        let bytes = serde_json::to_vec(&input).unwrap();
        let output = local_call(
            &product,
            &config,
            &bindings,
            &["work", "record", work],
            Some(&bytes),
        );
        assert!(output.status.success(), "{output:?}");
    }
    for outcome in ["scoped", "completed"] {
        let next = local_call(
            &product,
            &config,
            &bindings,
            &["work", "next", work, "--full"],
            None,
        );
        assert!(next.status.success(), "{next:?}");
        let next: Value = serde_json::from_slice(&next.stdout).unwrap();
        let assignment = next["next"]["assignment"].as_str().unwrap();
        let output = local_call(
            &product,
            &config,
            &bindings,
            &["work", "return", work, assignment, "--outcome", outcome],
            Some(b"Scoped result"),
        );
        assert!(output.status.success(), "{output:?}");
    }
    let first = local_call(&product, &config, &bindings, &["work", "check", work], None);
    assert!(first.status.success(), "{first:?}");
    let checked = local_call(&product, &config, &bindings, &["work", "check", work], None);
    assert!(checked.status.success(), "{checked:?}");
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let inspected = local_call(
        &product,
        &config,
        &bindings,
        &[
            "run",
            "inspect",
            &ledger,
            "--event",
            checked["event"]["eventSha256"].as_str().unwrap(),
        ],
        None,
    );
    assert!(inspected.status.success(), "{inspected:?}");
    let inspected: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    let event = &inspected["event"];
    assert_eq!(event["requirementId"], "first");
    assert_eq!(event["configSha256"], sha(&original));
    let view_a = local_call(
        &product,
        &config,
        &bindings,
        &["work", "continuation", work],
        None,
    );
    assert!(view_a.status.success(), "{view_a:?}");
    let view_a: Value = serde_json::from_slice(&view_a.stdout).unwrap();
    let mut changed: Value = serde_json::from_str(&original).unwrap();
    changed["agents"]["lead"]["purpose"] = json!("Changed lead purpose for configuration B");
    fs::write(&config, serde_json::to_string_pretty(&changed).unwrap()).unwrap();
    let view_b = local_call(
        &product,
        &config,
        &bindings,
        &["work", "continuation", work],
        None,
    );
    assert!(view_b.status.success(), "{view_b:?}");
    let view_b: Value = serde_json::from_slice(&view_b.stdout).unwrap();
    assert_eq!(view_a["currentInputsSha256"], view_b["currentInputsSha256"]);
    assert_ne!(
        view_a["currentConditionsSha256"],
        view_b["currentConditionsSha256"]
    );
    let history = state.join(".exitbind/session-goal.jsonl");
    let before = fs::read(&history).unwrap();
    let support = |conditions: &Value| {
        json!({"action":"support","expectedRevision":2,
        "bindingRevision":1,"requirementId":"first","requirementRevision":1,
        "resultEventSha256":event["eventSha256"],"conditionsSha256":conditions})
    };
    let bad = serde_json::to_vec(&support(&view_b["currentConditionsSha256"])).unwrap();
    let refused = local_call(
        &product,
        &config,
        &bindings,
        &["work", "record", work],
        Some(&bad),
    );
    assert!(!refused.status.success());
    assert_eq!(fs::read(&history).unwrap(), before);
    fs::write(&config, &original).unwrap();
    let good = serde_json::to_vec(&support(&view_a["currentConditionsSha256"])).unwrap();
    let accepted = local_call(
        &product,
        &config,
        &bindings,
        &["work", "record", work],
        Some(&good),
    );
    assert!(accepted.status.success(), "{accepted:?}");
    fs::write(&config, serde_json::to_string_pretty(&changed).unwrap()).unwrap();
    let later = local_call(
        &product,
        &config,
        &bindings,
        &["work", "continuation", work],
        None,
    );
    assert!(later.status.success(), "{later:?}");
    let later: Value = serde_json::from_slice(&later.stdout).unwrap();
    assert_eq!(later["requirements"][0]["unresolved"], true);
    assert_eq!(
        later["requirements"][0]["support"][0]["integrityCurrent"],
        true
    );
    assert_eq!(
        later["requirements"][0]["support"][0]["acquiredConditionsCurrent"],
        false
    );
    fs::remove_dir_all(base).unwrap();
}
