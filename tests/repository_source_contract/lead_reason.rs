//! Source classification of command forms taking a reason.
use super::cli_tables::option_commands;
use super::lead_reason_forms::{LEAD_ACTS, NOT_LEAD_ACTS};
use super::rust_source::production;

/// Every CLI command whose option check accepts `--reason`, read from the
/// CLI with the shared scanner (`cli_tables::option_commands`).
fn reason_forms() -> Vec<String> {
    let mut forms: Vec<String> = option_commands(&production(), "reason")
        .into_iter()
        .map(|(_, command)| command)
        .collect();
    forms.sort();
    forms.dedup();
    forms
}

#[test]
fn every_reason_form_is_classified_as_a_lead_act_or_other_command() {
    let mut classified: Vec<String> = LEAD_ACTS
        .iter()
        .chain(NOT_LEAD_ACTS.iter())
        .map(|form| (*form).to_owned())
        .collect();
    classified.sort();
    assert_eq!(
        reason_forms(),
        classified,
        "a command that takes --reason must be classified here"
    );
}
