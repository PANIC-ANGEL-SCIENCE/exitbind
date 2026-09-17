mod support;

mod matrix {
    include!("compatibility_matrix.rs");
}

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
if test -n "$FAKE_CALLS"; then printf '%s\n' "$*" >> "$FAKE_CALLS"; fi
case "$*" in
  *releases*) if [ "$FAKE_BAD" = "1" ]; then printf '%s' '{}' > "$out"; elif test -n "$FAKE_RELEASE_BODY"; then printf '%s' "$FAKE_RELEASE_BODY" > "$out"; else tag="$FAKE_RELEASE_TAG"; test -n "$tag" || tag=v0.20.$(printf '0'); pre="$FAKE_RELEASE_PRERELEASE"; test -n "$pre" || pre=false; printf '%s' "[{\"tag_name\":\"$tag\",\"draft\":false,\"prerelease\":$pre}]" > "$out"; fi ;;
  *) printf '%s' '#!/bin/sh
target="$EXITBIND_INSTALL_PREFIX/exitbind"
if [ "$FAKE_INSTALL_FAIL" = "1" ]; then exit 9; fi
if [ "$FAKE_INSTALL_DIRECTORY" = "1" ]; then rm -f "$SOULMATE_INSTALL_PREFIX/soulmate"; mkdir "$SOULMATE_INSTALL_PREFIX/soulmate"; exit 0; fi
version=${EXITBIND_VERSION#v}
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

fn available_update_tag() -> String {
    format!("v{}.{}.{}", 0, 20, 0)
}

fn exercise_matrix_origin(binary_path: &str, route_id: &str) {
    let route = matrix::route(route_id);
    let surface = matrix::row("surfaces", route["callerSurfaceId"].as_str().unwrap());
    let target_name = surface["callerBasename"].as_str().unwrap();
    let origin = matrix::row("origins", route["originId"].as_str().unwrap());
    let env_prefix = origin["installPrefixEnv"].as_str().unwrap();
    let root = support::temp(&format!("update-origin-{target_name}"));
    let bin = root.join("bin");
    let prefix = root.join("prefix");
    let calls = root.join("calls");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&prefix).unwrap();
    fake_curl(&bin);
    binary(&prefix.join(target_name), "0.17.0");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(binary_path)
        .arg("update")
        .env("PATH", path)
        .env("HOME", &root)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env(env_prefix, &prefix)
        .env("FAKE_RELEASE_TAG", available_update_tag())
        .env("FAKE_CALLS", &calls)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let calls_text = fs::read_to_string(&calls).unwrap();
    assert!(
        calls_text.contains(origin["api"].as_str().unwrap()),
        "{calls_text}"
    );
    assert!(
        calls_text.contains(&format!(
            "{}{}/install.sh",
            origin["rawInstaller"].as_str().unwrap(),
            available_update_tag()
        )),
        "{calls_text}"
    );
    let other_origin = if origin["id"] == "current-updater" {
        matrix::row("origins", "legacy-updater")
    } else {
        matrix::row("origins", "current-updater")
    };
    assert!(
        !calls_text.contains(other_origin["api"].as_str().unwrap()),
        "{calls_text}"
    );
    assert!(
        !calls_text.contains(other_origin["rawInstaller"].as_str().unwrap()),
        "{calls_text}"
    );
    assert_eq!(
        String::from_utf8_lossy(
            &Command::new(prefix.join(target_name))
                .arg("version")
                .output()
                .unwrap()
                .stdout
        )
        .trim(),
        available_update_tag().trim_start_matches('v')
    );
    fs::remove_dir_all(root).unwrap();
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
    assert_eq!(
        String::from_utf8_lossy(&installed.stdout).trim(),
        available_update_tag().trim_start_matches('v')
    );

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
fn updater_matrix_drives_exact_api_and_raw_installer_origins_for_both_callers() {
    exercise_matrix_origin(env!("CARGO_BIN_EXE_exitbind"), "current-canonical");
    exercise_matrix_origin(
        env!("CARGO_BIN_EXE_soulmate"),
        "canonical-legacy-historical",
    );
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

#[test]
fn updater_matrix_cases_preserve_parser_boundaries_with_a_valid_control() {
    let invalid_cases = ["updater-v01600-reject", "updater-build-reject"];
    for case_id in invalid_cases {
        let case = matrix::case(case_id);
        let root = support::temp(case_id);
        let bin = root.join("bin");
        let prefix = root.join("prefix");
        let calls = root.join("calls");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(&prefix).unwrap();
        fake_curl(&bin);
        let target = prefix.join("exitbind");
        binary(&target, "0.17.0");
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let malformed = case["version"].as_str().unwrap();
        let discriminating_malformed = if case_id == "updater-v01600-reject" {
            format!("v{}.{}.{}", 0, 20, "00")
        } else {
            format!("v{}.{}.{}+{}.{}", 0, 20, 0, "build", 7)
        };
        let valid = available_update_tag();
        let body = format!(
            "[{{\"tag_name\":\"{malformed}\",\"draft\":false,\"prerelease\":false}},{{\"tag_name\":\"{discriminating_malformed}\",\"draft\":false,\"prerelease\":false}},{{\"tag_name\":\"{valid}\",\"draft\":false,\"prerelease\":false}}]"
        );
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .arg("update")
            .env("PATH", path)
            .env("HOME", &root)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env("EXITBIND_INSTALL_PREFIX", &prefix)
            .env("FAKE_RELEASE_BODY", body)
            .env("FAKE_CALLS", &calls)
            .output()
            .unwrap();
        assert!(output.status.success(), "{case_id}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(&format!(
                "Updated exitbind to {}",
                valid.trim_start_matches('v')
            )),
            "{case_id}: {:?}",
            output
        );
        assert_eq!(
            String::from_utf8_lossy(
                &Command::new(&target)
                    .arg("version")
                    .output()
                    .unwrap()
                    .stdout,
            )
            .trim(),
            valid.trim_start_matches('v')
        );
        assert!(
            fs::read_to_string(&calls)
                .unwrap()
                .contains("repos/envy404/exitbind/releases"),
            "{case_id}: updater repository route"
        );
        let calls_text = fs::read_to_string(&calls).unwrap();
        let installer_call = calls_text
            .lines()
            .find(|line| line.contains("install.sh"))
            .unwrap_or_else(|| panic!("{case_id}: missing raw installer call: {calls_text}"));
        let route = matrix::route(case["routeId"].as_str().unwrap());
        let origin = matrix::row("origins", route["originId"].as_str().unwrap());
        let expected_installer = format!(
            "{}{}/install.sh",
            origin["rawInstaller"].as_str().unwrap(),
            valid
        );
        assert!(
            installer_call.contains(&expected_installer),
            "{installer_call}"
        );
        assert!(!installer_call.contains(malformed), "{installer_call}");
        assert!(
            !installer_call.contains(&discriminating_malformed),
            "{installer_call}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    let stable_case = matrix::case("updater-stable-ignore-prerelease");
    let stable_root = support::temp("updater-stable-prerelease");
    let stable_bin = stable_root.join("bin");
    let stable_prefix = stable_root.join("prefix");
    fs::create_dir(&stable_bin).unwrap();
    fs::create_dir(&stable_prefix).unwrap();
    fake_curl(&stable_bin);
    let stable_target = stable_prefix.join("exitbind");
    binary(&stable_target, "0.17.0");
    let stable_path = format!(
        "{}:{}",
        stable_bin.display(),
        std::env::var("PATH").unwrap()
    );
    let ignored = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", stable_path)
        .env("HOME", &stable_root)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_INSTALL_PREFIX", &stable_prefix)
        .env("FAKE_RELEASE_TAG", stable_case["version"].as_str().unwrap())
        .env("FAKE_RELEASE_PRERELEASE", "true")
        .output()
        .unwrap();
    assert!(ignored.status.success(), "{ignored:?}");
    assert_eq!(
        String::from_utf8_lossy(&Command::new(&stable_target).output().unwrap().stdout).trim(),
        "0.17.0"
    );
    fs::remove_dir_all(stable_root).unwrap();

    let upgrade_case = matrix::case("updater-prerelease-order");
    let upgrade_root = support::temp("updater-valid-control");
    let upgrade_bin = upgrade_root.join("bin");
    let upgrade_prefix = upgrade_root.join("prefix");
    fs::create_dir(&upgrade_bin).unwrap();
    fs::create_dir(&upgrade_prefix).unwrap();
    fake_curl(&upgrade_bin);
    let upgrade_target = upgrade_prefix.join("exitbind");
    binary(&upgrade_target, "0.17.0");
    let upgrade_path = format!(
        "{}:{}",
        upgrade_bin.display(),
        std::env::var("PATH").unwrap()
    );
    let upgraded = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", upgrade_path)
        .env("HOME", &upgrade_root)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_INSTALL_PREFIX", &upgrade_prefix)
        .env(
            "FAKE_RELEASE_TAG",
            upgrade_case["version"].as_str().unwrap(),
        )
        .output()
        .unwrap();
    assert!(upgraded.status.success(), "{upgraded:?}");
    assert_eq!(
        String::from_utf8_lossy(
            &Command::new(&upgrade_target)
                .arg("version")
                .output()
                .unwrap()
                .stdout
        )
        .trim(),
        upgrade_case["version"]
            .as_str()
            .unwrap()
            .trim_start_matches('v')
    );
    fs::remove_dir_all(upgrade_root).unwrap();
}
