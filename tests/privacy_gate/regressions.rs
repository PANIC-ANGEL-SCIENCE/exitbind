//! Regressions exercise the actual privacy gate with disposable synthetic inputs.

use std::ffi::OsString;
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use super::scan_patch;
use super::{python_context, scan, scan_with_options, Findings, ALLOWED_EMAIL, ALLOWED_NAME};

fn key() -> String {
    ["to", "ken"].concat()
}

fn group(number: &str) -> String {
    format!("{} = {}.group({number})", key(), "synthetic_receiver")
}

fn value() -> String {
    ["synthetic", "_fixture", "_only"].concat()
}

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = super::support::git_topology::git(root)
        .env("GIT_AUTHOR_NAME", ALLOWED_NAME)
        .env("GIT_AUTHOR_EMAIL", ALLOWED_EMAIL)
        .env("GIT_COMMITTER_NAME", ALLOWED_NAME)
        .env("GIT_COMMITTER_EMAIL", ALLOWED_EMAIL)
        .args(args)
        .output()
        .expect("fixture Git is available");
    assert!(output.status.success(), "fixture Git operation failed");
    output.stdout
}

fn commit(root: &Path) {
    git(root, &["add", "--all"]);
    git(root, &["commit", "-q", "-m", "synthetic fixture"]);
}

fn flagged(findings: &Findings, source: &str) -> bool {
    findings.contains(&("credential-assignment".into(), source.into()))
}

fn oid(root: &Path, revision: &str) -> String {
    String::from_utf8(git(root, &["rev-parse", revision]))
        .unwrap()
        .trim()
        .to_owned()
}

#[cfg(unix)]
fn patch_findings(root: &Path, patch: &str, options: python_context::Options) -> Findings {
    let mut context = python_context::Engine::new(root, options);
    let mut findings = Findings::new();
    scan_patch(patch.as_bytes(), &mut findings, &mut context);
    findings
}

#[cfg(unix)]
fn patch(old: &str, new: &str, old_line: &str, new_line: &str) -> String {
    format!(
        "diff --git a/example.py b/example.py\nindex {old}..{new} 100644\n--- a/example.py\n+++ b/example.py\n@@ -1 +1 @@\n-{old_line}\n+{new_line}\n"
    )
}

#[cfg(unix)]
#[test]
fn privacy_python_code_is_recognized_in_candidate_tracked_and_both_history_sides() {
    let root = super::temp("privacy-python-history");
    let old = group("1");
    let new = group("2");
    assert!(super::credential_assignment(&old));
    let boundary_receiver = format!("{} = marker.group(1)\n", key());
    fs::write(root.join("example.py"), boundary_receiver).unwrap();
    assert!(scan(&root).is_empty());
    fs::write(root.join("example.py"), format!("{old}\n")).unwrap();
    assert!(scan(&root).is_empty());

    git(&root, &["init", "-q"]);
    commit(&root);
    fs::write(root.join("example.py"), format!("{new}\n")).unwrap();
    commit(&root);
    assert!(scan(&root).is_empty());

    let old_oid = oid(&root, "HEAD^:example.py");
    let new_oid = oid(&root, "HEAD:example.py");
    let patch = patch(&old_oid, &new_oid, &old, &new);
    let mut context = python_context::Engine::new(&root, python_context::Options::default());
    let offsets = context.patch_offsets(&patch);
    assert_eq!(offsets.len(), 2, "both distinct blob sides must be proved");
    assert!(patch_findings(&root, &patch, python_context::Options::default()).is_empty());
}

#[cfg(unix)]
#[test]
fn privacy_python_context_is_occurrence_bound_and_preserves_other_findings() {
    let root = super::temp("privacy-python-occurrence");
    let line = group("1");
    let mixed = format!("{line}\nfixture = \"\"\"\n{line}\n\"\"\"\n");
    fs::write(root.join("example.py"), &mixed).unwrap();
    let mut context = python_context::Engine::new(&root, python_context::Options::default());
    let offsets = context.direct_offsets(&mixed);
    assert_eq!(offsets.len(), 1);
    assert!(flagged(&scan(&root), "candidate-file"));

    let other = format!("{line}\n{} = \"{}\"\n", key(), value());
    fs::write(root.join("example.py"), other).unwrap();
    assert!(flagged(&scan(&root), "candidate-file"));

    git(&root, &["init", "-q"]);
    fs::write(root.join("example.py"), &mixed).unwrap();
    commit(&root);
    fs::write(root.join("example.py"), "clean = True\n").unwrap();
    commit(&root);
    assert!(flagged(&scan(&root), "git-history:patches"));

    let mixed_oid = oid(&root, "HEAD^:example.py");
    let bad = format!(
        "diff --git a/example.py b/example.py\nindex {mixed_oid}..{mixed_oid} 100644\n--- a/example.py\n+++ b/example.py\n@@ -3 +3 @@\n-{line}\n+{line}\n"
    );
    assert!(flagged(
        &patch_findings(&root, &bad, python_context::Options::default()),
        "git-history:patches"
    ));
}

