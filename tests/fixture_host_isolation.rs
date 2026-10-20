//! Fixture children must not inherit the invoking host's Goal identity.
mod support;

use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

#[cfg(unix)]
fn protect_fixture_path(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

const HOST_CONTEXT: &[&str] = &[
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "CLAUDE_CODE_SESSION_ID",
    "EXITBIND_NATIVE_SESSION_ID",
    "EXITBIND_SESSION_GOALS_DIR",
    "CLAUDE_ENV_FILE",
    "XDG_STATE_HOME",
];

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = support::temp("fixture-host-isolation");
        #[cfg(unix)]
        protect_fixture_path(&path);
        Self(path)
    }

    fn config(&self) -> PathBuf {
        self.0.join("exitbind.json")
    }

    fn command(&self) -> Command {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&self.0);
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("fixture command JSON")
}

#[test]
fn isolate_removes_seeded_context_and_explicit_fixture_context_still_works() {
    let fixture = Fixture::new();
    let mut init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    init.args(["init", "--mode", "portable", "--root"])
        .arg(&fixture.0);
    let initialized = init.output().unwrap();
    assert!(initialized.status.success(), "{initialized:?}");

    // Confirm the shared constructor removes every selected context name while
    // retaining ordinary credential roots that fixture hosts may need.
    let mut probe = support::git_topology::command("/usr/bin/env");
    for name in HOST_CONTEXT {
        probe.env(name, format!("controlled-{name}"));
    }
    probe.env("HOME", "/controlled-fixture-home");
    probe.env("CODEX_HOME", "/controlled-fixture-codex-home");
    support::git_topology::isolate(&mut probe);
    let probe = probe.output().unwrap();
    assert!(probe.status.success(), "{probe:?}");
    let environment = String::from_utf8(probe.stdout).unwrap();
    for name in HOST_CONTEXT {
        assert!(
            !environment
                .lines()
                .any(|line| line.starts_with(&format!("{name}="))),
            "{name} survived fixture isolation"
        );
    }
    assert!(environment
        .lines()
        .any(|line| line == "HOME=/controlled-fixture-home"));
    assert!(environment
        .lines()
        .any(|line| line == "CODEX_HOME=/controlled-fixture-codex-home"));

    let identity = "r44-controlled-fixture-session";
    let store = fixture.0.join("private-session-goals");
    fs::create_dir_all(&store).unwrap();
    #[cfg(unix)]
    protect_fixture_path(&store);
    let title = "Synthetic session Goal for fixture isolation";
    let mut incorporate = fixture.command();
    incorporate
        .args([
            "goal",
            "incorporate",
            "--goal-id",
            "fixture",
            "--goal",
            title,
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
            "--config",
        ])
        .arg(fixture.config())
        .env("CODEX_THREAD_ID", identity)
        .env("EXITBIND_SESSION_GOALS_DIR", &store);
    let incorporated = incorporate.output().unwrap();
    assert!(incorporated.status.success(), "{incorporated:?}");

    // Seed a controlled private Goal context before isolation, matching the
    // inherited-host failure mode without using any real session identifier.
    let mut isolated = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    isolated
        .current_dir(&fixture.0)
        .args(["goal", "status", "--json", "--config"])
        .arg(fixture.config());
    for name in HOST_CONTEXT {
        isolated.env(name, format!("controlled-{name}"));
    }
    isolated.env("CODEX_THREAD_ID", identity);
    isolated.env("EXITBIND_SESSION_GOALS_DIR", &store);
    support::git_topology::isolate(&mut isolated);
    let isolated = json(isolated.output().unwrap());
    assert!(
        !isolated.to_string().contains(title),
        "fixture associated an ambient Goal"
    );
    assert_eq!(
        isolated["goalProgress"]["decomposition"]["reason"],
        "overall_goal_unavailable"
    );

    // A test that deliberately re-adds only its fake identity after isolate
    // still exercises the intended native session Goal path.
    let mut explicit = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    explicit
        .current_dir(&fixture.0)
        .args(["goal", "status", "--json", "--config"])
        .arg(fixture.config());
    support::git_topology::isolate(&mut explicit);
    explicit
        .env("CODEX_THREAD_ID", identity)
        .env("EXITBIND_SESSION_GOALS_DIR", &store);
    let explicit = json(explicit.output().unwrap());
    assert_eq!(explicit["goal"], title);
    assert_eq!(explicit["goalId"], "fixture");
}

