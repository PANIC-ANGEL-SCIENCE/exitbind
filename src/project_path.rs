use std::fs;
#[cfg(unix)]
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

pub fn file(root: &Path, requested: &str) -> Result<PathBuf, String> {
    if requested.contains('\0') || Path::new(requested).is_absolute() {
        return Err(format!("path escapes project root: {requested}"));
    }
    let real_root = fs::canonicalize(root)
        .map_err(|_| format!("project root does not exist: {}", root.display()))?;
    let candidate = normalize_lexical(&root.join(requested));
    let real_candidate = fs::canonicalize(candidate).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("declared file does not exist: {requested}")
        } else {
            error.to_string()
        }
    })?;
    if !real_candidate.starts_with(real_root) {
        return Err(format!("path escapes project root: {requested}"));
    }
    if !real_candidate.is_file() {
        return Err(format!("declared path is not a regular file: {requested}"));
    }
    Ok(real_candidate)
}

pub fn rel(root: &Path, path: &Path) -> Result<String, String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("project root cannot be canonicalized: {error}"))?;
    let path = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::symlink_metadata(path) {
                Ok(_) => return Err("path cannot be canonicalized".into()),
                Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {}
                Err(other) => return Err(other.to_string()),
            }
            let parent = path.parent().ok_or("path has no parent")?;
            let parent = fs::canonicalize(parent)
                .map_err(|error| format!("path parent cannot be canonicalized: {error}"))?;
            parent.join(path.file_name().ok_or("path has no file name")?)
        }
        Err(error) => return Err(error.to_string()),
    };
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| "path escapes project root".to_string())?;
    let rendered = relative
        .to_str()
        .ok_or("project-relative path is not valid UTF-8")?
        .replace('\\', "/");
    if rendered.is_empty() || rendered == "." || rendered == ".." || rendered.starts_with("../") {
        return Err("path escapes project root".into());
    }
    Ok(rendered)
}

pub fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(normalize_lexical(path))
    } else {
        Ok(normalize_lexical(
            &std::env::current_dir()
                .map_err(|error| format!("current directory cannot be resolved: {error}"))?
                .join(path),
        ))
    }
}

pub(crate) enum SecureBytesResult {
    Bytes(Vec<u8>),
    Absent(String),
    Unsafe(String),
    Unreadable(String),
    #[cfg(not(unix))]
    Unsupported(String),
}

impl SecureBytesResult {
    fn into_result(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::Absent(error) | Self::Unsafe(error) | Self::Unreadable(error) => Err(error),
            #[cfg(not(unix))]
            Self::Unsupported(error) => Err(error),
        }
    }
}

/// Read one project-relative regular file through no-follow directory handles.
pub fn secure_bytes(root: &Path, requested: &str, label: &str) -> Result<Vec<u8>, String> {
    secure_bytes_result(root, requested, label, true).into_result()
}

/// Read one project-relative regular file and retain the descriptor-level
/// observation for callers that need to distinguish absence from unsafe paths.
pub(crate) fn secure_bytes_observation(
    root: &Path,
    requested: &str,
    label: &str,
) -> SecureBytesResult {
    secure_bytes_result(root, requested, label, false)
}

