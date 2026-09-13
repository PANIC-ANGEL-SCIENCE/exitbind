use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const FORMAT_MARKER: &str = "x-soulmate-format-version";
const REQUIRED_LIMITATIONS: [&str; 4] = [
    "caller_reported_checks",
    "synthetic_not_frequency",
    "local_authority",
    "artifact_scope",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn write_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "soulmate-value-contract-{label}-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create fixture: {error}"),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn copy_relative(source: &Path, destination: &Path, relative: &str) {
    let from = source.join(relative);
    let to = destination.join(relative);
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(from, to).unwrap();
}

fn registry_fixture() -> TempDir {
    let source = repo_root();
    let fixture = TempDir::new("registry");
    let registry = read_json(&source.join("proof/claims.json"));

    copy_relative(&source, fixture.path(), "proof/claims.json");
    for claim in registry["claims"].as_array().unwrap() {
        for field in ["implementationRefs", "publicDocsRefs"] {
            for path in claim[field].as_array().unwrap() {
                copy_relative(&source, fixture.path(), path.as_str().unwrap());
            }
        }
    }
    for scenario in registry["scenarios"].as_array().unwrap() {
        copy_relative(
            &source,
            fixture.path(),
            scenario["executable"].as_str().unwrap(),
        );
        copy_relative(
            &source,
            fixture.path(),
            scenario["expected"].as_str().unwrap(),
        );
    }
    fixture
}

fn schema_fixture() -> TempDir {
    let source = repo_root();
    let fixture = TempDir::new("schemas");
    fs::create_dir_all(fixture.path().join("schema")).unwrap();
    for entry in fs::read_dir(source.join("schema")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().expect("schema filename should be UTF-8");
        if name.ends_with(".schema.json") {
            copy_relative(&source, fixture.path(), &format!("schema/{name}"));
        }
    }
    copy_relative(&source, fixture.path(), "proof/schema-lock.json");
    fixture
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn object_mut<'a>(value: &'a mut Value, label: &str) -> Result<&'a mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn array<'a>(value: &'a Value, label: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{label} must be an array"))
}

fn required_text<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    let value = object
        .get(key)
        .ok_or_else(|| format!("{label} is missing {key}"))?;
    let text = value
        .as_str()
        .ok_or_else(|| format!("{label}.{key} must be a string"))?;
    if text.trim().is_empty() {
        return Err(format!("{label}.{key} must be nonempty"));
    }
    Ok(text)
}

fn exact_keys(
    object: &Map<String, Value>,
    required: &[&str],
    optional: &[&str],
    label: &str,
) -> Result<(), String> {
    for key in required {
        if !object.contains_key(*key) {
            return Err(format!("{label} is missing {key}"));
        }
    }
    for key in object.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(format!("{label} has unknown field {key}"));
        }
    }
    Ok(())
}

fn required_relative_file(root: &Path, raw: &str, label: &str) -> Result<PathBuf, String> {
    let normalized = raw.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(":/")
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "..")
    {
        return Err(format!("{label} must be a confined relative path"));
    }
    let path = root.join(&normalized);
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("{label} does not exist: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{label} must be a regular file"));
    }
    Ok(path)
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn strings(value: &Value, label: &str, unique: bool) -> Result<Vec<String>, String> {
    let items = array(value, label)?;
    if items.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let text = item
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| format!("{label} contains a nonempty string requirement"))?;
        if unique && !seen.insert(text) {
            return Err(format!("{label} contains duplicate {text}"));
        }
        result.push(text.to_owned());
    }
    Ok(result)
}

