use serde_json::{json, Value};
mod support;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::Duration,
};

fn invoke(arguments: &[&str], codex: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_soulmate"));
    command.args(arguments);
    if let Some(codex) = codex {
        command.env("SOULMATE_AWAY_CODEX_BIN", codex);
    }
    command.output().unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn project() -> (PathBuf, String) {
    let root = support::temp("away-native");
    let initialized = invoke(&["init", "--root", root.to_str().unwrap()], None);
    assert!(initialized.status.success(), "{}", text(&initialized));
    let config = root.join("soulmate.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    value["agents"]["lead"]["runtime"] = json!({"host":"codex", "fallback":"none"});
    fs::write(
        &config,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    let config = config.to_string_lossy().into_owned();
    (root, config)
}

#[test]
fn required_harness_refuses_before_any_codex_launch() {
    let (root, config) = project();
    let started = invoke(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "bounded",
            "--ledger",
            ".soulmate/run.jsonl",
            "--config",
            &config,
        ],
        None,
    );
    assert!(started.status.success(), "{}", text(&started));

    let marker = root.join("codex-invoked");
    let fake = root.join("fake-codex");
    fs::write(
        &fake,
        format!("#!/bin/sh\nprintf invoked > '{}'\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let away = invoke(
        &[
            "away",
            "start",
            "lead",
            ".soulmate/run.jsonl",
            "--require-harness-receipt",
            "--config",
            &config,
        ],
        Some(&fake),
    );
    assert!(!away.status.success(), "{}", text(&away));
    assert!(text(&away).contains("requires a bound harness receipt"));
    assert!(!marker.exists());
    assert!(root.join(".soulmate/away").is_dir());
    assert!(fs::read_dir(root.join(".soulmate/away"))
        .unwrap()
        .next()
        .is_none());

    fs::write(
        root.join("harness-manifest.json"),
        serde_json::to_string_pretty(&json!({
            "version":1,
            "project":{"id":"away-native","session":"drift"},
            "harness":{"name":"codex","version":"test"},
            "activations":[{"kind":"skill","name":"soulmate","evidence":"presented"}]
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = invoke(
        &[
            "plan",
            "change",
            "--goal",
            "bounded",
            "--receipt",
            ".soulmate/harness-receipt.json",
            "--harness-manifest",
            "harness-manifest.json",
            "--config",
            &config,
        ],
        None,
    );
    assert!(receipt.status.success(), "{}", text(&receipt));
    let bound = invoke(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "bounded",
            "--ledger",
            ".soulmate/bound.jsonl",
            "--harness-receipt",
            ".soulmate/harness-receipt.json",
            "--config",
            &config,
        ],
        None,
    );
    assert!(bound.status.success(), "{}", text(&bound));
    use std::io::Write;
    writeln!(fs::OpenOptions::new()
        .append(true)
        .open(root.join(".soulmate/harness-receipt.json"))
        .unwrap())
    .unwrap();
    let drifted = invoke(
        &[
            "away",
            "start",
            "lead",
            ".soulmate/bound.jsonl",
            "--require-harness-receipt",
            "--config",
            &config,
        ],
        Some(&fake),
    );
    assert!(!drifted.status.success(), "{}", text(&drifted));
    assert!(text(&drifted).contains("harness receipt drift"));
    assert!(!marker.exists());
    assert!(root.join(".soulmate/away").is_dir());
    assert!(fs::read_dir(root.join(".soulmate/away"))
        .unwrap()
        .next()
        .is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn list_and_show_read_only_bounded_recovery_state() {
    let (root, config) = project();
    let run = root.join(".soulmate/away/20260830T000000Z-0123456789abcdef-test");
    fs::create_dir_all(&run).unwrap();
    fs::write(run.join("status"), "completed\n").unwrap();
    fs::write(run.join("agent"), "lead\n").unwrap();
    fs::write(run.join("sandbox-posture"), "unknown\n").unwrap();

    let listed = invoke(&["away", "list", "--config", &config], None);
    assert!(listed.status.success(), "{}", text(&listed));
    assert!(text(&listed).contains("completed"));
    let shown = invoke(
        &[
            "away",
            "show",
            "20260830T000000Z-0123456789abcdef-test",
            "--config",
            &config,
        ],
        None,
    );
    assert!(shown.status.success(), "{}", text(&shown));
    assert!(text(&shown).contains("status=completed"));
    assert!(text(&shown).contains("agent=lead"));
    assert!(text(&shown).contains("sandbox_posture=unknown"));
    fs::write(run.join("sandbox-posture"), "workspace-write\n").unwrap();
    let explicit = invoke(
        &[
            "away",
            "show",
            "20260830T000000Z-0123456789abcdef-test",
            "--config",
            &config,
        ],
        None,
    );
    assert!(explicit.status.success(), "{}", text(&explicit));
    assert!(text(&explicit).contains("sandbox_posture=workspace-write"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires a real tmux executable"]
fn real_tmux_child_presents_bound_evidence_without_persisting_the_prompt() {
    let (root, config) = project();
    fs::write(
        root.join("harness-manifest.json"),
        serde_json::to_string_pretty(&json!({
            "version":1,
            "project":{"id":"away-native","session":"tmux-e2e"},
            "harness":{"name":"codex","version":"test"},
            "activations":[
                {"kind":"skill","name":"soulmate","evidence":"presented"},
                {"kind":"ponytail","name":"ponytail:ponytail","evidence":"hook_observed"}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = invoke(
        &[
            "plan",
            "change",
            "--goal",
            "tmux evidence",
            "--receipt",
            ".soulmate/harness-receipt.json",
            "--harness-manifest",
            "harness-manifest.json",
            "--config",
            &config,
        ],
        None,
    );
    assert!(receipt.status.success(), "{}", text(&receipt));
    let run = invoke(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "tmux evidence",
            "--ledger",
            ".soulmate/run.jsonl",
            "--harness-receipt",
            ".soulmate/harness-receipt.json",
            "--config",
            &config,
        ],
        None,
    );
    assert!(run.status.success(), "{}", text(&run));

    let capture = root.join("captured-prompt");
    let captured_args = root.join("captured-args");
    let fake = root.join("fake-codex");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\nif [ \"$1\" = exec ] && [ \"$2\" = --help ]; then echo '-c -C --add-dir --ephemeral -m'; exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\ncat > '{}'\ncat '{}' >&2\nsleep 1\nexit 0\n",
            captured_args.display(),
            capture.display(),
            capture.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let away = invoke(
        &[
            "away",
            "start",
            "lead",
            ".soulmate/run.jsonl",
            "--require-harness-receipt",
            "--name",
            "e2e",
            "--sandbox-mode",
            "workspace-write",
            "--config",
            &config,
        ],
        Some(&fake),
    );
    assert!(away.status.success(), "{}", text(&away));
    let output = text(&away);
    let field = |name: &str| {
        output
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
            .unwrap()
            .to_owned()
    };
    let state = PathBuf::from(field("state"));
    let socket = field("socket");
    let duplicate = invoke(
        &[
            "away",
            "start",
            "lead",
            ".soulmate/run.jsonl",
            "--require-harness-receipt",
            "--name",
            "duplicate",
            "--config",
            &config,
        ],
        Some(&fake),
    );
    assert!(!duplicate.status.success(), "{}", text(&duplicate));
    assert!(text(&duplicate).contains("already has an active away runner"));
    for _ in 0..100 {
        let status = fs::read_to_string(state.join("status")).unwrap_or_default();
        if matches!(status.trim(), "unsubmitted" | "failed" | "completed") {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let status = fs::read_to_string(state.join("status")).unwrap();
    assert_eq!(status.trim(), "unsubmitted");
    assert_eq!(
        fs::read_to_string(state.join("sandbox-posture"))
            .unwrap()
            .trim(),
        "workspace-write"
    );
    assert!(fs::read_to_string(captured_args)
        .unwrap()
        .contains("sandbox_mode=\"workspace-write\""));
    let prompt = fs::read_to_string(&capture).unwrap();
    assert!(prompt.contains("ponytail:ponytail"));
    assert!(prompt.contains("hook_observed"));
    assert!(prompt.contains("# Authoritative execution contract"));
    assert!(prompt.contains("## Authoritative run context"));
    assert!(prompt.contains("## Authoritative assignment packet"));
    assert!(prompt.contains("# Reviewed role guidance"));
    assert!(prompt.contains("# Context-only memory (non-authoritative)"));
    assert!(prompt.contains("# Evidence-only harness claims (non-authoritative)"));
    assert!(prompt.contains("\"workflow\": \"change\""));
    assert!(prompt.contains("\"authority\": \"reviewed-guidance\""));
    assert!(prompt.contains("\"authority\": \"evidence-only\""));
    for private in [
        "prompt",
        "packet",
        "manifest",
        "transcript",
        "environment",
        "stderr.log",
        "final.txt",
    ] {
        assert!(!state.join(private).exists(), "{private}");
    }
    let _ = Command::new("tmux")
        .args(["-L", &socket, "kill-server"])
        .output();
    fs::remove_dir_all(root).unwrap();
}
