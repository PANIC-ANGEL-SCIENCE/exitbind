//! The temp-root guard removes a passing test's directory and keeps and names
//! a failing test's directory, proven on a real failing libtest run, and a
//! passing test that leaves a directory beside its root fails.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::temp_root::{temp, KEPT};

/// Set only by `a_failing_test_keeps_and_names_its_directory` for its child.
const SEED: &str = "EXITBIND_SEEDED_TEMP_ROOT_FAILURE";

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
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "temp_root_guard::seeded_failing_test",
            "--exact",
            "--test-threads=1",
        ])
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
