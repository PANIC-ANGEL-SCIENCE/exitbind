#[cfg(unix)]
mod support;

#[cfg(unix)]
mod unix {
    use super::support;

    use std::fs;
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};
    use std::path::Path;
    use std::process::{Command, Output};

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn invoke(root: &Path, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(root).args(args);
        unsafe {
            command.pre_exec(|| {
                libc::umask(0o002);
                Ok(())
            });
        }
        command.output().unwrap()
    }

    #[test]
    fn init_keeps_state_private_under_permissive_umask() {
        let root = support::temp("state-permissions");
        let output = invoke(
            &root,
            &["init", "--mode", "portable", "--skip-skills", "--root", "."],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        for directory in [
            ".exitbind",
            ".exitbind/runs",
            ".exitbind/memory",
            ".exitbind/artifacts",
            ".exitbind/receipts",
            ".exitbind/away",
            ".exitbind/locks",
        ] {
            assert_eq!(mode(&root.join(directory)), 0o700, "{directory}");
        }
        assert_eq!(mode(&root.join(".exitbind/.gitignore")), 0o600);

        assert_eq!(mode(&root.join("exitbind")), 0o775);
        assert_eq!(mode(&root.join("exitbind/agents")), 0o775);
        assert_eq!(mode(&root.join("exitbind.json")), 0o664);
        assert_eq!(mode(&root.join("exitbind/agents/worker.md")), 0o664);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn captured_raw_logs_are_private_under_permissive_umask() {
        let root = support::temp("capture-permissions");
        let init = invoke(&root, &["init", "--mode", "portable", "--root", "."]);
        assert!(
            init.status.success(),
            "{}",
            String::from_utf8_lossy(&init.stderr)
        );
        let ledger = ".exitbind/runs/permissions.jsonl";
        let started = invoke(
            &root,
            &[
                "run",
                "start",
                "change",
                "--goal",
                "observe",
                "--ledger",
                ledger,
                "--check-command",
                "printf stdout; printf stderr >&2",
                "--proof-origin",
                "local_report",
                "--config",
                "exitbind.json",
            ],
        );
        assert!(
            started.status.success(),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        fs::write(root.join(".exitbind/artifacts/worker.md"), b"worker\n").unwrap();
        let lead = invoke(
            &root,
            &[
                "run",
                "submit",
                "lead",
                ledger,
                "--outcome",
                "scoped",
                "--artifact",
                ".exitbind/artifacts/worker.md",
                "--artifact-root",
                "state",
                "--config",
                "exitbind.json",
            ],
        );
        assert!(
            lead.status.success(),
            "{}",
            String::from_utf8_lossy(&lead.stderr)
        );
        let worker = invoke(
            &root,
            &[
                "run",
                "submit",
                "worker",
                ledger,
                "--outcome",
                "completed",
                "--artifact",
                ".exitbind/artifacts/worker.md",
                "--artifact-root",
                "state",
                "--config",
                "exitbind.json",
            ],
        );
        assert!(
            worker.status.success(),
            "{}",
            String::from_utf8_lossy(&worker.stderr)
        );
        let event: serde_json::Value = serde_json::from_slice(&worker.stdout).unwrap();
        let target = event["event"]["eventSha256"].as_str().unwrap();
        let observed = invoke(
            &root,
            &[
                "run",
                "observe-check",
                ledger,
                "--target",
                target,
                "--config",
                "exitbind.json",
            ],
        );
        assert!(
            observed.status.success(),
            "{}",
            String::from_utf8_lossy(&observed.stderr)
        );
        let mut raw_logs = 0;
        for entry in fs::read_dir(root.join(".exitbind/artifacts")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) == Some("raw") {
                raw_logs += 1;
                assert_eq!(mode(&path), 0o600, "{}", path.display());
            }
        }
        assert_eq!(raw_logs, 2);
        fs::remove_dir_all(root).unwrap();
    }
}