#[cfg(unix)]
fn secure_bytes_result(
    root: &Path,
    requested: &str,
    label: &str,
    canonicalize_root: bool,
) -> SecureBytesResult {
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    if requested.trim().is_empty() || requested.contains('\0') || Path::new(requested).is_absolute()
    {
        return SecureBytesResult::Unsafe(format!("path escapes project root: {label}"));
    }
    let components = Path::new(requested)
        .components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::Normal(value) => Some(Ok(value)),
            _ => Some(Err(SecureBytesResult::Unsafe(format!(
                "path escapes project root: {label}"
            )))),
        })
        .collect::<Result<Vec<_>, SecureBytesResult>>();
    let components = match components {
        Ok(components) => components,
        Err(error) => return error,
    };
    let (file_name, parents) = match components.split_last() {
        Some(parts) => parts,
        None => return SecureBytesResult::Unsafe(format!("{label} path must name a project file")),
    };
    let real_root = if canonicalize_root {
        match fs::canonicalize(root) {
            Ok(root) => root,
            Err(_) => {
                return SecureBytesResult::Unreadable(format!(
                    "project root does not exist: {}",
                    root.display()
                ))
            }
        }
    } else {
        root.to_path_buf()
    };
    let mut directory = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(real_root)
    {
        Ok(directory) => directory,
        Err(error) => return classify_io_error(error, label),
    };
    for component in parents {
        let component = CString::new(component.as_bytes())
            .map_err(|_| SecureBytesResult::Unsafe(format!("path escapes project root: {label}")));
        let component = match component {
            Ok(component) => component,
            Err(error) => return error,
        };
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            let error = io::Error::last_os_error();
            return classify_io_error(error, label);
        }
        directory = unsafe { File::from_raw_fd(descriptor) };
    }
    let file_name = CString::new(file_name.as_bytes())
        .map_err(|_| SecureBytesResult::Unsafe(format!("path escapes project root: {label}")));
    let file_name = match file_name {
        Ok(file_name) => file_name,
        Err(error) => return error,
    };
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            file_name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        return classify_io_error(error, label);
    }
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    match file.metadata() {
        Ok(info) if info.is_file() => {}
        Ok(_) => return SecureBytesResult::Unsafe(format!("{label} must be a regular file")),
        Err(error) => return classify_io_error(error, label),
    }
    let mut bytes = Vec::new();
    if let Err(error) = file.read_to_end(&mut bytes) {
        return classify_io_error(error, label);
    }
    if let Err(error) = file.seek(SeekFrom::Start(0)) {
        return classify_io_error(error, label);
    }
    let mut confirmation = Vec::new();
    if let Err(error) = file.read_to_end(&mut confirmation) {
        return classify_io_error(error, label);
    }
    if bytes != confirmation {
        return SecureBytesResult::Unreadable(format!("{label} changed while reading"));
    }
    SecureBytesResult::Bytes(bytes)
}

#[cfg(not(unix))]
fn secure_bytes_result(_: &Path, _: &str, label: &str, _: bool) -> SecureBytesResult {
    SecureBytesResult::Unsupported(format!(
        "{label} secure reading requires Unix no-follow support"
    ))
}

#[cfg(unix)]
fn classify_io_error(error: io::Error, label: &str) -> SecureBytesResult {
    let message = format!("{label}: {error}");
    match error.raw_os_error() {
        Some(libc::ENOENT) => SecureBytesResult::Absent(message),
        Some(libc::ELOOP | libc::ENOTDIR) => SecureBytesResult::Unsafe(message),
        _ => SecureBytesResult::Unreadable(message),
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_allows_only_a_missing_leaf_under_a_real_parent() {
        let root =
            std::env::temp_dir().join(format!("soulmate-project-path-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        assert_eq!(
            rel(&root, &root.join("future.jsonl")).unwrap(),
            "future.jsonl"
        );
        assert!(rel(&root, &root.join("missing/future.jsonl")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn relative_path_rejects_non_utf8_evidence_names() {
        use std::os::unix::ffi::OsStringExt;

        let root =
            std::env::temp_dir().join(format!("soulmate-project-path-utf8-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let path = root.join(std::ffi::OsString::from_vec(vec![b'a', 0xff]));
        assert!(rel(&root, &path).unwrap_err().contains("not valid UTF-8"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn secure_observation_distinguishes_missing_from_unsafe_components() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "soulmate-project-path-secure-{}",
            std::process::id()
        ));
        let outside = root.with_extension("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();

        let requested = ".agents/skills/soulmate/SKILL.md";
        assert!(matches!(
            secure_bytes_observation(&root, requested, "managed project skill"),
            SecureBytesResult::Absent(_)
        ));

        fs::create_dir_all(root.join(".agents")).unwrap();
        fs::write(outside.join("SKILL.md"), b"outside").unwrap();
        symlink(&outside, root.join(".agents/skills")).unwrap();
        assert!(matches!(
            secure_bytes_observation(&root, requested, "managed project skill"),
            SecureBytesResult::Unsafe(_)
        ));

        fs::remove_file(root.join(".agents/skills")).unwrap();
        fs::create_dir_all(root.join(".agents/skills/soulmate")).unwrap();
        symlink(
            outside.join("SKILL.md"),
            root.join(".agents/skills/soulmate/SKILL.md"),
        )
        .unwrap();
        assert!(matches!(
            secure_bytes_observation(&root, requested, "managed project skill"),
            SecureBytesResult::Unsafe(_)
        ));

        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
