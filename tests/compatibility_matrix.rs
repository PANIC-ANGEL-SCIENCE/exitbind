use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const MATRIX_PATH: &str = "compatibility/rename-matrix.json";

fn exact_object<'a>(
    value: &'a Value,
    keys: &[&str],
    at: &str,
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{at} must be an object"))?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(format!("{at} has unknown or missing fields"));
    }
    Ok(object)
}

fn array<'a>(object: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a [Value], String> {
    object[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{at}.{key} must be an array"))
}

fn text<'a>(object: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a str, String> {
    object[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{at}.{key} must be a non-empty string"))
}

fn id_set(rows: &[Value], at: &str) -> Result<BTreeSet<String>, String> {
    let mut ids = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let object = row
            .as_object()
            .ok_or_else(|| format!("{at}[{index}] must be an object"))?;
        let id = text(object, "id", &format!("{at}[{index}]"))?;
        if !ids.insert(id.to_owned()) {
            return Err(format!("duplicate {at} id {id}"));
        }
    }
    Ok(ids)
}

fn string_array(object: &Map<String, Value>, key: &str, at: &str) -> Result<Vec<String>, String> {
    array(object, key, at)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_str()
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{at}.{key}[{index}] must be a non-empty string"))
        })
        .collect()
}

fn sha256(object: &Map<String, Value>, key: &str, at: &str) -> Result<(), String> {
    let value = text(object, key, at)?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{at}.{key} must be a SHA-256 digest"));
    }
    Ok(())
}

fn sha1(object: &Map<String, Value>, key: &str, at: &str) -> Result<(), String> {
    let value = text(object, key, at)?;
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{at}.{key} must be a commit/tree digest"));
    }
    Ok(())
}