fn validate_registry(root: &Path, registry: &Value) -> Result<(), String> {
    let registry_object = object(registry, "registry")?;
    exact_keys(
        registry_object,
        &["schemaVersion", "claims", "scenarios"],
        &["$schema"],
        "registry",
    )?;
    if registry_object["schemaVersion"] != json!(1) {
        return Err("registry.schemaVersion must be 1".into());
    }
    if let Some(schema) = registry_object.get("$schema") {
        required_text(
            &Map::from_iter([("$schema".to_owned(), schema.clone())]),
            "$schema",
            "registry",
        )?;
    }

    let scenario_values = array(&registry_object["scenarios"], "registry.scenarios")?;
    if scenario_values.is_empty() {
        return Err("registry.scenarios must not be empty".into());
    }
    let mut scenario_ids = BTreeSet::new();
    for (index, scenario) in scenario_values.iter().enumerate() {
        let label = format!("registry.scenarios[{index}]");
        let scenario = object(scenario, &label)?;
        exact_keys(
            scenario,
            &["id", "origin", "command", "executable", "expected"],
            &[],
            &label,
        )?;
        let id = required_text(scenario, "id", &label)?;
        if !scenario_ids.insert(id.to_owned()) {
            return Err(format!("duplicate scenario id {id}"));
        }
        if scenario["origin"] != json!("synthetic") {
            return Err(format!("{label}.origin must be synthetic"));
        }
        required_text(scenario, "command", &label)?;
        let executable_path = required_relative_file(
            root,
            required_text(scenario, "executable", &label)?,
            &format!("{label}.executable"),
        )?;
        if !executable(&executable_path) {
            return Err(format!("{label}.executable is not executable"));
        }
        let expected_path = required_relative_file(
            root,
            required_text(scenario, "expected", &label)?,
            &format!("{label}.expected"),
        )?;
        let expected = serde_json::from_slice::<Value>(&fs::read(&expected_path).unwrap())
            .map_err(|error| format!("{label}.expected is not JSON: {error}"))?;
        if !expected.is_object() {
            return Err(format!("{label}.expected must contain a JSON object"));
        }
    }

    let claim_values = array(&registry_object["claims"], "registry.claims")?;
    if claim_values.is_empty() {
        return Err("registry.claims must not be empty".into());
    }
    let mut claim_ids = BTreeSet::new();
    for (index, claim) in claim_values.iter().enumerate() {
        let label = format!("registry.claims[{index}]");
        let claim = object(claim, &label)?;
        exact_keys(
            claim,
            &[
                "id",
                "statement",
                "scope",
                "evidenceLevel",
                "limitations",
                "scenarioIds",
                "implementationRefs",
                "publicDocsRefs",
            ],
            &[],
            &label,
        )?;
        let id = required_text(claim, "id", &label)?;
        if !claim_ids.insert(id.to_owned()) {
            return Err(format!("duplicate claim id {id}"));
        }
        let statement = required_text(claim, "statement", &label)?;
        required_text(claim, "scope", &label)?;
        required_text(claim, "evidenceLevel", &label)?;

        let limitations = object(&claim["limitations"], &format!("{label}.limitations"))?;
        exact_keys(
            limitations,
            &REQUIRED_LIMITATIONS,
            &[],
            &format!("{label}.limitations"),
        )?;
        for key in REQUIRED_LIMITATIONS {
            required_text(limitations, key, &format!("{label}.limitations"))?;
        }

        let referenced_scenarios =
            strings(&claim["scenarioIds"], &format!("{label}.scenarioIds"), true)?;
        for scenario_id in referenced_scenarios {
            if !scenario_ids.contains(&scenario_id) {
                return Err(format!("{label} references unknown scenario {scenario_id}"));
            }
        }

        let implementation_refs = strings(
            &claim["implementationRefs"],
            &format!("{label}.implementationRefs"),
            true,
        )?;
        for reference in implementation_refs {
            required_relative_file(root, &reference, &format!("{label}.implementationRefs"))?;
        }

        let docs_refs = strings(
            &claim["publicDocsRefs"],
            &format!("{label}.publicDocsRefs"),
            true,
        )?;
        for reference in docs_refs {
            let path =
                required_relative_file(root, &reference, &format!("{label}.publicDocsRefs"))?;
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {reference}: {error}"))?;
            if !source.contains(statement) {
                return Err(format!(
                    "{label}.statement is absent from public document {reference}"
                ));
            }
        }
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn path_version(path: &str) -> Option<u64> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".schema.json")?;
    let marker = stem.rfind("-v")?;
    let digits = &stem[marker + 2..];
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn schema_paths(root: &Path) -> Result<BTreeMap<String, (u64, String)>, String> {
    let schema_root = root.join("schema");
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(schema_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("schema filename is not UTF-8")?;
        if !name.ends_with(".schema.json") {
            continue;
        }
        let relative = format!("schema/{name}");
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!("{relative} must be a regular file"));
        }
        let value = read_json(&path);
        let marker = value.get(FORMAT_MARKER);
        let Some(marker) = marker else {
            continue;
        };
        let format_version = marker
            .as_u64()
            .filter(|version| *version > 0)
            .ok_or_else(|| format!("{relative} format marker must be a positive integer"))?;
        let named_version =
            path_version(&relative).ok_or_else(|| format!("{relative} is not versioned"))?;
        if named_version != format_version {
            return Err(format!(
                "{relative} name version {named_version} does not match format version {format_version}"
            ));
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        if result
            .insert(relative.clone(), (format_version, sha256(&bytes)))
            .is_some()
        {
            return Err(format!("duplicate schema path {relative}"));
        }
    }
    Ok(result)
}

