mod support;

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn invoke(arguments: &[&str]) -> Output {
    invoke_env(arguments, &[])
}

fn invoke_env(arguments: &[&str], environment: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command.args(arguments);
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn setup_without_host(root: &Path, apply: bool, skip_skills: bool) -> Output {
    let root = root.to_str().unwrap();
    let mut args = vec![
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root,
        "--scope",
        "worker",
        "--observe",
        "README.md,src",
        "--write",
        "src",
        "--commands",
        "cargo fmt --check",
        "--check-command",
        "true",
        "--goal",
        "test setup",
        "--review-policy",
        "required",
    ];
    if skip_skills {
        args.push("--skip-skills");
    }
    if apply {
        args.push("--apply");
    }
    invoke_env(&args, &[("PATH", Path::new("/definitely/missing"))])
}

fn setup(root: &Path, apply: bool) -> Output {
    let root = root.to_str().unwrap();
    let mut args = vec![
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root,
        "--scope",
        "worker",
        "--observe",
        "README.md,src",
        "--write",
        "src",
        "--commands",
        "cargo fmt --check",
        "--check-command",
        "true",
        "--hosts",
        "codex",
        "--skip-skills",
        "--goal",
        "test setup",
        "--review-policy",
        "required",
    ];
    if apply {
        args.push("--apply");
    }
    invoke(&args)
}

fn setup_missing_path_host(root: &Path) -> Output {
    let root = root.to_str().unwrap();
    let args = [
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root,
        "--scope",
        "worker",
        "--observe",
        "README.md",
        "--write",
        "src",
        "--commands",
        "cargo fmt --check",
        "--check-command",
        "true",
        "--hosts",
        "codex",
        "--skip-skills",
        "--goal",
        "path",
        "--review-policy",
        "omitted",
    ];
    invoke_env(&args, &[("PATH", Path::new("/definitely/missing"))])
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn setup_previews_then_applies_and_repeats_without_changing_managed_bytes() {
    let root = support::temp("setup");
    let preview = setup(&root, false);
    assert!(preview.status.success(), "{}", text(&preview));
    assert!(text(&preview).contains("\"status\":\"preview\""));
    assert!(!root.join("exitbind.json").exists());

    let first = setup(&root, true);
    assert!(first.status.success(), "{}", text(&first));
    assert!(text(&first).contains("\"status\":\"applied\""));
    let config = fs::read(root.join("exitbind.json")).unwrap();
    let config_modified = fs::metadata(root.join("exitbind.json"))
        .unwrap()
        .modified()
        .unwrap();
    let profile = fs::read(root.join("exitbind/agents/worker.md")).unwrap();
    let native = fs::read(root.join(".codex/agents/worker.toml")).unwrap();

    let repeat = setup(&root, true);
    assert!(repeat.status.success(), "{}", text(&repeat));
    assert!(text(&repeat).contains("\"status\":\"unchanged\""));
    assert_eq!(fs::read(root.join("exitbind.json")).unwrap(), config);
    assert_eq!(
        fs::metadata(root.join("exitbind.json"))
            .unwrap()
            .modified()
            .unwrap(),
        config_modified
    );
    assert_eq!(
        fs::read(root.join("exitbind/agents/worker.md")).unwrap(),
        profile
    );
    assert_eq!(
        fs::read(root.join(".codex/agents/worker.toml")).unwrap(),
        native
    );

    let init = invoke(&[
        "init",
        "--mode",
        "portable",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(!init.status.success());
    assert!(
        text(&init).contains("init never overwrites it"),
        "{}",
        text(&init)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_refuses_external_projection_before_fresh_init() {
    let root = support::temp("setup-conflict");
    fs::create_dir_all(root.join(".codex/agents")).unwrap();
    fs::write(root.join(".codex/agents/worker.toml"), b"operator-owned\n").unwrap();
    let output = setup(&root, true);
    assert!(!output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("external file is preserved"));
    assert!(!root.join("exitbind.json").exists());
    assert_eq!(
        fs::read(root.join(".codex/agents/worker.toml")).unwrap(),
        b"operator-owned\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_preserves_custom_profile_and_reports_it_on_repeat() {
    let root = support::temp("setup-custom-profile");
    let first = setup_without_host(&root, true, true);
    assert!(first.status.success(), "{}", text(&first));
    let custom = b"# operator-owned worker profile\n";
    fs::write(root.join("exitbind/agents/worker.md"), custom).unwrap();
    let repeat = setup_without_host(&root, true, true);
    assert!(repeat.status.success(), "{}", text(&repeat));
    assert!(text(&repeat).contains("\"state\":\"custom\""));
    assert_eq!(
        fs::read(root.join("exitbind/agents/worker.md")).unwrap(),
        custom
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fresh_skip_skills_setup_reports_initializer_materialization_then_repeat_noop() {
    let root = support::temp("setup-fresh-no-host");
    let first = setup_without_host(&root, true, true);
    assert!(first.status.success(), "{}", text(&first));
    assert!(text(&first).contains("\"status\":\"applied\""));
    assert!(text(&first).contains("\"configChanged\":true"));
    let config = fs::read(root.join("exitbind.json")).unwrap();
    let modified = fs::metadata(root.join("exitbind.json"))
        .unwrap()
        .modified()
        .unwrap();

    let repeat = setup_without_host(&root, true, true);
    assert!(repeat.status.success(), "{}", text(&repeat));
    assert!(text(&repeat).contains("\"status\":\"unchanged\""));
    assert!(text(&repeat).contains("\"configChanged\":false"));
    assert_eq!(fs::read(root.join("exitbind.json")).unwrap(), config);
    assert_eq!(
        fs::metadata(root.join("exitbind.json"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_reports_missing_selected_host_path_without_claiming_activation() {
    let root = support::temp("setup-path");
    let output = setup_missing_path_host(&root);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("\"pathStatus\":\"missing\""));
    assert!(text(&output).contains("\"activation\":\"not performed\""));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn setup_does_not_report_a_nonexecutable_path_entry_as_an_available_host() {
    use std::os::unix::fs::PermissionsExt;
    let root = support::temp("setup-nonexecutable-host");
    let bin = support::temp("setup-nonexecutable-path");
    let host = bin.join("codex");
    fs::write(&host, "not an executable\n").unwrap();
    fs::set_permissions(&host, fs::Permissions::from_mode(0o600)).unwrap();
    let output = invoke_env(
        &[
            "setup",
            "--root",
            root.to_str().unwrap(),
            "--hosts",
            "codex",
            "--json",
        ],
        &[("PATH", &bin)],
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("\"pathStatus\":\"missing\""));
    assert!(!root.join("exitbind.json").exists());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(bin).unwrap();
}

#[test]
fn setup_reports_and_refuses_changed_owned_guidance() {
    let root = support::temp("setup-guidance");
    let first = setup_without_host(&root, true, false);
    assert!(first.status.success(), "{}", text(&first));
    let skill = root.join(".agents/skills/exitbind/SKILL.md");
    let mut changed = fs::read(&skill).unwrap();
    changed.extend_from_slice(b"operator changed managed guidance\n");
    fs::write(&skill, &changed).unwrap();
    let repeat = setup_without_host(&root, true, false);
    assert!(!repeat.status.success(), "{}", text(&repeat));
    assert!(text(&repeat).contains("managed guidance state"));
    assert_eq!(fs::read(skill).unwrap(), changed);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_supports_local_mode_and_non_ascii_preview_without_host_path() {
    let base = support::temp("setup-local");
    let product = base.join("项目");
    let control = base.join("control");
    let state = base.join("state");
    let bindings = base.join("bindings");
    fs::create_dir_all(&product).unwrap();
    fs::create_dir_all(&control).unwrap();
    fs::create_dir_all(&state).unwrap();
    let product_text = product.to_str().unwrap();
    let control_text = control.to_str().unwrap();
    let state_text = state.to_str().unwrap();
    let bindings_text = bindings.to_str().unwrap();
    let args = vec![
        "setup",
        "--json",
        "--apply",
        "--mode",
        "local",
        "--root",
        product_text,
        "--project-id",
        "unicode_project",
        "--control-root",
        control_text,
        "--state-root",
        state_text,
        "--scope",
        "worker",
        "--observe",
        "README.md",
        "--write",
        "src",
        "--commands",
        "cargo fmt --check",
        "--check-command",
        "true",
        "--goal",
        "local setup",
        "--review-policy",
        "omitted",
        "--skip-skills",
    ];
    let output = invoke_env(
        &args,
        &[
            ("PATH", Path::new("/definitely/missing")),
            ("EXITBIND_BINDINGS_DIR", Path::new(bindings_text)),
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(control.join("exitbind.json").is_file());
    assert!(state.join(".exitbind/runs").is_dir());
    fs::remove_dir_all(base).unwrap();
}
