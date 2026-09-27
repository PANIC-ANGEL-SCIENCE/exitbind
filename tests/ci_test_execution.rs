use std::io::Write;
use std::process::{Command, Stdio};

fn guard(log: &str) -> bool {
    let mut child = Command::new("sh")
        .args([
            concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/assert-test-ran.sh"),
            "real_tmux_child_presents_bound_evidence_without_persisting_the_prompt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(log.as_bytes())
        .unwrap();
    child.wait().unwrap().success()
}

#[test]
fn native_ci_requires_the_exact_test_to_run_and_pass() {
    let result =
        "test real_tmux_child_presents_bound_evidence_without_persisting_the_prompt ... ok\n";
    assert!(guard(&format!("running 1 test\n{result}")));
    assert!(!guard("running 0 tests\n"));
    assert!(!guard(&format!("running 0 tests\n{result}")));
    assert!(!guard("running 1 test\ntest other_test ... ok\n"));
    assert!(!guard("running 1 test\ntest real_tmux_child_presents_bound_evidence_without_persisting_the_prompt ... ignored\n"));
}
