//! Fixture identity is a precondition, never inferred from Exitbind's result.
use std::{ffi::OsStr, fs, path::Path, path::PathBuf, process::Command};

/// Owned fixture commands must observe the same Git environment as preflight.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    isolate(&mut command);
    command
}

pub fn git(root: &Path) -> Command {
    let mut command = command("git");
    command.arg("-C").arg(root);
    command
}

pub fn isolate(command: &mut Command) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        command.env_remove(name);
    }
}

pub fn marker(root: &Path) -> Option<PathBuf> {
    root.ancestors().find_map(|ancestor| {
        let marker = ancestor.join(".git");
        match fs::symlink_metadata(&marker) {
            Ok(_) => Some(marker),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("fixture cannot inspect {}: {error}", marker.display()),
        }
    })
}

pub fn worktree(root: &Path) -> Option<PathBuf> {
    let output = git(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("fixture Git dependency");
    output.status.success().then(|| {
        let text = String::from_utf8(output.stdout).expect("UTF-8 fixture Git identity");
        fs::canonicalize(text.trim()).expect("canonical fixture Git identity")
    })
}

pub fn assert_worktree(cwd: &Path, expected: &Path) {
    let expected = fs::canonicalize(expected).expect("canonical expected fixture source");
    assert_eq!(
        worktree(cwd),
        Some(expected.clone()),
        "fixture topology mismatch at {}",
        cwd.display()
    );
    eprintln!(
        "fixture topology: cwd={} canonical Git/source={}",
        cwd.display(),
        expected.display()
    );
}

pub fn repository(root: &Path) {
    assert!(git(root)
        .args(["init", "-q"])
        .status()
        .expect("initialize fixture Git")
        .success());
    assert_worktree(root, root);
}

/// Missing coding hosts do not imply a missing Git dependency. Keep only Git
/// on the fixture PATH so host discovery exercises the claimed subject.
pub fn git_only_path(root: &Path) -> PathBuf {
    let bin = root.join("fixture-git-bin");
    fs::create_dir_all(&bin).expect("fixture Git-only PATH");
    let source = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join("git"))
        .find(|path| path.is_file())
        .expect("fixture Git dependency on PATH");
    let target = bin.join("git");
    if !target.exists() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(fs::canonicalize(source).unwrap(), &target)
            .expect("fixture Git executable");
        #[cfg(not(unix))]
        fs::copy(source, &target).expect("fixture Git executable");
    }
    bin
}

/// A host can explicitly report unavailable no-Git coverage. By default a
/// contaminated temporary namespace fails instead of silently skipping tests.
pub fn marker_free_temp(label: &str) -> Option<PathBuf> {
    let parent = std::env::var_os("EXITBIND_TEST_NO_GIT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let parent = fs::canonicalize(parent).expect("canonical no-Git fixture parent");
    if let Some(marker) = marker(&parent) {
        let detail = format!(
            "marker-free coverage unavailable: {} inherits {}",
            parent.display(),
            marker.display()
        );
        if std::env::var("EXITBIND_TEST_NO_GIT_UNAVAILABLE").as_deref() == Ok("1") {
            eprintln!("COVERAGE UNAVAILABLE: {label}: {detail}; no product behavior was exercised");
            return None;
        }
        panic!("{detail}; provide a genuinely marker-free EXITBIND_TEST_NO_GIT_ROOT, or explicitly report unavailable coverage with EXITBIND_TEST_NO_GIT_UNAVAILABLE=1");
    }
    let root = parent.join(format!(
        "exitbind-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).expect("create marker-free fixture");
    assert!(
        marker(&root).is_none(),
        "fixture topology mismatch: marker-free ancestors required"
    );
    assert!(
        worktree(&root).is_none(),
        "fixture topology mismatch: no Git identity required"
    );
    Some(root)
}
