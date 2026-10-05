//! Deterministic bounded UTF-8 assertions; never evaluate source as a command.
use super::*;

pub(super) fn run(loaded: &Loaded) -> Result<Value, String> {
    let Some(current) = current(loaded)? else {
        return Ok(json!({"version": 1, "state": "absent", "outcome": "skipped", "checks": []}));
    };
    let mut sources = BTreeMap::new();
    let mut results = Vec::new();
    for check in &current.contract.checks {
        if !sources.contains_key(&check.path) {
            let bytes = read(loaded, &check.path, MAX_SOURCE_BYTES)?;
            let content = String::from_utf8(bytes)
                .map_err(|_| format!("architecture check source is not UTF-8: {}", check.path))?;
            sources.insert(check.path.clone(), content);
        }
        let content = &sources[&check.path];
        let contains = content.contains(&check.literal);
        let passed = match check.assertion {
            Assertion::Contains => contains,
            Assertion::Excludes => !contains,
        };
        results.push(json!({"id": check.id, "path": check.path,
            "sourceSha256": hash::text(content), "assertion": check.assertion,
            "passed": passed}));
    }
    assert_current(loaded)?;
    for (source, content) in &sources {
        if hash::bytes(&read(loaded, source, MAX_SOURCE_BYTES)?) != hash::text(content) {
            return Err(
                "architecture check inputs changed during observation; rerun the check".into(),
            );
        }
    }
    let passed = results.iter().all(|item| item["passed"] == true);
    Ok(
        json!({"version": 1, "state": "current", "provenance": current.provenance,
        "outcome": if passed { "passed" } else { "failed" }, "checks": results,
        "coverage": "exact named UTF-8 files and literal assertions; no inferred language dependency graph"}),
    )
}
