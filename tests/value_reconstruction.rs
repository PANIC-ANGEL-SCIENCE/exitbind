use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const FORMAT_MARKER: &str = "x-soulmate-format-version";
const SCHEMA_PATH: &str = "schema/run-event-v3.schema.json";
const BASELINE_PATH: &str = "ledgers/baseline.jsonl";
const BLOCKED_PATH: &str = "ledgers/blocked.jsonl";
const FINAL_PATH: &str = "ledgers/final.jsonl";
const MANIFEST_PATH: &str = "hash-manifest.json";
const FIXTURE_CONFIG: &str = "fixtures/soulmate.json";
const FIXTURE_INVALID: &str = "fixtures/verification-invalid.json";
const FIXTURE_VALID: &str = "fixtures/verification-valid.json";

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "soulmate-value-reconstruction-{label}-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create temporary directory: {error}"),
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

#[derive(Clone)]
struct ManifestEntry {
    sha256: String,
    bytes: u64,
}

type ManifestFiles = BTreeMap<String, ManifestEntry>;
type FixtureFiles = BTreeMap<String, Vec<u8>>;
type EventShape = (BTreeSet<String>, BTreeSet<String>);
type EventShapes = BTreeMap<String, EventShape>;

struct BundleSnapshot {
    fixture_paths: BTreeSet<String>,
}

#[test]
fn exported_bundle_reconstructs_without_result_or_report() {
    let parent = TempDir::new("run");
    let destination = parent.path().join("bundle");
    let destination_text = destination
        .to_str()
        .expect("temporary output path should be UTF-8");
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(["benchmark", "--output", destination_text, "--json"])
        .output()
        .expect("benchmark binary should start");
    assert!(
        output.status.success(),
        "benchmark failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let snapshot = reconstruct_bundle(&destination).expect("bundle should reconstruct");

    let tampered = parent.path().join("tampered");
    copy_tree(&destination, &tampered).expect("bundle copy should succeed");
    let tamper_relative = snapshot
        .fixture_paths
        .iter()
        .find(|path| {
            let path = path.as_str();
            path != FIXTURE_CONFIG
                && path != FIXTURE_INVALID
                && path != FIXTURE_VALID
                && path.ends_with(".md")
        })
        .expect("bundle should contain an artifact fixture")
        .clone();
    let tamper_path = tampered.join(tamper_relative);
    let mut tampered_bytes = fs::read(&tamper_path).expect("tamper fixture should be readable");
    tampered_bytes[0] ^= 1;
    fs::write(tamper_path, tampered_bytes).expect("tamper fixture should be writable");
    assert!(
        reconstruct_bundle(&tampered).is_err(),
        "manifest-bound artifact tampering must reject reconstruction"
    );
}

#[test]
fn current_and_legacy_benchmarks_report_their_selected_surface() {
    for (binary, product, producer, format_version, config, state, forbidden) in [
        (
            env!("CARGO_BIN_EXE_exitbind"),
            "Exitbind",
            "exitbind",
            5,
            "exitbind.json",
            ".exitbind",
            "Soulmate",
        ),
        (
            env!("CARGO_BIN_EXE_soulmate"),
            "Soulmate",
            "soulmate",
            4,
            "soulmate.json",
            ".soulmate",
            "Exitbind",
        ),
    ] {
        let output = Command::new(binary)
            .args(["benchmark", "--json"])
            .output()
            .expect("benchmark binary should start");
        assert!(
            output.status.success(),
            "benchmark failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).expect("benchmark JSON");
        assert_eq!(value["product"], product);
        assert_eq!(value["producer"], producer);
        assert_eq!(value["formatVersion"], format_version);
        assert_eq!(value["layout"]["config"], config);
        assert_eq!(value["layout"]["state"], state);
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains(forbidden),
            "selected benchmark leaked the other product surface"
        );
    }
}

#[test]
fn current_away_surface_names_exitbind_configuration_and_state() {
    let parent = TempDir::new("away-surface");
    let missing = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(parent.path())
        .args(["away", "list"])
        .output()
        .expect("exitbind should start");
    assert!(!missing.status.success());
    let missing_text = format!(
        "{}{}",
        String::from_utf8_lossy(&missing.stdout),
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(missing_text.contains("exitbind.json"));
    assert!(!missing_text.contains("soulmate.json"));

    let project = parent.path().join("project");
    fs::create_dir(&project).expect("project directory");
    let initialized = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&project)
        .args(["init", "--mode", "portable"])
        .output()
        .expect("exitbind init should start");
    assert!(initialized.status.success());
    let listed = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&project)
        .args(["away", "list"])
        .output()
        .expect("exitbind away should start");
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("no Exitbind away runs"));
}

