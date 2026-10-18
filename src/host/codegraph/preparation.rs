use super::provider::{run_provider, source_identity, status, IndexState, ProviderIdentity};
use super::{canonical, project_id, write_atomic, MAX_METADATA};
use crate::config;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, time::Instant};

const PREPARATION: &str = ".codegraph/exitbind-preparation.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreparationIdentity {
    kind: String,
    protocol: u64,
    product_root: String,
    project_id: String,
    provider: ProviderIdentity,
    index_path: String,
    index_identity: String,
    source_identity: String,
}

pub(super) struct Readiness {
    pub(super) provider: ProviderIdentity,
    pub(super) status: Value,
    pub(super) index_path: String,
    pub(super) index_identity: String,
    pub(super) source_identity: String,
}

fn saved_preparation(root: &Path) -> Result<Option<PreparationIdentity>, String> {
    let bytes = match crate::project::path::secure_bytes_observation_single_link_bounded(
        root,
        PREPARATION,
        "CodeGraph preparation metadata",
        MAX_METADATA as u64,
    ) {
        crate::project::path::SecureBytesResult::Absent(_) => return Ok(None),
        crate::project::path::SecureBytesResult::Bytes(bytes) => bytes,
        crate::project::path::SecureBytesResult::Unsafe(error)
        | crate::project::path::SecureBytesResult::Unreadable(error) => return Err(error),
        #[cfg(not(unix))]
        crate::project::path::SecureBytesResult::Unsupported(error) => return Err(error),
    };
    let metadata: PreparationIdentity = serde_json::from_slice(&bytes)
        .map_err(|_| "CodeGraph preparation metadata is malformed")?;
    Ok(Some(metadata))
}

fn preparation_matches(
    metadata: &PreparationIdentity,
    root: &Path,
    project_id: &str,
    provider: &ProviderIdentity,
    index_path: &str,
    index_identity: &str,
    source: &str,
) -> bool {
    metadata.kind == "exitbind_codegraph_preparation"
        && metadata.protocol == 1
        && metadata.product_root == root.to_string_lossy()
        && metadata.project_id == project_id
        && &metadata.provider == provider
        && metadata.index_path == index_path
        && metadata.index_identity == index_identity
        && metadata.source_identity == source
}

#[derive(Clone, Copy)]
enum PrepareAction {
    Init,
    Sync,
    FullIndex,
}

fn prepare_action(
    status: &IndexState,
    previous: Option<&PreparationIdentity>,
    root: &Path,
    project_id: &str,
    provider: &ProviderIdentity,
    source: &str,
) -> PrepareAction {
    match status {
        IndexState::Missing => PrepareAction::Init,
        IndexState::Ready {
            stale_reason: Some(_),
            ..
        } => PrepareAction::FullIndex,
        IndexState::Ready {
            index_path,
            index_identity,
            pending_changes,
            ..
        } => {
            let Some(previous) = previous else {
                return PrepareAction::FullIndex;
            };
            let target_matches = previous.kind == "exitbind_codegraph_preparation"
                && previous.protocol == 1
                && previous.product_root == root.to_string_lossy()
                && previous.project_id == project_id
                && previous.provider == *provider
                && previous.index_path == *index_path
                && previous.index_identity == *index_identity;
            if !target_matches {
                return PrepareAction::FullIndex;
            }
            if previous.source_identity == source {
                return PrepareAction::Sync;
            }
            if *pending_changes {
                PrepareAction::Sync
            } else {
                PrepareAction::FullIndex
            }
        }
    }
}

fn run_prepare_action(
    executable: &Path,
    root: &Path,
    action: PrepareAction,
    deadline: Instant,
) -> Result<(), String> {
    let root_string = root.to_str().ok_or("target path is not UTF-8")?;
    match action {
        PrepareAction::Init => {
            run_provider(
                executable,
                root,
                &["init", root_string, "--yes"],
                deadline,
                true,
            )?;
        }
        PrepareAction::Sync => {
            run_provider(
                executable,
                root,
                &["sync", root_string, "--quiet"],
                deadline,
                false,
            )?;
        }
        PrepareAction::FullIndex => {
            run_provider(
                executable,
                root,
                &["index", root_string, "--quiet"],
                deadline,
                false,
            )?;
        }
    }
    Ok(())
}

