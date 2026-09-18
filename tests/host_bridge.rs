//! One install must make Exitbind discoverable by an existing coding-agent
//! lead. These tests use isolated HOME fixtures; they prove file/lifecycle
//! behavior, never that a host model selected the skill.
#![cfg(unix)]

mod support;

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn host_home(label: &str) -> std::path::PathBuf {
    let root = support::temp(label);
    fs::create_dir_all(root.join(".codex")).unwrap();
    fs::create_dir_all(root.join(".claude")).unwrap();
    root
}

/// Hook installation requires a PATH-installed `exitbind` that speaks the hook
/// protocol. Provide the binary under test so results do not depend on whatever
/// the machine running these tests happens to have installed.
fn path_with_binary(home: &Path) -> String {
    let bin = home.join("path-bin");
    fs::create_dir_all(&bin).unwrap();
    let linked = bin.join("exitbind");
    if !linked.exists() {
        fs::copy(env!("CARGO_BIN_EXE_exitbind"), &linked).unwrap();
        fs::set_permissions(&linked, fs::Permissions::from_mode(0o755)).unwrap();
    }
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

fn exitbind(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(args)
        .env("HOME", home)
        .env("PATH", path_with_binary(home))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("SOULMATE_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap()
}

fn json(output: std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn bootstrap(home: &Path, host: &str) -> std::path::PathBuf {
    match host {
        "codex" => home.join(".codex/skills/exitbind/SKILL.md"),
        _ => home.join(".claude/skills/exitbind/SKILL.md"),
    }
}

#[test]
fn install_makes_both_hosts_discoverable_and_status_separates_the_layers() {
    let home = host_home("host-bridge-install");
    let before = json(exitbind(&home, &["host", "status", "--json"]));
    for host in before["hosts"].as_array().unwrap() {
        assert_eq!(host["bootstrapSkill"], "missing");
    }

    let installed = json(exitbind(&home, &["host", "install", "--json"]));
    let actions: Vec<&str> = installed["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, vec!["installed", "installed"]);

    for host in ["codex", "claude"] {
        let path = bootstrap(&home, host);
        let text = fs::read_to_string(&path).unwrap();
        // The description carries the triggers a host matches before it ever
        // loads the body.
        assert!(text.contains("description: Automatically use Exitbind for material coding work"));
        assert!(text.contains("refactors, migrations"));
        assert!(text.contains("Keep read-only questions and tiny obvious reversible edits direct."));
        assert!(text.contains("<!-- exitbind-managed-bootstrap:v1 -->"));
    }

    let after = json(exitbind(&home, &["host", "status", "--json"]));
    assert_eq!(after["binaryVersion"], env!("CARGO_PKG_VERSION"));
    for host in after["hosts"].as_array().unwrap() {
        assert_eq!(host["bootstrapSkill"], "current");
        assert_eq!(host["installedVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(host["hostPresent"], true);
    }
    // Files on disk are discovery, never activation.
    let text = String::from_utf8(exitbind(&home, &["host", "status"]).stdout).unwrap();
    assert!(text.contains("Discovery is not activation"));
    assert!(!text.to_lowercase().contains("active session"));

    let again = json(exitbind(&home, &["host", "install", "--json"]));
    assert_eq!(again["hosts"][0]["action"], "unchanged");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_stale_managed_bridge_is_visible_and_refreshed_while_foreign_files_are_refused() {
    let home = host_home("host-bridge-stale");
    exitbind(&home, &["host", "install", "--json"]);

    // Soulmate-era managed guidance: same path, older managed bytes.
    let codex = bootstrap(&home, "codex");
    fs::write(
        codex.as_path(),
        "---\nname: exitbind\n---\n\n<!-- exitbind-managed-bootstrap:v1 -->\n<!-- exitbind-bootstrap-version: 0.14.0 -->\n",
    )
    .unwrap();
    // A file Exitbind does not manage must never be overwritten.
    let claude = bootstrap(&home, "claude");
    fs::write(&claude, "# my own exitbind notes\n").unwrap();

    let status = json(exitbind(&home, &["host", "status", "--json"]));
    let by_host = |name: &str| -> serde_json::Value {
        status["hosts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["host"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(by_host("codex")["bootstrapSkill"], "stale");
    assert_eq!(by_host("codex")["installedVersion"], "0.14.0");
    assert_eq!(by_host("claude")["bootstrapSkill"], "unmanaged");

    let installed = json(exitbind(&home, &["host", "install", "--json"]));
    let action = |name: &str| -> String {
        installed["hosts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["host"] == name)
            .unwrap()["action"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(action("codex"), "refreshed");
    assert_eq!(action("claude"), "refused");
    assert_eq!(
        fs::read_to_string(&claude).unwrap(),
        "# my own exitbind notes\n"
    );
    let refreshed = json(exitbind(&home, &["host", "status", "--json"]));
    assert_eq!(by_host_state(&refreshed, "codex"), "current");
    assert_eq!(by_host_state(&refreshed, "claude"), "unmanaged");
    fs::remove_dir_all(home).unwrap();
}

fn by_host_state(status: &serde_json::Value, name: &str) -> String {
    status["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["host"] == name)
        .unwrap()["bootstrapSkill"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_broken_or_absent_bridge_is_named_exactly_instead_of_reported_healthy() {
    let home = host_home("host-bridge-broken");
    exitbind(&home, &["host", "install", "--json"]);
    let codex = bootstrap(&home, "codex");
    fs::remove_file(&codex).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", &codex).unwrap();

    let status = json(exitbind(&home, &["host", "status", "--json"]));
    assert_eq!(by_host_state(&status, "codex"), "unsafe");
    let installed = json(exitbind(
        &home,
        &["host", "install", "--hosts", "codex", "--json"],
    ));
    assert_eq!(installed["hosts"][0]["action"], "refused");
    assert!(fs::symlink_metadata(&codex)
        .unwrap()
        .file_type()
        .is_symlink());

    // An absent host is reported, not written to, unless it is named.
    let bare = support::temp("host-bridge-absent");
    fs::create_dir_all(bare.join(".claude")).unwrap();
    let skipped = json(exitbind(&bare, &["host", "install", "--json"]));
    let codex_action = skipped["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["host"] == "codex")
        .unwrap()["action"]
        .clone();
    assert_eq!(codex_action, "skipped_host_absent");
    assert!(!bare.join(".codex/skills/exitbind/SKILL.md").exists());
    assert!(bare.join(".claude/skills/exitbind/SKILL.md").is_file());
    fs::remove_dir_all(home).unwrap();
    fs::remove_dir_all(bare).unwrap();
}

#[test]
fn the_installer_installs_the_bridge_so_one_installation_is_enough() {
    let home = host_home("host-bridge-installer");
    let release = home.join("release");
    let prefix = home.join("bin");
    fs::create_dir_all(&release).unwrap();
    fs::create_dir_all(&prefix).unwrap();
    let target = target_triple();
    let staged = release.join(format!("exitbind-{target}"));
    fs::copy(env!("CARGO_BIN_EXE_exitbind"), &staged).unwrap();
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).unwrap();
    let archive = format!("exitbind-{target}.tar.gz");
    assert!(Command::new("tar")
        .current_dir(&release)
        .args(["-czf", &archive, &format!("exitbind-{target}")])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("sh")
        .current_dir(&release)
        .args([
            "-c",
            "if command -v sha256sum >/dev/null; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi > \"$1.sha256\"",
            "sh",
            &archive,
        ])
        .status()
        .unwrap()
        .success());
    let curl = home.join("fake").join("curl");
    fs::create_dir_all(curl.parent().unwrap()).unwrap();
    fs::write(
        &curl,
        format!(
            "#!/bin/sh\nout=\"\"\nfor arg in \"$@\"; do out=\"$arg\"; done\ncase \"$*\" in\n  *.sha256*) cp '{release}/{archive}.sha256' \"$out\" ;;\n  *.tar.gz*) cp '{release}/{archive}' \"$out\" ;;\n  *) exit 22 ;;\nesac\n",
            release = release.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                curl.parent().unwrap().display(),
                path_with_binary(&home)
            ),
        )
        .env("HOME", &home)
        .env("EXITBIND_INSTALL_PREFIX", &prefix)
        .env(
            "EXITBIND_VERSION",
            format!("v{}", env!("CARGO_PKG_VERSION")),
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(text.contains("host bridge:"), "{text}");
    for host in ["codex", "claude"] {
        assert!(bootstrap(&home, host).is_file(), "{host} bridge missing");
    }
    fs::remove_dir_all(home).unwrap();
}

fn target_triple() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        other => panic!("unsupported platform {other:?}"),
    }
}

/// Discovery must not depend on the repository already being configured: a
/// fresh session in an uninitialized project still learns when to select
/// Exitbind, and still learns that files are not activation.
#[test]
fn an_unconfigured_session_receives_a_small_bootstrap_and_a_subagent_does_not() {
    use std::io::Write;
    use std::process::Stdio;
    let home = host_home("host-bridge-session");
    let project = home.join("repo");
    fs::create_dir_all(&project).unwrap();

    let hook = |event: &str| -> String {
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .arg("hook-run")
            .env("HOME", &home)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let payload =
            serde_json::json!({"hook_event_name": event, "cwd": project.to_string_lossy()});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    };

    let session = hook("SessionStart");
    assert!(session.contains("not configured for it yet"), "{session}");
    assert!(session.contains("select Exitbind before consequential edits"));
    assert!(session.contains("exitbind init --mode portable --root ."));
    assert!(session.contains("tiny obvious reversible edits direct"));
    assert!(session.contains("Do not report Exitbind as active"));
    // Small bootstrap, not the protocol.
    assert!(session.len() < 2048, "bootstrap grew to {}", session.len());
    assert!(!session.contains("run record-check"));

    // A subagent start in an unconfigured project stays silent.
    assert!(hook("SubagentStart").trim().is_empty());
    fs::remove_dir_all(home).unwrap();
}

/// Only the exact record Exitbind itself published for the superseded caller is
/// retired. A wrapper, a variant, a duplicate, or a foreign hook that merely
/// mentions the old command stays untouched and is reported as a conflict.
#[test]
fn only_exact_published_legacy_hook_records_are_retired() {
    const PUBLISHED: &str = "command -v soulmate >/dev/null 2>&1 && soulmate hook-run || true";
    let wrapper = "if [ -f /opt/tool/pre.sh ]; then sh /opt/tool/pre.sh; fi; soulmate hook-run";
    let foreign = "/opt/other-tool/hook.sh --mentions soulmate hook-run";
    let variant = "/opt/legacy/bin/soulmate hook-run || true";

    let home = host_home("host-bridge-hook-exact");
    let hooks = home.join(".codex/hooks.json");
    fs::write(
        &hooks,
        serde_json::json!({
            "hooks": {
                "SessionStart": [
                    {"hooks": [
                        {"type": "command", "command": PUBLISHED, "timeout": 5, "additionalContextLimit": 4096},
                        {"type": "command", "command": PUBLISHED, "timeout": 5, "additionalContextLimit": 4096},
                        {"type": "command", "command": wrapper, "timeout": 5},
                        {"type": "command", "command": foreign, "timeout": 10},
                        {"type": "command", "command": variant, "timeout": 5}
                    ]}
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let installed = json(exitbind(
        &home,
        &["host", "install", "--hosts", "codex", "--json"],
    ));
    assert_eq!(
        installed["hosts"][0]["activationHook"]["state"],
        "installed"
    );

    let document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&hooks).unwrap()).unwrap();
    let commands: Vec<String> = document["hooks"]["SessionStart"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap().clone())
        .map(|handler| handler["command"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        commands.iter().any(|c| c.contains("exitbind hook-run")),
        "{commands:?}"
    );
    // Every published duplicate is retired; nothing else is.
    assert!(!commands.iter().any(|c| c == PUBLISHED), "{commands:?}");
    for preserved in [wrapper, foreign, variant] {
        assert!(
            commands.iter().any(|c| c == preserved),
            "removed a hook it does not own: {preserved}"
        );
    }
    fs::remove_dir_all(home).unwrap();
}

/// A malformed hook document is never rewritten destructively.
#[test]
fn malformed_hook_documents_are_refused_without_rewriting() {
    let home = host_home("host-bridge-hook-malformed");
    let hooks = home.join(".codex/hooks.json");
    let malformed = serde_json::json!({"hooks": {"SessionStart": "not-an-array"}}).to_string();
    fs::write(&hooks, &malformed).unwrap();

    let installed = json(exitbind(
        &home,
        &["host", "install", "--hosts", "codex", "--json"],
    ));
    assert_ne!(
        installed["hosts"][0]["activationHook"]["state"],
        "installed"
    );
    assert_eq!(fs::read_to_string(&hooks).unwrap(), malformed);
    // The bootstrap skill is independent of the hook and still installs.
    assert_eq!(installed["hosts"][0]["action"], "installed");
    fs::remove_dir_all(home).unwrap();
}

/// Bridge writes go through the hardened managed-settings writer: a symlinked
/// parent, a non-regular target, and a target replaced mid-update all fail
/// closed instead of clobbering, and file permissions survive a refresh.
#[test]
fn bridge_writes_fail_closed_on_unsafe_or_concurrently_changed_targets() {
    let home = host_home("host-bridge-write-hardening");

    // A parent path that escapes the host home is refused, so a redirected
    // skills directory cannot be used to write outside it.
    let outside = support::temp("host-bridge-outside");
    std::os::unix::fs::symlink(&outside, home.join(".claude/skills")).unwrap();
    let claude = exitbind(&home, &["host", "install", "--hosts", "claude", "--json"]);
    assert!(!claude.status.success(), "{claude:?}");
    assert!(!outside.join("exitbind/SKILL.md").exists());
    fs::remove_file(home.join(".claude/skills")).unwrap();
    fs::remove_dir_all(&outside).unwrap();

    // A directory where the bootstrap belongs is reported, never replaced.
    fs::create_dir_all(home.join(".codex/skills/exitbind/SKILL.md")).unwrap();
    let codex = json(exitbind(
        &home,
        &["host", "status", "--hosts", "codex", "--json"],
    ));
    assert_eq!(codex["hosts"][0]["bootstrapSkill"], "unsafe");
    let refused = json(exitbind(
        &home,
        &["host", "install", "--hosts", "codex", "--json"],
    ));
    assert_eq!(refused["hosts"][0]["action"], "refused");
    assert!(home.join(".codex/skills/exitbind/SKILL.md").is_dir());

    fs::remove_dir_all(home).unwrap();
}

/// Refreshing a managed bootstrap keeps the permissions the file already had.
#[test]
fn refreshing_a_managed_bootstrap_preserves_its_permissions() {
    let home = host_home("host-bridge-write-permissions");
    exitbind(&home, &["host", "install", "--hosts", "codex", "--json"]);
    let path = bootstrap(&home, "codex");
    fs::write(
        &path,
        "---\nname: exitbind\n---\n\n<!-- exitbind-managed-bootstrap:v1 -->\n<!-- exitbind-bootstrap-version: 0.14.0 -->\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    let refreshed = json(exitbind(
        &home,
        &["host", "install", "--hosts", "codex", "--json"],
    ));
    assert_eq!(refreshed["hosts"][0]["action"], "refreshed");
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "refresh changed the file mode");
    fs::remove_dir_all(home).unwrap();
}

/// The real first install has no `exitbind` on PATH yet. The bootstrap still
/// installs, the PATH-dependent session hook is reported as unavailable rather
/// than silently skipped, and installing again once the CLI resolves completes
/// the bridge.
#[test]
fn a_clean_install_without_the_cli_on_path_reports_the_hook_dependency() {
    let home = host_home("host-bridge-clean-path");
    let prefix = home.join("bin");
    fs::create_dir_all(&prefix).unwrap();
    let installed = prefix.join("exitbind");
    fs::copy(env!("CARGO_BIN_EXE_exitbind"), &installed).unwrap();
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o755)).unwrap();

    let without_path = Command::new(&installed)
        .args(["host", "install", "--json"])
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&without_path.stdout).unwrap();
    for host in value["hosts"].as_array().unwrap() {
        assert_eq!(host["action"], "installed");
        assert_eq!(host["activationHook"]["state"], "unavailable");
        assert!(host["activationHook"]["reason"]
            .as_str()
            .unwrap()
            .contains("PATH"));
    }
    let status = Command::new(&installed)
        .args(["host", "status", "--json"])
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    for host in status["hosts"].as_array().unwrap() {
        assert_eq!(host["bootstrapSkill"], "current");
        assert_ne!(host["activationHook"], "installed");
    }

    // Once the CLI resolves on PATH, the same command completes the bridge.
    let with_path = Command::new(&installed)
        .args(["host", "install", "--json"])
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", prefix.display()))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&with_path.stdout).unwrap();
    for host in value["hosts"].as_array().unwrap() {
        assert_eq!(host["activationHook"]["state"], "installed");
    }
    fs::remove_dir_all(home).unwrap();
}