#[test]
fn event_shapes_rejects_closed_all_of_fragment_missing_common_properties() {
    let schema = parse_json(
        include_bytes!("../schema/run-event-v3.schema.json"),
        SCHEMA_PATH,
    )
    .expect("checked-in event schema should be valid JSON");
    let mut mutated = schema;
    mutated["$defs"]["start"]["allOf"][1]["additionalProperties"] = Value::Bool(false);

    let error = event_shapes(&mutated).expect_err(
        "a closed action fragment that omits common fields must reject the composed schema",
    );
    assert!(
        error.contains("closed allOf fragment") && error.contains("producer"),
        "unexpected structural error: {error}"
    );
}

fn reconstruct_bundle(root: &Path) -> Result<BundleSnapshot, String> {
    let (entries, fixtures) = validate_manifest(root)?;
    let schema_path = "schema/run-event-v4.schema.json";
    let schema_bytes = manifest_bytes(root, &entries, schema_path)?;
    let schema = parse_json(&schema_bytes, schema_path)?;
    let shapes = event_shapes(&schema)?;
    let blocked_bytes = manifest_bytes(root, &entries, BLOCKED_PATH)?;
    let final_bytes = manifest_bytes(root, &entries, FINAL_PATH)?;
    let blocked = validate_ledger(&blocked_bytes, &shapes, "blocked ledger")?;
    let final_events = validate_ledger(&final_bytes, &shapes, "final ledger")?;

    let config = fixtures
        .get(FIXTURE_CONFIG)
        .ok_or("bundle is missing its config fixture")?;
    let invalid = fixtures
        .get(FIXTURE_INVALID)
        .ok_or("bundle is missing its invalid verification fixture")?;
    let valid = fixtures
        .get(FIXTURE_VALID)
        .ok_or("bundle is missing its valid verification fixture")?;
    if parse_json(config, FIXTURE_CONFIG)?.as_object().is_none()
        || parse_json(invalid, FIXTURE_INVALID)?.as_object().is_none()
        || parse_json(valid, FIXTURE_VALID)?.as_object().is_none()
        || invalid == valid
    {
        return Err("verification fixtures do not form two JSON object states".into());
    }

    reconstruct_workflow(&blocked, &final_events, config, &fixtures)
        .map_err(|error| format!("independent reconstruction failed: {error}"))?;
    Ok(BundleSnapshot {
        fixture_paths: fixtures.keys().cloned().collect(),
    })
}

