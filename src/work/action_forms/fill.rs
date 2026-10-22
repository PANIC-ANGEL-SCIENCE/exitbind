//! Who decides each current choice, and the fill route that runs a printed
//! form with its placeholders filled instead of edited by hand.
use super::*;
use std::collections::BTreeMap;

/// The fill route's name for the `beforeEditing` permit form.
pub(crate) const PERMIT_CHOICE: &str = "permit";
/// The fill route option that supplies a form's stdin result artifact.
pub(crate) const ARTIFACT_OPTION: &str = "--artifact-file";

pub(super) fn attach(loaded: &Loaded, work: &str, binding: &str, result: &mut Value) {
    if result["leadChoiceRequired"] == true {
        result["choiceActor"] = json!({"role": "lead", "agent": loaded.lead(),
            "meaning": "the Lead decides these choices and supplies every fill value"});
    }
    let mut fillable = false;
    if let Some(choices) = result.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            let label = choice["label"].as_str().unwrap_or_default().to_owned();
            fillable |= describe(choice, &label);
        }
    }
    if let Some(form) = result
        .get_mut("beforeEditing")
        .filter(|form| form.is_object())
    {
        fillable |= describe(form, PERMIT_CHOICE);
    }
    if fillable {
        result["fillRoute"] = command(
            loaded,
            vec![
                "work".into(),
                "fill".into(),
                work.into(),
                "--current-binding".into(),
                binding.into(),
            ],
        );
    }
}

fn printed(form: &Value) -> Vec<String> {
    form["command"]["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn describe(form: &mut Value, label: &str) -> bool {
    let options = crate::emitted::fill_flags(&printed(form));
    if options
        .as_object()
        .map_or(true, |options| options.is_empty())
    {
        return false;
    }
    let artifact = (form["stdin"]["required"] == true).then_some(ARTIFACT_OPTION);
    form["fill"] = json!({"choice": label, "options": options, "artifactOption": artifact});
    true
}

/// A current form filled from `values`, ready to run unchanged.
pub(crate) struct Bound {
    pub(crate) argv: Vec<String>,
    pub(crate) reads_artifact: bool,
}

/// Resolve `choice` among the current forms of `work` and fill it. The fill
/// route's binding must still be current, so a decision made on one view never
/// lands on another.
pub(crate) fn bound(
    loaded: &Loaded,
    work: &str,
    binding: &str,
    choice: &str,
    values: &BTreeMap<String, String>,
    artifact: bool,
) -> Result<Bound, String> {
    let next = crate::work::next_for(loaded, work, &crate::work::resolve(loaded, work)?)?;
    let current = next["current"]["binding"]
        .as_str()
        .ok_or("current action binding unavailable")?;
    if current != binding {
        let route = crate::emitted::command_line(
            &["work", "detail", work, "--view", "action"],
            Some(&loaded.path),
        );
        return Err(format!(
            "the action changed since binding {binding}; read {route} again"
        ));
    }
    let forms = full(loaded, work, &next, current);
    let form = if choice == PERMIT_CHOICE {
        Some(&forms["beforeEditing"]).filter(|form| form.is_object())
    } else {
        forms["choices"]
            .as_array()
            .and_then(|choices| choices.iter().find(|form| form["label"] == choice))
    };
    let form = form.ok_or_else(|| {
        let mut labels: Vec<&str> = forms["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|form| form["label"].as_str())
            .collect();
        if forms["beforeEditing"].is_object() {
            labels.push(PERMIT_CHOICE);
        }
        format!(
            "no current choice {choice:?}; current choices: {}",
            labels.join(", ")
        )
    })?;
    if form["command"]["sameExecutableRequired"] != false
        || form["command"]["sameConfigRequired"] != false
    {
        return Err("the bound form does not carry its executable and configuration".into());
    }
    let reads_artifact = form["stdin"]["required"] == true;
    match (reads_artifact, artifact) {
        (true, false) => {
            return Err(format!(
                "choice {choice:?} reads its result artifact; pass {ARTIFACT_OPTION} PATH"
            ))
        }
        (false, true) => {
            return Err(format!(
                "choice {choice:?} reads no result artifact; omit {ARTIFACT_OPTION}"
            ))
        }
        _ => {}
    }
    Ok(Bound {
        argv: crate::emitted::fill(&printed(form), values)?,
        reads_artifact,
    })
}
