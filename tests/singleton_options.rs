//! Repeated value options must fail in the ordinary CLI before any action.
mod support;

use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn duplicate_value_options_refuse_before_configuration_or_goal_effects() {
    let root = support::temp("singleton-options");
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap()
    };
    let duplicate_root = invoke(&["init", "--mode", "portable", "--root", ".", "--root=."]);
    assert!(!duplicate_root.status.success());
    assert!(String::from_utf8_lossy(&duplicate_root.stderr).contains("--root"));
    assert!(snapshot(&root).is_empty(), "refused init created files");
    assert!(invoke(&["init", "--mode", "portable", "--root", "."])
        .status
        .success());
    let before = snapshot(&root);
    for key in ["obligation", "config"] {
        for (first, second) in [("same", "same"), ("first", "second")] {
            for syntax in 0..4 {
                let mut args = vec![
                    "goal".to_owned(),
                    "incorporate".to_owned(),
                    "--goal-id".to_owned(),
                    "refused".to_owned(),
                    "--goal".to_owned(),
                    "retain both requirements".to_owned(),
                ];
                for (bit, value) in [(0, first), (1, second)] {
                    if syntax & (1 << bit) == 0 {
                        args.extend([format!("--{key}"), value.to_owned()]);
                    } else {
                        args.push(format!("--{key}={value}"));
                    }
                }
                let out = Command::new(env!("CARGO_BIN_EXE_exitbind"))
                    .current_dir(&root)
                    .args(&args)
                    .output()
                    .unwrap();
                assert!(!out.status.success(), "{args:?}: {out:?}");
                assert!(
                    String::from_utf8_lossy(&out.stderr)
                        .contains(&format!("option '--{key}' may only be supplied once")),
                    "{out:?}"
                );
                assert_eq!(snapshot(&root), before, "refused input changed files");
            }
        }
    }
    assert!(invoke(&["check", "--config", "exitbind.json"])
        .status
        .success());
    assert!(
        invoke(&["check", "--json", "--json", "--config", "exitbind.json"])
            .status
            .success()
    );
    for args in [
        vec!["check", "--unknown-option"],
        vec!["check", "--config"],
        vec!["check", "--json=true"],
        vec!["check", "--", "--config=x", "--config=x"],
    ] {
        assert!(!invoke(&args).status.success());
        assert_eq!(snapshot(&root), before);
    }
    fs::write(root.join("requirements.md"), "Retain alpha\nRetain beta\n").unwrap();
    assert!(invoke(&[
        "goal",
        "incorporate",
        "--goal-id",
        "distinct",
        "--goal",
        "Retain both",
    ])
    .status
    .success());
    for (id, obligation) in [("alpha", "Retain alpha"), ("beta", "Retain beta")] {
        assert!(invoke(&[
            "goal",
            "require",
            "--goal-id",
            "distinct",
            "--requirement",
            id,
            "--obligation",
            obligation,
            "--artifact",
            "requirements.md",
        ])
        .status
        .success());
    }
    let status = invoke(&["goal", "status", "--json"]);
    assert!(status.status.success());
    let text = String::from_utf8(status.stdout).unwrap();
    assert!(text.contains("Retain alpha") && text.contains("Retain beta"));
    fs::remove_dir_all(root).unwrap();
}
