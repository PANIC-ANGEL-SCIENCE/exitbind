use super::*;

#[test]
fn update_refreshes_only_present_managed_bridge_files() {
    let release = FakeRelease::at_version("update-competence", env!("CARGO_PKG_VERSION"));
    let home = release.root.join("home");
    let codex = home.join(".codex/skills/exitbind/SKILL.md");
    let reference = codex.parent().unwrap().join("references/competence.md");
    let claude = home.join(".claude/skills/exitbind/SKILL.md");
    fs::create_dir_all(reference.parent().unwrap()).unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    let old = "<!-- exitbind-managed-bootstrap:v1 -->\nold managed bytes\n";
    fs::write(&codex, old).unwrap();

    let (first, _) = release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "missing");
    assert!(first.status.success(), "{first:?}");
    assert!(!reference.exists(), "update recreated a removed reference");
    assert!(!claude.exists(), "update recreated a removed skill");
    assert!(String::from_utf8_lossy(&first.stdout).contains("competence reference missing"));
    assert!(fs::read_to_string(&codex)
        .unwrap()
        .contains("references/competence.md"));

    fs::write(&reference, old).unwrap();
    let (second, _) = release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "stale");
    assert!(second.status.success(), "{second:?}");
    assert!(
        String::from_utf8_lossy(&second.stdout).contains("stale competence reference refreshed")
    );
    assert_eq!(
        fs::read(&reference).unwrap(),
        &include_bytes!("../../skills/exitbind/references/competence.md")[..]
    );

    fs::write(&reference, "operator-owned reference\n").unwrap();
    fs::write(&codex, old).unwrap();
    let (third, _) = release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "foreign");
    assert!(third.status.success(), "{third:?}");
    assert!(String::from_utf8_lossy(&third.stdout).contains("competence reference unmanaged"));
    assert_eq!(
        fs::read_to_string(&reference).unwrap(),
        "operator-owned reference\n"
    );
    assert_eq!(fs::read_to_string(&codex).unwrap(), old);

    fs::remove_file(&reference).unwrap();
    let installed = support::run(
        support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["host", "install", "--hosts", "codex", "--json"])
            .env("HOME", &home)
            .env("PATH", &release.path)
            .env("EXITBIND_NO_UPDATE_CHECK", "1"),
    );
    assert!(installed.status.success(), "{installed:?}");
    assert_eq!(
        fs::read(&reference).unwrap(),
        &include_bytes!("../../skills/exitbind/references/competence.md")[..]
    );
    let script = codex
        .parent()
        .unwrap()
        .join("resources/publication/pre-push");
    fs::remove_file(&script).unwrap();
    let (resource_missing, _) = release.self_update(
        Path::new(env!("CARGO_BIN_EXE_exitbind")),
        "resource-missing",
    );
    assert!(resource_missing.status.success(), "{resource_missing:?}");
    assert!(!script.exists());
    fs::write(
        &script,
        include_bytes!("../../skills/exitbind/resources/publication/pre-push"),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o600)).unwrap();
    let (resource_stale, _) =
        release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "resource-stale");
    assert!(resource_stale.status.success(), "{resource_stale:?}");
    assert!(fs::metadata(&script).unwrap().permissions().mode() & 0o100 != 0);
    fs::write(&script, "foreign executable\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o600)).unwrap();
    let (resource_foreign, _) = release.self_update(
        Path::new(env!("CARGO_BIN_EXE_exitbind")),
        "resource-foreign",
    );
    assert!(resource_foreign.status.success(), "{resource_foreign:?}");
    assert_eq!(
        fs::metadata(&script).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_to_string(&script).unwrap(), "foreign executable\n");
    fs::remove_file(&script).unwrap();
    let reinstalled = support::run(
        support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["host", "install", "--hosts", "codex", "--json"])
            .env("HOME", &home)
            .env("PATH", &release.path)
            .env("EXITBIND_NO_UPDATE_CHECK", "1"),
    );
    assert!(reinstalled.status.success(), "{reinstalled:?}");
    let role = codex.parent().unwrap().join("references/roles/worker.md");
    fs::remove_file(&role).unwrap();
    let (fourth, _) =
        release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "role-missing");
    assert!(fourth.status.success(), "{fourth:?}");
    assert!(!role.exists(), "update recreated a removed role");
    assert!(String::from_utf8_lossy(&fourth.stdout).contains("roles/worker.md missing"));
    fs::write(&role, old).unwrap();
    let (fifth, _) = release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "role-stale");
    assert!(fifth.status.success(), "{fifth:?}");
    assert_eq!(
        fs::read(&role).unwrap(),
        &include_bytes!("../../skills/exitbind/references/roles/worker.md")[..]
    );
    fs::write(&role, "operator-owned role\n").unwrap();
    let (sixth, _) = release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "role-foreign");
    assert!(sixth.status.success(), "{sixth:?}");
    assert!(String::from_utf8_lossy(&sixth.stdout).contains("roles/worker.md unmanaged"));
    assert_eq!(fs::read_to_string(role).unwrap(), "operator-owned role\n");
    fs::remove_dir_all(&release.root).unwrap();
}
