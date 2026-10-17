//! Exact canonical configuration identities used by Goal recovery.

use super::ConfigBinding;
use crate::{config::Loaded, evidence::hash};
use std::path::Path;

fn canonical(path: &Path, label: &str) -> Result<String, String> {
    path.canonicalize()
        .map_err(|error| format!("{label} is unavailable: {error}"))?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} is not valid UTF-8"))
}

pub(super) fn config_binding(loaded: &Loaded) -> Result<ConfigBinding, String> {
    Ok(ConfigBinding {
        path: canonical(&loaded.path, "recovery configuration")?,
        config_sha256: hash::text(&loaded.source),
        product_root: canonical(&loaded.product_root, "recovery product root")?,
        state_root: canonical(&loaded.state_root, "recovery state root")?,
        project_id: loaded.project_id.clone(),
        project_identity_sha256: crate::session_goal::requirements::identity(loaded)?,
        local_identity_sha256: crate::session_goal::native::owner_for(loaded)?.identity_sha256,
    })
}
