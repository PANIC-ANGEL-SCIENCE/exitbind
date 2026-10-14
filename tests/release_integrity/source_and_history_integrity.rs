use super::*;

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
            "cde4c003ad885b99ec8e405c8bc80e89935bb050",
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
