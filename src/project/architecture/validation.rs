//! Closed, bounded contract shape and cohesive responsibility ownership.
use super::*;

fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096 && !value.contains('\0')
}

fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

fn relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value == value.trim()
        && !value
            .chars()
            .any(|c| c.is_control() || "\\*?[]:".contains(c))
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

pub(super) fn selection(selected: &Selection) -> Result<(), String> {
    if !relative(&selected.source_path)
        || !text(&selected.revision)
        || selected.source_sha256.len() != 64
        || !selected
            .source_sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("require normalized project-relative sourcePath, lowercase SHA256 and nonempty revision".into());
    }
    Ok(())
}

fn covers(parent: &str, child: &str) -> bool {
    parent == child
        || child
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(super) fn intersects(path: &str, scope: &str) -> bool {
    if matches!(scope, "." | "**") {
        return true;
    }
    let scope = scope.strip_suffix("/**").unwrap_or(scope);
    relative(scope) && (covers(path, scope) || covers(scope, path))
}

pub(super) fn contract(contract: &Contract) -> Result<(), String> {
    if contract.version != 1
        || !text(&contract.revision)
        || contract.responsibilities.is_empty()
        || contract.responsibilities.len() > 64
        || contract.interfaces.len() > 64
        || contract.checks.len() > 64
        || contract.dependencies.len() > 128
    {
        return Err("invalid architecture contract version, revision or collection bound".into());
    }
    let mut owners = BTreeMap::new();
    let mut paths: Vec<(&str, &str)> = Vec::new();
    for item in &contract.responsibilities {
        if !id(&item.id)
            || !text(&item.summary)
            || owners.insert(item.id.as_str(), item).is_some()
            || item.paths.is_empty()
            || item.paths.len() > 64
        {
            return Err("invalid or duplicate architecture responsibility".into());
        }
        for path in &item.paths {
            if !relative(path)
                || paths
                    .iter()
                    .any(|(_, prior)| covers(prior, path) || covers(path, prior))
            {
                return Err(
                    "architecture responsibility paths must be normalized and nonoverlapping"
                        .into(),
                );
            }
            paths.push((&item.id, path));
        }
    }
    let owned = |owner: &str, path: &str| {
        relative(path)
            && paths
                .iter()
                .any(|(id, root)| *id == owner && covers(root, path))
    };
    let mut interfaces = BTreeMap::new();
    for item in &contract.interfaces {
        if !id(&item.id)
            || !text(&item.summary)
            || !owned(&item.owner, &item.path)
            || interfaces.insert(item.id.as_str(), item).is_some()
        {
            return Err("invalid architecture interface or responsibility ownership".into());
        }
    }
    let mut edges = BTreeSet::new();
    for edge in &contract.dependencies {
        if edge.from == edge.to
            || !owners.contains_key(edge.from.as_str())
            || !owners.contains_key(edge.to.as_str())
            || !edges.insert((&edge.from, &edge.to))
            || edge.interface.as_ref().is_some_and(|id| {
                interfaces.get(id.as_str()).map_or(true, |item| {
                    item.owner != edge.from && item.owner != edge.to
                })
            })
        {
            return Err("invalid, duplicate or contradictory architecture dependency".into());
        }
    }
    let mut checks = BTreeSet::new();
    for check in &contract.checks {
        if !id(&check.id)
            || !checks.insert(&check.id)
            || !text(&check.literal)
            || !owned(&check.responsibility, &check.path)
            || check.dependency.as_ref().is_some_and(|edge| {
                edge.from != check.responsibility || !edges.contains(&(&edge.from, &edge.to))
            })
            || check.interface.as_ref().is_some_and(|id| {
                interfaces.get(id.as_str()).map_or(true, |item| {
                    item.owner != check.responsibility || item.path != check.path
                })
            })
        {
            return Err("invalid architecture check or associated dependency/interface".into());
        }
    }
    Ok(())
}
