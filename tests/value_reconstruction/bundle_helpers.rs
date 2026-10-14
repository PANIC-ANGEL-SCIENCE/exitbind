fn reconstruct_bundle(root: &Path) -> Result<BundleSnapshot, String> {
    let (entries, fixtures) = validate_manifest(root)?;
    let config_path = if fixtures.contains_key(CURRENT_FIXTURE_CONFIG) {
        CURRENT_FIXTURE_CONFIG
    } else if fixtures.contains_key(HISTORICAL_FIXTURE_CONFIG) {
        HISTORICAL_FIXTURE_CONFIG
    } else {
        return Err("bundle has no supported product config fixture".into());
    };
    let (schema_path, schema_version, format_version, producer) = match config_path {
        CURRENT_FIXTURE_CONFIG => (V5_SCHEMA_PATH, 5, 8, "exitbind"),
        HISTORICAL_FIXTURE_CONFIG => (V4_SCHEMA_PATH, 4, 4, "soulmate"),
        _ => unreachable!(),
    };
    if !entries.contains_key(schema_path) {
        return Err(format!("hash manifest is missing {schema_path}"));
    }
    let schema_bytes = manifest_bytes(root, &entries, schema_path)?;
    let schema = parse_json(&schema_bytes, schema_path)?;
    let mut shapes = event_shapes(&schema, schema_version)?;
    if format_version == 8 {
        // The export retains historical schemas through v5. This independent
        // reader adds only the fixed v8 synthetic scenario's closed fields;
        // it neither claims an exported v8 schema nor derives fields from data.
        for (action, required, allowed) in [
            ("start", &["governor"][..], &["governor"][..]),
            ("submit", &[][..], &["inputsSha256", "governorEvent"][..]),
            ("check", &["inputsSha256"][..], &["inputsSha256"][..]),
            ("protect", &["inputsSha256"][..], &["inputsSha256"][..]),
        ] {
            let (required_fields, allowed_fields) = shapes.get_mut(action).unwrap();
            required_fields.extend(required.iter().map(|field| (*field).to_owned()));
            allowed_fields.extend(allowed.iter().map(|field| (*field).to_owned()));
        }
    }
    let blocked_bytes = manifest_bytes(root, &entries, BLOCKED_PATH)?;
    let final_bytes = manifest_bytes(root, &entries, FINAL_PATH)?;
    let blocked = validate_ledger(
        &blocked_bytes,
        &shapes,
        format_version,
        producer,
        "blocked ledger",
    )?;
    let final_events = validate_ledger(
        &final_bytes,
        &shapes,
        format_version,
        producer,
        "final ledger",
    )?;

    let config = fixtures
        .get(config_path)
        .ok_or("bundle is missing its config fixture")?;
    let invalid = fixtures
        .get(FIXTURE_INVALID)
        .ok_or("bundle is missing its invalid verification fixture")?;
    let valid = fixtures
        .get(FIXTURE_VALID)
        .ok_or("bundle is missing its valid verification fixture")?;
    if parse_json(config, config_path)?.as_object().is_none()
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
    for required in [BLOCKED_PATH, FINAL_PATH] {
        if !entries.contains_key(required) {
            return Err(format!("hash manifest is missing {required}"));
        }
    }
    let config_path = if fixtures.contains_key(CURRENT_FIXTURE_CONFIG) {
        CURRENT_FIXTURE_CONFIG
    } else if fixtures.contains_key(HISTORICAL_FIXTURE_CONFIG) {
        HISTORICAL_FIXTURE_CONFIG
    } else {
        return Err("hash manifest has no supported product config fixture".into());
    };
    if fixtures.contains_key(CURRENT_FIXTURE_CONFIG)
        && fixtures.contains_key(HISTORICAL_FIXTURE_CONFIG)
    {
        return Err("hash manifest mixes current and historical product configs".into());
    }
    for required in [config_path, FIXTURE_INVALID, FIXTURE_VALID] {
        if !fixtures.contains_key(required) {
            return Err(format!("hash manifest is missing {required}"));
        }
    }
    if !fixtures.keys().any(|path| {
        !FIXTURE_CONFIGS.contains(&path.as_str())
            && path != FIXTURE_INVALID
            && path != FIXTURE_VALID
    }) {
        return Err("hash manifest contains no fixture artifact".into());
    }
    Ok((entries, fixtures))
}

fn is_allowed(path: &str) -> bool {
    matches!(path, V3_SCHEMA_PATH | V4_SCHEMA_PATH | V5_SCHEMA_PATH)
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

fn event_shapes(schema: &Value, expected_version: u64) -> Result<EventShapes, String> {
    let marker = if expected_version == 5 {
        EXITBIND_FORMAT_MARKER
    } else {
        FORMAT_MARKER
    };
    if schema.get(marker).and_then(Value::as_u64) != Some(expected_version)
        || schema.get("type") != Some(&Value::from("object"))
    {
        return Err(format!(
            "exported run-event schema does not match version {expected_version}"
        ));
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
