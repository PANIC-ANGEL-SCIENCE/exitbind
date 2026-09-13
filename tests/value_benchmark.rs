use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(arguments)
        .output()
        .expect("benchmark binary should start")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn temporary_parent(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("soulmate-value-test-{label}-{stamp}"));
    fs::create_dir(&path).expect("temporary parent should be created");
    path
}

#[test]
fn benchmark_json_reports_actual_proof_and_unknown_human_time() {
    let output = invoke(&["benchmark", "--json"]);
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).expect("benchmark JSON");
    assert_eq!(value["version"], 1);
    assert_eq!(value["scenarioId"], "false-completion-v1");
    assert_eq!(value["origin"], "synthetic");
    assert_eq!(value["manualJsonEdits"], 0);
    assert!(value["automatedElapsedMs"].is_u64());
    assert!(value["humanInteractionMs"].is_null());
    assert_eq!(
        value["expectedResult"]["protectedFailedCheck"]["protectionRecordCount"],
        1
    );
    assert_eq!(value["observedResult"]["baseline"]["checkExitCode"], 1);
    assert_eq!(
        value["observedResult"]["protectedRecovery"]["finalStatus"],
        "accepted"
    );
    let assertions = value["assertions"].as_array().expect("assertions");
    assert!(!assertions.is_empty());
    assert!(assertions
        .iter()
        .all(|item| item["expected"] == true && item["passed"] == true));
    let invocations = &value["cliInvocations"];
    let total = invocations["total"].as_u64().expect("total invocations");
    assert!(total > 0);
    assert_eq!(
        total,
        invocations["successful"].as_u64().unwrap() + invocations["failed"].as_u64().unwrap()
    );
    assert!(value["report"]["groups"]["synthetic"].is_object());
    assert!(value["report"]["groups"]["local_report"].is_object());
}

#[test]
fn benchmark_human_output_explains_the_bounded_result() {
    let output = invoke(&["benchmark"]);
    assert!(output.status.success(), "{}", text(&output));
    let rendered = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    assert!(rendered.contains("False-completion proof passed"));
    assert!(rendered.contains("Attempt 1:\n  Worker claim: completed."));
    assert!(rendered.contains("Host-reported check: failed (exit 1); synthetic caller report."));
    assert!(rendered.contains("Attempt 2:\n  Worker claim: completed."));
    assert!(rendered.contains("Host-reported check: passed (exit 0); synthetic caller report."));
    let pending = rendered
        .find("After the repair check, before final acceptance:\n  Current fresh reviewer assignment: pending.\n  Lead decision: pending.")
        .expect("intermediate pending review state");
    let approved = rendered
        .find("After review and lead acceptance:\n  Reviewer outcome: approved.\n  Lead decision: accepted.")
        .expect("final review and lead state");
    assert!(
        pending < approved,
        "pending state must precede final acceptance"
    );
    assert!(rendered
        .contains("Lead decision: pending; protocol refusal recorded (not a lead rejection)."));
    assert!(rendered.contains("Lead decision: accepted."));
    for expected in [
        "Worker claim:",
        "Host-reported check:",
        "Benchmark driver ran the fixture check; run record-check only records the result.",
        "Reviewer outcome:",
        "Lead decision:",
    ] {
        assert!(rendered.contains(expected), "missing {expected}");
    }
    assert!(rendered.contains("preserved the previous attempt"));
    assert!(rendered.contains("Source: synthetic"));
    assert!(rendered.contains("Human interaction time: unmeasured"));
    assert!(!rendered.contains("soulmate check --config"));
}

#[test]
fn benchmark_exports_inspectable_bundle_with_private_file_modes() {
    let parent = temporary_parent("export");
    let destination = parent.join("bundle");
    let destination_text = destination.to_str().unwrap();
    let output = invoke(&["benchmark", "--json", "--output", destination_text]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(destination.is_dir());
    let result: Value = serde_json::from_slice(&output.stdout).expect("benchmark JSON");
    assert_eq!(result["scenarioId"], "false-completion-v1");
    for required in [
        "result.json",
        "result.md",
        "report.json",
        "report.md",
        "hash-manifest.json",
        "ledgers/baseline.jsonl",
        "ledgers/blocked.jsonl",
        "ledgers/final.jsonl",
        "scenario/expected.json",
        "schema/run-event-v3.schema.json",
        "schema/value-report-v1.schema.json",
        "schema/value-proof-v1.schema.json",
    ] {
        assert!(destination.join(required).is_file(), "missing {required}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let manifest: Value =
        serde_json::from_slice(&fs::read(destination.join("hash-manifest.json")).unwrap())
            .expect("hash manifest JSON");
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["self"], "excluded");
    for item in manifest["files"].as_array().unwrap() {
        let relative = item["path"].as_str().unwrap();
        let path = destination.join(relative);
        assert!(path.is_file(), "manifest file missing: {relative}");
        let bytes = fs::read(path).unwrap();
        assert_eq!(bytes.len(), item["bytes"].as_u64().unwrap() as usize);
        assert_eq!(sha256(&bytes), item["sha256"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(destination.join(relative))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    assert!(
        !fs::symlink_metadata(destination.join("ledgers/blocked.jsonl"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn benchmark_refuses_existing_output_without_overwriting_it() {
    let parent = temporary_parent("existing");
    let destination = parent.join("already-there");
    fs::create_dir(&destination).unwrap();
    let sentinel = destination.join("sentinel");
    fs::write(&sentinel, b"keep\n").unwrap();
    let output = invoke(&[
        "benchmark",
        "--json",
        "--output",
        destination.to_str().unwrap(),
    ]);
    assert!(!output.status.success(), "{}", text(&output));
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep\n");
    assert!(!destination.join("result.json").exists());
    fs::remove_dir_all(parent).unwrap();
}

#[cfg(unix)]
#[test]
fn benchmark_refuses_symlink_output_without_following_it() {
    use std::os::unix::fs::symlink;

    let parent = temporary_parent("symlink");
    let real = parent.join("real");
    let link = parent.join("link");
    fs::create_dir(&real).unwrap();
    symlink(&real, &link).unwrap();
    let output = invoke(&["benchmark", "--json", "--output", link.to_str().unwrap()]);
    assert!(!output.status.success(), "{}", text(&output));
    assert!(!real.join("result.json").exists());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn benchmark_rejects_unknown_and_arbitrary_command_options() {
    let parent = temporary_parent("options");
    let marker = parent.join("marker");
    let unknown = invoke(&["benchmark", "--json", "--unknown"]);
    assert!(!unknown.status.success(), "{}", text(&unknown));
    let arbitrary_command = format!("touch {}", marker.display());
    let arbitrary = invoke(&[
        "benchmark",
        "--json",
        "--check-command",
        arbitrary_command.as_str(),
    ]);
    assert!(!arbitrary.status.success(), "{}", text(&arbitrary));
    assert!(!marker.exists());
    fs::remove_dir_all(parent).unwrap();
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
