use crate::config::{self, Loaded};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_SOURCE: u64 = 64 * 1024;
const CANDIDATES: [&str; 4] = [
    "Identity/runtime-brief.md",
    "runtime-brief.md",
    "AGENT.md",
    "AGENTS.md",
];

pub fn audit(path: &str, term: Option<&str>) -> Result<Value, String> {
    if term.is_some_and(|value| value.is_empty()) {
        return Err("--forbid-term requires a non-empty value".into());
    }
    let (selected, label) = select_source(path)?;
    let (bytes, source) = read_source(&selected)?;
    let findings = findings(&source, term);
    Ok(json!({"valid":findings.is_empty(),"source":label,"bytes":bytes.len(),"findings":findings}))
}

pub fn import(
    l: &Loaded,
    name: &str,
    source: &str,
    purpose: &str,
    term: Option<&str>,
) -> Result<String, String> {
    if !config::is_name(Some(name)) {
        return Err("profile name is not portable".into());
    }
    if purpose.trim().is_empty() || purpose.contains('\0') {
        return Err("profile import requires --purpose".into());
    }
    let (source_path, _) = select_source(source)?;
    let (source_bytes, source_text) = read_source(&source_path)?;
    let first_findings = findings(&source_text, term);
    if !first_findings.is_empty() {
        return Err(format!(
            "profile audit failed:\n{}",
            format_findings(&first_findings)
        ));
    }
    let mut next = l.config.clone();
    if next["agents"].get(name).is_some() {
        return Err(format!("agent '{name}' already exists"));
    }
    let proposed = json!({"profile":format!("{}/{name}.md", crate::project::layout_types::agents_dir()),"purpose":purpose.trim(),"memoryForget":[],"observe":[],"write":[],"commands":[],"skills":[],"memoryRead":[],"memoryWrite":[],"memoryReview":[],"memoryPromote":[],"memoryReject":[],"memoryRevoke":[],"memoryExpire":[],"retention":"task","crossContext":"none"});
    next["agents"][name] = proposed;
    if has_native_collision(&next, name) || !config::validate(&next).is_empty() {
        return Err("proposed configuration validation failed".into());
    }

    let project = fs::canonicalize(&l.control_root).map_err(|e| e.to_string())?;
    let config_path = regular(&l.path, "configuration")?;
    let profiles_dir = project.join(crate::project::layout_types::agents_dir());
    ensure_dir(&project, &profiles_dir)?;
    let destination = profiles_dir.join(format!("{name}.md"));
    ensure_absent(&destination, "profile target")?;
    unchanged(&config_path, &l.source)?;
    let (rechecked_bytes, _) = read_source(&source_path)?;
    if rechecked_bytes != source_bytes {
        return Err("source changed during import".into());
    }

    let profile_temp = profiles_dir.join(format!(".{name}.md.exitbind-{}.tmp", std::process::id()));
    let config_name = l
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("configuration path must name a UTF-8 file")?;
    let config_temp = l.path.with_file_name(format!(
        ".{config_name}.exitbind-{}.tmp",
        std::process::id()
    ));
    let mut profile_written = false;
    let mut config_written = false;
    let result: Result<(), String> = (|| {
        write_temp(&profile_temp, &source_bytes)?;
        profile_written = true;
        ensure_absent(&destination, "profile target")?;
        fs::rename(&profile_temp, &destination).map_err(|e| e.to_string())?;
        let (final_bytes, _) = read_source(&source_path)?;
        if final_bytes != source_bytes {
            return Err("source changed during import".into());
        }
        unchanged(&config_path, &l.source)?;
        let mut config_source =
            serde_json::to_string_pretty(&next).map_err(|error| error.to_string())?;
        config_source.push('\n');
        write_temp(&config_temp, config_source.as_bytes())?;
        config_written = true;
        unchanged(&config_path, &l.source)?;
        fs::rename(&config_temp, &config_path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        if profile_written {
            let _ = remove_if_same(&destination, &source_bytes);
        }
        if config_written {
            let _ = fs::remove_file(&config_temp);
        }
        let _ = fs::remove_file(&profile_temp);
        let _ = fs::remove_file(&config_temp);
    }
    result?;
    Ok(format!(
        "Imported profile {name} -> {}/{name}.md\n",
        crate::project::layout_types::agents_dir()
    ))
}

fn select_source(path: &str) -> Result<(PathBuf, String), String> {
    if path.trim().is_empty() || path.contains('\0') {
        return Err("profile source must be a non-empty path".into());
    }
    let requested = absolute(Path::new(path))?;
    let info = fs::symlink_metadata(&requested)
        .map_err(|e| format!("profile source does not exist: {e}"))?;
    if info.file_type().is_symlink() {
        return Err("profile source must not be a symlink".into());
    }
    if info.is_dir() {
        for candidate in CANDIDATES {
            let selected = requested.join(candidate);
            if fs::symlink_metadata(&selected).is_ok() {
                return Ok((selected, candidate.into()));
            }
        }
        return Err("profile source directory has no supported candidate".into());
    }
    if !info.is_file() {
        return Err("profile source must be a regular file or directory".into());
    }
    let label = requested
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("profile source must name a UTF-8 file")?
        .to_owned();
    Ok((requested, label))
}

fn read_source(path: &Path) -> Result<(Vec<u8>, String), String> {
    let info = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if info.file_type().is_symlink() {
        return Err("profile source must not be a symlink".into());
    }
    if !info.is_file() {
        return Err("profile source must be a regular file".into());
    }
    if info.len() > MAX_SOURCE {
        return Err(format!("profile source exceeds {MAX_SOURCE} bytes"));
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SOURCE {
        return Err(format!("profile source exceeds {MAX_SOURCE} bytes"));
    }
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| "profile source must be valid UTF-8".to_string())?;
    Ok((bytes, text))
}

fn findings(source: &str, term: Option<&str>) -> Vec<Value> {
    let mut result = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let checks = [
            ("private-home-path", private_home(line)),
            ("terminal-prompt", terminal_prompt(line)),
            ("private-key-material", private_key(&lower)),
            ("credential-token-signature", credential_signature(line)),
            ("credential-assignment", credential_assignment(line)),
            ("project-coupled-path", project_path(line)),
        ];
        for (category, hit) in checks {
            if hit {
                result.push(json!({"category":category,"line":line_index + 1}));
            }
        }
        if let Some(term) = term {
            if lower.contains(&term.to_ascii_lowercase()) {
                result.push(json!({"category":"forbidden-term","line":line_index + 1}));
            }
        }
    }
    result.sort_by(|a, b| {
        a["line"]
            .as_u64()
            .cmp(&b["line"].as_u64())
            .then_with(|| a["category"].as_str().cmp(&b["category"].as_str()))
    });
    result
}

fn private_home(line: &str) -> bool {
    for marker in ["/home/", "/Users/"] {
        if let Some(index) = line.find(marker) {
            let rest = &line[index + marker.len()..];
            if rest
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return true;
            }
        }
    }
    line.find(":\\Users\\").is_some_and(|index| {
        line[index + 8..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
    })
}
fn terminal_prompt(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("PS ") && trimmed.contains(":\\") && trimmed.contains('>') {
        return true;
    }
    let Some(at) = trimmed.find('@') else {
        return false;
    };
    let host = &trimmed[..at];
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return false;
    }
    trimmed.contains('$') || trimmed.contains('#') || trimmed.contains('%')
}
fn credential_signature(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("github_pat_")
        || lower
            .split_whitespace()
            .any(|word| (word.starts_with("akia") || word.starts_with("asia")) && word.len() >= 20)
        || lower.contains("bearer ")
        || lower.contains("aiza")
}
fn private_key(lower: &str) -> bool {
    let Some(begin) = lower.find("begin ") else {
        return false;
    };
    lower[begin..].contains(" private key")
}
fn credential_assignment(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    for key in [
        "api_key",
        "api-key",
        "access_token",
        "access-token",
        "auth_token",
        "password",
        "secret",
        "token",
    ] {
        if let Some(index) = lower.find(key) {
            let tail = &lower[index + key.len()..];
            if tail
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| c == ':' || c == '=')
                && tail
                    .chars()
                    .filter(|c| {
                        !c.is_whitespace() && *c != ':' && *c != '=' && *c != '"' && *c != '\''
                    })
                    .count()
                    >= 12
            {
                return true;
            }
        }
    }
    false
}
fn project_path(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "/agents/",
        "/system/",
        "/lab/",
        "/vision/",
        "/vault/",
        "/workspaces/",
        "/runtime/",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || [
            "Agents/",
            "System/",
            "Lab/",
            "Vision/",
            "Vault/",
            "Workspaces/",
            "Runtime/",
        ]
        .iter()
        .any(|needle| line.contains(needle))
}

