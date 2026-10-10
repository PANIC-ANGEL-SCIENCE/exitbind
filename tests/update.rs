mod support;

mod matrix {
    include!("compatibility_matrix.rs");
}

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fake_curl(root: &Path) {
    let curl = root.join("curl");
    // The default release this fake serves is always one ahead of the build,
    // so bumping the package version cannot turn these updates into no-ops.
    fs::write(
        &curl,
        r##"#!/bin/sh
out=""
for arg in "$@"; do out="$arg"; done
if test -n "$FAKE_CALLS"; then printf '%s\n' "$*" >> "$FAKE_CALLS"; fi
case "$*" in
  *releases*) if [ "$FAKE_BAD" = "1" ]; then printf '%s' '{}' > "$out"; elif test -n "$FAKE_RELEASE_BODY"; then printf '%s' "$FAKE_RELEASE_BODY" > "$out"; else tag="$FAKE_RELEASE_TAG"; test -n "$tag" || tag=v0.21.$(printf '0'); pre="$FAKE_RELEASE_PRERELEASE"; test -n "$pre" || pre=false; printf '%s' "[{\"tag_name\":\"$tag\",\"draft\":false,\"prerelease\":$pre}]" > "$out"; fi ;;
  *) printf '%s' '#!/bin/sh
target="$EXITBIND_INSTALL_PREFIX/exitbind"
if [ "$FAKE_INSTALL_FAIL" = "1" ]; then exit 9; fi
if [ "$FAKE_INSTALL_DIRECTORY" = "1" ]; then rm -f "$EXITBIND_INSTALL_PREFIX/exitbind"; mkdir "$EXITBIND_INSTALL_PREFIX/exitbind"; exit 0; fi
version=${EXITBIND_VERSION#v}
if [ "$FAKE_INSTALL_WRONG" = "1" ]; then version=0.14.0-rc.9; fi
if [ "$FAKE_INSTALL_RENAME" = "1" ]; then stage="$EXITBIND_INSTALL_PREFIX/.exitbind-install-$$"; aside="$EXITBIND_INSTALL_PREFIX/.exitbind-previous-$$"; printf "%s\n" "#!/bin/sh" "if [ \"\$1\" = version ]; then echo $version; fi" > "$stage"; chmod 755 "$stage"; if test -f "$target" && test ! -L "$target"; then mkdir "$aside"; mv -f "$target" "$aside/exitbind"; fi; mv -f "$stage" "$target"; exit 0; fi
printf "%s\n" "#!/bin/sh" "if [ \"\$1\" = version ]; then echo $version; fi" > "$target"
chmod 755 "$target"' > "$out" ;;
esac
"##
        .replace("v0.21.$(printf '0')", &available_update_tag()),
    )
    .unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o700)).unwrap();
}

fn binary(path: &Path, version: &str) {
    fs::write(path, format!("#!/bin/sh\necho {version}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

/// One release ahead of this build. Deriving it means a version bump cannot
/// quietly turn these updates into no-ops that still pass.
fn available_update_tag() -> String {
    let mut parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or_default());
    let major = parts.next().unwrap_or_default();
    let minor = parts.next().unwrap_or_default();
    format!("v{major}.{}.0", minor + 1)
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
    let target = prefix.join("exitbind");
    binary(&target, "0.14.0-rc.1");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_INSTALL_PREFIX", &prefix)
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
    let failed = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_INSTALL_PREFIX", &prefix)
        .env("FAKE_INSTALL_FAIL", "1")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let restored = Command::new(&target).arg("version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&restored.stdout).trim(),
        "0.14.0-rc.1"
    );

    let wrong = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_INSTALL_PREFIX", &prefix)
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
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["help", "advanced"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("exitbind update"));
}

#[test]
fn current_updater_uses_exact_api_and_raw_installer_origin() {
    exercise_matrix_origin(env!("CARGO_BIN_EXE_exitbind"), "current-canonical");
}

#[test]
fn double_update_failure_names_and_retains_private_backup() {
    let root = support::temp("update-double-failure");
    let bin = root.join("bin");
    let prefix = root.join("prefix");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&prefix).unwrap();
    fake_curl(&bin);
    let target = prefix.join("exitbind");
    binary(&target, "0.14.0-rc.1");
    let original = fs::read(&target).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let failed = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_INSTALL_PREFIX", &prefix)
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
                .is_some_and(|name| name.starts_with(".exitbind-old-"))
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
    binary(&prefix.join("exitbind"), "0.14.0-rc.1");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let bad = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", &path)
        .env("FAKE_BAD", "1")
        .env("EXITBIND_INSTALL_PREFIX", &prefix)
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let bad_text = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(bad_text.contains("release metadata"), "{bad_text}");
    let missing = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("update")
        .env("PATH", root.join("missing"))
        .env("EXITBIND_INSTALL_PREFIX", &prefix)
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
            format!("v{}.{}.{}", 0, 21, "00")
        } else {
            format!("v{}.{}.{}+{}.{}", 0, 21, 0, "build", 7)
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

    let upgrade_version = available_update_tag();
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
        .env("FAKE_RELEASE_TAG", &upgrade_version)
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
        upgrade_version.trim_start_matches('v')
    );
    fs::remove_dir_all(upgrade_root).unwrap();
}

