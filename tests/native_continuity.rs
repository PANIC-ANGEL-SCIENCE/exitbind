mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Fixture {
    base: PathBuf,
    product: PathBuf,
    control: PathBuf,
    bindings: PathBuf,
    host: PathBuf,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let base = support::temp(&format!("native-continuity-{mode}"));
        let product = base.join("product");
        let bindings = base.join("bindings");
        let host = base.join("host");
        for path in [&product, &bindings, &host] {
            fs::create_dir(path).unwrap();
        }
        let control = if mode == "local" {
            base.join("control")
        } else {
            product.clone()
        };
        let mut init = Command::new(env!("CARGO_BIN_EXE_soulmate"));
        init.env("SOULMATE_BINDINGS_DIR", &bindings)
            .args(["init", "--mode", mode, "--root"])
            .arg(&product);
        if mode == "local" {
            let state = base.join("state");
            fs::create_dir(&control).unwrap();
            fs::create_dir(&state).unwrap();
            init.args([
                "--project-id",
                "native_continuity_fixture",
                "--control-root",
            ])
            .arg(&control)
            .arg("--state-root")
            .arg(state);
        }
        let output = init.output().unwrap();
        assert!(output.status.success(), "{:?}", output);
        fs::write(host.join("session.jsonl"), b"native session sentinel\n").unwrap();
        fs::write(host.join("config.toml"), b"native config sentinel\n").unwrap();
        fs::create_dir(host.join("commands")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["codex", "claude"] {
                let path = host.join("commands").join(name);
                fs::write(
                    &path,
                    b"#!/bin/sh\nprintf launched >> \"$SOULMATE_TEST_LAUNCH_SENTINEL\"\nexit 99\n",
                )
                .unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        Self {
            base,
            product,
            control,
            bindings,
            host,
        }
    }

    fn hook(&self, input: &[u8]) -> Output {
        self.hook_with_update(input, false)
    }

    fn hook_with_update(&self, input: &[u8], update: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_soulmate"));
        command
            .arg("hook-run")
            .env("SOULMATE_BINDINGS_DIR", &self.bindings)
            .env("PATH", self.host.join("commands"))
            .env("HOME", &self.base)
            .env("XDG_CACHE_HOME", self.base.join("cache"))
            .env("SOULMATE_TEST_LAUNCH_SENTINEL", self.host.join("launched"));
        if !update {
            command.env("SOULMATE_NO_UPDATE_CHECK", "1");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn payload(&self, event: &str, source: &str) -> Value {
        json!({
            "hook_event_name": event,
            "source": source,
            "cwd": self.product,
            "session_id": "existing-root-sentinel",
            "transcript_path": self.host.join("session.jsonl"),
            "session_path": self.host.join("session.jsonl"),
            "config_path": self.host.join("config.toml"),
            "command": "codex resume existing-root-sentinel"
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.base).unwrap();
    }
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(snapshot(&path));
        } else {
            files.push((path.clone(), fs::read(path).unwrap()));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn session_lifecycle_hooks_only_add_bounded_context_and_preserve_host_files() {
    for mode in ["portable", "local"] {
        let fixture = Fixture::new(mode);
        let before = snapshot(&fixture.base);
        let mut previous = None;
        for source in ["startup", "resume", "compact", "clear"] {
            let payload = fixture.payload("SessionStart", source);
            let output = fixture.hook(&serde_json::to_vec(&payload).unwrap());
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            assert!(output.stdout.len() <= 16 * 1024 + 1);
            let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(parsed.as_object().unwrap().len(), 1);
            let fields = parsed["hookSpecificOutput"].as_object().unwrap();
            assert_eq!(fields.len(), 2);
            assert_eq!(fields["hookEventName"], "SessionStart");
            let context = fields["additionalContext"].as_str().unwrap();
            assert!(context.contains("Preserve the existing root host conversation"));
            assert!(!context.contains("native session sentinel"));
            assert!(!context.contains("existing-root-sentinel"));
            if let Some(previous) = &previous {
                assert_eq!(&output.stdout, previous);
            }
            previous = Some(output.stdout);
        }
        assert_eq!(snapshot(&fixture.base), before);
        assert!(!fixture.host.join("launched").exists());
    }
}

#[test]
fn ordinary_turns_and_invalid_or_unconfigured_inputs_remain_silent() {
    let fixture = Fixture::new("portable");
    let before = snapshot(&fixture.base);
    let mut inputs = vec![
        Vec::new(),
        b"not json".to_vec(),
        b"{}".to_vec(),
        b"[]".to_vec(),
        vec![b'x'; 64 * 1024 + 1],
    ];
    for event in ["UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"] {
        inputs.push(serde_json::to_vec(&fixture.payload(event, "resume")).unwrap());
    }
    let mut unconfigured = fixture.payload("SessionStart", "startup");
    unconfigured["cwd"] = json!(fixture.host.clone());
    inputs.push(serde_json::to_vec(&unconfigured).unwrap());
    for input in inputs {
        let output = fixture.hook(&input);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    assert_eq!(snapshot(&fixture.base), before);
    fs::write(fixture.control.join("soulmate.json"), b"malformed config").unwrap();
    let malformed_before = snapshot(&fixture.base);
    let output =
        fixture.hook(&serde_json::to_vec(&fixture.payload("SessionStart", "resume")).unwrap());
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(snapshot(&fixture.base), malformed_before);
}

#[test]
fn session_update_context_is_fresh_cache_bound_and_subagent_silent() {
    let fixture = Fixture::new("portable");
    let cache = fixture.base.join("cache/soulmate");
    let next = "v0.20.0";
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("update.json"),
        format!(
            "{{\"checked_at\":{},\"channel\":\"stable\",\"latest\":\"{next}\"}}",
            chrono::Utc::now().timestamp(),
        ),
    )
    .unwrap();
    let configured = fixture.hook_with_update(
        &serde_json::to_vec(&fixture.payload("SessionStart", "startup")).unwrap(),
        true,
    );
    let configured_text: Value = serde_json::from_slice(&configured.stdout).unwrap();
    assert!(configured_text["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("Run `soulmate` update"));
    let configured_context = configured_text["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(configured_context.contains("once"));
    assert!(configured_context.contains("explicit authorization"));

    let mut unconfigured = fixture.payload("SessionStart", "startup");
    unconfigured["cwd"] = json!(fixture.host.clone());
    let unconfigured = fixture.hook_with_update(&serde_json::to_vec(&unconfigured).unwrap(), true);
    assert!(String::from_utf8_lossy(&unconfigured.stdout).contains("Run `soulmate` update"));

    let mut subagent = fixture.payload("SubagentStart", "startup");
    subagent["agent_name"] = json!("worker");
    let subagent = fixture.hook_with_update(&serde_json::to_vec(&subagent).unwrap(), true);
    assert!(!String::from_utf8_lossy(&subagent.stdout).contains("Run `soulmate` update"));
}

#[test]
fn fresh_no_update_cache_stays_silent_without_curl() {
    let fixture = Fixture::new("portable");
    let cache = fixture.base.join("cache/soulmate");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("update.json"),
        format!(
            "{{\"checked_at\":{},\"channel\":\"stable\"}}",
            chrono::Utc::now().timestamp()
        ),
    )
    .unwrap();
    let calls = fixture.host.join("fresh-curl-calls");
    let curl = fixture.host.join("commands/curl");
    fs::write(
        &curl,
        format!("#!/bin/sh\nprintf x >> '{}'\n", calls.display()),
    )
    .unwrap();
    fs::set_permissions(&curl, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let orphan = fixture.base.join("orphan");
    fs::create_dir(&orphan).unwrap();
    let mut payload = fixture.payload("SessionStart", "startup");
    payload["cwd"] = json!(orphan);
    let output = fixture.hook_with_update(&serde_json::to_vec(&payload).unwrap(), true);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!calls.exists());
}

#[test]
fn failed_session_lookup_backs_off_and_opt_out_stays_silent() {
    let fixture = Fixture::new("portable");
    let cache = fixture.base.join("cache/soulmate");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("update.json"),
        b"{\"checked_at\":0,\"channel\":\"stable\",\"latest\":\"v0.14.0\"}",
    )
    .unwrap();
    let calls = fixture.host.join("failed-curl-calls");
    let curl = fixture.host.join("commands/curl");
    fs::write(
        &curl,
        format!("#!/bin/sh\nprintf x >> '{}'\nexit 7\n", calls.display()),
    )
    .unwrap();
    fs::set_permissions(&curl, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let payload = fixture.payload("SessionStart", "startup");
    let failed = fixture.hook_with_update(&serde_json::to_vec(&payload).unwrap(), true);
    assert!(failed.status.success());
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("Run `soulmate` update"));
    let cache_text = fs::read_to_string(cache.join("update.json")).unwrap();
    assert!(cache_text.contains("backoff_until"));
    let calls_before = fs::read_to_string(&calls).unwrap();
    let backed_off = fixture.hook_with_update(&serde_json::to_vec(&payload).unwrap(), true);
    assert!(!String::from_utf8_lossy(&backed_off.stdout).contains("Run `soulmate` update"));
    assert_eq!(fs::read_to_string(&calls).unwrap(), calls_before);
    let opted_out = fixture.hook(&serde_json::to_vec(&payload).unwrap());
    assert!(!String::from_utf8_lossy(&opted_out.stdout).contains("Run `soulmate` update"));
}

#[test]
fn stale_session_cache_refreshes_with_local_fake_curl() {
    let fixture = Fixture::new("portable");
    let cache = fixture.base.join("cache/soulmate");
    let next = "v0.20.0";
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("update.json"),
        b"{\"checked_at\":0,\"channel\":\"stable\",\"latest\":\"v0.14.0\"}",
    )
    .unwrap();
    let calls = fixture.host.join("curl-calls");
    let curl = fixture.host.join("commands/curl");
    fs::write(
        &curl,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nout=\"\"; for arg in \"$@\"; do out=\"$arg\"; done\nprintf '%s' '[{{\"tag_name\":\"{next}\",\"draft\":false,\"prerelease\":false}}]' > \"$out\"\n",
            calls.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&curl, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let output = fixture.hook_with_update(
        &serde_json::to_vec(&fixture.payload("SessionStart", "startup")).unwrap(),
        true,
    );
    assert!(output.status.success());
    assert!(calls.is_file());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Run `soulmate` update"));
}
