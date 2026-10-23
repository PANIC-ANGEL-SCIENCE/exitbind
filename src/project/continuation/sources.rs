//! Local, bounded source observations. Git is an already authorized checkout;
//! exported snapshots retain declared provenance and never imply a live fetch.

use super::model::{contains_path, validate_path, Source, SourceKind, Spec};
use crate::{config::Loaded, evidence::hash, project::path};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

const SPEC_LIMIT: u64 = 64 * 1024;
const SOURCE_LIMIT: u64 = 32 * 1024;
const TOTAL_SOURCE_LIMIT: usize = 128 * 1024;

/// The source digest identifies the exact specification bytes, not a claim
/// that an exported document is live or that its author approved execution.
pub(crate) fn read(loaded: &Loaded, name: &str) -> Result<(Spec, String), String> {
    crate::work::details::ensure_current_config(loaded)?;
    let bytes = observed(loaded, name, SPEC_LIMIT)?;
    let spec: Spec = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid project continuation specification: {e}"))?;
    spec.validate()?;
    let mut total = 0usize;
    let mut sources = Vec::with_capacity(spec.sources.len());
    for source in &spec.sources {
        let remaining = TOTAL_SOURCE_LIMIT.saturating_sub(total);
        if remaining == 0 {
            return Err("approved project sources exceed the 131072-byte total bound".into());
        }
        let content = verify(loaded, source, SOURCE_LIMIT.min(remaining as u64))?;
        total = total
            .checked_add(content.len())
            .ok_or("project source byte count overflow")?;
        if total > TOTAL_SOURCE_LIMIT {
            return Err("approved project sources exceed the 131072-byte total bound".into());
        }
        sources.push(content);
    }
    // Re-read all selected evidence before handing it to admission. Secure
    // file reads also verify descriptor identity and bytes internally.
    for (source, expected) in spec.sources.iter().zip(sources) {
        if verify(loaded, source, SOURCE_LIMIT)? != expected {
            return Err("project source changed during observation; prepare again".into());
        }
    }
    if observed(loaded, name, SPEC_LIMIT)? != bytes {
        return Err("project specification changed during observation; prepare again".into());
    }
    crate::work::details::ensure_current_config(loaded)?;
    Ok((spec, hash::bytes(&bytes)))
}

fn allowed(loaded: &Loaded, name: &str) -> Result<(), String> {
    validate_path(name)?;
    let lead = loaded
        .lead()
        .and_then(|name| loaded.agent(name))
        .ok_or("project source observation requires a configured Lead")?;
    if lead.observe.iter().any(|scope| {
        crate::config::boundary::maximum_contains(scope, name)
            || (validate_path(scope).is_ok()
                && contains_path(scope, name)
                && std::fs::symlink_metadata(loaded.product_root.join(scope))
                    .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()))
    }) {
        Ok(())
    } else {
        Err("project source path is outside the configured Lead observe boundary".into())
    }
}

fn observed(loaded: &Loaded, name: &str, limit: u64) -> Result<Vec<u8>, String> {
    allowed(loaded, name)?;
    match path::secure_bytes_observation_bounded(
        &loaded.product_root,
        name,
        "project continuation source",
        limit,
    ) {
        path::SecureBytesResult::Bytes(bytes) => Ok(bytes),
        path::SecureBytesResult::Absent(error)
        | path::SecureBytesResult::Unsafe(error)
        | path::SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(error) => Err(error),
    }
}

fn verify(loaded: &Loaded, source: &Source, limit: u64) -> Result<Vec<u8>, String> {
    let bytes = observed(loaded, &source.path, limit)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| "project source must contain valid UTF-8")?;
    if text.is_empty() || text.contains('\0') || hash::bytes(&bytes) != source.sha256 {
        return Err(
            "project source is empty, contains NUL or differs from its exact SHA256".into(),
        );
    }
    if source.section.as_ref().is_some_and(|s| !text.contains(s)) {
        return Err("project source does not contain its declared literal section locator".into());
    }
    if source.kind == SourceKind::Git {
        verify_git(loaded, source, &bytes)?;
    }
    Ok(bytes)
}