fn validate_manifest(root: &Path) -> Result<(ManifestFiles, FixtureFiles), String> {
    let manifest_file_bytes = read_regular(root, MANIFEST_PATH)?;
    let manifest = parse_json(&manifest_file_bytes, MANIFEST_PATH)?;
    let manifest_object = object(&manifest, MANIFEST_PATH)?;
    if manifest_object.get("version") != Some(&Value::from(1))
        || manifest_object.get("self") != Some(&Value::from("excluded"))
    {
        return Err("hash manifest must be version 1 with self excluded".into());
    }
    let files = manifest_object
        .get("files")
        .and_then(Value::as_array)
        .filter(|files| !files.is_empty())
        .ok_or("hash manifest files must be nonempty")?;
    let mut entries = BTreeMap::new();
    for (index, item) in files.iter().enumerate() {
        let label = format!("hash manifest files[{index}]");
        let item = object(item, &label)?;
        let path = text_field(item, "path", &label)?;
        confined(path, &label)?;
        if path == MANIFEST_PATH {
            return Err("hash manifest must exclude itself".into());
        }
        let hash = text_field(item, "sha256", &label)?;
        if !valid_hash(hash) {
            return Err(format!("{label}.sha256 is not lowercase SHA-256"));
        }
        let bytes = item
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{label}.bytes must be an integer"))?;
        if entries
            .insert(
                path.to_owned(),
                ManifestEntry {
                    sha256: hash.to_owned(),
                    bytes,
                },
            )
            .is_some()
        {
            return Err(format!("hash manifest repeats {path}"));
        }
    }

    let mut physical = BTreeSet::new();
    collect_files(root, root, &mut physical)?;
    physical.remove(MANIFEST_PATH);
    let listed = entries.keys().cloned().collect::<BTreeSet<_>>();
    if physical != listed {
        return Err("hash manifest file set differs from bundle file set".into());
    }

    let mut fixtures = BTreeMap::new();
    for path in &listed {
        if is_allowed(path) {
            let bytes = manifest_bytes(root, &entries, path)?;
            if path.starts_with("fixtures/") {
                fixtures.insert(path.clone(), bytes);
            }
        } else if !is_outcome(path) {
            return Err(format!("unexpected bundle path {path}"));
        }
    }
    for required in [SCHEMA_PATH, BLOCKED_PATH, FINAL_PATH] {
        if !entries.contains_key(required) {
            return Err(format!("hash manifest is missing {required}"));
        }
    }
    for required in [FIXTURE_CONFIG, FIXTURE_INVALID, FIXTURE_VALID] {
        if !fixtures.contains_key(required) {
            return Err(format!("hash manifest is missing {required}"));
        }
    }
    if !fixtures
        .keys()
        .any(|path| *path != FIXTURE_CONFIG && *path != FIXTURE_INVALID && *path != FIXTURE_VALID)
    {
        return Err("hash manifest contains no fixture artifact".into());
    }
    Ok((entries, fixtures))
}

fn is_allowed(path: &str) -> bool {
    path == SCHEMA_PATH
        || path == BLOCKED_PATH
        || path == FINAL_PATH
        || path.starts_with("fixtures/")
}

fn is_outcome(path: &str) -> bool {
    matches!(
        path,
        BASELINE_PATH
            | "schema/run-event-v4.schema.json"
            | "schema/value-proof-v1.schema.json"
            | "schema/value-report-v1.schema.json"
            | "result.json"
            | "result.md"
            | "report.json"
            | "report.md"
            | "scenario/README.md"
            | "scenario/expected.json"
    )
}

fn manifest_bytes(
    root: &Path,
    entries: &BTreeMap<String, ManifestEntry>,
    relative: &str,
) -> Result<Vec<u8>, String> {
    let entry = entries
        .get(relative)
        .ok_or_else(|| format!("hash manifest has no {relative}"))?;
    let bytes = read_regular(root, relative)?;
    if bytes.len() as u64 != entry.bytes || sha256(&bytes) != entry.sha256 {
        return Err(format!("hash manifest mismatch for {relative}"));
    }
    Ok(bytes)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("bundle contains a symlink".into());
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "bundle file escapes root")?
                .to_str()
                .ok_or("bundle file path is not UTF-8")?
                .replace('\\', "/");
            files.insert(relative);
        } else {
            return Err("bundle contains a non-file entry".into());
        }
    }
    Ok(())
}

