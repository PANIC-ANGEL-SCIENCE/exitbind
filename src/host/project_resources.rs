use serde_json::{json, Value};
use std::path::Path;

const OS_SOURCE: &str = "std::env::consts::OS";
const ARCHITECTURE_SOURCE: &str = "std::env::consts::ARCH";
const LOGICAL_CPU_SOURCE: &str = "libc::sysconf(_SC_NPROCESSORS_ONLN)";
const PARALLELISM_SOURCE: &str = "std::thread::available_parallelism";
const FILESYSTEM_SOURCE: &str = "statvfs f_bavail * f_frsize at project root";

pub(crate) fn observe(project_root: &Path) -> Value {
    json!({
        "version": 1,
        "observedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "os": known(OS_SOURCE, json!(std::env::consts::OS)),
        "architecture": known(ARCHITECTURE_SOURCE, json!(std::env::consts::ARCH)),
        "logicalCpuCount": logical_cpu_count(),
        "processAvailableParallelism": process_available_parallelism(),
        "filesystemAvailableBytes": filesystem_available_observation(project_root),
    })
}

fn known(source: &str, value: Value) -> Value {
    json!({"state":"known", "source":source, "value":value})
}

fn unknown(source: &str, reason: &'static str) -> Value {
    json!({"state":"unknown", "source":source, "reason":reason})
}

#[cfg(unix)]
fn logical_cpu_count() -> Value {
    let count = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    logical_cpu_observation(count)
}

#[cfg(unix)]
fn logical_cpu_observation(count: libc::c_long) -> Value {
    if count == -1 {
        return unknown(LOGICAL_CPU_SOURCE, "sysconf returned -1");
    }
    if count <= 0 {
        return unknown(LOGICAL_CPU_SOURCE, "sysconf returned a non-positive count");
    }
    match u64::try_from(count) {
        Ok(count) => known(LOGICAL_CPU_SOURCE, json!(count)),
        Err(_) => unknown(LOGICAL_CPU_SOURCE, "logical CPU count is out of range"),
    }
}

#[cfg(not(unix))]
fn logical_cpu_count() -> Value {
    unknown(LOGICAL_CPU_SOURCE, "unsupported platform")
}

fn process_available_parallelism() -> Value {
    match std::thread::available_parallelism() {
        Ok(count) => match u64::try_from(count.get()) {
            Ok(count) => known(PARALLELISM_SOURCE, json!(count)),
            Err(_) => unknown(PARALLELISM_SOURCE, "parallelism count is out of range"),
        },
        Err(_) => unknown(PARALLELISM_SOURCE, "available parallelism is unavailable"),
    }
}

#[cfg(unix)]
fn filesystem_available_observation(project_root: &Path) -> Value {
    filesystem_observation(filesystem_available_bytes(project_root))
}

#[cfg(not(unix))]
fn filesystem_available_observation(_: &Path) -> Value {
    unknown(FILESYSTEM_SOURCE, "unsupported platform")
}

#[cfg(unix)]
// libc's statvfs field aliases vary by Unix target; retain checked conversion.
#[allow(clippy::useless_conversion)]
fn filesystem_available_bytes(project_root: &Path) -> Result<u64, &'static str> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(project_root.as_os_str().as_bytes())
        .map_err(|_| "project path contains NUL")?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err("statvfs failed");
    }
    let stats = unsafe { stats.assume_init() };
    let available_blocks =
        u64::try_from(stats.f_bavail).map_err(|_| "available block count is out of range")?;
    let fragment_size =
        u64::try_from(stats.f_frsize).map_err(|_| "filesystem fragment size is out of range")?;
    checked_available_bytes(available_blocks, fragment_size)
}

fn checked_available_bytes(available_blocks: u64, fragment_size: u64) -> Result<u64, &'static str> {
    if fragment_size == 0 {
        return Err("filesystem fragment size is zero");
    }
    available_blocks
        .checked_mul(fragment_size)
        .ok_or("available byte count overflow")
}

fn filesystem_observation(result: Result<u64, &'static str>) -> Value {
    match result {
        Ok(bytes) => known(FILESYSTEM_SOURCE, json!(bytes)),
        Err(reason) => unknown(FILESYSTEM_SOURCE, reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_available_bytes_are_known_and_failures_are_unknown() {
        let full_filesystem = filesystem_observation(Ok(0));
        let failed_filesystem = filesystem_observation(Err("statvfs failed"));

        assert_eq!(full_filesystem["state"], "known");
        assert_eq!(full_filesystem["value"], 0);
        assert_eq!(failed_filesystem["state"], "unknown");
        assert_eq!(failed_filesystem["reason"], "statvfs failed");
    }

    #[test]
    fn available_byte_overflow_is_unknown() {
        let observation = filesystem_observation(checked_available_bytes(u64::MAX, 2));

        assert_eq!(observation["state"], "unknown");
        assert_eq!(observation["reason"], "available byte count overflow");
    }

    #[cfg(unix)]
    #[test]
    fn nul_path_is_unknown_instead_of_observing_a_truncated_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(OsStr::from_bytes(b"/tmp\0/other"));
        let observation = filesystem_available_observation(path);

        assert_eq!(observation["state"], "unknown");
        assert_eq!(observation["reason"], "project path contains NUL");
    }

    #[cfg(unix)]
    #[test]
    fn invalid_sysconf_counts_are_unknown() {
        assert_eq!(logical_cpu_observation(-1)["state"], "unknown");
        assert_eq!(logical_cpu_observation(0)["state"], "unknown");
        assert_eq!(logical_cpu_observation(2)["value"], 2);
    }

    #[test]
    fn observation_has_bounded_fields_for_the_project_root() {
        let facts = observe(Path::new(env!("CARGO_MANIFEST_DIR")));

        assert_eq!(facts["version"], 1);
        assert!(facts["observedAt"].as_str().is_some());
        assert_eq!(facts["os"]["value"], std::env::consts::OS);
        assert_eq!(facts["architecture"]["value"], std::env::consts::ARCH);
        for field in [
            "os",
            "architecture",
            "logicalCpuCount",
            "processAvailableParallelism",
            "filesystemAvailableBytes",
        ] {
            let observation = facts[field].as_object().expect("observation object");
            assert!(observation["source"].as_str().is_some());
            match observation["state"].as_str() {
                Some("known") => assert!(
                    observation["value"].as_u64().is_some()
                        || observation["value"].as_str().is_some()
                ),
                Some("unknown") => assert!(observation["reason"].as_str().is_some()),
                _ => panic!("invalid observation state for {field}"),
            }
        }
        assert!(
            facts["filesystemAvailableBytes"]["value"]
                .as_u64()
                .is_some()
                || facts["filesystemAvailableBytes"]["state"] == "unknown"
        );
    }
}
