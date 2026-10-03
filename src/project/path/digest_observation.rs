//! Two descriptor reads with a fixed buffer; no retained guard content.
use super::*;

#[cfg(unix)]
pub(crate) fn secure_digest_observation(
    root: &Path,
    requested: &str,
    label: &str,
) -> Result<Option<String>, String> {
    observe(root, requested, label, || {})
}

#[cfg(not(unix))]
pub(crate) fn secure_digest_observation(
    _: &Path,
    _: &str,
    label: &str,
) -> Result<Option<String>, String> {
    Err(format!(
        "{label} secure reading requires Unix no-follow support"
    ))
}

#[cfg(unix)]
fn observe(
    root: &Path,
    requested: &str,
    label: &str,
    between: impl FnOnce(),
) -> Result<Option<String>, String> {
    use std::os::unix::fs::MetadataExt;
    let mut file = match secure_open(root, requested, label, false, false) {
        Ok(file) => file,
        Err(SecureBytesResult::Absent(_)) => return Ok(None),
        Err(error) => return error.into_result().map(|_| None),
    };
    let identity = |info: &fs::Metadata| {
        (
            info.dev(),
            info.ino(),
            info.len(),
            info.mtime(),
            info.mtime_nsec(),
            info.ctime(),
            info.ctime_nsec(),
        )
    };
    let before = file
        .metadata()
        .map_err(|error| format!("{label}: {error}"))?;
    let first = digest(&mut file, label)?;
    between();
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("{label}: {error}"))?;
    let second = digest(&mut file, label)?;
    let after = file
        .metadata()
        .map_err(|error| format!("{label}: {error}"))?;
    let reopened = secure_open(root, requested, label, false, false)
        .map_err(|_| format!("{label} path changed while reading"))?;
    let current = reopened
        .metadata()
        .map_err(|error| format!("{label}: {error}"))?;
    if first != second
        || identity(&before) != identity(&after)
        || identity(&after) != identity(&current)
    {
        return Err(format!("{label} changed while reading"));
    }
    Ok(Some(first))
}

#[cfg(unix)]
fn digest(file: &mut fs::File, label: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("{label}: {error}")),
        };
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn streaming_digest_rejects_change_replacement_and_unsafe_paths() {
        let root = std::env::temp_dir().join(format!("exitbind-streaming-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        for size in [0, 1, 32767, 32768, 32769, 8 * 1024 * 1024] {
            let bytes = vec![0xff; size];
            fs::write(root.join("guard"), &bytes).unwrap();
            assert_eq!(
                secure_digest_observation(&root, "guard", "guard").unwrap(),
                Some(crate::evidence::hash::bytes(&bytes))
            );
        }
        assert_eq!(
            secure_digest_observation(&root, "missing", "guard").unwrap(),
            None
        );
        assert!(secure_digest_observation(&root, ".", "guard").is_err());
        assert!(observe(&root, "guard", "guard", || fs::write(
            root.join("guard"),
            b"changed"
        )
        .unwrap())
        .is_err());
        assert!(observe(&root, "guard", "guard", || {
            fs::write(root.join("replacement"), b"changed").unwrap();
            fs::rename(root.join("replacement"), root.join("guard")).unwrap();
        })
        .is_err());
        std::os::unix::fs::symlink("guard", root.join("link")).unwrap();
        assert!(secure_digest_observation(&root, "link", "guard").is_err());
        assert!(secure_digest_observation(&root, "../guard", "guard").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
