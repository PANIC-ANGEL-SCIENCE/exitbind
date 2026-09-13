mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn fake_curl(root: &Path) {
    let curl = root.join("curl");
    fs::write(
        &curl,
        r##"#!/bin/sh
out=""
for arg in "$@"; do out="$arg"; done
version=0.18.0
case "$*" in
  *releases*) if [ "$FAKE_BAD" = "1" ]; then printf '%s' '{}' > "$out"; else printf '%s' "[{\"tag_name\":\"v$version\",\"draft\":false,\"prerelease\":false}]" > "$out"; fi ;;
  *) printf '%s' '#!/bin/sh
target="$EXITBIND_INSTALL_PREFIX/exitbind"
if [ "$FAKE_INSTALL_FAIL" = "1" ]; then exit 9; fi
if [ "$FAKE_INSTALL_DIRECTORY" = "1" ]; then rm -f "$SOULMATE_INSTALL_PREFIX/soulmate"; mkdir "$SOULMATE_INSTALL_PREFIX/soulmate"; exit 0; fi
version=0.18.0
if [ "$FAKE_INSTALL_WRONG" = "1" ]; then version=0.14.0-rc.9; fi
printf "%s\n" "#!/bin/sh" "if [ \"\$1\" = version ]; then echo $version; fi" > "$target"
chmod 755 "$target"' > "$out" ;;
esac
"##,
    )
    .unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o700)).unwrap();
}

fn binary(path: &Path, version: &str) {
    fs::write(path, format!("#!/bin/sh\necho {version}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn explicit_update_uses_fixed_fake_release_and_restores_on_failure() {
    let root = support::temp("update");
    let bin = root.join("bin");
    let prefix = root.join("prefix");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&prefix).unwrap();
    fake_curl(&bin);
    let target = prefix.join("soulmate");
    binary(&target, "0.14.0-rc.1");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("SOULMATE_NO_UPDATE_CHECK", "1")
        .env("SOULMATE_INSTALL_PREFIX", &prefix)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let installed = Command::new(&target).arg("version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&installed.stdout).trim(), "0.18.0");

    binary(&target, "0.14.0-rc.1");
    let failed = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("SOULMATE_NO_UPDATE_CHECK", "1")
        .env("SOULMATE_INSTALL_PREFIX", &prefix)
        .env("FAKE_INSTALL_FAIL", "1")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let restored = Command::new(&target).arg("version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&restored.stdout).trim(),
        "0.14.0-rc.1"
    );

    let wrong = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("SOULMATE_NO_UPDATE_CHECK", "1")
        .env("SOULMATE_INSTALL_PREFIX", &prefix)
        .env("FAKE_INSTALL_WRONG", "1")
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    let restored_again = Command::new(&target).arg("version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&restored_again.stdout).trim(),
        "0.14.0-rc.1"
    );
}

#[test]
fn update_is_discoverable_in_advanced_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(["help", "advanced"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("soulmate update"));
}

#[test]
fn double_update_failure_names_and_retains_private_backup() {
    let root = support::temp("update-double-failure");
    let bin = root.join("bin");
    let prefix = root.join("prefix");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&prefix).unwrap();
    fake_curl(&bin);
    let target = prefix.join("soulmate");
    binary(&target, "0.14.0-rc.1");
    let original = fs::read(&target).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let failed = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("SOULMATE_NO_UPDATE_CHECK", "1")
        .env("SOULMATE_INSTALL_PREFIX", &prefix)
        .env("FAKE_INSTALL_DIRECTORY", "1")
        .output()
        .unwrap();
    assert!(!failed.status.success());

    let backup = fs::read_dir(&prefix)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".soulmate-old-"))
        })
        .expect("double failure must retain its backup");
    assert_eq!(fs::read(&backup).unwrap(), original);
    assert_eq!(
        fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(message.contains(backup.to_str().unwrap()), "{message}");
    assert!(message.contains(target.to_str().unwrap()), "{message}");
    assert!(!message.contains("restore "), "{message}");
    assert!(target.is_dir());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_release_response_and_missing_curl_are_errors() {
    let root = support::temp("update-errors");
    let bin = root.join("bin");
    let prefix = root.join("prefix");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&prefix).unwrap();
    fake_curl(&bin);
    binary(&prefix.join("soulmate"), "0.14.0-rc.1");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let bad = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("update")
        .env("PATH", &path)
        .env("FAKE_BAD", "1")
        .env("SOULMATE_INSTALL_PREFIX", &prefix)
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let bad_text = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(bad_text.contains("release metadata"), "{bad_text}");
    let missing = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("update")
        .env("PATH", root.join("missing"))
        .env("SOULMATE_INSTALL_PREFIX", &prefix)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("curl unavailable"));
}
