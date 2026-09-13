#![cfg(unix)]

mod support;

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

const INSTALLER: &str = "install.sh";
const REPOSITORY: &str = "example/project";
const VERSION: &str = "v99.98.97";

fn executable(path: &Path, contents: impl AsRef<[u8]>) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn command_path(name: &str) -> PathBuf {
    optional_command_path(name).unwrap_or_else(|| panic!("{name} is unavailable"))
}

fn optional_command_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("sh")
        .args(["-c", "command -v \"$1\"", "sh", name])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8(output.stdout).unwrap().trim()))
}

fn digest(path: &Path) -> String {
    let output = if optional_command_path("sha256sum").is_some() {
        Command::new("sha256sum").arg(path).output().unwrap()
    } else {
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
            .unwrap()
    };
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

struct Fixture {
    root: PathBuf,
    bin: PathBuf,
    archive: PathBuf,
    checksum: PathBuf,
    payload: Vec<u8>,
    target: String,
}

impl Fixture {
    fn new(target: &str) -> Self {
        let root = support::temp(&format!("installer-{target}"));
        let bin = root.join("bin");
        let server = root.join("server");
        let payload_dir = root.join("payload");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&server).unwrap();
        fs::create_dir_all(&payload_dir).unwrap();

        let stem = format!("soulmate-{target}");
        let payload = b"fixture release executable\n".to_vec();
        let payload_path = payload_dir.join(&stem);
        executable(&payload_path, &payload);
        let archive = server.join(format!("{stem}.tar.gz"));
        let status = Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .args(["-C"])
            .arg(&payload_dir)
            .arg(&stem)
            .status()
            .unwrap();
        assert!(status.success(), "tar failed: {status}");
        let checksum = server.join(format!("{stem}.tar.gz.sha256"));
        let checksum_name = archive.file_name().unwrap().to_string_lossy();
        fs::write(
            &checksum,
            format!("{}  {checksum_name}\n", digest(&archive)),
        )
        .unwrap();

        executable(
            &bin.join("uname"),
            br#"#!/bin/sh
set -eu
case "$1" in
  -s) printf '%s\n' "$INSTALLER_UNAME_S" ;;
  -m) printf '%s\n' "$INSTALLER_UNAME_M" ;;
  *) exit 2 ;;
esac
"#,
        );
        executable(
            &bin.join("curl"),
            br#"#!/bin/sh
set -eu
test "$#" -eq 4 && test "$1" = -fsSL && test "$3" = -o || exit 2
printf '%s\n' "$2" >> "$INSTALLER_CALLS"
case "$2" in
  "$INSTALLER_BASE/$INSTALLER_ARCHIVE") cp "$INSTALLER_ARCHIVE_SOURCE" "$4" ;;
  "$INSTALLER_BASE/$INSTALLER_CHECKSUM") cp "$INSTALLER_CHECKSUM_SOURCE" "$4" ;;
  *) exit 1 ;;