pub fn parse_matrix(value: &Value) -> Result<(), String> {
    let root = exact_object(
        value,
        &[
            "schemaVersion",
            "matrixId",
            "contractDigest",
            "amendmentDigest",
            "baselineCommit",
            "baselineTree",
            "requiredBoundaryIds",
            "surfaces",
            "repositories",
            "origins",
            "paths",
            "producers",
            "producerControls",
            "releaseLines",
            "routes",
            "cases",
            "caseOwners",
            "boundaries",
            "proof",
        ],
        "matrix",
    )?;
    if root["schemaVersion"] != 1 {
        return Err("matrix schemaVersion must be 1".into());
    }
    text(root, "matrixId", "matrix")?;
    if root["contractDigest"] != "8197bf54e22c5aeffeb97c883e20177b0a4b89613038e0a2f52c35e856320eb1"
        || root["amendmentDigest"]
            != "6b8538f9f5fbf7b0f477d40462b8be76329f12170e050daa888899d88ad56d60"
        || root["baselineCommit"] != "773d11658ca6cc632001d3308c7171fef7cbc752"
        || root["baselineTree"] != "cac6e68955be1b0bed9f08463f35585de65c635e"
    {
        return Err("matrix contract or baseline identity drifted".into());
    }
    for key in ["contractDigest", "amendmentDigest"] {
        sha256(root, key, "matrix")?;
    }
    for key in ["baselineCommit", "baselineTree"] {
        sha1(root, key, "matrix")?;
    }

    let required_boundaries = string_array(root, "requiredBoundaryIds", "matrix")?;
    let required_boundaries = required_boundaries.into_iter().collect::<BTreeSet<_>>();
    if required_boundaries.len() != 6 {
        return Err("matrix must name six required boundaries".into());
    }

    let surfaces = array(root, "surfaces", "matrix")?;
    let surface_ids = id_set(surfaces, "surfaces")?;
    if surface_ids != BTreeSet::from(["current-exitbind".into(), "legacy-soulmate".into()]) {
        return Err("matrix surface IDs do not match the accepted identities".into());
    }
    for (index, row) in surfaces.iter().enumerate() {
        let at = format!("surfaces[{index}]");
        let object = exact_object(
            row,
            &[
                "id",
                "callerBasename",
                "product",
                "producerId",
                "pathId",
                "installedCommands",
                "status",
            ],
            &at,
        )?;
        let id = text(object, "id", &at)?;
        let caller = text(object, "callerBasename", &at)?;
        let product = text(object, "product", &at)?;
        let producer = text(object, "producerId", &at)?;
        let path = text(object, "pathId", &at)?;
        let commands = string_array(object, "installedCommands", &at)?;
        let status = text(object, "status", &at)?;
        let expected = match id {
            "current-exitbind" => (
                "exitbind",
                "exitbind",
                "exitbind-v6",
                "current-defaults",
                vec!["exitbind"],
            ),
            "legacy-soulmate" => (
                "soulmate",
                "soulmate",
                "soulmate-v1-write",
                "legacy-defaults",
                vec!["soulmate"],
            ),
            _ => return Err(format!("unknown surface {id}")),
        };
        let expected_status = if id == "current-exitbind" {
            "current"
        } else {
            "historical-only"
        };
        if status != expected_status {
            return Err(format!("invalid surface support status at {at}"));
        }
        if (caller, product, producer, path, commands)
            != (
                expected.0,
                expected.1,
                expected.2,
                expected.3,
                expected.4.into_iter().map(str::to_owned).collect(),
            )
        {
            return Err(format!(
                "invalid caller/product/producer/path combination at {at}"
            ));
        }
    }

    let repositories = array(root, "repositories", "matrix")?;
    let repository_ids = id_set(repositories, "repositories")?;
    if repository_ids
        != BTreeSet::from([
            "canonical-current".into(),
            "canonical-legacy".into(),
            "custom-example".into(),
        ])
    {
        return Err("matrix repository IDs do not match the accepted routes".into());
    }
    for (index, row) in repositories.iter().enumerate() {
        let at = format!("repositories[{index}]");
        let object = exact_object(row, &["id", "kind", "value"], &at)?;
        let id = text(object, "id", &at)?;
        let kind = text(object, "kind", &at)?;
        let value = text(object, "value", &at)?;
        let expected = match id {
            "canonical-current" => ("canonical-current", "PANIC-ANGEL-SCIENCE/exitbind"),
            "canonical-legacy" => ("canonical-legacy", "envy404/soulmate"),
            "custom-example" => ("custom", "example/project"),
            _ => return Err(format!("unknown repository {id}")),
        };
        if (kind, value) != expected {
            return Err(format!("invalid repository route at {at}"));
        }
    }

    let origins = array(root, "origins", "matrix")?;
    let origin_ids = id_set(origins, "origins")?;
    if origin_ids != BTreeSet::from(["current-updater".into(), "legacy-updater".into()]) {
        return Err("matrix origin IDs do not match the accepted updater surfaces".into());
    }
    for (index, row) in origins.iter().enumerate() {
        let at = format!("origins[{index}]");
        let object = exact_object(
            row,
            &[
                "id",
                "callerSurfaceId",
                "installPrefixEnv",
                "api",
                "rawInstaller",
                "status",
            ],
            &at,
        )?;
        let id = text(object, "id", &at)?;
        let caller = text(object, "callerSurfaceId", &at)?;
        let install_prefix = text(object, "installPrefixEnv", &at)?;
        let api = text(object, "api", &at)?;
        let raw = text(object, "rawInstaller", &at)?;
        let status = text(object, "status", &at)?;
        let expected = match id {
            "current-updater" => (
                "current-exitbind",
                "https://api.github.com/repos/PANIC-ANGEL-SCIENCE/exitbind/releases?per_page=20",
                "https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/",
            ),
            "legacy-updater" => (
                "legacy-soulmate",
                "https://api.github.com/repos/envy404/soulmate/releases?per_page=20",
                "https://raw.githubusercontent.com/envy404/soulmate/",
            ),
            _ => return Err(format!("unknown updater origin {id}")),
        };
        let expected_install_prefix = if id == "current-updater" {
            "EXITBIND_INSTALL_PREFIX"
        } else {
            "SOULMATE_INSTALL_PREFIX"
        };
        let expected_status = if id == "current-updater" {
            "current"
        } else {
            "historical-only"
        };
        if (caller, install_prefix, api, raw, status)
            != (
                expected.0,
                expected_install_prefix,
                expected.1,
                expected.2,
                expected_status,
            )
        {
            return Err(format!("invalid updater origin at {at}"));
        }
    }

    let paths = array(root, "paths", "matrix")?;
    let path_ids = id_set(paths, "paths")?;
    if path_ids != BTreeSet::from(["current-defaults".into(), "legacy-defaults".into()]) {
        return Err("matrix path IDs do not match the accepted paths".into());
    }
    for (index, row) in paths.iter().enumerate() {
        let at = format!("paths[{index}]");
        let object = exact_object(
            row,
            &[
                "id",
                "defaultConfig",
                "defaultControl",
                "defaultState",
                "legacyConfig",
                "legacyControl",
                "legacyState",
                "status",
            ],
            &at,
        )?;
        let id = text(object, "id", &at)?;
        let actual = (
            text(object, "defaultConfig", &at)?,
            text(object, "defaultControl", &at)?,
            text(object, "defaultState", &at)?,
            text(object, "legacyConfig", &at)?,
            text(object, "legacyControl", &at)?,
            text(object, "legacyState", &at)?,
            text(object, "status", &at)?,
        );
        let expected = match id {
            "current-defaults" => (
                "exitbind.json",
                "exitbind",
                ".exitbind",
                "soulmate.json",
                "soulmate",
                ".soulmate",
                "current",
            ),
            "legacy-defaults" => (
                "soulmate.json",
                "soulmate",
                ".soulmate",
                "soulmate.json",
                "soulmate",
                ".soulmate",
                "historical-only",
            ),
            _ => return Err(format!("unknown path {id}")),
        };
        if actual != expected {
            return Err(format!("invalid default or preserved legacy paths at {at}"));
        }
    }

    let producers = array(root, "producers", "matrix")?;
    let producer_ids = id_set(producers, "producers")?;
    if producer_ids
        != BTreeSet::from([
            "exitbind-v6".into(),
            "soulmate-v1-write".into(),
            "historical-soulmate-v1-v4".into(),
        ])
    {
        return Err("matrix producer IDs do not match the accepted formats".into());
    }
    for (index, row) in producers.iter().enumerate() {
        let at = format!("producers[{index}]");
        let object = exact_object(
            row,
            &["id", "name", "eventFormat", "historical", "writable"],
            &at,
        )?;
        let id = text(object, "id", &at)?;
        let actual = (
            text(object, "name", &at)?,
            text(object, "eventFormat", &at)?,
            object["historical"].as_bool(),
            object["writable"].as_bool(),
        );
        let expected = match id {
            "exitbind-v6" => ("exitbind", "v6", Some(false), Some(true)),
            "soulmate-v1-write" => ("soulmate", "v1", Some(true), Some(false)),
            "historical-soulmate-v1-v4" => ("soulmate", "v1-v4", Some(true), Some(false)),
            _ => return Err(format!("unknown producer {id}")),
        };
        if actual != expected {
            return Err(format!("invalid producer identity at {at}"));
        }
    }

    let producer_controls = array(root, "producerControls", "matrix")?;
    let producer_control_ids = id_set(producer_controls, "producerControls")?;
    if producer_control_ids
        != BTreeSet::from([
            "producer-current-v6-write".into(),
            "producer-historical-v1-v4-read".into(),
        ])
    {
        return Err(
            "matrix producer control IDs do not match the accepted evidence controls".into(),
        );
    }
    for (index, row) in producer_controls.iter().enumerate() {
        let at = format!("producerControls[{index}]");
        let object = exact_object(
            row,
            &[
                "id",
                "producerId",
                "callerSurfaceId",
                "operation",
                "versions",
            ],
            &at,
        )?;
        let id = text(object, "id", &at)?;
        let producer = text(object, "producerId", &at)?;
        let caller = text(object, "callerSurfaceId", &at)?;
        let operation = text(object, "operation", &at)?;
        let versions = array(object, "versions", &at)?;
        if !producer_ids.contains(producer)
            || !surface_ids.contains(caller)
            || !matches!(operation, "read" | "write")
            || versions.is_empty()
            || versions.iter().any(|version| version.as_u64().is_none())
        {
            return Err(format!("invalid producer control at {at}"));
        }
        let expected = match id {
            "producer-current-v6-write" => ("exitbind-v6", "current-exitbind", "write", vec![6]),
            "producer-historical-v1-v4-read" => (
                "historical-soulmate-v1-v4",
                "legacy-soulmate",
                "read",
                vec![1, 2, 3, 4],
            ),
            _ => return Err(format!("unknown producer control {id}")),
        };
        let actual_versions = versions
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect::<Vec<_>>();
        if (producer, caller, operation, actual_versions)
            != (expected.0, expected.1, expected.2, expected.3)
        {
            return Err(format!("invalid producer control pairing at {at}"));
        }
    }

    let release_lines = array(root, "releaseLines", "matrix")?;
    let release_ids = id_set(release_lines, "releaseLines")?;
    if release_ids
        != BTreeSet::from([
            "historical-pre-017".into(),
            "current-bridge-017-plus".into(),
            "malformed-current-fallback".into(),
            "custom-legacy".into(),
        ])
    {
        return Err("matrix release-line IDs do not match the accepted decisions".into());
    }
    for (index, row) in release_lines.iter().enumerate() {
        let at = format!("releaseLines[{index}]");
        let id = text(
            row.as_object()
                .ok_or_else(|| format!("{at} must be an object"))?,
            "id",
            &at,
        )?;
        let object = if id == "custom-legacy" {
            let object = exact_object(
                row,
                &["id", "status", "selectionRule", "versions", "assetPrefix"],
                &at,
            )?;
            if text(object, "status", &at)? != "historical-only" {
                return Err(format!("{at}.status must be historical-only"));
            }
            object
        } else {
            exact_object(
                row,
                &["id", "selectionRule", "versions", "assetPrefix"],
                &at,
            )?
        };
        text(object, "selectionRule", &at)?;
        if string_array(object, "versions", &at)?.is_empty() {
            return Err(format!("{at}.versions cannot be empty"));
        }
        let asset = text(object, "assetPrefix", &at)?;
        if asset != "exitbind" && asset != "soulmate" {
            return Err(format!("invalid asset prefix at {at}"));
        }
    }

    let routes = array(root, "routes", "matrix")?;
    let route_ids = id_set(routes, "routes")?;
    if route_ids
        != BTreeSet::from([
            "current-canonical".into(),
            "canonical-legacy-historical".into(),
            "canonical-legacy-current".into(),
            "canonical-legacy-malformed".into(),
            "custom-legacy".into(),
            "custom-current".into(),
            "partial-exitbind-custom-legacy".into(),
        ])
    {
        return Err("matrix route IDs do not match the accepted routes".into());
    }
    let release_map = release_lines
        .iter()
        .map(|row| {
            let object = row.as_object().expect("release row object");
            (object["id"].as_str().unwrap(), object)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for (index, row) in routes.iter().enumerate() {
        let at = format!("routes[{index}]");
        let object = exact_object(
            row,
            &[
                "id",
                "callerSurfaceId",
                "product",
                "producerId",
                "originId",
                "repositoryId",
                "fetchRepositoryId",
                "releaseLineId",
                "assetPrefix",
                "installedCommands",
                "resultSurfaceId",
                "status",
            ],
            &at,
        )?;
        let caller = text(object, "callerSurfaceId", &at)?;
        let product = text(object, "product", &at)?;
        let producer = text(object, "producerId", &at)?;
        let origin = text(object, "originId", &at)?;
        let repository = text(object, "repositoryId", &at)?;
        let fetch_repository = text(object, "fetchRepositoryId", &at)?;
        let release = text(object, "releaseLineId", &at)?;
        let asset = text(object, "assetPrefix", &at)?;
        let result = text(object, "resultSurfaceId", &at)?;
        let status = text(object, "status", &at)?;
        if !surface_ids.contains(caller)
            || !producer_ids.contains(producer)
            || !origin_ids.contains(origin)
            || !repository_ids.contains(repository)
            || !repository_ids.contains(fetch_repository)
            || !release_ids.contains(release)
            || !surface_ids.contains(result)
        {
            return Err(format!("route references an unknown identity at {at}"));
        }
        let commands = string_array(object, "installedCommands", &at)?;
        let valid = match (caller, product, producer, result) {
            ("current-exitbind", "exitbind", "exitbind-v6", "current-exitbind") => {
                commands == vec!["exitbind"]
            }
            ("legacy-soulmate", "soulmate", "soulmate-v1-write", "legacy-soulmate") => {
                commands == vec!["soulmate"]
            }
            ("legacy-soulmate", "exitbind", "exitbind-v6", "current-exitbind") => {
                commands == vec!["soulmate", "exitbind"]
            }
            _ => false,
        };
        if !valid {
            return Err(format!(
                "invalid caller/product/producer combination at {at}"
            ));
        }
        let expected_status = match object["id"].as_str() {
            Some("current-canonical" | "custom-current" | "partial-exitbind-custom-legacy") => {
                "current"
            }
            _ => "historical-only",
        };
        if status != expected_status {
            return Err(format!("invalid route support status at {at}"));
        }
        let expected_origin = if caller == "legacy-soulmate" {
            "legacy-updater"
        } else {
            "current-updater"
        };
        if origin != expected_origin {
            return Err(format!(
                "route origin does not preserve caller routing at {at}"
            ));
        }
        let expected_fetch_repository = if repository == "canonical-legacy" {
            "canonical-current"
        } else {
            repository
        };
        if fetch_repository != expected_fetch_repository {
            return Err(format!(
                "route fetch repository does not preserve bridge routing at {at}"
            ));
        }
        let expected_asset = release_map
            .get(release)
            .and_then(|release| release["assetPrefix"].as_str())
            .ok_or_else(|| format!("unknown release-line asset at {at}"))?;
        if asset != expected_asset {
            return Err(format!(
                "route asset does not match its release line at {at}"
            ));
        }
    }

    let cases = array(root, "cases", "matrix")?;
    let case_ids = id_set(cases, "cases")?;
    let required_cases = BTreeSet::from([
        "installer-only-exitbind-v017".into(),
        "installer-conflicting-namespaces".into(),
        "installer-partial-exitbind-custom-legacy".into(),
        "installer-legacy-v012".into(),
        "installer-legacy-v016".into(),
        "installer-legacy-v016-build".into(),
        "installer-legacy-v01600".into(),
        "installer-legacy-v016-short".into(),
        "installer-legacy-v017-rc1".into(),
        "installer-legacy-v017".into(),
        "installer-legacy-v017-build".into(),
        "installer-legacy-v018".into(),
        "installer-custom-legacy-pre017".into(),
        "installer-custom-legacy-v017".into(),
        "installer-custom-current".into(),
        "updater-v01600-reject".into(),
        "updater-build-reject".into(),
        "updater-stable-ignore-prerelease".into(),
        "updater-prerelease-order".into(),
        "runtime-current-defaults".into(),
        "runtime-legacy-defaults".into(),
        "runtime-explicit-legacy-paths".into(),
        "producer-historical-v1-v4".into(),
        "producer-current-v5".into(),
        "proof-claims".into(),
        "proof-ci".into(),
        "proof-release".into(),
        "proof-ci-drift-warning".into(),
        "proof-release-drift-warning".into(),
        "proof-wsl".into(),
        "release-assets-current".into(),
    ]);
    if case_ids != required_cases {
        return Err("matrix is missing a required boundary case or has an unexpected case".into());
    }
    for (index, row) in cases.iter().enumerate() {
        let at = format!("cases[{index}]");
        let object = exact_object(
            row,
            &[
                "id",
                "kind",
                "routeId",
                "version",
                "expectedSurfaceId",
                "boundaryId",
            ],
            &at,
        )?;
        text(object, "id", &at)?;
        let kind = text(object, "kind", &at)?;
        if !matches!(
            kind,
            "installer" | "updater" | "runtime" | "producer" | "proof" | "release"
        ) {
            return Err(format!("invalid case kind at {at}"));
        }
        if !route_ids.contains(text(object, "routeId", &at)?)
            || !surface_ids.contains(text(object, "expectedSurfaceId", &at)?)
        {
            return Err(format!("case has unknown route or surface at {at}"));
        }
        let version = text(object, "version", &at)?;
        if kind == "installer" {
            let route_id = text(object, "routeId", &at)?;
            let route = routes
                .iter()
                .find(|route| route["id"].as_str() == Some(route_id))
                .ok_or_else(|| format!("case has unknown route at {at}"))?;
            let release_id = route["releaseLineId"]
                .as_str()
                .ok_or_else(|| format!("route has no release line at {at}"))?;
            let release = release_map
                .get(release_id)
                .ok_or_else(|| format!("case has unknown release line at {at}"))?;
            let versions = release["versions"]
                .as_array()
                .ok_or_else(|| format!("release line has no versions at {at}"))?;
            if !versions
                .iter()
                .any(|candidate| candidate.as_str() == Some(version))
            {
                return Err(format!(
                    "installer case version is outside its release line at {at}"
                ));
            }
        }
        if !required_boundaries.contains(text(object, "boundaryId", &at)?) {
            return Err(format!("case has unknown boundary at {at}"));
        }
    }

    let case_owners = array(root, "caseOwners", "matrix")?;
    let case_owner_ids = id_set(case_owners, "caseOwners")?;
    if case_owner_ids != case_ids {
        return Err("every case must have exactly one owning field/test reference".into());
    }
    for (index, row) in case_owners.iter().enumerate() {
        let at = format!("caseOwners[{index}]");
        let object = exact_object(row, &["id", "field", "test"], &at)?;
        let id = text(object, "id", &at)?;
        let field = text(object, "field", &at)?;
        let test = text(object, "test", &at)?;
        let case = cases
            .iter()
            .find(|case| case["id"].as_str() == Some(id))
            .ok_or_else(|| format!("case owner references unknown case at {at}"))?;
        let expected_field = match case["kind"].as_str() {
            Some("installer") => "routes",
            Some("updater") => "origins",
            Some("runtime") => "paths",
            Some("producer") => "producerControls",
            Some("proof") => "proof.invocations",
            Some("release") => "releaseLines",
            _ => return Err(format!("case owner has unknown case kind at {at}")),
        };
        if field != expected_field || !test.contains(".rs") {
            return Err(format!(
                "case owner does not identify its boundary consumer at {at}"
            ));
        }
    }

    let boundaries = array(root, "boundaries", "matrix")?;
    let boundary_ids = id_set(boundaries, "boundaries")?;
    if boundary_ids != required_boundaries {
        return Err("matrix is missing a required boundary or has an unexpected boundary".into());
    }
    let mut boundary_cases = BTreeSet::new();
    for (index, row) in boundaries.iter().enumerate() {
        let at = format!("boundaries[{index}]");
        let object = exact_object(row, &["id", "kind", "caseIds"], &at)?;
        let id = text(object, "id", &at)?;
        let kind = text(object, "kind", &at)?;
        if !matches!(
            kind,
            "installer" | "updater" | "runtime" | "producer" | "proof" | "release"
        ) {
            return Err(format!("invalid boundary kind at {at}"));
        }
        for case_id in string_array(object, "caseIds", &at)? {
            if !case_ids.contains(&case_id) || !boundary_cases.insert(case_id.clone()) {
                return Err(format!("boundary case is missing or duplicated: {case_id}"));
            }
            let case = cases
                .iter()
                .find(|case| case["id"].as_str() == Some(case_id.as_str()))
                .ok_or_else(|| format!("unknown case {case_id}"))?;
            if case["boundaryId"].as_str() != Some(id) {
                return Err(format!("case {case_id} points at the wrong boundary"));
            }
        }
    }
    if boundary_cases != case_ids {
        return Err("every case must be owned by exactly one boundary".into());
    }

    let proof = exact_object(
        &root["proof"],
        &["binary", "releaseAssetPrefix", "invocations"],
        "proof",
    )?;
    if text(proof, "binary", "proof")? != "target/debug/exitbind"
        || text(proof, "releaseAssetPrefix", "proof")? != "exitbind"
    {
        return Err("proof matrix must bind the exact Exitbind binary and asset prefix".into());
    }
    let invocations = array(proof, "invocations", "proof")?;
    let invocation_ids = id_set(invocations, "proof.invocations")?;
    if invocation_ids
        != BTreeSet::from([
            "claims".into(),
            "ci".into(),
            "ci-drift-warning".into(),
            "release".into(),
            "release-drift-warning".into(),
            "wsl".into(),
        ])
    {
        return Err("proof matrix must bind claims, CI, release, and WSL invocations".into());
    }
    for (index, row) in invocations.iter().enumerate() {
        let at = format!("proof.invocations[{index}]");
        let object = exact_object(row, &["id", "path", "command", "occurrences"], &at)?;
        text(object, "id", &at)?;
        text(object, "path", &at)?;
        text(object, "command", &at)?;
        if object["occurrences"]
            .as_u64()
            .map_or(true, |count| count == 0)
        {
            return Err(format!("{at}.occurrences must be positive"));
        }
    }
    Ok(())
}

pub fn matrix() -> Value {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(MATRIX_PATH)).unwrap(),
    )
    .unwrap();
    parse_matrix(&value).unwrap();
    value
}

pub fn row(kind: &str, id: &str) -> Value {
    matrix()[kind]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("missing {kind} row {id}"))
        .clone()
}

#[allow(dead_code)]
pub fn case(id: &str) -> Value {
    row("cases", id)
}

#[allow(dead_code)]
pub fn route(id: &str) -> Value {
    row("routes", id)
}

#[path = "compatibility_matrix/matrix_validation.rs"]
mod matrix_validation;
#[path = "compatibility_matrix/projection_compatibility.rs"]
mod projection_compatibility;
