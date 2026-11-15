// Observes a secure file digest using bounded descriptor reads.
// Refuses bytes or identity changed during observation; see `src/project/path.rs`.
//! Two descriptor reads with a fixed buffer; optional bounded page retention.
use super::*;

pub(crate) struct ObservedPage {
    pub(crate) sha256: String,
    pub(crate) total_bytes: u64,
    pub(crate) bytes: Vec<u8>,
}

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
    Ok(observe_page(root, requested, label, 0, 0, between)?.map(|page| page.sha256))
}

/// Read an exact byte window while hashing the entire current file twice.
/// Only the requested window is retained in memory.
#[cfg(unix)]
pub(crate) fn secure_page_observation(
    root: &Path,
    requested: &str,
    label: &str,
    offset: u64,
    limit: usize,
) -> Result<Option<ObservedPage>, String> {
    observe_page(root, requested, label, offset, limit, || {})
}

#[cfg(not(unix))]
pub(crate) fn secure_page_observation(
    _: &Path,
    _: &str,
    label: &str,
    _: u64,
    _: usize,
) -> Result<Option<ObservedPage>, String> {
    Err(format!(
        "{label} secure reading requires Unix no-follow support"
    ))
}

#[cfg(unix)]
fn observe_page(
    root: &Path,
    requested: &str,
    label: &str,
    offset: u64,
    limit: usize,
    between: impl FnOnce(),
) -> Result<Option<ObservedPage>, String> {
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
    let first = digest(&mut file, label, offset, limit)?;
    between();
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("{label}: {error}"))?;
    let second = digest(&mut file, label, offset, 0)?;
    let after = file
        .metadata()
        .map_err(|error| format!("{label}: {error}"))?;
    let reopened = secure_open(root, requested, label, false, false)
        .map_err(|_| format!("{label} path changed while reading"))?;
    let current = reopened
        .metadata()
        .map_err(|error| format!("{label}: {error}"))?;
    if first.sha256 != second.sha256
        || first.total_bytes != second.total_bytes
        || identity(&before) != identity(&after)
        || identity(&after) != identity(&current)
    {
        return Err(format!("{label} changed while reading"));
    }
    Ok(Some(first))
}

#[cfg(unix)]
fn digest(
    file: &mut fs::File,
    label: &str,
    offset: u64,
    limit: usize,
) -> Result<ObservedPage, String> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 32 * 1024];
    let mut total = 0u64;
    let end = offset.saturating_add(limit as u64);
    let mut bytes = Vec::with_capacity(limit.min(buffer.len()));
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
        let chunk_end = total
            .checked_add(read as u64)
            .ok_or("file length overflow")?;
        let start = offset.max(total);
        let stop = end.min(chunk_end);
        if start < stop {
            bytes.extend_from_slice(&buffer[(start - total) as usize..(stop - total) as usize]);
        }
        total = chunk_end;
    }
    Ok(ObservedPage {
        sha256: format!("{:x}", digest.finalize()),
        total_bytes: total,
        bytes,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn streaming_digest_rejects_change_replacement_and_unsafe_paths() {
        let root = crate::test_root::temp("streaming");
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

    #[test]
    fn bounded_page_is_bound_to_both_complete_reads() {
        let root = crate::test_root::temp("streaming-page");
        let bytes = vec![b'a'; 128 * 1024];
        fs::write(root.join("profile"), &bytes).unwrap();
        let page = secure_page_observation(&root, "profile", "profile", 4095, 4096)
            .unwrap()
            .unwrap();
        assert_eq!(page.total_bytes, bytes.len() as u64);
        assert_eq!(page.bytes, bytes[4095..8191]);
        assert_eq!(page.sha256, crate::evidence::hash::bytes(&bytes));
        assert!(observe_page(&root, "profile", "profile", 0, 4096, || {
            fs::write(root.join("profile"), b"changed").unwrap();
        })
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
