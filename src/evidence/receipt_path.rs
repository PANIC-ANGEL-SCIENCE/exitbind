//! Confined receipt output and read paths.

use std::fs;
use std::path::{Component, Path};

const OUTPUT_SCOPE: &str = "receipt --output must be inside StateRoot; use a StateRoot-relative path or an absolute path whose parent resolves inside StateRoot";

pub(super) fn state_relative(root: &Path, requested: &str) -> Result<String, String> {
    if requested.trim().is_empty() || requested.contains('\0') || requested.contains('\\') {
        return Err("receipt path must remain beneath StateRoot".into());
    }
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let path = Path::new(requested);
    let relative = if path.is_absolute() {
        path.strip_prefix(&root)
            .map_err(|_| "receipt path must remain beneath StateRoot".to_owned())?
    } else {
        path
    };
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("receipt path must be a normalized relative path".into());
    }
    let rendered = relative.to_str().ok_or("receipt path must be UTF-8")?;
    if !is_relative_path(rendered) {
        return Err("receipt path must be a normalized relative path".into());
    }
    Ok(rendered.replace('\\', "/"))
}

pub(super) fn is_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.trim().is_empty()
        && !value.contains('\0')
        && !value.contains('\\')
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(super) fn receipt_path(root: &Path, requested: &str) -> Result<std::path::PathBuf, String> {
    let path = Path::new(requested);
    if path.is_absolute() {
        let parent = path.parent().ok_or(OUTPUT_SCOPE)?;
        let real_parent = fs::canonicalize(parent).map_err(|error| error.to_string())?;
        let real_state = fs::canonicalize(root).map_err(|error| error.to_string())?;
        if !real_parent.starts_with(real_state) {
            return Err(OUTPUT_SCOPE.into());
        }
        return Ok(path.to_path_buf());
    }
    if requested.trim().is_empty()
        || requested.contains('\0')
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("receipt --output must be a normalized StateRoot-relative path without parent traversal".into());
    }
    Ok(root.join(path))
}

#[cfg(test)]
mod tests {
    use super::receipt_path;
    use std::fs;

    #[test]
    fn outside_absolute_output_explains_state_root_scope() {
        let root =
            std::env::temp_dir().join(format!("exitbind-receipt-scope-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let outside = std::env::temp_dir().join("outside-receipt.json");
        let error = receipt_path(&root, outside.to_str().unwrap()).unwrap_err();
        assert!(error.contains("--output"));
        assert!(error.contains("StateRoot-relative"));
        assert_eq!(
            receipt_path(&root, "receipt.json").unwrap(),
            root.join("receipt.json")
        );
        assert_eq!(
            receipt_path(&root, root.join("receipt.json").to_str().unwrap()).unwrap(),
            root.join("receipt.json")
        );
        fs::remove_dir(root).unwrap();
    }
}
