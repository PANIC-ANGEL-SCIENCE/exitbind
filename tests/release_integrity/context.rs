#[path = "../support/mod.rs"]
mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "README.md",
    "REFERENCE.md",
    "SECURITY.md",
    "CHANGELOG.md",
    "install.sh",
    "plugin.json",
    "plugins/exitbind/plugin.json",
    "plugins/exitbind/.codex-plugin/plugin.json",
    "plugins/exitbind/.claude-plugin/plugin.json",
    "tooling/repository/release/check-plugin-distribution.py",
    "systems.soulmate/.codex-plugin/plugin.json",
    "systems.soulmate/.claude-plugin/plugin.json",
    "tooling/repository/ci/ci-wsl.sh",
    "docs/goal-requirements.md",
    "docs/legacy-compatibility.md",
    "docs/release-version-policy.md",
    "tooling/repository/release/check-installation-pointer.py",
];
const SCAN_DIRS: &[&str] = &["docs", "examples", "schema", "scripts", "src"];
/// The only install command README and REFERENCE may show: GitHub's latest
/// stable release, which never names an unpublished version.
const LATEST_INSTALL: &str =
    "curl -fsSL https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/latest/download/install.sh | sh";

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        Self(support::temp(&format!("release-integrity-{label}")))
    }

    fn release() -> Self {
        let fixture = Self::new("refs");
        for name in RELEASE_FILES {
            let path = fixture.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join(name), path).unwrap();
        }
        for name in SCAN_DIRS {
            fs::create_dir_all(fixture.0.join(name)).unwrap();
        }
        fixture
    }

    fn replace(&self, file: &str, old: &str, new: &str) {
        let path = self.0.join(file);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(old),
            "fixture did not contain {old} in {file}"
        );
        fs::write(path, text.replacen(old, new, 1)).unwrap();
    }

    fn replace_all(&self, file: &str, old: &str, new: &str) {
        let path = self.0.join(file);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(old),
            "fixture did not contain {old} in {file}"
        );
        fs::write(path, text.replace(old, new)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn gate(name: &str, root: &Path) -> Output {
    gate_args(name, root, &[])
}

fn gate_args(name: &str, root: &Path, arguments: &[&str]) -> Output {
    gate_command(name, root, arguments).output().unwrap()
}

fn gate_command(name: &str, root: &Path, arguments: &[&str]) -> Command {
    let mut command = support::git_topology::command("sh");
    command
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join(name),
        )
        .args(arguments)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        // Supported installer overrides must not replace its checked source default.
        .env("SOULMATE_VERSION", "v99.98.97")
        .env("SOULMATE_REPOSITORY", "example/override");
    command
}

fn expect_success(output: Output) {
    assert!(output.status.success(), "{output:?}");
}

fn expect_failure(output: Output, context: &str) {
    assert!(!output.status.success(), "unexpected pass: {context}");
    assert!(
        !output.stderr.is_empty(),
        "missing failure detail: {context}"
    );
}

fn current_changelog_heading() -> &'static str {
    include_str!("../../CHANGELOG.md")
        .lines()
        .find(|line| line.starts_with("## "))
        .unwrap()
}

