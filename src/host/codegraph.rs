//! Optional, task-bound CodeGraph provider integration.
//!
//! Provider readiness belongs to one explicitly enabled project. Query and
//! continuation results remain bounded observations, never Work authority.
use crate::{config, evidence::hash, host::session_recovery};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

mod preparation;
mod provider;
use preparation::{prepare, Readiness};
pub(crate) use provider::clear_git_environment;
use provider::{
    discover_provider, provider_identity, run_provider, source_identity, status, IndexState,
    ProviderIdentity,
};

const MARKER: &str = ".codegraph/exitbind-authority.json";
const QUERY_CACHE: &str = ".codegraph/exitbind-query-cache.json";
const MAX_OUTPUT: usize = 1024 * 1024;
const MAX_ARTIFACT: usize = 1024 * 1024;
const MAX_METADATA: usize = 16 * 1024;
const QUERY_LIMIT: Duration = Duration::from_secs(15);
const PREPARE_LIMIT: Duration = Duration::from_secs(120);

pub(crate) struct Target {
    loaded: config::Loaded,
    work: Option<String>,
}

fn canonical(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("CodeGraph target unavailable: {error}"))
}

fn target(explicit: Option<&str>) -> Result<Target, String> {
    match session_recovery::task_target(explicit)? {
        Some(resolved) => Ok(Target {
            loaded: resolved.loaded,
            work: resolved.storage_work,
        }),
        None => {
            // task_target returns None only when no native session is selected.
            // A native owner/Goal/hint lookup error propagates above; never fall
            // back to a launch-cwd project when that association is unresolved.
            let loaded = config::load(None)?;
            Ok(Target { loaded, work: None })
        }
    }
}

fn project_id(loaded: &config::Loaded) -> &str {
    loaded.project_id.as_deref().unwrap_or("")
}

fn safe_codegraph_dir(root: &Path, create: bool) -> Result<PathBuf, String> {
    let directory = root.join(".codegraph");
    if create && !directory.exists() {
        fs::create_dir(&directory)
            .map_err(|error| format!("cannot create project CodeGraph directory: {error}"))?;
    }
    if directory.exists() {
        let meta = fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("CodeGraph index directory is not a safe project directory".into());
        }
        let resolved = canonical(&directory)?;
        if resolved.parent() != Some(root)
            || resolved.file_name().and_then(|v| v.to_str()) != Some(".codegraph")
        {
            return Err("CodeGraph index directory resolves outside the exact project root".into());
        }
    }
    if create && !directory.is_dir() {
        return Err("CodeGraph index directory could not be created safely".into());
    }
    Ok(directory)
}

