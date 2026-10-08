//! Work locator syntax shared by run naming, resume, and focus owners.

pub(super) const WORK_PREFIX: &str = "smw_";

pub(super) fn runs_dir() -> String {
    format!("{}/runs", crate::project::layout_types::state_namespace())
}

pub(crate) fn locator_for_ledger(relative: &str) -> Option<String> {
    let prefix = format!("{}/work-", runs_dir());
    let locator_suffix = relative.strip_prefix(&prefix)?.strip_suffix(".jsonl")?;
    valid_token(locator_suffix).then(|| format!("{WORK_PREFIX}{locator_suffix}"))
}

pub(super) fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}
