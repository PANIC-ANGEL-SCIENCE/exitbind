//! The release publishes the tag's own install.sh with its checksum, and the
//! post-publication smoke runs only that verified asset.

use super::*;

#[cfg(unix)]
#[test]
fn publisher_ships_the_tag_installer_with_its_checksum_and_refuses_another_default() {
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"),
    )
    .unwrap();
    let valid = publisher_fixture("installer-asset", None);
    expect_success(run_publisher_steps(&workflow, &valid));
    assert!(valid.0.join("upload-marker").exists());
    assert_eq!(
        fs::read(valid.0.join("dist/install.sh")).unwrap(),
        fs::read(valid.0.join("install.sh")).unwrap()
    );
    let checksum = fs::read_to_string(valid.0.join("dist/install.sh.sha256")).unwrap();
    assert_eq!(
        checksum,
        format!("{}  install.sh\n", host_sha256(&valid.0.join("install.sh")))
    );
    assert_eq!(fs::read_dir(valid.0.join("dist")).unwrap().count(), 11);

    // An installer that would default to another release is never published.
    let stale = publisher_fixture("installer-stale-default", None);
    set_fixture_installer_default(&stale, "v98.0.0");
    let output = run_publisher_steps(&workflow, &stale);
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not the release tag 'v99.0.0'"));
    assert!(!stale.0.join("upload-marker").exists());
    assert!(!stale.0.join("dist").exists());
}