fn lock_entries(lock: &Value) -> Result<BTreeMap<String, (u64, String)>, String> {
    let lock = object(lock, "schema lock")?;
    exact_keys(lock, &["version", "entries"], &[], "schema lock")?;
    if lock["version"] != json!(1) {
        return Err("schema lock version must be 1".into());
    }
    let entries = array(&lock["entries"], "schema lock.entries")?;
    if entries.is_empty() {
        return Err("schema lock.entries must not be empty".into());
    }
    let mut result = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let label = format!("schema lock.entries[{index}]");
        let entry = object(entry, &label)?;
        exact_keys(entry, &["path", "formatVersion", "sha256"], &[], &label)?;
        let path = required_text(entry, "path", &label)?;
        let normalized = path.replace('\\', "/");
        if !normalized.starts_with("schema/")
            || normalized["schema/".len()..].contains('/')
            || normalized.contains(":/")
            || normalized
                .split('/')
                .any(|part| part.is_empty() || part == "..")
        {
            return Err(format!("{label}.path must be schema/*.schema.json"));
        }
        if !normalized.ends_with(".schema.json") {
            return Err(format!("{label}.path must end in .schema.json"));
        }
        let format_version = entry["formatVersion"]
            .as_u64()
            .filter(|version| *version > 0)
            .ok_or_else(|| format!("{label}.formatVersion must be positive"))?;
        let named_version =
            path_version(&normalized).ok_or_else(|| format!("{label}.path is not versioned"))?;
        if named_version != format_version {
            return Err(format!(
                "{label}.path version {named_version} does not match format version {format_version}"
            ));
        }
        let hash = required_text(entry, "sha256", &label)?;
        if !valid_hash(hash) {
            return Err(format!("{label}.sha256 must be lowercase SHA-256"));
        }
        if result
            .insert(normalized.clone(), (format_version, hash.to_owned()))
            .is_some()
        {
            return Err(format!("duplicate schema lock path {normalized}"));
        }
    }
    Ok(result)
}

fn validate_schema_lock(root: &Path, base: Option<&str>) -> Result<(), String> {
    let lock = read_json(&root.join("proof/schema-lock.json"));
    let entries = lock_entries(&lock)?;
    let discovered = schema_paths(root)?;
    for (path, (format_version, hash)) in &discovered {
        let locked = entries
            .get(path)
            .ok_or_else(|| format!("marked schema {path} is absent from lock"))?;
        if locked.0 != *format_version || locked.1 != *hash {
            return Err(format!("schema lock does not match {path}"));
        }
    }
    for (path, (format_version, hash)) in &entries {
        let file = required_relative_file(root, path, "schema lock entry")?;
        let discovered_value = discovered
            .get(path)
            .ok_or_else(|| format!("locked schema {path} has no numeric format marker"))?;
        if discovered_value.0 != *format_version || discovered_value.1 != *hash {
            return Err(format!("schema lock bytes or version mismatch for {path}"));
        }
        let actual = sha256(&fs::read(file).map_err(|error| error.to_string())?);
        if actual != *hash {
            return Err(format!("schema lock hash mismatch for {path}"));
        }
    }
    if let Some(base) = base {
        validate_schema_base(root, &entries, base)?;
    }
    Ok(())
}

fn git(root: &Path, args: &[String]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git executable")
}

fn git_static(root: &Path, args: &[&str]) -> Output {
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    git(root, &args)
}

