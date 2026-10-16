//! Bounded component names for a pre-return binding refusal.
//! Exact saved values remain available through the read-only attempt route.
use crate::config::Loaded;
use serde_json::Value;

pub(super) fn mismatch(
    loaded: &Loaded,
    saved: &Value,
    current: &Value,
    state: &Value,
    attempt_id: &str,
) -> String {
    let mut differences = differences(&saved["binding"], current);
    if let Err(error) = super::assert_current_configuration(loaded, state) {
        let owning_component = category(&error);
        if !differences.contains(&owning_component) {
            differences.insert(0, owning_component);
        }
    }
    let route = super::read_route(
        loaded,
        saved["binding"]["selector"].as_str().unwrap_or(""),
        attempt_id,
        None,
    );
    format!(
        "pre-return attempt no longer matches the current result binding; changed components: {}; inspect the saved attempt with {}; no return was recorded",
        component_names(&differences),
        serde_json::to_string(&route["argv"]).unwrap_or_else(|_| "unavailable".into())
    )
}

pub(super) fn readback(loaded: &Loaded, attempt_id: Option<&str>, attempt: &Value) -> String {
    let id = attempt_id.unwrap_or_else(|| attempt["attemptId"].as_str().unwrap_or(""));
    let route = super::read_route(
        loaded,
        attempt["binding"]["selector"].as_str().unwrap_or(""),
        id,
        None,
    );
    serde_json::to_string(&route["argv"]).unwrap_or_else(|_| "unavailable".into())
}

pub(super) fn category(error: &str) -> &'static str {
    if error.contains("configuration") || error.contains("architecture") {
        "config"
    } else if error.contains("checker") {
        "checker"
    } else if error.contains("input") {
        "inputs"
    } else {
        "checker_or_inputs_unavailable"
    }
}

fn differences(saved: &Value, current: &Value) -> Vec<&'static str> {
    [
        ("work", "work"),
        ("ledger", "ledger"),
        ("runId", "run"),
        ("stage", "assignment"),
        ("attempt", "assignment"),
        ("agent", "assignment"),
        ("role", "assignment"),
        ("assignmentSha256", "assignment"),
        ("resultSha256", "artifact"),
        ("inputsSha256", "inputs"),
        ("subjectSha256", "subject"),
        ("configSha256", "config"),
        ("policySha256", "policy"),
        ("grantSetSha256", "grant_set"),
        ("checkerExecutableSha256", "checker"),
        ("producer", "producer"),
    ]
    .into_iter()
    .filter_map(|(field, category)| (saved.get(field) != current.get(field)).then_some(category))
    .fold(Vec::new(), |mut result, category| {
        if !result.contains(&category) {
            result.push(category);
        }
        result
    })
}

fn component_names(differences: &[&str]) -> String {
    if differences.is_empty() {
        "unavailable".into()
    } else {
        differences.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::{component_names, differences};
    use serde_json::json;

    #[test]
    fn bounded_component_names_cover_saved_binding_fields_and_unavailable() {
        for (field, category) in [
            ("stage", "assignment"),
            ("grantSetSha256", "grant_set"),
            ("resultSha256", "artifact"),
            ("configSha256", "config"),
            ("checkerExecutableSha256", "checker"),
            ("inputsSha256", "inputs"),
        ] {
            let saved = json!({"binding":{(field):"saved"}});
            let current = json!({(field):"current"});
            assert_eq!(differences(&saved["binding"], &current), vec![category]);
        }
        assert_eq!(component_names(&[]), "unavailable");
    }
}
