use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const SOULMATE: &str = include_str!("../skills/soulmate/SKILL.md");
const SOULMATE_REFERENCE: &str = include_str!("../skills/soulmate/references/manual.md");
const EXITBIND: &str = include_str!("../skills/exitbind/SKILL.md");
const EXITBIND_PRESERVATION: &str = include_str!("../skills/exitbind/references/preservation.md");
const COFFEE: &str = include_str!("../skills/coffee/SKILL.md");
const SKILL_MARKER: &str = "<!-- soulmate-managed-skill:v1 -->";
const EXITBIND_SKILL_MARKER: &str = "<!-- exitbind-managed-skill:v1 -->";
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillObservationState {
    Absent,
    EmbeddedMatch,
    ManagedDifferent,
    Unmanaged,
    Unsafe,
    Unreadable,
    #[allow(dead_code)]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillObservation {
    pub(crate) skill: &'static str,
    pub(crate) path: &'static str,
    pub(crate) optional: bool,
    pub(crate) state: SkillObservationState,
    pub(crate) embedded_sha256: String,
    pub(crate) observed_sha256: Option<String>,
}

pub(crate) fn package_version() -> &'static str {
    PACKAGE_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillRefreshState {
    Created,
    Refreshed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillRefreshStatus {
    pub(crate) path: String,
    pub(crate) state: SkillRefreshState,
    pub(crate) skill: &'static str,
    pub(crate) embedded_sha256: String,
}

struct SkillDestination {
    path: PathBuf,
    content: String,
    label: String,
    skill: &'static str,
    state: SkillRefreshState,
}

pub(crate) fn activate(control: &Path, coffee: bool) -> Result<(), String> {
    let destinations = selected_destinations(control, coffee)?;
    if let Some(destination) = destinations
        .iter()
        .find(|destination| destination.state == SkillRefreshState::Refreshed)
    {
        let command = if crate::producer::exitbind_surface() {
            "exitbind"
        } else {
            "soulmate"
        };
        return Err(format!(
            "project skill differs from embedded bytes: {}; inspect it and explicitly run {command} init --refresh-skills --root {}",
            destination.path.display(),
            control.display()
        ));
    }
    for destination in destinations {
        let directory = destination
            .path
            .parent()
            .ok_or("project skill asset has no parent")?;
        crate::managed_files::ensure_managed_directory(control, directory)?;
        if destination.state == SkillRefreshState::Created {
            crate::managed_files::write_exclusive(
                &destination.path,
                destination.content.as_bytes(),
            )?;
        }
    }
    Ok(())
}

pub(crate) fn refresh(control: &Path, coffee: bool) -> Result<Vec<SkillRefreshStatus>, String> {
    let destinations = selected_destinations(control, coffee)?;
    for destination in &destinations {
        let directory = destination
            .path
            .parent()
            .ok_or("project skill asset has no parent")?;
        crate::managed_files::ensure_managed_directory(control, directory)?;
    }
    for destination in &destinations {
        match destination.state {
            SkillRefreshState::Created => crate::managed_files::write_exclusive(
                &destination.path,
                destination.content.as_bytes(),
            )?,
            SkillRefreshState::Refreshed => refresh_owned(&destination.path, &destination.content)?,
            SkillRefreshState::Unchanged => {}
        }
    }
    Ok(destinations
        .into_iter()
        .map(|destination| SkillRefreshStatus {
            path: destination.label,
            state: destination.state,
            skill: destination.skill,
            embedded_sha256: crate::hash::text(&destination.content),
        })
        .collect())
}

pub(crate) fn diagnose(control: &Path) -> Vec<SkillObservation> {
    let exitbind = crate::producer::exitbind_surface();
    let observations = if exitbind {
        vec![
            (
                "Exitbind",
                ".agents/skills/exitbind/SKILL.md",
                EXITBIND,
                false,
            ),
            (
                "Exitbind",
                ".claude/skills/exitbind/SKILL.md",
                EXITBIND,
                false,
            ),
            (
                "Exitbind",
                ".agents/skills/exitbind/references/preservation.md",
                EXITBIND_PRESERVATION,
                false,
            ),
            (
                "Exitbind",
                ".claude/skills/exitbind/references/preservation.md",
                EXITBIND_PRESERVATION,
                false,
            ),
        ]
    } else {
        vec![
            (
                "Soulmate",
                ".agents/skills/soulmate/SKILL.md",
                SOULMATE,
                false,
            ),
            (
                "Soulmate",
                ".claude/skills/soulmate/SKILL.md",
                SOULMATE,
                false,
            ),
            ("Coffee", ".agents/skills/coffee/SKILL.md", COFFEE, true),
            ("Coffee", ".claude/skills/coffee/SKILL.md", COFFEE, true),
        ]
    };
    observations
        .into_iter()
        .map(|(skill, path, content, optional)| {
            inspect_observation(control, skill, path, content, optional)
        })
        .collect()
}

fn inspect_observation(
    control: &Path,
    skill: &'static str,
    relative: &'static str,
    content: &str,
    optional: bool,
) -> SkillObservation {
    let (state, observed_sha256) = inspect_observed_bytes(control, relative, content);
    SkillObservation {
        skill,
        path: relative,
        optional,
        state,
        embedded_sha256: crate::hash::text(content),
        observed_sha256,
    }
}

fn inspect_observed_bytes(
    control: &Path,
    relative: &str,
    content: &str,
) -> (SkillObservationState, Option<String>) {
    match crate::project_path::secure_bytes_observation(control, relative, "managed project skill")
    {
        crate::project_path::SecureBytesResult::Bytes(bytes) => {
            let observed_sha256 = crate::hash::bytes(&bytes);
            let state = if bytes == content.as_bytes() {
                SkillObservationState::EmbeddedMatch
            } else if has_managed_marker(&bytes) {
                SkillObservationState::ManagedDifferent
            } else {
                SkillObservationState::Unmanaged
            };
            (state, Some(observed_sha256))
        }
        crate::project_path::SecureBytesResult::Absent(_) => (SkillObservationState::Absent, None),
        crate::project_path::SecureBytesResult::Unsafe(_) => (SkillObservationState::Unsafe, None),
        crate::project_path::SecureBytesResult::Unreadable(_) => {
            (SkillObservationState::Unreadable, None)
        }
        #[cfg(not(unix))]
        crate::project_path::SecureBytesResult::Unsupported(_) => {
            (SkillObservationState::Unsupported, None)
        }
    }
}

fn has_managed_marker(bytes: &[u8]) -> bool {
    let markers = [SKILL_MARKER.as_bytes(), EXITBIND_SKILL_MARKER.as_bytes()];
    bytes.split(|byte| *byte == b'\n').any(|line| {
        markers.iter().any(|marker| {
            line == *marker || line.strip_suffix(b"\r").is_some_and(|line| line == *marker)
        })
    })
}

fn selected_destinations(control: &Path, coffee: bool) -> Result<Vec<SkillDestination>, String> {
    let exitbind = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == "exitbind")
        })
        .unwrap_or(false);
    if exitbind {
        let mut destinations = Vec::new();
        for base in [".agents/skills", ".claude/skills"] {
            // The delayed preservation detail ships with the skill so a project
            // never has to fetch a separate preservation package.
            for (relative, content) in [
                ("SKILL.md", EXITBIND),
                ("references/preservation.md", EXITBIND_PRESERVATION),
            ] {
                let path = control.join(base).join("exitbind").join(relative);
                validate_managed_directory(
                    control,
                    path.parent().ok_or("project skill asset has no parent")?,
                )?;
                let state = inspect_skill(&path, content)?;
                destinations.push(SkillDestination {
                    path,
                    content: content.to_owned(),
                    label: format!("{base}/exitbind/{relative}"),
                    skill: "Exitbind",
                    state,
                });
            }
        }
        return Ok(destinations);
    }
    let mut assets = vec![
        ("soulmate", "SKILL.md", SOULMATE),
        ("soulmate", "references/manual.md", SOULMATE_REFERENCE),
    ];
    if coffee {
        assets.push(("coffee", "SKILL.md", COFFEE));
    }
    let mut destinations = Vec::new();
    for base in [".agents/skills", ".claude/skills"] {
        for (name, relative, content) in &assets {
            let path = control.join(base).join(name).join(relative);
            validate_managed_directory(
                control,
                path.parent().ok_or("project skill asset has no parent")?,
            )?;
            let state = inspect_skill(&path, content)?;
            destinations.push(SkillDestination {
                path,
                content: content.to_string(),
                label: format!("{base}/{name}/{relative}"),
                skill: name,
                state,
            });
        }
    }
    Ok(destinations)
}