fn validate_schema_base(
    root: &Path,
    current: &BTreeMap<String, (u64, String)>,
    base: &str,
) -> Result<(), String> {
    if base.trim().is_empty() {
        return Err("VALUE_PROOF_BASE must be nonempty".into());
    }
    let commit_spec = format!("{base}^{{commit}}");
    let revision = git(root, &["rev-parse".into(), "--verify".into(), commit_spec]);
    if !revision.status.success() {
        return Err(format!(
            "VALUE_PROOF_BASE is not a valid Git revision: {base}"
        ));
    }
    let revision = String::from_utf8(revision.stdout)
        .map_err(|_| "VALUE_PROOF_BASE revision is not UTF-8".to_owned())?
        .trim()
        .to_owned();
    let lock_spec = format!("{revision}:proof/schema-lock.json");
    let lock_exists = git(root, &["cat-file".into(), "-e".into(), lock_spec.clone()]);
    if !lock_exists.status.success() {
        return Ok(());
    }
    let shown_lock = git(root, &["show".into(), lock_spec]);
    if !shown_lock.status.success() {
        return Err("could not read base schema lock".into());
    }
    let base_lock = serde_json::from_slice::<Value>(&shown_lock.stdout)
        .map_err(|error| format!("base schema lock is invalid JSON: {error}"))?;
    let base_entries = lock_entries(&base_lock)?;
    for (path, (format_version, hash)) in base_entries {
        let Some(current_entry) = current.get(&path) else {
            return Err(format!("base schema lock entry was removed: {path}"));
        };
        if current_entry.0 != format_version || current_entry.1 != hash {
            return Err(format!("base schema lock entry changed: {path}"));
        }
        let schema_spec = format!("{revision}:{path}");
        let shown_schema = git(root, &["show".into(), schema_spec]);
        if !shown_schema.status.success() {
            return Err(format!("base schema file is unavailable: {path}"));
        }
        let current_bytes = fs::read(root.join(&path)).map_err(|error| error.to_string())?;
        if current_bytes != shown_schema.stdout {
            return Err(format!("base schema bytes changed: {path}"));
        }
    }
    Ok(())
}

fn assert_reject(result: Result<(), String>) {
    assert!(result.is_err(), "expected rejection, got {result:?}");
}

fn add_lock_entry(root: &Path, path: &str, format_version: u64) {
    let schema_path = root.join(path);
    let hash = sha256(&fs::read(schema_path).unwrap());
    let lock_path = root.join("proof/schema-lock.json");
    let mut lock = read_json(&lock_path);
    lock["entries"].as_array_mut().unwrap().push(json!({
        "path": path,
        "formatVersion": format_version,
        "sha256": hash,
    }));
    write_json(&lock_path, &lock);
}

fn remove_lock_path(root: &Path, path: &str) {
    let lock_path = root.join("proof/schema-lock.json");
    let mut lock = read_json(&lock_path);
    lock["entries"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["path"] != json!(path));
    write_json(&lock_path, &lock);
}

fn git_schema_fixture() -> (TempDir, String) {
    let fixture = schema_fixture();
    assert!(git_static(fixture.path(), &["init", "-q"]).status.success());
    assert!(
        git_static(fixture.path(), &["add", "schema", "proof/schema-lock.json"])
            .status
            .success()
    );
    let mut commit = Command::new("git");
    commit
        .arg("-C")
        .arg(fixture.path())
        .arg("-c")
        .arg("user.name=Value Contract Fixture")
        .arg("-c")
        .arg("user.email=value-contract-fixture@example.invalid")
        .args(["commit", "-qm", "base"]);
    assert!(commit.status().unwrap().success());
    let revision = git_static(fixture.path(), &["rev-parse", "HEAD"]);
    assert!(revision.status.success());
    let revision = String::from_utf8(revision.stdout).unwrap();
    (fixture, revision.trim().to_owned())
}

fn git_schema_fixture_without_lock() -> (TempDir, String) {
    let fixture = schema_fixture();
    fs::remove_file(fixture.path().join("proof/schema-lock.json")).unwrap();
    assert!(git_static(fixture.path(), &["init", "-q"]).status.success());
    assert!(git_static(fixture.path(), &["add", "schema"])
        .status
        .success());
    let mut commit = Command::new("git");
    commit
        .arg("-C")
        .arg(fixture.path())
        .arg("-c")
        .arg("user.name=Value Contract Fixture")
        .arg("-c")
        .arg("user.email=value-contract-fixture@example.invalid")
        .args(["commit", "-qm", "base-without-lock"]);
    assert!(commit.status().unwrap().success());
    let revision = git_static(fixture.path(), &["rev-parse", "HEAD"]);
    assert!(revision.status.success());
    let revision = String::from_utf8(revision.stdout).unwrap();
    copy_relative(&repo_root(), fixture.path(), "proof/schema-lock.json");
    (fixture, revision.trim().to_owned())
}

