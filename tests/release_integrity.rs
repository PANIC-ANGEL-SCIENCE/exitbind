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
    "systems.soulmate/.codex-plugin/plugin.json",
    "systems.soulmate/.claude-plugin/plugin.json",
    "scripts/ci-wsl.sh",
];
const SCAN_DIRS: &[&str] = &["docs", "examples", "schema", "scripts", "src"];

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
    let mut command = Command::new("sh");
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

#[test]
fn current_release_and_historical_changelog_pass_with_installer_overrides() {
    let fixture = Fixture::release();
    let path = fixture.0.join("CHANGELOG.md");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("\n## 0.1.0\n\nHistorical references such as v0.1.0 remain historical.\n");
    fs::write(path, text).unwrap();
    expect_success(gate("check-release-refs.sh", &fixture.0));
}

#[test]
fn closed_maintenance_packet_preserves_only_its_historical_release() {
    let fixture = Fixture::release();
    let directory = fixture.0.join("docs/maintenance/v0.25.1");
    fs::create_dir_all(&directory).unwrap();
    let report = directory.join("README.md");
    let history = "# v0.25.1 follow-up\n- branch: `fix/r13-v0.25.1-hardening`;\n";
    fs::write(&report, history).unwrap();
    expect_success(gate("check-release-refs.sh", &fixture.0));
    assert_eq!(fs::read_to_string(&report).unwrap(), history);

    fs::write(
        &report,
        format!("{history}\nstale v0.10.0 install reference\n"),
    )
    .unwrap();
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "unrelated stale version in a historical packet",
    );

    fs::write(&report, history).unwrap();
    fs::write(directory.join("active.md"), "stale v0.25.1 reference\n").unwrap();
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "unselected file in a historical directory",
    );
}

#[test]
fn stale_security_release_references_are_rejected() {
    let fixture = Fixture::release();
    fixture.replace("SECURITY.md", &format!("v{VERSION}"), "v0.10.0");
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "stale current security contract",
    );
}

#[test]
fn stale_and_malformed_readme_install_commands_are_rejected() {
    let current = format!("curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v{VERSION}/install.sh | sh");
    for replacement in [
        current.replace(&format!("v{VERSION}"), "v0.10.0"),
        current.replace("install.sh", "install.sh.invalid"),
        format!("# {current}"),
        String::new(),
    ] {
        let fixture = Fixture::release();
        fixture.replace("README.md", &current, &replacement);
        expect_failure(gate("check-release-refs.sh", &fixture.0), &replacement);
    }
}

#[test]
fn html_comments_cannot_hide_the_readme_install_instruction() {
    let command = format!("curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v{VERSION}/install.sh | sh");
    let block = format!("```sh\n{command}\nexport PATH=\"$HOME/.local/bin:$PATH\"\n```");
    for hidden in [
        format!("<!--\n{block}\n-->"),
        format!("<!-- {block} -->"),
        format!("Installation: <!-- hidden\n{block}\n-->"),
        format!("<!-- closed --> <!-- hidden\n{block}\n-->"),
        format!("<!--\n{block}"),
        block.replace(&command, &format!("<!-- {command} -->")),
    ] {
        let fixture = Fixture::release();
        fixture.replace("README.md", &block, &hidden);
        expect_failure(
            gate("check-release-refs.sh", &fixture.0),
            "HTML-commented install command",
        );
    }
    for visible in [
        format!("<!-- release installation -->\n{block}"),
        format!("<!-- first --> <!-- second -->\n{block}"),
        format!("<!-- historical note\nclosed here -->\n{block}"),
    ] {
        let fixture = Fixture::release();
        fixture.replace("README.md", &block, &visible);
        expect_success(gate("check-release-refs.sh", &fixture.0));
    }
}

#[test]
fn complete_reference_tokens_reject_prefix_collisions_and_other_majors() {
    for wrong in [
        "v0.10.0".to_owned(),
        format!("v{VERSION}0"),
        "v1.0.0".to_owned(),
        format!("v{VERSION}-rc.1"),
        format!("v{VERSION}+rebuild"),
    ] {
        let fixture = Fixture::release();
        // Leave the valid README command intact to exercise the recursive token scan.
        fs::write(
            fixture.0.join("docs/reference.md"),
            format!("release {wrong}\n"),
        )
        .unwrap();
        expect_failure(gate("check-release-refs.sh", &fixture.0), &wrong);
    }
}

