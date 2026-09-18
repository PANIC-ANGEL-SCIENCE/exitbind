//! User-level host bridge: the managed Exitbind bootstrap skill that lets an
//! existing coding-agent lead discover Exitbind in any repository after one
//! installation.
//!
//! This is deliberately separate from project projection (`project_skills`) and
//! from hooks (`hooks`). A present bootstrap proves discovery is possible; it
//! never proves that a host selected Exitbind or opened governed work.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const BOOTSTRAP: &str = include_str!("../skills/exitbind-bootstrap/SKILL.md");
const MARKER: &str = "<!-- exitbind-managed-bootstrap:v1 -->";
const VERSION_PREFIX: &str = "<!-- exitbind-bootstrap-version:";

/// Hosts whose native user-level skill directory Exitbind can manage. The
/// Codex location is the installed host's own skills directory, verified
/// against a real installation rather than taken from documentation alone.
const HOSTS: [Host; 2] = [
    Host {
        name: "codex",
        relative: ".codex/skills/exitbind/SKILL.md",
        evidence: ".codex",
    },
    Host {
        name: "claude",
        relative: ".claude/skills/exitbind/SKILL.md",
        evidence: ".claude",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Host {
    pub(crate) name: &'static str,
    relative: &'static str,
    evidence: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BridgeState {
    /// Managed bootstrap equals the bytes this binary ships.
    Current,
    /// Managed bootstrap from another Exitbind version.
    Stale,
    /// Nothing is installed for this host.
    Missing,
    /// Someone else's file occupies the path; Exitbind will not touch it.
    Unmanaged,
    /// The path is a symlink or otherwise unsafe to manage.
    Unsafe,
    Unreadable,
}

impl BridgeState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Unmanaged => "unmanaged",
            Self::Unsafe => "unsafe",
            Self::Unreadable => "unreadable",
        }
    }

    fn writable(self) -> bool {
        matches!(self, Self::Current | Self::Stale | Self::Missing)
    }
}

/// The bootstrap bytes this binary installs, carrying its own version so a
/// stale bridge is visible without guessing.
pub(crate) fn content() -> String {
    format!(
        "{BOOTSTRAP}\n{VERSION_PREFIX} {} -->\n",
        crate::project_skills::package_version()
    )
}

/// Host bridge paths become machine-readable output and host command
/// arguments, so a non-UTF-8 path is an error rather than lossy text.
fn utf8(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("host bridge path is not UTF-8: {}", path.display()))
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is required to manage the host bridge".to_owned())
}

fn selected(hosts: Option<&str>) -> Result<Vec<Host>, String> {
    let Some(hosts) = hosts else {
        return Ok(HOSTS.to_vec());
    };
    let mut selection = Vec::new();
    for name in hosts
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let host = HOSTS
            .iter()
            .find(|host| host.name == name)
            .ok_or_else(|| format!("unsupported host '{name}'; use codex or claude"))?;
        selection.push(*host);
    }
    if selection.is_empty() {
        return Err("--hosts requires at least one host".into());
    }
    Ok(selection)
}

/// A host counts as present when its own configuration directory exists. An
/// absent host is reported, never guessed at or written to.
fn present(home: &Path, host: &Host) -> bool {
    home.join(host.evidence).is_dir()
}

fn observe(path: &Path, expected: &str) -> BridgeState {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BridgeState::Missing,
        Err(_) => BridgeState::Unreadable,
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => BridgeState::Unsafe,
        Ok(_) => match fs::read(path) {
            Err(_) => BridgeState::Unreadable,
            Ok(bytes) if bytes == expected.as_bytes() => BridgeState::Current,
            Ok(bytes) if managed(&bytes) => BridgeState::Stale,
            Ok(_) => BridgeState::Unmanaged,
        },
    }
}

fn managed(bytes: &[u8]) -> bool {
    bytes
        .split(|byte| *byte == b'\n')
        .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == MARKER.as_bytes())
}

fn installed_version(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes).lines().find_map(|line| {
        line.trim()
            .strip_prefix(VERSION_PREFIX)?
            .trim()
            .strip_suffix("-->")
            .map(|version| version.trim().to_owned())
    })
}

