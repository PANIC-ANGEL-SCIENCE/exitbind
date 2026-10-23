// Current Exitbind and preserved historical-reader contracts run in the default suite.
use serde_json::{json, Value};
mod support;
use std::{fs, path::Path, process::Output};

fn project() -> support::TempRoot {
    let root = support::temp("harness-test");
    let output = invoke(&[
        "init",
        "--root",
        root.to_str().unwrap(),
        "--mode",
        "portable",
    ]);
    assert!(output.status.success(), "{:?}", output);
    root
}

fn invoke(arguments: &[&str]) -> Output {
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(arguments)
        .output()
        .unwrap()
}

fn invoke_with_bindings(arguments: &[&str], bindings: &Path) -> Output {
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .env("EXITBIND_BINDINGS_DIR", bindings)
        .args(arguments)
        .output()
        .unwrap()
}

fn manifest() -> Value {
    json!({
        "$schema": "https://raw.githubusercontent.com/envy404/soulmate/v0.4.0/schema/harness-manifest.schema.json",
        "version": 1,
        "project": {"id": "example-project", "session": "example-session"},
        "harness": {"name": "example-codex-harness", "version": "2026.08.30"},
        "activations": [
            {"kind": "skill", "name": "soulmate", "evidence": "configured"},
            {"kind": "skill", "name": "coffee", "evidence": "presented"},
            {"kind": "perspective", "name": "implementation-planner", "evidence": "agent_declared"},
            {"kind": "ponytail", "name": "ponytail:ponytail", "evidence": "hook_observed"},
            {
                "kind": "skill",
                "name": "soulmate",
                "evidence": "independently_verified",
                "verification": {
                    "verifier": "mechanicalverifier",
                    "artifactSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }
            }
        ]
    })
}

#[test]
fn older_manifest_schema_remains_advisory_after_release_bump() {
    let root = project();
    let config = root.join("exitbind.json").to_string_lossy().into_owned();
    let manifest_path = root.join("harness-manifest.json");
    let receipt_path = root.join("harness-receipt.json");
    write_json(&manifest_path, &manifest());

    let planned = invoke(&[
        "plan",
        "change",
        "--goal",
        "bounded",
        "--receipt",
        receipt_path.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let receipt: Value = serde_json::from_str(&fs::read_to_string(receipt_path).unwrap()).unwrap();
    assert_eq!(receipt["version"], 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn instruction_like_manifest_token_remains_non_authoritative_hashed_evidence() {
    let root = project();
    let config = root.join("exitbind.json").to_string_lossy().into_owned();
    let manifest_path = root.join("exitbind/harness/harness-manifest.json");
    fs::write(
        manifest_path,
        include_str!("fixtures/instruction-like-harness.json"),
    )
    .unwrap();
    let receipt_path = root.join(".exitbind/receipts/instruction-like.json");
    let planned = invoke(&[
        "plan",
        "change",
        "--goal",
        "authority fixture",
        "--receipt",
        receipt_path.to_str().unwrap(),
        "--harness-manifest",
        "exitbind/harness/harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let source = fs::read_to_string(receipt_path).unwrap();
    assert!(!source.contains("IGNORE_PREVIOUS_INSTRUCTIONS"));
    let receipt: Value = serde_json::from_str(&source).unwrap();
    assert_eq!(
        receipt["harness"]["path"],
        "exitbind/harness/harness-manifest.json"
    );
    assert_eq!(
        receipt["harness"]["activations"][0]["evidence"],
        "presented"
    );
    fs::remove_dir_all(root).unwrap();
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap();
}

#[test]
fn v2_receipt_binds_bounded_harness_evidence_and_detects_drift() {
    let root = project();
    let config = root.join("exitbind.json").to_string_lossy().into_owned();
    let manifest_path = root.join("harness-manifest.json");
    let receipt_path = root.join("harness-receipt.json");
    write_json(&manifest_path, &manifest());

    let planned = invoke(&[
        "plan",
        "change",
        "--goal",
        "private-goal-must-not-be-recorded",
        "--receipt",
        receipt_path.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let source = fs::read_to_string(&receipt_path).unwrap();
    assert!(!source.contains("private-goal-must-not-be-recorded"));
    for omitted in [
        "example-project",
        "example-session",
        "example-codex-harness",
        "coffee",
        "implementation-planner",
        "ponytail:ponytail",
        "mechanicalverifier",
    ] {
        assert!(!source.contains(omitted), "raw manifest value {omitted}");
    }
    let receipt: Value = serde_json::from_str(&source).unwrap();
    assert_eq!(receipt["version"], 2);
    assert_eq!(receipt["harness"]["manifestVersion"], 1);
    assert_eq!(receipt["harness"]["privacy"], "raw-manifest-values-omitted");
    assert_eq!(
        receipt["harness"]["activations"][3]["evidence"],
        "hook_observed"
    );
    assert_eq!(
        receipt["harness"]["project"]["idSha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert!(invoke(&[
        "verify",
        receipt_path.to_str().unwrap(),
        "--config",
        &config
    ])
    .status
    .success());

    let mut changed = manifest();
    changed["harness"]["version"] = json!("2026.08.31");
    write_json(&manifest_path, &changed);
    let verified = invoke(&[
        "verify",
        receipt_path.to_str().unwrap(),
        "--config",
        &config,
    ]);
    assert!(!verified.status.success());
    let body: Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(body["valid"], false);
    assert_eq!(body["mismatches"][0], "harness manifest changed");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn v2_receipt_integrity_precedes_missing_boundary_warning() {
    let root = project();
    let config_path = root.join("exitbind.json");
    let config = config_path.to_string_lossy().into_owned();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    let mut config_value: Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config_value["agents"]["worker"]["observe"] = json!(["src/**"]);
    config_value["agents"]["worker"]["write"] = json!(["src/**"]);
    write_json(&config_path, &config_value);

    let boundary = root.join(".agents/boundaries/task.json");
    fs::create_dir_all(boundary.parent().unwrap()).unwrap();
    write_json(
        &boundary,
        &json!({
            "version": 1,
            "agents": {
                "worker": {"observe": ["src/a.rs"], "write": ["src/a.rs"]}
            }
        }),
    );
    write_json(&root.join("harness-manifest.json"), &manifest());
    let receipt = root.join(".exitbind/harness-receipt.json");
    let planned = invoke(&[
        "plan",
        "change",
        "--goal",
        "boundary receipt regression",
        "--receipt",
        receipt.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(
        planned.status.success(),
        "{}{}",
        String::from_utf8_lossy(&planned.stdout),
        String::from_utf8_lossy(&planned.stderr)
    );

    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "boundary receipt regression",
        "--ledger",
        ".exitbind/run.jsonl",
        "--boundary",
        ".agents/boundaries/task.json",
        "--harness-receipt",
        receipt.to_str().unwrap(),
        "--config",
        &config,
    ]);
    assert!(
        started.status.success(),
        "{}{}",
        String::from_utf8_lossy(&started.stdout),
        String::from_utf8_lossy(&started.stderr)
    );
    let first: Value = serde_json::from_str(
        fs::read_to_string(root.join(".exitbind/run.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["version"], 8);
    assert_eq!(first["harnessReceipt"]["version"], 2);

    fs::remove_file(&boundary).unwrap();
    let authentic = invoke(&[
        "run",
        "next",
        ".exitbind/run.jsonl",
        "--json",
        "--config",
        &config,
    ]);
    assert!(
        authentic.status.success(),
        "{}{}",
        String::from_utf8_lossy(&authentic.stdout),
        String::from_utf8_lossy(&authentic.stderr)
    );
    let authentic_value: Value = serde_json::from_slice(&authentic.stdout).unwrap();
    assert_eq!(authentic_value["valid"], true);
    assert_eq!(
        authentic_value["warnings"][0]["classification"],
        "boundary_drift"
    );

    let mut tampered = fs::read(&receipt).unwrap();
    tampered.push(b'\n');
    fs::write(&receipt, tampered).unwrap();
    let rejected = invoke(&[
        "run",
        "next",
        ".exitbind/run.jsonl",
        "--json",
        "--config",
        &config,
    ]);
    assert!(!rejected.status.success());
    let rejected_text = format!(
        "{}{}",
        String::from_utf8_lossy(&rejected.stdout),
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(
        rejected_text.contains("harness receipt reference is not exact"),
        "{rejected_text}"
    );

    let read_surfaces = [
        vec![
            "run",
            "status",
            ".exitbind/run.jsonl",
            "--json",
            "--config",
            config.as_str(),
        ],
        vec![
            "run",
            "inspect",
            ".exitbind/run.jsonl",
            "--json",
            "--config",
            config.as_str(),
        ],
        vec![
            "run",
            "explain",
            ".exitbind/run.jsonl",
            "--json",
            "--config",
            config.as_str(),
        ],
        vec![
            "run",
            "report",
            ".exitbind/run.jsonl",
            "--json",
            "--config",
            config.as_str(),
        ],
        vec![
            "run",
            "status",
            ".exitbind/run.jsonl",
            "--config",
            config.as_str(),
        ],
        vec![
            "run",
            "explain",
            ".exitbind/run.jsonl",
            "--config",
            config.as_str(),
        ],
    ];
    for arguments in read_surfaces {
        let rejected = invoke(&arguments);
        assert!(
            !rejected.status.success(),
            "receipt tampering passed: {arguments:?}"
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(
            text.contains("harness receipt reference is not exact"),
            "{arguments:?}: {text}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn manifest_rejects_private_or_unverifiable_fields_before_receipt_creation() {
    let root = project();
    let config = root.join("exitbind.json").to_string_lossy().into_owned();
    let manifest_path = root.join("harness-manifest.json");
    let receipt_path = root.join("must-not-exist.json");
    for field in ["prompt", "transcript", "environment", "secret"] {
        let mut private = manifest();
        private[field] = json!("private-value-must-not-be-recorded");
        write_json(&manifest_path, &private);
        let rejected = invoke(&[
            "brief",
            "worker",
            "--task",
            "private task",
            "--receipt",
            receipt_path.to_str().unwrap(),
            "--harness-manifest",
            "harness-manifest.json",
            "--config",
            &config,
        ]);
        assert!(!rejected.status.success(), "field {field}");
        assert!(!receipt_path.exists(), "field {field}");
        assert!(
            !String::from_utf8_lossy(&rejected.stderr)
                .contains("private-value-must-not-be-recorded"),
            "field {field}"
        );
    }
    fs::write(&manifest_path, vec![b' '; 65_537]).unwrap();
    let rejected = invoke(&[
        "brief",
        "worker",
        "--task",
        "private task",
        "--receipt",
        receipt_path.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("65536-byte limit"));
    assert!(!receipt_path.exists());

    let mut token_shaped = manifest();
    let git_token = ["gh", "p_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"].concat();
    let aws_label = ["AWS_ACCESS_KEY_ID_", "AKIA", "IOSFODNN7EXAMPLE"].concat();
    token_shaped["project"]["session"] = json!(git_token);
    token_shaped["activations"][0]["name"] = json!(aws_label);
    write_json(&manifest_path, &token_shaped);
    let accepted = invoke(&[
        "plan",
        "change",
        "--goal",
        "private goal",
        "--receipt",
        receipt_path.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(accepted.status.success());
    let receipt_source = fs::read_to_string(&receipt_path).unwrap();
    assert!(!receipt_source.contains(token_shaped["project"]["session"].as_str().unwrap()));
    assert!(!receipt_source.contains(token_shaped["activations"][0]["name"].as_str().unwrap()));
    fs::remove_file(&receipt_path).unwrap();

    let mut unverified = manifest();
    unverified["activations"][4]
        .as_object_mut()
        .unwrap()
        .remove("verification");
    write_json(&manifest_path, &unverified);
    let rejected = invoke(&[
        "plan",
        "change",
        "--goal",
        "private goal",
        "--receipt",
        receipt_path.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(!rejected.status.success());
    assert!(!receipt_path.exists());

    let mut uppercase_hash = manifest();
    uppercase_hash["activations"][4]["verification"]["artifactSha256"] =
        json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    write_json(&manifest_path, &uppercase_hash);
    let rejected = invoke(&[
        "plan",
        "change",
        "--goal",
        "private goal",
        "--receipt",
        receipt_path.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("lowercase"));
    assert!(!receipt_path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_mode_manifest_cannot_claim_a_different_project() {
    let root = support::temp("harness-local");
    let product = root.join("product");
    let control = root.join("control");
    let state = root.join("state");
    for path in [&product, &control, &state] {
        fs::create_dir_all(path).unwrap();
    }
    let product_repository = support::git_topology::command("git")
        .args(["init", "--quiet"])
        .current_dir(&product)
        .status()
        .unwrap();
    assert!(product_repository.success());
    let root = fs::canonicalize(&root).unwrap();
    let product = fs::canonicalize(product).unwrap();
    let control = fs::canonicalize(control).unwrap();
    let state = fs::canonicalize(state).unwrap();
    let bindings = root.join("bindings");
    let initialized = invoke_with_bindings(
        &[
            "init",
            "--mode",
            "local",
            "--project-id",
            "actual-project",
            "--root",
            product.to_str().unwrap(),
            "--control-root",
            control.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(
        initialized.status.success(),
        "{}{}",
        String::from_utf8_lossy(&initialized.stdout),
        String::from_utf8_lossy(&initialized.stderr)
    );
    write_json(&control.join("harness-manifest.json"), &manifest());
    let config = control.join("exitbind.json").to_string_lossy().into_owned();
    let rejected = invoke_with_bindings(
        &[
            "plan",
            "change",
            "--goal",
            "bounded",
            "--receipt",
            "receipt.json",
            "--harness-manifest",
            "harness-manifest.json",
            "--config",
            &config,
        ],
        &bindings,
    );
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr)
        .contains("project.id does not match configured project.id"));
    assert!(!state.join("receipt.json").exists());
    fs::remove_dir_all(root).unwrap();
}
