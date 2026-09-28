use std::collections::BTreeSet;
mod support;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const IGNORED: &[&str] = &[".git", "node_modules", ".cache", "coverage", "target"];
const ALLOWED_NAME: &str = "envy404";
const ALLOWED_EMAIL: &str = "legacy-operator@users.noreply.github.com";
const ALLOWED_ACCOUNT_ALIAS_NAME: &str = "envy404";
const ALLOWED_ACCOUNT_ALIAS_EMAIL: &str = "287204764+envy404@users.noreply.github.com";
const GITHUB_COMMITTER_NAME: &str = "GitHub";
const GITHUB_COMMITTER_EMAIL: &str = "noreply@github.com";

type Findings = BTreeSet<(String, String)>;

fn scan(root: &Path) -> Findings {
    let mut findings = Findings::new();
    let inside_git =
        git(root, &["rev-parse", "--is-inside-work-tree"]).is_some_and(|bytes| bytes == b"true\n");
    let marker = root.join(".git").exists();
    if inside_git {
        let tracked = git(root, &["ls-files", "--cached", "-z"]);
        let candidates = git(
            root,
            &[
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ],
        );
        if tracked.is_none() {
            add(&mut findings, "scan-incomplete", "git-tracked-enumeration");
        }
        if let Some(candidates) = candidates {
            let tracked = nul_paths(tracked.as_deref().unwrap_or_default())
                .into_iter()
                .collect::<BTreeSet<_>>();
            for path in nul_paths(&candidates) {
                let source = if tracked.contains(&path) {
                    "tracked-file"
                } else {
                    "untracked-file"
                };
                scan_file(root, &path, source, &mut findings);
            }
        } else {
            add(&mut findings, "scan-incomplete", "git-file-enumeration");
        }

        let metadata = git(
            root,
            &[
                "log",
                "--all",
                "HEAD",
                "--full-history",
                "--use-mailmap",
                "--pretty=format:%H%x00%aN%x00%aE%x00%cn%x00%ce%x00",
            ],
        );
        let patches = git(
            root,
            &[
                "log",
                "--all",
                "HEAD",
                "--full-history",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--format=",
                "-p",
            ],
        );
        let synthetic_merge = pull_request_merge(root);
        match metadata {
            Some(bytes) => scan_metadata(&bytes, synthetic_merge.as_deref(), &mut findings),
            None => add(&mut findings, "scan-incomplete", "git-history-metadata"),
        }
        match patches {
            Some(bytes) => scan_patch(&bytes, &mut findings),
            None => add(&mut findings, "scan-incomplete", "git-history-patches"),
        }
    } else {
        if marker {
            add(&mut findings, "scan-incomplete", "git-probe");
        }
        let mut files = Vec::new();
        collect_files(root, root, &mut files, &mut findings);
        files.sort();
        for path in files {
            scan_file(root, &path, "candidate-file", &mut findings);
        }
    }
    findings
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>, findings: &mut Findings) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => {
            add(findings, "scan-incomplete", "candidate-directory");
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            add(findings, "scan-incomplete", "candidate-directory");
            continue;
        };
        if kind.is_symlink()
            || (kind.is_dir() && IGNORED.contains(&entry.file_name().to_str().unwrap_or("")))
        {
            continue;
        }
        if kind.is_dir() {
            collect_files(root, &path, files, findings);
        } else if kind.is_file() {
            match path.strip_prefix(root) {
                Ok(relative) => files.push(relative.to_path_buf()),
                Err(_) => add(findings, "scan-incomplete", "candidate-directory"),
            }
        }
    }
}

fn scan_file(root: &Path, relative: &Path, source: &str, findings: &mut Findings) {
    scan_path(relative, source, findings);
    if private_filename(relative) {
        return;
    }
    let path = root.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => {
            add(findings, "scan-incomplete", source);
            return;
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return;
    }
    match fs::read(path) {
        Ok(bytes) if !bytes.iter().take(8192).any(|byte| *byte == 0) => {
            scan_text(&String::from_utf8_lossy(&bytes), source, findings)
        }
        Ok(_) => {}
        Err(_) => add(findings, "scan-incomplete", source),
    }
}

fn scan_path(path: &Path, source: &str, findings: &mut Findings) {
    if private_filename(path) {
        add(findings, "private-config-filename", source);
    }
    scan_text(&path.to_string_lossy(), source, findings);
}

