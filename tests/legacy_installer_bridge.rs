#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const TARGET: &str = "x86_64-unknown-linux-gnu";
const VERSION: &str = "v0.17.0";

struct Fixture {
    root: PathBuf,
    archive: PathBuf,
    checksum: PathBuf,
    payload: Vec<u8>,
    calls: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        Self::new_with_asset(
            label,
            "exitbind",
            b"#!/bin/sh\nif [ \"$1\" = version ]; then printf '%s\\n' 0.17.0; fi\n",
        )
    }

    fn new_with_asset(label: &str, asset_surface: &str, payload: &[u8]) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("exitbind-legacy-installer-{label}-{stamp}"));
        let bin = root.join("bin");
        let server = root.join("server");
        let payload_dir = root.join("payload");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&server).unwrap();
        fs::create_dir_all(&payload_dir).unwrap();

        let payload = payload.to_vec();
        let stem = format!("{asset_surface}-{TARGET}");
        let payload_path = payload_dir.join(&stem);
        executable(&payload_path, &payload);
        let archive = server.join(format!("{stem}.tar.gz"));
        assert!(Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .args(["-C"])
            .arg(&payload_dir)
            .arg(&stem)
            .status()
            .unwrap()
            .success());
        let checksum = server.join(format!("{stem}.tar.gz.sha256"));
        let digest = String::from_utf8(
            Command::new("sha256sum")
                .arg(&archive)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
        fs::write(&checksum, format!("{digest}  {stem}.tar.gz\n")).unwrap();

        executable(
            &bin.join("uname"),
            b"#!/bin/sh\ncase \"$1\" in -s) echo Linux;; -m) echo x86_64;; *) exit 2;; esac\n",
        );
        let calls = root.join("calls");
        executable(
            &bin.join("curl"),
            br#"#!/bin/sh
set -eu
test "$#" -eq 4 && test "$1" = -fsSL && test "$3" = -o
printf '%s\n' "$2" >> "$BRIDGE_CALLS"
case "$2" in
  "$BRIDGE_ARCHIVE_URL") cp "$BRIDGE_ARCHIVE_SOURCE" "$4" ;;
  "$BRIDGE_CHECKSUM_URL") cp "$BRIDGE_CHECKSUM_SOURCE" "$4" ;;
  *) exit 1 ;;
esac
"#,
        );
        Self {
            root,
            archive,
            checksum,
            payload,
            calls,
        }
    }

    fn path(&self) -> String {
        let bin = self.root.join("bin");
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
    }

    fn run(&self, prefix: &Path, legacy: bool) -> std::process::Output {
        self.run_version(prefix, legacy, VERSION, "exitbind")
    }

    fn run_version(
        &self,
        prefix: &Path,
        legacy: bool,
        version: &str,
        asset_surface: &str,
    ) -> std::process::Output {
        self.run_version_with_repo(
            prefix,
            legacy,
            version,
            asset_surface,
            "envy404/exitbind",
        )
    }

    fn run_version_with_repo(
        &self,
        prefix: &Path,
        legacy: bool,
        version: &str,
        asset_surface: &str,
        repository: &str,
    ) -> std::process::Output {
        let archive_url = format!(
            "https://github.com/{repository}/releases/download/{version}/{asset_surface}-{TARGET}.tar.gz"
        );
        let checksum_url = format!(
            "https://github.com/{repository}/releases/download/{version}/{asset_surface}-{TARGET}.tar.gz.sha256"
        );
        let legacy_repository = if repository == "envy404/exitbind" {
            "envy404/soulmate"
        } else {
            repository
        };
        let mut command = Command::new("sh");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .env("PATH", self.path())
            .env("HOME", self.root.join("home"))
            .env("BRIDGE_CALLS", &self.calls)
            .env("BRIDGE_ARCHIVE_URL", &archive_url)
            .env("BRIDGE_CHECKSUM_URL", &checksum_url)
            .env("BRIDGE_ARCHIVE_SOURCE", &self.archive)
            .env("BRIDGE_CHECKSUM_SOURCE", &self.checksum);
        if legacy {
            command
                .env_remove("EXITBIND_REPOSITORY")
                .env_remove("EXITBIND_VERSION")
                .env_remove("EXITBIND_INSTALL_PREFIX")
                .env("SOULMATE_REPOSITORY", legacy_repository)
                .env("SOULMATE_VERSION", version)
                .env("SOULMATE_INSTALL_PREFIX", prefix);
        } else {
            command
                .env("EXITBIND_REPOSITORY", "envy404/exitbind")
                .env("EXITBIND_VERSION", VERSION)
                .env("EXITBIND_INSTALL_PREFIX", prefix)
                .env("SOULMATE_REPOSITORY", "envy404/soulmate")
                .env("SOULMATE_VERSION", "v0.16.0")
                .env("SOULMATE_INSTALL_PREFIX", self.root.join("legacy-prefix"));
        }
        command.output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn executable(path: &Path, contents: impl AsRef<[u8]>) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn legacy_soulmate_inputs_fetch_exitbind_assets_and_leave_both_names() {
    let fixture = Fixture::new("legacy");
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output = fixture.run(&prefix, true);
    assert!(output.status.success(), "{output:?}");
    for name in ["soulmate", "exitbind"] {
        let installed = prefix.join(name);
        assert!(installed.is_file());
        assert_eq!(fs::read(&installed).unwrap(), fixture.payload);
        assert_ne!(
            fs::metadata(installed).unwrap().permissions().mode() & 0o111,
            0
        );
        assert_eq!(
            String::from_utf8(
                Command::new(prefix.join(name))
                    .arg("version")
                    .output()
                    .unwrap()
                    .stdout
            )
            .unwrap()
            .trim(),
            "0.17.0"
        );
    }
    assert_eq!(fs::read_to_string(&fixture.calls).unwrap(), format!(
        "https://github.com/envy404/exitbind/releases/download/{VERSION}/exitbind-{TARGET}.tar.gz\nhttps://github.com/envy404/exitbind/releases/download/{VERSION}/exitbind-{TARGET}.tar.gz.sha256\n"
    ));
}

#[test]
fn historical_soulmate_version_fetches_historical_asset_and_name_only() {
    const HISTORICAL_VERSION: &str = "v0.16.0";
    let fixture = Fixture::new_with_asset(
        "historical",
        "soulmate",
        b"#!/bin/sh\nprintf '%s\\n' historical-soulmate\n",
    );
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output = fixture.run_version(&prefix, true, HISTORICAL_VERSION, "soulmate");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(prefix.join("soulmate")).unwrap(), fixture.payload);
    assert!(!prefix.join("exitbind").exists());
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        format!(
            "https://github.com/envy404/exitbind/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz\nhttps://github.com/envy404/exitbind/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz.sha256\n"
        )
    );
}