#[test]
fn claim_registry_and_schema_are_valid() {
    let root = repo_root();
    let registry = read_json(&root.join("proof/claims.json"));
    validate_registry(&root, &registry).unwrap();

    let schema = read_json(&root.join("schema/claim-registry-v1.schema.json"));
    assert_eq!(schema[FORMAT_MARKER], json!(1));
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(
        schema["required"].as_array().unwrap(),
        &vec![json!("schemaVersion"), json!("claims"), json!("scenarios")]
    );
}

#[test]
fn registry_rejects_missing_or_unknown_scenarios() {
    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");

    let mut missing = read_json(&path);
    missing["scenarios"] = json!([]);
    write_json(&path, &missing);
    assert_reject(validate_registry(fixture.path(), &missing));

    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut unknown = read_json(&path);
    unknown["claims"][0]["scenarioIds"] = json!(["does-not-exist"]);
    write_json(&path, &unknown);
    assert_reject(validate_registry(fixture.path(), &unknown));
}

#[test]
fn registry_rejects_missing_executable_or_expected() {
    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut missing_executable = read_json(&path);
    object_mut(&mut missing_executable["scenarios"][0], "scenario")
        .unwrap()
        .remove("executable");
    write_json(&path, &missing_executable);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));

    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut missing_expected = read_json(&path);
    object_mut(&mut missing_expected["scenarios"][0], "scenario")
        .unwrap()
        .remove("expected");
    write_json(&path, &missing_expected);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));
}

#[test]
fn registry_rejects_missing_limitation_or_statement() {
    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut missing_limitation = read_json(&path);
    object_mut(
        &mut missing_limitation["claims"][0]["limitations"],
        "limitations",
    )
    .unwrap()
    .remove("local_authority");
    write_json(&path, &missing_limitation);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));

    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut missing_statement = read_json(&path);
    object_mut(&mut missing_statement["claims"][0], "claim")
        .unwrap()
        .remove("statement");
    write_json(&path, &missing_statement);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));
}

#[test]
fn registry_rejects_invalid_version_and_duplicate_ids() {
    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut invalid_version = read_json(&path);
    invalid_version["schemaVersion"] = json!(2);
    write_json(&path, &invalid_version);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));

    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut duplicate = read_json(&path);
    let first = duplicate["claims"][0].clone();
    duplicate["claims"].as_array_mut().unwrap().push(first);
    write_json(&path, &duplicate);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));
}

#[test]
fn registry_rejects_non_synthetic_or_non_executable_scenarios() {
    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let mut non_synthetic = read_json(&path);
    non_synthetic["scenarios"][0]["origin"] = json!("local_report");
    write_json(&path, &non_synthetic);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));

    let fixture = registry_fixture();
    let path = fixture.path().join("proof/claims.json");
    let non_executable = read_json(&path);
    let executable_path = fixture.path().join(
        non_executable["scenarios"][0]["executable"]
            .as_str()
            .unwrap(),
    );
    let mut permissions = fs::metadata(&executable_path).unwrap().permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(executable_path, permissions).unwrap();
    write_json(&path, &non_executable);
    assert_reject(validate_registry(fixture.path(), &read_json(&path)));
}

#[test]
fn schema_lock_matches_all_current_marked_schemas() {
    let root = repo_root();
    let base = env::var("VALUE_PROOF_BASE").ok();
    validate_schema_lock(&root, base.as_deref()).unwrap();
}

#[test]
fn schema_lock_rejects_same_version_byte_change() {
    let fixture = schema_fixture();
    let path = fixture.path().join("schema/value-report-v1.schema.json");
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
    assert_reject(validate_schema_lock(fixture.path(), None));
}