#[test]
fn official_ci_launcher_clears_all_host_goal_selectors_for_raw_builders() {
    let host = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        host => panic!("unsupported native CI test host: {host:?}"),
    };

    let fixture = Fixture::new();
    let bin = fixture.0.join("bin");
    fs::create_dir(&bin).unwrap();
    let fake_cargo = bin.join("cargo");
    let fake_rustc = bin.join("rustc");
    let fake_tmux = bin.join("tmux");
    let cargo_capture = fixture.0.join("cargo-argv.txt");
    let rustc_capture = fixture.0.join("rustc-environment.txt");
    let tmux_capture = fixture.0.join("tmux-invoked.txt");
    let target_root = fixture.0.join("target-root");
    let temp_root = fixture.0.join("temp-root");
    let log_root = fixture.0.join("log-root");
    let home = fixture.0.join("home");
    let codex_home = fixture.0.join("codex-home");

    let cargo_script = r##"#!/bin/sh
{
  printf 'ARGS'
  for arg do printf '\t%s' "$arg"; done
  printf '\n'
} >> "$FAKE_CARGO_CAPTURE"
if [ "$#" -eq 1 ] && [ "$1" = "--version" ]; then
  printf 'cargo 1.75.0 (controlled fixture)\n'
  exit 0
fi
printf 'unexpected fake cargo invocation' >&2
exit 91
"##;
    let rustc_script = r##"#!/bin/sh
{
  printf 'ARGS'
  for arg do printf '\t%s' "$arg"; done
  printf '\n'
  printf 'CODEX_THREAD_ID=%s\n' "${CODEX_THREAD_ID-__UNSET__}"
  printf 'CODEX_SESSION_ID=%s\n' "${CODEX_SESSION_ID-__UNSET__}"
  printf 'CLAUDE_CODE_SESSION_ID=%s\n' "${CLAUDE_CODE_SESSION_ID-__UNSET__}"
  printf 'EXITBIND_NATIVE_SESSION_ID=%s\n' "${EXITBIND_NATIVE_SESSION_ID-__UNSET__}"
  printf 'EXITBIND_SESSION_GOALS_DIR=%s\n' "${EXITBIND_SESSION_GOALS_DIR-__UNSET__}"
  printf 'CLAUDE_ENV_FILE=%s\n' "${CLAUDE_ENV_FILE-__UNSET__}"
  printf 'XDG_STATE_HOME=%s\n' "${XDG_STATE_HOME-__UNSET__}"
  printf 'HOME=%s\n' "${HOME-__UNSET__}"
  printf 'CODEX_HOME=%s\n' "${CODEX_HOME-__UNSET__}"
  printf '%s\n' '---'
} >> "$FAKE_RUSTC_CAPTURE"
if [ "$#" -eq 1 ] && [ "$1" = "-vV" ]; then
  printf 'rustc 1.75.0 (controlled fixture)\nhost: %s\nrelease: 1.75.0\n' "$FAKE_HOST_TRIPLE"
  exit 0
fi
if [ "$#" -eq 1 ] && [ "$1" = "--version" ]; then
  printf 'rustc 1.75.0 (controlled fixture)\n'
  exit 0