fn event_shapes(schema: &Value) -> Result<EventShapes, String> {
    if !matches!(
        schema.get(FORMAT_MARKER).and_then(Value::as_u64),
        Some(3 | 4)
    ) || schema.get("type") != Some(&Value::from("object"))
    {
        return Err("exported run-event schema is not version 3".into());
    }
    let one_of = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .ok_or("exported run-event schema has no oneOf")?;
    let expected_refs = [
        "#/$defs/start",
        "#/$defs/submit",
        "#/$defs/check",
        "#/$defs/protect",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    let refs = one_of
        .iter()
        .filter_map(|item| item.get("$ref").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if refs != expected_refs {
        return Err("exported run-event schema action definitions changed".into());
    }
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .ok_or("exported run-event schema has no definitions")?;
    let common = defs
        .get("common")
        .ok_or("schema has no common event shape")?;
    let common_required = string_set(common.get("required"), "common required fields")?;
    let common_properties = property_set(common, "common properties")?;
    let mut shapes = BTreeMap::new();
    for action in ["start", "submit", "check", "protect"] {
        let definition = defs
            .get(action)
            .ok_or_else(|| format!("schema has no {action} definition"))?;
        if definition.get("unevaluatedProperties") != Some(&Value::from(false)) {
            return Err(format!(
                "schema {action} must close composed event properties with unevaluatedProperties"
            ));
        }
        let action_parts = definition
            .get("allOf")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("schema {action} has no allOf"))?;
        let mut required = common_required.clone();
        let mut properties = common_properties.clone();
        let mut fragments = Vec::new();
        let mut saw_action_object = false;
        for part in action_parts {
            if part.get("$ref").is_some() {
                continue;
            }
            let object = object(part, &format!("schema {action} action object"))?;
            let fragment_properties = property_set(part, &format!("schema {action} properties"))?;
            required.extend(string_set(
                object.get("required"),
                &format!("schema {action} required fields"),
            )?);
            properties.extend(fragment_properties.iter().cloned());
            fragments.push((
                fragment_properties,
                object.get("additionalProperties") == Some(&Value::from(false)),
            ));
            if object
                .get("properties")
                .and_then(|properties| properties.get("action"))
                .and_then(|action_value| action_value.get("const"))
                .and_then(Value::as_str)
                == Some(action)
            {
                saw_action_object = true;
            }
        }
        if !saw_action_object {
            return Err(format!("schema {action} has no action discriminator"));
        }
        for (fragment_properties, closed) in fragments {
            if closed {
                let missing = properties
                    .difference(&fragment_properties)
                    .cloned()
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    return Err(format!(
                        "schema {action} closed allOf fragment omits composed properties: {}",
                        missing.join(", ")
                    ));
                }
            }
        }
        shapes.insert(action.to_owned(), (required, properties));
    }
    Ok(shapes)
}

fn string_set(value: Option<&Value>, label: &str) -> Result<BTreeSet<String>, String> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{label} must contain strings"))
        })
        .collect()
}

fn property_set(value: &Value, label: &str) -> Result<BTreeSet<String>, String> {
    value
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{label} must have properties"))
        .map(|properties| properties.keys().cloned().collect())
}

fn validate_ledger(
    bytes: &[u8],
    shapes: &BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    label: &str,
) -> Result<Vec<Value>, String> {
    let source =
        std::str::from_utf8(bytes).map_err(|error| format!("{label} is not UTF-8: {error}"))?;
    let events = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map_err(|error| format!("{label} has invalid JSON: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if events.is_empty() {
        return Err(format!("{label} is empty"));
    }
    let run_id = events[0]
        .get("runId")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} start has no runId"))?
        .to_owned();
    let mut previous: Option<String> = None;
    for (index, event) in events.iter().enumerate() {
        validate_event(event, shapes, &format!("{label} event {}", index + 1))?;
        if event["runId"] != run_id {
            return Err(format!("{label} changed runId"));
        }
        let event_hash = event["eventSha256"]
            .as_str()
            .ok_or_else(|| format!("{label} has no event hash"))?;
        if event_hash != sha256(canonical(&canonical_without_event_hash(event)).as_bytes()) {
            return Err(format!("{label} event {} hash mismatch", index + 1));
        }
        match previous.as_deref() {
            None if event["previousEventSha256"] != Value::Null => {
                return Err(format!("{label} does not begin a chain"))
            }
            Some(previous) if event["previousEventSha256"] != previous => {
                return Err(format!("{label} event {} chain mismatch", index + 1))
            }
            _ => {}
        }
        previous = Some(event_hash.to_owned());
    }
    Ok(events)
}