/// The real installer replaces the running binary with `mv`. On Linux the
/// running process then sees its executable as deleted; the updater must not
/// change surface mid-update and truncate the freshly installed binary.
#[test]
fn self_update_that_renames_over_the_running_binary_keeps_the_new_binary() {
    let root = support::temp("update-self-rename");
    let bin = root.join("bin");
    let prefix = root.join("prefix");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&prefix).unwrap();
    fake_curl(&bin);
    let target = prefix.join("exitbind");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &target);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut update = Command::new(&target);
    update
        .arg("update")
        .env("PATH", &path)
        .env("HOME", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("EXITBIND_INSTALL_PREFIX")
        .env_remove("EXITBIND_INSTALL_PREFIX")
        .env("FAKE_INSTALL_RENAME", "1");
    let output = support::run(&mut update);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::metadata(&target).unwrap().len() > 0,
        "installed binary was truncated"
    );
    let mut version = Command::new(&target);
    version.arg("version");
    let installed = support::run(&mut version);
    assert_eq!(
        String::from_utf8_lossy(&installed.stdout).trim(),
        available_update_tag().trim_start_matches('v')
    );
    assert!(fs::read_dir(&prefix).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".exitbind-old-")));
    fs::remove_dir_all(root).unwrap();
}

/// A local stand-in for a GitHub release: the real repository `install.sh`, a
/// tarball holding a stub binary that reports `version`, and its checksum.
/// `curl` is replaced so an updater under test reaches only these files.
struct FakeRelease {
    root: std::path::PathBuf,
    path: String,
    version: &'static str,
}

impl FakeRelease {
    fn new(label: &str) -> Self {
        Self::build(
            label,
            Box::leak(
                available_update_tag()
                    .trim_start_matches('v')
                    .to_owned()
                    .into_boxed_str(),
            ),
            None,
        )
    }

    /// A release whose artifact is the real candidate binary rather than a
    /// version-printing stub, so what the old updater installs is the product.
    fn with_candidate(label: &str, version: &'static str) -> Self {
        Self::build(
            label,
            version,
            Some(Path::new(env!("CARGO_BIN_EXE_exitbind"))),
        )
    }

    fn at_version(label: &str, version: &'static str) -> Self {
        Self::build(label, version, None)
    }