fn scan_text(text: &str, source: &str, findings: &mut Findings) {
    if private_home(text) {
        add(findings, "private-home-path", source);
    }
    if text.split_whitespace().any(|word| {
        word.split_once('@').is_some_and(|(user, host)| {
            !user.is_empty()
                && user.chars().all(identity_character)
                && host.trim_matches(|character: char| !identity_character(character))
                    == "localhost"
        })
    }) {
        add(findings, "terminal-prompt", source);
    }
    let key_begin = ["-----", "BEGIN "].concat();
    let key_kind = ["PRIVATE", " KEY"].concat();
    if text.lines().any(|line| {
        line.find(&key_begin).is_some_and(|begin| {
            let tail = &line[begin + key_begin.len()..];
            tail.find(&key_kind).is_some_and(|kind| {
                tail[..kind].chars().all(|character| {
                    character.is_ascii_uppercase()
                        || character.is_ascii_digit()
                        || matches!(character, ' ' | '-')
                })
            })
        })
    }) {
        add(findings, "private-key-material", source);
    }
    if credential_signature(text) {
        add(findings, "credential-token-signature", source);
    }
    if credential_assignment(text) {
        add(findings, "credential-assignment", source);
    }
}

fn private_home(text: &str) -> bool {
    for marker in ["/home/", "/Users/"] {
        let mut rest = text;
        while let Some(index) = rest.find(marker) {
            let account = rest[index + marker.len()..]
                .split(|character: char| !identity_character(character))
                .next()
                .unwrap_or("");
            if !account.is_empty()
                && !["private", "user", "username", "example", "runner"]
                    .contains(&account.to_ascii_lowercase().as_str())
            {
                return true;
            }
            rest = &rest[index + marker.len()..];
        }
    }
    false
}

fn credential_signature(text: &str) -> bool {
    let git_token = ["gh", "p_"].concat();
    let personal_token = ["github", "_pat_"].concat();
    has_prefixed_token(text, &git_token, 12)
        || has_prefixed_token(text, &personal_token, 12)
        || text
            .split(|character: char| !credential_character(character) || character == '=')
            .any(|word| {
                ((word.starts_with("AKIA") || word.starts_with("ASIA")) && word.len() == 20)
                    || (word.starts_with("AIza") && word.len() >= 24)
            })
        || text.split_whitespace().any(|word| word == "Bearer")
            && text
                .split_whitespace()
                .any(|word| word.len() >= 20 && word.chars().all(credential_character))
}

fn has_prefixed_token(text: &str, prefix: &str, minimum: usize) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.match_indices(prefix).any(|(index, _)| {
        lower[index..]
            .chars()
            .take_while(|character| credential_character(*character))
            .count()
            >= minimum
    })
}

fn credential_assignment(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "api_key",
        "api-key",
        "access_token",
        "access-token",
        "auth_token",
        "auth-token",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|key| {
        lower.match_indices(key).any(|(index, _)| {
            let tail = lower[index + key.len()..].trim_start();
            let Some(value) = tail.strip_prefix(':').or_else(|| tail.strip_prefix('=')) else {
                return false;
            };
            value
                .trim_start_matches([' ', '\t', '"', '\''])
                .chars()
                .take_while(|character| credential_character(*character))
                .count()
                >= 12
        })
    })
}

fn private_filename(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    name == ".env"
        || (name.starts_with(".env.") && !name.ends_with(".example"))
        || [
            ".npmrc",
            ".pypirc",
            ".netrc",
            ".git-credentials",
            ".dockerconfigjson",
            "id_rsa",
            "id_ed25519",
            "credentials",
            "credential",
            "secrets",
        ]
        .contains(&name.as_str())
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        || [
            "credentials",
            "credential",
            "secrets",
            "secret",
            "password",
            "token",
        ]
        .iter()
        .any(|stem| {
            name.starts_with(stem)
                && [".json", ".yaml", ".yml", ".toml", ".ini", ".cfg", ".txt"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        })
        || name.ends_with("config.local")
        || name.ends_with("settings.local")
}

fn pull_request_merge(root: &Path) -> Option<Vec<u8>> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    let parents = git(root, &["rev-list", "--parents", "-n", "1", "HEAD"])?;
    verified_pull_request_merge(
        std::env::var_os("GITHUB_EVENT_NAME").as_deref(),
        std::env::var_os("GITHUB_SHA").as_deref(),
        &head,
        &parents,
    )
}

fn verified_pull_request_merge(
    event: Option<&OsStr>,
    expected: Option<&OsStr>,
    head: &[u8],
    parents: &[u8],
) -> Option<Vec<u8>> {
    if event != Some(OsStr::new("pull_request")) {
        return None;
    }
    let head = head.strip_suffix(b"\n").unwrap_or(head);
    if expected?.as_encoded_bytes() != head {
        return None;
    }
    let fields = parents
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    (fields.len() == 3 && fields[0] == head).then(|| head.to_vec())
}

