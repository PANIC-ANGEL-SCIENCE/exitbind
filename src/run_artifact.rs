//! Typed run-artifact evidence and immutable-byte revalidation.

use crate::{config, config::Loaded, hash};
use serde_json::{json, Value};
use std::{fs, path::Path};

pub(crate) fn evidence(
    loaded: &Loaded,
    requested_root: Option<&str>,
    requested: &str,
) -> Result<Value, String> {
    let (root_name, root) = match requested_root.unwrap_or("product") {
        "product" => ("product", &loaded.product_root),
        "state" => ("state", &loaded.state_root),
        _ => return Err("--artifact-root must be product or state".into()),
    };
    let path = confined(root, requested)?;
    if fs::symlink_metadata(path)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err(format!("artifact must not be a symlink: {requested}"));
    }
    let real = config::file(root, requested)?;
    if root_name == "state" {
        let targets = [real.as_path()];
        crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    }
    Ok(json!({
        "root": root_name,
        "path": config::rel(root, &real)?,
        "sha256": hash::bytes(&fs::read(real).map_err(|error| error.to_string())?)
    }))
}

pub(crate) fn assert_current(loaded: &Loaded, state: &Value) -> Result<(), String> {
    for item in state["submissions"].as_array().unwrap_or(&Vec::new()) {
        let root = match item["artifact"]["root"].as_str().unwrap_or("product") {
            "product" => &loaded.product_root,
            "state" => &loaded.state_root,
            _ => return Err("artifact drift detected: invalid root".into()),
        };
        let requested = item["artifact"]["path"].as_str().unwrap_or("");
        let path = confined(root, requested)
            .map_err(|_| format!("artifact drift detected: {requested}"))?;
        if fs::symlink_metadata(&path)
            .map_err(|_| format!("artifact drift detected: {requested}"))?
            .file_type()
            .is_symlink()
        {
            return Err(format!("artifact drift detected: {requested}"));
        }
        let real = config::file(root, requested)
            .map_err(|_| format!("artifact drift detected: {requested}"))?;
        if config::rel(root, &real).map_err(|_| format!("artifact drift detected: {requested}"))?
            != requested
            || hash::bytes(
                &fs::read(real).map_err(|_| format!("artifact drift detected: {requested}"))?,
            ) != item["artifact"]["sha256"]
        {
            return Err(format!("artifact drift detected: {requested}"));
        }
    }
    Ok(())
}

fn confined(root: &Path, requested: &str) -> Result<std::path::PathBuf, String> {
    let portable = requested.replace('\\', "/");
    if requested.trim().is_empty()
        || requested.contains('\0')
        || Path::new(requested).is_absolute()
        || portable.starts_with('/')
        || portable.contains(":/")
        || portable == ".."
        || portable.starts_with("../")
    {
        return Err(format!("path escapes project root: {requested}"));
    }
    let path = root.join(requested);
    let parent = path.parent().ok_or("path escapes project root")?;
    let real_parent = fs::canonicalize(parent).map_err(|error| error.to_string())?;
    if !real_parent.starts_with(root) {
        return Err(format!("path escapes project root: {requested}"));
    }
    if !path.exists() {
        return Err(format!("declared file does not exist: {requested}"));
    }
    Ok(path)
}