fn validate_managed_directory(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("managed path escapes project root: {}", path.display()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(info) if info.file_type().is_symlink() => {
                return Err(format!(
                    "managed directory must not be a symlink: {}",
                    current.display()
                ))
            }
            Ok(info) if !info.is_dir() => {
                return Err(format!(
                    "managed path must be a directory: {}",
                    current.display()
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
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

fn inspect_skill(path: &Path, content: &str) -> Result<SkillRefreshState, String> {
    match fs::symlink_metadata(path) {
        Ok(info) => {
            if info.file_type().is_symlink() {
                return Err(format!(
                    "project skill must not be a symlink: {}",
                    path.display()
                ));
            }
            if !info.is_file() {
                return Err(format!(
                    "project skill path must be a regular file: {}",
                    path.display()
                ));
            }
            let existing = fs::read_to_string(path).map_err(|e| e.to_string())?;
            if existing == content {
                return Ok(SkillRefreshState::Unchanged);
            }
            // Either managed marker makes this file ours to refresh; anything
            // else is the operator's and is never overwritten.
            if !has_managed_marker(existing.as_bytes()) {
                return Err(format!(
                    "refusing to overwrite existing project skill: {}",
                    path.display()
                ));
            }
            Ok(SkillRefreshState::Refreshed)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SkillRefreshState::Created)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn refresh_owned(path: &Path, content: &str) -> Result<(), String> {
    let info = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let old = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("managed skill path must name a UTF-8 file")?;
    let temp = path.with_file_name(format!(
        ".{}.exitbind-refresh-{}.tmp",
        name,
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().ok();
    drop(file);
    let result = (|| {
        let current = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if current.file_type().is_symlink()
            || !current.is_file()
            || current.len() != info.len()
            || fs::read_to_string(path).map_err(|e| e.to_string())? != old
        {
            return Err(format!(
                "project skill changed during refresh: {}",
                path.display()
            ));
        }
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
