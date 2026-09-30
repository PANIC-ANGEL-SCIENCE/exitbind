//! Human presentation is deliberately secondary to the structured result.

use serde_json::Value;

pub fn human(value: &Value) -> String {
    let status = value["status"].as_str().unwrap_or("failure");
    let findings = value["findings"].as_array().map_or(0, Vec::len);
    let interval = &value["interval"];
    let mut out = format!("After-done retrospective: {status}\n");
    out.push_str(&format!(
        "  interval: {} to {}\n",
        interval["since"].as_str().unwrap_or("?"),
        interval["until"].as_str().unwrap_or("?")
    ));
    out.push_str(&format!("  findings: {findings}\n"));
    for finding in value["findings"].as_array().into_iter().flatten() {
        let claim = finding["claim"]["scope"]
            .as_str()
            .unwrap_or("completion claim");
        let outcome = finding["outcome"]
            .as_str()
            .unwrap_or("insufficient_evidence");
        let counts = finding["activity"]["counts"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(key, value)| format!("{key}={}", value.as_u64().unwrap_or(0)))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push_str(&format!("  - {claim}: {outcome} ({counts})\n"));
        if let Some(evidence) = finding["evidence"].as_array() {
            let references = evidence
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("    evidence: {references}\n"));
        }
    }
    if status != "complete" {
        out.push_str("  coverage is limited; absence of a finding is not proof of closure.\n");
    }
    out.push_str(
        "  evidence is available only through opaque references and explicit expansion.\n",
    );
    out
}