fn scan_metadata(bytes: &[u8], ignored_commit: Option<&[u8]>, findings: &mut Findings) {
    let fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    for commit in fields.chunks(5) {
        if commit.len() < 5 {
            continue;
        }
        let sha = &commit[0][commit[0]
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(commit[0].len())..];
        if ignored_commit.is_some_and(|ignored| sha == ignored) {
            continue;
        }
        for (name, email, committer) in
            [(commit[1], commit[2], false), (commit[3], commit[4], true)]
        {
            let name = String::from_utf8_lossy(name);
            let email = String::from_utf8_lossy(email).trim().to_ascii_lowercase();
            let canonical = name.trim() == ALLOWED_NAME && email == ALLOWED_EMAIL;
            let account_alias =
                name.trim() == ALLOWED_ACCOUNT_ALIAS_NAME && email == ALLOWED_ACCOUNT_ALIAS_EMAIL;
            let github_committer = committer
                && name.trim() == GITHUB_COMMITTER_NAME
                && email == GITHUB_COMMITTER_EMAIL;
            if !canonical && !account_alias && !github_committer {
                if name.trim() != ALLOWED_NAME {
                    add(
                        findings,
                        "public-identity-name-mismatch",
                        "git-history:commit-metadata",
                    );
                }
                if email != ALLOWED_EMAIL {
                    add(
                        findings,
                        "public-identity-email-mismatch",
                        "git-history:commit-metadata",
                    );
                }
            }
            if !github_committer
                && !email.ends_with("@users.noreply.github.com")
                && !email.contains("@noreply.")
            {
                add(
                    findings,
                    "non-noreply-author-email",
                    "git-history:commit-metadata",
                );
            }
        }
    }
}

fn scan_patch(bytes: &[u8], findings: &mut Findings) {
    let patch = String::from_utf8_lossy(bytes);
    scan_text(&patch, "git-history:patches", findings);
    for line in patch
        .lines()
        .filter(|line| line.starts_with("diff --git a/"))
    {
        let mut fields = line.split_whitespace();
        let _ = fields.next();
        let _ = fields.next();
        if let (Some(before), Some(after)) = (fields.next(), fields.next()) {
            scan_path(
                Path::new(before.trim_start_matches("a/")),
                "git-history:patch-paths",
                findings,
            );
            scan_path(
                Path::new(after.trim_start_matches("b/")),
                "git-history:patch-paths",
                findings,
            );
        }
    }
}

fn git(root: &Path, arguments: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_CEILING_DIRECTORIES", root.parent().unwrap_or(root))
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn nul_paths(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
        .collect()
}

fn add(findings: &mut Findings, category: &str, source: &str) {
    findings.insert((category.to_owned(), source.to_owned()));
}

fn identity_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

fn credential_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || "_./+=-".contains(character)
}

fn temp(label: &str) -> PathBuf {
    support::temp(&format!("privacy-{label}"))
}

#[test]
fn publication_privacy_gate_passes_the_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let findings = scan(root);
    assert!(findings.is_empty(), "privacy findings: {findings:?}");
}