    fn build(label: &str, version: &'static str, candidate: Option<&Path>) -> Self {
        let root = support::temp(label);
        let bin = root.join("bin");
        let release = root.join("release");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(&release).unwrap();
        let os = Command::new("uname").arg("-s").output().unwrap();
        let arch = Command::new("uname").arg("-m").output().unwrap();
        let target = match (
            String::from_utf8_lossy(&os.stdout)
                .trim()
                .to_lowercase()
                .as_str(),
            String::from_utf8_lossy(&arch.stdout).trim(),
        ) {
            ("linux", "x86_64" | "amd64") => "x86_64-unknown-linux-gnu",
            ("darwin", "arm64") => "aarch64-apple-darwin",
            ("darwin", "x86_64") => "x86_64-apple-darwin",
            other => panic!("unsupported platform {other:?}"),
        };
        let stub = release.join(format!("exitbind-{target}"));
        match candidate {
            Some(binary) => support::place_executable(binary, &stub),
            None => {
                fs::write(
                    &stub,
                    format!("#!/bin/sh\nif [ \"$1\" = version ]; then echo {version}; fi\n"),
                )
                .unwrap();
                fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let archive = format!("exitbind-{target}.tar.gz");
        let packed = Command::new("tar")
            .current_dir(&release)
            .args(["-czf", &archive, &format!("exitbind-{target}")])
            .status()
            .unwrap();
        assert!(packed.success());
        let checksum = Command::new("sh")
            .current_dir(&release)
            .args([
                "-c",
                "if command -v sha256sum >/dev/null; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi > \"$1.sha256\"",
                "sh",
                &archive,
            ])
            .status()
            .unwrap();
        assert!(checksum.success());
        let installer = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
        let curl = bin.join("curl");
        fs::write(
            &curl,
            format!(
                r##"#!/bin/sh
out=""
url=""
for arg in "$@"; do
  case "$arg" in https://*) url="$arg" ;; esac
  out="$arg"
done
case "$url" in
  *releases\?per_page*) printf '%s' '[{{"tag_name":"v{version}","draft":false,"prerelease":false}}]' > "$out" ;;
  */install.sh) cp '{installer}' "$out" ;;
  *.tar.gz.sha256) cp '{release}/{archive}.sha256' "$out" ;;
  *.tar.gz) cp '{release}/{archive}' "$out" ;;
  *) exit 22 ;;
esac
"##,
                installer = installer.display(),
                release = release.display(),
            ),
        )
        .unwrap();
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o700)).unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        Self {
            root,
            path,
            version,
        }
    }

    /// Run `UPDATER update` from its own install directory, as users do.
    fn self_update(
        &self,
        updater: &Path,
        name: &str,
    ) -> (std::process::Output, std::path::PathBuf) {
        let prefix = self.root.join(format!("prefix-{name}"));
        fs::create_dir(&prefix).unwrap();
        let target = prefix.join("exitbind");
        support::place_executable(updater, &target);
        let output = support::run(
            Command::new(&target)
                .arg("update")
                .env("PATH", &self.path)
                .env("HOME", self.root.join("home"))
                .env("XDG_CACHE_HOME", self.root.join("cache"))
                .env("EXITBIND_NO_UPDATE_CHECK", "1")
                .env("EXITBIND_NO_UPDATE_CHECK", "1")
                .env_remove("EXITBIND_INSTALL_PREFIX")
                .env_remove("EXITBIND_INSTALL_PREFIX")
                .env_remove("EXITBIND_REPOSITORY")
                .env_remove("EXITBIND_VERSION"),
        );
        (output, target)
    }

    fn assert_installed(&self, output: &std::process::Output, target: &Path) {
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            fs::metadata(target).unwrap().len() > 0,
            "installed binary was truncated"
        );
        let installed = support::run(Command::new(target).arg("version"));
        assert_eq!(
            String::from_utf8_lossy(&installed.stdout).trim(),
            self.version
        );
    }
}

#[test]
fn real_installer_self_update_keeps_the_new_binary_and_one_aside_copy() {
    let release = FakeRelease::new("update-real-installer");
    let (output, target) =
        release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "current");
    release.assert_installed(&output, &target);
    let prefix = target.parent().unwrap();
    let asides: Vec<_> = fs::read_dir(prefix)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".exitbind-previous-"))
        .collect();
    assert_eq!(asides.len(), 1, "{asides:?}");
    assert!(prefix.join(&asides[0]).join("exitbind").is_file());
    // A later install removes the earlier aside copy before creating its own.
    let again = Command::new("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
        .env("PATH", &release.path)
        .env("HOME", release.root.join("home"))
        .env("EXITBIND_INSTALL_PREFIX", prefix)
        .env("EXITBIND_VERSION", available_update_tag())
        .output()
        .unwrap();
    assert!(again.status.success(), "{again:?}");
    let remaining = fs::read_dir(prefix)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".exitbind-previous-")
        })
        .count();
    assert_eq!(remaining, 1);
    assert!(!prefix.join(&asides[0]).exists());
    fs::remove_dir_all(&release.root).unwrap();
}

