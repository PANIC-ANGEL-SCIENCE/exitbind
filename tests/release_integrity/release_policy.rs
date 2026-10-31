use super::*;

#[test]
fn installation_pointer_requires_exact_published_stable_and_complete_assets() {
    const PRODUCER: &str = "0123456789abcdef0123456789abcdef01234567";
    let stable = format!("v{VERSION}");
    let stable = stable.as_str();
    let fixture = Fixture::new("installation-pointer");
    let project_root = fixture.0.join("project");
    fs::create_dir_all(project_root.join("docs")).unwrap();
    let set_source_pointer = |tag: &str| {
        fs::write(
            project_root.join("docs/release-version-policy.md"),
            format!("Source candidate tag: {tag}\n"),
        )
        .unwrap();
        fs::write(
            project_root.join("README.md"),
            format!("{LATEST_INSTALL}\n"),
        )
        .unwrap();
    };
    set_source_pointer(stable);
    let assets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ]
    .into_iter()
    .flat_map(|target| {
        let archive = format!("exitbind-{target}.tar.gz");
        [
            format!("exitbind-{target}"),
            archive.clone(),
            format!("{archive}.sha256"),
        ]
    })
    .chain(["install.sh".to_owned(), "install.sh.sha256".to_owned()])
    .map(|name| {
        serde_json::json!({
            "name":name,
            "digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        })
    })
    .collect::<Vec<_>>();
    let mut metadata = serde_json::json!({
        "tag_name":stable,
        "draft":false,
        "prerelease":false,
        "published_at":"2026-10-14T00:00:00Z",
        "producer_commit":PRODUCER,
        "assets":assets.clone(),
    });
    let metadata_path = fixture.0.join("release.json");
    let check = |metadata: &Value, version: &str, producer: &str| {
        fs::write(&metadata_path, serde_json::to_vec(metadata).unwrap()).unwrap();
        super::support::git_topology::command("python3")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tooling/repository/release/check-installation-pointer.py"),
            )
            .args([
                "--metadata",
                metadata_path.to_str().unwrap(),
                "--root",
                project_root.to_str().unwrap(),
                "--expected-version",
                version,
                "--expected-producer",
                producer,
            ])
            .output()
            .unwrap()
    };
    assert!(check(&metadata, stable, PRODUCER).status.success());

    let candidate = "v99.98.97";
    let mut delivered = metadata.clone();
    delivered["tag_name"] = serde_json::json!(candidate);
    expect_failure(
        check(&delivered, candidate, PRODUCER),
        "post-delivery release with stale published pointer",
    );
    set_source_pointer(candidate);
    assert!(check(&delivered, candidate, PRODUCER).status.success());
    set_source_pointer(stable);
    // A versioned command would lag or lead the release; only the latest form passes.
    fs::write(
        project_root.join("README.md"),
        format!("curl -fsSL https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/{stable}/install.sh | sh\n"),
    )
    .unwrap();
    expect_failure(
        check(&metadata, stable, PRODUCER),
        "versioned README install command",
    );
    set_source_pointer(stable);

    let mut other_tag = metadata.clone();
    other_tag["tag_name"] = serde_json::json!(format!("{stable}-candidate"));
    expect_failure(
        check(&other_tag, stable, PRODUCER),
        "different selected tag",
    );
    expect_failure(
        check(
            &metadata,
            stable,
            "1123456789abcdef0123456789abcdef01234567",
        ),
        "different producer",
    );
    let mut draft = metadata.clone();
    draft["draft"] = serde_json::json!(true);
    expect_failure(check(&draft, stable, PRODUCER), "draft release");
    let mut unpublished = metadata.clone();
    unpublished["published_at"] = serde_json::Value::Null;
    expect_failure(
        check(&unpublished, stable, PRODUCER),
        "unpublished source candidate",
    );

    metadata["prerelease"] = serde_json::json!(true);
    expect_failure(check(&metadata, stable, PRODUCER), "prerelease pointer");
    metadata["prerelease"] = serde_json::json!(false);
    let _ = metadata["assets"].as_array_mut().unwrap().pop();
    expect_failure(check(&metadata, stable, PRODUCER), "missing release asset");
    metadata["assets"] = serde_json::json!(assets.clone());
    metadata["assets"]
        .as_array_mut()
        .unwrap()
        .retain(|asset| asset["name"] != "install.sh");
    expect_failure(
        check(&metadata, stable, PRODUCER),
        "missing published installer asset",
    );
    metadata["assets"] = serde_json::json!(assets.clone());
    metadata["assets"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name":"unselected.tar.gz",
            "digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }));
    expect_failure(
        check(&metadata, stable, PRODUCER),
        "unexpected release asset",
    );
    metadata["assets"] = serde_json::json!(assets.clone());
    metadata["assets"][0]["digest"] = serde_json::json!("unknown");
    expect_failure(
        check(&metadata, stable, PRODUCER),
        "unverified asset digest",
    );

    let missing_metadata = fixture.0.join("missing-release.json");
    let unavailable = super::support::git_topology::command("python3")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tooling/repository/release/check-installation-pointer.py"),
        )
        .args([
            "--metadata",
            missing_metadata.to_str().unwrap(),
            "--root",
            project_root.to_str().unwrap(),
            "--expected-version",
            stable,
            "--expected-producer",
            PRODUCER,
        ])
        .output()
        .unwrap();
    assert_eq!(unavailable.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unavailable.stderr).contains("verification unavailable"));
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
fn release_policy_declares_no_separate_older_published_tag() {
    expect_success(gate("check-release-refs.sh", &Fixture::release().0));
    // The removed lag: a separate published tag beside the source version, even
    // one that happens to equal it, is refused by the policy rule itself.
    for published in ["v0.0.0".to_owned(), format!("v{VERSION}")] {
        let fixture = Fixture::release();
        fixture.replace(
            "docs/release-version-policy.md",
            "Source candidate tag: ",
            &format!("Published stable tag: {published}\nSource candidate tag: "),
        );
        let output = gate("check-release-refs.sh", &fixture.0);
        expect_failure(output.clone(), "separate published stable tag");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("separate, older published stable tag"),
            "{output:?}"
        );
    }
}

