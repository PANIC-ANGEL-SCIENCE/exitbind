use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

mod support;

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

#[test]
fn copied_exitbind_keeps_identity_while_soulmate_target_stays_legacy() {
    let root = support::temp("build-identity");
    let copied = root.join("exitbind-pinned");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &copied);
    let soulmate_copy = root.join("soulmate");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &soulmate_copy);

    let assert_identity = |binary: &Path, expected: &str| {
        let output = support::run(Command::new(binary).args(["version", "--json"]));
        assert!(output.status.success(), "{output:?}");
        let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(identity["name"], expected);
    };
    let assert_init = |binary: &Path, project: &Path, config: &str, skill: &str| {
        std::fs::create_dir(project).unwrap();
        let init = support::run(
            Command::new(binary)
                .args(["init", "--mode", "portable", "--root"])
                .arg(project),
        );
        assert!(init.status.success(), "{init:?}");
        assert!(project.join(config).is_file());
        assert!(!project
            .join(if config == "exitbind.json" {
                "soulmate.json"
            } else {
                "exitbind.json"
            })
            .exists());
        for base in [".agents/skills", ".claude/skills"] {
            assert!(project.join(base).join(skill).join("SKILL.md").is_file());
            let other = if skill == "exitbind" {
                "soulmate"
            } else {
                "exitbind"
            };
            assert!(!project.join(base).join(other).join("SKILL.md").exists());
        }
    };

    assert_identity(&copied, "exitbind");
    assert_init(
        &copied,
        &root.join("versioned-project"),
        "exitbind.json",
        "exitbind",
    );
    assert_identity(&soulmate_copy, "soulmate");
    assert_init(
        &soulmate_copy,
        &root.join("soulmate-project"),
        "soulmate.json",
        "soulmate",
    );
    let legacy = Path::new(env!("CARGO_BIN_EXE_soulmate"));
    assert_identity(legacy, "soulmate");
    assert_init(
        legacy,
        &root.join("built-soulmate-project"),
        "soulmate.json",
        "soulmate",
    );

    std::fs::remove_dir_all(root).unwrap();
}