#[test]
fn privacy_python_shape_does_not_hide_config_literals_or_other_expressions() {
    let root = super::temp("privacy-python-negatives");
    let line = group("1");
    let name = key();
    let literal = value();
    let cases = [
        (
            "config.txt",
            format!("{name}={}.group(1)\n", "synthetic_receiver"),
        ),
        ("config.txt", format!("{line}\n")),
        ("example.py", format!("{name} = \"{literal}\"\n")),
        ("example.py", format!("# {name} = {literal}\n")),
        (
            "example.py",
            format!("{name} = \"\"\"{literal}\ncontinued\n\"\"\"\n"),
        ),
        (
            "example.py",
            format!("{name} = os.environ.get(\"fixture\")\n"),
        ),
        ("example.py", format!("{line} # {name} = {literal}\n")),
        ("example.py", format!("{line}; {name} = \"{literal}\"\n")),
        ("example.py", format!("é = 1; {line}\n")),
        ("example.py", format!("{}\n", group("10"))),
        ("example.py", format!("{}\n", group("123456789012"))),
        (
            "example.py",
            format!("{}\n", group(&format!("\"{literal}\""))),
        ),
        (
            "example.py",
            format!("{name} = synthetic_receiver.group(\n  1\n)\n"),
        ),
    ];
    for (index, (file, contents)) in cases.into_iter().enumerate() {
        let folder = root.join(index.to_string());
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join(file), contents).unwrap();
        assert!(flagged(&scan(&folder), "candidate-file"), "case {index}");
    }
}

#[test]
fn privacy_deleted_python_string_and_ini_history_remain_scanned() {
    let line = group("1");
    let cases = [
        (
            "example.py",
            format!("fixture = \"\"\"\n{line}\n\"\"\"\n"),
            "clean = True\n",
        ),
        ("settings.ini", format!("{line}\n"), "clean\n"),
    ];
    for (path, contents, clean) in cases {
        let root = super::temp("privacy-integration-deleted");
        git(&root, &["init", "-q"]);
        fs::write(root.join(path), contents).unwrap();
        commit(&root);
        fs::write(root.join(path), clean).unwrap();
        commit(&root);
        assert!(flagged(&scan(&root), "git-history:patches"));
    }
}

#[test]
fn privacy_deleted_and_other_branch_history_remain_scanned() {
    let root = super::temp("privacy-python-branches");
    fs::write(root.join("example.txt"), "clean\n").unwrap();
    git(&root, &["init", "-q"]);
    commit(&root);
    let original = oid(&root, "HEAD");
    git(&root, &["checkout", "-q", "-b", "fixture-branch"]);
    fs::write(
        root.join("example.txt"),
        format!("{} = {}\n", key(), value()),
    )
    .unwrap();
    commit(&root);
    fs::write(root.join("example.txt"), "clean\n").unwrap();
    commit(&root);
    git(&root, &["checkout", "-q", "--detach", &original]);
    assert!(flagged(&scan(&root), "git-history:patches"));
}

#[cfg(unix)]
#[test]
fn privacy_historical_context_refuses_ambiguous_invalid_or_mismatched_inputs() {
    let root = super::temp("privacy-python-invalid-patch");
    git(&root, &["init", "-q"]);
    let old_line = group("1");
    let new_line = group("2");
    fs::write(root.join("example.py"), format!("{old_line}\n")).unwrap();
    commit(&root);
    fs::write(root.join("example.py"), format!("{new_line}\n")).unwrap();
    commit(&root);
    let old = oid(&root, "HEAD^:example.py");
    let new = oid(&root, "HEAD:example.py");
    let good = patch(&old, &new, &old_line, &new_line);
    assert!(patch_findings(&root, &good, python_context::Options::default()).is_empty());
    let missing = "f".repeat(old.len());
    let controls = [
        patch(&old[..7], &new[..7], &old_line, &new_line),
        patch(&missing, &new, &old_line, &new_line),
        patch(&old, &new, &new_line, &old_line),
        good.replace("@@ -1 +1 @@", "@@ -2 +2 @@"),
        good.replace("@@ -1 +1 @@", "@@ -1,2 +1 @@"),
        format!("{good}+unexpected extra hunk line\n"),
        good.replace(
            "a/example.py b/example.py",
            "\"a/example.py\" \"b/example.py\"",
        ),
        good.replace("100644", "120000"),
        good.replace("--- a/example.py", "--- a/other.py"),
        good.replace("b/example.py", "b/other.py"),
        good.replace(
            &format!("-{old_line}\n"),
            &format!("-{old_line}\n\\ No newline at end of file\n"),
        ),
        good.replace("index ", "missing-index "),
    ];
    for (index, control) in controls.into_iter().enumerate() {
        assert!(
            flagged(
                &patch_findings(&root, &control, python_context::Options::default()),
                "git-history:patches"
            ),
            "case {index}"
        );
    }
    let limited = python_context::Options {
        max_objects: 1,
        ..Default::default()
    };
    assert!(flagged(
        &patch_findings(&root, &good, limited),
        "git-history:patches"
    ));
}

