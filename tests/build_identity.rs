use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

mod support;

#[test]
fn version_json_names_build_and_local_executable_digest_without_authenticating() {
    let binary = env!("CARGO_BIN_EXE_exitbind");
    let plain = support::git_topology::command(binary)
        .arg("version")
        .output()
        .unwrap();
    assert!(plain.status.success(), "{plain:?}");
    assert_eq!(
        String::from_utf8(plain.stdout).unwrap().trim(),
        env!("CARGO_PKG_VERSION")
    );

    let output = support::git_topology::command(binary)
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

    let refused = support::git_topology::command(binary)
        .args(["version", "--json", "extra"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
}

#[test]
fn pinned_exitbind_keeps_identity_and_soulmate_copy_refuses_before_cli_effects() {
    let root = support::temp("build-identity");
    let copied = root.join("exitbind-pinned");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &copied);
    let soulmate_copy = root.join("soulmate");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &soulmate_copy);
    let renamed = support::run(support::git_topology::command(&copied).args(["version", "--json"]));
    assert!(renamed.status.success(), "{renamed:?}");
    let identity: Value = serde_json::from_slice(&renamed.stdout).unwrap();
    assert_eq!(identity["name"], "exitbind");

    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    let config = project.join("exitbind.json");
    std::fs::write(&config, b"this must not be parsed\n").unwrap();
    let before = std::fs::read(&config).unwrap();
    let refuse = |binary: &Path| {
        support::run(
            support::git_topology::command(binary)
                .current_dir(&project)
                .args(["--config"])
                .arg(&config)
                .args(["run", "start", "change", "--json"]),
        )
    };
    let output = refuse(&soulmate_copy);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Soulmate is no longer supported"));
    assert_eq!(std::fs::read(&config).unwrap(), before);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let output = support::run(
            Command::new(&soulmate_copy)
                .arg0("exitbind")
                .current_dir(&project)
                .args(["--config"])
                .arg(&config)
                .args(["run", "start", "change", "--json"]),
        );
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("Soulmate is no longer supported"));
        assert_eq!(std::fs::read(&config).unwrap(), before);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let alias = root.join("alias").join("soulmate");
        std::fs::create_dir_all(alias.parent().unwrap()).unwrap();
        symlink(Path::new(env!("CARGO_BIN_EXE_exitbind")), &alias).unwrap();
        let output = refuse(&alias);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("Soulmate is no longer supported"));
        assert_eq!(std::fs::read(&config).unwrap(), before);
    }

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn binary_consumer_binds_exact_executable_and_refuses_same_version_mismatches() {
    let root = support::temp("consumer-identity");
    let copied = root.join("exitbind candidate's bytes");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &copied);
    let version = support::run(support::git_topology::command(&copied).args(["version", "--json"]));
    assert!(version.status.success(), "{version:?}");
    let identity: Value = serde_json::from_slice(&version.stdout).unwrap();
    let commit = identity["commit"]
        .as_str()
        .unwrap_or("0000000000000000000000000000000000000000");
    let digest = format!("{:x}", Sha256::digest(std::fs::read(&copied).unwrap()));
    let invoke = |expected_commit: &str, expected_digest: &str| {
        Command::new("python3")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify-binary.py"))
            .arg(&copied)
            .args([
                "--class",
                "development",
                "--commit",
                expected_commit,
                "--sha256",
                expected_digest,
            ])
            .output()
            .unwrap()
    };
    let output = invoke(commit, &digest);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["executable"], copied.to_str().unwrap());
    assert_eq!(report["acquisitionClass"], "development");
    assert_eq!(output.status.success(), identity["commit"].is_string());
    assert_eq!(report["resolved"], identity["commit"].is_string());
    if identity["commit"].is_null() {
        assert!(report["reason"]
            .as_str()
            .unwrap()
            .contains("producer unresolved"));
    }
    for (wrong_commit, wrong_digest) in [
        ("f".repeat(40), digest),
        (commit.to_owned(), "f".repeat(64)),
    ] {
        let output = invoke(&wrong_commit, &wrong_digest);
        assert!(!output.status.success(), "{output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["producer"]["version"], identity["version"]);
        assert_eq!(report["resolved"], false);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn binary_consumer_keeps_null_producer_unresolved_with_matching_local_bytes() {
    let root = support::temp("unresolved-producer");
    let fixture = root.join("fixture-producer");
    std::fs::write(&fixture, "#!/usr/bin/env python3\nimport hashlib,json\nprint(json.dumps({'name':'exitbind','version':'0.27.2','commit':None,'executableSha256':hashlib.sha256(open(__file__,'rb').read()).hexdigest(),'executableSha256Error':None}))\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fixture, std::fs::Permissions::from_mode(0o700)).unwrap();
    let digest = format!("{:x}", Sha256::digest(std::fs::read(&fixture).unwrap()));
    let output = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify-binary.py"))
        .arg(&fixture)
        .args([
            "--class",
            "release",
            "--commit",
            &"a".repeat(40),
            "--sha256",
            &digest,
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["sha256"], digest);
    assert_eq!(report["resolved"], false);
    assert!(report["reason"]
        .as_str()
        .unwrap()
        .contains("producer unresolved"));
    assert!(report["authentication"]
        .as_str()
        .unwrap()
        .starts_with("none:"));
    std::fs::remove_dir_all(root).unwrap();
}
