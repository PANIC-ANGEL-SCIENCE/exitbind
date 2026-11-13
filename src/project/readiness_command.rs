// Observes simple script inputs named by a check command.
// Shell syntax remains unknown rather than interpreted; see `src/project/readiness.rs`.
//! Conservative observations of a check command's directly named script.
//!
//! Shell syntax and interpreter options are intentionally not guessed. This
//! supplies an early fact, not permission or a guarantee that a check passes.

use serde_json::{json, Value};
use std::path::Path;

pub(super) fn input(root: &Path, command: Option<&str>) -> Value {
    let Some(command) = command else {
        return json!({"state":"not-stated"});
    };
    if command
        .chars()
        .any(|c| c.is_control() || "'\"\\$`;&|()<>{}*?[]#~".contains(c))
    {
        return json!({"state":"unknown", "reason":"shell syntax requires the host's interpretation"});
    }
    let mut words = command.split_whitespace();
    let Some(program) = words.next() else {
        return json!({"state":"not-stated"});
    };
    let interpreter = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str());
    if !matches!(interpreter, Some("sh" | "bash" | "python" | "python3")) {
        return json!({"state":"not-applicable"});
    }
    let Some(script) = words.next().filter(|value| !value.starts_with('-')) else {
        return json!({"state":"unknown", "reason":"interpreter input is not a simple script path"});
    };
    let path = if Path::new(script).is_absolute() {
        Path::new(script).to_path_buf()
    } else {
        root.join(script)
    };
    let state = match std::fs::metadata(&path) {
        Ok(metadata) if !metadata.is_file() => "not-file",
        Ok(_) if std::fs::File::open(&path).is_ok() => "readable",
        Ok(_) => "unreadable",
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing",
        Err(_) => "unavailable",
    };
    json!({"state":state,"argument":script,"path":path.to_str(),
        "meaning":"current observation only; a task may intentionally create this check input"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_interpreter_inputs_are_observed_without_guessing_shell_syntax() {
        let root = crate::test_root::temp("checker-script");
        assert_eq!(input(&root, Some("sh ./missing.sh"))["state"], "missing");
        std::fs::write(root.join("check.sh"), "exit 0\n").unwrap();
        assert_eq!(input(&root, Some("sh check.sh"))["state"], "readable");
        assert_eq!(input(&root, Some("sh -c 'echo hello'"))["state"], "unknown");
        assert_eq!(input(&root, Some("cargo test"))["state"], "not-applicable");
    }
}
