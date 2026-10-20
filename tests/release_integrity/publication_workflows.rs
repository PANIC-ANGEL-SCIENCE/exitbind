use super::*;

#[test]
#[cfg(unix)]
fn stable_publication_requires_a_matching_release_and_publishes_drafts_last() {
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
core=${3%%+*}
case "$core" in *-*) channel=true; other=false ;; *) channel=false; other=true ;; esac
case "$1 $2" in
  'release view')
    case "$*" in
      *'--json assets'*)
        for file in dist/*; do printf '%s %s\n' "${file#dist/}" "$(wc -c < "$file" | tr -d ' ')"; done
        exit 0 ;;
    esac
    case "$SCENARIO" in
      missing|read_error) exit 1 ;;
      true_failure) printf 'false %s\n' "$channel"; exit 1 ;;
      stable) printf 'false %s\n' "$other" ;;
      draft) printf 'true %s\n' "$channel" ;;
      malformed) printf 'unexpected\n' ;;
      *) printf 'false %s\n' "$channel" ;;
    esac ;;
  'release create'|'release upload'|'release edit') exit 0 ;;
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
        ("v99.98.97", "draft", true, false),
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
        // Only a draft (new or resumed) is published, after its assets are read back.
        let drafted = success && (create || scenario == "draft");
        assert_eq!(calls.contains("release edit "), drafted, "{calls}");
        if create {
            assert!(calls.contains("--verify-tag"), "{calls}");
            assert!(calls.contains("--generate-notes"), "{calls}");
            assert!(calls.contains("--title Exitbind v99.98.97"), "{calls}");
            assert!(calls.contains("--generate-notes --draft\n"), "{calls}");
        }
        if drafted {
            assert!(calls.contains("--json assets"), "{calls}");
            assert!(calls.contains("--draft=false --latest\n"), "{calls}");
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
        ("draft", true, false),
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
        assert!(calls.contains("--json isPrerelease,isDraft"));
        assert_eq!(calls.contains("release create "), create, "{calls}");
        assert_eq!(calls.contains("release upload "), success, "{calls}");
        assert!(!calls.contains("--clobber"));
        if create {
            assert!(
                calls.contains("--draft --prerelease --latest=false"),
                "{calls}"
            );
            assert!(calls.contains("--verify-tag"), "{calls}");
        }
        if success && (create || scenario == "draft") {
            assert!(calls.contains("--draft=false --latest=false"), "{calls}");
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
        assert!(calls.contains("--json isPrerelease,isDraft"));
        assert_eq!(calls.contains("--prerelease --latest=false"), prerelease);
        assert_eq!(calls.contains("--draft=false --latest=false"), prerelease);
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
    let evidence_step = workflow.find("verify-release-evidence.sh").unwrap();
    let evidence = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tooling/repository/release/verify-release-evidence.sh"),
    )
    .unwrap();
    let provenance = evidence.find("gh attestation verify").unwrap();
    assert!(evidence.contains("sha256sum -c \"$archive.sha256\""));
    assert!(evidence
        .contains("test \"$(tar -tzf \"release-evidence/$archive\")\" = \"exitbind-$target\""));
    assert!(evidence.contains("--repo \"$GITHUB_REPOSITORY\""));
    assert!(evidence.contains("--source-ref \"$GITHUB_REF\""));
    assert!(evidence.contains("--source-digest \"$GITHUB_SHA\""));
    assert!(evidence
        .contains("cmp \"$payload_dir/exitbind-$target\" \"release-evidence/exitbind-$target\""));
    assert!(evidence.contains("test ! -L \"$payload_dir/exitbind-$target\""));
    let publish = workflow
        .find("sh tooling/repository/release/publish-release-assets.sh")
        .unwrap();
    assert!(evidence_step < publish);
    assert!(provenance < evidence.find("mkdir dist").unwrap());
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
        assert!(workflow.contains(concat!(
            "$CARGO_TARGET_DIR/$",
            "{{ matrix.target }}/release/exitbind"
        )));
    }
    assert!(workflow.contains("installer-smoke.sh"));
    assert!(workflow.contains("github.sha"));
    assert!(workflow.contains("needs: [channel, linux, macos, windows-wsl]"));
    assert!(workflow.contains("pattern: release-*"));
    assert!(workflow.contains("merge-multiple: true"));
    assert!(workflow.contains("verify-release-evidence.sh"));
    let evidence = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tooling/repository/release/verify-release-evidence.sh"),
    )
    .unwrap();
    assert!(evidence.contains("for target in $RELEASE_TARGETS; do"));
    assert!(evidence.contains("mkdir dist"));
    assert!(evidence.contains("cp \"release-evidence/exitbind-$target.tar.gz.sha256\" dist/"));
    assert!(workflow.contains("RELEASE_TARGETS: ${{ needs.channel.outputs.targets }}"));
    assert!(workflow.contains("needs.channel.outputs.prerelease == 'true' || (needs.macos.result == 'success' && needs.windows-wsl.result == 'success')"));
    assert!(evidence.contains("--deny-self-hosted-runners"));
    assert!(workflow.matches("actions/attest@").count() >= 2);
    assert!(workflow.matches("gh attestation verify").count() >= 2);
    assert_eq!(evidence.matches("gh attestation verify").count(), 1);
    let publish = workflow
        .find("run: sh tooling/repository/release/publish-release-assets.sh")
        .unwrap();
    assert!(workflow[..publish].contains("verify-release-evidence.sh"));
    assert!(
        workflow[..publish]
            .matches("--deny-self-hosted-runners")
            .count()
            >= 2
    );
    assert!(workflow.contains("retention-days: 1"));
    assert!(!workflow.contains("--clobber"));
    assert!(!workflow.contains("gh release create"));
    assert!(!workflow.contains("gh release upload"));

    let installed = workflow
        .split_once("\n  installed:\n")
        .unwrap_or_else(|| panic!("missing installed post-publication job"))
        .1;
    assert!(installed.starts_with("    needs: [channel, publish]\n"));
    assert!(installed.contains("matrix: ${{ fromJSON(needs.channel.outputs.installed) }}"));
    let channel = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tooling/repository/release/release-channel.py"),
    )
    .unwrap();
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
        assert!(channel.contains(&format!("'runner': '{runner}'")));
        assert!(channel.contains(&format!("'host_os': '{host_os}'")));
        assert!(channel.contains(&format!("'host_arch': '{host_arch}'")));
        assert!(channel.contains(target));
    }
    assert!(installed.contains("ref: ${{ github.ref }}"));
    assert!(installed.contains("Download same-workflow expected release artifact"));
    assert!(installed.contains("published-install-smoke.sh"));
    assert!(installed.contains("EXITBIND_VERSION: ${{ github.ref_name }}"));
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

    let host_os = String::from_utf8(
        super::support::git_topology::command("uname")
            .arg("-s")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let host_arch = String::from_utf8(
        super::support::git_topology::command("uname")
            .arg("-m")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
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

    let output = super::support::git_topology::command(
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
    let verify_step = release_workflow_step(&workflow, "Verify transferred release evidence");
    assert_eq!(
        verify_step,
        "./tooling/repository/release/verify-release-evidence.sh"
    );
    let provenance = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tooling/repository/release/verify-release-evidence.sh"),
    )
    .unwrap();
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

#[cfg(unix)]
#[test]
fn linux_prerelease_inventory_and_tamper_checks_require_three_target_assets_and_the_installer() {
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"),
    )
    .unwrap();
    for tampered in [false, true] {
        let fixture = publisher_fixture(
            "linux-prerelease",
            if tampered {
                Some("x86_64-unknown-linux-gnu")
            } else {
                None
            },
        );
        fs::write(
            fixture.0.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"99.0.0-rc.1\"\n",
        )
        .unwrap();
        set_fixture_installer_default(&fixture, "v99.0.0-rc.1");
        for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
            for suffix in ["", ".tar.gz", ".tar.gz.sha256"] {
                fs::remove_file(
                    fixture
                        .0
                        .join("release-evidence")
                        .join(format!("exitbind-{target}{suffix}")),
                )
                .unwrap();
            }
            fs::remove_file(
                fixture
                    .0
                    .join("release-evidence")
                    .join(format!("{target}.bundle.json")),
            )
            .unwrap();
        }
        let output = run_publisher_steps_for_targets(
            &workflow,
            &fixture,
            "x86_64-unknown-linux-gnu",
            "v99.0.0-rc.1",
        );
        if tampered {
            assert!(
                !output.status.success(),
                "tampered Linux bytes were accepted: {output:?}"
            );
            assert!(!fixture.0.join("upload-marker").exists());
        } else {
            expect_success(output);
            assert!(fixture.0.join("upload-marker").exists());
            assert_eq!(fs::read_dir(fixture.0.join("dist")).unwrap().count(), 5);
        }
        assert_eq!(
            fs::read_to_string(fixture.0.join("gh-calls"))
                .unwrap()
                .matches("attestation verify")
                .count(),
            1
        );
    }
}