fn has_native_collision(config: &Value, added: &str) -> bool {
    let Some(agents) = config["agents"].as_object() else {
        return true;
    };
    let added_name = config::native_name(added, &agents[added]);
    if added_name.is_empty()
        || added_name.len() > 64
        || !added_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return true;
    }
    agents
        .iter()
        .filter(|(name, _)| name.as_str() != added)
        .any(|(name, agent)| config::native_name(name, agent) == added_name)
}
fn format_findings(findings: &[Value]) -> String {
    findings
        .iter()
        .map(|value| {
            format!(
                "- {} (line {})",
                value["category"].as_str().unwrap_or("unknown"),
                value["line"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn ensure_dir(root: &Path, path: &Path) -> Result<(), String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| "profile path escapes project root".to_string())?;
    let mut current = root.to_path_buf();
    for component in rel.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(info) => {
                if info.file_type().is_symlink() || !info.is_dir() {
                    return Err(format!(
                        "managed profile directory must be a non-symlink directory: {}",
                        current.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
fn regular(path: &Path, label: &str) -> Result<PathBuf, String> {
    let info = fs::symlink_metadata(path).map_err(|_| format!("{label} does not exist"))?;
    if info.file_type().is_symlink() || !info.is_file() {
        return Err(format!("{label} must be a non-symlink regular file"));
    }
    Ok(path.to_path_buf())
}
fn ensure_absent(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(info) if info.file_type().is_symlink() => Err(format!("{label} must not be a symlink")),
        Ok(_) => Err(format!("{label} already exists")),
        Err(error) => Err(error.to_string()),
    }
}
fn unchanged(path: &Path, expected: &str) -> Result<(), String> {
    let _ = regular(path, "configuration")?;
    let current = fs::read(path).map_err(|e| e.to_string())?;
    if current != expected.as_bytes() {
        return Err("configuration drift detected".into());
    }
    Ok(())
}
fn write_temp(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().ok();
    Ok(())
}
fn remove_if_same(path: &Path, expected: &[u8]) -> Result<(), String> {
    if fs::read(path).map_err(|e| e.to_string())? == expected {
        fs::remove_file(path).map_err(|e| e.to_string())
    } else {
        Err("imported profile was replaced; cleanup refused".into())
    }
}
fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|error| format!("current directory cannot be resolved: {error}"))?
            .join(path))
    }
}