#[test]
fn release_refs_skip_only_immutable_legacy_artifacts() {
    let fixture = Fixture::release();
    for name in [
        "schema/harness-manifest.schema.json",
        "schema/soulmate.schema.json",
        "schema/run-boundary.schema.json",
    ] {
        fs::write(
            fixture.0.join(name),
            "https://raw.githubusercontent.com/envy404/soulmate/v0.16.0-rc.1/schema/legacy.json\n",
        )
        .unwrap();
    }
    expect_success(gate("check-release-refs.sh", &fixture.0));

    let fixture = Fixture::release();
    fixture.replace(
        "install.sh",
        "if test \"$surface\" = soulmate && test \"$version\" = v0.25.0; then",
        "if test \"$surface\" = soulmate && test \"$version\" = v0.16.0; then",
    );
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "changed legacy installer cutoff",
    );

    let fixture = Fixture::release();
    fixture.replace(
        "install.sh",
        "Soulmate distribution ended at v0.25.0; install Exitbind instead",
        "Soulmate distribution ended at v0.16.0; install Exitbind instead",
    );
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "changed legacy installer notice",
    );

    let fixture = Fixture::release();
    fs::write(
        fixture.0.join("schema/unrelated.schema.json"),
        "stale v0.16.0-rc.1 reference\n",
    )
    .unwrap();
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "unrelated stale schema reference",
    );
}

#[test]
fn missing_release_sources_and_scan_roots_are_rejected() {
    for name in RELEASE_FILES.iter().chain(SCAN_DIRS) {
        let fixture = Fixture::release();
        let path = fixture.0.join(name);
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        expect_failure(gate("check-release-refs.sh", &fixture.0), name);
    }
}

#[test]
fn absent_or_drifted_required_version_values_are_rejected() {
    let package = format!("version = \"{VERSION}\"");
    let lock = format!("name = \"exitbind\"\nversion = \"{VERSION}\"");
    let manifest = format!("\"version\": \"{VERSION}\",");
    let historical_manifest = "\"version\": \"0.25.0-rc.5\",";
    let installer = format!("version=\"${{EXITBIND_VERSION:-v{VERSION}}}\"");
    let wsl = format!("test \"$(exitbind version)\" = \"{VERSION}\"");
    let changelog = format!("## {VERSION}\n");
    for (file, marker) in [
        ("Cargo.toml", package.as_str()),
        ("Cargo.lock", lock.as_str()),
        ("plugin.json", manifest.as_str()),
        (
            "systems.soulmate/.codex-plugin/plugin.json",
            historical_manifest,
        ),
        (
            "systems.soulmate/.claude-plugin/plugin.json",
            historical_manifest,
        ),
        ("install.sh", installer.as_str()),
        ("scripts/ci-wsl.sh", wsl.as_str()),
        ("CHANGELOG.md", changelog.as_str()),
    ] {
        let malformed = if file.starts_with("systems.soulmate/") {
            marker.replace("0.25.0-rc.5", "1.2.3")
        } else {
            marker.replace(VERSION, "1.2.3")
        };
        for replacement in [String::new(), malformed] {
            let fixture = Fixture::release();
            fixture.replace(file, marker, &replacement);
            expect_failure(gate("check-release-refs.sh", &fixture.0), file);
        }
    }
}

