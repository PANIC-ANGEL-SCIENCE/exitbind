//! Which product paths of a marked run are launch-only guidance and may be
//! omitted from its tested-input fingerprint: the frozen memory sources and
//! selection ledgers, minus every Git-tracked, checker, architecture or
//! declared verdict input that keeps a path strict.
use super::{enabled, INPUT_CONTRACT_FIELD};
use serde_json::Value;
use std::collections::BTreeSet;

/// Exact memory source and selection-ledger paths frozen into the run. Only
/// these validated paths can be omitted from the tested-input fingerprint.
fn selected_source_paths(state: &Value) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    if !enabled(state) {
        return paths;
    }
    let Some(stages) = state["plan"]["stages"].as_array() else {
        return paths;
    };
    for stage in stages {
        for agent in stage["agents"].as_array().into_iter().flatten() {
            for reference in agent["memoryReferences"].as_array().into_iter().flatten() {
                for field in ["sourcePath", "ledgerPath"] {
                    if let Some(path) = reference[field].as_str() {
                        if safe_relative(path) {
                            paths.insert(path.to_owned());
                        }
                    }
                }
            }
        }
    }
    paths
}

pub(super) fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Advisory files are provenance unless the reviewed verification contract
/// names them as verdict inputs. Direct checker and architecture sources stay
/// strict; arbitrary checker reads cannot establish semantic dependence.
pub(crate) fn unambiguous_paths(loaded: &crate::config::Loaded, state: &Value) -> BTreeSet<String> {
    let mut candidates = selected_source_paths(state);
    if !enabled(state) {
        return BTreeSet::new();
    }
    // A quarantine verification is an exact frozen execution. Its marker
    // records the full tested-input fingerprint, so the verification Work
    // must not omit launch-only guidance from its own tested inputs.
    if state.get("quarantineVerification").is_some() {
        return BTreeSet::new();
    }
    if (state.get("checkPolicy").is_some() || state.get("preservation").is_some())
        && state["verificationCheckerSources"].as_array().is_none()
    {
        return BTreeSet::new();
    }
    if state.get("preReturnPolicy").is_some()
        && state["preReturnPolicy"]["checkerSources"]
            .as_array()
            .is_none()
    {
        return BTreeSet::new();
    }
    let mut memory_candidates = std::mem::take(&mut candidates);
    add_product_path(&loaded.product_root, &loaded.path, &mut candidates);
    if let Some(stages) = state["plan"]["stages"].as_array() {
        for agent in stages
            .iter()
            .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
        {
            let Some(name) = agent["name"].as_str() else {
                continue;
            };
            let Some(configured) = loaded.agent(name) else {
                continue;
            };
            if let Ok(path) = crate::config::file(&loaded.control_root, &configured.profile) {
                add_product_path(&loaded.product_root, &path, &mut candidates);
            }
            let Some(scope_ceiling) = agent["declaredBoundary"]["memoryRead"].as_array() else {
                return BTreeSet::new();
            };
            let Some(scope_ceiling) = scope_ceiling
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
            else {
                return BTreeSet::new();
            };
            let Some(cross_context) = agent["declaredBoundary"]["crossContext"].as_str() else {
                return BTreeSet::new();
            };
            let Some(task) = state["goal"].as_str() else {
                return BTreeSet::new();
            };
            let Ok(current) = crate::memory::selection::resolve_for_task_optional_with_ceiling(
                loaded,
                name,
                Some(task),
                &scope_ceiling,
                cross_context,
            ) else {
                return BTreeSet::new();
            };
            for reference in current.references {
                for field in ["sourcePath", "ledgerPath"] {
                    if let Some(path) = reference[field].as_str() {
                        if safe_relative(path) {
                            memory_candidates.insert(path.to_owned());
                        }
                    }
                }
            }
        }
    }
    // A memory item that points at tracked product source is still product
    // source: its edits change the result, so it stays a verification input.
    let Some(tracked) = tracked_paths(&loaded.product_root, &memory_candidates) else {
        return BTreeSet::new();
    };
    candidates.extend(
        memory_candidates
            .into_iter()
            .filter(|path| !tracked.contains(path)),
    );
    // Harness receipts retain their own exact product binding. Keep every
    // input strict for them because a path may be evidence even when it is not
    // named by the selected verification recipe.
    if state.get("harnessReceipt").is_some() {
        return BTreeSet::new();
    }
    let mut strict = BTreeSet::new();
    if let Some(path) = state["plan"]["architectureContract"]["sourcePath"].as_str() {
        strict.insert(path.to_owned());
    }
    for input in state[INPUT_CONTRACT_FIELD]["guidanceVerdictInputs"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(path) = input["path"].as_str() {
            strict.insert(path.to_owned());
        }
    }
    for source in state["verificationCheckerSources"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            state["preReturnPolicy"]["checkerSources"]
                .as_array()
                .into_iter()
                .flatten(),
        )
    {
        for field in ["path", "argumentPath"] {
            if let Some(path) = source[field].as_str() {
                let source = std::path::Path::new(path);
                let resolved = if source.is_absolute() {
                    source.to_path_buf()
                } else {
                    loaded.product_root.join(source)
                };
                add_product_path(&loaded.product_root, &resolved, &mut strict);
            }
        }
    }
    let strict_canonical = strict
        .iter()
        .filter_map(|path| loaded.product_root.join(path).canonicalize().ok())
        .collect::<BTreeSet<_>>();
    candidates.retain(|path| {
        !strict.contains(path)
            && loaded
                .product_root
                .join(path)
                .canonicalize()
                .map_or(true, |resolved| !strict_canonical.contains(&resolved))
    });
    candidates
}

/// The subset of `paths` tracked by Git, or `None` when Git cannot answer
/// (callers then keep every candidate strict). Outside Git nothing is tracked.
pub(super) fn tracked_paths(
    root: &std::path::Path,
    paths: &BTreeSet<String>,
) -> Option<BTreeSet<String>> {
    if paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let inside = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .env("GIT_LITERAL_PATHSPECS", "1")
        .output()
        .ok()?;
    if !inside.status.success() || String::from_utf8_lossy(&inside.stdout).trim() != "true" {
        return Some(BTreeSet::new());
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--"])
        .args(paths)
        .env("GIT_LITERAL_PATHSPECS", "1")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| std::str::from_utf8(part).ok().map(str::to_owned))
        .collect()
}

fn add_product_path(root: &std::path::Path, path: &std::path::Path, paths: &mut BTreeSet<String>) {
    if let Ok(path) = path.canonicalize() {
        if let Ok(relative) = path.strip_prefix(root) {
            if let Some(relative) = relative.to_str() {
                let value = relative.replace('\\', "/");
                if safe_relative(&value) {
                    paths.insert(value);
                }
            }
        }
    }
}