fn safe_relative(path: &str) -> bool {
    let value = Path::new(path);
    !value.as_os_str().is_empty()
        && value
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Accept only bounded artifact routes beneath Exitbind's namespaced state area.
pub(crate) fn valid_artifact_path(path: &str) -> bool {
    if path.len() > 512 || path.contains('\\') {
        return false;
    }
    let prefix = format!(
        "{}/codegraph/",
        crate::project::layout_types::state_namespace()
    );
    let Some(name) = path.strip_prefix(&prefix) else {
        return false;
    };
    let Some(digest) = name.strip_suffix(".json") else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn read_bounded(root: &Path, relative: &str, label: &str, limit: usize) -> Result<Vec<u8>, String> {
    if !safe_relative(relative) {
        return Err(format!("{label} path is unsafe"));
    }
    match crate::project::path::secure_bytes_observation_single_link_bounded(
        root,
        relative,
        label,
        limit as u64,
    ) {
        crate::project::path::SecureBytesResult::Bytes(bytes) if bytes.len() <= limit => Ok(bytes),
        crate::project::path::SecureBytesResult::Bytes(_) => {
            Err(format!("{label} exceeds its byte bound"))
        }
        crate::project::path::SecureBytesResult::Absent(_) => Err(format!("{label} is absent")),
        crate::project::path::SecureBytesResult::Unsafe(error)
        | crate::project::path::SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        crate::project::path::SecureBytesResult::Unsupported(error) => Err(error),
    }
}

fn marker_path(root: &Path, create: bool) -> Result<PathBuf, String> {
    let directory = safe_codegraph_dir(root, create)?;
    Ok(directory.join("exitbind-authority.json"))
}

fn read_marker(root: &Path, expected_project_id: &str) -> Result<bool, String> {
    let _ = marker_path(root, false)?;
    let bytes = match crate::project::path::secure_bytes_observation_single_link_bounded(
        root,
        MARKER,
        "CodeGraph authority marker",
        4096,
    ) {
        crate::project::path::SecureBytesResult::Absent(_) => return Ok(false),
        crate::project::path::SecureBytesResult::Bytes(bytes) => bytes,
        crate::project::path::SecureBytesResult::Unsafe(error)
        | crate::project::path::SecureBytesResult::Unreadable(error) => return Err(error),
        #[cfg(not(unix))]
        crate::project::path::SecureBytesResult::Unsupported(error) => return Err(error),
    };
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "CodeGraph authority marker is malformed")?;
    if value["kind"] != "exitbind_codegraph_authority"
        || value["protocol"] != 1
        || value["productRoot"].as_str() != root.to_str()
        || value["projectId"].as_str().unwrap_or("") != expected_project_id
    {
        return Err("CodeGraph authority marker belongs to another project".into());
    }
    Ok(true)
}

fn write_atomic(root: &Path, relative: &str, bytes: &[u8], limit: usize) -> Result<(), String> {
    if bytes.len() > limit || !safe_relative(relative) {
        return Err("CodeGraph metadata exceeds its bound or has an unsafe path".into());
    }
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or("CodeGraph metadata path has no parent")?;
    let parent_meta = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
    if !parent_meta.is_dir() || parent_meta.file_type().is_symlink() || canonical(parent)? != parent
    {
        return Err("CodeGraph metadata directory is unsafe".into());
    }
    if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("CodeGraph metadata file must not be a symlink".into());
    }
    let temp = parent.join(format!(
        ".exitbind-tmp-{}-{}",
        std::process::id(),
        hash::bytes(bytes)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(&temp, &path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        error.to_string()
    })
}

fn write_marker(root: &Path, expected_project_id: &str) -> Result<(), String> {
    let _ = marker_path(root, true)?;
    let path = root.join(MARKER);
    if path.exists() && !read_marker(root, expected_project_id)? {
        return Err("CodeGraph authority marker does not match this project".into());
    }
    let value = json!({
        "kind":"exitbind_codegraph_authority",
        "protocol":1,
        "productRoot":root,
        "projectId":expected_project_id
    });
    write_atomic(
        root,
        MARKER,
        &serde_json::to_vec(&value).map_err(|error| error.to_string())?,
        4096,
    )
}

fn context(
    executable: &Path,
    root: &Path,
    query: &str,
    deadline: Instant,
) -> Result<Value, String> {
    let root_string = root.to_str().ok_or("target path is not UTF-8")?;
    let output = run_provider(
        executable,
        root,
        &[
            "context",
            "--path",
            root_string,
            "--format",
            "json",
            "--max-nodes",
            "8",
            query,
        ],
        deadline,
        false,
    )
    .map_err(|error| format!("query_failure: {error}"))?;
    let value: Value = serde_json::from_slice(&output)
        .map_err(|_| "query_failure: CodeGraph context returned malformed JSON")?;
    validate_context(&value)?;
    Ok(value)
}

fn validate_context(value: &Value) -> Result<(), String> {
    for field in [
        "nodes",
        "edges",
        "codeBlocks",
        "entryPoints",
        "relatedFiles",
    ] {
        if value.get(field).and_then(Value::as_array).is_none() {
            return Err(format!(
                "query_failure: CodeGraph context omitted structured {field}"
            ));
        }
    }
    if value.get("summary").and_then(Value::as_str).is_none()
        || value.get("query").and_then(Value::as_str).is_none()
        || value.get("stats").and_then(Value::as_object).is_none()
    {
        return Err("query_failure: CodeGraph context omitted summary, query, or stats".into());
    }
    Ok(())
}

fn context_is_empty(value: &Value) -> bool {
    [
        "nodes",
        "edges",
        "codeBlocks",
        "entryPoints",
        "relatedFiles",
    ]
    .iter()
    .all(|field| value[*field].as_array().is_some_and(Vec::is_empty))
}

fn result_summary(value: &Value) -> String {
    let summary = value["summary"].as_str().unwrap_or_default().trim();
    if !summary.is_empty() && summary.len() <= 160 {
        return summary.to_owned();
    }
    format!(
        "{} nodes, {} edges, {} code blocks",
        value["nodes"].as_array().map_or(0, Vec::len),
        value["edges"].as_array().map_or(0, Vec::len),
        value["codeBlocks"].as_array().map_or(0, Vec::len)
    )
}

fn continuation_summary(value: &Value) -> String {
    // attach_graph_result adds "CodeGraph: " (11 UTF-8 bytes) and caps the
    // complete label at 128 bytes, so keep the supplied summary at 117 bytes.
    let summary = result_summary(value);
    let mut end = summary.len().min(117);
    while !summary.is_char_boundary(end) {
        end -= 1;
    }
    summary[..end].to_owned()
}

fn verify_provider_identity(
    executable: &Path,
    root: &Path,
    expected: &ProviderIdentity,
    deadline: Instant,
) -> Result<(), String> {
    let observed = provider_identity(executable, root, deadline)?;
    if &observed == expected {
        Ok(())
    } else {
        Err("provider_changed: CodeGraph executable identity changed during operation".into())
    }
}

fn cache_key(
    root: &Path,
    loaded: &config::Loaded,
    work: Option<&str>,
    query: &str,
    ready: &Readiness,
) -> String {
    hash::value(&json!({
        "root":root,
        "projectId":project_id(loaded),
        "work":work,
        "query":query,
        "sourceIdentity":ready.source_identity,
        "provider":ready.provider,
        "indexPath":ready.index_path,
        "indexIdentity":ready.index_identity
    }))
}

fn persist_state_artifact(
    loaded: &config::Loaded,
    artifact: &Value,
) -> Result<(String, String), String> {
    let state_root = canonical(&loaded.state_root)?;
    let namespace = crate::project::layout_types::state_namespace();
    let namespace_root = state_root.join(namespace);
    let directory = namespace_root.join("codegraph");
    crate::project::managed_files::ensure_state_directory(&state_root, &directory)?;
    let metadata = fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || canonical(&directory)?.parent() != Some(namespace_root.as_path())
    {
        return Err("artifact_unavailable: CodeGraph artifact directory is unsafe".into());
    }
    let bytes = serde_json::to_vec(artifact).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_ARTIFACT {
        return Err("artifact_limit: CodeGraph result exceeds 1 MiB".into());
    }
    let sha = hash::bytes(&bytes);
    let relative = format!("{namespace}/codegraph/{sha}.json");
    if !valid_artifact_path(&relative) {
        return Err("artifact_unavailable: generated CodeGraph artifact route is invalid".into());
    }
    let target = state_root.join(&relative);
    if target.exists() {
        let existing = read_bounded(
            &state_root,
            &relative,
            "cached CodeGraph result",
            MAX_ARTIFACT,
        )?;
        if hash::bytes(&existing) != sha || existing != bytes {
            return Err(
                "artifact_collision: existing result bytes do not match their digest".into(),
            );
        }
    } else {
        let temp = directory.join(format!(".{sha}.tmp-{}", std::process::id()));
        let mut file = crate::project::managed_files::open_state_file(&temp)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(temp, &target).map_err(|error| error.to_string())?;
    }
    Ok((relative, sha))
}

fn cache_metadata(root: &Path) -> Result<Option<Value>, String> {
    match crate::project::path::secure_bytes_observation_single_link_bounded(
        root,
        QUERY_CACHE,
        "CodeGraph query cache metadata",
        MAX_METADATA as u64,
    ) {
        crate::project::path::SecureBytesResult::Absent(_) => Ok(None),
        crate::project::path::SecureBytesResult::Bytes(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "query_cache_invalid: metadata is malformed".into()),
        crate::project::path::SecureBytesResult::Unsafe(error)
        | crate::project::path::SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        crate::project::path::SecureBytesResult::Unsupported(error) => Err(error),
    }
}

fn cached_context(
    target: &Target,
    root: &Path,
    query: &str,
    ready: &Readiness,
) -> Result<Option<(Value, String, String)>, String> {
    let key = cache_key(root, &target.loaded, target.work.as_deref(), query, ready);
    let Some(meta) = cache_metadata(root)? else {
        return Ok(None);
    };
    if meta.get("kind").and_then(Value::as_str) != Some("exitbind_codegraph_query_cache")
        || meta.get("protocol").and_then(Value::as_u64) != Some(1)
        || meta.get("key").and_then(Value::as_str) != Some(key.as_str())
    {
        return Ok(None);
    }
    let relative = meta
        .get("artifactPath")
        .and_then(Value::as_str)
        .ok_or("query_cache_invalid: artifact path missing")?;
    let sha = meta
        .get("artifactSha256")
        .and_then(Value::as_str)
        .ok_or("query_cache_invalid: artifact digest missing")?;
    if !valid_artifact_path(relative) {
        return Err("query_cache_invalid: artifact path is unsafe".into());
    }
    let bytes = read_bounded(
        &canonical(&target.loaded.state_root)?,
        relative,
        "cached CodeGraph result",
        MAX_ARTIFACT,
    )?;
    if hash::bytes(&bytes) != sha {
        return Err("query_cache_invalid: artifact digest changed".into());
    }
    let artifact: Value =
        serde_json::from_slice(&bytes).map_err(|_| "query_cache_invalid: artifact is malformed")?;
    if artifact["kind"] != "exitbind_codegraph_query"
        || artifact["target"].as_str() != root.to_str()
        || artifact["projectId"].as_str().unwrap_or("") != project_id(&target.loaded)
        || artifact["work"].as_str() != target.work.as_deref()
        || artifact["query"].as_str() != Some(query)
        || artifact["sourceIdentity"].as_str() != Some(ready.source_identity.as_str())
        || artifact["provider"]
            != serde_json::to_value(&ready.provider).map_err(|error| error.to_string())?
        || artifact["indexPath"].as_str() != Some(ready.index_path.as_str())
        || artifact["indexIdentity"].as_str() != Some(ready.index_identity.as_str())
    {
        return Ok(None);
    }
    let context = artifact
        .get("context")
        .cloned()
        .ok_or("query_cache_invalid: context is absent")?;
    validate_context(&context)?;
    Ok(Some((context, relative.to_owned(), sha.to_owned())))
}

fn save_cache(
    root: &Path,
    key: &str,
    artifact_path: &str,
    artifact_sha: &str,
) -> Result<(), String> {
    let value = json!({"kind":"exitbind_codegraph_query_cache","protocol":1,"key":key,"artifactPath":artifact_path,"artifactSha256":artifact_sha});
    write_atomic(
        root,
        QUERY_CACHE,
        &serde_json::to_vec(&value).map_err(|error| error.to_string())?,
        MAX_METADATA,
    )
}

fn verify_ready_index(
    executable: &Path,
    root: &Path,
    ready: &Readiness,
    deadline: Instant,
) -> Result<Value, String> {
    match status(executable, root, deadline)? {
        IndexState::Ready {
            status,
            index_path,
            index_identity,
            stale_reason: None,
            pending_changes: false,
        } if index_path == ready.index_path && index_identity == ready.index_identity => Ok(status),
        _ => Err("index_changed: CodeGraph index changed or became stale during query".into()),
    }
}

fn graph_query(target: &Target, query: &str) -> Result<Value, String> {
    if query.trim().is_empty() || query.len() > 2048 {
        return Err("query_invalid: CodeGraph query must be 1..2048 bytes".into());
    }
    let root = canonical(&target.loaded.product_root)?;
    if !read_marker(&root, project_id(&target.loaded))? {
        return Ok(
            json!({"status":"unavailable","reason":"authority_denied: CodeGraph is disabled for this exact project","target":root,"fallback":"Continue ordinary source investigation in the validated project; explicitly enable CodeGraph for this project to use indexing."}),
        );
    }
    let deadline = Instant::now() + PREPARE_LIMIT;
    let executable = discover_provider()?;
    let provider = provider_identity(&executable, &root, deadline)?;
    let mut reconciliation_left = 1;
    let mut ready = prepare(
        &target.loaded,
        &executable,
        &provider,
        deadline,
        &mut reconciliation_left,
    )?;
    let query_deadline = Instant::now() + QUERY_LIMIT;
    let mut source_before_query = source_identity(&target.loaded, query_deadline)?;
    if source_before_query != ready.source_identity {
        if reconciliation_left == 0 {
            return Err(
                "source_changed: source changed after preparation; no reconciliation remains"
                    .into(),
            );
        }
        reconciliation_left -= 1;
        ready = prepare(
            &target.loaded,
            &executable,
            &provider,
            deadline,
            &mut reconciliation_left,
        )?;
        source_before_query = source_identity(&target.loaded, query_deadline)?;
        if source_before_query != ready.source_identity {
            return Err("source_changed: source changed during preparation reconciliation".into());
        }
    }
    if let Some((cached, artifact_path, artifact_sha)) =
        cached_context(target, &root, query, &ready)?
    {
        let source_after_cache = source_identity(&target.loaded, query_deadline)?;
        if source_after_cache != source_before_query {
            return Err("source_changed: source changed while validating the cached result".into());
        }
        verify_provider_identity(&executable, &root, &ready.provider, query_deadline)?;
        ready.status = verify_ready_index(&executable, &root, &ready, query_deadline)?;
        if source_identity(&target.loaded, query_deadline)? != source_before_query {
            return Err(
                "source_changed: source changed while revalidating the cached index".into(),
            );
        }
        return finish_result(ResultInput {
            target,
            query,
            root: &root,
            ready: &ready,
            executable: &executable,
            deadline: query_deadline,
            context: cached,
            artifact_path,
            artifact_sha,
            cached: true,
        });
    }
    let mut result = context(&executable, &root, query, query_deadline)?;
    verify_provider_identity(&executable, &root, &ready.provider, query_deadline)?;
    let mut source_after_query = source_identity(&target.loaded, query_deadline)?;
    if source_after_query != source_before_query {
        if reconciliation_left == 0 {
            return Err(
                "source_changed: source changed during query; results are unavailable".into(),
            );
        }
        reconciliation_left -= 1;
        ready = prepare(
            &target.loaded,
            &executable,
            &provider,
            deadline,
            &mut reconciliation_left,
        )?;
        source_before_query = source_identity(&target.loaded, query_deadline)?;
        if source_before_query != ready.source_identity {
            return Err("source_changed: source changed during bounded re-preparation".into());
        }
        result = context(&executable, &root, query, query_deadline)?;
        verify_provider_identity(&executable, &root, &ready.provider, query_deadline)?;
        source_after_query = source_identity(&target.loaded, query_deadline)?;
        if source_after_query != source_before_query {
            return Err(
                "source_changed: source changed again after one bounded reconciliation".into(),
            );
        }
    }
    verify_provider_identity(&executable, &root, &ready.provider, query_deadline)?;
    let final_status = verify_ready_index(&executable, &root, &ready, query_deadline)?;
    if source_identity(&target.loaded, query_deadline)? != source_after_query {
        return Err("source_changed: source changed while validating final provider status".into());
    }
    ready.status = final_status.clone();
    let artifact = json!({
        "kind":"exitbind_codegraph_query",
        "protocol":1,
        "target":root,
        "projectId":project_id(&target.loaded),
        "work":target.work,
        "query":query,
        "sourceIdentity":source_after_query,
        "provider":ready.provider,
        "indexPath":ready.index_path,
        "indexIdentity":ready.index_identity,
        "indexStatus":final_status,
        "context":result
    });
    let (artifact_path, artifact_sha) = persist_state_artifact(&target.loaded, &artifact)?;
    let key = cache_key(&root, &target.loaded, target.work.as_deref(), query, &ready);
    save_cache(&root, &key, &artifact_path, &artifact_sha)?;
    finish_result(ResultInput {
        target,
        query,
        root: &root,
        ready: &ready,
        executable: &executable,
        deadline: query_deadline,
        context: result,
        artifact_path,
        artifact_sha,
        cached: false,
    })
}

struct ResultInput<'a> {
    target: &'a Target,
    query: &'a str,
    root: &'a Path,
    ready: &'a Readiness,
    executable: &'a Path,
    deadline: Instant,
    context: Value,
    artifact_path: String,
    artifact_sha: String,
    cached: bool,
}

