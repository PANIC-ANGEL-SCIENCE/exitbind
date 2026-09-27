//! Read-only accepted result reference evidence for historical and current goal projections.

use super::*;

/// Recorded facts for a result reference.  `inputs_current` is deliberately
/// separate from the accepted and artifact checks: an accepted result keeps
/// its recorded input identity even when the product has changed since it ran.
pub(crate) struct ResultRefEvidence {
    pub(crate) accepted: bool,
    pub(crate) artifact_current: bool,
    pub(crate) drift: Option<Value>,
    pub(crate) inputs_current: Option<bool>,
}

/// Locate and validate the exact accepted result for a reference while
/// retaining the recorded and current tested-input identities. Callers decide
/// whether input drift is a warning or part of a stricter currentness query.
pub(crate) fn result_ref_evidence(
    loaded: &Loaded,
    reference: &str,
) -> Result<Option<ResultRefEvidence>, String> {
    let runs = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("runs");
    let entries = match fs::read_dir(runs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let requested_name = reference
        .strip_prefix("smw_")
        .map(|token| format!("work-{token}.jsonl"));
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("run ledger path is not valid UTF-8")?;
        if let Some(requested_name) = &requested_name {
            if name != requested_name {
                continue;
            }
        }
        let relative = format!(
            "{}/runs/{name}",
            crate::project::layout_types::state_namespace()
        );
        let (_, events, _) = match load(loaded, &relative) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(last) = events.last() else { continue };
        if requested_name.is_none() && last["eventSha256"].as_str() != Some(reference) {
            continue;
        }
        let state = run_state::reduce(&events)?;
        if let Some(reference) = state.get("harnessReceipt") {
            crate::evidence::receipt::assert_exact_reference(loaded, reference)?;
        }
        let inputs_current = if state["version"].as_u64() >= Some(6) {
            let expected = state["inputsSha256"]
                .as_str()
                .ok_or("accepted run is missing its tested-input hash")?;
            Some(crate::run::inputs::fingerprint(loaded)? == expected)
        } else {
            None
        };
        let drift = match assert_no_drift(loaded, &state) {
            Ok(()) => None,
            Err(error) => drift_value(&error).map(Some).ok_or(error)?,
        };
        let artifact_current = artifact_current(loaded, &state)?;
        return Ok(Some(ResultRefEvidence {
            accepted: state["status"] == "accepted"
                && last["action"] == "submit"
                && last["role"] == "lead"
                && last["outcome"] == "accepted",
            artifact_current,
            drift,
            inputs_current,
        }));
    }
    Ok(None)
}
