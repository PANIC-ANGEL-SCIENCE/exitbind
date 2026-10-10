// Current Exitbind and preserved historical-reader contracts run in the default suite.
mod support;

use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const EXITBIND_SKILL: &[u8] = include_bytes!("../skills/exitbind/SKILL.md");
const EXITBIND_REFERENCE: &[u8] = include_bytes!("../skills/exitbind/references/operators.md");

fn temp(label: &str) -> PathBuf {
    support::temp(&format!("skill-diagnostics-{label}"))
}

fn invoke(arguments: &[&str], bindings: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .env("EXITBIND_BINDINGS_DIR", bindings)
        .args(arguments)
        .output()
        .unwrap()
}

#[cfg(unix)]
fn invoke_with_path(arguments: &[&str], bindings: &Path, path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .env("EXITBIND_BINDINGS_DIR", bindings)
        .env("PATH", path)
        .args(arguments)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn hash(bytes: &[u8]) -> String {
    let mut output = String::new();
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

#[test]
fn cache_guidance_keeps_host_and_session_layers_unverified() {
    let guidance = String::from_utf8_lossy(EXITBIND_REFERENCE).to_lowercase();
    for phrase in [
        "ordinary host-managed workers/reviewers consume",
        "read this only for an initialized same-work cross-host handoff",
        "never search raw state",
        "pass the `mutationcontext.token`",
        "a preview never approves a proposal",
    ] {
        assert!(
            guidance.contains(phrase),
            "missing current operator guidance: {phrase}"
        );
    }
}

fn local_project(label: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let base = temp(label);
    let product = base.join("product");
    let control = base.join("control");
    let state = base.join("state");
    let bindings = base.join("bindings");
    for path in [&product, &control, &state, &bindings] {
        fs::create_dir(path).unwrap();
    }
    let product_repository = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&product)
        .status()
        .unwrap();
    assert!(
        product_repository.success(),
        "local fixture requires its own Git root"
    );
    let initialized = invoke(
        &[
            "init",
            "--mode",
            "local",
            "--project-id",
            "skill_diagnostics_fixture",
            "--root",
            product.to_str().unwrap(),
            "--control-root",
            control.to_str().unwrap(),
            "--state-root",
            state.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(
        initialized.status.success(),
        "local init failed: stdout={} stderr={}",
        text(&initialized.stdout),
        text(&initialized.stderr)
    );
    (base, product, control, bindings)
}

fn expected_json() -> String {
    "{\"mode\":\"local\",\"projectId\":\"skill_diagnostics_fixture\",\"valid\":true,\"warnings\":[{\"classification\":\"empty_starter_boundary\",\"detail\":\"empty starter boundary: review observe, write, and commands before tasks needing project files or commands; empty declarations do not grant host permission or establish task readiness\"}]}\n".into()
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[test]
fn check_reports_hashes_and_keeps_json_stdout_stable() {
    let (base, _product, control, bindings) = local_project("match");
    let config = control.join("exitbind.json");
    let json = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(json.status.success(), "{}", text(&json.stderr));
    assert_eq!(json.stdout, expected_json().as_bytes());
    assert!(json.stderr.is_empty());

    let human = invoke(&["check", "--config", config.to_str().unwrap()], &bindings);
    assert!(human.status.success(), "{}", text(&human.stderr));
    let stdout = text(&human.stdout);
    let embedded = hash(EXITBIND_SKILL);
    assert!(stdout.contains(&format!("Exitbind package {}", env!("CARGO_PKG_VERSION"))));
    assert!(stdout.contains(&format!("embedded SHA256 {embedded}")));
    assert!(stdout.contains(&format!("observed SHA256 {embedded}")));
    assert!(stdout.contains(".agents/skills/exitbind/SKILL.md"));
    assert!(stdout.contains(".claude/skills/exitbind/SKILL.md"));
    assert!(!stdout.contains("Coffee"));

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn managed_drift_warns_on_stderr_without_editing_or_relabeling_version() {
    let (base, _product, control, bindings) = local_project("drift-older-newer");
    let config = control.join("exitbind.json");
    let skill = control.join(".agents/skills/exitbind/SKILL.md");
    let edited = b"<!-- exitbind-managed-skill:v1 -->\noperator edit\n";
    fs::write(&skill, edited).unwrap();

    let checked = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert_eq!(checked.stdout, expected_json().as_bytes());
    let stderr = text(&checked.stderr);
    assert!(stderr.contains("differs from this binary's embedded skill"));
    assert!(stderr.contains(&format!("Exitbind package {}", env!("CARGO_PKG_VERSION"))));
    assert!(stderr.contains(&format!("embedded SHA256 {}", hash(EXITBIND_SKILL))));
    assert!(stderr.contains(&format!("observed SHA256 {}", hash(edited))));
    assert!(stderr.contains("init --refresh-skills --root"));
    let shell_quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let expected_warning = format!(
        "warning: Exitbind managed skill .agents/skills/exitbind/SKILL.md differs from this binary's embedded skill (Exitbind package {}; invoking binary {}; embedded SHA256 {}; observed SHA256 {}). Inspect the invoking binary version/path; Explicitly run matching binary {} init --refresh-skills --root {}. Install the intended release if needed.\n",
        env!("CARGO_PKG_VERSION"),
        shell_quote(env!("CARGO_BIN_EXE_exitbind")),
        hash(EXITBIND_SKILL),
        hash(edited),
        shell_quote(env!("CARGO_BIN_EXE_exitbind")),
        shell_quote(control.to_str().unwrap()),
    );
    assert_eq!(stderr, expected_warning);
    assert!(stderr.contains("older-newer"));
    assert_eq!(fs::read(&skill).unwrap(), edited);

    let human = invoke(&["check", "--config", config.to_str().unwrap()], &bindings);
    assert!(human.status.success(), "{}", text(&human.stderr));
    assert!(text(&human.stderr).contains("differs from this binary's embedded skill"));
    assert!(text(&human.stderr).contains("init --refresh-skills --root"));

    fs::remove_dir_all(base).unwrap();
}

#[cfg(unix)]
#[test]
fn hostile_control_root_paths_are_escaped_and_marked_non_copyable() {
    let (base, _product, control, bindings) = local_project("hostile-\u{1b}[31m");
    let config = control.join("exitbind.json");
    fs::write(
        control.join(".agents/skills/exitbind/SKILL.md"),
        b"<!-- exitbind-managed-skill:v1 -->\noperator edit\n",
    )
    .unwrap();

    let checked = invoke(&["check", "--config", config.to_str().unwrap()], &bindings);
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(!checked.stdout.contains(&0x1b));
    assert!(!checked.stderr.contains(&0x1b));
    assert!(text(&checked.stderr).contains("\\u{1b}"));
    assert!(text(&checked.stderr).contains("No copyable refresh command is available"));
    assert!(!text(&checked.stderr).contains("exitbind init --refresh-skills --root"));

    let json = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(json.status.success(), "{}", text(&json.stderr));
    assert_eq!(json.stdout, expected_json().as_bytes());
    assert!(!json.stdout.contains(&0x1b));
    assert!(!json.stderr.contains(&0x1b));

    fs::remove_dir_all(base).unwrap();
}

#[cfg(unix)]
#[test]
fn drift_warning_uses_exact_invoking_binary_when_path_has_another_exitbind() {
    let (base, _product, control, bindings) = local_project("path-binary");
    let config = control.join("exitbind.json");
    fs::write(
        control.join(".agents/skills/exitbind/SKILL.md"),
        b"<!-- exitbind-managed-skill:v1 -->\noperator edit\n",
    )
    .unwrap();
    let fake_bin = base.join("other-bin");
    fs::create_dir(&fake_bin).unwrap();
    let fake_exitbind = fake_bin.join("exitbind");
    fs::write(&fake_exitbind, b"#!/bin/sh\nexit 77\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake_exitbind, fs::Permissions::from_mode(0o755)).unwrap();

    let checked = invoke_with_path(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
        &fake_bin,
    );
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    let stderr = text(&checked.stderr);
    let exact_binary = shell_quote(env!("CARGO_BIN_EXE_exitbind"));
    let exact_root = shell_quote(control.to_str().unwrap());
    assert!(stderr.contains(&format!(
        "Explicitly run matching binary {exact_binary} init --refresh-skills --root {exact_root}."
    )));
    assert!(!stderr.contains(&format!(
        "Explicitly run matching binary {}",
        shell_quote("exitbind")
    )));
    assert!(!stderr.contains(fake_exitbind.to_str().unwrap()));

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn check_does_not_create_absent_skill_directories() {
    let (base, _product, control, bindings) = local_project("absent");
    let config = control.join("exitbind.json");
    let agents_skills = control.join(".agents/skills");
    let claude_skills = control.join(".claude/skills");
    fs::remove_dir_all(&agents_skills).unwrap();
    fs::remove_dir_all(&claude_skills).unwrap();

    let checked = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert_eq!(checked.stdout, expected_json().as_bytes());
    assert!(checked.stderr.is_empty());
    assert!(!agents_skills.exists());
    assert!(!claude_skills.exists());

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn current_refresh_is_exitbind_only_and_refuses_the_retired_coffee_flag() {
    let (base, _product, control, bindings) = local_project("refresh-metadata");
    let config_before = fs::read(control.join("exitbind.json")).unwrap();
    let refreshed = invoke(
        &[
            "init",
            "--refresh-skills",
            "--root",
            control.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(refreshed.status.success(), "{}", text(&refreshed.stderr));
    let stdout = text(&refreshed.stdout);
    assert!(stdout.contains(&format!("Exitbind {}", env!("CARGO_PKG_VERSION"))));
    assert!(stdout.contains(&format!(
        "Exitbind embedded SHA256 {}",
        hash(EXITBIND_SKILL)
    )));
    assert!(!stdout.contains("coffee embedded SHA256"));
    assert_eq!(
        fs::read(control.join("exitbind.json")).unwrap(),
        config_before
    );

    let repeated = invoke(
        &[
            "init",
            "--refresh-skills",
            "--root",
            control.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(repeated.status.success(), "{}", text(&repeated.stderr));
    assert!(text(&repeated.stdout).contains("unchanged .agents/skills/exitbind/SKILL.md"));
    assert!(!text(&repeated.stdout).contains("coffee"));

    let refused_coffee = invoke(
        &[
            "init",
            "--refresh-skills",
            "--with-coffee",
            "--root",
            control.to_str().unwrap(),
        ],
        &bindings,
    );
    assert!(!refused_coffee.status.success());
    assert!(text(&refused_coffee.stderr).contains("--with-coffee is retired"));
    assert_eq!(
        fs::read(control.join("exitbind.json")).unwrap(),
        config_before
    );

    fs::remove_dir_all(base).unwrap();
}

#[cfg(unix)]
#[test]
fn check_classifies_unmanaged_invalid_bytes_and_unsafe_paths_without_following_them() {
    use std::os::unix::fs::symlink;

    let (base, _product, control, bindings) = local_project("unsafe");
    let config = control.join("exitbind.json");
    let unmanaged = control.join(".agents/skills/exitbind/SKILL.md");
    let invalid_utf8 = control.join(".claude/skills/exitbind/SKILL.md");
    fs::write(unmanaged, b"host-managed skill\n").unwrap();
    fs::write(&invalid_utf8, [0xff, 0xfe, 0x00]).unwrap();

    let third_party = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(
        third_party.status.success(),
        "{}",
        text(&third_party.stderr)
    );
    assert_eq!(third_party.stdout, expected_json().as_bytes());
    assert!(third_party.stderr.is_empty());

    let outside = base.join("outside");
    fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("sentinel.md");
    fs::write(&sentinel, b"outside bytes\n").unwrap();
    fs::remove_file(&invalid_utf8).unwrap();
    symlink(&sentinel, &invalid_utf8).unwrap();

    let symlink_check = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(
        symlink_check.status.success(),
        "{}",
        text(&symlink_check.stderr)
    );
    assert_eq!(symlink_check.stdout, expected_json().as_bytes());
    assert!(text(&symlink_check.stderr).contains("unsafe path or nonregular"));
    assert_eq!(fs::read(&sentinel).unwrap(), b"outside bytes\n");

    fs::remove_dir_all(base).unwrap();
}

#[cfg(unix)]
#[test]
fn check_classifies_symlink_ancestors_without_creating_or_traversing_them() {
    use std::os::unix::fs::symlink;

    let (base, _product, control, bindings) = local_project("ancestor");
    let config = control.join("exitbind.json");
    let outside = base.join("outside");
    let outside_skills = outside.join("skills");
    fs::create_dir_all(outside_skills.join("exitbind")).unwrap();
    let sentinel = outside_skills.join("exitbind/SKILL.md");
    fs::write(&sentinel, b"outside bytes\n").unwrap();
    let agents_skills = control.join(".agents/skills");
    fs::remove_dir_all(&agents_skills).unwrap();
    symlink(&outside_skills, &agents_skills).unwrap();

    let checked = invoke(
        &["check", "--json", "--config", config.to_str().unwrap()],
        &bindings,
    );
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert_eq!(checked.stdout, expected_json().as_bytes());
    assert!(text(&checked.stderr).contains("unsafe path or nonregular"));
    assert_eq!(fs::read(&sentinel).unwrap(), b"outside bytes\n");

    fs::remove_dir_all(base).unwrap();
}