fn finish_result(input: ResultInput<'_>) -> Result<Value, String> {
    let ResultInput {
        target,
        query,
        root,
        ready,
        executable,
        deadline,
        context,
        artifact_path,
        artifact_sha,
        cached,
    } = input;
    verify_provider_identity(executable, root, &ready.provider, deadline)?;
    let empty = context_is_empty(&context);
    let state_relative = artifact_path;
    let continuation = if let Some(work) = target.work.as_deref() {
        match session_recovery::attach_graph_result(
            work,
            &continuation_summary(&context),
            &state_relative,
            &artifact_sha,
        ) {
            Ok(true) => json!({"attached":true,"consumption":"generated_query_result"}),
            Ok(false) => {
                json!({"attached":false,"reason":"no matching existing Work-bound recovery hint"})
            }
            Err(error) => json!({"attached":false,"reason":crate::drift::plain(&error)}),
        }
    } else {
        json!({"attached":false,"reason":"no validated Goal-member storage binding"})
    };
    verify_provider_identity(executable, root, &ready.provider, deadline)?;
    Ok(json!({
        "status":if empty {"empty"} else {"usable"},
        "target":root,
        "query":query,
        "summary":result_summary(&context),
        "context":context,
        "artifact":{"root":"state","path":state_relative,"sha256":artifact_sha},
        "continuation":continuation,
        "cache":{"reused":cached},
        "provenance":{"sourceIdentity":ready.source_identity,"provider":ready.provider,"indexPath":ready.index_path,"indexIdentity":ready.index_identity,"indexStatus":ready.status}
    }))
}