fn store_preparation(
    root: &Path,
    metadata: &PreparationIdentity,
    deadline: Instant,
) -> Result<(), String> {
    if Instant::now() >= deadline {
        return Err("prepare_timeout: deadline elapsed before metadata write".into());
    }
    write_atomic(
        root,
        PREPARATION,
        &serde_json::to_vec(metadata).map_err(|error| error.to_string())?,
        MAX_METADATA,
    )
}

pub(super) fn prepare(
    loaded: &config::Loaded,
    executable: &Path,
    provider: &ProviderIdentity,
    deadline: Instant,
    reconciliation_left: &mut u8,
) -> Result<Readiness, String> {
    let root = canonical(&loaded.product_root)?;
    let project_id = project_id(loaded);
    let mut last_change = false;
    loop {
        if Instant::now() >= deadline {
            return Err("prepare_timeout: 120-second preparation deadline exceeded".into());
        }
        let source_before = source_identity(loaded, deadline)?;
        let saved = saved_preparation(&root)?;
        let index_state = status(executable, &root, deadline)?;
        if let IndexState::Ready {
            status,
            index_path,
            index_identity,
            stale_reason: None,
            pending_changes: false,
        } = &index_state
        {
            if saved.as_ref().is_some_and(|metadata| {
                preparation_matches(
                    metadata,
                    &root,
                    project_id,
                    provider,
                    index_path,
                    index_identity,
                    &source_before,
                )
            }) {
                let source_after = source_identity(loaded, deadline)?;
                if source_after == source_before {
                    return Ok(Readiness {
                        provider: provider.clone(),
                        status: status.clone(),
                        index_path: index_path.clone(),
                        index_identity: index_identity.clone(),
                        source_identity: source_before,
                    });
                }
                if *reconciliation_left == 0 {
                    return Err(
                        "source_changed: source changed while checking the saved preparation"
                            .into(),
                    );
                }
                *reconciliation_left -= 1;
                last_change = true;
                continue;
            }
        }
        let action = if last_change {
            PrepareAction::FullIndex
        } else {
            prepare_action(
                &index_state,
                saved.as_ref(),
                &root,
                project_id,
                provider,
                &source_before,
            )
        };
        run_prepare_action(executable, &root, action, deadline)?;
        let source_after = source_identity(loaded, deadline)?;
        let after_state = status(executable, &root, deadline)?;
        let (status_value, index_path, index_identity, stale_reason, pending_changes) =
            match after_state {
                IndexState::Missing => {
                    return Err(
                        "index_unavailable: provider preparation did not create an index".into(),
                    )
                }
                IndexState::Ready {
                    status,
                    index_path,
                    index_identity,
                    stale_reason,
                    pending_changes,
                } => (
                    status,
                    index_path,
                    index_identity,
                    stale_reason,
                    pending_changes,
                ),
            };
        if source_after != source_before {
            if *reconciliation_left == 0 {
                return Err("source_changed: source changed during preparation after one bounded reconciliation".into());
            }
            *reconciliation_left -= 1;
            last_change = true;
            continue;
        }
        if let Some(reason) = stale_reason {
            return Err(format!(
                "index_stale: {reason}; bounded full indexing did not produce a complete index"
            ));
        }
        if pending_changes {
            return Err(
                "index_stale: provider still reports pending source changes after synchronization"
                    .into(),
            );
        }
        let metadata = PreparationIdentity {
            kind: "exitbind_codegraph_preparation".into(),
            protocol: 1,
            product_root: root.to_string_lossy().into_owned(),
            project_id: project_id.to_owned(),
            provider: provider.clone(),
            index_path: index_path.clone(),
            index_identity: index_identity.clone(),
            source_identity: source_before.clone(),
        };
        store_preparation(&root, &metadata, deadline)?;
        return Ok(Readiness {
            provider: provider.clone(),
            status: status_value,
            index_path,
            index_identity,
            source_identity: source_before,
        });
    }
}
