//! Human-readable rendering of the redacted, validated run report.

use serde_json::Value;

pub(crate) fn markdown(report: &Value) -> String {
    let groups = &report["groups"];
    format!(
        "Value proof report v{}\n\nRuns: {}\n\nLocal reports: {} runs, {} checks, {} protections\nSynthetic scenarios: {} runs, {} checks, {} protections\nUnclassified runs: {} runs, {} checks, {} protections\n\nSynthetic incident frequency, user-confirmed avoided loss, and human time are unknown.\nGoals, commands, prompts, profiles, paths, and artifact content are omitted. Hashes are not anonymization.\n",
        report["version"],
        report["runs"],
        groups["local_report"]["runs"],
        groups["local_report"]["checks"],
        groups["local_report"]["protections"],
        groups["synthetic"]["runs"],
        groups["synthetic"]["checks"],
        groups["synthetic"]["protections"],
        groups["unclassified"]["runs"],
        groups["unclassified"]["checks"],
        groups["unclassified"]["protections"],
    )
}