pub(crate) fn command(
    action: &str,
    explicit_config: Option<&str>,
    query: Option<&str>,
) -> Result<Value, String> {
    if !matches!(action, "enable" | "disable" | "query") {
        return Err("project graph requires enable, disable, or query".into());
    }
    let target = match target(explicit_config) {
        Ok(target) => target,
        Err(reason) if action == "query" => {
            let query = query.ok_or("query_invalid: project graph query requires --query TEXT")?;
            return Ok(json!({
                "status":"unavailable",
                "reason":reason,
                "query":query,
                "fallback":"Continue ordinary source investigation through the verified project route; no launch-cwd or nearby checkout was selected."
            }));
        }
        Err(reason) => return Err(reason),
    };
    let root = canonical(&target.loaded.product_root)?;
    let id = project_id(&target.loaded);
    match action {
        "enable" => {
            write_marker(&root, id)?;
            let deadline = Instant::now() + PREPARE_LIMIT;
            let executable = discover_provider()?;
            let provider = provider_identity(&executable, &root, deadline)?;
            let mut remaining = 1;
            let ready = prepare(
                &target.loaded,
                &executable,
                &provider,
                deadline,
                &mut remaining,
            )?;
            verify_provider_identity(&executable, &root, &provider, deadline)?;
            Ok(
                json!({"status":"usable","enabled":true,"target":root,"provider":ready.provider,"indexPath":ready.index_path,"indexIdentity":ready.index_identity,"indexStatus":ready.status}),
            )
        }
        "disable" => {
            let _ = marker_path(&root, false)?;
            match fs::remove_file(root.join(MARKER)) {
                Ok(()) => Ok(json!({"status":"disabled","target":root})),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(json!({"status":"disabled","target":root}))
                }
                Err(error) => Err(format!("authority_marker_write_failed: {error}")),
            }
        }
        "query" => {
            let query = query.ok_or("query_invalid: project graph query requires --query TEXT")?;
            match graph_query(&target, query) {
                Ok(value) => Ok(value),
                Err(reason) => Ok(
                    json!({"status":"unavailable","reason":reason,"target":root,"fallback":"Continue ordinary source investigation in the validated project. Retry CodeGraph only through the exact project graph query route."}),
                ),
            }
        }
        _ => Err("project graph requires enable, disable, or query".into()),
    }
}

