mod support;

#[cfg(unix)]
#[path = "setup/lead_goal_text.rs"]
mod lead_goal_text;
#[cfg(unix)]
#[path = "setup/lesson_carry.rs"]
mod lesson_carry;
#[cfg(unix)]
#[path = "setup/native_display.rs"]
mod native_display;
#[cfg(unix)]
#[path = "setup/plan_card.rs"]
mod plan_card;

use std::{fs, path::Path, process::Output};

fn fixture(label: &str) -> std::path::PathBuf {
    let root = support::temp(label);
    support::git_topology::repository(&root);
    root
}

fn invoke(arguments: &[&str]) -> Output {
    invoke_env(arguments, &[])
}

fn invoke_env(arguments: &[&str], environment: &[(&str, &Path)]) -> Output {
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    support::git_topology::isolate(&mut command);
    command.args(arguments);
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn setup_without_host(root: &Path, apply: bool, skip_skills: bool) -> Output {
    let bin = support::git_topology::git_only_path(root);
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
    invoke_env(&args, &[("PATH", &bin)])
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
    let bin = support::git_topology::git_only_path(root);
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
    invoke_env(&args, &[("PATH", &bin)])
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
    let root = fixture("setup");
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
    let root = fixture("setup-conflict");
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
    let root = fixture("setup-custom-profile");
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
    let root = fixture("setup-fresh-no-host");
    let bin = support::git_topology::git_only_path(&root);
    let root_arg = root.to_str().unwrap();
    let first = invoke_env(
        &[
            "setup",
            "--mode",
            "portable",
            "--root",
            root_arg,
            "--apply",
            "--skip-skills",
            "--json",
        ],
        &[("PATH", &bin)],
    );
    assert!(first.status.success(), "{}", text(&first));
    assert!(text(&first).contains("\"status\":\"applied\""));
    assert!(text(&first).contains("\"configChanged\":true"));
    let config = fs::read(root.join("exitbind.json")).unwrap();
    let modified = fs::metadata(root.join("exitbind.json"))
        .unwrap()
        .modified()
        .unwrap();

    let repeat = invoke_env(
        &[
            "setup",
            "--mode",
            "portable",
            "--root",
            root_arg,
            "--apply",
            "--skip-skills",
            "--json",
        ],
        &[("PATH", &bin)],
    );
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
    let root = fixture("setup-path");
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
    let root = fixture("setup-nonexecutable-host");
    let bin = support::temp("setup-nonexecutable-path");
    let git_only = support::git_topology::git_only_path(&root);
    std::fs::copy(git_only.join("git"), bin.join("git")).unwrap();
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
fn setup_previews_and_persists_the_opt_in_owned_outputs_contract() {
    let root = fixture("setup-verification-preparation");
    let bin = support::git_topology::git_only_path(&root);
    let root_text = root.to_str().unwrap();
    let base = [
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root_text,
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
        "--verification-preparation",
        "owned-outputs",
        "--goal",
        "test setup",
        "--review-policy",
        "required",
        "--skip-skills",
    ];
    let preview = invoke_env(&base, &[("PATH", &bin)]);
    assert!(preview.status.success(), "{}", text(&preview));
    let preview: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(
        preview["approvedFacts"]["verificationPreparation"]["version"],
        1
    );
    assert_eq!(
        preview["approvedFacts"]["verificationPreparation"]["pythonBytecode"],
        "disabled"
    );
    assert_eq!(
        preview["approvedFacts"]["verificationPreparation"]["scratch"],
        "taskOwned"
    );
    assert!(
        preview["approvedFacts"]["verificationPreparation"]["scratchPath"]
            .as_str()
            .unwrap()
            .contains(".exitbind/verification/<run-id>/<operation-id>")
    );
    let mut apply = base.to_vec();
    apply.extend(["--apply"]);
    let result = invoke_env(&apply, &[("PATH", &bin)]);
    assert!(result.status.success(), "{}", text(&result));
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
    let canonical_schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema/exitbind.schema.json")).unwrap();
    let compatibility_schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema/soulmate.schema.json")).unwrap();
    assert_eq!(
        canonical_schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(config["$schema"], canonical_schema["$id"]);
    let declared_project_properties = canonical_schema["properties"]["project"]["properties"]
        .as_object()
        .unwrap();
    for property in config["project"].as_object().unwrap().keys() {
        assert!(
            declared_project_properties.contains_key(property),
            "generated project property {property:?} is absent from the canonical schema"
        );
    }
    let expected_policy = serde_json::json!({
        "version": 1,
        "pythonBytecode": "disabled",
        "scratch": "taskOwned"
    });
    assert_eq!(
        config["project"]["verificationPreparation"],
        expected_policy
    );
    for (name, schema) in [
        ("canonical", &canonical_schema),
        ("compatibility", &compatibility_schema),
    ] {
        let policy_schema =
            &schema["properties"]["project"]["properties"]["verificationPreparation"];
        assert!(policy_schema.is_object(), "{name} schema omits the policy");
        let required: std::collections::BTreeSet<_> = policy_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(
            required,
            ["pythonBytecode", "scratch", "version"]
                .into_iter()
                .collect()
        );
        let declared_policy_properties: std::collections::BTreeSet<_> = policy_schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let expected_properties: std::collections::BTreeSet<_> = expected_policy
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(declared_policy_properties, expected_properties);
        assert_eq!(policy_schema["properties"]["version"]["const"], 1);
        assert_eq!(
            policy_schema["properties"]["pythonBytecode"]["const"],
            "disabled"
        );
        assert_eq!(policy_schema["properties"]["scratch"]["const"], "taskOwned");
        assert_eq!(policy_schema["additionalProperties"], false);
    }
    assert_eq!(
        config["project"]["verificationPreparation"],
        serde_json::json!({
            "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
        })
    );
    let persisted = fs::read(root.join("exitbind.json")).unwrap();
    let repeat = invoke_env(&apply, &[("PATH", &bin)]);
    assert!(repeat.status.success(), "{}", text(&repeat));
    assert_eq!(fs::read(root.join("exitbind.json")).unwrap(), persisted);
    let repeated: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
    assert_eq!(
        repeated["project"]["verificationPreparation"],
        config["project"]["verificationPreparation"]
    );
}

#[test]
fn setup_reports_and_refuses_changed_owned_guidance() {
    let root = fixture("setup-guidance");
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
    let base = fixture("setup-local");
    let product = base.join("项目");
    let control = base.join("control");
    let state = base.join("state");
    let bindings = base.join("bindings");
    fs::create_dir_all(&product).unwrap();
    fs::create_dir_all(&control).unwrap();
    fs::create_dir_all(&state).unwrap();
    support::git_topology::repository(&product);
    let bin = support::git_topology::git_only_path(&product);
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
            ("PATH", &bin),
            ("EXITBIND_BINDINGS_DIR", Path::new(bindings_text)),
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(control.join("exitbind.json").is_file());
    assert!(state.join(".exitbind/runs").is_dir());
    fs::remove_dir_all(base).unwrap();
}