fn validate_event(
    event: &Value,
    shapes: &BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    label: &str,
) -> Result<(), String> {
    let object = object(event, label)?;
    if !matches!(event.get("version").and_then(Value::as_u64), Some(3 | 4))
        || event.get("kind") != Some(&Value::from("run"))
    {
        return Err(format!("{label} is not a version 3 run event"));
    }
    let action = event
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} has no action"))?;
    let (required, properties) = shapes
        .get(action)
        .ok_or_else(|| format!("{label} has unknown action {action}"))?;
    for field in required {
        if !object.contains_key(field) {
            return Err(format!("{label} is missing {field}"));
        }
    }
    if object.keys().any(|field| !properties.contains(field)) {
        return Err(format!("{label} contains an unknown field"));
    }
    if !valid_hash(event["runId"].as_str().unwrap_or(""))
        || !valid_hash(event["eventSha256"].as_str().unwrap_or(""))
    {
        return Err(format!("{label} has an invalid identity hash"));
    }
    if event["producer"]["name"] != "soulmate"
        || event["producer"]["version"]
            .as_str()
            .map_or(true, str::is_empty)
        || !(event["producer"]["commit"].is_null() || event["producer"]["commit"].is_string())
    {
        return Err(format!("{label} has invalid producer evidence"));
    }
    if event["timestamp"].as_str().map_or(true, str::is_empty) {
        return Err(format!("{label} has no timestamp"));
    }
    match action {
        "start" => validate_start(event, label),
        "submit" => validate_submit(event, label),
        "check" => validate_check(event, label),
        "protect" => validate_protect(event, label),
        _ => Err(format!("{label} has unsupported action")),
    }
}

