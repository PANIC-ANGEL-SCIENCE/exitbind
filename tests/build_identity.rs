use serde_json::Value;
use sha2::{Digest, Sha256};
use std::process::Command;

#[test]
fn version_json_names_build_and_local_executable_digest_without_authenticating() {
    let binary = env!("CARGO_BIN_EXE_exitbind");
    let plain = Command::new(binary).arg("version").output().unwrap();
    assert!(plain.status.success(), "{plain:?}");
    assert_eq!(
        String::from_utf8(plain.stdout).unwrap().trim(),
        env!("CARGO_PKG_VERSION")
    );

    let output = Command::new(binary)
        .args(["version", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(identity["name"], "exitbind");
    assert_eq!(identity["version"], env!("CARGO_PKG_VERSION"));
    match option_env!("EXITBIND_BUILD_COMMIT") {
        Some(commit) => assert_eq!(identity["commit"], commit),
        None => assert!(identity["commit"].is_null()),
    }
    let expected = format!("{:x}", Sha256::digest(std::fs::read(binary).unwrap()));
    assert_eq!(identity["executableSha256"], expected);
    assert!(identity["executableSha256Error"].is_null());
    assert!(identity["authentication"]
        .as_str()
        .unwrap()
        .starts_with("none:"));

    let refused = Command::new(binary)
        .args(["version", "--json", "extra"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
}
