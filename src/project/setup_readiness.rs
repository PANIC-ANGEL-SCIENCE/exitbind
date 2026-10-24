//! Setup's environment-readiness preflight and preview section.
//!
//! The resolver itself lives in `project::readiness`; this module only wires
//! setup's facts to it, applies the one pre-write refusal on `--apply`, and
//! prints the same section `check` prints in the human preview.

use serde_json::Value;
use std::path::Path;

/// Read readiness before setup's first write. `--apply` adds the one readiness
/// refusal; a preview only states the facts.
pub(crate) fn preflight(
    root: &Path,
    state_root: &Path,
    check_command: Option<&str>,
    hosts: &[String],
    apply: bool,
) -> Result<Value, String> {
    let readiness = crate::project::readiness::assess(&crate::project::readiness::Facts {
        root,
        state_root,
        check_command,
        hosts,
    });
    if apply {
        if let Some(command) = check_command {
            if let Some(refusal) = crate::project::readiness::checker_refusal(root, command) {
                return Err(format!(
                    "setup refused before writing; no files changed: {refusal}"
                ));
            }
        }
    }
    Ok(readiness)
}

/// The human preview's readiness lines. A report without readiness (an older
/// shape) prints nothing.
pub(crate) fn print(report: &Value) {
    if report.is_object() && report["facts"].is_string() {
        for line in crate::project::readiness::text_lines(report) {
            println!("{line}");
        }
    }
}