#[test]
fn schema_lock_rejects_removed_old_schema_or_entry() {
    let fixture = schema_fixture();
    fs::remove_file(fixture.path().join("schema/value-report-v1.schema.json")).unwrap();
    assert_reject(validate_schema_lock(fixture.path(), None));

    let fixture = schema_fixture();
    remove_lock_path(fixture.path(), "schema/value-report-v1.schema.json");
    assert_reject(validate_schema_lock(fixture.path(), None));
}

#[test]
fn schema_lock_rejects_changed_old_hash_and_duplicate_path() {
    let fixture = schema_fixture();
    let path = fixture.path().join("proof/schema-lock.json");
    let mut changed_hash = read_json(&path);
    changed_hash["entries"][0]["sha256"] = json!("0".repeat(64));
    write_json(&path, &changed_hash);
    assert_reject(validate_schema_lock(fixture.path(), None));

    let fixture = schema_fixture();
    let path = fixture.path().join("proof/schema-lock.json");
    let mut duplicate = read_json(&path);
    let first = duplicate["entries"][0].clone();
    duplicate["entries"].as_array_mut().unwrap().push(first);
    write_json(&path, &duplicate);
    assert_reject(validate_schema_lock(fixture.path(), None));
}

#[test]
fn schema_lock_rejects_invalid_base_and_allows_first_introduction() {
    let fixture = schema_fixture();
    assert_reject(validate_schema_lock(
        fixture.path(),
        Some("missing-value-contract-base"),
    ));

    let (fixture, revision) = git_schema_fixture_without_lock();
    validate_schema_lock(fixture.path(), Some(&revision)).unwrap();
}

#[test]
fn schema_lock_base_rejects_changed_or_removed_old_entries() {
    let (fixture, revision) = git_schema_fixture();
    let schema_path = fixture.path().join("schema/value-report-v1.schema.json");
    let mut bytes = fs::read(&schema_path).unwrap();
    bytes.push(b'\n');
    let new_hash = sha256(&bytes);
    fs::write(&schema_path, bytes).unwrap();
    let lock_path = fixture.path().join("proof/schema-lock.json");
    let mut lock = read_json(&lock_path);
    for entry in lock["entries"].as_array_mut().unwrap() {
        if entry["path"] == json!("schema/value-report-v1.schema.json") {
            entry["sha256"] = json!(new_hash);
        }
    }
    write_json(&lock_path, &lock);
    assert_reject(validate_schema_lock(fixture.path(), Some(&revision)));

    let (fixture, revision) = git_schema_fixture();
    fs::remove_file(fixture.path().join("schema/value-report-v1.schema.json")).unwrap();
    assert_reject(validate_schema_lock(fixture.path(), Some(&revision)));

    let (fixture, revision) = git_schema_fixture();
    remove_lock_path(fixture.path(), "schema/value-report-v1.schema.json");
    assert_reject(validate_schema_lock(fixture.path(), Some(&revision)));
}

#[test]
fn schema_lock_allows_new_versioned_file_with_base() {
    let (fixture, revision) = git_schema_fixture();
    let old_path = fixture.path().join("schema/value-report-v1.schema.json");
    let new_path = fixture.path().join("schema/value-report-v2.schema.json");
    let mut source = String::from_utf8(fs::read(old_path).unwrap()).unwrap();
    source = source.replace("value-report-v1.schema.json", "value-report-v2.schema.json");
    source = source.replace(
        "\"x-soulmate-format-version\": 1",
        "\"x-soulmate-format-version\": 2",
    );
    fs::write(new_path, source).unwrap();
    add_lock_entry(fixture.path(), "schema/value-report-v2.schema.json", 2);
    validate_schema_lock(fixture.path(), Some(&revision)).unwrap();
}

#[test]
fn schema_lock_rejects_unknown_or_unversioned_lock_paths() {
    let fixture = schema_fixture();
    let path = fixture.path().join("proof/schema-lock.json");
    let mut unknown = read_json(&path);
    unknown["entries"][0]["path"] = json!("schema/missing-v1.schema.json");
    write_json(&path, &unknown);
    assert_reject(validate_schema_lock(fixture.path(), None));

    let fixture = schema_fixture();
    let path = fixture.path().join("proof/schema-lock.json");
    let mut unversioned = read_json(&path);
    unversioned["entries"][0]["path"] = json!("schema/claim-registry.schema.json");
    write_json(&path, &unversioned);
    assert_reject(validate_schema_lock(fixture.path(), None));
}
