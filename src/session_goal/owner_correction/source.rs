// Binds recorded source snapshots to confined target files.
// Requires readable regular files without symlinks; see `src/session_goal/owner_correction.rs`.
//! Bind recorded source snapshots to confined, byte-identical target files.

use super::model::{decisions, SourceRelocation};
use crate::{config::Loaded, evidence::hash};
use serde_json::Value;

fn bytes(loaded: &Loaded, path: &str) -> Result<(String, Vec<u8>), String> {
    use crate::project::path::SecureBytesResult;
    let relative = crate::project::path::inputs::confined(
        &loaded.product_root,
        "relocated requirement source",
        path,
    )?;
    let data = match crate::project::path::secure_bytes_observation(
        &loaded.product_root,
        &relative,
        "relocated requirement source",
    ) {
        SecureBytesResult::Bytes(bytes) => bytes,
        _ => {
            return Err(
                "relocated source must be a readable regular project file without symlinks".into(),
            )
        }
    };
    Ok((relative, data))
}

pub(super) fn relocations(
    loaded: &Loaded,
    record: &Value,
    raw: &str,
) -> Result<Vec<SourceRelocation>, String> {
    if raw.len() > 256 * 1024 {
        return Err("source relocation selection exceeds its bound".into());
    }
    let mut selected: Vec<SourceRelocation> = serde_json::from_str(raw).map_err(|_| {
        "--relocations must be a JSON list of originalRef, recordedSha256 and targetPath".to_owned()
    })?;
    let sources = super::super::requirements::recorded_sources(record)?;
    if selected.len() != sources.len() {
        return Err(
            "relocations must select every current Goal and requirement source exactly once".into(),
        );
    }
    selected.sort_by(|a, b| {
        (&a.original_ref, &a.recorded_sha256).cmp(&(&b.original_ref, &b.recorded_sha256))
    });
    for relocation in &mut selected {
        if selected_key_is_duplicate(
            &sources,
            &relocation.original_ref,
            &relocation.recorded_sha256,
        ) {
            return Err("recorded source identities are ambiguous".into());
        }
        let (_, _, text) = sources
            .iter()
            .find(|(reference, sha, _)| {
                reference == &relocation.original_ref && sha == &relocation.recorded_sha256
            })
            .ok_or("relocation names no current recorded source")?;
        let (relative, data) = bytes(loaded, &relocation.target_path)?;
        if hash::bytes(&data) != relocation.recorded_sha256 || data != text.as_bytes() {
            return Err("relocated source bytes do not match the recorded source snapshot".into());
        }
        relocation.target_path = relative;
    }
    if selected.windows(2).any(|pair| {
        pair[0].original_ref == pair[1].original_ref
            && pair[0].recorded_sha256 == pair[1].recorded_sha256
    }) {
        return Err("relocation source identity is duplicated".into());
    }
    Ok(selected)
}

fn selected_key_is_duplicate(
    sources: &[(String, String, String)],
    reference: &str,
    sha: &str,
) -> bool {
    sources
        .iter()
        .filter(|(r, s, _)| r == reference && s == sha)
        .count()
        != 1
}

pub(super) fn current_path(record: &Value, reference: &str, sha: &str) -> Result<String, String> {
    let decisions = decisions(record)?;
    Ok(decisions
        .last()
        .and_then(|decision| {
            decision
                .relocations
                .iter()
                .find(|item| item.original_ref == reference && item.recorded_sha256 == sha)
        })
        .map_or_else(|| reference.to_owned(), |item| item.target_path.clone()))
}

pub(super) fn current(loaded: &Loaded, record: &Value) -> Result<(), String> {
    for (reference, sha, text) in super::super::requirements::recorded_sources(record)? {
        let path = current_path(record, &reference, &sha)?;
        let (_, actual) = bytes(loaded, &path)?;
        let actual_sha = hash::bytes(&actual);
        if actual_sha != sha || actual != text.as_bytes() {
            let refusal = crate::drift::refusal(
                crate::drift::Class::GoalOwnerSource,
                &sha,
                Some(&actual_sha),
                None,
                crate::drift::Route::GoalStatus {
                    config: &loaded.path,
                },
            );
            return Err(format!("relocated source {path}: {refusal}"));
        }
    }
    Ok(())
}
