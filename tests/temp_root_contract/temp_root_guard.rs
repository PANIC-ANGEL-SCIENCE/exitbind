//! The temp-root guard removes a passing test's directory and keeps and names
//! a failing test's directory, proven on a real failing libtest run, and a
//! passing test that leaves a directory beside its root fails.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::temp_root::{create_root, temp, KEPT, NOT_REMOVED};

/// Set only by `a_failing_test_keeps_and_names_its_directory` for its child.
const SEED: &str = "EXITBIND_SEEDED_TEMP_ROOT_FAILURE";
/// Set only by `a_root_that_cannot_be_removed_is_named` for its child.
const UNREMOVABLE: &str = "EXITBIND_SEEDED_UNREMOVABLE_TEMP_ROOT";

#[test]
fn a_passing_test_leaves_no_directory() {
    let kept: PathBuf = {
        let root = temp("temp-root-passing");
        fs::write(root.join("file"), b"x").unwrap();
        fs::create_dir_all(root.join("nested/deeper")).unwrap();
        root.to_path_buf()
    };
    assert!(!kept.exists(), "{}", kept.display());
}

/// A passing test that derived a directory beside its root (`<root>-x`, the
/// shape of the leaked `.home` and `-native-session-goals` siblings) fails
/// and names it; a second root of the same process is never "beside".
#[test]
fn a_directory_left_beside_the_root_fails_the_passing_test() {
    let outer = temp("temp-root-beside");
    let other = super::temp_root::temp_in(Some(&outer), "neighbour");
    let sibling = {
        let root = super::temp_root::temp_in(Some(&outer), "neighbour");
        let sibling = PathBuf::from(format!("{}-store", root.display()));
        fs::create_dir(&sibling).unwrap();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(root)));
        let message = caught.expect_err("a sibling must fail the test");
        let message = message
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(message.contains(&*sibling.to_string_lossy()), "{message}");
        sibling
    };
    assert!(sibling.is_dir(), "the sibling is kept for inspection");
    assert!(other.is_dir(), "another root of this process is not beside");
}

/// Fails only in the child run that sets `SEED`; otherwise it passes at once.
#[test]
fn seeded_failing_test() {
    if std::env::var_os(SEED).is_none() {
        return;
    }
    let root = temp("seeded-failure");
    fs::write(root.join("evidence"), b"kept").unwrap();
    panic!("seeded failure");
}

#[test]
fn a_failing_test_keeps_and_names_its_directory() {
    let outer = temp("seeded-failure-parent");
    // raw-spawn: this test binary re-runs one of its own tests; no exitbind process or session state.
    let module = module_path!().split_once("::").unwrap().1;
    let output = Command::new(std::env::current_exe().unwrap())
        .arg(format!("{module}::seeded_failing_test"))
        .args(["--exact", "--test-threads=1"])
        .env(SEED, "1")
        .env("TMPDIR", &outer)
        .env("TMP", &outer)
        .env("TEMP", &outer)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{stdout}");
    assert!(stdout.contains("1 failed"), "{stdout}");
    let named = stdout
        .lines()
        .find_map(|line| line.strip_prefix(KEPT))
        .unwrap_or_else(|| panic!("no kept directory named:\n{stdout}"));
    let kept = PathBuf::from(named.trim_end());
    assert_eq!(
        fs::read(kept.join("evidence")).unwrap(),
        b"kept",
        "{stdout}"
    );
    assert!(
        kept.starts_with(fs::canonicalize(&outer).unwrap()),
        "{} is outside {}",
        kept.display(),
        outer.display()
    );
}

/// F9 review D2: a sibling that existed before the root (left by an earlier
/// process that had the same PID and sequence) does not fail a passing test;
/// one derived after the root still does (above).
#[test]
fn a_sibling_older_than_the_root_does_not_fail_the_passing_test() {
    let outer = temp("temp-root-reused");
    let name = "exitbind-reused-1-0";
    let stale = outer.join(format!("{name}-store"));
    fs::create_dir(&stale).unwrap();
    let root = create_root(&outer, name).expect("the name is free");
    fs::write(root.join("file"), b"x").unwrap();
    drop(root);
    assert!(
        stale.is_dir(),
        "the stale sibling is not the test's to remove"
    );
    assert!(!outer.join(name).exists());
    assert!(create_root(&outer, &format!("{name}-store")).is_none());
}

/// Passes, except in the child run that sets `UNREMOVABLE`, where it leaves
/// a read-only directory inside its root and still passes.
#[test]
fn seeded_unremovable_root() {
    if std::env::var_os(UNREMOVABLE).is_none() {
        return;
    }
    let root = temp("unremovable");
    let locked = root.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("file"), b"x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
    }
}

/// F9 review D1: a passing test whose root cannot be removed stays passing
/// and names the root on stderr, past libtest's capture.
#[cfg(unix)]
#[test]
fn a_root_that_cannot_be_removed_is_named() {
    use std::os::unix::fs::PermissionsExt;
    let outer = temp("unremovable-parent");
    // raw-spawn: this test binary re-runs one of its own tests; no exitbind process or session state.
    let module = module_path!().split_once("::").unwrap().1;
    let output = Command::new(std::env::current_exe().unwrap())
        .arg(format!("{module}::seeded_unremovable_root"))
        .args(["--exact", "--test-threads=1"])
        .env(UNREMOVABLE, "1")
        .env("TMPDIR", &outer)
        .env("TMP", &outer)
        .env("TEMP", &outer)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    let left: Vec<PathBuf> = fs::read_dir(&outer)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    for path in &left {
        // Give the test's own root back its rights so `outer` is removed.
        let _ = fs::set_permissions(path.join("locked"), fs::Permissions::from_mode(0o700));
    }
    if left.is_empty() {
        // A privileged user removes read-only directories; nothing to name.
        return;
    }
    assert_eq!(left.len(), 1, "{left:?}");
    let named = stderr
        .lines()
        .find_map(|line| line.strip_prefix(NOT_REMOVED))
        .unwrap_or_else(|| panic!("the unremoved root is not named:\n{stderr}"));
    assert!(
        named.starts_with(&*left[0].to_string_lossy()),
        "{named} is not {}",
        left[0].display()
    );
}
