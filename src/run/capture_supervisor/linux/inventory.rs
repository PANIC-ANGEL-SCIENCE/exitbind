//! Bounded candidates only. Every action still requires immediate waitid.
use super::*;
const MAX_ENTRIES: usize = 4096;
const MAX_BYTES: usize = 1024 * 1024;

pub(super) fn candidates(deadline: Instant) -> io::Result<Vec<i32>> {
    if fault("inventory") {
        return Err(io::Error::other("observer inventory unavailable"));
    }
    let tgid = std::process::id();
    // Production is single-threaded. The exclusive unit seam additionally has
    // an idle harness main thread; adoption may attach to that main task.
    if !fault("fallback") {
        match File::open(format!("/proc/self/task/{tgid}/children")) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(65537).read_to_end(&mut bytes)?;
                if bytes.len() > 65536 {
                    return Err(io::Error::other("observer child list exceeds bound"));
                }
                let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
                let mut children = parse_list(text)?;
                // In the unit seam, include the sole lifecycle worker task.
                let tid = unsafe { libc::syscall(libc::SYS_gettid) };
                if tid != i64::from(tgid) {
                    let mut bytes = Vec::new();
                    File::open(format!("/proc/self/task/{tid}/children"))?
                        .take(65537)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() > 65536 {
                        return Err(io::Error::other("observer child list exceeds bound"));
                    }
                    children.extend(parse_list(
                        std::str::from_utf8(&bytes).map_err(io::Error::other)?,
                    )?);
                    if children.len() > MAX_ENTRIES {
                        return Err(io::Error::other("observer child count exceeds bound"));
                    }
                }
                return Ok(children);
            }
            Err(e) if matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR)) => {}
            Err(e) => return Err(e),
        }
    }
    // Some Linux kernels omit the optional task/children interface. A scan is
    // a finite candidate inventory, never absence/termination proof or signal
    // authority. Vanished entries are skipped; persistent failures stay unknown.
    let mut children = Vec::new();
    let mut bytes_read = 0usize;
    for (index, entry) in std::fs::read_dir("/proc")?.enumerate() {
        if index >= MAX_ENTRIES || Instant::now() >= deadline {
            return Err(io::Error::other(
                "observer process inventory exceeds its bound",
            ));
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name
            .to_str()
            .filter(|name| name.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        let Ok(pid) = name.parse::<i32>() else {
            continue;
        };
        if pid <= 0 {
            continue;
        }
        let read = || -> io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            File::open(entry.path().join("stat"))?
                .take(4097)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        let bytes = match read() {
            Ok(bytes) => bytes,
            Err(e) if matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => continue,
            Err(e) => return Err(e),
        };
        bytes_read = bytes_read.saturating_add(bytes.len());
        if bytes.len() > 4096 || bytes_read > MAX_BYTES {
            return Err(io::Error::other(
                "observer process inventory bytes exceed bound",
            ));
        }
        if parent(&bytes, pid)? == tgid {
            children.push(pid);
        }
    }
    Ok(children)
}

fn parse_list(text: &str) -> io::Result<Vec<i32>> {
    let mut out = Vec::new();
    for entry in text.split_ascii_whitespace() {
        let pid = entry.parse::<i32>().map_err(io::Error::other)?;
        if pid <= 0 || out.len() >= MAX_ENTRIES {
            return Err(io::Error::other("invalid observer child inventory"));
        }
        out.push(pid);
    }
    Ok(out)
}

fn parent(bytes: &[u8], pid: i32) -> io::Result<u32> {
    let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
    let (identity, rest) = text
        .split_once('(')
        .ok_or_else(|| io::Error::other("invalid process candidate identity"))?;
    if identity.trim().parse::<i32>().ok() != Some(pid) {
        return Err(io::Error::other("process candidate identity changed"));
    }
    let (_, fields) = rest
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("invalid process candidate comm"))?;
    fields
        .split_ascii_whitespace()
        .nth(1)
        .ok_or_else(|| io::Error::other("missing candidate parent"))?
        .parse()
        .map_err(io::Error::other)
}

#[cfg(test)]
#[test]
fn candidate_parent_handles_spaces_and_parentheses_without_authority() {
    assert_eq!(parent(b"17 (a ) nested (name)) S 23 0 0", 17).unwrap(), 23);
    assert!(parent(b"18 (name) S 23", 17).is_err());
    assert!(parent(b"17 missing comm", 17).is_err());
    assert!(parse_list("1 2 nope").is_err());
}
