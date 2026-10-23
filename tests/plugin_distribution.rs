#[allow(dead_code)]
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tooling/repository/release")
        .join(name)
}

fn run_check(archive: Option<&Path>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = support::git_topology::command("python3");
    command.arg(script("check-plugin-distribution.py"));
    if let Some(archive) = archive {
        command.args(["--archive", archive.to_str().unwrap()]);
    }
    let output = command.current_dir(root).output().unwrap();
    assert!(output.status.success(), "plugin check failed: {output:?}");
}

#[test]
fn isolated_distribution_and_catalogs_pass_the_deterministic_check() {
    run_check(None);
}

#[test]
fn archive_is_reproducible_and_existing_output_is_not_overwritten() {
    let output_dir = support::temp("plugin-archive");
    let first = output_dir.join("one.tar.gz");
    let second = output_dir.join("two.tar.gz");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for output in [&first, &second] {
        let result = support::git_topology::command("sh")
            .arg(script("package-plugin.sh"))
            .arg(output)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(result.status.success(), "package failed: {result:?}");
    }
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    run_check(Some(&first));

    let result = support::git_topology::command("sh")
        .arg(script("package-plugin.sh"))
        .arg(&first)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!result.status.success(), "existing output was overwritten");
    fs::remove_dir_all(output_dir).unwrap();
}
