//! Parent host help exposes supported children and their direct help forms.
#![cfg(unix)]

use std::process::Command;

fn invoke(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn host_parent_and_supported_child_help_are_discoverable() {
    let parent = invoke(&["host", "--help"]);
    assert!(parent.status.success(), "{parent:?}");
    let parent = String::from_utf8_lossy(&parent.stdout);
    assert!(parent.contains("host status"), "{parent}");
    assert!(parent.contains("host install"), "{parent}");
    assert!(parent.contains("host <child> --help"), "{parent}");

    for child in ["status", "install"] {
        let output = invoke(&["host", child, "--help"]);
        assert!(output.status.success(), "{child}: {output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(&format!("host {child}")), "{text}");
    }
}
