//! Bootstrap keeps source identity while authority and runtime state are owned
//! outside a read-only source. No source relocation or writable .git is needed.
#![cfg(unix)]
mod support;
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    base: PathBuf,
    source: PathBuf,
    state: PathBuf,
    bindings: PathBuf,
    git_root: Option<PathBuf>,
    invalid_marker: bool,
}
impl Fixture {
    fn no_git() -> Option<Self> {
        Some(Self::from_base(support::git_topology::marker_free_temp(
            "portability",
        )?))
    }
    fn repository() -> Self {
        let mut fixture = Self::from_base(support::temp("portability-git"));
        support::git_topology::repository(&fixture.base);
        fixture.git(&["init", "-q"]);
        fixture.git_root = Some(fixture.source.clone());
        fixture
    }
    fn invalid() -> Self {
        let mut fixture = Self::from_base(support::temp("portability-invalid"));
        support::git_topology::repository(&fixture.base);
        fs::write(
            fixture.source.join(".git"),
            "gitdir: missing-fixture-git-directory\n",
        )
        .unwrap();
        fixture.invalid_marker = true;
        fixture
    }
    fn from_base(base: PathBuf) -> Self {
        let source = base.join("source");
        let state = base.join("user-state");
        let bindings = state.join("bindings");
        for path in [&source, &state] {
            fs::create_dir(path).unwrap();
        }
        fs::create_dir(source.join("nested")).unwrap();
        Self {
            base,
            source,
            state,
            bindings,
            git_root: None,
            invalid_marker: false,
        }
    }
    fn preflight(&self, cwd: &Path) {
        if let Some(root) = &self.git_root {
            support::git_topology::assert_worktree(cwd, root);
        } else if self.invalid_marker {
            assert_eq!(
                support::git_topology::marker(cwd),
                Some(self.source.join(".git"))
            );
            assert!(
                support::git_topology::worktree(cwd).is_none(),
                "invalid fixture unexpectedly resolves Git"
            );
        } else {
            assert!(
                support::git_topology::marker(cwd).is_none(),
                "fixture topology mismatch: inherited .git marker at {}",
                cwd.display()
            );
            assert!(
                support::git_topology::worktree(cwd).is_none(),
                "fixture topology mismatch: unexpected Git identity"
            );
        }
    }
    fn command(&self, cwd: &Path, args: &[&str]) -> Command {
        self.preflight(cwd);
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(cwd)
            .args(args)
            .env("XDG_STATE_HOME", &self.state)
            .env("EXITBIND_BINDINGS_DIR", &self.bindings)
            .env("HOME", &self.base);
        support::git_topology::isolate(&mut command);
        command
    }
    fn call(&self, cwd: &Path, args: &[&str]) -> Output {
        self.command(cwd, args).output().unwrap()
    }
    fn context(&self, cwd: &Path) -> Value {
        let out = self.call(cwd, &["project", "context", "--json"]);
        assert!(out.status.success(), "{out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn git(&self, args: &[&str]) {
        assert!(support::git_topology::git(&self.source)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained portability fixture: {}", self.base.display());
            return;
        }
        for root in [&self.base, &self.state, &self.bindings] {
            if root.exists() {
                fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        fs::set_permissions(&self.source, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(&self.base).unwrap();
    }
}

#[test]
fn portable_home_context_does_not_implicitly_configure_an_unrelated_workspace() {
    let Some(f) = Fixture::no_git() else {
        return;
    };
    let init = f.call(&f.base, &["init", "--mode", "portable", "--skip-skills"]);
    assert!(init.status.success(), "{init:?}");
    let out = f.call(
        &f.source.join("nested"),
        &[
            "work",
            "classify",
            "--material-consequence",
            "true",
            "--promotion-required",
            "false",
            "--json",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["availability"]["configuration"], false);
    assert_eq!(result["assessment"]["activation"], "blocked");
    let explicit = f.call(
        &f.source.join("nested"),
        &[
            "project",
            "context",
            "--config",
            f.base.join("exitbind.json").to_str().unwrap(),
            "--json",
        ],
    );
    assert!(explicit.status.success(), "{explicit:?}");
    assert_eq!(
        f.context(&f.base)["placement"]["sourceRoot"],
        f.base.to_str().unwrap()
    );
}

#[test]
fn readonly_source_bootstraps_without_surgery_and_nested_context_has_one_identity() {
    let Some(f) = Fixture::no_git() else {
        return;
    };
    fs::write(f.source.join("keep.txt"), "source sentinel").unwrap();
    fs::set_permissions(&f.source, fs::Permissions::from_mode(0o555)).unwrap();
    let out = f.call(&f.source, &["init", "--skip-skills"]);
    assert!(out.status.success(), "{out:?}");
    assert!(!f.source.join("exitbind.json").exists());
    let root = f.context(&f.source);
    let nested = f.context(&f.source.join("nested"));
    assert_eq!(
        root["project"]["memoryIdentity"],
        nested["project"]["memoryIdentity"]
    );
    assert_eq!(root["placement"]["sourceRoot"], f.source.to_str().unwrap());
    assert_eq!(root["placement"]["sourceCapability"], "read-only");
    assert_eq!(root["placement"]["stateCapability"], "writable");
    assert_eq!(
        root["placement"]["stateRoot"],
        nested["placement"]["stateRoot"]
    );
    assert_ne!(
        root["placement"]["stateRoot"],
        root["placement"]["sourceRoot"]
    );
    assert_eq!(
        fs::read_to_string(f.source.join("keep.txt")).unwrap(),
        "source sentinel"
    );
    // Context must stay readable when both source and private binding metadata
    // are read-only: reader must not chmod the binding directory.
    fs::set_permissions(&f.bindings, fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(f.context(&f.source)["project"], root["project"]);
    fs::set_permissions(&f.bindings, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn readonly_xdg_uses_home_registry_and_keeps_it_after_xdg_becomes_writable() {
    let Some(f) = Fixture::no_git() else {
        return;
    };
    let call = |args: &[&str]| {
        f.preflight(&f.source.join("nested"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        support::git_topology::isolate(&mut command);
        command
            .current_dir(f.source.join("nested"))
            .args(args)
            .env("XDG_STATE_HOME", &f.state)
            .env("HOME", &f.base)
            .env_remove("EXITBIND_BINDINGS_DIR")
            .env_remove("SOULMATE_BINDINGS_DIR")
            .output()
            .unwrap()
    };
    fs::set_permissions(&f.source, fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o500)).unwrap();
    let out = call(&[
        "init",
        "--mode",
        "auto",
        "--skip-skills",
        "--root",
        f.source.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{out:?}");
    let first = call(&["project", "context", "--json"]);
    assert!(first.status.success(), "{first:?}");
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
    let later = call(&["project", "context", "--json"]);
    assert!(later.status.success(), "{later:?}");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let later: Value = serde_json::from_slice(&later.stdout).unwrap();
    assert_eq!(first["project"], later["project"]);
    assert_eq!(
        first["placement"]["stateRoot"],
        later["placement"]["stateRoot"]
    );
    assert!(!f.state.join("exitbind/bindings").exists());
    let home_registry = f.base.join(".local/state/exitbind/bindings");
    fs::set_permissions(&home_registry, fs::Permissions::from_mode(0o500)).unwrap();
    let unrelated = f.base.join("unrelated-source");
    fs::create_dir(&unrelated).unwrap();
    let other = call(&[
        "init",
        "--mode",
        "auto",
        "--skip-skills",
        "--root",
        unrelated.to_str().unwrap(),
    ]);
    fs::set_permissions(&home_registry, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(other.status.success(), "{other:?}");
    assert!(f.state.join("exitbind/bindings").exists());
    let both = call(&["project", "context", "--json"]);
    assert!(both.status.success(), "{both:?}");
    let both: Value = serde_json::from_slice(&both.stdout).unwrap();
    assert_eq!(first["project"], both["project"]);
    assert_eq!(
        first["placement"]["stateRoot"],
        both["placement"]["stateRoot"]
    );
    let repeated = call(&[
        "init",
        "--mode",
        "auto",
        "--skip-skills",
        "--root",
        f.source.to_str().unwrap(),
    ]);
    assert!(!repeated.status.success());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("already configured"));
}

#[test]
fn readonly_preferred_state_automatically_uses_external_owned_storage() {
    let Some(f) = Fixture::no_git() else {
        return;
    };
    let preferred = f.source.join(".exitbind");
    fs::create_dir(&preferred).unwrap();
    fs::set_permissions(&preferred, fs::Permissions::from_mode(0o500)).unwrap();
    let out = f.call(&f.source, &["init", "--skip-skills"]);
    fs::set_permissions(&preferred, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(out.status.success(), "{out:?}");
    let facts = f.context(&f.source.join("nested"));
    assert_eq!(facts["placement"]["sourceCapability"], "writable");
    assert_eq!(facts["placement"]["stateCapability"], "writable");
    assert!(!f.source.join("exitbind.json").exists());
    assert_ne!(facts["placement"]["stateRoot"], preferred.to_str().unwrap());
}

#[test]
fn gitfile_worktree_and_nested_cwd_discover_git_identity_without_mutating_metadata() {
    let mut f = Fixture::repository();
    f.git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--allow-empty",
        "-qm",
        "fixture",
    ]);
    let tree = f.base.join("linked");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "fixture-linked",
        tree.to_str().unwrap(),
    ]);
    fs::create_dir(tree.join("nested")).unwrap();
    f.git_root = Some(tree.clone());
    let marker = fs::read(tree.join(".git")).unwrap();
    assert!(fs::metadata(tree.join(".git")).unwrap().is_file());
    assert!(marker.starts_with(b"gitdir: "));
    let out = f.call(
        &tree.join("nested"),
        &["init", "--mode", "auto", "--skip-skills"],
    );
    assert!(out.status.success(), "{out:?}");
    let facts = f.context(&tree.join("nested"));
    assert_eq!(facts["placement"]["sourceRoot"], tree.to_str().unwrap());
    assert_eq!(facts["placement"]["git"]["topology"], "worktree");
    assert_ne!(
        facts["placement"]["git"]["gitDir"],
        facts["placement"]["git"]["commonDir"]
    );
    assert_eq!(fs::read(tree.join(".git")).unwrap(), marker);
    assert!(!tree.join("exitbind.json").exists());
}

#[test]
fn explicit_roots_win_and_unavailable_storage_is_a_classified_refusal() {
    let f = Fixture::repository();
    let control = f.base.join("chosen-control");
    let state = f.base.join("chosen-state");
    for root in [&control, &state] {
        fs::create_dir(root).unwrap();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        support::git_topology::assert_worktree(root, &f.base);
        assert_eq!(fs::canonicalize(root).unwrap(), *root);
    }
    let out = f.call(
        &f.source,
        &[
            "init",
            "--mode",
            "auto",
            "--skip-skills",
            "--project-id",
            "chosen",
            "--control-root",
            control.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let facts = f.context(&f.source.join("nested"));
    assert_eq!(facts["placement"]["stateRoot"], state.to_str().unwrap());
    assert_eq!(facts["project"]["identity"]["id"], "chosen");
    assert_eq!(facts["placement"]["git"]["state"], "available");
    let other = Fixture::repository();
    fs::set_permissions(&other.state, fs::Permissions::from_mode(0o500)).unwrap();
    fs::set_permissions(&other.base, fs::Permissions::from_mode(0o500)).unwrap();
    let out = other.call(&other.source, &["init", "--mode", "auto", "--skip-skills"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unsupported storage"),
        "{out:?}"
    );
    assert!(!other.source.join("exitbind.json").exists());
    fs::set_permissions(&other.base, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&other.state, fs::Permissions::from_mode(0o700)).unwrap();
}

fn source_snapshot(source: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, entries: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut children: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        children.sort();
        for child in children {
            let metadata = fs::symlink_metadata(&child).unwrap();
            let bytes = if metadata.is_file() {
                fs::read(&child).unwrap()
            } else {
                vec![]
            };
            entries.push((child.strip_prefix(root).unwrap().to_owned(), bytes));
            if metadata.is_dir() {
                visit(root, &child, entries);
            }
        }
    }
    let mut entries = vec![];
    visit(source, source, &mut entries);
    entries
}

#[test]
fn enclosing_git_nested_cwd_and_readonly_source_keep_canonical_identity() {
    let f = Fixture::repository();
    fs::write(f.source.join("keep.txt"), "source sentinel").unwrap();
    fs::set_permissions(&f.source, fs::Permissions::from_mode(0o555)).unwrap();
    let before = source_snapshot(&f.source);
    let out = f.call(
        &f.source.join("nested"),
        &["init", "--mode", "auto", "--skip-skills"],
    );
    assert!(out.status.success(), "{out:?}");
    let root = f.context(&f.source);
    let nested = f.context(&f.source.join("nested"));
    assert_eq!(root["project"], nested["project"]);
    assert_eq!(root["placement"]["sourceRoot"], f.source.to_str().unwrap());
    assert_eq!(root["placement"]["git"]["root"], f.source.to_str().unwrap());
    assert_eq!(root["placement"]["sourceCapability"], "read-only");
    assert_eq!(root["placement"]["stateCapability"], "writable");
    assert_eq!(source_snapshot(&f.source), before);
}

#[test]
fn valid_git_missing_executable_is_actionable_and_leaves_source_unchanged() {
    let f = Fixture::repository();
    let before = source_snapshot(&f.source);
    let out = f
        .command(
            &f.source.join("nested"),
            &["init", "--mode", "auto", "--skip-skills"],
        )
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let detail = String::from_utf8_lossy(&out.stderr);
    assert!(
        detail.contains("Git executable not found on PATH"),
        "{out:?}"
    );
    assert!(detail.contains("provide Git on PATH"), "{out:?}");
    assert_eq!(source_snapshot(&f.source), before);
}

#[test]
fn suspicious_gitfile_is_refused_before_project_mutation() {
    let f = Fixture::invalid();
    let before = source_snapshot(&f.source);
    for missing_git in [false, true] {
        let mut command = f.command(
            &f.source.join("nested"),
            &["init", "--mode", "auto", "--skip-skills"],
        );
        if missing_git {
            command.env("PATH", "/nonexistent");
        }
        let out = command.output().unwrap();
        assert!(!out.status.success());
        let detail = String::from_utf8_lossy(&out.stderr);
        assert!(
            detail.contains(if missing_git {
                "Git executable not found"
            } else {
                "Git topology preflight failed"
            }),
            "{out:?}"
        );
        assert!(
            detail.contains(if missing_git {
                "provide Git on PATH"
            } else {
                "inspect the marker"
            }),
            "{out:?}"
        );
        assert_eq!(source_snapshot(&f.source), before);
    }
}

#[test]
fn incompatible_explicit_roots_are_refused_before_creating_source_paths() {
    let f = Fixture::repository();
    let control = f.source.join("must-not-create-control");
    let state = f.source.join("must-not-create-state");
    let before = source_snapshot(&f.source);
    for mode in ["auto", "portable"] {
        let out = f.call(
            &f.source,
            &[
                "init",
                "--mode",
                mode,
                "--skip-skills",
                "--control-root",
                control.to_str().unwrap(),
                "--state-root",
                state.to_str().unwrap(),
            ],
        );
        assert!(!out.status.success());
        let detail = String::from_utf8_lossy(&out.stderr);
        assert!(detail.contains("preflight"), "{out:?}");
        assert!(
            detail.contains(if mode == "auto" {
                "outside source"
            } else {
                "require --mode local or auto"
            }),
            "{out:?}"
        );
        assert_eq!(source_snapshot(&f.source), before);
    }
}

#[test]
fn unsafe_storage_is_actionable_without_source_mutation() {
    let f = Fixture::repository();
    let unsafe_root = f.base.join("unsafe-root");
    std::os::unix::fs::symlink(&f.source, &unsafe_root).unwrap();
    let before = source_snapshot(&f.source);
    let out = f.call(
        &f.source,
        &[
            "init",
            "--mode",
            "auto",
            "--skip-skills",
            "--control-root",
            unsafe_root.to_str().unwrap(),
            "--state-root",
            f.state.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    let detail = String::from_utf8_lossy(&out.stderr);
    assert!(detail.contains("storage preflight"), "{out:?}");
    assert!(detail.contains("symlinks"), "{out:?}");
    assert!(detail.contains("outside source"), "{out:?}");
    assert_eq!(source_snapshot(&f.source), before);
}

#[test]
fn marker_free_missing_git_keeps_explicit_fallback_and_source_identity() {
    let Some(f) = Fixture::no_git() else {
        return;
    };
    let out = f
        .command(&f.source, &["init", "--mode", "portable", "--skip-skills"])
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let out = f
        .command(&f.source, &["project", "context", "--json"])
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let context: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        context["placement"]["sourceRoot"],
        f.source.to_str().unwrap()
    );
    assert_eq!(
        context["placement"]["git"]["discovery"],
        "marker-free-fallback"
    );
}

#[test]
fn topology_preflight_observes_enclosing_repository_and_invalid_gitfile() {
    let f = Fixture::repository();
    assert!(support::git_topology::marker(&f.source.join("nested")).is_some());
    assert_eq!(
        support::git_topology::worktree(&f.source.join("nested")),
        Some(f.source.clone())
    );
    fs::write(
        f.source.join("nested/.git"),
        "gitdir: missing-fixture-git-directory\n",
    )
    .unwrap();
    assert_eq!(
        support::git_topology::marker(&f.source.join("nested")),
        Some(f.source.join("nested/.git"))
    );
    assert!(support::git_topology::worktree(&f.source.join("nested")).is_none());
}
