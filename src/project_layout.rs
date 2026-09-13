//! Typed project roots and machine-local bindings for portable and local modes.

use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

const BINDING_VERSION: u64 = 1;
pub(crate) const CANONICAL_AGENTS_DIR: &str = "soulmate/agents";
pub(crate) const CANONICAL_CONTROL_DIRS: [&str; 4] = [
    CANONICAL_AGENTS_DIR,
    "soulmate/boundaries",
    "soulmate/policies",
    "soulmate/harness",
];
pub(crate) const CANONICAL_STATE_DIRS: [&str; 6] = [
    ".soulmate/runs",
    ".soulmate/memory",
    ".soulmate/artifacts",
    ".soulmate/receipts",
    ".soulmate/away",
    ".soulmate/locks",
];

/// The legacy constants above remain stable for readers and compatibility
/// tests. New Exitbind invocations select their own namespace at this small
/// boundary so an old `soulmate` invocation never rewrites its layout.
pub(crate) fn exitbind_surface() -> bool {
    crate::producer::exitbind_surface()
}

pub(crate) fn agents_dir() -> &'static str {
    if exitbind_surface() {
        "exitbind/agents"
    } else {
        CANONICAL_AGENTS_DIR
    }
}

pub(crate) fn control_dirs() -> [&'static str; 4] {
    if exitbind_surface() {
        [
            "exitbind/agents",
            "exitbind/boundaries",
            "exitbind/policies",
            "exitbind/harness",
        ]
    } else {
        CANONICAL_CONTROL_DIRS
    }
}

pub(crate) fn state_dirs() -> [&'static str; 6] {
    if exitbind_surface() {
        [
            ".exitbind/runs",
            ".exitbind/memory",
            ".exitbind/artifacts",
            ".exitbind/receipts",
            ".exitbind/away",
            ".exitbind/locks",
        ]
    } else {
        CANONICAL_STATE_DIRS
    }
}

pub(crate) fn state_namespace() -> &'static str {
    if exitbind_surface() {
        ".exitbind"
    } else {
        ".soulmate"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Portable,
    Local,
}

#[derive(Clone, Debug)]
pub struct ProjectLayout {
    pub mode: Mode,
    pub control_root: PathBuf,
    pub product_root: PathBuf,
    pub state_root: PathBuf,
    pub project_id: Option<String>,
}

pub fn resolve(config_path: &Path, config: &Value) -> Result<ProjectLayout, String> {
    let config_parent = config_path
        .parent()
        .ok_or("configuration must have a parent directory")?;
    let project = config
        .get("project")
        .and_then(Value::as_object)
        .ok_or("project must be an object")?;
    let mode = match project.get("mode").and_then(Value::as_str) {
        None | Some("portable") => Mode::Portable,
        Some("local") => Mode::Local,
        Some(_) => return Err("project.mode must be 'portable' or 'local'".into()),
    };
    let declared_root = project["root"]
        .as_str()
        .ok_or("project.root must be a string")?;
    let control_root = canonical_directory(config_parent)?;
    if mode == Mode::Portable {
        let root = canonical_directory(&config_parent.join(declared_root))?;
        return Ok(ProjectLayout {
            mode,
            control_root: root.clone(),
            product_root: root.clone(),
            state_root: root,
            project_id: None,
        });
    }
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .ok_or("local projects require project.id")?;
    validate_id(project_id)?;
    if Path::new(declared_root).is_absolute() || declared_root.contains('\0') {
        return Err("local project.root must be relative to ControlRoot".into());
    }
    if declared_root != "." {
        return Err("local project.root must be '.' relative to ControlRoot".into());
    }
    let binding = read_binding(project_id)?;
    if binding["projectId"] != project_id {
        return Err("project binding id mismatch".into());
    }
    if binding.get("controlRoot").is_some()
        && bound_directory(&binding, "controlRoot")? != control_root
    {
        return Err("project binding ControlRoot mismatch".into());
    }
    let product_root = bound_directory(&binding, "productRoot")?;
    let state_root = bound_directory(&binding, "stateRoot")?;
    ensure_distinct_roots(&control_root, &product_root, &state_root)?;
    Ok(ProjectLayout {
        mode,
        control_root,
        product_root,
        state_root,
        project_id: Some(project_id.to_owned()),
    })
}