esac
"#,
        );
        Self {
            root,
            bin,
            archive,
            checksum,
            payload,
            target: target.to_owned(),
        }
    }

    fn path(&self) -> OsString {
        std::env::join_paths(
            std::iter::once(self.bin.clone())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap()
    }

    fn command(&self, os: &str, arch: &str, prefix: &Path, path: OsString) -> Command {
        let stem = format!("soulmate-{}", self.target);
        let archive = format!("{stem}.tar.gz");
        let checksum = format!("{archive}.sha256");
        let calls = self.root.join("calls");
        fs::write(&calls, b"").unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join(INSTALLER))
            .env("PATH", path)
            .env("HOME", self.root.join("home"))
            .env("SOULMATE_INSTALL_PREFIX", prefix)
            .env("SOULMATE_REPOSITORY", REPOSITORY)
            .env("SOULMATE_VERSION", VERSION)
            .env("INSTALLER_UNAME_S", os)
            .env("INSTALLER_UNAME_M", arch)
            .env("INSTALLER_CALLS", &calls)
            .env(
                "INSTALLER_BASE",
                format!("https://github.com/{REPOSITORY}/releases/download/{VERSION}"),
            )
            .env("INSTALLER_ARCHIVE", &archive)
            .env("INSTALLER_CHECKSUM", &checksum)
            .env("INSTALLER_ARCHIVE_SOURCE", &self.archive)
            .env("INSTALLER_CHECKSUM_SOURCE", &self.checksum);
        command
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.root.join("calls")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn supported_platforms_map_to_their_native_archives() {
    for (os, arch, target) in [
        ("Linux", "x86_64", "x86_64-unknown-linux-gnu"),
        ("Linux", "amd64", "x86_64-unknown-linux-gnu"),
        ("Darwin", "arm64", "aarch64-apple-darwin"),
        ("Darwin", "x86_64", "x86_64-apple-darwin"),
    ] {
        let fixture = Fixture::new(target);
        let prefix = fixture.root.join("prefix with spaces");
        let output = fixture
            .command(os, arch, &prefix, fixture.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{os} {arch}: {output:?}");
        assert_eq!(fs::read(prefix.join("soulmate")).unwrap(), fixture.payload);
        assert_ne!(
            fs::metadata(prefix.join("soulmate"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        let base = format!("https://github.com/{REPOSITORY}/releases/download/{VERSION}");
        assert_eq!(
            fixture.calls(),
            format!("{base}/soulmate-{target}.tar.gz\n{base}/soulmate-{target}.tar.gz.sha256\n")
        );
    }
}

#[test]
fn unsupported_platform_refuses_before_fetch_and_preserves_installation() {
    let fixture = Fixture::new("x86_64-unknown-linux-gnu");
    let prefix = fixture.root.join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    let installed = prefix.join("soulmate");
    fs::write(&installed, b"operator-installed binary\n").unwrap();

    for (os, arch) in [("FreeBSD", "x86_64"), ("Darwin", "amd64")] {
        let output = fixture
            .command(os, arch, &prefix, fixture.path())
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "unexpected pass: {os} {arch}: {output:?}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported platform"));
        assert_eq!(
            fs::read(&installed).unwrap(),
            b"operator-installed binary\n"
        );
        assert!(
            fixture.calls().is_empty(),
            "unsupported platform fetched files"
        );
    }
}

#[test]
fn checksum_failure_does_not_fall_back_or_clobber_existing_binary() {
    let fixture = Fixture::new("aarch64-apple-darwin");
    let checksum_name = fixture.checksum.file_name().unwrap().to_string_lossy();
    fs::write(
        &fixture.checksum,
        format!("{}  {checksum_name}\n", "0".repeat(64)),
    )
    .unwrap();
    executable(
        &fixture.bin.join("sha256sum"),
        br#"#!/bin/sh
printf '%s\n' sha256sum >> "$INSTALLER_TOOLS"
exit 1
"#,
    );
    executable(
        &fixture.bin.join("shasum"),
        br#"#!/bin/sh
printf '%s\n' shasum >> "$INSTALLER_TOOLS"
exit 0
"#,
    );
    let tools = fixture.root.join("tools-used");
    fs::write(&tools, b"").unwrap();
    let prefix = fixture.root.join("prefix with spaces");
    fs::create_dir_all(&prefix).unwrap();
    let installed = prefix.join("soulmate");
    fs::write(&installed, b"operator-installed binary\n").unwrap();

    let output = fixture
        .command("Darwin", "arm64", &prefix, fixture.path())
        .env("INSTALLER_TOOLS", &tools)
        .output()
        .unwrap();
    assert!(!output.status.success(), "unexpected pass: {output:?}");
    assert_eq!(
        fs::read(&installed).unwrap(),
        b"operator-installed binary\n"
    );
    assert_eq!(fs::read_to_string(tools).unwrap(), "sha256sum\n");
}

#[test]
fn shasum_fallback_works_when_sha256sum_is_unavailable() {
    let fixture = Fixture::new("x86_64-apple-darwin");
    let tools = fixture.root.join("minimal-tools");
    fs::create_dir_all(&tools).unwrap();
    for name in [
        "tr", "mktemp", "find", "mkdir", "tar", "gzip", "install", "mv", "cp",
    ] {
        symlink(command_path(name), tools.join(name)).unwrap();
    }
    let real_tool = if optional_command_path("sha256sum").is_some() {
        "sha256sum"
    } else {
        "shasum"
    };
    executable(
        &tools.join("shasum"),
        br#"#!/bin/sh
set -eu
if test "$INSTALLER_REAL_SHA_KIND" = sha256sum && test "$1" = -a; then shift 2; fi
exec "$INSTALLER_REAL_SHA" "$@"
"#,
    );
    let path = std::env::join_paths([fixture.bin.clone(), tools.clone()]).unwrap();
    let output = fixture
        .command(
            "Darwin",
            "x86_64",
            &fixture.root.join("prefix with spaces"),
            path,
        )
        .env("INSTALLER_REAL_SHA", command_path(real_tool))
        .env("INSTALLER_REAL_SHA_KIND", real_tool)
        .output()
        .unwrap();
    assert!(output.status.success(), "fallback failed: {output:?}");
    assert_eq!(
        fs::read(fixture.root.join("prefix with spaces").join("soulmate")).unwrap(),
        fixture.payload
    );
}
