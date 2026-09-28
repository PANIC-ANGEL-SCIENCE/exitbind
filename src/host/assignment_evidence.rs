//! Evidence read routes presented to a native child with its assignment.
//!
//! The child receives references it can resolve itself through `work expand`,
//! never evidence content or a Lead's summary of it. References come from the
//! same validated assignment context the packet digest covers, so a stale or
//! foreign reference is never offered as current evidence.

use serde_json::Value;

/// Entries presented at most; the full packet names the rest.
const MAX_ENTRIES: usize = 8;

pub(crate) fn lines(work: &str, context: &Value) -> Vec<String> {
    let caller = crate::compatibility::profile().caller;
    let expand = |reference: &Value| {
        reference["id"]
            .as_str()
            .filter(|id| {
                id.starts_with("ref:") && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_graphic())
            })
            .map(|id| format!("`{caller} work expand {work} {id}`"))
    };
    let entries = context["evidence"].as_array().cloned().unwrap_or_default();
    let mut lines = vec![format!(
        "Evidence for this assignment (current subject SHA-256 {}). Resolve each reference yourself; a summary from the Lead is not evidence:",
        context["subject"]["sha256"].as_str().unwrap_or("unavailable")
    )];
    let mut checks = 0usize;
    for entry in entries.iter().take(MAX_ENTRIES) {
        if entry["kind"] == "check" {
            checks += 1;
            let mut line = format!(
                "- check {} ({}), checker `{}`, event {}",
                text(&entry["status"]),
                text(&entry["freshness"]),
                text(&entry["checker"]),
                text(&entry["checkEventSha256"]),
            );
            if let Some(command) = expand(&entry["checkEventRef"]) {
                line.push_str(&format!(": record {command}"));
            }
            if let Some(command) = expand(&entry["checkLogRef"]) {
                line.push_str(&format!(", log {command}"));
            }
            lines.push(line);
        } else if let Some(command) = expand(&entry["rawEventRef"]) {
            lines.push(format!("- {} event: {command}", text(&entry["kind"])));
        }
    }
    if entries.len() > MAX_ENTRIES {
        lines.push(format!(
            "- {} more entries are in the full packet.",
            entries.len() - MAX_ENTRIES
        ));
    }
    if checks == 0 {
        lines.push("- No check record belongs to this assignment yet.".into());
    }
    lines.push(format!(
        "Full assignment packet: `{caller} work next {work} --full`. A passing check is evidence for your own verdict, not an approval."
    ));
    lines
}

/// One line of display text: printable, bounded, no line breaks.
fn text(value: &Value) -> String {
    let raw = value.as_str().unwrap_or("unavailable");
    let mut clean = raw
        .chars()
        .map(|c| if c.is_control() || c == '`' { ' ' } else { c })
        .collect::<String>();
    if clean.len() > 160 {
        let mut end = 157;
        while !clean.is_char_boundary(end) {
            end -= 1;
        }
        clean.truncate(end);
        clean.push_str("...");
    }
    clean
}