#[test]
fn plugin_manifests_keep_portable_openai_and_compatibility_presentation() {
    let first_sentence =
        "Exitbind — No result exits unbound. Bind the evidence to the exact result before exit.";
    let cli_boundary = "For your existing Codex or Claude lead; this skills-only plugin installs guidance, not the separate Exitbind CLI.";
    let presentation = format!("{first_sentence} {cli_boundary}");
    let legacy_sentence =
        "It verifies what you asked an agent to do and what came back, in the same record.";
    let legacy_boundary = "Use it as an opt-in, bounded project guidance surface; ordinary work remains unchanged. Plugin installation provides host guidance and does not install the separate soulmate CLI.";
    let legacy_presentation = format!("{legacy_sentence} {legacy_boundary}");
    let root: Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    let codex: Value = serde_json::from_str(include_str!(
        "../systems.soulmate/.codex-plugin/plugin.json"
    ))
    .unwrap();
    let claude: Value = serde_json::from_str(include_str!(
        "../systems.soulmate/.claude-plugin/plugin.json"
    ))
    .unwrap();

    assert!(root["description"]
        .as_str()
        .unwrap()
        .starts_with(first_sentence));
    assert!(root["description"].as_str().unwrap().contains(cli_boundary));
    for description in [
        codex["description"].as_str().unwrap(),
        claude["description"].as_str().unwrap(),
    ] {
        assert!(description.starts_with(legacy_sentence));
        assert!(description.contains(legacy_boundary));
    }

    assert_eq!(
        root["$schema"].as_str(),
        Some("https://agent-plugins.org/schemas/1.0.0/plugin.schema.json")
    );
    let allowed_root_fields = [
        "$schema",
        "name",
        "version",
        "description",
        "author",
        "homepage",
        "repository",
        "license",
        "keywords",
        "extensions",
    ];
    for field in root.as_object().unwrap().keys() {
        assert!(
            allowed_root_fields.contains(&field.as_str()),
            "portable root manifest has schema-unknown field {field:?}"
        );
    }
    assert!(!root.as_object().unwrap().contains_key("skills"));
    assert!(!root.as_object().unwrap().contains_key("hooks"));
    assert_eq!(
        codex["hooks"].as_str(),
        Some("./systems.soulmate/hooks/hooks.json")
    );
    assert_eq!(
        claude["hooks"].as_str(),
        Some("./systems.soulmate/hooks/hooks.json")
    );
    assert!(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills/soulmate/SKILL.md")
        .is_file());
    assert_eq!(
        root["extensions"]["com.openai"]["interface"]["longDescription"].as_str(),
        Some(presentation.as_str())
    );
    assert_eq!(
        codex["interface"]["longDescription"].as_str(),
        Some(legacy_presentation.as_str())
    );
    assert_eq!(codex["skills"].as_str(), Some("./skills/"));
    assert!(root["extensions"].get("systems.soulmate").is_none());
    // Everything a current plugin consumer is shown is Exitbind only.
    for presented in [
        root["name"].clone(),
        root["description"].clone(),
        root["repository"].clone(),
        root["keywords"].clone(),
        root["extensions"]["com.openai"].clone(),
        root["extensions"]["systems.exitbind"].clone(),
    ] {
        assert!(
            !presented.to_string().to_lowercase().contains("soulmate"),
            "current plugin presentation carries the previous product: {presented}"
        );
    }
}

#[cfg(unix)]
#[test]
fn recursive_scan_errors_are_not_treated_as_clean_results() {
    let fixture = Fixture::release();
    std::os::unix::fs::symlink("missing-reference", fixture.0.join("docs/broken-link")).unwrap();
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "broken scan link",
    );
}

#[cfg(unix)]
#[test]
fn source_scan_does_not_depend_on_recursive_grep_link_or_error_behavior() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::release();
    let bin = fixture.0.join("test-bin");
    fs::create_dir(&bin).unwrap();
    let lookup = Command::new("sh")
        .args(["-c", "command -v grep"])
        .output()
        .unwrap();
    assert!(lookup.status.success());
    let real_grep = String::from_utf8(lookup.stdout).unwrap();
    let shim = bin.join("grep");
    fs::write(
        &shim,
        br#"#!/bin/sh
case "$1" in
  -R*|-r*)
    printf recursive > "$SOULMATE_TEST_GREP_CALLED"
    # Model a recursive scan silently omitting linked sources and their errors.
    exit 0
    ;;
  -InE)
    printf explicit > "$SOULMATE_TEST_GREP_CALLED"
    if test -f "$SOULMATE_TEST_GREP_ERROR"; then exit 2; fi
    ;;
esac
exec "$SOULMATE_TEST_REAL_GREP" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    let old_path = std::env::var_os("PATH").unwrap();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&old_path))).unwrap();
    let called = fixture.0.join("grep-called");
    let read_error = fixture.0.join("grep-error");
    let run = || {
        gate_command("check-release-refs.sh", &fixture.0, &[])
            .env("PATH", &path)
            .env("SOULMATE_TEST_REAL_GREP", real_grep.trim())
            .env("SOULMATE_TEST_GREP_CALLED", &called)
            .env("SOULMATE_TEST_GREP_ERROR", &read_error)
            .output()
            .unwrap()
    };
    // A file with no matching references exercises grep status 1 as success.
    fs::write(fixture.0.join("docs/no-reference"), "ordinary source\n").unwrap();
    expect_success(run());
    assert_eq!(fs::read_to_string(&called).unwrap(), "explicit");
    std::os::unix::fs::symlink("missing-reference", fixture.0.join("docs/broken-link")).unwrap();
    expect_failure(run(), "grep may silently skip broken links");
    fs::remove_file(fixture.0.join("docs/broken-link")).unwrap();

    let reference = fixture.0.join("linked-reference");
    fs::write(&reference, format!("release v{VERSION}\n")).unwrap();
    std::os::unix::fs::symlink("../linked-reference", fixture.0.join("docs/readable-link"))
        .unwrap();
    expect_success(run());
    fs::write(&reference, "release v1.0.0\n").unwrap();
    expect_failure(run(), "grep may silently skip readable source links");

    fs::write(&reference, format!("release v{VERSION}\n")).unwrap();
    fs::write(&read_error, "fail explicit scan\n").unwrap();
    expect_failure(run(), "explicit grep read errors must propagate");
}