#[test]
fn privacy_python_parser_failures_and_limits_retain_findings() {
    let root = super::temp("privacy-python-fail-closed");
    let line = group("1");
    fs::write(root.join("example.py"), format!("{line}\n")).unwrap();
    let missing = python_context::Options {
        python: OsString::from(root.join("absent-python")),
        ..Default::default()
    };
    assert!(flagged(
        &scan_with_options(&root, missing),
        "candidate-file"
    ));

    let failed = python_context::Options {
        python: OsString::from("/bin/false"),
        ..Default::default()
    };
    assert!(flagged(&scan_with_options(&root, failed), "candidate-file"));

    let exhausted = python_context::Options {
        max_children: 0,
        ..Default::default()
    };
    assert!(flagged(
        &scan_with_options(&root, exhausted),
        "candidate-file"
    ));

    fs::write(root.join("example.py"), format!("{line}\nunfinished = (\n")).unwrap();
    assert!(flagged(&scan(&root), "candidate-file"));
    let mut invalid_utf8 = format!("{line}\n").into_bytes();
    invalid_utf8.push(0xff);
    fs::write(root.join("example.py"), invalid_utf8).unwrap();
    assert!(flagged(&scan(&root), "candidate-file"));

    let oversized = format!("{line}\n# {}\n", "x".repeat(512 * 1024));
    fs::write(root.join("example.py"), oversized).unwrap();
    assert!(flagged(&scan(&root), "candidate-file"));
}

#[cfg(unix)]
#[test]
fn privacy_oversized_history_blobs_retain_the_original_finding() {
    let root = super::temp("privacy-python-large-blob");
    git(&root, &["init", "-q"]);
    let old_line = group("1");
    let new_line = group("2");
    let padding = "x".repeat(512 * 1024);
    fs::write(
        root.join("example.py"),
        format!("{old_line}\n# {padding}\n"),
    )
    .unwrap();
    commit(&root);
    fs::write(
        root.join("example.py"),
        format!("{new_line}\n# {padding}\n"),
    )
    .unwrap();
    commit(&root);
    let old = oid(&root, "HEAD^:example.py");
    let new = oid(&root, "HEAD:example.py");
    let synthetic_patch = patch(&old, &new, &old_line, &new_line);
    assert!(flagged(
        &patch_findings(&root, &synthetic_patch, python_context::Options::default()),
        "git-history:patches"
    ));
}

#[cfg(unix)]
#[test]
fn privacy_open_output_from_a_descendant_times_out_without_signalling_it() {
    use std::os::unix::fs::PermissionsExt;
    let root = super::temp("privacy-python-descendant");
    fs::write(root.join("example.py"), format!("{}\n", group("1"))).unwrap();
    let marker = root.join("descendant-finished");
    let escaped = marker.to_str().unwrap().replace('\'', "'\\''");
    let parser = root.join("forking-parser");
    fs::write(
        &parser,
        format!("#!/bin/sh\n(sleep 0.2; printf done > '{escaped}') &\nexit 0\n"),
    )
    .unwrap();
    fs::set_permissions(&parser, fs::Permissions::from_mode(0o700)).unwrap();
    let options = python_context::Options {
        python: parser.into_os_string(),
        child_timeout: Duration::from_millis(30),
        ..Default::default()
    };
    assert!(flagged(
        &scan_with_options(&root, options),
        "candidate-file"
    ));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read(marker).unwrap(), b"done");
}

#[cfg(unix)]
#[test]
fn privacy_python_parser_timeout_reaps_its_owned_child() {
    use std::os::unix::fs::PermissionsExt;
    let root = super::temp("privacy-python-timeout");
    fs::write(root.join("example.py"), format!("{}\n", group("1"))).unwrap();
    let parser = root.join("slow-parser");
    fs::write(&parser, "#!/bin/sh\nexec sleep 3\n").unwrap();
    fs::set_permissions(&parser, fs::Permissions::from_mode(0o700)).unwrap();
    let options = python_context::Options {
        python: parser.into_os_string(),
        child_timeout: Duration::from_millis(30),
        ..Default::default()
    };
    let before = std::time::Instant::now();
    assert!(flagged(
        &scan_with_options(&root, options),
        "candidate-file"
    ));
    assert!(before.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn privacy_python_source_is_parsed_without_being_executed() {
    let root = super::temp("privacy-python-no-execution");
    let marker = root.join("unexpected-effect");
    let code = format!(
        "open({:?}, 'w').write('unexpected')\n{}\n",
        marker.to_str().unwrap(),
        group("1")
    );
    fs::write(root.join("example.py"), code).unwrap();
    assert!(scan(&root).is_empty());
    assert!(!marker.exists());
}

#[cfg(not(unix))]
#[test]
fn privacy_unsupported_platform_retains_the_original_finding() {
    let root = super::temp("privacy-python-unsupported");
    fs::write(root.join("example.py"), format!("{}\n", group("1"))).unwrap();
    assert!(flagged(&scan(&root), "candidate-file"));
}