#[test]
fn current_guides_and_sources_cannot_name_an_older_release() {
    let current = format!("v{VERSION}");
    for guide in ["REFERENCE.md", "SECURITY.md", "docs/goal-requirements.md"] {
        let fixture = Fixture::release();
        fixture.replace(guide, &current, "v0.0.0");
        expect_failure(gate("check-release-refs.sh", &fixture.0), guide);
    }
    for name in [
        "schema/candidate.json",
        "src/candidate.rs",
        "docs/unlisted.md",
    ] {
        let fixture = Fixture::release();
        fs::write(fixture.0.join(name), "release v0.0.0\n").unwrap();
        expect_failure(gate("check-release-refs.sh", &fixture.0), name);
    }
}

#[test]
fn release_policy_must_bind_the_candidate_and_one_valid_stable_tag() {
    let candidate = format!("Source candidate tag: v{VERSION}");
    for (marker, replacement) in [
        (
            candidate.as_str(),
            "Source candidate tag: v99.98.97".to_owned(),
        ),
        (
            candidate.as_str(),
            "Source candidate tag: v0.31.01".to_owned(),
        ),
        (candidate.as_str(), format!("{candidate}\n{candidate}")),
        (candidate.as_str(), String::new()),
    ] {
        let fixture = Fixture::release();
        fixture.replace("docs/release-version-policy.md", marker, &replacement);
        expect_failure(
            gate("check-release-refs.sh", &fixture.0),
            "missing, duplicate or drifted release policy",
        );
    }
}

#[test]
fn numeric_version_fixture_literals_do_not_allow_other_stale_references() {
    let fixture = Fixture::release();
    let path = fixture
        .0
        .join("src/distribution/update/version_policy_tests.rs");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "v(\"v0.31.0\"); v(\"v0.31.9\"); invalid(\"v0.31.01\");\n",
    )
    .unwrap();
    expect_success(gate("check-release-refs.sh", &fixture.0));
    fs::write(&path, "v(\"v99.98.97\");\n").unwrap();
    expect_failure(
        gate("check-release-refs.sh", &fixture.0),
        "unrelated version in numeric fixture",
    );
}

#[test]
fn packaged_plugin_mirrors_cannot_drift_from_the_release() {
    for file in [
        "plugins/exitbind/plugin.json",
        "plugins/exitbind/.codex-plugin/plugin.json",
        "plugins/exitbind/.claude-plugin/plugin.json",
    ] {
        let fixture = Fixture::release();
        fixture.replace(file, VERSION, "99.98.97");
        expect_failure(gate("check-release-refs.sh", &fixture.0), file);
    }
}