#[cfg(unix)]
#[test]
fn readable_source_links_are_scanned_and_git_directories_stay_excluded() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::release();
    let targets = fixture.0.join("source-targets");
    fs::create_dir(&targets).unwrap();
    let reference = targets.join("reference.md");
    fs::write(&reference, format!("release v{VERSION}\n")).unwrap();
    symlink(
        "../source-targets/reference.md",
        fixture.0.join("docs/readable-file"),
    )
    .unwrap();
    symlink(
        "../source-targets",
        fixture.0.join("examples/readable-directory"),
    )
    .unwrap();
    let ignored = fixture.0.join("docs/.git");
    fs::create_dir(&ignored).unwrap();
    symlink("missing-reference", ignored.join("broken-link")).unwrap();
    fs::write(ignored.join("history"), "release v1.0.0\n").unwrap();
    expect_success(gate("check-release-refs.sh", &fixture.0));

    fs::write(reference, "release v1.0.0\n").unwrap();
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "readable linked source still participates in the scan",
    );
}

fn git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
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

#[test]
fn published_checkpoint_and_descendants_pass_but_rewritten_parentage_fails() {
    let fixture = Fixture::new("ancestry");
    let checkout = fixture.0.join("checkout");
    expect_success(git(
        &fixture.0,
        &[
            "clone",
            "--no-hardlinks",
            "--quiet",
            env!("CARGO_MANIFEST_DIR"),
            "checkout",
        ],
    ));
    expect_success(gate("check-public-history.sh", &checkout));
    expect_success(git(
        &checkout,
        &[
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "Public fixture descendant",
        ],
    ));
    expect_success(gate("check-public-history.sh", &checkout));

    expect_success(git(&checkout, &["checkout", "--orphan", "rewritten"]));
    expect_success(git(
        &checkout,
        &["commit", "--quiet", "-m", "Public fixture orphan"],
    ));
    expect_failure(gate("check-public-history.sh", &checkout), "orphan commit");

    // A replacement may invent a parent view, but is not stored commit ancestry.
    expect_success(git(
        &checkout,
        &[
            "replace",
            "--graft",
            "HEAD",
            "7f1d0146694d6b049f60e6bfba968e2a8c3a104d",
        ],
    ));
    expect_failure(
        gate("check-public-history.sh", &checkout),
        "replacement parent view",
    );
}

#[test]
fn absent_checkpoint_and_shallow_git_evidence_are_rejected() {
    let fixture = Fixture::new("missing-history");
    expect_failure(
        gate("check-public-history.sh", &fixture.0),
        "no Git worktree",
    );
    expect_success(git(&fixture.0, &["init", "--quiet"]));
    expect_success(git(
        &fixture.0,
        &[
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "Unrelated public fixture",
        ],
    ));
    expect_failure(
        gate("check-public-history.sh", &fixture.0),
        "missing checkpoint",
    );

    let shallow = Fixture::new("shallow");
    expect_success(git(
        &shallow.0,
        &[
            "clone",
            "--no-local",
            "--depth",
            "1",
            "--quiet",
            env!("CARGO_MANIFEST_DIR"),
            "checkout",
        ],
    ));
    expect_failure(
        gate("check-public-history.sh", &shallow.0.join("checkout")),
        "shallow checkout",
    );
}