fn git(root: &Path, args: &[&str]) -> Output {
    support::git_topology::command("git")
        .args([
            "-c",
            "user.name=Soulmate Test",
            "-c",
            "user.email=soulmate-test@users.noreply.github.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .output()
        .unwrap()
}

#[cfg(unix)]
fn non_native_target_for_host(host_os: &str, host_arch: &str) -> Option<&'static str> {
    match (host_os, host_arch) {
        ("Linux", "x86_64") => Some("aarch64-apple-darwin"),
        ("Darwin", "arm64") => Some("x86_64-apple-darwin"),
        ("Darwin", "x86_64") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

#[cfg(unix)]
fn release_workflow_step(workflow: &str, name: &str) -> String {
    let marker = format!("      - name: {name}\n");
    let step = workflow
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing workflow step {name}"))
        .1;
    let run = step
        .lines()
        .find_map(|line| line.strip_prefix("        run: "))
        .unwrap_or_else(|| panic!("missing run command for workflow step {name}"));
    if run != "|" {
        return run.to_owned();
    }
    let mut body = Vec::new();
    let mut in_run = false;
    for line in step.lines() {
        if !in_run {
            if line.starts_with("        run: |") {
                in_run = true;
            }
            continue;
        }
        match line.strip_prefix("          ") {
            Some(line) => body.push(line),
            None => break,
        }
    }
    body.join("\n")
}

#[cfg(unix)]
fn host_sha256(path: &Path) -> String {
    let output = match support::git_topology::command("sha256sum")
        .arg(path)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => support::git_topology::command("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
            .unwrap(),
    };
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn publisher_fixture(label: &str, tampered_target: Option<&str>) -> Fixture {
    let fixture = Fixture::new(&format!("publisher-{label}"));
    fs::write(
        fixture.0.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"99.0.0\"\n",
    )
    .unwrap();
    let evidence = fixture.0.join("release-evidence");
    let payloads = fixture.0.join("payloads");
    let bin = fixture.0.join("bin");
    fs::create_dir_all(&evidence).unwrap();
    fs::create_dir_all(&payloads).unwrap();
    fs::create_dir_all(&bin).unwrap();

    let real_sha256sum = support::git_topology::command("sh")
        .args(["-c", "command -v sha256sum"])
        .output()
        .unwrap();
    let checksum_shim = bin.join("sha256sum");
    if real_sha256sum.status.success() {
        fs::write(
            &checksum_shim,
            b"#!/bin/sh\nexec \"$SOULMATE_REAL_SHA256SUM\" \"$@\"\n",
        )
        .unwrap();
    } else {
        let real_shasum = support::git_topology::command("sh")
            .args(["-c", "command -v shasum"])
            .output()
            .unwrap();
        assert!(real_shasum.status.success(), "shasum is required");
        fs::write(
            &checksum_shim,
            b"#!/bin/sh\nexec \"$SOULMATE_REAL_SHASUM\" -a 256 \"$@\"\n",
        )
        .unwrap();
    }
    make_executable(&checksum_shim);

    fs::write(
        bin.join("gh"),
        br#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$SOULMATE_GH_CALLS"
require_equal() {
  if test "$1" != "$2"; then
    printf '%s\n' "$3" >&2
    exit 1
  fi
}
case "$1 $2" in
  'attestation verify')
    archive=$3
    bundle=
    repository=
    signer_workflow=
    source_ref=
    source_digest=
    deny_self_hosted=no
    while test "$#" -gt 0; do
      case "$1" in
        --bundle) bundle=$2; shift 2 ;;
        --repo) repository=$2; shift 2 ;;
        --signer-workflow) signer_workflow=$2; shift 2 ;;
        --source-ref) source_ref=$2; shift 2 ;;
        --source-digest) source_digest=$2; shift 2 ;;
        --deny-self-hosted-runners) deny_self_hosted=yes; shift ;;
        *) shift ;;
      esac
    done
    require_equal "$repository" "$GITHUB_REPOSITORY" 'attestation repository mismatch'
    require_equal "$signer_workflow" "$GITHUB_REPOSITORY/.github/workflows/release.yml" 'attestation workflow mismatch'
    require_equal "$source_ref" "$GITHUB_REF" 'attestation source ref mismatch'
    require_equal "$source_digest" "$GITHUB_SHA" 'attestation source digest mismatch'
    require_equal "$deny_self_hosted" yes 'self-hosted runner denial is missing'
    expected=$(sed -n 's/.*"fixture_archive_sha256":"\([0-9a-f]*\)".*/\1/p' "$bundle")
    actual=$(sha256sum "$archive" | cut -d ' ' -f 1)
    require_equal "$actual" "$expected" 'attestation archive digest mismatch'
    exit 0
    ;;
  'release view') exit 1 ;;
  'release create') exit 0 ;;
  'release upload')
    count=0
    for argument do
      case "$argument" in
        dist/*) count=$((count + 1)) ;;
        --clobber) exit 1 ;;
      esac
    done
    test "$count" -eq "$EXITBIND_EXPECTED_ASSET_COUNT"
    : > "$SOULMATE_UPLOAD_MARKER"
    exit 0
    ;;
  *) exit 99 ;;
esac
"#,
    )
    .unwrap();
    make_executable(&bin.join("gh"));

    let targets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ];
    for target in targets {
        let stem = format!("exitbind-{target}");
        let payload_dir = payloads.join(target);
        fs::create_dir_all(&payload_dir).unwrap();
        let payload = payload_dir.join(&stem);
        fs::write(&payload, format!("verified fixture payload {target}\n")).unwrap();
        make_executable(&payload);
        let archive = evidence.join(format!("{stem}.tar.gz"));
        let output = support::git_topology::command("tar")
            .args(["-czf"])
            .arg(&archive)
            .args(["-C"])
            .arg(&payload_dir)
            .arg(&stem)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let digest = host_sha256(&archive);
        fs::write(
            evidence.join(format!("{stem}.tar.gz.sha256")),
            format!("{digest}  {stem}.tar.gz\n"),
        )
        .unwrap();
        fs::write(
            evidence.join(format!("{target}.bundle.json")),
            format!("{{\"fixture_archive_sha256\":\"{digest}\"}}\n"),
        )
        .unwrap();
        let transferred = evidence.join(&stem);
        fs::copy(&payload, &transferred).unwrap();
        make_executable(&transferred);
        if tampered_target == Some(target) {
            fs::write(&transferred, b"unverified transferred raw binary\n").unwrap();
        }
    }
    set_fixture_installer_default(&fixture, "v99.0.0");
    fs::create_dir_all(fixture.0.join("tooling/repository/release")).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tooling/repository/release/publish-release-assets.sh"),
        fixture
            .0
            .join("tooling/repository/release/publish-release-assets.sh"),
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tooling/repository/release/verify-release-evidence.sh"),
        fixture
            .0
            .join("tooling/repository/release/verify-release-evidence.sh"),
    )
    .unwrap();
    fixture
}

/// The publish job ships the tag's own install.sh; the fixture carries one whose
/// default names `tag`, as the release commit's installer does.
#[cfg(unix)]
fn set_fixture_installer_default(fixture: &Fixture, tag: &str) {
    fs::write(
        fixture.0.join("install.sh"),
        format!("#!/bin/sh\nversion=\"${{EXITBIND_VERSION:-{tag}}}\"\n"),
    )
    .unwrap();
}

#[cfg(unix)]
fn run_publisher_steps(workflow: &str, fixture: &Fixture) -> Output {
    run_publisher_steps_for_targets(
        workflow,
        fixture,
        "x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin",
        "v99.0.0",
    )
}

#[cfg(unix)]
fn run_publisher_steps_for_targets(
    workflow: &str,
    fixture: &Fixture,
    targets: &str,
    tag: &str,
) -> Output {
    let script = ["Verify transferred release evidence", "Publish tag assets"]
        .into_iter()
        .map(|name| release_workflow_step(workflow, name))
        .collect::<Vec<_>>()
        .join("\n");
    let old_path = std::env::var_os("PATH").unwrap();
    let path = std::env::join_paths(
        std::iter::once(fixture.0.join("bin")).chain(std::env::split_paths(&old_path)),
    )
    .unwrap();
    let calls = fixture.0.join("gh-calls");
    let marker = fixture.0.join("upload-marker");
    let real_sha256sum = support::git_topology::command("sh")
        .args(["-c", "command -v sha256sum || true"])
        .output()
        .unwrap();
    let real_sha256sum = String::from_utf8(real_sha256sum.stdout).unwrap();
    let real_shasum = support::git_topology::command("sh")
        .args(["-c", "command -v shasum || true"])
        .output()
        .unwrap();
    let real_shasum = String::from_utf8(real_shasum.stdout).unwrap();
    fs::write(&calls, b"").unwrap();
    support::git_topology::command("bash")
        .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", &script])
        .current_dir(&fixture.0)
        .env("PATH", path)
        .env("SOULMATE_REAL_SHA256SUM", real_sha256sum.trim())
        .env("SOULMATE_REAL_SHASUM", real_shasum.trim())
        .env("SOULMATE_GH_CALLS", &calls)
        .env("SOULMATE_UPLOAD_MARKER", &marker)
        .env("GITHUB_REPOSITORY", "fixture/project")
        .env("RELEASE_TARGETS", targets)
        .env(
            "EXITBIND_EXPECTED_ASSET_COUNT",
            // Three files per target, plus install.sh and install.sh.sha256.
            (targets.split_whitespace().count() * 3 + 2).to_string(),
        )
        .env("GITHUB_REF_NAME", tag)
        .env("GITHUB_REF", format!("refs/tags/{tag}"))
        .env("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .env("GH_TOKEN", "fixture-token")
        .output()
        .unwrap()
}

#[cfg(unix)]
fn run_attestation_shim(fixture: &Fixture, arguments: &[&str]) -> Output {
    let old_path = std::env::var_os("PATH").unwrap();
    let path = std::env::join_paths(
        std::iter::once(fixture.0.join("bin")).chain(std::env::split_paths(&old_path)),
    )
    .unwrap();
    let real_sha256sum = support::git_topology::command("sh")
        .args(["-c", "command -v sha256sum || true"])
        .output()
        .unwrap();
    let real_sha256sum = String::from_utf8(real_sha256sum.stdout).unwrap();
    let real_shasum = support::git_topology::command("sh")
        .args(["-c", "command -v shasum || true"])
        .output()
        .unwrap();
    let real_shasum = String::from_utf8(real_shasum.stdout).unwrap();
    support::git_topology::command(fixture.0.join("bin/gh"))
        .args(arguments)
        .current_dir(&fixture.0)
        .env("PATH", path)
        .env("SOULMATE_REAL_SHA256SUM", real_sha256sum.trim())
        .env("SOULMATE_REAL_SHASUM", real_shasum.trim())
        .env("SOULMATE_GH_CALLS", fixture.0.join("gh-calls"))
        .env("SOULMATE_UPLOAD_MARKER", fixture.0.join("upload-marker"))
        .env("GITHUB_REPOSITORY", "fixture/project")
        .env("GITHUB_REF", "refs/tags/v99.0.0")
        .env("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .output()
        .unwrap()
}
