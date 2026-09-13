use serde_json::{json, Value};
mod support;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn temp(label: &str) -> PathBuf {
    support::temp(&format!("rust-contract-{label}"))
}

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(arguments)
        .output()
        .unwrap()
}

fn project(label: &str) -> (PathBuf, String) {
    let root = temp(label);
    let output = invoke(&["init", "--root", root.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = root.join("soulmate.json").to_string_lossy().into_owned();
    (root, config)
}

#[test]
fn profile_audit_and_import_use_only_reviewed_bytes() {
    let (root, config) = project("profile");
    let source = root.join("portable.md");
    fs::write(
        &source,
        "# Portable worker\n\nBounded implementation only.\n",
    )
    .unwrap();
    let audit = invoke(&[
        "profile",
        "audit",
        source.to_str().unwrap(),
        "--json",
        "--config",
        &config,
    ]);
    assert!(audit.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&audit.stdout).unwrap()["valid"],
        true
    );

    let imported = invoke(&[
        "profile",
        "import",
        "portable_worker",
        source.to_str().unwrap(),
        "--purpose",
        "Implement one bounded task",
        "--config",
        &config,
    ]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert_eq!(
        fs::read(root.join("soulmate/agents/portable_worker.md")).unwrap(),
        fs::read(&source).unwrap()
    );
    let configured: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(
        configured["agents"]["portable_worker"]["profile"],
        "soulmate/agents/portable_worker.md"
    );
    assert!(invoke(&["check", "--config", &config]).status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_distribution_profile_path_remains_valid() {
    let (root, config) = project("legacy-profile");
    let legacy = root.join(".agents/profiles");
    fs::create_dir_all(&legacy).unwrap();
    fs::copy(
        root.join("soulmate/agents/worker.md"),
        legacy.join("worker.md"),
    )
    .unwrap();
    let mut configured: Value =
        serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    configured["agents"]["worker"]["profile"] = json!(".agents/profiles/worker.md");
    fs::write(
        &config,
        format!("{}\n", serde_json::to_string_pretty(&configured).unwrap()),
    )
    .unwrap();

    assert!(invoke(&["check", "--config", &config]).status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn forgetting_receipt_is_content_free_and_requires_a_terminal_item() {
    let (root, config) = project("forgetting");
    let mut configured: Value =
        serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    for right in [
        "memoryWrite",
        "memoryReview",
        "memoryReject",
        "memoryForget",
    ] {
        configured["agents"]["worker"][right] = json!(["invariants"]);
    }
    fs::write(
        &config,
        format!("{}\n", serde_json::to_string_pretty(&configured).unwrap()),
    )
    .unwrap();
    fs::write(root.join("memory.md"), "private governed fixture\n").unwrap();
    let ledger = ".soulmate/memory.jsonl";
    assert!(invoke(&[
        "memory",
        "propose",
        "worker",
        "memory.md",
        "--scope",
        "invariants",
        "--ledger",
        ledger,
        "--config",
        &config,
    ])
    .status
    .success());
    assert!(
        invoke(&["memory", "review", "worker", ledger, "--config", &config])
            .status
            .success()
    );
    assert!(
        invoke(&["memory", "reject", "worker", ledger, "--config", &config])
            .status
            .success()
    );
    fs::remove_file(root.join("memory.md")).unwrap();
    let receipt = invoke(&[
        "memory",
        "attest-forgotten",
        "worker",
        ledger,
        "--receipt",
        ".soulmate/forgotten.json",
        "--config",
        &config,
    ]);
    assert!(
        receipt.status.success(),
        "{}",
        String::from_utf8_lossy(&receipt.stderr)
    );
    let bytes = fs::read_to_string(root.join(".soulmate/forgotten.json")).unwrap();
    let value: Value = serde_json::from_str(&bytes).unwrap();
    assert_eq!(value["terminalState"], "rejected");
    assert_eq!(value["observed"], "source-absent");
    assert!(!bytes.contains("memory.md"));
    assert!(!bytes.contains("private governed fixture"));
    fs::remove_dir_all(root).unwrap();
}

fn with_binary_path(command: &mut Command, bin: &Path) {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(&current));
    command.env("PATH", std::env::join_paths(paths).unwrap());
}

#[cfg(unix)]
#[test]
fn hooks_preserve_unrelated_settings_and_hook_runtime_presents_bounded_context() {
    use std::os::unix::fs::symlink;

    let (root, config) = project("hooks");
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    symlink(env!("CARGO_BIN_EXE_soulmate"), bin.join("soulmate")).unwrap();
    fs::create_dir(root.join(".codex")).unwrap();
    fs::write(root.join(".codex/hooks.json"), "{\"keep\":true}\n").unwrap();

    let mut apply = Command::new(env!("CARGO_BIN_EXE_soulmate"));
    with_binary_path(&mut apply, &bin);
    let applied = apply
        .args([
            "hooks",
            "apply",
            "--hosts",
            "codex",
            "--root",
            root.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(root.join(".codex/hooks.json")).unwrap()).unwrap();
    assert_eq!(settings["keep"], true);
    assert!(settings["hooks"]["SessionStart"].is_array());

    let mut child = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        child.stdin.take().unwrap(),
        "{}",
        json!({"hook_event_name":"SubagentStart","cwd":root,"agent_name":"worker"})
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let context: Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = context["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(text.contains("Profile selected/presented"));
    assert!(text.contains("does not prove a model read or followed"));

    let mut remove = Command::new(env!("CARGO_BIN_EXE_soulmate"));
    with_binary_path(&mut remove, &bin);
    let removed = remove
        .args([
            "hooks",
            "remove",
            "--hosts",
            "codex",
            "--root",
            root.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(removed.status.success());
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(root.join(".codex/hooks.json")).unwrap()).unwrap();
    assert_eq!(settings["keep"], true);
    assert!(match settings["hooks"].get("SessionStart") {
        None => true,
        Some(value) => value.as_array().is_some_and(Vec::is_empty),
    });
    let _ = config;
    fs::remove_dir_all(root).unwrap();
}
