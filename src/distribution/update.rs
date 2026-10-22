use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

const MAX_RELEASE_BYTES: u64 = 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 8 * 1024;
const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn exitbind_surface() -> bool {
    crate::compatibility::is_exitbind()
}

fn api_url() -> &'static str {
    crate::compatibility::profile().api
}

fn raw_origin() -> &'static str {
    crate::compatibility::profile().raw_installer
}

fn cache_name() -> &'static str {
    crate::compatibility::profile().cache
}

fn marker_name() -> &'static str {
    crate::compatibility::profile().marker
}

fn product_name() -> &'static str {
    crate::compatibility::profile().product
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Pre {
    Alpha(u64),
    Beta(u64),
    Rc(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<Pre>,
    tag: String,
}

impl Version {
    fn parse(tag: &str) -> Option<Self> {
        let mut pieces = tag.strip_prefix('v')?.split('-');
        let base = pieces.next()?;
        let suffix = pieces.next();
        if pieces.next().is_some() {
            return None;
        }
        let numbers = base.split('.').collect::<Vec<_>>();
        if numbers.len() != 3
            || numbers
                .iter()
                .any(|n| n.is_empty() || (n.len() > 1 && n.starts_with('0')))
        {
            return None;
        }
        let major = number(numbers[0])?;
        let minor = number(numbers[1])?;
        let patch = number(numbers[2])?;
        let pre = match suffix {
            None => None,
            Some(value) => Some({
                let mut fields = value.split('.');
                let kind = fields.next()?;
                let number = number(fields.next()?)?;
                if fields.next().is_some() || number == 0 {
                    return None;
                }
                match kind {
                    "alpha" => Pre::Alpha(number),
                    "beta" => Pre::Beta(number),
                    "rc" => Pre::Rc(number),
                    _ => return None,
                }
            }),
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
            tag: tag.to_owned(),
        })
    }

    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => pre_key(a).cmp(&pre_key(b)),
            })
    }

    fn channel(&self) -> &'static str {
        if self.pre.is_some() {
            "all"
        } else {
            "stable"
        }
    }
}

fn number(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if value.len() > 1 && value.starts_with('0') {
        return None;
    }
    value.parse().ok()
}

fn pre_key(pre: &Pre) -> (u8, u64) {
    match pre {
        Pre::Alpha(n) => (0, *n),
        Pre::Beta(n) => (1, *n),
        Pre::Rc(n) => (2, *n),
    }
}