#[test]
fn synthetic_merge_cannot_mask_a_rewritten_or_missing_candidate() {
    let fixture = Fixture::new("merge-candidate");
    let checkout = fixture.0.join("checkout");
    expect_success(git(
        &fixture.0,
        &[
            "clone",
            "--no-hardlinks",
            "--quiet",
            env!("CARGO_MANIFEST_DIR"),
            "checkout",
        ],
    ));
    expect_success(git(&checkout, &["branch", "preserved"]));
    expect_success(git(&checkout, &["checkout", "--orphan", "rewritten"]));
    expect_success(git(
        &checkout,
        &["commit", "--quiet", "-m", "Public fixture rewritten head"],
    ));
    let output = git(&checkout, &["rev-parse", "HEAD"]);
    assert!(output.status.success());
    let rewritten = String::from_utf8(output.stdout).unwrap();
    expect_success(git(&checkout, &["checkout", "preserved"]));
    expect_success(git(
        &checkout,
        &[
            "merge",
            "--allow-unrelated-histories",
            "--no-edit",
            "rewritten",
        ],
    ));
    expect_success(gate("check-public-history.sh", &checkout));
    for candidate in [rewritten.trim(), "", "unavailable-candidate"] {
        expect_failure(
            gate_args("check-public-history.sh", &checkout, &[candidate]),
            "merge must not hide missing candidate ancestry",
        );
    }
    // Supplying a valid revision cannot bypass the mandatory native HEAD check.
    expect_success(git(&checkout, &["checkout", "rewritten"]));
    expect_failure(
        gate_args("check-public-history.sh", &checkout, &["preserved"]),
        "native HEAD remains mandatory",
    );
}

#[test]
#[cfg(unix)]
fn stable_publication_requires_a_published_non_draft_release() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new("preview-publication");
    fs::write(
        fixture.0.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"99.98.97\"\n",
    )
    .unwrap();
    let gh = fixture.0.join("gh");
    fs::write(
        &gh,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$CALLS"
case "$1 $2" in
  'release view')
    case "$SCENARIO" in
      missing|read_error) exit 1 ;;
      true_failure) printf 'true\n'; exit 1 ;;
      stable|draft) printf 'false\n' ;;
      malformed) printf 'unexpected\n' ;;
      *) printf 'true\n' ;;
    esac ;;
  'release create'|'release upload') exit 0 ;;
  *) exit 99 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(fixture.0.join("dist")).unwrap();
    fs::write(fixture.0.join("dist/archive"), b"fixture").unwrap();
    let calls = fixture.0.join("calls");
    let path = std::env::join_paths(
        std::iter::once(fixture.0.clone())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    for (tag, scenario, success, create) in [
        ("v99.98.97", "ready", true, false),
        ("v99.98.97", "missing", true, true),
        ("v99.98.97", "stable", false, false),
        ("v99.98.97", "draft", false, false),
        ("v99.98.97", "malformed", false, false),
        ("v99.98.97-rc.1", "ready", false, false),
        ("v99.98.97+build-one", "ready", false, false),
        ("v99.98.97foo", "ready", false, false),
        ("v99.98.970", "ready", false, false),
        ("v99.98.98", "ready", false, false),
    ] {
        fs::write(&calls, b"").unwrap();
        let output = gate_command("publish-release-assets.sh", &fixture.0, &[])
            .env("PATH", &path)
            .env("GITHUB_REF_NAME", tag)
            .env("GITHUB_REPOSITORY", "example/project")
            .env("SCENARIO", scenario)
            .env("CALLS", &calls)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{tag} {scenario}: {output:?}"
        );
        let calls = fs::read_to_string(&calls).unwrap();
        assert_eq!(calls.contains("release create "), create, "{calls}");
        assert_eq!(calls.contains("release upload "), success, "{calls}");
        assert!(!calls.contains("--clobber"));
        for call in calls.lines() {
            assert!(call.contains(tag), "missing exact tag: {call}");
            assert!(
                call.contains("--repo example/project"),
                "wrong repository: {call}"
            );
        }
        if create {
            assert!(calls.contains("--verify-tag"), "{calls}");
            assert!(calls.contains("--generate-notes"), "{calls}");
            assert!(calls.contains("--title Exitbind v99.98.97"), "{calls}");
        }
    }
    fs::write(
        fixture.0.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"99.98.97-rc.1\"\n",
    )
    .unwrap();
    for (scenario, success, create) in [
        ("ready", true, false),
        ("missing", true, true),
        ("stable", false, false),
        ("draft", false, false),
        ("malformed", false, false),
    ] {
        fs::write(&calls, b"").unwrap();
        let output = gate_command("publish-release-assets.sh", &fixture.0, &[])
            .env("PATH", &path)
            .env("GITHUB_REF_NAME", "v99.98.97-rc.1")
            .env("GITHUB_REPOSITORY", "example/project")
            .env("SCENARIO", scenario)
            .env("CALLS", &calls)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), success, "{scenario}: {output:?}");
        let calls = fs::read_to_string(&calls).unwrap();
        assert!(calls.contains(".isPrerelease == true and .isDraft == false"));
        assert_eq!(calls.contains("release create "), create, "{calls}");
        assert_eq!(calls.contains("release upload "), success, "{calls}");
        assert!(!calls.contains("--clobber"));
        if create {
            assert!(calls.contains("--prerelease --latest=false"), "{calls}");
            assert!(calls.contains("--verify-tag"), "{calls}");
        }
    }
    for (version, prerelease) in [
        ("99.98.97+build-one", false),
        ("99.98.97-rc.1+build-one", true),
    ] {
        fs::write(
            fixture.0.join("Cargo.toml"),
            format!("[package]\nname = \"fixture\"\nversion = \"{version}\"\n"),
        )
        .unwrap();
        fs::write(&calls, b"").unwrap();
        let output = gate_command("publish-release-assets.sh", &fixture.0, &[])
            .env("PATH", &path)
            .env("GITHUB_REF_NAME", format!("v{version}"))
            .env("GITHUB_REPOSITORY", "example/project")
            .env("SCENARIO", "missing")
            .env("CALLS", &calls)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let calls = fs::read_to_string(&calls).unwrap();
        assert!(calls.contains(&format!(
            ".isPrerelease == {prerelease} and .isDraft == false"
        )));
        assert_eq!(calls.contains("--prerelease --latest=false"), prerelease);
    }
    for missing in ["GITHUB_REF_NAME", "GITHUB_REPOSITORY"] {
        fs::write(&calls, b"").unwrap();
        let output = gate_command("publish-release-assets.sh", &fixture.0, &[])
            .env("PATH", &path)
            .env("GITHUB_REF_NAME", "v99.98.97-rc.1")
            .env("GITHUB_REPOSITORY", "example/project")
            .env_remove(missing)
            .env("SCENARIO", "ready")
            .env("CALLS", &calls)
            .output()
            .unwrap();
        assert!(!output.status.success(), "missing {missing}: {output:?}");
        assert!(fs::read_to_string(&calls).unwrap().is_empty());
    }
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"),
    )
    .unwrap();
    let provenance = workflow.find("gh attestation verify").unwrap();
    let publish = workflow
        .find("sh scripts/publish-release-assets.sh")
        .unwrap();
    assert!(provenance < publish);
    assert!(!workflow.contains("gh release create"));
    assert!(!workflow.contains("gh release upload"));
}