pub fn create_binding(
    project_id: &str,
    control_root: &Path,
    product_root: &Path,
    state_root: &Path,
) -> Result<PathBuf, String> {
    validate_id(project_id)?;
    let control_root = canonical_directory(control_root)?;
    let product_root = canonical_directory(product_root)?;
    let state_root = canonical_directory(state_root)?;
    ensure_distinct_roots(&control_root, &product_root, &state_root)?;
    let directory = binding_directory(true)?;
    let path = directory.join(format!("{project_id}.json"));
    if fs::symlink_metadata(&path).is_ok() {
        return Err("project binding already exists; refusing to replace it".into());
    }
    let value = json!({
        "version": BINDING_VERSION,
        "projectId": project_id,
        "controlRoot": control_root,
        "productRoot": product_root,
        "stateRoot": state_root,
    });
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    let serialized = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    file.write_all(format!("{serialized}\n").as_bytes())
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    Ok(path)
}

pub fn ensure_binding_available(project_id: &str) -> Result<(), String> {
    validate_id(project_id)?;
    let directory = binding_directory(true)?;
    let path = directory.join(format!("{project_id}.json"));
    if fs::symlink_metadata(path).is_ok() {
        return Err("project binding already exists; refusing to replace it".into());
    }
    Ok(())
}

pub fn bind_from_config(
    config_path: &Path,
    config: &Value,
    product_root: &Path,
    state_root: &Path,
) -> Result<PathBuf, String> {
    let project = config["project"]
        .as_object()
        .ok_or("project must be an object")?;
    if project.get("mode").and_then(Value::as_str) != Some("local") {
        return Err("bind requires a local project configuration".into());
    }
    let id = project["id"]
        .as_str()
        .ok_or("local projects require project.id")?;
    let control = config_path.parent().ok_or("configuration has no parent")?;
    match create_binding(id, control, product_root, state_root) {
        Ok(path) => Ok(path),
        Err(error) if error == "project binding already exists; refusing to replace it" => {
            upgrade_binding(id, control, product_root, state_root)
        }
        Err(error) => Err(error),
    }
}

fn upgrade_binding(
    project_id: &str,
    control_root: &Path,
    product_root: &Path,
    state_root: &Path,
) -> Result<PathBuf, String> {
    let control_root = canonical_directory(control_root)?;
    let product_root = canonical_directory(product_root)?;
    let state_root = canonical_directory(state_root)?;
    ensure_distinct_roots(&control_root, &product_root, &state_root)?;
    let current = read_binding(project_id)?;
    if bound_directory(&current, "productRoot")? != product_root
        || bound_directory(&current, "stateRoot")? != state_root
    {
        return Err("existing project binding roots do not match; refusing to replace it".into());
    }
    if let Some(existing) = current.get("controlRoot") {
        if canonical_directory(Path::new(
            existing.as_str().ok_or("invalid project binding")?,
        ))? != control_root
        {
            return Err(
                "existing project binding ControlRoot does not match; refusing to replace it"
                    .into(),
            );
        }
        return Ok(binding_directory(false)?.join(format!("{project_id}.json")));
    }
    let directory = binding_directory(false)?;
    let path = directory.join(format!("{project_id}.json"));
    let temporary = directory.join(format!(".{project_id}.{}.tmp", std::process::id()));
    let value = json!({
        "version": BINDING_VERSION,
        "projectId": project_id,
        "controlRoot": control_root,
        "productRoot": product_root,
        "stateRoot": state_root,
    });
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let serialized = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(format!("{serialized}\n").as_bytes()) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    drop(file);
    fs::rename(&temporary, &path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })?;
    Ok(path)
}

fn read_binding(project_id: &str) -> Result<Value, String> {
    let directory = binding_directory(false)?;
    let path = directory.join(format!("{project_id}.json"));
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("project binding: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("project binding must be a regular file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    let before = file.metadata().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if before.permissions().mode() & 0o077 != 0 {
            return Err("project binding must be private".into());
        }
    }
    let mut source = String::new();
    file.read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    let mut confirmation = String::new();
    file.rewind().map_err(|error| error.to_string())?;
    file.read_to_string(&mut confirmation)
        .map_err(|error| error.to_string())?;
    let after = file.metadata().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let replaced = {
        use std::os::unix::fs::MetadataExt;
        before.dev() != after.dev() || before.ino() != after.ino()
    };
    #[cfg(not(unix))]
    let replaced = false;
    if source != confirmation || before.len() != after.len() || replaced {
        return Err("project binding changed while reading".into());
    }
    let value: Value = serde_json::from_str(&source)
        .map_err(|error| format!("invalid project binding: {error}"))?;
    let object = value.as_object().ok_or("invalid project binding")?;
    let allowed = [
        "version",
        "projectId",
        "controlRoot",
        "productRoot",
        "stateRoot",
    ];
    if !matches!(object.len(), 4 | 5)
        || object
            .keys()
            .any(|field| !allowed.contains(&field.as_str()))
        || value["version"] != BINDING_VERSION
        || value["projectId"] != project_id
        || (object.contains_key("controlRoot") && !value["controlRoot"].is_string())
        || !value["productRoot"].is_string()
        || !value["stateRoot"].is_string()
    {
        return Err("invalid project binding".into());
    }
    Ok(value)
}