/// Validate one saved generated result for continuation. It remains a prior
/// observation: check bounded artifact bytes, exact target and stored source
/// binding; current source is unverified until the normal graph query route.
pub(crate) fn validate_saved_result(
    loaded: &config::Loaded,
    path: &str,
    expected_sha: &str,
    work: &str,
    producer_executable_sha256: &str,
) -> Result<Value, String> {
    if !valid_artifact_path(path) || work.trim().is_empty() {
        return Err("saved graph result route is malformed".into());
    }
    let build_identity = crate::producer::build_identity();
    let current_executable = build_identity["executableSha256"]
        .as_str()
        .ok_or("current executable identity is unavailable")?;
    if producer_executable_sha256 != current_executable {
        return Err("saved graph result was produced by another executable".into());
    }
    let state_root = canonical(&loaded.state_root)?;
    let bytes = read_bounded(&state_root, path, "saved graph result", MAX_ARTIFACT)?;
    if hash::bytes(&bytes) != expected_sha {
        return Err("saved graph result digest changed".into());
    }
    let artifact: Value =
        serde_json::from_slice(&bytes).map_err(|_| "saved graph result is malformed")?;
    let root = canonical(&loaded.product_root)?;
    let expected_index_path = root.join(".codegraph");
    if artifact["kind"] != "exitbind_codegraph_query"
        || artifact["target"].as_str() != root.to_str()
        || artifact["projectId"].as_str().unwrap_or("") != project_id(loaded)
        || artifact["work"].as_str() != Some(work)
        || artifact["sourceIdentity"]
            .as_str()
            .filter(|value| valid_sha(value))
            .is_none()
        || artifact["indexPath"].as_str() != expected_index_path.to_str()
        || artifact["indexIdentity"]
            .as_str()
            .filter(|value| valid_sha(value))
            .is_none()
        || artifact["provider"]["entrySha256"]
            .as_str()
            .filter(|value| valid_sha(value))
            .is_none()
        || artifact["provider"]["version"].as_str().is_none()
    {
        return Err("saved graph result target or source binding changed".into());
    }
    let query = artifact["query"]
        .as_str()
        .ok_or("saved graph result query is malformed")?;
    if query.trim().is_empty() || query.len() > 2048 {
        return Err("saved graph result query exceeds its bound".into());
    }
    let context = artifact
        .get("context")
        .ok_or("saved graph result has no context")?;
    validate_context(context)?;
    Ok(json!({
        "query":artifact["query"],
        "summary":result_summary(context),
        "nodeCount":context["nodes"].as_array().map_or(0,Vec::len),
        "edgeCount":context["edges"].as_array().map_or(0,Vec::len),
        "codeBlockCount":context["codeBlocks"].as_array().map_or(0,Vec::len),
        "sourceIdentity":artifact["sourceIdentity"],
        "provider":artifact["provider"],
        "indexIdentity":artifact["indexIdentity"],
        "sourceCurrent":null,
        "sourceCheck":"not_revalidated",
        "currentness":"prior_observation_source_provider_index_not_revalidated"
    }))
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