#[test]
fn release_workflow_requires_all_native_targets_and_wsl_before_publish() {
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"),
    )
    .unwrap();
    for (runner, arch, target) in [
        ("macos-15", "arm64", "aarch64-apple-darwin"),
        ("macos-15-intel", "x86_64", "x86_64-apple-darwin"),
    ] {
        assert!(workflow.contains(&format!("runner: {runner}")));
        assert!(workflow.contains(&format!("host_arch: {arch}")));
        assert!(workflow.contains(&format!("target: {target}")));
        assert!(workflow.contains(concat!("target/$", "{{ matrix.target }}/release/exitbind")));
    }
    assert!(workflow.contains("installer-smoke.sh"));
    assert!(workflow.contains("github.sha"));
    assert!(workflow.contains("needs: [linux, macos, windows-wsl]"));
    assert!(workflow.contains("pattern: release-*"));
    assert!(workflow.contains("merge-multiple: true"));
    assert!(workflow.contains("aarch64-apple-darwin.bundle.json"));
    assert!(workflow.contains("exitbind-aarch64-apple-darwin.tar.gz.sha256"));
    assert!(workflow.contains("exitbind-x86_64-apple-darwin.tar.gz.sha256"));
    assert!(workflow.contains("exitbind-x86_64-unknown-linux-gnu.tar.gz.sha256"));
    assert!(workflow.matches("--deny-self-hosted-runners").count() >= 3);
    assert!(workflow.matches("actions/attest@").count() >= 2);
    let publish = workflow
        .find("run: sh scripts/publish-release-assets.sh")
        .unwrap();
    assert!(workflow[..publish].matches("gh attestation verify").count() >= 3);
    assert!(workflow.contains("retention-days: 1"));
    assert!(!workflow.contains("--clobber"));
    assert!(!workflow.contains("gh release create"));
    assert!(!workflow.contains("gh release upload"));

    let installed = workflow
        .split_once("\n  installed:\n")
        .unwrap_or_else(|| panic!("missing installed post-publication job"))
        .1;
    assert!(installed.starts_with("    needs: publish\n"));
    for (runner, host_os, host_arch, target) in [
        (
            "ubuntu-latest",
            "Linux",
            "x86_64",
            "x86_64-unknown-linux-gnu",
        ),
        ("macos-15", "Darwin", "arm64", "aarch64-apple-darwin"),
        ("macos-15-intel", "Darwin", "x86_64", "x86_64-apple-darwin"),
    ] {
        assert!(installed.contains(&format!("runner: {runner}")));
        assert!(installed.contains(&format!("host_os: {host_os}")));
        assert!(installed.contains(&format!("host_arch: {host_arch}")));
        assert!(installed.contains(&format!("target: {target}")));
    }
    assert!(installed.contains("ref: ${{ github.ref }}"));
    assert!(installed.contains("Download same-workflow expected release artifact"));
    assert!(installed.contains("published-install-smoke.sh"));
    assert!(installed.contains("EXITBIND_VERSION: ${{ github.ref_name }}"));
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
#[test]
fn required_host_fixture_targets_are_non_native() {
    for ((host_os, host_arch), expected_target) in [
        (("Linux", "x86_64"), "aarch64-apple-darwin"),
        (("Darwin", "arm64"), "x86_64-apple-darwin"),
        (("Darwin", "x86_64"), "aarch64-apple-darwin"),
    ] {
        assert_eq!(
            non_native_target_for_host(host_os, host_arch),
            Some(expected_target),
            "fixture mapping for {host_os}/{host_arch}"
        );
    }
}