/// Resolve a local-mode configuration from the private machine binding that
/// names `product_root`. Portable projects keep their configuration directly
/// under the product root and do not need this lookup.
pub(crate) fn config_for_product(product_root: &Path) -> Result<Option<PathBuf>, String> {
    let product_root = canonical_directory(product_root)?;
    let directory = match binding_directory(false) {
        Ok(value) => value,
        Err(error) if error.starts_with("binding directory:") => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut names = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("project binding directory contains an unsafe entry".into());
        }
        let name = entry
            .file_name()
            .to_str()
            .ok_or("project binding name is not UTF-8")?
            .to_owned();
        let Some(project_id) = name.strip_suffix(".json") else {
            return Err("project binding name is invalid".into());
        };
        validate_id(project_id)?;
        names.push(project_id.to_owned());
    }
    names.sort();
    if names.len() > 256 {
        return Err("project binding count exceeds lookup limit".into());
    }
    let mut found = None;
    for project_id in names {
        let binding = read_binding(&project_id)?;
        if bound_directory(&binding, "productRoot")? != product_root {
            continue;
        }
        let Some(control) = binding.get("controlRoot") else {
            continue;
        };
        let control = canonical_directory(Path::new(
            control.as_str().ok_or("invalid project binding")?,
        ))?;
        let candidate = [control.join("exitbind.json"), control.join("soulmate.json")]
            .into_iter()
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| "local project binding has no configuration".to_owned())?;
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("local project binding configuration is unsafe".into());
        }
        if found.replace(candidate).is_some() {
            return Err("multiple local project bindings match this ProductRoot".into());
        }
    }
    Ok(found)
}

fn bound_directory(binding: &Value, field: &str) -> Result<PathBuf, String> {
    let value = binding[field].as_str().ok_or("invalid project binding")?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("project binding roots must be absolute".into());
    }
    canonical_directory(&path)
}

fn binding_directory(create: bool) -> Result<PathBuf, String> {
    let base = if exitbind_surface() {
        std::env::var_os("EXITBIND_BINDINGS_DIR")
            .or_else(|| std::env::var_os("SOULMATE_BINDINGS_DIR"))
    } else {
        std::env::var_os("SOULMATE_BINDINGS_DIR")
    }
    .map(PathBuf::from)
    .or_else(|| {
        std::env::var_os("XDG_STATE_HOME").map(|value| {
            PathBuf::from(value).join(if exitbind_surface() {
                "exitbind/bindings"
            } else {
                "soulmate/bindings"
            })
        })
    })
    .or_else(|| {
        std::env::var_os("HOME").map(|value| {
            PathBuf::from(value).join(if exitbind_surface() {
                ".local/state/exitbind/bindings"
            } else {
                ".local/state/soulmate/bindings"
            })
        })
    })
    .ok_or("cannot determine machine-local binding directory")?;
    if !base.is_absolute() {
        return Err("machine-local binding directory must be absolute".into());
    }
    let mut current = PathBuf::new();
    for component in base.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(entry) => {
                if entry.file_type().is_symlink() || !entry.is_dir() {
                    return Err("binding directory must not contain symlinks or files".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                fs::create_dir(&current).map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(format!("binding directory: {error}")),
        }
    }
    let metadata =
        fs::symlink_metadata(&base).map_err(|error| format!("binding directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("binding directory must be a real directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(base)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("root path must be absolute: {}", path.display()));
    }
    reject_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "path must be an existing regular directory: {}",
            path.display()
        ));
    }
    fs::canonicalize(path).map_err(|error| error.to_string())
}

fn reject_symlink_components(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "root path must not contain symlinks: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn ensure_distinct_roots(control: &Path, product: &Path, state: &Path) -> Result<(), String> {
    if control == product || control == state || product == state {
        return Err("ControlRoot, ProductRoot, and StateRoot must be distinct".into());
    }
    if product.starts_with(control) || state.starts_with(control) {
        return Err("ProductRoot and StateRoot must not be beneath ControlRoot".into());
    }
    if control.starts_with(product)
        || control.starts_with(state)
        || product.starts_with(state)
        || state.starts_with(product)
    {
        return Err("project roots must not be nested".into());
    }
    Ok(())
}

pub fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("project.id must be a portable identifier".into());
    }
    Ok(())
}