fn parse_releases(body: &str, current: &Version) -> Result<Option<Version>, String> {
    let value: Value = serde_json::from_str(body).map_err(|_| "invalid release metadata")?;
    let values = value
        .as_array()
        .ok_or("release metadata was not an array")?;
    Ok(values
        .iter()
        .cloned()
        .filter_map(|release| {
            if release.get("draft")?.as_bool()? {
                return None;
            }
            let tag = release.get("tag_name")?.as_str()?;
            let version = Version::parse(tag)?;
            let marked_pre = release
                .get("prerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if marked_pre != version.pre.is_some() || current.pre.is_none() && version.pre.is_some()
            {
                return None;
            }
            (version.cmp(current) == std::cmp::Ordering::Greater).then_some(version)
        })
        .max_by(|a, b| a.cmp(b)))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cache_is_fresh(checked: u64, current: u64) -> bool {
    checked <= current && current - checked < 86_400
}

fn cache_root() -> Option<PathBuf> {
    cache_root_from(
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn cache_root_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let root = if let Some(value) = xdg {
        value
    } else {
        let home = home?;
        if home == Path::new("/") {
            return None;
        }
        home.join(".cache")
    };
    (root.is_absolute() && root != Path::new("/")).then_some(root)
}

fn paths() -> Option<(PathBuf, PathBuf)> {
    let root = cache_root()?;
    Some((root.join(cache_name()), root.join(marker_name())))
}

fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_file())
        .unwrap_or(false)
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    if !fs::symlink_metadata(parent)
        .map(|meta| meta.file_type().is_dir())
        .unwrap_or(false)
    {
        return Err(io::Error::other("cache parent is not a real directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn read_cache(path: &Path) -> Option<Value> {
    if !regular(path) || fs::metadata(path).ok()?.len() > MAX_CACHE_BYTES {
        return None;
    }
    let mut text = String::new();
    open_read(path).ok()?.read_to_string(&mut text).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get("checked_at")?.as_u64()?;
    value.get("channel")?.as_str()?;
    Some(value)
}

fn open_read(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        return OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path);
    }
    #[cfg(not(unix))]
    {
        File::open(path)
    }
}

fn write_cache(
    path: &Path,
    latest: Option<&str>,
    channel: &str,
    backoff_until: Option<u64>,
) -> io::Result<()> {
    ensure_parent(path)?;
    if fs::symlink_metadata(path).is_ok() && !regular(path) {
        return Err(io::Error::other("cache path is not a regular file"));
    }
    let (temp, mut file) = cache_temp(path)?;
    let result = (|| {
        let mut object = serde_json::Map::new();
        object.insert("checked_at".into(), Value::from(now()));
        object.insert("channel".into(), Value::from(channel));
        if let Some(value) = latest {
            object.insert("latest".into(), Value::from(value));
        }
        if let Some(value) = backoff_until {
            object.insert("backoff_until".into(), Value::from(value));
        }
        serde_json::to_writer(&mut file, &Value::Object(object)).map_err(io::Error::other)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn cache_temp(path: &Path) -> io::Result<(PathBuf, File)> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    let base = path
        .file_name()
        .ok_or_else(|| io::Error::other("cache path has no filename"))?
        .to_str()
        .ok_or_else(|| io::Error::other("cache path has a non-UTF-8 filename"))?;
    let prefix = format!("{base}.tmp-");
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        if fs::symlink_metadata(entry.path())
            .map(|meta| meta.file_type().is_file())
            .unwrap_or(false)
        {
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    for attempt in 0..16 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = parent.join(format!(
            "{base}.tmp-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(error) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                        drop(file);
                        let _ = fs::remove_file(&temp);
                        return Err(error);
                    }
                }
                return Ok((temp, file));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cache temporary path collision",
    ))
}

fn temp_file(label: &str) -> io::Result<PathBuf> {
    for attempt in 0..8 {
        let path = std::env::temp_dir().join(format!(
            "{}-{label}-{}-{attempt}",
            product_name(),
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(fs::Permissions::from_mode(0o600))?;
                }
                drop(file);
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary path collision",
    ))
}

fn curl(url: &str, output: &Path) -> Result<(), String> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--connect-timeout",
            "1",
            "--max-time",
            "3",
            "--max-filesize",
            "1048576",
            "--proto",
            "=https",
            "--tlsv1.2",
            url,
            "--output",
        ])
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("curl unavailable: {e}"))?;
    if status.success() && regular(output) {
        Ok(())
    } else {
        Err("public release lookup failed".into())
    }
}

fn discover(current: &Version) -> Result<Option<Version>, String> {
    let path = temp_file("releases").map_err(|e| e.to_string())?;
    let result = curl(api_url(), &path).and_then(|_| {
        let body = read_release_body(&path)?;
        parse_releases(&body, current)
    });
    let _ = fs::remove_file(path);
    result
}

fn read_release_body(path: &Path) -> Result<String, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_RELEASE_BYTES {
        return Err("release metadata exceeded the bounded response size".into());
    }
    let mut body = String::new();
    open_read(path)
        .map_err(|e| e.to_string())?
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(body)
}

fn cached_or_refresh() -> Option<Version> {
    let Some((cache, marker)) = paths() else {
        return None;
    };
    let current = Version::parse(&format!("v{CURRENT}"));
    let Some(current) = current else {
        return None;
    };
    if let Some(value) = read_cache(&cache) {
        let valid_channel = value.get("channel").and_then(Value::as_str) == Some(current.channel());
        let checked_now = now();
        let fresh = value
            .get("checked_at")
            .and_then(Value::as_u64)
            .is_some_and(|checked| cache_is_fresh(checked, checked_now));
        let not_backed_off = value
            .get("backoff_until")
            .and_then(Value::as_u64)
            .map_or(true, |until| until <= now());
        if let Some(tag) = cached_latest(&value, &current) {
            if fresh && not_backed_off {
                if tag.cmp(&current).is_gt() {
                    return Some(tag);
                }
                return None;
            }
        } else if valid_channel && fresh && value.get("latest").is_none() {
            return None;
        }
    }
    let _lock = acquire_marker(&marker)?;
    let _ = refresh_cache();
    read_cache(&cache).and_then(|value| cached_latest(&value, &current))
}

fn cached_latest(value: &Value, current: &Version) -> Option<Version> {
    if value.get("channel").and_then(Value::as_str) != Some(current.channel()) {
        return None;
    }
    let Some(raw) = value.get("latest").and_then(Value::as_str) else {
        return None;
    };
    let tag = Version::parse(raw)?;
    if current.channel() == "stable" && tag.pre.is_some() {
        return None;
    }
    Some(tag)
}

fn acquire_marker(marker: &Path) -> Option<File> {
    if fs::symlink_metadata(marker)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|age| age > std::time::Duration::from_secs(600))
    {
        let _ = fs::remove_file(marker);
    }
    ensure_parent(marker).ok()?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)
        .ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .ok()?;
    }
    Some(file)
}

