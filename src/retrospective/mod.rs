//! Opt-in, read-only after-done retrospection over one repository and source.

pub mod analysis;
pub mod git;
pub mod output;
pub mod source;

use chrono::{DateTime, FixedOffset};
use std::path::Path;

pub fn inspect(
    repo: &Path,
    source_path: &Path,
    since: &str,
    until: &str,
) -> Result<serde_json::Value, String> {
    let since = parse_bound(since, "since")?;
    let until = parse_bound(until, "until")?;
    if since > until {
        return Err("retrospective --since must not be after --until".into());
    }
    let repository = git::inspect(repo)?;
    let source_data = source::read(source_path)?;
    Ok(analysis::inspect(&repository, &source_data, since, until))
}

pub fn expand(
    repo: &Path,
    source_path: &Path,
    reference: &str,
    since: &str,
    until: &str,
) -> Result<serde_json::Value, String> {
    let since = parse_bound(since, "since")?;
    let until = parse_bound(until, "until")?;
    if since > until {
        return Err("retrospective --since must not be after --until".into());
    }
    let repository = git::inspect(repo)?;
    let source_data = source::read(source_path)?;
    analysis::expand(&repository, &source_data, reference, since, until)
}

fn parse_bound(value: &str, name: &str) -> Result<DateTime<FixedOffset>, String> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|_| format!("retrospective --{name} must be RFC3339"))
}