#[test]
fn current_guides_reject_obsolete_development_only_availability() {
    for (file, claim) in [
        (
            "docs/goal-requirements.md",
            format!("This path is available in the\ndevelopment source; it does not change the stable v{VERSION} installation."),
        ),
        (
            "docs/legacy-compatibility.md",
            "This unreleased\nsource line also retires the old executable.".to_owned(),
        ),
    ] {
        let fixture = Fixture::release();
        let path = fixture.0.join(file);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(path, format!("{text}\n{claim}\n")).unwrap();
        expect_failure(gate("check-release-refs.sh", &fixture.0), file);
    }
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
    let versioned = |tag: &str| {
        format!("curl -fsSL https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/{tag}/install.sh | sh")
    };
    for guide in ["README.md", "REFERENCE.md"] {
        for replacement in [
            versioned(&format!("v{VERSION}")),
            versioned("v0.10.0"),
            LATEST_INSTALL.replace("install.sh", "install.sh.invalid"),
            format!("# {LATEST_INSTALL}"),
            String::new(),
        ] {
            let fixture = Fixture::release();
            fixture.replace_all(guide, LATEST_INSTALL, &replacement);
            expect_failure(gate("check-release-refs.sh", &fixture.0), &replacement);
        }
        // A second, versioned command beside the latest one is still rejected.
        let fixture = Fixture::release();
        fixture.replace(
            guide,
            LATEST_INSTALL,
            &format!("{LATEST_INSTALL}\n{}", versioned(&format!("v{VERSION}"))),
        );
        expect_failure(gate("check-release-refs.sh", &fixture.0), guide);
    }
}

#[test]
fn html_comments_cannot_hide_the_readme_install_instruction() {
    let command = LATEST_INSTALL;
    let block = format!("```sh\n{command}\nexport PATH=\"$HOME/.local/bin:$PATH\"\n```");
    for hidden in [
        format!("<!--\n{block}\n-->"),
        format!("<!-- {block} -->"),
        format!("Installation: <!-- hidden\n{block}\n-->"),
        format!("<!-- closed --> <!-- hidden\n{block}\n-->"),
        format!("<!--\n{block}"),
        block.replace(command, &format!("<!-- {command} -->")),
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
    let changelog = format!("{}\n", current_changelog_heading());
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
        ("tooling/repository/ci/ci-wsl.sh", wsl.as_str()),
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

/// FR55-18: the release commit wrote "Unreleased source candidate; no stable
/// tag or release assets are published." and only the next release commit
/// flipped it, so the public CHANGELOG was false after every publish. The
/// newest entry's first line now names its release tag, which stays true.
#[test]
fn the_newest_changelog_entry_names_its_release_tag_and_no_pre_publish_status() {
    let line = release_line(VERSION);
    let entry = format!("{}\n\n{line}\n", current_changelog_heading());
    assert!(
        include_str!("../../CHANGELOG.md").contains(&entry),
        "the current entry starts with: {line}"
    );
    expect_success(gate("check-release-refs.sh", &Fixture::release().0));
    let heading = current_changelog_heading().to_owned();
    assert_eq!(heading, format!("## {VERSION}"));
    let old_sentence =
        "Unreleased source candidate; no stable tag or release assets are published.";
    let old_heading = format!("## Unreleased — {VERSION} source candidate");
    for (label, old, new) in [
        ("missing heading", heading.clone(), String::new()),
        (
            "another version's heading",
            heading.clone(),
            heading.replace(VERSION, "1.2.3"),
        ),
        ("old status sentence", line.clone(), old_sentence.to_owned()),
        (
            "old status sentence below the tag line",
            line.clone(),
            format!("{line}\n\n{old_sentence}"),
        ),
        ("missing tag line", format!("{line}\n"), String::new()),
        ("another version's tag", line.clone(), release_line("1.2.3")),
        ("old heading", heading.clone(), old_heading.clone()),
        (
            "old heading above the current entry",
            heading.clone(),
            format!("{old_heading}\n\n{line}\n\n{heading}"),
        ),
    ] {
        let fixture = Fixture::release();
        fixture.replace("CHANGELOG.md", &old, &new);
        expect_failure(gate("check-release-refs.sh", &fixture.0), label);
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
    let root: Value = serde_json::from_str(include_str!("../../plugin.json")).unwrap();
    let codex: Value = serde_json::from_str(include_str!(
        "../../systems.soulmate/.codex-plugin/plugin.json"
    ))
    .unwrap();
    let claude: Value = serde_json::from_str(include_str!(
        "../../systems.soulmate/.claude-plugin/plugin.json"
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
