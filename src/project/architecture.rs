//! Optional architecture context over owner-selected project configuration.
//! No approval, command execution, or acceptance state is owned here.

use crate::{config::Loaded, evidence::hash, project::path};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

mod adoption;
mod checks;
pub(crate) use adoption::select;
mod validation;

const MAX_SOURCE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Selection {
    pub(crate) source_path: String,
    pub(crate) source_sha256: String,
    pub(crate) revision: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    version: u64,
    revision: String,
    responsibilities: Vec<Responsibility>,
    dependencies: Vec<Dependency>,
    interfaces: Vec<Interface>,
    checks: Vec<Check>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Responsibility {
    id: String,
    summary: String,
    paths: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    from: String,
    to: String,
    direction: Direction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interface: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Direction {
    Allowed,
    Forbidden,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Interface {
    id: String,
    owner: String,
    path: String,
    summary: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Check {
    id: String,
    responsibility: String,
    path: String,
    assertion: Assertion,
    literal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependency: Option<Edge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interface: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Edge {
    from: String,
    to: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Assertion {
    Contains,
    Excludes,
}

pub(crate) fn validate_selection(value: &Value, errors: &mut Vec<String>) {
    match selection(value) {
        Ok(_) => {}
        Err(error) => errors.push(format!("project.architectureContract: {error}")),
    }
}

pub(crate) fn selection(value: &Value) -> Result<Selection, String> {
    let selected: Selection = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid source selection: {error}"))?;
    validation::selection(&selected)?;
    Ok(selected)
}

struct Current {
    contract: Contract,
    provenance: Value,
}

fn current(loaded: &Loaded) -> Result<Option<Current>, String> {
    let Some(selected) = &loaded.architecture_contract else {
        return Ok(None);
    };
    let ensure_config = || -> Result<(), String> {
        let source = std::fs::read_to_string(&loaded.path).map_err(|error| error.to_string())?;
        if source != loaded.source {
            return Err("architecture configuration changed during delivery; reload the current configuration".into());
        }
        Ok(())
    };
    ensure_config()?;
    let bytes = read(loaded, &selected.source_path, MAX_SOURCE_BYTES)?;
    if hash::bytes(&bytes) != selected.source_sha256 {
        return Err("architecture contract source changed; restore the selected bytes or use the existing owner approval/configuration and run supersede paths".into());
    }
    let contract: Contract = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid architecture contract: {error}"))?;
    validation::contract(&contract)?;
    if contract.revision != selected.revision {
        return Err("architecture contract revision differs from project configuration".into());
    }
    ensure_config()?;
    Ok(Some(Current {
        provenance: json!({"sourcePath": selected.source_path,
            "sourceSha256": selected.source_sha256, "revision": selected.revision,
            "schemaVersion": contract.version, "configurationSha256": hash::text(&loaded.source),
            "selection": "current project configuration"}),
        contract,
    }))
}

fn read(loaded: &Loaded, source: &str, limit: u64) -> Result<Vec<u8>, String> {
    match path::secure_bytes_observation_bounded(
        &loaded.product_root,
        source,
        "architecture source",
        limit,
    ) {
        path::SecureBytesResult::Bytes(bytes) => Ok(bytes),
        path::SecureBytesResult::Absent(reason)
        | path::SecureBytesResult::Unsafe(reason)
        | path::SecureBytesResult::Unreadable(reason) => Err(reason),
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(reason) => Err(reason),
    }
}

pub(crate) fn assert_current(loaded: &Loaded) -> Result<(), String> {
    current(loaded).map(|_| ())
}

/// Legacy away has no bounded scoped-delivery adapter. A frozen selection
/// still requires delivery when current configuration has removed it.
pub(crate) fn away_supported(loaded: &Loaded, assignment: &Value) -> Result<(), String> {
    if loaded.architecture_contract.is_some()
        || assignment
            .get("architectureContractSha256")
            .is_some_and(|identity| !identity.is_null())
    {
        return Err("Architecture Contract delivery is unsupported by away; use the current Work delivery routes".into());
    }
    Ok(())
}

/// The run already owns frozen configuration and plan evidence. Keep only the
/// optional source selection there, never another architecture acceptance log.
pub(crate) fn bind_plan(loaded: &Loaded, plan: &mut Value) -> Result<(), String> {
    assert_current(loaded)?;
    if let Some(selected) = &loaded.architecture_contract {
        plan["architectureContract"] =
            serde_json::to_value(selected).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn reference(loaded: &Loaded) -> Result<Option<Value>, String> {
    Ok(current(loaded)?.map(|current| current.provenance))
}

/// A Lead may inspect the current architecture and propose a replacement.
/// Recipients get only the responsibilities intersecting their assignment.
pub(crate) fn delivery(loaded: &Loaded, packet: &Value) -> Result<Option<Value>, String> {
    let selected = loaded
        .architecture_contract
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| error.to_string())?;
    let frozen = packet["architectureContractSha256"].as_str();
    if frozen
        .is_some_and(|expected| selected.as_ref().map(hash::value).as_deref() != Some(expected))
        || ((frozen.is_some() || selected.is_some())
            && packet["context"]["scope"]["configSha256"]
                .as_str()
                .is_some_and(|expected| expected != hash::text(&loaded.source)))
    {
        return Err("architecture selection differs from the assignment configuration; use the existing run supersede path".into());
    }
    let Some(current) = current(loaded)? else {
        return Ok(None);
    };
    let role = packet["role"]
        .as_str()
        .ok_or("architecture recipient role is missing")?;
    if !matches!(role, "lead" | "worker" | "reviewer" | "adviser") {
        return Err("architecture recipient role is unsupported".into());
    }
    let paths: Vec<&str> = ["observe", "write"]
        .into_iter()
        .flat_map(|field| {
            packet["declaredBoundary"][field]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .collect();
    Ok(Some(slice(current, role, &paths)))
}

fn slice(current: Current, role: &str, paths: &[&str]) -> Value {
    let contract = current.contract;
    let selected: BTreeSet<&str> = contract
        .responsibilities
        .iter()
        .filter(|item| {
            role == "lead"
                || item.paths.iter().any(|path| {
                    paths
                        .iter()
                        .any(|scope| validation::intersects(path, scope))
                })
        })
        .map(|item| item.id.as_str())
        .collect();
    let dependencies: Vec<_> = contract
        .dependencies
        .iter()
        .filter(|edge| selected.contains(edge.from.as_str()) || selected.contains(edge.to.as_str()))
        .collect();
    let interfaces: Vec<_> = contract
        .interfaces
        .iter()
        .filter(|item| {
            selected.contains(item.owner.as_str())
                || dependencies
                    .iter()
                    .any(|edge| edge.interface.as_deref() == Some(item.id.as_str()))
        })
        .collect();
    json!({"version": 1, "state": "current", "provenance": current.provenance,
        "role": role, "scopePaths": paths,
        "responsibilities": contract.responsibilities.iter()
            .filter(|item| selected.contains(item.id.as_str())).collect::<Vec<_>>(),
        "dependencies": dependencies, "interfaces": interfaces,
        "checks": contract.checks.iter()
            .filter(|item| selected.contains(item.responsibility.as_str())).collect::<Vec<_>>()})
}

pub(crate) fn inspect(loaded: &Loaded) -> Result<Value, String> {
    Ok(delivery(loaded, &json!({"role": "lead"}))?
        .unwrap_or_else(|| json!({"version": 1, "state": "absent"})))
}

pub(crate) fn check(loaded: &Loaded) -> Result<Value, String> {
    checks::run(loaded)
}
