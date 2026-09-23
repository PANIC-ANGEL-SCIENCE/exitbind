use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub(crate) fn ensure_managed_directory(root: &Path, path: &Path) -> Result<(), String> {
    ensure_directory(root, path, false)
}

pub(crate) fn ensure_state_directory(root: &Path, path: &Path) -> Result<(), String> {
    ensure_directory(root, path, true)
}

fn ensure_directory(root: &Path, path: &Path, private: bool) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("managed path escapes project root: {}", path.display()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(info) => {
                if info.file_type().is_symlink() {
                    return Err(format!(
                        "managed directory must not be a symlink: {}",
                        current.display()
                    ));
                }
                if !info.is_dir() {
                    return Err(format!(
                        "managed path must be a directory: {}",
                        current.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if private {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::DirBuilderExt;
                        fs::DirBuilder::new()
                            .mode(0o700)
                            .create(&current)
                            .map_err(|e| e.to_string())?;
                    }
                    #[cfg(not(unix))]
                    fs::create_dir(&current).map_err(|e| e.to_string())?;
                } else {
                    fs::create_dir(&current).map_err(|e| e.to_string())?;
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let real_root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let real = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !real.starts_with(real_root) {
        return Err(format!(
            "managed path escapes project root: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn write_exclusive(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

pub(crate) fn open_state_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub(crate) fn write_state_exclusive(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = open_state_file(path).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}
