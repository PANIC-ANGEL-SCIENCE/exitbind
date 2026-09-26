//! Read-only CLI presentation of already validated run packets.

use serde_json::Value;

pub(crate) fn shell_quote(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    format!("'{}'", escaped.replace('\'', "'\\''"))
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn printf_encoded(value: &str) -> String {
    use std::fmt::Write;

    let mut encoded = String::new();
    for byte in value.as_bytes() {
        write!(&mut encoded, "\\0{byte:03o}").expect("writing to String cannot fail");
    }
    encoded
}

fn exact_assignment(name: &str, value: &str) -> String {
    let encoded = printf_encoded(value);
    format!("{name}=$(printf '%bX' '{encoded}'); {name}=${{{name}%X}}")
}

pub(crate) fn read_command(action: &str, config: &str, ledger: &str) -> String {
    let command = if crate::producer::exitbind_surface() {
        "exitbind"
    } else {
        "soulmate"
    };
    let format = if action == "next" { " --text" } else { "" };
    if has_control(config) || has_control(ledger) {
        return format!(
            "{}; {}; {command} run {action}{format} --config=\"$__sm_config\" -- \"$__sm_ledger\"",
            exact_assignment("__sm_config", config),
            exact_assignment("__sm_ledger", ledger),
        );
    }
    format!(
        "{command} run {action}{format} --config={} -- {}",
        shell_quote(config),
        shell_quote(ledger)
    )
}

pub(crate) fn print_next(value: &Value) -> Result<(), String> {
    if value["warnings"]
        .as_array()
        .is_some_and(|warnings| !warnings.is_empty())
    {
        eprintln!(
            "warning: run drift detected after start; continuing with recorded assignments. Inspect the run before changing or replacing recorded evidence."
        );
    }
    let assignments = value["assignments"]
        .as_array()
        .ok_or("next packet must contain an assignments array")?;
    println!(
        "Run {}: {}\nWorkflow: {}",
        value["runId"], value["status"], value["workflow"]
    );
    if assignments.is_empty() {
        println!("No pending assignments.");
        return Ok(());
    }
    println!("Pending assignments: {}", assignments.len());
    for assignment in assignments {
        // JSON string escaping keeps embedded control characters visibly inert.
        println!(
            "\nAgent: {} (role {})\nStage: {} / Attempt: {}\nGoal: {}\nPurpose: {}\nDisplay name: {}\nNative task name: {}\nProfile: {}\nRequested runtime: {}\nDeclared boundary: {}\nResult path: {}\nResult root: {}",
            assignment["agent"], assignment["role"],
            assignment["stage"], assignment["attempt"],
            assignment["goal"], assignment["purpose"],
            assignment["displayName"], assignment["nativeTaskName"],
            assignment["profile"], assignment["runtime"], assignment["declaredBoundary"],
            assignment["artifactPathHint"], assignment["artifactRootHint"]
        );
        if let Some(policy) = assignment.get("checkPolicy") {
            println!(
                "Checked run: caller-reported result required for each current worker submission.\nFrozen check command: {}\nCheck command SHA-256: {}\nCheck origin: {}",
                policy["command"], policy["commandSha256"], policy["origin"]
            );
        } else {
            println!("Unchecked run: no check-result requirement is configured.");
        }
        println!("Upstream artifacts (immutable; keep the exact recorded bytes):");
        if let Some(upstream) = assignment["upstreamArtifacts"].as_array() {
            if upstream.is_empty() {
                println!("  none");
            }
            for artifact in upstream {
                println!(
                    "  {} attempt {} / stage {}: agent={} role={} root={} path={} sha256={}",
                    artifact["attemptStatus"],
                    artifact["attempt"],
                    artifact["stage"],
                    artifact["agent"],
                    artifact["role"],
                    artifact["root"],
                    artifact["path"],
                    artifact["sha256"]
                );
            }
        }
        if let Some(references) = assignment.get("memoryReferences") {
            println!("Role-scoped memory references (no contents): {references}");
        }
        if let Some(receipt) = assignment.get("harnessReceipt") {
            println!("Harness receipt reference: {receipt}");
        }
    }
    println!("\nThe host performs the assignment and executes checks; this command only reads the validated run context.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::{
        env, fs,
        os::unix::fs::PermissionsExt,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    fn hex(value: &[u8]) -> String {
        use std::fmt::Write;

        let mut output = String::new();
        for byte in value {
            write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    }

    #[test]
    fn missing_assignments_fail_at_the_presentation_boundary() {
        assert!(super::print_next(&serde_json::json!({"valid": true})).is_err());
    }

    #[test]
    fn shell_quote_renders_control_characters_as_data() {
        let command = crate::compatibility::profile().caller;
        let quoted = super::shell_quote("line\n\u{1b}[31m");
        assert!(!quoted.bytes().any(|byte| byte < 0x20 || byte == 0x7f));
        assert!(quoted.contains("\\n"));
        assert!(quoted.contains("\\u{1b}"));
        assert_eq!(
            super::read_command("inspect", "config.json", "ledger.json"),
            format!("{command} run inspect --config='config.json' -- 'ledger.json'")
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_command_preserves_control_bytes_under_posix_sh() {
        let command = crate::compatibility::profile().caller;
        let root = env::temp_dir().join(format!(
            "{command}-presentation-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("temporary directory should be created");
        let binary = root.join(command);
        fs::write(
            &binary,
            "#!/bin/sh\nprintf '%s\\n' \"$#\" > \"$SM_OUTPUT\"\nfor arg in \"$@\"; do\n  printf '%s' \"$arg\" | od -An -tx1 -v | tr -d '[:space:]' >> \"$SM_OUTPUT\"\n  printf '\\n' >> \"$SM_OUTPUT\"\ndone\n",
        )
        .expect("fake executable should be written");
        let mut permissions = fs::metadata(&binary)
            .expect("fake executable metadata should be readable")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&binary, permissions).expect("fake executable should be executable");

        let output = root.join("argv.hex");
        let config = "config\nname\t\u{1b}'\\$(touch SHOULD_NOT_EXIST)\n";
        let ledger = "ledger\n";
        let command = super::read_command("inspect", config, ledger);
        assert!(!command.bytes().any(|byte| byte < 0x20 || byte == 0x7f));
        let existing_path = env::var_os("PATH").unwrap_or_default();
        let path =
            env::join_paths(std::iter::once(root.clone()).chain(env::split_paths(&existing_path)))
                .expect("fixture root and existing PATH should form a valid PATH");
        let result = Command::new("/bin/sh")
            .current_dir(&root)
            .args(["-c", &command])
            .env("PATH", path)
            .env("SM_OUTPUT", &output)
            .output()
            .expect("POSIX shell should start");
        assert!(
            result.status.success(),
            "shell failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );

        let lines = fs::read_to_string(&output)
            .expect("argv output should be readable")
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut config_arg = b"--config=".to_vec();
        config_arg.extend_from_slice(config.as_bytes());
        assert_eq!(
            lines,
            vec![
                "5".to_owned(),
                hex(b"run"),
                hex(b"inspect"),
                hex(&config_arg),
                hex(b"--"),
                hex(ledger.as_bytes()),
            ]
        );
        assert!(!root.join("SHOULD_NOT_EXIST").exists());
        fs::remove_dir_all(root).expect("temporary directory should be removed");
    }
}