/// Per-host bridge facts. `installed` describes files only; it is not a claim
/// that any session discovered or selected Exitbind.
pub(crate) fn status(hosts: Option<&str>) -> Result<Vec<Value>, String> {
    let home = home()?;
    let home_text = utf8(&home)?;
    let expected = content();
    selected(hosts)?
        .into_iter()
        .map(|host| -> Result<Value, String> {
            let path = home.join(host.relative);
            let state = observe(&path, &expected);
            let version = fs::read(&path).ok().as_deref().and_then(installed_version);
            let hook = crate::hooks::manage("status", host.name, home_text)
                .ok()
                .and_then(|items| items.into_iter().next())
                .map_or_else(|| json!("unavailable"), |item| item["state"].clone());
            Ok(json!({
                "host": host.name,
                "hostPresent": present(&home, &host),
                "bootstrapSkill": state.as_str(),
                "activationHook": hook,
                "installedVersion": version,
                "expectedVersion": crate::project_skills::package_version(),
                "path": utf8(&path)?,
            }))
        })
        .collect()
}

/// The user-level activation hook: a fresh session in any repository gets the
/// small bootstrap context, which is what turns a discoverable skill into a
/// selected one. Hook records stay managed and conflicts are reported, never
/// overwritten.
fn install_hook(host: &Host, home: &str) -> Value {
    match crate::hooks::manage("apply", host.name, home) {
        Ok(items) => items
            .into_iter()
            .next()
            .map(|item| {
                if item["blocked"] == Value::Bool(true) {
                    json!({"state": "conflict", "reason": item["reason"]})
                } else {
                    json!({"state": item["state"], "path": item["targetPath"]})
                }
            })
            .unwrap_or_else(|| json!({"state": "unavailable"})),
        Err(error) => json!({"state": "unavailable", "reason": error}),
    }
}

/// Install or refresh the managed bootstrap for the selected hosts. Only
/// missing paths and Exitbind-managed files are written.
pub(crate) fn install(hosts: Option<&str>, all: bool) -> Result<Vec<Value>, String> {
    let home = home()?;
    let home_text = utf8(&home)?.to_owned();
    let expected = content();
    let mut results = Vec::new();
    for host in selected(hosts)? {
        let path = home.join(host.relative);
        let path_text = utf8(&path)?.to_owned();
        let state = observe(&path, &expected);
        let host_present = present(&home, &host);
        let action = if !host_present && !all && hosts.is_none() {
            "skipped_host_absent"
        } else if state == BridgeState::Current {
            "unchanged"
        } else if !state.writable() {
            "refused"
        } else {
            let previous = if state == BridgeState::Missing {
                None
            } else {
                Some(fs::read_to_string(&path).map_err(|error| error.to_string())?)
            };
            write_managed(&home, &path, &expected, previous.as_deref())?;
            if state == BridgeState::Missing {
                "installed"
            } else {
                "refreshed"
            }
        };
        let hook = if action == "skipped_host_absent" {
            json!({"state": "skipped"})
        } else {
            install_hook(&host, &home_text)
        };
        results.push(json!({
            "host": host.name,
            "hostPresent": host_present,
            "action": action,
            "previousState": state.as_str(),
            "path": path_text,
            "activationHook": hook,
        }));
    }
    Ok(results)
}

/// Write through the hardened managed-settings writer: it validates every
/// existing parent component, refuses symlinked parents, creates the temporary
/// file exclusively, and re-checks the target immediately before replacing it,
/// so a concurrent change fails closed instead of clobbering.
fn write_managed(
    home: &Path,
    path: &Path,
    bytes: &str,
    previous: Option<&str>,
) -> Result<(), String> {
    let mode = fs::symlink_metadata(path)
        .ok()
        .map(|info| std::os::unix::fs::PermissionsExt::mode(&info.permissions()) & 0o777);
    let root = fs::canonicalize(home).map_err(|error| error.to_string())?;
    crate::hook_settings::atomic_write(path, bytes, mode, previous, &root)
        .map_err(|error| format!("cannot install host bridge: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_marker_and_version_are_read_from_installed_bytes() {
        let bytes = content();
        assert!(managed(bytes.as_bytes()));
        assert_eq!(
            installed_version(bytes.as_bytes()).as_deref(),
            Some(crate::project_skills::package_version())
        );
        assert!(!managed(b"# someone else's skill\n"));
        assert_eq!(installed_version(b"# no version\n"), None);
    }

    #[test]
    fn host_selection_rejects_unknown_names() {
        assert_eq!(selected(None).unwrap().len(), HOSTS.len());
        assert_eq!(selected(Some("claude")).unwrap()[0].name, "claude");
        assert!(selected(Some("cursor")).is_err());
        assert!(selected(Some(" ")).is_err());
    }

    #[test]
    fn only_missing_or_managed_paths_are_writable() {
        assert!(BridgeState::Missing.writable());
        assert!(BridgeState::Stale.writable());
        assert!(!BridgeState::Unmanaged.writable());
        assert!(!BridgeState::Unsafe.writable());
    }
}