#[test]
fn historical_stable_version_fetches_historical_asset_and_name_only() {
    const HISTORICAL_VERSION: &str = "v0.12.0";
    let fixture = Fixture::new_with_asset(
        "historical-stable",
        "soulmate",
        b"#!/bin/sh\nprintf '%s\\n' historical-soulmate-stable\n",
    );
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output = fixture.run_version(&prefix, true, HISTORICAL_VERSION, "soulmate");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(prefix.join("soulmate")).unwrap(), fixture.payload);
    assert!(!prefix.join("exitbind").exists());
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        format!(
            "https://github.com/envy404/exitbind/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz\nhttps://github.com/envy404/exitbind/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz.sha256\n"
        )
    );
}

#[test]
fn historical_version_with_build_metadata_fetches_historical_asset_and_name_only() {
    const HISTORICAL_VERSION: &str = "v0.16.999-rc.2+build.7";
    let fixture = Fixture::new_with_asset(
        "historical-build",
        "soulmate",
        b"#!/bin/sh\nprintf '%s\\n' historical-soulmate-build\n",
    );
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output = fixture.run_version(&prefix, true, HISTORICAL_VERSION, "soulmate");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(prefix.join("soulmate")).unwrap(), fixture.payload);
    assert!(!prefix.join("exitbind").exists());
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        format!(
            "https://github.com/envy404/exitbind/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz\nhttps://github.com/envy404/exitbind/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz.sha256\n"
        )
    );
}

#[test]
fn malformed_version_keeps_current_bridge_asset_path() {
    const MALFORMED_VERSION: &str = "v0.16";
    let fixture = Fixture::new("malformed");
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output = fixture.run_version(&prefix, true, MALFORMED_VERSION, "exitbind");
    assert!(output.status.success(), "{output:?}");
    for name in ["soulmate", "exitbind"] {
        assert_eq!(fs::read(prefix.join(name)).unwrap(), fixture.payload);
    }
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        format!(
            "https://github.com/envy404/exitbind/releases/download/{MALFORMED_VERSION}/exitbind-{TARGET}.tar.gz\nhttps://github.com/envy404/exitbind/releases/download/{MALFORMED_VERSION}/exitbind-{TARGET}.tar.gz.sha256\n"
        )
    );
}

#[test]
fn leading_zero_patch_keeps_current_bridge_asset_path() {
    const MALFORMED_VERSION: &str = "v0.16.00";
    let fixture = Fixture::new("leading-zero-patch");
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output = fixture.run_version(&prefix, true, MALFORMED_VERSION, "exitbind");
    assert!(output.status.success(), "{output:?}");
    for name in ["soulmate", "exitbind"] {
        assert_eq!(fs::read(prefix.join(name)).unwrap(), fixture.payload);
    }
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        format!(
            "https://github.com/envy404/exitbind/releases/download/{MALFORMED_VERSION}/exitbind-{TARGET}.tar.gz\nhttps://github.com/envy404/exitbind/releases/download/{MALFORMED_VERSION}/exitbind-{TARGET}.tar.gz.sha256\n"
        )
    );
}

#[test]
fn custom_legacy_repository_keeps_soulmate_asset_and_name() {
    const HISTORICAL_VERSION: &str = "v0.12.0";
    const REPOSITORY: &str = "example/project";
    let fixture = Fixture::new_with_asset(
        "custom-repository",
        "soulmate",
        b"#!/bin/sh\nprintf '%s\\n' custom-soulmate\n",
    );
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output =
        fixture.run_version_with_repo(&prefix, true, HISTORICAL_VERSION, "soulmate", REPOSITORY);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(prefix.join("soulmate")).unwrap(), fixture.payload);
    assert!(!prefix.join("exitbind").exists());
    assert_eq!(
        fs::read_to_string(&fixture.calls).unwrap(),
        format!(
            "https://github.com/{REPOSITORY}/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz\nhttps://github.com/{REPOSITORY}/releases/download/{HISTORICAL_VERSION}/soulmate-{TARGET}.tar.gz.sha256\n"
        )
    );
}

#[test]
fn exitbind_inputs_keep_normal_install_single_named_target() {
    let fixture = Fixture::new("current");
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let output = fixture.run(&prefix, false);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(prefix.join("exitbind")).unwrap(), fixture.payload);
    assert!(!prefix.join("soulmate").exists());
}