fn verify_git(loaded: &Loaded, source: &Source, bytes: &[u8]) -> Result<(), String> {
    let root = &loaded.product_root;
    let checkout = git_text(root, &["rev-parse", "--show-toplevel"])?;
    let checkout = std::fs::canonicalize(checkout).map_err(|e| e.to_string())?;
    let checkout_text = checkout.to_str().ok_or("Git checkout path must be UTF-8")?;
    let repository = source
        .repository
        .as_deref()
        .ok_or("Git source requires a repository identity")?;
    // A canonical checkout locator supports entirely local disposable
    // projects. Otherwise require the exact configured origin, without
    // fetching it or treating its URL as a credential/tool authorization.
    if repository != checkout_text
        && git_text(root, &["config", "--local", "--get", "remote.origin.url"])? != repository
    {
        return Err("Git source repository differs from the authorized checkout identity".into());
    }
    let head = git_text(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    if head != source.revision {
        return Err("Git project source revision differs from the current checkout HEAD".into());
    }
    let prefix = git_text(root, &["rev-parse", "--show-prefix"])?;
    let object = format!("{}:{}{}", source.revision, prefix, source.path);
    let length = git_text(root, &["cat-file", "-s", &object])?
        .parse::<u64>()
        .map_err(|_| "Git document size is invalid")?;
    if length > SOURCE_LIMIT || length != bytes.len() as u64 {
        return Err("Git document size is outside its bound or differs from local source".into());
    }
    let committed = git(
        root,
        &["show", "--no-ext-diff", "--no-textconv", &object],
        SOURCE_LIMIT,
    )?;
    if committed != bytes || hash::bytes(&committed) != source.sha256 {
        return Err("Git project source bytes differ from the specified commit".into());
    }
    if git_text(root, &["rev-parse", "--verify", "HEAD^{commit}"])? != head
        || git_text(root, &["rev-parse", "--show-toplevel"])? != checkout_text
    {
        return Err("Git checkout changed during source observation".into());
    }
    Ok(())
}

fn git_text(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let bytes = git(root, arguments, 4096)?;
    let value = String::from_utf8(bytes).map_err(|_| "Git source identity must be UTF-8")?;
    // Remove Git's record terminator only; spaces in repository/path names
    // remain part of their exact identity.
    Ok(value.strip_suffix('\n').unwrap_or(&value).to_owned())
}

fn git(root: &Path, arguments: &[&str], limit: u64) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .args(arguments)
        // A missing object in a partial clone must not turn a local read
        // into an implicit transport/lazy fetch, even on older Git versions.
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_NO_LAZY_FETCH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // The supplied root, not ambient Git selectors, owns this observation.
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(variable);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Git source observation could not start the existing Git executable")?;
    let mut bytes = Vec::new();
    let observation = child
        .stdout
        .take()
        .ok_or("Git source stdout is unavailable")?
        .take(limit + 1)
        .read_to_end(&mut bytes);
    if observation.is_err() || bytes.len() as u64 > limit {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Git source observation failed or exceeded its byte bound".into());
    }
    let status = child
        .wait()
        .map_err(|_| "Git source exit status is unavailable")?;
    if !status.success() {
        return Err("Git source observation failed; no remote acquisition was attempted".into());
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::fs;

    struct Fixture {
        root: crate::test_root::TempRoot,
        loaded: Loaded,
    }

    impl Fixture {
        fn new() -> Self {
            let root = crate::test_root::temp("project-sources");
            git(&root, &["init", "-q"], 4096).unwrap();
            let configuration = crate::project::onboarding::init_with_options(
                crate::project::onboarding::InitOptions {
                    product_root: root.to_str().unwrap(),
                    coffee: false,
                    skip_skills: true,
                    mode: Some("portable"),
                    project_id: None,
                    control_root: None,
                    state_root: None,
                },
            )
            .unwrap();
            let mut config: Value =
                serde_json::from_slice(&fs::read(&configuration).unwrap()).unwrap();
            config["agents"]["lead"]["observe"] = json!(["docs", "spec.json"]);
            fs::write(&configuration, config.to_string()).unwrap();
            fs::create_dir(root.join("docs")).unwrap();
            fs::write(
                root.join("docs/brief.md"),
                "# Direction\nMake the existing project useful.\n",
            )
            .unwrap();
            let loaded = crate::config::load(configuration.to_str()).unwrap();
            Self { root, loaded }
        }

        fn spec(&self) -> Value {
            json!({"version":1,"projectId":"example","mandate":{"id":"advance","revision":1,
                "outcome":"Make the existing project useful.","mode":"propose","allowedSources":["brief"],
                "write":["src"],"commands":["true"],"roles":["worker","reviewer"],"preserve":["Keep existing behavior."],
                "priorityPolicy":"Choose a useful small gap.","expiresAt":"2020-01-01T00:00:00Z", "maxWorks":2,
                "maxActions":12,"maxParallel":1,"maxElapsedSeconds":600,"externalPublication":false},
                "sources":[{"id":"brief","kind":"export","repository":null,"revision":"export-1",
                "path":"docs/brief.md","section":"Direction","sha256":hash::bytes(&fs::read(self.root.join("docs/brief.md")).unwrap()),
                "acquiredAt":"2019-01-01T00:00:00Z","currentness":"declared_export","trust":"ownerDirection"}]})
        }

        fn save(&self, spec: &Value) {
            fs::write(self.root.join("spec.json"), spec.to_string()).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn exported_snapshot_retains_exact_version_and_expired_history_without_execution() {
        let fixture = Fixture::new();
        let spec = fixture.spec();
        fixture.save(&spec);
        let (readback, digest) = read(&fixture.loaded, "spec.json").unwrap();
        assert_eq!(readback.sources[0].revision, "export-1");
        assert_eq!(readback.sources[0].currentness, "declared_export");
        assert_eq!(
            digest,
            hash::bytes(&fs::read(fixture.root.join("spec.json")).unwrap())
        );
        assert!(fs::read_dir(fixture.root.join(".exitbind/runs"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn changed_unapproved_unsafe_and_oversized_sources_are_refused() {
        let fixture = Fixture::new();
        let mut spec = fixture.spec();
        fixture.save(&spec);
        fs::write(fixture.root.join("docs/brief.md"), "Changed direction").unwrap();
        assert!(read(&fixture.loaded, "spec.json")
            .unwrap_err()
            .contains("SHA256"));
        spec["sources"][0]["path"] = json!("outside.md");
        fixture.save(&spec);
        assert!(read(&fixture.loaded, "spec.json")
            .unwrap_err()
            .contains("observe boundary"));
        spec["sources"][0]["path"] = json!("docs/brief.md");
        fixture.save(&spec);
        fs::remove_file(fixture.root.join("docs/brief.md")).unwrap();
        std::os::unix::fs::symlink("../spec.json", fixture.root.join("docs/brief.md")).unwrap();
        assert!(read(&fixture.loaded, "spec.json").is_err());
        fs::remove_file(fixture.root.join("docs/brief.md")).unwrap();
        fs::write(
            fixture.root.join("docs/brief.md"),
            vec![b'x'; SOURCE_LIMIT as usize + 1],
        )
        .unwrap();
        assert!(read(&fixture.loaded, "spec.json").is_err());
    }

    #[test]
    fn git_source_requires_exact_checkout_commit_and_committed_bytes() {
        let fixture = Fixture::new();
        git(&fixture.root, &["add", "docs/brief.md"], 4096).unwrap();
        git(
            &fixture.root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "Add fixture direction",
            ],
            4096,
        )
        .unwrap();
        let revision = git_text(&fixture.root, &["rev-parse", "HEAD"]).unwrap();
        let mut spec = fixture.spec();
        spec["sources"][0]["kind"] = json!("git");
        spec["sources"][0]["repository"] =
            json!(fs::canonicalize(&fixture.root).unwrap().to_str().unwrap());
        spec["sources"][0]["revision"] = json!(revision);
        spec["sources"][0]["currentness"] = json!("checkout_head");
        fixture.save(&spec);
        read(&fixture.loaded, "spec.json").unwrap();
        fs::write(
            fixture.root.join("docs/brief.md"),
            "# Direction\nDifferent uncommitted direction.\n",
        )
        .unwrap();
        spec["sources"][0]["sha256"] = json!(hash::bytes(
            &fs::read(fixture.root.join("docs/brief.md")).unwrap()
        ));
        fixture.save(&spec);
        assert!(read(&fixture.loaded, "spec.json").is_err());
        spec["sources"][0]["revision"] = json!("a".repeat(40));
        fixture.save(&spec);
        assert!(read(&fixture.loaded, "spec.json")
            .unwrap_err()
            .contains("HEAD"));
    }

    #[test]
    fn aggregate_source_bound_and_revoked_observe_config_are_refused() {
        let fixture = Fixture::new();
        let mut spec = fixture.spec();
        let mut sources = Vec::new();
        let mut ids = Vec::new();
        let content = vec![b'x'; SOURCE_LIMIT as usize];
        for index in 0..5 {
            let id = format!("source-{index}");
            let path = format!("docs/brief-{index}.md");
            fs::write(fixture.root.join(&path), &content).unwrap();
            let mut source = spec["sources"][0].clone();
            source["id"] = json!(id);
            source["path"] = json!(path);
            source["sha256"] = json!(hash::bytes(&content));
            source["section"] = Value::Null;
            ids.push(id);
            sources.push(source);
        }
        spec["sources"] = json!(sources);
        spec["mandate"]["allowedSources"] = json!(ids);
        fixture.save(&spec);
        assert!(read(&fixture.loaded, "spec.json")
            .unwrap_err()
            .contains("total bound"));
        let mut configuration: Value =
            serde_json::from_slice(&fs::read(&fixture.loaded.path).unwrap()).unwrap();
        configuration["agents"]["lead"]["observe"] = json!([]);
        fs::write(&fixture.loaded.path, configuration.to_string()).unwrap();
        assert!(read(&fixture.loaded, "spec.json")
            .unwrap_err()
            .contains("configuration changed"));
    }
}