#[cfg(unix)]
#[test]
fn published_install_smoke_refuses_a_non_native_target_before_network_access() {
    let fixture = Fixture::new("published-install-host-refusal");
    let expected = fixture.0.join("expected");
    fs::write(&expected, "#!/bin/sh\nexit 0\n").unwrap();
    make_executable(&expected);

    let host_os =
        String::from_utf8(Command::new("uname").arg("-s").output().unwrap().stdout).unwrap();
    let host_arch =
        String::from_utf8(Command::new("uname").arg("-m").output().unwrap().stdout).unwrap();
    let target =
        non_native_target_for_host(host_os.trim(), host_arch.trim()).unwrap_or_else(|| {
            panic!(
                "unsupported host fixture for published-install refusal: {}/{}",
                host_os.trim(),
                host_arch.trim()
            )
        });

    let bin = fixture.0.join("bin");
    fs::create_dir(&bin).unwrap();
    let curl = bin.join("curl");
    fs::write(
        &curl,
        "#!/bin/sh\nprintf called >> \"$SOULMATE_CURL_CALLS\"\nexit 99\n",
    )
    .unwrap();
    make_executable(&curl);
    let curl_calls = fixture.0.join("curl-calls");
    let mut path_entries = vec![bin];
    if let Some(path) = std::env::var_os("PATH") {
        path_entries.extend(std::env::split_paths(&path));
    }
    let path = std::env::join_paths(path_entries).unwrap();

    let output = Command::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/published-install-smoke.sh"),
    )
    .args([target, expected.to_str().unwrap()])
    .current_dir(&fixture.0)
    .env("PATH", path)
    .env("SOULMATE_CURL_CALLS", &curl_calls)
    .output()
    .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not match target"));
    assert!(!curl_calls.exists() || fs::read_to_string(curl_calls).unwrap().is_empty());
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
    let output = match Command::new("sha256sum").arg(path).output() {
        Ok(output) if output.status.success() => output,
        _ => Command::new("shasum")
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

    let real_sha256sum = Command::new("sh")
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
        let real_shasum = Command::new("sh")
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
    test "$count" -eq 9
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
        let output = Command::new("tar")
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
    fs::create_dir_all(fixture.0.join("scripts")).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/publish-release-assets.sh"),
        fixture.0.join("scripts/publish-release-assets.sh"),
    )
    .unwrap();
    fixture
}