fn validate_start(event: &Value, label: &str) -> Result<(), String> {
    if event["previousEventSha256"] != Value::Null
        || event["workflow"].as_str().map_or(true, str::is_empty)
        || event["goal"].as_str().map_or(true, str::is_empty)
        || !valid_hash(event["configSha256"].as_str().unwrap_or(""))
    {
        return Err(format!("{label} has invalid start evidence"));
    }
    let policy = object(&event["checkPolicy"], &format!("{label} check policy"))?;
    if policy.get("version") != Some(&Value::from(1))
        || policy.get("origin") != Some(&Value::from("synthetic"))
        || policy
            .get("command")
            .and_then(Value::as_str)
            .map_or(true, str::is_empty)
        || !valid_hash(
            policy
                .get("commandSha256")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
    {
        return Err(format!("{label} has invalid synthetic check policy"));
    }
    Ok(())
}

fn validate_submit(event: &Value, label: &str) -> Result<(), String> {
    if event["stage"].as_u64().map_or(true, |value| value == 0)
        || event["attempt"].as_u64().map_or(true, |value| value == 0)
        || event["agent"].as_str().map_or(true, str::is_empty)
        || event["outcome"].as_str().map_or(true, str::is_empty)
        || !matches!(
            event["role"].as_str(),
            Some("lead" | "worker" | "reviewer" | "adviser")
        )
    {
        return Err(format!("{label} has invalid submission identity"));
    }
    let artifact = object(&event["artifact"], &format!("{label} artifact"))?;
    if !matches!(
        artifact.get("root").and_then(Value::as_str),
        Some("product" | "state")
    ) || artifact
        .get("path")
        .and_then(Value::as_str)
        .map_or(true, |path| {
            confined(path, &format!("{label} artifact")).is_err()
        })
        || !valid_hash(artifact.get("sha256").and_then(Value::as_str).unwrap_or(""))
    {
        return Err(format!("{label} has invalid artifact evidence"));
    }
    Ok(())
}

fn validate_check(event: &Value, label: &str) -> Result<(), String> {
    if event["version"] == 4 {
        if !valid_hash(event["targetEventSha256"].as_str().unwrap_or(""))
            || event["checkCommand"].as_str().map_or(true, str::is_empty)
            || !valid_hash(event["checkCommandSha256"].as_str().unwrap_or(""))
            || event["origin"] != "synthetic"
            || !matches!(event["acquisition"].as_str(), Some("reported" | "observed"))
            || event["result"].get("kind").is_none()
        {
            return Err(format!("{label} has invalid check evidence"));
        }
        return Ok(());
    }
    if !valid_hash(event["targetEventSha256"].as_str().unwrap_or(""))
        || event["checkCommand"].as_str().map_or(true, str::is_empty)
        || !valid_hash(event["checkCommandSha256"].as_str().unwrap_or(""))
        || event["origin"] != "synthetic"
        || event["exitCode"].as_u64().is_none()
    {
        return Err(format!("{label} has invalid check evidence"));
    }
    Ok(())
}

fn validate_protect(event: &Value, label: &str) -> Result<(), String> {
    if event["version"] == 4 {
        let evidence = event["checkEvidence"]
            .as_array()
            .filter(|evidence| !evidence.is_empty())
            .ok_or_else(|| format!("{label} has no check evidence"))?;
        if event["reason"] != "check_failed" || event["origin"] != "synthetic" {
            return Err(format!("{label} has invalid protection evidence"));
        }
        for item in evidence {
            if item["status"] != "failed"
                || !valid_hash(item["targetEventSha256"].as_str().unwrap_or(""))
                || !valid_hash(item["checkEventSha256"].as_str().unwrap_or(""))
                || item["result"].get("kind").is_none()
            {
                return Err(format!("{label} has invalid failed-check evidence"));
            }
        }
        return Ok(());
    }
    if event["stage"].as_u64().map_or(true, |value| value == 0)
        || event["attempt"].as_u64().map_or(true, |value| value == 0)
        || event["actor"].as_str().map_or(true, str::is_empty)
        || event["role"] != "lead"
        || event["attemptedOutcome"] != "accepted"
        || event["reason"] != "check_failed"
        || event["origin"] != "synthetic"
    {
        return Err(format!("{label} has invalid protection evidence"));
    }
    let evidence = event["checkEvidence"]
        .as_array()
        .filter(|evidence| !evidence.is_empty())
        .ok_or_else(|| format!("{label} has no check evidence"))?;
    for item in evidence {
        let item = object(item, &format!("{label} check evidence"))?;
        if item.get("status") != Some(&Value::from("failed"))
            || !valid_hash(
                item.get("targetEventSha256")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            || !valid_hash(
                item.get("checkEventSha256")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            || item.get("exitCode").and_then(Value::as_u64).is_none()
        {
            return Err(format!("{label} has invalid failed-check evidence"));
        }
    }
    Ok(())
}

fn reconstruct_workflow(
    blocked: &[Value],
    final_events: &[Value],
    config: &[u8],
    fixtures: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    if blocked.len() != 6
        || final_events.len() != 11
        || blocked
            .iter()
            .map(|event| event["action"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            != ["start", "submit", "submit", "check", "submit", "protect"]
        || final_events
            .iter()
            .map(|event| event["action"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            != [
                "start", "submit", "submit", "check", "submit", "protect", "submit", "submit",
                "check", "submit", "submit",
            ]
    {
        return Err("ledgers do not contain the fixed synthetic workflow sequence".into());
    }
    if blocked != &final_events[..blocked.len()] {
        return Err("final ledger does not preserve the blocked prefix".into());
    }
    let start = &blocked[0];
    if start["workflow"] != "change"
        || start["checkPolicy"]["origin"] != "synthetic"
        || start["configSha256"] != sha256(config)
    {
        return Err("start event is not bound to the exported synthetic fixture".into());
    }
    verify_plan_roles(start)?;
    let command = start["checkPolicy"]["command"]
        .as_str()
        .ok_or("start event has no check command")?;
    let worker_one = &blocked[2];
    let check_one = &blocked[3];
    let protection = &blocked[5];
    assert_submission(worker_one, 2, 1, "worker", "worker", "completed")?;
    assert_submission(&blocked[1], 1, 1, "lead", "lead", "scoped")?;
    assert_submission(&blocked[4], 3, 1, "reviewer", "reviewer", "approved")?;
    assert_check(check_one, worker_one, command, 1)?;
    if protection["checkEvidence"][0]["targetEventSha256"] != worker_one["eventSha256"]
        || protection["checkEvidence"][0]["checkEventSha256"] != check_one["eventSha256"]
        || check_exit_code(&protection["checkEvidence"][0]) != Some(1)
        || has_acceptance(blocked)
    {
        return Err("failed-check protection did not leave a pending lead decision".into());
    }

    let worker_two = &final_events[7];
    let check_two = &final_events[8];
    assert_submission(&final_events[6], 4, 1, "lead", "lead", "rework")?;
    assert_submission(worker_two, 2, 2, "worker", "worker", "completed")?;
    assert_submission(&final_events[9], 3, 2, "reviewer", "reviewer", "approved")?;
    assert_submission(&final_events[10], 4, 2, "lead", "lead", "accepted")?;
    assert_check(check_two, worker_two, command, 0)?;
    if has_acceptance(&final_events[..10])
        || final_events[10]["agent"] != "lead"
        || final_events[10]["role"] != "lead"
    {
        return Err("canonical lead acceptance was not last after fresh review".into());
    }
    verify_artifacts(final_events, fixtures)?;
    let worker_one_artifact = artifact_key(worker_one)?;
    let worker_two_artifact = artifact_key(worker_two)?;
    if worker_one_artifact == worker_two_artifact
        || artifact_bytes(worker_one, fixtures)? == artifact_bytes(worker_two, fixtures)?
        || artifact_key_from(final_events, worker_one)? != worker_one_artifact
    {
        return Err("worker rework did not preserve a distinct prior artifact".into());
    }
    Ok(())
}

fn verify_plan_roles(start: &Value) -> Result<(), String> {
    let stages = start["plan"]["stages"]
        .as_array()
        .filter(|stages| stages.len() == 4)
        .ok_or("start plan does not expose four workflow stages")?;
    for (index, expected_role) in ["lead", "worker", "reviewer", "lead"]
        .into_iter()
        .enumerate()
    {
        if stages[index]["stage"] != index + 1
            || !stages[index]["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == expected_role))
        {
            return Err(format!("start plan stage {} has the wrong role", index + 1));
        }
    }
    Ok(())
}

fn assert_submission(
    event: &Value,
    stage: u64,
    attempt: u64,
    agent: &str,
    role: &str,
    outcome: &str,
) -> Result<(), String> {
    if event["stage"] != stage
        || event["attempt"] != attempt
        || event["agent"] != agent
        || event["role"] != role
        || event["outcome"] != outcome
    {
        return Err(format!("unexpected submission {agent}/{outcome}"));
    }
    Ok(())
}

fn assert_check(
    event: &Value,
    worker: &Value,
    command: &str,
    exit_code: u64,
) -> Result<(), String> {
    if event["targetEventSha256"] != worker["eventSha256"]
        || event["checkCommand"] != command
        || event["checkCommandSha256"] != sha256(command.as_bytes())
        || event["origin"] != "synthetic"
        || check_exit_code(event) != Some(exit_code)
    {
        return Err(format!("check does not bind to worker exit {exit_code}"));
    }
    Ok(())
}

fn check_exit_code(event: &Value) -> Option<u64> {
    event["exitCode"].as_u64().or_else(|| {
        (event["result"]["kind"] == "exit")
            .then(|| event["result"]["code"].as_u64())
            .flatten()
    })
}

fn verify_artifacts(events: &[Value], fixtures: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let mut artifacts = BTreeSet::new();
    for event in events {
        if event["action"] != "submit" {
            continue;
        }
        let artifact = object(&event["artifact"], "submission artifact")?;
        let root = artifact["root"].as_str().ok_or("artifact has no root")?;
        let path = artifact["path"].as_str().ok_or("artifact has no path")?;
        confined(path, "submission artifact")?;
        let bundle_path = format!("fixtures/{path}");
        let bytes = fixtures
            .get(&bundle_path)
            .ok_or_else(|| format!("bundle lacks artifact {root}:{path}"))?;
        let hash = artifact["sha256"].as_str().ok_or("artifact has no hash")?;
        if sha256(bytes) != hash || !artifacts.insert((root, path, hash)) {
            return Err(format!(
                "artifact bytes or identity mismatch for {root}:{path}"
            ));
        }
    }
    if artifacts.len() != 7 {
        return Err(format!(
            "expected seven distinct protected submission artifacts, got {}",
            artifacts.len()
        ));
    }
    Ok(())
}

fn artifact_key(event: &Value) -> Result<(String, String, String), String> {
    let artifact = object(&event["artifact"], "worker artifact")?;
    Ok((
        artifact["root"].as_str().ok_or("artifact root")?.to_owned(),
        artifact["path"].as_str().ok_or("artifact path")?.to_owned(),
        artifact["sha256"]
            .as_str()
            .ok_or("artifact hash")?
            .to_owned(),
    ))
}

fn artifact_key_from(events: &[Value], worker: &Value) -> Result<(String, String, String), String> {
    let target = worker["eventSha256"].as_str().ok_or("worker hash")?;
    let event = events
        .iter()
        .find(|event| event["action"] == "submit" && event["eventSha256"] == target)
        .ok_or_else(|| "prior worker event missing from final ledger".to_owned())?;
    artifact_key(event)
}

fn artifact_bytes(event: &Value, fixtures: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let path = event["artifact"]["path"].as_str().ok_or("artifact path")?;
    fixtures
        .get(&format!("fixtures/{path}"))
        .cloned()
        .ok_or_else(|| format!("artifact fixture missing for {path}"))
}

fn has_acceptance(events: &[Value]) -> bool {
    events.iter().any(|event| {
        event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "accepted"
    })
}

fn canonical_without_event_hash(event: &Value) -> Value {
    let mut object = event.as_object().expect("event object").clone();
    object.remove("eventSha256");
    Value::Object(object)
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON key should serialize"),
                        canonical(&object[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).expect("JSON value should serialize"),
    }
}

fn parse_json(bytes: &[u8], label: &str) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("{label} is invalid JSON: {error}"))
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn text_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{label}.{key} must be nonempty text"))
}

fn confined(path: &str, label: &str) -> Result<(), String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("{label} is not a confined relative path"));
    }
    Ok(())
}

fn read_regular(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    confined(relative, "bundle path")?;
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path).map_err(|error| format!("{relative}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{relative} is not a regular file"));
    }
    fs::read(path).map_err(|error| format!("{relative}: {error}"))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("cannot copy a symlinked bundle entry".into());
        }
        if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(source_path, destination_path).map_err(|error| error.to_string())?;
        } else {
            return Err("cannot copy a non-file bundle entry".into());
        }
    }
    Ok(())
}
