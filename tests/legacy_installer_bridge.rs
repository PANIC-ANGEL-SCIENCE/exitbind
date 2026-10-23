#![cfg(unix)]
#[allow(dead_code)]
mod support;

use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

mod matrix {
    include!("compatibility_matrix.rs");
}

const TARGET: &str = "x86_64-unknown-linux-gnu";

#[test]
fn stable_installer_refuses_legacy_command_before_fetch() {
    let root = support::temp("stable-legacy-refusal");
    let output = support::git_topology::command("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
        .env("HOME", &root)
        .env("SOULMATE_VERSION", "v0.25.0")
        .env("SOULMATE_INSTALL_PREFIX", root.join("bin"))
        .env_remove("EXITBIND_VERSION")
        .env_remove("EXITBIND_REPOSITORY")
        .env_remove("EXITBIND_INSTALL_PREFIX")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Soulmate distribution ended"),
        "{output:?}"
    );
    assert!(!root.join("bin").exists());
    fs::remove_dir_all(root).unwrap();
}

struct Fixture {
    root: support::TempRoot,
    archive: PathBuf,
    checksum: PathBuf,
    payload: Vec<u8>,
    calls: PathBuf,
}

impl Fixture {
    fn new_with_asset(label: &str, asset_surface: &str, payload: &[u8]) -> Self {
        let root = support::temp(&format!("legacy-installer-{label}"));
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
        assert!(support::git_topology::command("tar")
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
            support::git_topology::command("sha256sum")
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

    fn run_matrix_case(&self, prefix: &Path, case_id: &str) -> std::process::Output {
        let case = matrix::case(case_id);
        let route = matrix::route(case["routeId"].as_str().unwrap());
        let repository = matrix::row("repositories", route["repositoryId"].as_str().unwrap());
        let fetch_repository =
            matrix::row("repositories", route["fetchRepositoryId"].as_str().unwrap());
        let version = case["version"].as_str().unwrap();
        let asset = route["assetPrefix"].as_str().unwrap();
        let fetch_value = fetch_repository["value"].as_str().unwrap();
        let archive_url = format!(
            "https://github.com/{fetch_value}/releases/download/{version}/{asset}-{TARGET}.tar.gz"
        );
        let checksum_url = format!(
            "https://github.com/{fetch_value}/releases/download/{version}/{asset}-{TARGET}.tar.gz.sha256"
        );
        let source_value = repository["value"].as_str().unwrap();
        let mut command = support::git_topology::command("sh");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .env("PATH", self.path())
            .env("HOME", self.root.join("home"))
            .env("BRIDGE_CALLS", &self.calls)
            .env("BRIDGE_ARCHIVE_URL", &archive_url)
            .env("BRIDGE_CHECKSUM_URL", &checksum_url)
            .env("BRIDGE_ARCHIVE_SOURCE", &self.archive)
            .env("BRIDGE_CHECKSUM_SOURCE", &self.checksum)
            .env_remove("EXITBIND_REPOSITORY")
            .env_remove("EXITBIND_VERSION")
            .env_remove("EXITBIND_INSTALL_PREFIX")
            .env_remove("SOULMATE_REPOSITORY")
            .env_remove("SOULMATE_VERSION")
            .env_remove("SOULMATE_INSTALL_PREFIX");
        match case_id {
            "installer-partial-exitbind-custom-legacy" => {
                command
                    .env("EXITBIND_VERSION", version)
                    .env("SOULMATE_REPOSITORY", source_value)
                    .env("SOULMATE_VERSION", "v0.16.0")
                    .env("SOULMATE_INSTALL_PREFIX", prefix);
            }
            "installer-conflicting-namespaces" => {
                command
                    .env("EXITBIND_REPOSITORY", source_value)
                    .env("EXITBIND_VERSION", version)
                    .env("EXITBIND_INSTALL_PREFIX", prefix)
                    .env("SOULMATE_REPOSITORY", "envy404/soulmate")
                    .env("SOULMATE_VERSION", "v0.16.0")
                    .env("SOULMATE_INSTALL_PREFIX", self.root.join("legacy-prefix"));
            }
            _ if route["callerSurfaceId"] == "legacy-soulmate" => {
                command
                    .env("SOULMATE_REPOSITORY", source_value)
                    .env("SOULMATE_VERSION", version)
                    .env("SOULMATE_INSTALL_PREFIX", prefix);
            }
            _ => {
                command
                    .env("EXITBIND_REPOSITORY", source_value)
                    .env("EXITBIND_VERSION", version)
                    .env("EXITBIND_INSTALL_PREFIX", prefix);
            }
        }
        command.output().unwrap()
    }
}

fn executable(path: &Path, contents: impl AsRef<[u8]>) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn installer_matrix_cases_drive_the_existing_selector() {
    let matrix_value = matrix::matrix();
    let cases = matrix_value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "installer")
        .map(|case| case["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 15);
    let mut executed = BTreeSet::new();
    for case_id in &cases {
        executed.insert(case_id.clone());
        let case = matrix::case(case_id);
        let route = matrix::route(case["routeId"].as_str().unwrap());
        let fixture = Fixture::new_with_asset(
            case_id,
            route["assetPrefix"].as_str().unwrap(),
            format!("#!/bin/sh\nprintf '%s\\n' {case_id}\n").as_bytes(),
        );
        let prefix = fixture.root.join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let output = fixture.run_matrix_case(&prefix, case_id);
        assert!(output.status.success(), "{case_id}: {output:?}");
        let expected = route["installedCommands"].as_array().unwrap();
        for command in ["exitbind", "soulmate"] {
            let path = prefix.join(command);
            if expected.iter().any(|name| name.as_str() == Some(command)) {
                assert!(path.is_file(), "{case_id}: missing {command}");
                assert_eq!(fs::read(&path).unwrap(), fixture.payload);
                assert_ne!(fs::metadata(&path).unwrap().permissions().mode() & 0o111, 0);
            } else {
                assert!(!path.exists(), "{case_id}: unexpected {command}");
            }
        }
        let fetch_repository =
            matrix::row("repositories", route["fetchRepositoryId"].as_str().unwrap());
        let fetch_value = fetch_repository["value"].as_str().unwrap();
        let version = case["version"].as_str().unwrap();
        let asset = route["assetPrefix"].as_str().unwrap();
        assert_eq!(
            fs::read_to_string(&fixture.calls).unwrap(),
            format!(
                "https://github.com/{fetch_value}/releases/download/{version}/{asset}-{TARGET}.tar.gz\nhttps://github.com/{fetch_value}/releases/download/{version}/{asset}-{TARGET}.tar.gz.sha256\n"
            ),
            "{case_id}: fetch routing"
        );
    }
    assert_eq!(executed.len(), cases.len());
}