/// Released updaters cannot be changed; the new installer must keep them from
/// truncating what it installs. Set `EXITBIND_RELEASED_UPDATERS` to a
/// colon-separated list of pinned released `exitbind` binaries to run this.
#[test]
fn released_updaters_do_not_truncate_the_binary_the_new_installer_places() {
    let Some(updaters) = std::env::var_os("EXITBIND_RELEASED_UPDATERS") else {
        eprintln!("skipped: set EXITBIND_RELEASED_UPDATERS to released exitbind binaries");
        return;
    };
    let release = FakeRelease::new("update-released-updaters");
    for (index, updater) in std::env::split_paths(&updaters).enumerate() {
        let (output, target) = release.self_update(&updater, &format!("released-{index}"));
        eprintln!(
            "released updater {}: {}",
            updater.display(),
            String::from_utf8_lossy(&output.stdout).trim()
        );
        release.assert_installed(&output, &target);
    }
    fs::remove_dir_all(&release.root).unwrap();
}

/// The one upgrade path that cannot be fixed by future source: a genuinely
/// published updater installing this candidate. Point
/// `EXITBIND_RELEASED_UPDATER` at a pinned released binary to run it.
///
/// What matters after the old process exits is not only that the binary is
/// whole, but whose guidance is on disk. A released updater re-synchronises the
/// host bridge from its own embedded copy after the installer finishes, so it
/// leaves the older guidance behind. This pins that boundary down and tests the
/// one-time repair the release notes give for it.
#[test]
fn a_released_updater_installs_this_candidate_and_its_guidance_needs_one_repair() {
    let Some(updater) = std::env::var_os("EXITBIND_RELEASED_UPDATER") else {
        eprintln!("skipped: set EXITBIND_RELEASED_UPDATER to a pinned released exitbind binary");
        return;
    };
    let updater = PathBuf::from(updater);
    let candidate = Path::new(env!("CARGO_BIN_EXE_exitbind"));
    let release =
        FakeRelease::with_candidate("update-released-candidate", env!("CARGO_PKG_VERSION"));
    let home = release.root.join("home");
    for host in [".codex", ".claude"] {
        fs::create_dir_all(home.join(host)).unwrap();
    }

    let (output, target) = release.self_update(&updater, "released-candidate");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{said}");

    // The installed executable is this candidate, whole and runnable.
    assert_eq!(
        digest(&target),
        digest(candidate),
        "the installed binary is not the candidate: {said}"
    );
    assert!(fs::metadata(&target).unwrap().len() > 0);
    let installed = support::run(Command::new(&target).arg("version"));
    assert!(installed.status.success());
    assert_eq!(
        String::from_utf8_lossy(&installed.stdout).trim(),
        env!("CARGO_PKG_VERSION")
    );

    // The guidance on disk is this candidate's, including the delayed
    // reference that only this version ships.
    let bootstrap = home.join(".codex/skills/exitbind/SKILL.md");
    let status = support::run(
        Command::new(&target)
            .args(["host", "status", "--json"])
            .env("HOME", &home)
            .env("PATH", &release.path)
            .env("EXITBIND_NO_UPDATE_CHECK", "1"),
    );
    assert!(status.status.success(), "{status:?}");
    let seen: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(seen["binaryVersion"], env!("CARGO_PKG_VERSION"));
    let codex = |seen: &Value| {
        seen["hosts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|host| host["host"] == "codex")
            .unwrap()
            .clone()
    };

    // The boundary this test exists to pin down: code already running inside a
    // released updater re-synchronises the bridge from its own embedded copy
    // after the installer finishes, so the guidance on disk is the old one.
    // Future source cannot change that; it can only be recovered from.
    assert_eq!(
        codex(&seen)["bootstrapSkill"],
        "stale",
        "the released updater's behaviour changed: {}",
        fs::read_to_string(&bootstrap).unwrap_or_default()
    );

    // The documented one-time repair, run with the binary that was installed.
    let repaired = support::run(
        Command::new(&target)
            .args(["host", "install", "--hosts", "codex"])
            .env("HOME", &home)
            .env("PATH", &release.path)
            .env("EXITBIND_NO_UPDATE_CHECK", "1"),
    );
    assert!(repaired.status.success(), "{repaired:?}");
    let after = support::run(
        Command::new(&target)
            .args(["host", "status", "--json"])
            .env("HOME", &home)
            .env("PATH", &release.path)
            .env("EXITBIND_NO_UPDATE_CHECK", "1"),
    );
    let after: Value = serde_json::from_slice(&after.stdout).unwrap();
    assert_eq!(codex(&after)["bootstrapSkill"], "current");
    assert_eq!(codex(&after)["installedVersion"], env!("CARGO_PKG_VERSION"));
    assert!(fs::read_to_string(&bootstrap)
        .unwrap()
        .contains(env!("CARGO_PKG_VERSION")));
    fs::remove_dir_all(&release.root).unwrap();
}

fn digest(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path).unwrap());
    format!("{:x}", hasher.finalize())
}