/// The post-publication smoke runs the installer asset the release published,
/// only after its checksum verifies and it equals the tagged install.sh.
#[cfg(unix)]
#[test]
fn published_install_smoke_verifies_the_published_installer_asset_before_running_it() {
    let host = |flag: &str| {
        let output = super::support::git_topology::command("uname")
            .arg(flag)
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let target = match (host("-s").as_str(), host("-m").as_str()) {
        ("Linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("Darwin", "arm64") => "aarch64-apple-darwin",
        ("Darwin", "x86_64") => "x86_64-apple-darwin",
        (os, arch) => panic!("unsupported host for the published installer fixture: {os}/{arch}"),
    };
    let checked_in = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    for (case, served, checksum_of, reaches_install, detail) in [
        ("identical", "checked", "served", true, ""),
        (
            "tampered",
            "tampered",
            "served",
            false,
            "differs from the tagged install.sh",
        ),
        (
            "bad-checksum",
            "checked",
            "other",
            false,
            "does not match its checksum",
        ),
    ] {
        let fixture = Fixture::new(&format!("published-installer-{case}"));
        let expected = fixture.0.join("expected");
        fs::write(&expected, "#!/bin/sh\nprintf '99.0.0\\n'\n").unwrap();
        make_executable(&expected);
        let assets = fixture.0.join("assets");
        fs::create_dir(&assets).unwrap();
        let installer = assets.join("install.sh");
        if served == "tampered" {
            let mut bytes = fs::read(&checked_in).unwrap();
            bytes.extend_from_slice(b"# tampered after tagging\n");
            fs::write(&installer, bytes).unwrap();
        } else {
            fs::copy(&checked_in, &installer).unwrap();
        }
        let digest = if checksum_of == "served" {
            host_sha256(&installer)
        } else {
            "0".repeat(64)
        };
        fs::write(
            assets.join("install.sh.sha256"),
            format!("{digest}  install.sh\n"),
        )
        .unwrap();
        // The fake curl serves only the two installer assets; the archive fetch
        // made by a started installer is recorded and then refused.
        let bin = fixture.0.join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(
            bin.join("curl"),
            "#!/bin/sh\nurl=\nout=\nwhile test \"$#\" -gt 0; do\n  case \"$1\" in\n    -o) out=$2; shift 2 ;;\n    -*) shift ;;\n    *) url=$1; shift ;;\n  esac\ndone\nprintf '%s\\n' \"$url\" >> \"$FIXTURE_CURL_CALLS\"\ncase \"$url\" in\n  */releases/download/v99.0.0/install.sh|*/releases/download/v99.0.0/install.sh.sha256)\n    cp \"$FIXTURE_ASSETS/${url##*/}\" \"$out\" ;;\n  *) exit 22 ;;\nesac\n",
        )
        .unwrap();
        make_executable(&bin.join("curl"));
        let calls = fixture.0.join("curl-calls");
        let home = fixture.0.join("home");
        fs::create_dir(&home).unwrap();
        let mut path_entries = vec![bin];
        path_entries.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let output = super::support::git_topology::command(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/published-install-smoke.sh"),
        )
        .args([target, expected.to_str().unwrap()])
        .current_dir(&fixture.0)
        .env("PATH", std::env::join_paths(path_entries).unwrap())
        .env("HOME", &home)
        .env("EXITBIND_VERSION", "v99.0.0")
        .env("EXITBIND_REPOSITORY", "fixture/project")
        .env("FIXTURE_ASSETS", &assets)
        .env("FIXTURE_CURL_CALLS", &calls)
        .output()
        .unwrap();
        // No case can pass: the fixture serves no binary archive.
        assert!(!output.status.success(), "{case}: {output:?}");
        let calls = fs::read_to_string(&calls).unwrap();
        assert!(
            calls.starts_with(
                "https://github.com/fixture/project/releases/download/v99.0.0/install.sh\n\
                 https://github.com/fixture/project/releases/download/v99.0.0/install.sh.sha256\n"
            ),
            "{case}: {calls}"
        );
        assert_eq!(
            calls.contains(&format!("exitbind-{target}.tar.gz")),
            reaches_install,
            "{case}: {calls}"
        );
        if !reaches_install {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(detail),
                "{case}: {output:?}"
            );
        }
    }
}

/// releases/latest/download/install.sh must never point at a release whose
/// assets are missing: the release stays a draft until every asset is uploaded
/// and read back, and only then is it published (and becomes latest).
#[cfg(unix)]
#[test]
fn publisher_publishes_the_release_only_after_every_asset_is_uploaded_and_read_back() {
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"),
    )
    .unwrap();
    let valid = publisher_fixture("draft-order", None);
    expect_success(run_publisher_steps(&workflow, &valid));
    let calls = fs::read_to_string(valid.0.join("gh-calls")).unwrap();
    let position = |needle: &str| {
        calls
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle}: {calls}"))
    };
    let create = position("release create v99.0.0");
    let upload = position("release upload v99.0.0");
    let readback = position("--json assets");
    let publish = position("release edit v99.0.0");
    assert!(
        create < upload && upload < readback && readback < publish,
        "{calls}"
    );
    assert!(calls[create..upload].contains("--draft"), "{calls}");
    assert!(
        calls[publish..].contains("--draft=false --latest"),
        "{calls}"
    );

    // A failed upload or a read-back that differs from dist leaves only a draft.
    for (label, from, to) in [
        (
            "upload-fails",
            "    : > \"$SOULMATE_UPLOAD_MARKER\"\n    exit 0\n",
            "    exit 1\n",
        ),
        (
            "readback-differs",
            "        exit 0 ;;\n    esac\n    exit 1 ;;",
            "        printf 'unexpected.tar.gz 1\\n'\n        exit 0 ;;\n    esac\n    exit 1 ;;",
        ),
    ] {
        let fixture = publisher_fixture(&format!("draft-{label}"), None);
        let gh = fixture.0.join("bin/gh");
        let text = fs::read_to_string(&gh).unwrap();
        assert!(text.contains(from), "{label}: fake gh shape changed");
        fs::write(&gh, text.replace(from, to)).unwrap();
        let output = run_publisher_steps(&workflow, &fixture);
        assert!(!output.status.success(), "{label}: {output:?}");
        let calls = fs::read_to_string(fixture.0.join("gh-calls")).unwrap();
        assert!(calls.contains("--draft"), "{label}: {calls}");
        assert!(!calls.contains("release edit "), "{label}: {calls}");
    }
}