pub fn session_context() -> Option<String> {
    if std::env::var_os("EXITBIND_NO_UPDATE_CHECK")
        .or_else(|| std::env::var_os("SOULMATE_NO_UPDATE_CHECK"))
        .is_some()
    {
        return None;
    }
    let current = Version::parse(&format!("v{CURRENT}"))?;
    let latest = cached_or_refresh()?;
    (latest.cmp(&current).is_gt()).then(|| format!("Update available: current {} · latest {}. Tell the user once in the next natural response; do not run it without explicit authorization. Run `{}` update to install.", current.tag.trim_start_matches('v'), latest.tag.trim_start_matches('v'), if exitbind_surface() { "exitbind" } else { "soulmate" }))
}

fn refresh_cache() -> Result<(), String> {
    let Some((cache, marker)) = paths() else {
        return Ok(());
    };
    let current = Version::parse(&format!("v{CURRENT}")).ok_or("invalid current version")?;
    let result = discover(&current);
    let write = match &result {
        Ok(Some(version)) => write_cache(&cache, Some(&version.tag), current.channel(), None),
        Ok(None) => write_cache(&cache, None, current.channel(), None),
        Err(_) => write_cache(&cache, None, current.channel(), Some(now() + 3600)),
    };
    let _ = fs::remove_file(marker);
    write.map_err(|e| e.to_string()).map(|_| ())
}

fn safe_prefix() -> Result<PathBuf, String> {
    let profile = crate::compatibility::profile();
    let compatibility_prefix = if profile.surface == crate::compatibility::Surface::Exitbind {
        "SOULMATE_INSTALL_PREFIX"
    } else {
        "EXITBIND_INSTALL_PREFIX"
    };
    if let Some(prefix) = std::env::var_os(profile.install_prefix_env)
        .or_else(|| std::env::var_os(compatibility_prefix))
        .filter(|v| !v.is_empty())
    {
        let path = PathBuf::from(prefix);
        if !path.is_absolute()
            || path == Path::new("/")
            || !fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_dir())
                .unwrap_or(false)
        {
            return Err("install prefix must be an existing absolute non-root directory".into());
        }
        return Ok(path);
    }
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot locate current executable: {e}"))?;
    let parent = exe
        .parent()
        .ok_or("current executable has no safe install directory")?;
    if !parent.is_absolute()
        || parent == Path::new("/")
        || !fs::metadata(parent).map(|m| m.is_dir()).unwrap_or(false)
    {
        return Err("current executable directory is not a safe install destination".into());
    }
    Ok(parent.to_path_buf())
}

fn remove_target(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return fs::remove_file(path).map_err(|e| e.to_string());
            }
            if !meta.file_type().is_file() {
                return Err("install target is not a regular file".into());
            }
            fs::remove_file(path).map_err(|e| e.to_string())
        }
    }
}

/// Report bridge state without changing a successful binary update.
/// Report bootstrap state without installing anything the operator
/// may have removed on purpose. Only bytes Exitbind already manages are
/// refreshed.
fn bridge_health() -> Vec<String> {
    let Ok(hosts) = crate::host::bridge::status(None) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for host in hosts {
        let name = host["host"].as_str().unwrap_or("unknown").to_owned();
        match host["bootstrapSkill"].as_str().unwrap_or_default() {
            "current" => {}
            "stale" => {
                let refreshed = crate::host::bridge::install(Some(&name), false)
                    .ok()
                    .and_then(|mut items| items.pop())
                    .map(|item| item["action"].as_str().unwrap_or("unknown").to_owned())
                    .unwrap_or_else(|| "unavailable".to_owned());
                lines.push(format!("Host bridge {name}: stale bootstrap {refreshed}"));
            }
            other => {
                if host["hostPresent"] == Value::Bool(true) {
                    let install = crate::emitted::command_text("host install");
                    lines.push(format!(
                        "Host bridge {name}: bootstrap {other}; run '{install}' to restore it"
                    ));
                }
            }
        }
    }
    lines
}