/// Binary current is not host integration current: an up-to-date binary
/// refreshes a stale bootstrap it manages and reports a missing one instead of
/// reinstalling guidance the operator may have removed.
#[test]
fn an_up_to_date_binary_reports_and_repairs_only_what_it_manages() {
    let release = FakeRelease::at_version("update-bridge-health", env!("CARGO_PKG_VERSION"));
    let home = release.root.join("home");
    let codex = home.join(".codex/skills/exitbind/SKILL.md");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(
        &codex,
        "---\nname: exitbind\n---\n\n<!-- exitbind-managed-bootstrap:v1 -->\n<!-- exitbind-bootstrap-version: 0.14.0 -->\n",
    )
    .unwrap();

    let (output, _) = release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "health");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(text.contains("Already up to date"), "{text}");
    assert!(text.contains("codex: stale bootstrap refreshed"), "{text}");
    // Claude's bootstrap is absent, not managed: report it, never reinstall it
    // silently.
    assert!(text.contains("claude: bootstrap missing"), "{text}");
    assert!(!home.join(".claude/skills/exitbind/SKILL.md").exists());
    assert!(fs::read_to_string(&codex)
        .unwrap()
        .contains("Use Exitbind when the actual target and existing project policy make coding work consequential"));
    fs::remove_dir_all(&release.root).unwrap();
}

/// The installed binary owns its own host bridge. An updating process must not
/// synchronize the bridge from its own embedded bootstrap: after the installer
/// places a newer one, writing the updater's bytes back would silently
/// downgrade host guidance.
#[test]
fn updating_never_downgrades_a_newer_managed_host_bridge() {
    let release = FakeRelease::new("update-bridge-ownership");
    let home = release.root.join("home");
    let bridge = home.join(".codex/skills/exitbind/SKILL.md");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::create_dir_all(bridge.parent().unwrap()).unwrap();
    // A bootstrap from a newer Exitbind, exactly as a newer installer would
    // leave it behind before control returns to the older updater process.
    let newer = "---\nname: exitbind\n---\n\n<!-- exitbind-managed-bootstrap:v1 -->\nnewer guidance\n<!-- exitbind-bootstrap-version: 99.0.0 -->\n";
    fs::write(&bridge, newer).unwrap();

    let (output, target) = release.self_update(Path::new(env!("CARGO_BIN_EXE_exitbind")), "owner");
    release.assert_installed(&output, &target);
    assert_eq!(
        fs::read_to_string(&bridge).unwrap(),
        newer,
        "the updating process overwrote a newer managed bootstrap"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Host bridge"), "{text}");
    fs::remove_dir_all(&release.root).unwrap();
}