#[test]
fn mailmap_accepts_only_role_specific_github_squash_identities() {
    let root = temp("mailmap");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".mailmap"),
        root.join(".mailmap"),
    )
    .unwrap();
    fs::write(root.join("notes.txt"), "clean\n").unwrap();
    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "init", "-q"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "add", "."])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "-c",
            "user.name=GitHub",
            "-c",
            "user.email=noreply@github.com",
            "commit",
            "-qm",
            "canonical squash identity",
            "--author",
            "envy404 <287204764+envy404@users.noreply.github.com>",
        ])
        .status()
        .unwrap()
        .success());
    assert!(scan(&root).is_empty());
    let canonical = Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "log",
            "-1",
            "--use-mailmap",
            "--format=%aN <%aE>",
        ])
        .output()
        .unwrap();
    assert!(canonical.status.success());
    assert_eq!(
        String::from_utf8(canonical.stdout).unwrap(),
        "envy404 <legacy-operator@users.noreply.github.com>\n"
    );

    let github_author = format!(
        "commit\0{GITHUB_COMMITTER_NAME}\0{GITHUB_COMMITTER_EMAIL}\0{ALLOWED_NAME}\0{ALLOWED_EMAIL}\0"
    );
    let mut github_author_findings = Findings::new();
    scan_metadata(github_author.as_bytes(), None, &mut github_author_findings);
    assert!(github_author_findings
        .iter()
        .any(|(category, _)| category == "public-identity-name-mismatch"));
    assert!(github_author_findings
        .iter()
        .any(|(category, _)| category == "public-identity-email-mismatch"));

    fs::write(root.join("notes.txt"), "outsider\n").unwrap();
    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "add", "notes.txt"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "-c",
            "user.name=other-account",
            "-c",
            "user.email=123456+other-account@users.noreply.github.com",
            "commit",
            "-qm",
            "unrelated identity",
        ])
        .status()
        .unwrap()
        .success());
    let findings = scan(&root);
    assert!(findings
        .iter()
        .any(|(category, _)| category == "public-identity-name-mismatch"));
    assert!(findings
        .iter()
        .any(|(category, _)| category == "public-identity-email-mismatch"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn privacy_categories_detect_candidate_and_historical_exposure_without_echoing_values() {
    let root = temp("adversarial");
    let private_path = ["/", "home", "/", "local-account", "/project"].concat();
    let token = ["github", "_pat_", "TESTVALUE123"].concat();
    fs::write(
        root.join("notes.txt"),
        format!("{private_path}\nTOKEN={token}\n"),
    )
    .unwrap();
    fs::write(root.join(".env.local"), "not read\n").unwrap();
    let findings = scan(&root);
    assert!(findings
        .iter()
        .any(|(category, _)| category == "private-home-path"));
    assert!(findings
        .iter()
        .any(|(category, _)| category == "credential-token-signature"));
    assert!(findings
        .iter()
        .any(|(category, _)| category == "private-config-filename"));

    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "init", "-q"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "add", "notes.txt"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "-c",
            "user.name=envy404",
            "-c",
            "user.email=legacy-operator@users.noreply.github.com",
            "commit",
            "-qm",
            "fixture"
        ])
        .status()
        .unwrap()
        .success());
    fs::write(root.join("notes.txt"), "clean\n").unwrap();
    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "add", "notes.txt"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "-c",
            "user.name=envy404",
            "-c",
            "user.email=legacy-operator@users.noreply.github.com",
            "commit",
            "-qm",
            "remove fixture"
        ])
        .status()
        .unwrap()
        .success());
    let historical = scan(&root);
    assert!(historical.iter().any(
        |(category, source)| category == "credential-token-signature"
            && source == "git-history:patches"
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn incomplete_git_probe_fails_closed_and_placeholder_homes_stay_clean() {
    let root = temp("incomplete");
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(
        root.join("notes.txt"),
        ["/", "home", "/", "private", "/fixture"].concat(),
    )
    .unwrap();
    let findings = scan(&root);
    assert!(findings.contains(&("scan-incomplete".into(), "git-probe".into())));
    assert!(!findings
        .iter()
        .any(|(category, _)| category == "private-home-path"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn verified_github_pull_merge_does_not_replace_parent_identity_checks() {
    let head = "1111111111111111111111111111111111111111";
    let parent = "2222222222222222222222222222222222222222";
    let metadata = format!(
        "{head}\0external account\0external@example.com\0GitHub\0noreply@github.com\0\n{parent}\0envy404\0{ALLOWED_EMAIL}\0envy404\0{ALLOWED_EMAIL}\0"
    );
    let mut unfiltered = Findings::new();
    scan_metadata(metadata.as_bytes(), None, &mut unfiltered);
    assert!(unfiltered
        .iter()
        .any(|(category, _)| category == "public-identity-name-mismatch"));
    assert!(unfiltered
        .iter()
        .any(|(category, _)| category == "non-noreply-author-email"));

    let parents = format!("{head} {parent} 3333333333333333333333333333333333333333\n");
    let ignored = verified_pull_request_merge(
        Some(OsStr::new("pull_request")),
        Some(OsStr::new(head)),
        format!("{head}\n").as_bytes(),
        parents.as_bytes(),
    )
    .unwrap();
    let mut filtered = Findings::new();
    scan_metadata(metadata.as_bytes(), Some(&ignored), &mut filtered);
    assert!(filtered.is_empty(), "privacy findings: {filtered:?}");
    assert!(verified_pull_request_merge(
        Some(OsStr::new("push")),
        Some(OsStr::new(head)),
        head.as_bytes(),
        parents.as_bytes(),
    )
    .is_none());
    assert!(verified_pull_request_merge(
        Some(OsStr::new("pull_request")),
        Some(OsStr::new("0000000000000000000000000000000000000000")),
        head.as_bytes(),
        parents.as_bytes(),
    )
    .is_none());
}