fn delegate_bridge_sync(installed: &Path) -> Vec<String> {
    if !regular(installed) {
        return vec!["Host bridge not synchronized: installed binary is unavailable".to_owned()];
    }
    let output = Command::new(installed)
        .args(["host", "install", "--json"])
        .output();
    let Ok(output) = output else {
        return vec![
            "Host bridge not synchronized: the installed binary could not be run".to_owned(),
        ];
    };
    if !output.status.success() {
        let status = format!(
            "{} host status",
            crate::emitted::word(&installed.display().to_string())
        );
        return vec![format!(
            "Host bridge not synchronized; run '{status}' to inspect it"
        )];
    }
    let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) else {
        return vec!["Host bridge synchronization returned unreadable output".to_owned()];
    };
    value["hosts"]
        .as_array()
        .map(|hosts| {
            hosts
                .iter()
                .map(|host| {
                    format!(
                        "Host bridge {}: {}",
                        host["host"].as_str().unwrap_or("unknown"),
                        host["action"].as_str().unwrap_or("unknown")
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn restore_backup(backup: &Path, target: &Path) -> Result<(), String> {
    if !regular(backup) {
        return Err("rollback backup is missing or not a regular file".into());
    }
    remove_target(target).map_err(|error| format!("cannot remove replacement: {error}"))?;
    fs::copy(backup, target).map_err(|error| format!("cannot restore backup: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(target, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("cannot set restored permissions: {error}"))?;
    }
    fs::remove_file(backup).map_err(|error| format!("cannot remove rollback backup: {error}"))?;
    Ok(())
}

pub fn explicit_update() -> Result<(), String> {
    let current = Version::parse(&format!("v{CURRENT}")).ok_or("invalid current version")?;
    let Some(version) = discover(&current)? else {
        println!("Already up to date ({CURRENT}).");
        // Binary current is not host integration current.
        for line in bridge_health() {
            println!("{line}");
        }
        return Ok(());
    };
    let installer = temp_file("installer").map_err(|e| e.to_string())?;
    let url = format!("{}{}/install.sh", raw_origin(), version.tag);
    let download = curl(&url, &installer);
    if let Err(error) = download {
        let _ = fs::remove_file(&installer);
        return Err(format!("{} update: {error}", product_name()));
    }
    let prefix = match safe_prefix() {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_file(&installer);
            return Err(error);
        }
    };
    let product = crate::compatibility::profile().product;
    let target = prefix.join(product);
    if !regular(&target) {
        let _ = fs::remove_file(&installer);
        return Err(format!(
            "{} update requires an existing regular binary to preserve rollback",
            product_name()
        ));
    }
    let backup = prefix.join(format!(".{product}-old-{}", std::process::id()));
    if fs::symlink_metadata(&backup).is_ok() {
        let _ = fs::remove_file(&installer);
        return Err(format!(
            "{} update found a conflicting rollback path",
            product_name()
        ));
    }
    let backup_setup = (|| -> Result<(), String> {
        fs::copy(&target, &backup).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&backup, fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = backup_setup {
        let _ = fs::remove_file(&backup);
        let _ = fs::remove_file(&installer);
        return Err(format!(
            "{} update backup setup failed: {error}",
            product_name()
        ));
    }
    let result = (|| {
        let output = Command::new("sh")
            .arg(&installer)
            .env("EXITBIND_VERSION", &version.tag)
            .env("EXITBIND_INSTALL_PREFIX", &prefix)
            .env("EXITBIND_REPOSITORY", "PANIC-ANGEL-SCIENCE/exitbind")
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!("installer failed with status {}", output.status));
        }
        let installed = prefix.join("exitbind");
        if !regular(&installed) {
            return Err("installer did not produce a regular Exitbind binary".into());
        }
        let checked = Command::new(&installed)
            .arg("version")
            .output()
            .map_err(|e| e.to_string())?;
        if !checked.status.success()
            || String::from_utf8_lossy(&checked.stdout).trim()
                != version.tag.trim_start_matches('v')
        {
            return Err("installed binary reported an unexpected version".into());
        }
        // Copying a path onto itself truncates it; never do that.
        if !exitbind_surface() && installed != target {
            fs::copy(&installed, &target)
                .map_err(|error| format!("compatibility binary replacement failed: {error}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    })();
    let _ = fs::remove_file(&installer);
    match result {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            println!(
                "Updated {product} to {}.",
                version.tag.trim_start_matches('v')
            );
            // The newly installed binary owns its own host bridge. This
            // Older process: if it synchronized the bridge from
            // its own embedded bootstrap it could write old bytes over the new
            // ones the installer just placed.
            for line in delegate_bridge_sync(&prefix.join("exitbind")) {
                println!("{line}");
            }
            Ok(())
        }
        Err(error) => match restore_backup(&backup, &target) {
            Ok(()) => Err(format!("{product} update: {error}")),
            Err(rollback) => Err(format!(
                "{product} update: {error}; rollback failed: {rollback}; retained backup '{}' for install target '{}'",
                backup.display(),
                target.display()
            )),
        },
    }
}

#[cfg(test)]
#[path = "update/version_policy_tests.rs"]
mod version_policy_tests;

#[cfg(test)]
#[path = "update/tests.rs"]
mod tests;