fi
printf 'unexpected fake rustc invocation' >&2
exit 92
"##;
    fs::write(&fake_cargo, cargo_script).unwrap();
    fs::write(&fake_rustc, rustc_script).unwrap();
    fs::write(
        &fake_tmux,
        "#!/bin/sh\nprintf 'unexpected fake tmux invocation\\n' >> \"$FAKE_TMUX_CAPTURE\"\nexit 93\n",
    )
    .unwrap();
    #[cfg(unix)]
    for executable in [&fake_cargo, &fake_rustc, &fake_tmux] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut launcher = support::git_topology::command("/bin/sh");
    launcher
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-local.sh"))
        .arg("--preflight")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_clear()
        .env("CARGO", &fake_cargo)
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("CARGO_TARGET_DIR", &target_root)
        .env("TMPDIR", &temp_root)
        .env("CI_LOG_ROOT", &log_root)
        .env("FAKE_CARGO_CAPTURE", &cargo_capture)
        .env("FAKE_RUSTC_CAPTURE", &rustc_capture)
        .env("FAKE_TMUX_CAPTURE", &tmux_capture)
        .env("FAKE_HOST_TRIPLE", host)
        .env("CARGO_BUILD_JOBS", "1");
    for name in HOST_CONTEXT {
        launcher.env(name, format!("controlled-{name}"));
    }
    let output = launcher.output().unwrap();
    assert!(
        output.status.success(),
        "ci-local preflight failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("preflight output is UTF-8");
    assert!(stdout.contains("preflight=ready\n"), "{stdout}");
    assert!(
        !tmux_capture.exists(),
        "preflight must discover tmux without executing it"
    );
    assert!(
        stdout.contains("cargo 1.75.0 (controlled fixture)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("rustc 1.75.0 (controlled fixture)"),
        "{stdout}"
    );
    assert!(
        target_root.is_dir(),
        "preflight did not prepare its target root"
    );
    assert!(
        temp_root.is_dir(),
        "preflight did not prepare its temp root"
    );
    assert!(log_root.is_dir(), "preflight did not prepare its log root");
    assert!(
        !target_root.join(".ci-local-lock").exists(),
        "preflight must not acquire the build lock"
    );

    let cargo_calls = fs::read_to_string(&cargo_capture).expect("fake cargo invocation record");
    assert_eq!(cargo_calls.lines().collect::<Vec<_>>(), ["ARGS\t--version"]);
    let rustc_calls = fs::read_to_string(&rustc_capture).expect("fake rustc environment record");
    assert_eq!(rustc_calls.matches("ARGS\t").count(), 2, "{rustc_calls}");
    assert!(rustc_calls.contains("ARGS\t-vV\n"), "{rustc_calls}");
    assert!(rustc_calls.contains("ARGS\t--version\n"), "{rustc_calls}");
    for name in HOST_CONTEXT {
        assert_eq!(
            rustc_calls.matches(&format!("{name}=__UNSET__\n")).count(),
            2,
            "{name} reached a raw preflight child: {rustc_calls}"
        );
    }
    assert_eq!(
        rustc_calls
            .matches(&format!("HOME={}\n", home.display()))
            .count(),
        2,
        "HOME must remain available to preflight children"
    );
    assert_eq!(
        rustc_calls
            .matches(&format!("CODEX_HOME={}\n", codex_home.display()))
            .count(),
        2,
        "CODEX_HOME must remain available to preflight children"
    );
}

/// Every process a test spawns, including the exitbind binary, is built by
/// `support::git_topology::command`, which removes the host session variables.
/// A spawn that bypassed it would inherit the invoking session's Goal identity
/// (the R52 incident wrote 32 test Works into a real Lead session Goal).
#[test]
fn every_test_spawn_goes_through_the_isolating_helper() {
    let helper = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    let removed: Vec<_> = helper
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    for name in HOST_CONTEXT {
        assert!(
            removed.iter().any(|item| item == name),
            "{name} would be inherited"
        );
    }
    let mut offenders = Vec::new();
    let mut pending = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && !path.ends_with("support/git_topology.rs")
            {
                let text = fs::read_to_string(&path).unwrap();
                let lines: Vec<&str> = text.lines().collect();
                for (index, line) in lines.iter().enumerate() {
                    // A deliberate raw probe names its reason on the line above.
                    let excepted = index > 0 && lines[index - 1].contains("raw-spawn:");
                    if line.contains(concat!("Command", "::new(")) && !excepted {
                        offenders.push(format!("{}:{}", path.display(), index + 1));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "spawn through support::git_topology::command so host session variables are removed: {offenders:?}"
    );
}