#[cfg(unix)]
fn run_publisher_steps(workflow: &str, fixture: &Fixture) -> Output {
    let script = [
        "Validate exact release inventory and transferred digests",
        "Verify transferred archive provenance",
        "Stage exactly the published release assets",
        "Publish tag assets",
    ]
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
    let real_sha256sum = Command::new("sh")
        .args(["-c", "command -v sha256sum || true"])
        .output()
        .unwrap();
    let real_sha256sum = String::from_utf8(real_sha256sum.stdout).unwrap();
    let real_shasum = Command::new("sh")
        .args(["-c", "command -v shasum || true"])
        .output()
        .unwrap();
    let real_shasum = String::from_utf8(real_shasum.stdout).unwrap();
    fs::write(&calls, b"").unwrap();
    Command::new("bash")
        .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", &script])
        .current_dir(&fixture.0)
        .env("PATH", path)
        .env("SOULMATE_REAL_SHA256SUM", real_sha256sum.trim())
        .env("SOULMATE_REAL_SHASUM", real_shasum.trim())
        .env("SOULMATE_GH_CALLS", &calls)
        .env("SOULMATE_UPLOAD_MARKER", &marker)
        .env("GITHUB_REPOSITORY", "fixture/project")
        .env("GITHUB_REF_NAME", "v99.0.0")
        .env("GITHUB_REF", "refs/tags/v99.0.0")
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
    let real_sha256sum = Command::new("sh")
        .args(["-c", "command -v sha256sum || true"])
        .output()
        .unwrap();
    let real_sha256sum = String::from_utf8(real_sha256sum.stdout).unwrap();
    let real_shasum = Command::new("sh")
        .args(["-c", "command -v shasum || true"])
        .output()
        .unwrap();
    let real_shasum = String::from_utf8(real_shasum.stdout).unwrap();
    Command::new(fixture.0.join("bin/gh"))
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

#[cfg(unix)]
#[test]
fn publisher_attestation_shim_rejects_changed_archive_and_wrong_provenance() {
    let fixture = publisher_fixture("attestation-negative", None);
    let valid = [
        "attestation",
        "verify",
        "release-evidence/exitbind-x86_64-apple-darwin.tar.gz",
        "--bundle",
        "release-evidence/x86_64-apple-darwin.bundle.json",
        "--repo",
        "fixture/project",
        "--signer-workflow",
        "fixture/project/.github/workflows/release.yml",
        "--source-ref",
        "refs/tags/v99.0.0",
        "--source-digest",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--deny-self-hosted-runners",
    ];
    expect_success(run_attestation_shim(&fixture, &valid));

    let archive = fixture
        .0
        .join("release-evidence/exitbind-x86_64-apple-darwin.tar.gz");
    let original = fs::read(&archive).unwrap();
    let mut altered = original.clone();
    altered.extend_from_slice(b"altered after transfer");
    fs::write(&archive, altered).unwrap();
    expect_failure(
        run_attestation_shim(&fixture, &valid),
        "attestation shim accepted an altered archive",
    );
    fs::write(&archive, original).unwrap();

    let malformed = [
        "attestation",
        "verify",
        "release-evidence/exitbind-x86_64-apple-darwin.tar.gz",
        "--bundle",
        "release-evidence/x86_64-apple-darwin.bundle.json",
        "--repo",
        "wrong/project",
        "--signer-workflow",
        "fixture/project/.github/workflows/release.yml",
        "--source-ref",
        "refs/tags/v99.0.0",
        "--source-digest",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--deny-self-hosted-runners",
    ];
    expect_failure(
        run_attestation_shim(&fixture, &malformed),
        "attestation shim accepted a wrong repository",
    );
}

#[cfg(unix)]
#[test]
fn publisher_rejects_tampered_transferred_raw_bytes_before_upload_for_each_target() {
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"),
    )
    .unwrap();
    let provenance = release_workflow_step(&workflow, "Verify transferred archive provenance");
    assert!(provenance.contains("cmp \"$payload_dir/exitbind-$target\""));

    let valid = publisher_fixture("valid", None);
    let output = run_publisher_steps(&workflow, &valid);
    expect_success(output);
    assert!(valid.0.join("upload-marker").exists());
    assert_eq!(
        fs::read_to_string(valid.0.join("gh-calls"))
            .unwrap()
            .matches("attestation verify")
            .count(),
        3
    );

    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ] {
        let fixture = publisher_fixture(target, Some(target));
        let output = run_publisher_steps(&workflow, &fixture);
        assert!(
            !output.status.success(),
            "unexpected pass: tampered transferred raw binary for {target}: {output:?}"
        );
        assert!(
            !output.stdout.is_empty() || !output.stderr.is_empty(),
            "missing failure detail: tampered transferred raw binary for {target}"
        );
        let calls = fs::read_to_string(fixture.0.join("gh-calls")).unwrap();
        assert_eq!(calls.matches("attestation verify").count(), 3);
        assert!(!fixture.0.join("upload-marker").exists());
        assert!(!fixture.0.join("dist").exists());
    }
}
